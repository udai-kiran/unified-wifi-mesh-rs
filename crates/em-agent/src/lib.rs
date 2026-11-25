// SPDX-License-Identifier: Apache-2.0

//! Synchronous agent role core for slices S1 and S2 (design notes 0012 D2, D9, D10 and
//! 0013 D4, D5).
//!
//! [`AgentCore`] owns the orchestrator, the node table and the timers. It is driven
//! by explicit inputs and `Instant`s and returns frames, platform requests and its next
//! deadline; it has no sockets and no runtime. The event loop run ([`run`]) moves inputs
//! in and frames out, and hands platform requests to the [`platform::Platform`].

use std::fmt;
use std::time::{Duration, Instant};

use em_cmd::{
    ChainId, Command, CommandType, CommandView, DevInit, NodeKey, NodeKind, OneWifiCb, OrchState,
    RadioSpec,
};
use em_core::configuration;
use em_core::wsc::BssConfig;
use em_core::{AgentState, EngineCtx, MsgIdCounter, NodeTable, Output, Outputs};
use em_crypto::rand::RandomSource;
use em_msg::{CmduHeader, validate};
use em_orch::{OrchRole, Orchestrator, RoleEffect};
use em_types::{MacAddr, MsgType, ProfileType};

use crate::platform::ConfirmedBss;

mod actor;
pub mod cli;
pub mod platform;
mod rules;
mod s2;
mod wsc;

pub use actor::{ActorError, AgentConfig, run};
pub use rules::AgentRules;

/// Idle time after which a node's proto tick fires (`em.cpp:644-680`).
const PROTO_IDLE: Duration = Duration::from_secs(1);
/// Idle time after which the orchestrator ticks (`EM_ORCH_TICK`, 250 ms).
const ORCH_IDLE: Duration = Duration::from_millis(250);
/// A radio waits for its M2 at most this long after its M1 left (0013 D5).
const M2_WAIT_SECS: u64 = 10;

/// A CMDU for the agent to send; the loop adds the local AL MAC and session (0012 D9).
#[derive(Debug, PartialEq, Eq)]
pub struct Frame {
    /// Destination AL MAC or the IEEE 1905 multicast address.
    pub remote: MacAddr,
    /// One CMDU, without an Ethernet header.
    pub cmdu: Vec<u8>,
}

/// A registration returned a different AL MAC than the first one. Fatal (0012 D9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlMacChanged {
    /// The AL MAC recorded at the first registration.
    pub recorded: MacAddr,
    /// The AL MAC of the new registration.
    pub new: MacAddr,
}

impl fmt::Display for AlMacChanged {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "AL MAC changed from {} to {} across registrations",
            self.recorded, self.new
        )
    }
}

impl std::error::Error for AlMacChanged {}

/// The BSSs of one radio, to be applied by the platform (0013 D4).
#[derive(Debug)]
pub struct ApplyRadioConfig {
    /// The radio the accepted M2 named.
    pub radio: MacAddr,
    /// One configuration per M2 TLV, in M2 order.
    pub bss: Vec<BssConfig>,
}

/// Which command types ran before an orchestrator pass, to name why a pass left a radio
/// unconfigured.
#[derive(Debug, Clone, Copy)]
struct RunningTypes {
    onewifi_cb: bool,
    cfg_renew: bool,
}

/// The proto tick deadline of one node.
#[derive(Debug, Clone, Copy)]
struct NodeDeadline {
    key: NodeKey,
    due: Instant,
}

/// The production random source, named once so the type ban still covers every field.
#[allow(
    clippy::disallowed_types,
    reason = "0016 U27: the production random source, named once"
)]
type ProductionRandom = em_crypto::rand::OsRandom;

/// The agent's protocol and orchestration state.
pub struct AgentCore<R: RandomSource = ProductionRandom> {
    orch: Orchestrator<AgentRules>,
    rules: AgentRules,
    nodes: NodeTable<AgentState>,
    msg_ids: MsgIdCounter,
    proto_deadlines: Vec<NodeDeadline>,
    orch_deadline: Option<Instant>,
    /// Recorded at the first registration and fixed for the life of the process.
    al_mac: Option<MacAddr>,
    registered: bool,
    radios: Vec<RadioSpec>,
    random: R,
    /// The clock for the D9 timing logs; the only clock the core reads.
    measure_now: fn() -> Instant,
    /// The BSS list of the last platform confirmation, standing in for the agent's model
    /// (0013 D4).
    confirmed: Option<Vec<ConfirmedBss>>,
    platform_requests: Vec<ApplyRadioConfig>,
}

