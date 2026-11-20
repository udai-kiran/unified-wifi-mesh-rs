// SPDX-License-Identifier: Apache-2.0

//! `AgentModel`, the `ModelAccess` trait and the re-borrow pattern for cross-node reads
//! (plan C8, A2).

use super::fixtures::{agent_model, mac, net, node_init, radio};
use crate::AgentModel;
use crate::model::access::{ModelAccess, NodeCtx, NodeRef};
use crate::model::keys::NodeId;
use crate::model::network::NetworkModel;
use crate::model::node::{NodeModel, NodeRole};
use std::collections::BTreeMap;

fn id(net_id: &str, last: u8) -> NodeId {
    NodeId {
        net: net(net_id),
        al: mac(last),
    }
}

// --- AgentModel ---

#[test]
fn the_agent_model_holds_one_network_and_one_node_of_the_init() {
    let mut init = node_init(NodeRole::Agent);
    init.net = net("AgentNet");
    init.al = mac(7);
    let model = AgentModel::new(init);
    assert_eq!(model.network().net(), net("AgentNet"));
    assert_eq!(model.network().info().id.as_str(), "AgentNet");
    assert_eq!(model.node_model().net(), net("AgentNet"));
    assert_eq!(model.node_model().agent_al_interface_mac(), mac(7));
    assert!(!model.node_model().is_controller());
}

/// Upstream's agent `get_data_model` ignores its arguments (`inc/em_agent.h:862`).
#[test]
fn the_agent_model_ignores_the_node_id() {
    let mut model = agent_model();
    let unrelated = id("somewhere else", 99);
    assert!(model.node(&unrelated).is_some());
    assert_eq!(
        model
            .node(&unrelated)
            .unwrap()
            .node
            .agent_al_interface_mac(),
        mac(1)
    );
    assert!(model.node_ctx(&unrelated).is_some());
    assert_eq!(model.nodes().count(), 1);
}

#[test]
fn the_node_ref_reports_its_own_id() {
    let model = agent_model();
    let node = model.node(&id("x", 0)).unwrap();
    assert_eq!(node.id(), id("OneWifiMesh", 1));
}

#[test]
fn a_write_through_the_context_is_seen_by_a_later_read() {
    let mut model = agent_model();
    {
        let ctx = model.node_ctx(&id("x", 0)).unwrap();
        ctx.node.put_radio(radio(mac(10), true)).unwrap();
        ctx.network.set_controller_id(mac(3));
    }
    let node = model.node(&id("x", 0)).unwrap();
    assert_eq!(node.node.radios().len(), 1);
    assert_eq!(node.network.controller_interface_mac(), mac(3));
}

// --- the trait over a role-agnostic caller ---

/// A two-node model for the cross-node pattern. The mesh model of brief 3b is the real one;
/// the trait is what a handler sees.
struct TwoNodes {
    nodes: BTreeMap<NodeId, (NetworkModel, NodeModel)>,
}

impl TwoNodes {
    fn new() -> Self {
        let mut nodes = BTreeMap::new();
        for last in [1_u8, 2] {
            let mut init = node_init(NodeRole::Agent);
            init.al = mac(last);
            let node_id = NodeId {
                net: init.net,
                al: init.al,
            };
            nodes.insert(node_id, (NetworkModel::new(init.net), NodeModel::new(init)));
        }
        Self { nodes }
    }
}

impl ModelAccess for TwoNodes {
    fn node_ctx(&mut self, id: &NodeId) -> Option<NodeCtx<'_>> {
        self.nodes.get_mut(id).map(|(network, node)| NodeCtx {
            id: *id,
            network,
            node,
        })
    }

    fn node(&self, id: &NodeId) -> Option<NodeRef<'_>> {
        self.nodes
            .get_key_value(id)
            .map(|(&id, (network, node))| NodeRef { id, network, node })
    }

    fn nodes(&self) -> Box<dyn Iterator<Item = NodeRef<'_>> + '_> {
        Box::new(
            self.nodes
                .iter()
                .map(|(&id, (network, node))| NodeRef { id, network, node }),
        )
    }
}

/// A handler that needs another node's data re-borrows through `node()` between uses of its
/// own context; it never holds two at once (`src/em/config/em_configuration.cpp:1752`).
fn copy_radio_count_from_peer(model: &mut dyn ModelAccess, own: &NodeId, peer: &NodeId) -> usize {
    // First use of the own context.
    model.node_ctx(own).unwrap().node.set_topo_state(true);
    // The peer is read through a shared borrow after the first context is gone.
    let peer_radios = model.node(peer).unwrap().node.radios().len();
    // Second use of the own context.
    let ctx = model.node_ctx(own).unwrap();
    ctx.node.set_msg_id(u16::try_from(peer_radios).unwrap());
    peer_radios
}

#[test]
fn a_handler_reads_another_node_by_reborrowing_between_two_context_uses() {
    let mut model = TwoNodes::new();
    let own = NodeId {
        net: node_init(NodeRole::Agent).net,
        al: mac(1),
    };
    let peer = NodeId { al: mac(2), ..own };
    model
        .node_ctx(&peer)
        .unwrap()
        .node
        .put_radio(radio(mac(10), true))
        .unwrap();
    model
        .node_ctx(&peer)
        .unwrap()
        .node
        .put_radio(radio(mac(11), true))
        .unwrap();

    let seen = copy_radio_count_from_peer(&mut model, &own, &peer);

    assert_eq!(seen, 2);
    let own_ref = model.node(&own).unwrap();
    assert!(own_ref.node.topo_state());
    assert_eq!(own_ref.node.msg_id(), 2);
}

#[test]
fn the_same_handler_runs_against_the_agent_model() {
    let mut model = agent_model();
    let own = id("OneWifiMesh", 1);
    let seen = copy_radio_count_from_peer(&mut model, &own, &own);
    assert_eq!(seen, 0);
    assert!(model.node_model().topo_state());
}

#[test]
fn nodes_iterate_in_key_order_and_an_unknown_id_finds_nothing() {
    let model = TwoNodes::new();
    let order: Vec<_> = model.nodes().map(|node| node.id().al).collect();
    assert_eq!(order, vec![mac(1), mac(2)]);
    assert!(model.node(&id("nope", 1)).is_none());
}
