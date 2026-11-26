// SPDX-License-Identifier: Apache-2.0

//! Synchronous controller role core for slices S1 and S2 (design notes 0012 D3, D9, D10
//! and 0013 D2, D3, D7).
//!
//! [`CtrlCore`] routes a Search, lets the AL node answer it, keeps the in-memory peer
//! model, answers each M1 with M2 and drives `em_config` until the agent's Topology
//! Response. It is driven by explicit inputs and has no sockets and no runtime; the event
//! loop run ([`run`]) moves inputs in and frames out.

use std::collections::VecDeque;
use std::fmt;
use std::time::{Duration, Instant};

use em_cmd::{CommandType, CommandView, NoCommands, NodeKey, NodeKind, OrchState};
use em_core::configuration::{self, SearchRoute};
use em_core::{CtrlState, EngineCtx, MsgIdCounter, Node, NodeTable, Output, Outputs};
use em_crypto::rand::RandomSource;
use em_dm::NetworkSsidInfo;
use em_msg::{CmduHeader, readers};
use em_orch::{Orchestrator, RoleEffect};
use em_types::{MacAddr, MsgType};
use em_utils::SortedVecMap;

use crate::networks::Networks;
use crate::rules::{CtrlNodes, CtrlRules};

mod actor;
pub mod cli;
mod networks;
mod rules;
mod s2;

pub use actor::{ActorError, CtrlConfig, run};

/// The most peer models the core keeps. Upstream keeps them without limit. S2 keeps
/// the map in memory and persists nothing (0012 D3, 0013 D2).
const MAX_PEER_MODELS: usize = 256;

/// The most radios one agent may have (`EM_MAX_RADIO_PER_AGENT`, `inc/em_base.h:90`).
const EM_MAX_RADIO_PER_AGENT: usize = 4;

/// The profile of the controller itself, given to its AL node (0012 D3).
const CONTROLLER_PROFILE: u8 = 3;

/// Idle time after which a node's proto tick fires (`em.cpp:644-680`).
const PROTO_IDLE: Duration = Duration::from_secs(1);
/// Idle time after which the orchestrator ticks (`EM_ORCH_TICK`, 250 ms).
const ORCH_IDLE: Duration = Duration::from_millis(250);

/// The proto tick deadline of one radio node.
#[derive(Debug, Clone, Copy)]
struct NodeDeadline {
    key: NodeKey,
    due: Instant,
}

/// A Renew retry episode of one radio: the Renews sent so far and when it started.
#[derive(Debug, Clone, Copy)]
struct RenewEpisode {
    sent: u8,
    started: Instant,
}

/// What the core does with each M2 it builds (0013 D3, D10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum M2Handoff {
    /// Hand the M2 to the transport.
    Deliver,
    /// The test switch: the first M2's hand-off is a no-op that reports success, so the
    /// controller behaves as after a write that the network then loses.
    DropNext,
    /// A hand-off that fails, to test that no output is applied.
    #[cfg(test)]
    Fail,
}

/// A CMDU for the controller to send; the loop adds the local AL MAC and session (0012 D9).
#[derive(Debug, PartialEq, Eq)]
pub struct Frame {
    /// Destination AL MAC.
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

/// What the controller remembers about an agent, from its first Search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerModel {
    /// The profile of the first Search as routed: 0 to 3 as sent, or 1 when the
    /// Profile TLV is absent, malformed, or 4 or more (`em_ctrl.cpp:1219-1229`;
    /// 0012 rev 7 erratum to D3).
    pub profile: u8,
    /// Whether the agent's `SupportedService` names the EM+ agent.
    pub is_emplus_agent: bool,
}

/// The production random source, named once so the type ban still covers every field.
#[allow(
    clippy::disallowed_types,
    reason = "0016 U27: the production random source, named once"
)]
type ProductionRandom = em_crypto::rand::OsRandom;