impl AgentCore<ProductionRandom> {
    /// Creates a core that has not yet registered. `profile` is the port's
    /// `--profile` switch (0012 D4); `radios` come from the platform. This is the
    /// production constructor: OS randomness and the real clock.
    #[must_use]
    #[allow(
        clippy::disallowed_methods,
        clippy::disallowed_types,
        reason = "0016 U27: the production constructor builds OsRandom (OsRandom::new) and reads the real clock (Instant::now)"
    )]
    pub fn new(profile: u8, radios: Vec<RadioSpec>) -> Self {
        // `Instant::now` here is the production measurement clock.
        Self::with_sources(
            profile,
            radios,
            em_crypto::rand::OsRandom::new(),
            Instant::now,
        )
    }
}

impl<R: RandomSource> AgentCore<R> {
    /// Creates a core with the given random source and measurement clock, for replay
    /// tests (a seeded source and a fixed clock). [`AgentCore::new`] is the production
    /// constructor.
    #[must_use]
    pub fn with_sources(
        profile: u8,
        radios: Vec<RadioSpec>,
        random: R,
        measure_now: fn() -> Instant,
    ) -> Self {
        Self {
            orch: Orchestrator::new(),
            rules: AgentRules::new(profile),
            nodes: NodeTable::new(),
            msg_ids: MsgIdCounter::new(),
            proto_deadlines: Vec::new(),
            orch_deadline: None,
            al_mac: None,
            registered: false,
            radios,
            random,
            measure_now,
            confirmed: None,
            platform_requests: Vec::new(),
        }
    }

    /// The node table, for logging and tests.
    #[must_use]
    pub const fn nodes(&self) -> &NodeTable<AgentState> {
        &self.nodes
    }

    /// The AL MAC recorded at the first registration.
    #[must_use]
    pub const fn al_mac(&self) -> Option<MacAddr> {
        self.al_mac
    }

    /// The transport registered with `al_mac`.
    ///
    /// The first registration records the AL MAC and submits `dev_init`; later
    /// ones with the same AL MAC change nothing but the link flag (0012 D9).
    ///
    /// # Errors
    ///
    /// [`AlMacChanged`] when a later registration returns a different AL MAC.
    pub fn registered(&mut self, al_mac: MacAddr, now: Instant) -> Result<(), AlMacChanged> {
        match self.al_mac {
            Some(recorded) if recorded != al_mac => {
                return Err(AlMacChanged {
                    recorded,
                    new: al_mac,
                });
            }
            Some(_) => {}
            None => {
                self.al_mac = Some(al_mac);
                self.submit_dev_init(al_mac, now);
            }
        }
        self.registered = true;
        Ok(())
    }

    /// The transport lost its registration. No proto tick runs until the next one.
    pub fn unregistered(&mut self) {
        self.registered = false;
    }

    /// Takes the configurations the accepted M2s asked the platform to apply.
    pub fn take_platform_requests(&mut self) -> Vec<ApplyRadioConfig> {
        std::mem::take(&mut self.platform_requests)
    }

    /// A CMDU arrived from `remote`. The agent handles a Response, an M2, a Topology
    /// Query and a Renew (`em_agent.cpp:2366-2447`).
    pub fn received(&mut self, remote: MacAddr, cmdu: &[u8], now: Instant) -> Vec<Frame> {
        let mut frames = Vec::new();
        let msg_type = match CmduHeader::parse(cmdu) {
            Ok((header, _)) => header.msg_type,
            Err(error) => {
                tracing::warn!(%error, %remote, "dropping CMDU with an unreadable header");
                return frames;
            }
        };
        match msg_type {
            MsgType::AP_AUTOCONFIGURATION_RESPONSE => {
                match configuration::route_response(&self.nodes, cmdu) {
                    Ok(Some(key)) => self.dispatch_response(key, remote, cmdu, now, &mut frames),
                    Ok(None) => tracing::debug!(%remote, "no radio accepts this Response"),
                    Err(error) => tracing::warn!(%error, %remote, "Response could not be routed"),
                }
            }
            MsgType::AP_AUTOCONFIGURATION_WSC => self.handle_wsc(remote, cmdu, now, &mut frames),
            MsgType::TOPOLOGY_QUERY => self.handle_topology_query(remote, cmdu, now, &mut frames),
            MsgType::AP_AUTOCONFIGURATION_RENEW => self.handle_renew(cmdu, now),
            _ => tracing::debug!(?msg_type, %remote, "ignoring CMDU the agent does not handle"),
        }
        frames
    }

