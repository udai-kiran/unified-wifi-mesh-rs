// SPDX-License-Identifier: Apache-2.0

//! Role-agnostic access to the nodes of a model (plan C8).
//!
//! The controller's `MeshModel` and the agent's `AgentModel` implement [`ModelAccess`], so
//! engine code is written once. The access is short-lived: a handler that needs another node's
//! data (for example `src/em/config/em_configuration.cpp:1752`) calls [`ModelAccess::node`]
//! between two uses of its own [`NodeCtx`] and never holds both. [`NodeCtx`] is a split borrow of
//! two separate fields of the model, in safe Rust.

use crate::model::keys::NodeId;
use crate::model::network::NetworkModel;
use crate::model::node::NodeModel;

/// Mutable access to one node and its network.
#[derive(Debug)]
pub struct NodeCtx<'a> {
    /// The key the node is stored under: the one it was fetched by, which does not follow later
    /// writes to the device's AL MAC.
    pub id: NodeId,
    /// The node's network record and SSID table.
    pub network: &'a mut NetworkModel,
    /// The node.
    pub node: &'a mut NodeModel,
}

/// Shared access to one node and its network.
#[derive(Debug, Clone, Copy)]
pub struct NodeRef<'a> {
    /// The key the node is stored under: the one it was fetched by, which does not follow later
    /// writes to the device's AL MAC.
    pub id: NodeId,
    /// The node's network record and SSID table.
    pub network: &'a NetworkModel,
    /// The node.
    pub node: &'a NodeModel,
}

impl NodeRef<'_> {
    /// The node's identity: the [`NodeId`] it is stored under.
    ///
    /// It is the storage key, not a value derived from the device record: the AL MAC of the
    /// device can change (`set_ctrl_al_interface_mac`, `set_agent_al_interface_mac`,
    /// `device_mut`, `put_device`, `update_device`) while the node stays under its key, and
    /// `node(&r.id())` must find the node again.
    #[must_use]
    pub fn id(&self) -> NodeId {
        self.id
    }
}

/// Access to the nodes of a model, whatever its role.
pub trait ModelAccess {
    /// The node `id` with mutable access, or `None` if the model has no such node.
    ///
    /// `AgentModel` ignores `id` and returns its single node, as the agent's `get_data_model`
    /// ignores its arguments (`inc/em_agent.h:862`).
    fn node_ctx(&mut self, id: &NodeId) -> Option<NodeCtx<'_>>;

    /// The node `id` with shared access; the same rule for `AgentModel`.
    fn node(&self, id: &NodeId) -> Option<NodeRef<'_>>;

    /// Every node, in key order (`NodeId` order).
    fn nodes(&self) -> Box<dyn Iterator<Item = NodeRef<'_>> + '_>;
}