/// The controller's protocol state and peer models.
pub struct CtrlCore<R: RandomSource = ProductionRandom> {
    orch: Orchestrator<CtrlRules>,
    rules: CtrlRules,
    nodes: CtrlNodes,
    msg_ids: MsgIdCounter,
    /// Recorded at the first registration and fixed for the life of the process.
    al_mac: Option<MacAddr>,
    registered: bool,
    models: SortedVecMap<MacAddr, PeerModel>,
    /// AL MACs of `models`, least recently heard from first. A Search, an M1 whose M2 was
    /// built and validated (an ignored or refused one does not count) and a Topology Response
    /// move the agent to the back (0014 D18 R3).
    model_order: VecDeque<MacAddr>,
    networks: Networks,
    random: R,
    /// The clock for the M2 build timing; a fixed one makes replays repeatable.
    measure_now: fn() -> Instant,
    m2_handoff: M2Handoff,
    proto_deadlines: Vec<NodeDeadline>,
    /// The current Renew retry episode of a radio (0014 D18 R1). A radio has an entry from
    /// the cancel that started its episode, and the entry stays once the episode has sent
    /// its five Renews. A move or an eviction removes it; no entry means no episode.
    renew_counts: SortedVecMap<NodeKey, RenewEpisode>,
    orch_deadline: Option<Instant>,
    /// Frames produced by the current input, taken by its entry point.
    outbox: Vec<Frame>,
}

impl CtrlCore<ProductionRandom> {
    /// Creates a core that has not yet registered.
    ///
    /// `networks` is the reset file's `NetworkSSIDList` (0013 D2). With `drop_first_m2` the
    /// first M2's hand-off is replaced by a no-op, the test-only switch of 0013 D10.
    #[must_use]
    #[allow(
        clippy::disallowed_methods,
        clippy::disallowed_types,
        reason = "0016 U27: the production constructor builds OsRandom (OsRandom::new) and reads the real clock (Instant::now)"
    )]
    pub fn new(networks: Vec<Box<NetworkSsidInfo>>, drop_first_m2: bool) -> Self {
        // The one place the production core reads the real clock.
        Self::with_sources(
            networks,
            drop_first_m2,
            em_crypto::rand::OsRandom::new(),
            Instant::now,
        )
    }
}

impl<R: RandomSource> CtrlCore<R> {
    /// Creates a core with the given random source and measurement clock.
    ///
    /// For replay tests (a seeded source and a fixed clock); [`CtrlCore::new`] is the
    /// production constructor.
    #[must_use]
    pub fn with_sources(
        networks: Vec<Box<NetworkSsidInfo>>,
        drop_first_m2: bool,
        random: R,
        measure_now: fn() -> Instant,
    ) -> Self {
        Self {
            orch: Orchestrator::new(),
            rules: CtrlRules,
            nodes: CtrlNodes::default(),
            msg_ids: MsgIdCounter::new(),
            al_mac: None,
            registered: false,
            models: SortedVecMap::new(),
            model_order: VecDeque::new(),
            networks: Networks::new(networks),
            random,
            measure_now,
            m2_handoff: if drop_first_m2 {
                M2Handoff::DropNext
            } else {
                M2Handoff::Deliver
            },
            proto_deadlines: Vec::new(),
            renew_counts: SortedVecMap::new(),
            orch_deadline: None,
            outbox: Vec::new(),
        }
    }

    /// The node table, for logging and tests.
    #[must_use]
    pub const fn nodes(&self) -> &NodeTable<CtrlState> {
        &self.nodes.table
    }

    /// The model of the agent with this AL MAC, if a Search created one.
    #[must_use]
    pub fn peer(&self, al_mac: MacAddr) -> Option<&PeerModel> {
        self.models.get(&al_mac)
    }