    /// Runs whatever is due at `now`: an orchestrator pass, then the proto
    /// tick of each due node. While unregistered no proto tick runs (0012 D9).
    pub fn tick(&mut self, now: Instant) -> Vec<Frame> {
        let mut frames = Vec::new();
        if self.orch_deadline.is_some_and(|due| due <= now) {
            self.run_pass(now, true);
        }
        if self.registered {
            let due_nodes: Vec<NodeKey> = self
                .proto_deadlines
                .iter()
                .filter(|deadline| deadline.due <= now)
                .map(|deadline| deadline.key)
                .collect();
            for key in due_nodes {
                self.dispatch_proto_tick(key, now, &mut frames);
            }
        }
        frames
    }

    /// The earliest instant at which [`Self::tick`] has work, if any.
    #[must_use]
    pub fn next_deadline(&self) -> Option<Instant> {
        let proto = if self.registered {
            self.proto_deadlines
                .iter()
                .map(|deadline| deadline.due)
                .min()
        } else {
            None
        };
        [self.orch_deadline, proto].into_iter().flatten().min()
    }

    // intentional-difference:onewifi-cb-per-device
    // Upstream creates one `onewifi_cb` per radio, named by the first BSS of OneWifi's
    // callback, and drops callbacks while one runs (`em_agent.cpp:276-277`,
    // `em_orch_agent.cpp:397-405`). The port submits one for the device (0013 D4).
    //
    /// The platform applied every radio's configuration (0013 D4).
    ///
    /// Stores the BSS list and submits one `onewifi_cb` for every radio. Upstream drops a
    /// callback while one is in progress (`em_agent.cpp:276-277`); so does the port, after
    /// storing the list. While a radio still waits for its own configuration (a restart
    /// is under way) the port submits nothing: the platform confirms again once that
    /// radio is applied, and one `onewifi_cb` then covers the whole device.
    pub fn platform_confirmed(&mut self, bss: Vec<ConfirmedBss>, now: Instant) {
        tracing::info!(bss = bss.len(), "platform confirmed");
        self.confirmed = Some(bss);
        let waiting = self.nodes.iter().any(|node| {
            node.key.kind == NodeKind::Radio
                && (node.state < AgentState::OwconfigPending
                    || node.state == AgentState::AutoconfigRenewPending)
        });
        if waiting {
            tracing::info!("a radio still waits for its configuration, onewifi_cb deferred");
            return;
        }
        if self.orch.is_type_in_progress(CommandType::OneWifiCb) {
            tracing::info!("onewifi_cb is in progress, the confirmation starts no new one");
            return;
        }
        let radios = self.radio_keys();
        if self.submit(Command::OneWifiCb(OneWifiCb { radios }), now) {
            // The Docker checks count this line.
            tracing::info!("onewifi_cb submitted");
        } else {
            tracing::warn!("onewifi_cb has no radio waiting in owconfig_pending");
        }
    }

    fn radio_keys(&self) -> Vec<NodeKey> {
        self.nodes
            .iter()
            .filter(|node| node.key.kind == NodeKind::Radio)
            .map(|node| node.key)
            .collect()
    }

    fn submit_dev_init(&mut self, al_mac: MacAddr, now: Instant) {
        let command = Command::DevInit(DevInit {
            al_mac,
            radios: Vec::from(self.radios.as_slice()),
        });
        self.submit(command, now);
    }

    /// Pre-processes, admits and starts a command. Returns whether it was admitted.
    fn submit(&mut self, command: Command, now: Instant) -> bool {
        let kind = command.kind();
        let (chain, effects) = self
            .orch
            .submit(&mut self.rules, &mut self.nodes, command, now);
        if kind == CommandType::DevInit {
            // The Docker check 6a counts this line (0012 D8).
            tracing::info!(chain = chain.0, "dev_init submitted");
        }
        let mut out = Vec::new();
        for effect in effects {
            self.rules.apply(&mut self.nodes, effect, &mut out);
        }
        self.drain_orch_out(out, now);
        self.admit(chain, kind, now)
    }

