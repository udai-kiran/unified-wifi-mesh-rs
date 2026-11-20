// SPDX-License-Identifier: Apache-2.0

//! `AgentModel`: the agent's one network and one node.

use crate::model::access::{ModelAccess, NodeCtx, NodeRef};
use crate::model::keys::NodeId;
use crate::model::network::NetworkModel;
use crate::model::node::{NodeInit, NodeModel};

/// The agent's model: its own node and the network that node belongs to (0007 D2).
///
/// ```compile_fail
/// fn needs_clone<T: Clone>() {}
/// needs_clone::<em_dm::AgentModel>();
/// ```
///
/// ```compile_fail
/// fn needs_copy<T: Copy>() {}
/// needs_copy::<em_dm::AgentModel>();
/// ```
///
/// A compiling control for the two probes above:
///
/// ```
/// fn needs_clone<T: Clone>() {}
/// needs_clone::<em_dm::MacAddr>();
/// let _: Option<em_dm::AgentModel> = None;
/// ```
#[derive(Debug, PartialEq)]
pub struct AgentModel {
    // Boxed so the model itself stays small when it is moved (0007 criterion 4); a node is 680
    // bytes and a network 160.
    network: Box<NetworkModel>,
    node: Box<NodeModel>,
    // The key the node was created under; later writes to the device's AL MAC do not change it.
    id: NodeId,
}

impl AgentModel {
    /// Creates the agent's model: a default network for `init.net` (plan C7) and the node of
    /// [`NodeModel::new`].
    #[must_use]
    pub fn new(init: NodeInit) -> Self {
        let id = NodeId {
            net: init.net,
            al: init.al,
        };
        Self {
            network: Box::new(NetworkModel::new(init.net)),
            node: Box::new(NodeModel::new(init)),
            id,
        }
    }

    /// The agent's network.
    #[must_use]
    pub fn network(&self) -> &NetworkModel {
        &self.network
    }

    /// The agent's node.
    #[must_use]
    pub fn node_model(&self) -> &NodeModel {
        &self.node
    }

    /// Mutable access to the node and its network.
    pub fn ctx(&mut self) -> NodeCtx<'_> {
        NodeCtx {
            id: self.id,
            network: &mut self.network,
            node: &mut self.node,
        }
    }
}

impl ModelAccess for AgentModel {
    fn node_ctx(&mut self, _id: &NodeId) -> Option<NodeCtx<'_>> {
        Some(self.ctx())
    }

    fn node(&self, _id: &NodeId) -> Option<NodeRef<'_>> {
        Some(NodeRef {
            id: self.id,
            network: &self.network,
            node: &self.node,
        })
    }

    fn nodes(&self) -> Box<dyn Iterator<Item = NodeRef<'_>> + '_> {
        Box::new(std::iter::once(NodeRef {
            id: self.id,
            network: &self.network,
            node: &self.node,
        }))
    }
}