    /// The transport registered with `al_mac`.
    ///
    /// The first registration creates the AL node (a temporary difference until the
    /// database exists, 0012 D3). Later ones with the same AL MAC change nothing.
    ///
    /// # Errors
    ///
    /// [`AlMacChanged`] when a later registration returns a different AL MAC.
    pub fn registered(&mut self, al_mac: MacAddr) -> Result<(), AlMacChanged> {
        match self.al_mac {
            Some(recorded) if recorded != al_mac => Err(AlMacChanged {
                recorded,
                new: al_mac,
            }),
            Some(_) => {
                self.registered = true;
                Ok(())
            }
            None => {
                self.al_mac = Some(al_mac);
                let key = NodeKey {
                    mac: al_mac,
                    kind: NodeKind::Al,
                };
                self.nodes.table.insert(Node::new(
                    key,
                    CtrlState::Unconfigured,
                    CONTROLLER_PROFILE,
                    None,
                ));
                self.registered = true;
                Ok(())
            }
        }
    }

    /// The transport lost its registration. No proto tick runs until the next one.
    pub fn unregistered(&mut self) {
        self.registered = false;
    }

    /// A CMDU arrived from `remote`. The controller handles a Search, an M1 and a Topology
    /// Response (`em_ctrl.cpp:1197-1270`, `em_configuration.cpp:6553-6622`).
    pub fn received(&mut self, remote: MacAddr, cmdu: &[u8], now: Instant) -> Vec<Frame> {
        let msg_type = match CmduHeader::parse(cmdu) {
            Ok((header, _)) => header.msg_type,
            Err(error) => {
                tracing::warn!(%error, %remote, "dropping CMDU with an unreadable header");
                return Vec::new();
            }
        };
        match msg_type {
            MsgType::AP_AUTOCONFIGURATION_SEARCH => self.handle_search(cmdu, now),
            MsgType::AP_AUTOCONFIGURATION_WSC => self.handle_wsc(remote, cmdu, now),
            MsgType::TOPOLOGY_RESPONSE => self.handle_topology_response(remote, cmdu, now),
            _ => {
                tracing::debug!(?msg_type, %remote, "ignoring CMDU the controller does not handle");
            }
        }
        std::mem::take(&mut self.outbox)
    }