    fn admit(&mut self, chain: ChainId, kind: CommandType, now: Instant) -> bool {
        if self.orch.admit(&mut self.rules, &mut self.nodes, chain) {
            self.run_event_pass(now);
            true
        } else {
            tracing::warn!(?kind, "command has no candidates: not ready");
            false
        }
    }

    /// Activation and readiness after an event; restarts the idle orchestrator deadline.
    fn run_event_pass(&mut self, now: Instant) {
        self.run_pass(now, false);
    }

    fn running_types(&self) -> RunningTypes {
        RunningTypes {
            onewifi_cb: self.orch.is_type_in_progress(CommandType::OneWifiCb),
            cfg_renew: self.orch.is_type_in_progress(CommandType::CfgRenew),
        }
    }

    fn run_pass(&mut self, now: Instant, timer: bool) {
        let before = self.running_types();
        let unconfigured_before = self.unconfigured_radios();
        let mut out = Vec::new();
        if timer {
            self.orch
                .tick(&mut self.rules, &mut self.nodes, now, &mut out);
        } else {
            self.orch
                .on_event(&mut self.rules, &mut self.nodes, now, &mut out);
        }
        self.drain_orch_out(out, now);
        self.orch_deadline = Some(now + ORCH_IDLE);
        let cancelled_in_pass = self
            .unconfigured_radios()
            .iter()
            .any(|key| !unconfigured_before.contains(key));
        self.reopen_search(before, cancelled_in_pass, now);
    }

    fn unconfigured_radios(&self) -> Vec<NodeKey> {
        self.nodes
            .iter()
            .filter(|node| {
                node.key.kind == NodeKind::Radio && node.state == AgentState::Unconfigured
            })
            .map(|node| node.key)
            .collect()
    }

    // intentional-difference:renew-timeout-restart
    // intentional-difference:onewifi-cb-timeout-restart
    // Upstream leaves a radio `unconfigured` after the cancel with no command to search
    // again (`em_orch_agent.cpp:64-71`).
    //
    /// Submits a new `dev_init` when a radio is unconfigured and none is active or pending,
    /// so the Search gate reopens (0013 D4: `onewifi-cb-timeout-restart` and
    /// `renew-timeout-restart`). First cancels every `cfg_renew` of the radios, queued or
    /// running, because the new `dev_init` onboards them all again.
    ///
    /// `cancelled_in_pass` is whether the orchestrator pass just made a radio
    /// `unconfigured`: only a timeout cancel does that, and a cancelled command still
    /// counts as in progress until a later pass destroys it.
    fn reopen_search(&mut self, before: RunningTypes, cancelled_in_pass: bool, now: Instant) {
        let Some(al_mac) = self.al_mac else {
            return;
        };
        if self.orch.is_type_in_progress(CommandType::DevInit) {
            return;
        }
        let any_unconfigured = self
            .nodes
            .iter()
            .any(|node| node.key.kind == NodeKind::Radio && node.state == AgentState::Unconfigured);
        if !any_unconfigured {
            return;
        }
        let timeout = if !cancelled_in_pass {
            None
        } else if before.onewifi_cb {
            Some("onewifi_cb timeout")
        } else if before.cfg_renew {
            Some("cfg_renew timeout")
        } else {
            None
        };
        if let Some(reason) = timeout {
            tracing::info!(reason, "restart discovery");
        } else {
            // The radio's own timer logged its restart already.
            tracing::info!("a radio is unconfigured and no dev_init is active");
        }
        // intentional-difference:onewifi-cb-timeout-restart
        // The new dev_init onboards every radio again, so a cfg_renew still queued or
        // running for one of them is stale. Upstream has no recovery to clash with it.
        let radios = self.radio_keys();
        let mut out = Vec::new();
        self.orch.cancel_command(
            &mut self.rules,
            &mut self.nodes,
            CommandType::CfgRenew,
            &radios,
            &mut out,
        );
        self.drain_orch_out(out, now);
        self.submit_dev_init(al_mac, now);
    }

    fn drain_orch_out(&mut self, requests: Vec<RoleEffect<AgentState>>, now: Instant) {
        for request in requests {
            match request {
                RoleEffect::RestartProto(key) => self.restart_proto_deadline(key, now),
                RoleEffect::Renew(key) => {
                    tracing::warn!(radio = %key.mac, "the agent never asks for a renew");
                }
                RoleEffect::CreateNode { .. } | RoleEffect::SetState(_, _) => {
                    unreachable!("node effects are applied by the role")
                }
            }
        }
    }

