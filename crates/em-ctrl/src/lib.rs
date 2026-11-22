// SPDX-License-Identifier: Apache-2.0

//! Synchronous controller role core for slice S1 (design note 0012 D3, D9, D10).
//!
//! [`CtrlCore`] routes a Search, lets the AL node answer it and keeps the in-memory
//! peer model. It is driven by explicit inputs and has no sockets and no runtime; the tokio
//! role actor ([`run`]) moves inputs in and frames out.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::time::Instant;

use em_cmd::{NoCommands, NodeKey, NodeKind};
use em_core::configuration::{self, SearchRoute};
use em_core::{CtrlState, EngineCtx, MsgIdCounter, Node, NodeTable, Output, Outputs};
use em_msg::{CmduHeader, readers};
use em_types::{MacAddr, MsgType};

mod actor;
pub mod cli;
mod pending;

pub use actor::{ActorError, CtrlConfig, run};

/// The most peer models the core keeps. Upstream keeps them without limit; S2
/// replaces the map with the database (0012 D3, rev 7).
const MAX_PEER_MODELS: usize = 256;

/// The profile of the controller itself, given to its AL node (0012 D3).
const CONTROLLER_PROFILE: u8 = 3;

/// A CMDU for the role actor to send; the actor adds the local AL MAC and session (0012 D9).
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

/// The controller's protocol state and peer models.
pub struct CtrlCore {
    nodes: NodeTable<CtrlState>,
    msg_ids: MsgIdCounter,
    /// Recorded at the first registration and fixed for the life of the process.
    al_mac: Option<MacAddr>,
    models: HashMap<MacAddr, PeerModel>,
    /// AL MACs of `models`, oldest creation first. Updates do not reorder it.
    model_order: VecDeque<MacAddr>,
}

impl Default for CtrlCore {
    fn default() -> Self {
        Self::new()
    }
}

impl CtrlCore {
    /// Creates a core that has not yet registered.
    #[must_use]
    pub fn new() -> Self {
        Self {
            nodes: NodeTable::new(),
            msg_ids: MsgIdCounter::new(),
            al_mac: None,
            models: HashMap::new(),
            model_order: VecDeque::new(),
        }
    }

    /// The node table, for logging and tests.
    #[must_use]
    pub const fn nodes(&self) -> &NodeTable<CtrlState> {
        &self.nodes
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
            Some(_) => Ok(()),
            None => {
                self.al_mac = Some(al_mac);
                let key = NodeKey {
                    mac: al_mac,
                    kind: NodeKind::Al,
                };
                self.nodes.insert(Node::new(
                    key,
                    CtrlState::Unconfigured,
                    CONTROLLER_PROFILE,
                    None,
                ));
                Ok(())
            }
        }
    }

    /// A CMDU arrived from `remote`. Only an AP-Autoconfiguration Search is
    /// handled in S1 (`em_ctrl.cpp:1197-1238`).
    pub fn received(&mut self, remote: MacAddr, cmdu: &[u8], now: Instant) -> Vec<Frame> {
        let msg_type = match CmduHeader::parse(cmdu) {
            Ok((header, _)) => header.msg_type,
            Err(error) => {
                tracing::warn!(%error, %remote, "dropping CMDU with an unreadable header");
                return Vec::new();
            }
        };
        if msg_type != MsgType::AP_AUTOCONFIGURATION_SEARCH {
            tracing::debug!(?msg_type, %remote, "ignoring CMDU not handled in S1");
            return Vec::new();
        }
        self.handle_search(cmdu, now)
    }

    fn handle_search(&mut self, cmdu: &[u8], now: Instant) -> Vec<Frame> {
        let Some(al_mac) = self.al_mac else {
            tracing::warn!("dropping Search received before the first registration");
            return Vec::new();
        };
        let route = match configuration::route_search(cmdu) {
            Ok(route) => route,
            Err(error) => {
                tracing::warn!(%error, "Search could not be routed");
                return Vec::new();
            }
        };
        let mut out = Outputs::new();
        let key = NodeKey {
            mac: al_mac,
            kind: NodeKind::Al,
        };
        let mut ctx = EngineCtx {
            nodes: &mut self.nodes,
            key,
            commands: &NoCommands,
            msg_ids: &mut self.msg_ids,
            now,
            out: &mut out,
        };
        if let Err(error) = configuration::handle_search(&mut ctx, cmdu) {
            tracing::warn!(%error, agent = %route.al_mac, "Search rejected");
        }
        let frames = collect_frames(out);
        // Upstream requests the model commit in routing, before the handler runs,
        // so it happens whatever the handler did. It is applied after the
        // handler's frames are handed over (0012 D3).
        self.commit_peer(&route);
        frames
    }

    fn commit_peer(&mut self, route: &SearchRoute) {
        if let Some(model) = self.models.get_mut(&route.al_mac) {
            model.is_emplus_agent = route.is_emplus_agent;
            tracing::info!(agent = %route.al_mac, "existing peer model");
        } else {
            if self.models.len() >= MAX_PEER_MODELS {
                self.evict_oldest_model();
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
}

impl CtrlCore {
    fn evict_oldest_model(&mut self) {
        if let Some(oldest) = self.model_order.pop_front() {
            self.models.remove(&oldest);
            tracing::info!(agent = %oldest, "peer model evicted, the table is full");
        }
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
mod tests;