    /// Runs whatever is due at `now`: an orchestrator pass, then the proto tick of each due
    /// radio node. While unregistered no proto tick runs.
    pub fn tick(&mut self, now: Instant) -> Vec<Frame> {
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
                self.dispatch_proto_tick(key, now);
            }
        }
        std::mem::take(&mut self.outbox)
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

    fn handle_search(&mut self, cmdu: &[u8], now: Instant) {
        let Some(al_mac) = self.al_mac else {
            tracing::warn!("dropping Search received before the first registration");
            return;
        };
        let route = match configuration::route_search(cmdu) {
            Ok(route) => route,
            Err(error) => {
                tracing::warn!(%error, "Search could not be routed");
                return;
            }
        };
        let mut out = Outputs::new();
        let key = NodeKey {
            mac: al_mac,
            kind: NodeKind::Al,
        };
        let mut ctx = EngineCtx {
            nodes: &mut self.nodes.table,
            key,
            commands: &NoCommands,
            msg_ids: &mut self.msg_ids,
            now,
            out: &mut out,
        };
        if let Err(error) = configuration::handle_search(&mut ctx, cmdu) {
            tracing::warn!(%error, agent = %route.al_mac, "Search rejected");
        }
        self.outbox.extend(collect_frames(out));
        // Upstream requests the model commit in routing, before the handler runs,
        // so it happens whatever the handler did. It is applied after the
        // handler's frames are handed over (0012 D3).
        self.commit_peer(&route, now);
    }

    fn commit_peer(&mut self, route: &SearchRoute, now: Instant) {
        if let Some(model) = self.models.get_mut(&route.al_mac) {
            model.is_emplus_agent = route.is_emplus_agent;
            self.touch_model(route.al_mac);
            tracing::info!(agent = %route.al_mac, "existing peer model");
        } else {
            if self.models.len() >= MAX_PEER_MODELS {
                self.evict_least_recent_model(now);
            }
            self.model_order.push_back(route.al_mac);
            self.models.insert(
                route.al_mac,
                PeerModel {
                    profile: route.profile,
                    is_emplus_agent: route.is_emplus_agent,
                },
            );
            tracing::info!(
                agent = %route.al_mac,
                profile = route.profile,
                "peer model created"
            );
        }
    }

    /// Marks the agent's model as heard from just now. The order is a short list (at most
    /// [`MAX_PEER_MODELS`]), so the linear search is fine.
    pub(crate) fn touch_model(&mut self, agent_al: MacAddr) {
        if let Some(position) = self.model_order.iter().position(|mac| *mac == agent_al) {
            self.model_order.remove(position);
            self.model_order.push_back(agent_al);
        }
    }

    /// intentional-difference:peer-model-lru-eviction. Upstream keeps models without limit;
    /// here the model heard from least recently goes, with everything the agent owns: its
    /// `em_config` chains first, then its radio nodes, links and per-radio state. Nothing of
    /// the agent is left to tick, time out or be sent a Renew.
    fn evict_least_recent_model(&mut self, now: Instant) {
        let Some(evicted) = self.model_order.pop_front() else {
            return;
        };
        self.models.remove(&evicted);
        let radios = self.nodes.radios_of(evicted);
        for radio in &radios {
            self.remove_em_config_chain(*radio, now);
        }
        for radio in radios {
            self.nodes.table.remove(radio);
            self.nodes.links.remove(&radio);
            self.reset_radio_per_radio_state(radio);
        }
        tracing::info!(agent = %evicted, "peer model evicted, the table is full");
    }

    /// Forgets the state a radio keeps outside its node and link: its proto deadline and its
    /// Renew retry counter. Called when a radio moves to another agent and when its agent is
    /// evicted. Neither is a cancel, so no Renew is sent and no episode starts.
    pub(crate) fn reset_radio_per_radio_state(&mut self, key: NodeKey) {
        self.proto_deadlines.retain(|deadline| deadline.key != key);
        self.renew_counts.remove(&key);
    }

    /// Activation and readiness after an event; restarts the idle orchestrator deadline.
    fn run_event_pass(&mut self, now: Instant) {
        self.run_pass(now, false);
    }

    fn run_pass(&mut self, now: Instant, timer: bool) {
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
    }

    fn drain_orch_out(&mut self, requests: Vec<RoleEffect<CtrlState>>, now: Instant) {
        for request in requests {
            match request {
                RoleEffect::RestartProto(key) => self.restart_proto_deadline(key, now),
                RoleEffect::Renew(key) => self.start_renew_episode(key, now),
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

    /// Whether `em_config` is executing on the node: upstream handles a node's state only
    /// while its orchestration state is `Progress` (`em.cpp:538-546`).
    fn em_config_in_progress(&self, key: NodeKey) -> bool {
        self.orch.orch_state(key) == OrchState::Progress
            && self
                .orch
                .active(key)
                .is_some_and(|active| active.kind == CommandType::EmConfig)
    }
}

fn collect_frames(out: Outputs) -> Vec<Frame> {
    let mut frames = Vec::new();
    for output in out.events {
        if let Output::Frame { remote, cmdu } = output {
            log_response_sent(&cmdu);
            frames.push(Frame { remote, cmdu });
        }
    }
    frames
}

fn log_response_sent(cmdu: &[u8]) {
    let Ok((header, area)) = CmduHeader::parse(cmdu) else {
        return;
    };
    let band = readers::freq_band(area).ok().flatten();
    let profile = readers::profile(area).ok().flatten().map(u8::from);
    tracing::info!(?band, mid = header.msg_id, ?profile, "Response sent");
}

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "0016 U27: unit tests read the real clock once, for the fixed start, and use the seeded random source"
)]
mod s2_tests;
#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "0016 U27: unit tests read the real clock once, for the fixed start"
)]
mod tests;