    fn restart_proto_deadline(&mut self, key: NodeKey, now: Instant) {
        let due = now + PROTO_IDLE;
        match self
            .proto_deadlines
            .iter_mut()
            .find(|deadline| deadline.key == key)
        {
            Some(deadline) => deadline.due = due,
            None => self.proto_deadlines.push(NodeDeadline { key, due }),
        }
    }

    fn dispatch_proto_tick(&mut self, key: NodeKey, now: Instant, frames: &mut Vec<Frame>) {
        let Some(state) = self.nodes.get(key).map(|node| node.state) else {
            return;
        };
        let mut out = Outputs::new();
        match state {
            AgentState::Unconfigured | AgentState::AutoconfigRspPending => {
                let mut ctx = EngineCtx {
                    nodes: &mut self.nodes,
                    key,
                    commands: &self.orch,
                    msg_ids: &mut self.msg_ids,
                    now,
                    out: &mut out,
                };
                if let Err(error) = configuration::agent_proto_tick(&mut ctx) {
                    tracing::warn!(%error, radio = %key.mac, "proto tick failed");
                }
            }
            AgentState::AutoconfigRenewPending => self.renew_proto_tick(key, now, &mut out),
            AgentState::WscM2Pending => self.m2_wait_tick(key, now, &mut out),
            _ => {}
        }
        self.finish_event(key, now, out, frames);
    }

    /// Whether a command of one of `kinds` is executing on the node, the gate of
    /// `em.cpp:450-468` and `em.cpp:538-546`.
    fn gate_open(&self, key: NodeKey, kinds: &[CommandType]) -> bool {
        self.orch.orch_state(key) == OrchState::Progress
            && self
                .orch
                .active(key)
                .is_some_and(|active| kinds.contains(&active.kind))
    }

    /// Sends M1 from a renewed radio and enters `wsc_m2_pending`
    /// (`em_configuration.cpp:6739-6768`), under the gate of `cfg_renew`.
    fn renew_proto_tick(&mut self, key: NodeKey, now: Instant, out: &mut Outputs) {
        if !self.gate_open(key, &[CommandType::CfgRenew]) {
            return;
        }
        let Some(controller) = self.nodes.get(key).and_then(|node| node.controller) else {
            tracing::warn!(radio = %key.mac, "renewed radio has no controller to send M1 to");
            return;
        };
        if self.send_m1(key, controller.al_mac, now, out) {
            self.set_state(key, AgentState::WscM2Pending, out);
        }
    }

    // intentional-difference:restart-discovery-on-m2-timeout
    //
    /// Upstream has no M2 timer (`em_configuration.cpp:6781-6784`); 1905.1 §10.1.2 says an
    /// enrollee restarts the registrar discovery phase when configuration does not
    /// complete. After more than 10 s from the M1 hand-off the radio clears its session and
    /// returns to `unconfigured` (0013 D5).
    fn m2_wait_tick(&mut self, key: NodeKey, now: Instant, out: &mut Outputs) {
        if !self.gate_open(key, &[CommandType::DevInit, CommandType::CfgRenew]) {
            return;
        }
        let Some(handed_off) = self.rules.wsc.m1_handed_off(key) else {
            return;
        };
        if now.saturating_duration_since(handed_off).as_secs() <= M2_WAIT_SECS {
            return;
        }
        tracing::info!(
            radio = %key.mac,
            reason = "no M2 within 10 s of M1",
            "restart discovery"
        );
        self.rules.wsc.clear(key);
        self.set_state(key, AgentState::Unconfigured, out);
    }

