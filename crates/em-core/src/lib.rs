// SPDX-License-Identifier: Apache-2.0

//! Synchronous nodes, outputs, and S1 configuration discovery (note 0012).
//! The caller owns time, orchestration, the message ID counter, and transport.

use std::time::Instant;

use em_cmd::{CommandView, NodeKey};
use em_types::{FreqBand, MacAddr};

pub mod configuration;
pub mod topology;
pub mod wsc;

/// Agent states used by discovery, with their upstream `em_state_t` values
/// (`inc/em_base.h:2344-2369`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u16)]
pub enum AgentState {
    /// The radio has not started configuration.
    Unconfigured = 0,
    /// The 1905 layer is not configured.
    Ieee1905Unconfigured = 1,
    /// The 1905 layer is being secured.
    Ieee1905Securing = 2,
    /// A Search was handed off and a Response is awaited.
    AutoconfigRspPending = 3,
    /// A Response was accepted; the agent core then sends WSC M1 (0013 D4).
    WscM2Pending = 4,
    /// WSC M2 was processed.
    WscM2Processed = 5,
    /// Platform configuration is awaited.
    OwconfigPending = 6,
    /// Platform BSS configuration was indicated.
    OneWifiBssconfigInd = 7,
    /// An autoconfiguration renewal is pending.
    AutoconfigRenewPending = 8,
    /// Topology has synchronized.
    TopoSynchronized = 9,
    /// An AP capability report is pending.
    ApCapReport = 10,
    /// A channel preference query is pending.
    ChannelPrefQuery = 11,
    /// Channel selection is pending.
    ChannelSelectionPending = 12,
    /// Channel selection configuration is pending.
    ChannelSelectConfigurationPending = 13,
    /// A channel report is pending.
    ChannelReportPending = 14,
    /// A channel scan result is pending.
    ChannelScanResultPending = 15,
    /// Unassociated station metrics are pending.
    UnassocStaMetricsReportPending = 16,
    /// Configuration has completed.
    Configured = 17,
    /// A topology notification is pending.
    TopologyNotify = 18,
    /// A client capability report is pending.
    ClientCapReport = 19,
    /// Station link metrics are pending.
    StaLinkMetricsPending = 20,
    /// A steering response is pending.
    SteerBtmResPending = 21,
    /// A link quality report is pending.
    LinkQualityReportPending = 22,
}

/// Controller states needed by S1 and S2 (`inc/em_base.h:2371-2373`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum CtrlState {
    /// The controller node has not yet handled a Search.
    Unconfigured = 0x100,
    /// A Response was handed off and WSC M1 is awaited.
    WscM1Pending = 0x101,
    /// M2 was handed to the transport.
    WscM2Sent = 0x102,
    /// Topology synchronization is pending.
    TopoSyncPending = 0x103,
    /// The device topology is synchronized.
    TopoSynchronized = 0x104,
    /// Configuration timed out.
    Misconfigured = 0x110,
}

/// Controller identity learned from a Response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControllerPeer {
    /// AL MAC address carried by the transport's remote address.
    pub al_mac: MacAddr,
    /// Profile after applying the reserved-value rule.
    pub profile: u8,
}

/// One node's identity, protocol state and profile. Orchestration state lives in `em-orch`.
#[derive(Debug, PartialEq, Eq)]
pub struct Node<S> {
    /// Identity including AL/radio kind.
    pub key: NodeKey,
    /// Role-specific protocol state.
    pub state: S,
    /// Local Multi-AP profile, set when the node is created.
    pub profile: u8,
    /// Radio frequency band; AL nodes have no band.
    pub band: Option<FreqBand>,
    /// Controller learned by an agent radio, if any.
    pub controller: Option<ControllerPeer>,
}

impl<S> Node<S> {
    /// Creates a node without a learned controller.
    #[must_use]
    pub const fn new(key: NodeKey, state: S, profile: u8, band: Option<FreqBand>) -> Self {
        Self {
            key,
            state,
            profile,
            band,
            controller: None,
        }
    }
}

/// A node table in insertion order, including distinct AL and radio keys.
#[derive(Debug, PartialEq, Eq)]
pub struct NodeTable<S> {
    nodes: Vec<Node<S>>,
}

impl<S> Default for NodeTable<S> {
    fn default() -> Self {
        Self { nodes: Vec::new() }
    }
}

impl<S> NodeTable<S> {
    /// Creates an empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a node, replacing an identical key in place and returning its old value.
    pub fn insert(&mut self, node: Node<S>) -> Option<Node<S>> {
        if let Some(existing) = self.nodes.iter_mut().find(|item| item.key == node.key) {
            Some(std::mem::replace(existing, node))
        } else {
            self.nodes.push(node);
            None
        }
    }

    /// Borrows the node at `key`.
    #[must_use]
    pub fn get(&self, key: NodeKey) -> Option<&Node<S>> {
        self.nodes.iter().find(|node| node.key == key)
    }

    /// Mutably borrows the node at `key`.
    pub fn get_mut(&mut self, key: NodeKey) -> Option<&mut Node<S>> {
        self.nodes.iter_mut().find(|node| node.key == key)
    }

    /// Removes a node while preserving the order of the others.
    pub fn remove(&mut self, key: NodeKey) -> Option<Node<S>> {
        let index = self.nodes.iter().position(|node| node.key == key)?;
        Some(self.nodes.remove(index))
    }

    /// Iterates over nodes in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = &Node<S>> {
        self.nodes.iter()
    }

    /// Iterates mutably over nodes in insertion order.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Node<S>> {
        self.nodes.iter_mut()
    }
}

/// The process-owned message ID counter (`src/em/em_mgr.cpp:525-531,626`).
#[derive(Debug, Default)]
pub struct MsgIdCounter(u16);

impl MsgIdCounter {
    /// Creates a counter whose first issued ID is one.
    #[must_use]
    pub const fn new() -> Self {
        Self(0)
    }

    /// Advances the counter, skipping zero when it wraps.
    pub fn next_id(&mut self) -> u16 {
        self.0 = self.0.wrapping_add(1);
        if self.0 == 0 {
            self.0 = 1;
        }
        self.0
    }
}

/// One ordered effect of a synchronous handler.
#[derive(Debug, PartialEq, Eq)]
pub enum Output {
    /// A CMDU handed to the role actor for transmission.
    Frame {
        /// Destination AL MAC or multicast address.
        remote: MacAddr,
        /// Complete CMDU without an Ethernet header.
        cmdu: Vec<u8>,
    },
    /// A node changed state and orchestration readiness can be reevaluated.
    Progress(NodeKey),
}

/// Ordered effects. The role actor adds the local address and session to frames.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outputs {
    /// Effects in the order in which the handler produced them.
    pub events: Vec<Output>,
}

impl Outputs {
    /// Creates an empty list of effects.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Disjoint role-owned state borrowed for one synchronous engine invocation.
pub struct EngineCtx<'a, S> {
    /// The role's node table.
    pub nodes: &'a mut NodeTable<S>,
    /// Node receiving the event.
    pub key: NodeKey,
    /// Read-only active command and orchestration state.
    pub commands: &'a dyn CommandView,
    /// Process-wide message IDs.
    pub msg_ids: &'a mut MsgIdCounter,
    /// Time of this invocation, supplied by the role.
    pub now: Instant,
    /// Ordered effects emitted by this invocation.
    pub out: &'a mut Outputs,
}

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "0016 U27: unit tests use the real clock"
)]
mod tests;
