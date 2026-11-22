// SPDX-License-Identifier: Apache-2.0

//! Synchronous agent role core for slice S1 (design note 0012 D2, D9, D10).
//!
//! [`AgentCore`] owns the orchestrator, the node table and the timers. It is driven
//! by explicit inputs and `Instant`s and returns frames and its next deadline; it
//! has no sockets and no runtime. The tokio role actor ([`run`]) moves inputs in
//! and frames out.

use std::fmt;
use std::time::{Duration, Instant};

use em_cmd::{ChainId, Command, DevInit, NodeKey, NodeKind, RadioSpec};
use em_core::configuration;
use em_core::{AgentState, EngineCtx, MsgIdCounter, Node, NodeTable, Output, Outputs};
use em_msg::CmduHeader;
use em_orch::{Orchestrator, RoleEffect};
use em_types::{MacAddr, MsgType};

mod actor;
pub mod cli;
mod pending;
pub mod platform;
mod rules;

pub use actor::{ActorError, AgentConfig, run};
pub use rules::AgentRules;

/// Idle time after which a node's proto tick fires (`em.cpp:644-680`).
const PROTO_IDLE: Duration = Duration::from_secs(1);
/// Idle time after which the orchestrator ticks (`EM_ORCH_TICK`, 250 ms).
const ORCH_IDLE: Duration = Duration::from_millis(250);

/// A CMDU for the role actor to send; the actor adds the local AL MAC and session (0012 D9).
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

/// The proto tick deadline of one node.
#[derive(Debug, Clone, Copy)]
struct NodeDeadline {
    key: NodeKey,
    due: Instant,
}

/// The agent's protocol and orchestration state.
pub struct AgentCore {
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
}

impl AgentCore {
    /// Creates a core that has not yet registered. `profile` is the port's
    /// `--profile` switch (0012 D4); `radios` come from the platform.
    #[must_use]
    pub fn new(profile: u8, radios: Vec<RadioSpec>) -> Self {
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

    /// A CMDU arrived from `remote`. Only an AP-Autoconfiguration Response is
    /// handled in S1 (`em_agent.cpp:2366-2396`).
    pub fn received(&mut self, remote: MacAddr, cmdu: &[u8], now: Instant) -> Vec<Frame> {
        let mut frames = Vec::new();
        let msg_type = match CmduHeader::parse(cmdu) {
            Ok((header, _)) => header.msg_type,
            Err(error) => {
                tracing::warn!(%error, %remote, "dropping CMDU with an unreadable header");
                return frames;
            }
        };
        if msg_type != MsgType::AP_AUTOCONFIGURATION_RESPONSE {
            tracing::debug!(?msg_type, %remote, "ignoring CMDU not handled in S1");
            return frames;
        }
        match configuration::route_response(&self.nodes, cmdu) {
            Ok(Some(key)) => self.dispatch_response(key, remote, cmdu, now, &mut frames),
            Ok(None) => tracing::debug!(%remote, "no radio accepts this Response"),
            Err(error) => tracing::warn!(%error, %remote, "Response could not be routed"),
        }
        frames
    }

    /// Runs whatever is due at `now`: an orchestrator pass, then the proto
    /// tick of each due node. While unregistered no proto tick runs (0012 D9).
    pub fn tick(&mut self, now: Instant) -> Vec<Frame> {
        let mut frames = Vec::new();
        if self.orch_deadline.is_some_and(|due| due <= now) {
            let effects = self.orch.tick(&mut self.rules, &self.nodes, now);
            self.apply(effects, now);
            self.orch_deadline = Some(now + ORCH_IDLE);
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

    fn submit_dev_init(&mut self, al_mac: MacAddr, now: Instant) {
        let command = Command::DevInit(DevInit {
            al_mac,
            radios: Vec::from(self.radios.as_slice()),
        });
        let (chain, effects) = self.orch.submit(&mut self.rules, command, now);
        // The Docker check 6a counts this line (0012 D8).
        tracing::info!(chain = chain.0, "dev_init submitted");
        self.apply(effects, now);
        self.admit(chain, now);
    }

    fn admit(&mut self, chain: ChainId, now: Instant) {
        if self.orch.admit(&self.rules, &self.nodes, chain) {
            self.run_event_pass(now);
        } else {
            tracing::warn!("dev_init has no candidates: not ready");
        }
    }

    /// Activation and readiness after an event; restarts the idle orchestrator deadline.
    fn run_event_pass(&mut self, now: Instant) {
        let effects = self.orch.on_event(&mut self.rules, &self.nodes, now);
        self.apply(effects, now);
        self.orch_deadline = Some(now + ORCH_IDLE);
    }

    fn apply(&mut self, effects: Vec<RoleEffect<AgentState>>, now: Instant) {
        for effect in effects {
            match effect {
                RoleEffect::CreateNode {
                    key,
                    band,
                    profile,
                    state,
                } => {
                    self.nodes.insert(Node::new(key, state, profile, band));
                    self.restart_proto_deadline(key, now);
                }
                RoleEffect::SetState(key, state) => {
                    if let Some(node) = self.nodes.get_mut(key) {
                        node.state = state;
                    }
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
        let mut out = Outputs::new();
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
        self.finish_event(key, now, out, frames);
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
        }
        self.finish_event(key, now, out, frames);
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
mod tests;