    /// Builds M1 for the radio, hands it to the transport and records the hand-off time
    /// (0013 D4). Returns false, with no frame, when M1 cannot be built or fails
    /// validation; a failed validation also clears the radio's session.
    fn send_m1(
        &mut self,
        key: NodeKey,
        controller: MacAddr,
        now: Instant,
        out: &mut Outputs,
    ) -> bool {
        let Some(al_mac) = self.al_mac else {
            return false;
        };
        let Some(band) = self.nodes.get(key).and_then(|node| node.band) else {
            tracing::warn!(radio = %key.mac, "radio has no band, M1 not built");
            return false;
        };
        // intentional-difference:no-early-ap-report-s2
        // Upstream sends an Early AP Capability Report before M1 when the controller asks
        // (`em_configuration.cpp:6351-6365`); it is deferred to S3 (0013 D8).
        let params = wsc::m1_params(al_mac, controller, key.mac, band);
        let session = self.rules.wsc.session_mut(key);
        // The build runs on the loop task: its time is the loop's stall for the DH key
        // generation (0013 D9).
        let build_started = (self.measure_now)();
        let cmdu =
            match em_core::wsc::build_m1(&params, &mut self.msg_ids, &mut self.random, session) {
                Ok(cmdu) => cmdu,
                Err(error) => {
                    tracing::warn!(%error, radio = %key.mac, "M1 could not be built");
                    return false;
                }
            };
        if let Err(error) = validate(&cmdu, ProfileType::MULTI_AP_PROFILE_3) {
            tracing::warn!(%error, radio = %key.mac, "M1 failed validation, not sent");
            self.rules.wsc.clear(key);
            return false;
        }
        let build_us = (self.measure_now)()
            .saturating_duration_since(build_started)
            .as_micros();
        let mid = CmduHeader::parse(&cmdu).map_or(0, |(header, _)| header.msg_id);
        out.events.push(Output::Frame {
            remote: controller,
            cmdu,
        });
        self.rules.wsc.record_m1_handoff(key, now);
        tracing::info!(radio = %key.mac, mid, mac = %al_mac, build_us, "M1 sent");
        true
    }

    /// Changes a radio's state and asks for an orchestrator pass.
    fn set_state(&mut self, key: NodeKey, state: AgentState, out: &mut Outputs) {
        if let Some(node) = self.nodes.get_mut(key) {
            node.state = state;
            out.events.push(Output::Progress(key));
        }
    }

    fn dispatch_response(
        &mut self,
        key: NodeKey,
        remote: MacAddr,
        cmdu: &[u8],
        now: Instant,
        frames: &mut Vec<Frame>,
    ) {
        let state_before = self.nodes.get(key).map(|node| node.state);
        let mut out = Outputs::new();
        let mut ctx = EngineCtx {
            nodes: &mut self.nodes,
            key,
            commands: &self.orch,
            msg_ids: &mut self.msg_ids,
            now,
            out: &mut out,
        };
        if let Err(error) = configuration::handle_response(&mut ctx, remote, cmdu) {
            tracing::warn!(%error, radio = %key.mac, %remote, "Response rejected");
        }
        let found = self.nodes.get(key).and_then(|node| {
            let moved = state_before != Some(node.state);
            node.controller.filter(|_| moved)
        });
        if let Some(controller) = found {
            tracing::info!(
                radio = %key.mac,
                controller = %controller.al_mac,
                profile = controller.profile,
                "radio found controller"
            );
            if !self.send_m1(key, controller.al_mac, now, &mut out) {
                // Upstream leaves the state unchanged when M1 cannot be sent
                // (`em_configuration.cpp:6376-6391`): the radio keeps searching.
                self.revert_to_searching(key);
            }
        }
        self.finish_event(key, now, out, frames);
    }

    fn revert_to_searching(&mut self, key: NodeKey) {
        if let Some(node) = self.nodes.get_mut(key) {
            node.state = AgentState::AutoconfigRspPending;
            node.controller = None;
        }
    }

    /// Restarts the node's idle deadline, collects its frames and reevaluates
    /// orchestration when the node reported progress.
    fn finish_event(&mut self, key: NodeKey, now: Instant, out: Outputs, frames: &mut Vec<Frame>) {
        self.restart_proto_deadline(key, now);
        let mut progressed = false;
        for output in out.events {
            match output {
                Output::Frame { remote, cmdu } => {
                    self.log_search_sent(key, &cmdu);
                    frames.push(Frame { remote, cmdu });
                }
                Output::Progress(_) => progressed = true,
            }
        }
        if progressed {
            self.run_event_pass(now);
        }
    }

    fn log_search_sent(&self, key: NodeKey, cmdu: &[u8]) {
        let Ok((header, _)) = CmduHeader::parse(cmdu) else {
            return;
        };
        if header.msg_type != MsgType::AP_AUTOCONFIGURATION_SEARCH || key.kind != NodeKind::Radio {
            return;
        }
        let band = self.nodes.get(key).and_then(|node| node.band);
        tracing::info!(radio = %key.mac, ?band, mid = header.msg_id, "Search sent");
    }
}

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "0016 U27: unit tests read the real clock once, for the fixed start, and use the seeded random source"
)]
mod tests;
