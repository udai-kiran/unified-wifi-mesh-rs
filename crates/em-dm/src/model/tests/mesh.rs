// SPDX-License-Identifier: Apache-2.0

//! `MeshModel`: the network lifecycle (plan C7), global finds (C4), removals (U2), node needed
//! (A6), the side maps, the walks, the list residue and the gate tests of plan A9.

use super::fixtures::{self, mac, net};
use super::mesh_support::{
    MAC1, MAC2, MAC3, MAC4, bss_in, bss_key, device_id, device_in, init, node_id, policy_in,
    policy_key, radio_in, scan_in, scan_key_in, sta_on, zeroed_network,
};
use crate::enums::{HaulType, OpClassType, PolicyIdType, ScannerType};
use crate::model::access::ModelAccess;
use crate::model::dirty::DbCfgType;
use crate::model::error::{ModelError, Put, Table};
use crate::model::keys::{NodeId, OpClassKey, ScanKey, SsidKey, StaKey};
use crate::model::node::{NodeRole, StaMapTarget};
use crate::records::{BeaconReport, Neighbor, WideBwChannel};
use crate::{
    ListOp, MAX_NETWORKS, MeshModel, NetId, SCAN_RESULT_SELF_INDEX, Ssid2VidChange, Ssid2VidKey,
};
use em_types::{MacAddr, MediaType, ProfileType};

fn agent(network: &str, al: MacAddr) -> crate::NodeInit {
    init(
        network,
        al,
        ProfileType::MULTI_AP_PROFILE_3,
        NodeRole::Agent,
    )
}

fn controller(network: &str, al: MacAddr) -> crate::NodeInit {
    init(
        network,
        al,
        ProfileType::MULTI_AP_PROFILE_3,
        NodeRole::Controller,
    )
}

fn ssid_key(id: &str) -> SsidKey {
    SsidKey::new(id.as_bytes()).unwrap()
}

fn not_found(table: Table) -> ModelError {
    ModelError::NotFound { table }
}

fn full(table: Table, capacity: usize) -> ModelError {
    ModelError::Full { table, capacity }
}

fn mesh_with_nodes(nodes: &[(&str, MacAddr)]) -> MeshModel {
    let mut mesh = MeshModel::new(None);
    for (network, al) in nodes {
        mesh.create_data_model(agent(network, *al)).unwrap();
    }
    mesh
}

fn neighbor(bssid: MacAddr) -> Neighbor {
    Neighbor {
        bssid,
        ssid: crate::CStrBytes::default(),
        signal_strength: 0,
        bandwidth: crate::ChannelBandwidth::MHZ_20,
        bss_color: 0,
        channel_util: 0,
        sta_count: 0,
        bss_load_element_present: 0,
    }
}

// --- network lifecycle (C7) ---

/// Plan C7: a node created in a network with no model gets a default network record with that
/// id; upstream leaves the node's network zeroed (`dm_easy_mesh_list.cpp:1781-1798`).
/// intentional-difference:c7-network-lifecycle
#[test]
fn creating_a_node_in_an_unknown_network_creates_a_default_network() {
    let mut mesh = MeshModel::new(None);
    assert!(mesh.get_network(&net("NetA")).is_none());
    mesh.create_data_model(agent("NetA", MAC1)).unwrap();
    let network = mesh.get_network(&net("NetA")).unwrap();
    assert_eq!(network.info().id.as_str(), "NetA");
    assert!(network.controller_interface_mac().is_zero());
    assert_eq!(network.num_network_ssids(), 0);
    // A second node of the network adds no network.
    mesh.create_data_model(agent("NetA", MAC2)).unwrap();
    assert_eq!(mesh.num_networks(), 1);
}

/// Plan C7: at 5 networks every path that would create a sixth fails and changes nothing.
/// Upstream overflows `m_network_list` in `put_network` (`:156-158`) and has no limit in
/// `create_data_model`. intentional-difference:c7-network-lifecycle
#[test]
fn the_sixth_network_is_refused_on_every_creation_path() {
    let mut mesh = MeshModel::new(None);
    for index in 0..5 {
        mesh.create_data_model(agent(&format!("Net{index}"), MAC1))
            .unwrap();
    }
    assert_eq!(mesh.num_networks(), MAX_NETWORKS);
    let before = (mesh.num_nodes(), mesh.num_networks());
    let expected = Err(full(Table::Network, 5));

    assert_eq!(mesh.create_data_model(agent("Sixth", MAC1)), expected);
    assert_eq!(mesh.put_network(net("Sixth"), zeroed_network()), expected);
    let key = device_id("Sixth", MAC1, MediaType(0));
    assert_eq!(
        mesh.put_device(key, device_in("Sixth", MAC1), false),
        expected
    );
    assert_eq!((mesh.num_nodes(), mesh.num_networks()), before);
    assert!(mesh.get_network(&net("Sixth")).is_none());

    // A node in an existing network is not a new network.
    assert_eq!(
        mesh.create_data_model(agent("Net0", MAC2)),
        Ok(Put::Inserted)
    );
}

/// Plan C7: `remove_network` is refused while the network has nodes, with an error and no
/// change; after the nodes are removed it succeeds. Upstream's body is empty
/// (`dm_easy_mesh_list.cpp:124-127`, plan U2). intentional-difference:c7-network-lifecycle
#[test]
fn remove_network_is_refused_while_nodes_exist() {
    let mut mesh = mesh_with_nodes(&[("NetA", MAC1), ("NetA", MAC2), ("NetB", MAC1)]);
    assert_eq!(
        mesh.remove_network(&net("NetA")).err(),
        Some(ModelError::NetworkInUse { nodes: 2 })
    );
    assert_eq!(mesh.num_networks(), 2);
    assert_eq!(mesh.num_nodes(), 3);

    mesh.delete_data_model(&node_id("NetA", MAC1)).unwrap();
    assert_eq!(
        mesh.remove_network(&net("NetA")).err(),
        Some(ModelError::NetworkInUse { nodes: 1 })
    );
    mesh.delete_data_model(&node_id("NetA", MAC2)).unwrap();
    assert!(mesh.remove_network(&net("NetA")).is_ok());
    assert!(mesh.get_network(&net("NetA")).is_none());
    assert!(mesh.get_network(&net("NetB")).is_some());
    assert_eq!(
        mesh.remove_network(&net("NetA")).err(),
        Some(not_found(Table::Network))
    );
}

/// A node removed with `delete_data_model` leaves its network, which can then be removed.
#[test]
fn deleting_the_last_node_leaves_the_network_in_place() {
    let mut mesh = mesh_with_nodes(&[("NetA", MAC1)]);
    assert!(mesh.delete_data_model(&node_id("NetA", MAC1)).is_some());
    assert_eq!(mesh.num_networks(), 1);
    assert_eq!(mesh.num_nodes(), 0);
}

/// `put_network` creates the controller's node when `(key, ctrl MAC)` has none and replaces the
/// record when it has one (`dm_easy_mesh_list.cpp:129-177`).
#[test]
fn put_network_creates_the_controller_node_once() {
    let mut mesh = MeshModel::new(None);
    let mut record = zeroed_network();
    record.ctrl_id.mac = MAC1;
    assert_eq!(mesh.put_network(net("NetA"), record), Ok(Put::Inserted));
    let node = mesh.get_data_model(&node_id("NetA", MAC1)).unwrap();
    assert!(node.is_controller());
    assert_eq!(node.device().profile, ProfileType::MULTI_AP_PROFILE_3);
    assert_eq!(node.device().id.dev_mac, MAC1);

    let mut again = zeroed_network();
    again.ctrl_id.mac = MAC1;
    again.num_of_devices = 7;
    assert_eq!(mesh.put_network(net("NetA"), again), Ok(Put::Replaced));
    assert_eq!(mesh.num_nodes(), 1);
    assert_eq!(
        mesh.get_network(&net("NetA"))
            .unwrap()
            .info()
            .num_of_devices,
        7
    );
}

/// Plan U1: the network record is stored once. A `put_network` is seen by every node of the
/// network and by no node of another one; upstream copies it into every node of every network
/// (`dm_easy_mesh_list.cpp:161-167`, research 7.4). intentional-difference:u1-network-stored-once
#[test]
fn the_network_record_is_stored_once_and_seen_by_every_node_of_the_network() {
    let mut mesh = mesh_with_nodes(&[("NetA", MAC1), ("NetA", MAC2), ("NetB", MAC1)]);
    let mut record = zeroed_network();
    record.ctrl_id.mac = MAC3;
    record.num_of_devices = 9;
    mesh.put_network(net("NetA"), record).unwrap();

    let first = mesh.node(&node_id("NetA", MAC1)).unwrap();
    let second = mesh.node(&node_id("NetA", MAC2)).unwrap();
    assert!(std::ptr::eq(first.network, second.network));
    assert_eq!(first.network.info().num_of_devices, 9);
    let other = mesh.node(&node_id("NetB", MAC1)).unwrap();
    assert_eq!(other.network.info().num_of_devices, 0);
    assert!(!std::ptr::eq(first.network, other.network));
}

/// `put_network` with a controller MAC that differs from the list's: the record's controller MAC
/// becomes the list's (`dm_easy_mesh_list.cpp:139-142`) and the controller's node is marked
/// `network_list_update` dirty (`:172-175`, plan C5). No mark without the mismatch.
#[test]
fn put_network_controller_id_mismatch_rewrites_the_record_and_marks_the_controller_node() {
    let mut mesh = MeshModel::new(Some(MAC1));
    let mut record = zeroed_network();
    record.ctrl_id.mac = MAC2;
    mesh.put_network(NetId::global(), record).unwrap();
    let controller_node = node_id(NetId::GLOBAL, MAC1);
    let node = mesh.get_data_model(&controller_node).unwrap();
    assert!(node.dirty().is_set(DbCfgType::NetworkListUpdate));
    assert_eq!(
        mesh.get_network(&NetId::global())
            .unwrap()
            .controller_interface_mac(),
        MAC1
    );
    assert!(mesh.get_data_model(&node_id(NetId::GLOBAL, MAC2)).is_none());

    // A matching controller MAC, a zero list MAC and another network id are no mismatch.
    let mut matching = MeshModel::new(Some(MAC1));
    let mut record = zeroed_network();
    record.ctrl_id.mac = MAC1;
    matching.put_network(NetId::global(), record).unwrap();
    assert!(
        !matching
            .get_data_model(&controller_node)
            .unwrap()
            .dirty()
            .any()
    );

    let mut zero = MeshModel::new(Some(MacAddr::ZERO));
    let mut record = zeroed_network();
    record.ctrl_id.mac = MAC2;
    zero.put_network(NetId::global(), record).unwrap();
    assert!(
        !zero
            .get_data_model(&node_id(NetId::GLOBAL, MAC2))
            .unwrap()
            .dirty()
            .any()
    );

    let mut other = MeshModel::new(Some(MAC1));
    let mut record = zeroed_network();
    record.ctrl_id.mac = MAC2;
    other.put_network(net("NetA"), record).unwrap();
    assert!(
        !other
            .get_data_model(&node_id("NetA", MAC2))
            .unwrap()
            .dirty()
            .any()
    );
}

/// A colocated agent created in a known network writes the colocated agent MAC into the one
/// network record and marks the controller's node dirty (`dm_easy_mesh_list.cpp:1736-1747`,
/// plan C5, U1); nothing is written for a non-colocated agent or an unknown network.
#[test]
fn creating_a_colocated_agent_records_its_mac_and_marks_the_controller_node() {
    let mut mesh = MeshModel::new(None);
    let mut record = zeroed_network();
    record.ctrl_id.mac = MAC1;
    mesh.put_network(NetId::global(), record).unwrap();
    let ctrl = node_id(NetId::GLOBAL, MAC1);

    mesh.create_data_model(agent(NetId::GLOBAL, MAC2)).unwrap();
    assert!(
        mesh.get_network(&NetId::global())
            .unwrap()
            .colocated_agent_interface()
            .mac
            .is_zero()
    );
    assert!(!mesh.get_data_model(&ctrl).unwrap().dirty().any());

    let mut colocated = agent(NetId::GLOBAL, MAC3);
    colocated.colocated = true;
    mesh.create_data_model(colocated).unwrap();
    let network = mesh.get_network(&NetId::global()).unwrap();
    assert_eq!(network.colocated_agent_interface().mac, MAC3);
    assert!(
        mesh.get_data_model(&ctrl)
            .unwrap()
            .dirty()
            .is_set(DbCfgType::NetworkListUpdate)
    );
    assert!(
        mesh.get_data_model(&node_id(NetId::GLOBAL, MAC3))
            .unwrap()
            .is_colocated()
    );

    let mut unknown = agent("NetA", MAC4);
    unknown.colocated = true;
    mesh.create_data_model(unknown).unwrap();
    assert!(
        mesh.get_network(&net("NetA"))
            .unwrap()
            .colocated_agent_interface()
            .mac
            .is_zero()
    );
}

/// Matching is exact (plan C4): `Network1` does not find `Network10`, in the network and node
/// lookups. Upstream compares a prefix (`dm_easy_mesh_list.cpp:114`, research 7.6).
/// intentional-difference:c4-exact-keys
#[test]
fn net_id_matching_is_exact_not_a_prefix() {
    let mesh = mesh_with_nodes(&[("Network10", MAC1)]);
    assert!(mesh.get_network(&net("Network1")).is_none());
    assert!(mesh.get_network(&net("Network10")).is_some());
    assert!(mesh.get_data_model(&node_id("Network1", MAC1)).is_none());
    assert!(mesh.get_device(&node_id("Network1", MAC1)).is_none());
    assert!(mesh.node(&node_id("Network1", MAC1)).is_none());
    assert!(mesh.get_data_model(&node_id("Network10", MAC1)).is_some());
}

/// Networks iterate in key order, which replaces `m_network_list`'s insertion order (plan C3).
/// intentional-difference:c3-key-order
#[test]
fn networks_iterate_in_key_order() {
    let mesh = mesh_with_nodes(&[("NetC", MAC1), ("NetA", MAC1), ("NetB", MAC1)]);
    let first = mesh.first_network().unwrap();
    assert_eq!(first.net(), net("NetA"));
    let second = mesh.next_network(&first.net()).unwrap();
    assert_eq!(second.net(), net("NetB"));
    let third = mesh.next_network(&second.net()).unwrap();
    assert_eq!(third.net(), net("NetC"));
    assert!(mesh.next_network(&third.net()).is_none());
    assert!(mesh.next_network(&net("NetZ")).is_none());
}

/// `create_data_model` replaces a node with the same id, as `hash_map_put` does.
#[test]
fn creating_a_node_with_an_existing_id_replaces_it() {
    let mut mesh = mesh_with_nodes(&[("NetA", MAC1)]);
    mesh.data_model_mut(&node_id("NetA", MAC1))
        .unwrap()
        .set_msg_id(5);
    assert_eq!(
        mesh.create_data_model(agent("NetA", MAC1)),
        Ok(Put::Replaced)
    );
    assert_eq!(
        mesh.get_data_model(&node_id("NetA", MAC1))
            .unwrap()
            .msg_id(),
        0
    );
    assert_eq!(mesh.num_nodes(), 1);
}

// --- put_device and the device count ---

/// Plan A4: `put_device` counts distinct devices. Upstream does `num_of_devices++` on every call
/// (`dm_easy_mesh_list.cpp:263`, research 7.22). intentional-difference:a4-device-count
#[test]
fn put_device_counts_distinct_devices_not_calls() {
    let mut mesh = MeshModel::new(None);
    let count = |mesh: &MeshModel| {
        mesh.get_network(&net("NetA"))
            .unwrap()
            .info()
            .num_of_devices
    };
    let key1 = device_id("NetA", MAC1, MediaType(0));
    assert_eq!(
        mesh.put_device(key1, device_in("NetA", MAC1), false),
        Ok(Put::Inserted)
    );
    assert_eq!(count(&mesh), 1);
    for _ in 0..3 {
        let key = device_id("NetA", MAC1, MediaType(0));
        assert_eq!(
            mesh.put_device(key, device_in("NetA", MAC1), false),
            Ok(Put::Replaced)
        );
    }
    assert_eq!(count(&mesh), 1);
    let key2 = device_id("NetA", MAC2, MediaType(0));
    mesh.put_device(key2, device_in("NetA", MAC2), false)
        .unwrap();
    assert_eq!(count(&mesh), 2);
}

/// `put_device` creates an agent node from the device when there is none
/// (`dm_easy_mesh_list.cpp:250`): the AL MAC, network id and profile come from the device, and
/// `colocated` is the caller's host lookup.
#[test]
fn put_device_creates_an_agent_node_from_the_device() {
    let mut mesh = MeshModel::new(None);
    let mut device = device_in("NetA", MAC1);
    device.profile = ProfileType::MULTI_AP_PROFILE_2;
    let key = device_id("NetA", MAC1, MediaType(0));
    assert_eq!(mesh.put_device(key, device, true), Ok(Put::Inserted));
    let node = mesh.get_data_model(&node_id("NetA", MAC1)).unwrap();
    assert!(!node.is_controller());
    assert!(node.is_colocated());
    assert_eq!(node.device().profile, ProfileType::MULTI_AP_PROFILE_2);
    assert_eq!(node.device().id, key);
}

/// A controller node with the device's MAC takes the device and the key's `dev_mac` becomes the
/// node's AL MAC (`dm_easy_mesh_list.cpp:241-246`).
#[test]
fn put_device_reuses_the_controller_node() {
    let mut mesh = MeshModel::new(None);
    mesh.create_data_model(controller("NetA", MAC1)).unwrap();
    let key = device_id("NetA", MAC2, MediaType(0));
    let put = mesh.put_device(key, device_in("NetA", MAC1), false);
    assert_eq!(put, Ok(Put::Replaced));
    assert_eq!(mesh.num_nodes(), 1);
    assert_eq!(
        mesh.get_device(&node_id("NetA", MAC1)).unwrap().id.dev_mac,
        MAC1
    );
}

// --- ModelAccess (C8) ---

/// `node_ctx` gives the node and its network as split borrows of the mesh's two maps.
#[test]
fn node_ctx_borrows_the_node_and_its_network_together() {
    let mut mesh = mesh_with_nodes(&[("NetA", MAC1), ("NetB", MAC1)]);
    let ctx = mesh.node_ctx(&node_id("NetA", MAC1)).unwrap();
    ctx.node.set_msg_id(11);
    ctx.network.info_mut().num_of_devices = 4;
    assert_eq!(
        mesh.get_data_model(&node_id("NetA", MAC1))
            .unwrap()
            .msg_id(),
        11
    );
    assert_eq!(
        mesh.get_network(&net("NetA"))
            .unwrap()
            .info()
            .num_of_devices,
        4
    );
    assert_eq!(
        mesh.get_network(&net("NetB"))
            .unwrap()
            .info()
            .num_of_devices,
        0
    );
    assert!(mesh.node_ctx(&node_id("NetA", MAC3)).is_none());
}

/// Plan A2, C8: a handler reads another node's data by re-borrowing between two uses of its own
/// context, here through the trait object a handler would hold.
#[test]
fn a_cross_node_read_reborrows_between_two_context_uses() {
    fn handler(model: &mut dyn ModelAccess, own: &NodeId, other: &NodeId) -> (u16, u16) {
        model.node_ctx(own).unwrap().node.set_msg_id(7);
        let seen = model.node(other).unwrap().node.msg_id();
        let ctx = model.node_ctx(own).unwrap();
        ctx.node.set_msg_id(ctx.node.msg_id() + seen);
        (seen, ctx.node.msg_id())
    }
    let mut mesh = mesh_with_nodes(&[("NetA", MAC1), ("NetA", MAC2)]);
    mesh.data_model_mut(&node_id("NetA", MAC2))
        .unwrap()
        .set_msg_id(30);
    let (seen, own) = handler(&mut mesh, &node_id("NetA", MAC1), &node_id("NetA", MAC2));
    assert_eq!((seen, own), (30, 37));
}

/// `nodes()` iterates in key order, network first and then AL MAC.
#[test]
fn nodes_iterate_in_key_order() {
    let mesh = mesh_with_nodes(&[("NetB", MAC1), ("NetA", MAC2), ("NetA", MAC1)]);
    let order: Vec<NodeId> = mesh.nodes().map(|view| view.id()).collect();
    assert_eq!(
        order,
        vec![
            node_id("NetA", MAC1),
            node_id("NetA", MAC2),
            node_id("NetB", MAC1)
        ]
    );
}

// --- C4: same MAC in two networks, global finds ---

/// Plan A9, "the same MAC in different networks": two nodes with one AL MAC are two nodes, each
/// with its own device, radios and policies, and a radio put in one network leaves the other's
/// radio of the same MAC alone. Upstream's global `get_radio(key)` finds the other network's
/// radio and overwrites it (`dm_easy_mesh_list.cpp:418, 428`, research 7.7).
/// intentional-difference:c4-same-mac-in-two-networks
#[test]
fn the_same_mac_in_different_networks_is_two_separate_nodes() {
    let mut mesh = mesh_with_nodes(&[("NetA", MAC1), ("NetB", MAC1)]);
    let radio_mac = mac(9);
    let mut in_a = radio_in("NetA", MAC1, radio_mac);
    in_a.noise = 11;
    let mut in_b = radio_in("NetB", MAC1, radio_mac);
    in_b.noise = 22;
    mesh.put_radio(in_a).unwrap();
    mesh.put_radio(in_b).unwrap();
    let noise = |network: &str| {
        mesh.get_data_model(&node_id(network, MAC1))
            .unwrap()
            .radio_by_mac(radio_mac)
            .unwrap()
            .noise
    };
    assert_eq!(noise("NetA"), 11);
    assert_eq!(noise("NetB"), 22);
    assert_eq!(
        mesh.get_data_model(&node_id("NetA", MAC1))
            .unwrap()
            .radios()
            .len(),
        1
    );

    mesh.update_device(&node_id("NetA", MAC1), {
        let mut device = device_in("NetA", MAC1);
        device.dfs_enable = true;
        device
    })
    .unwrap();
    assert!(mesh.get_device(&node_id("NetA", MAC1)).unwrap().dfs_enable);
    assert!(!mesh.get_device(&node_id("NetB", MAC1)).unwrap().dfs_enable);

    let key = policy_key("NetA", MAC1, radio_mac, PolicyIdType::QOS_MGT);
    mesh.put_policy(&key, policy_in(&key)).unwrap();
    assert!(mesh.get_policy(&key).is_some());
    let other = policy_key("NetB", MAC1, radio_mac, PolicyIdType::QOS_MGT);
    assert!(mesh.get_policy(&other).is_none());
}

/// Plan C4: the global find of a radio by MAC returns the first match in key order.
#[test]
fn get_radio_returns_the_first_match_in_key_order() {
    let mut mesh = mesh_with_nodes(&[("NetB", MAC1), ("NetA", MAC1)]);
    let radio_mac = mac(9);
    mesh.put_radio(radio_in("NetB", MAC1, radio_mac)).unwrap();
    mesh.put_radio(radio_in("NetA", MAC1, radio_mac)).unwrap();
    let found = mesh.get_radio(radio_mac).unwrap();
    assert_eq!(found.at, node_id("NetA", MAC1));
    // Removing it exposes the next one, in key order.
    assert!(mesh.remove_radio(radio_mac).is_some());
    assert_eq!(mesh.get_radio(radio_mac).unwrap().at, node_id("NetB", MAC1));
}

/// The global finds of a station and of an op class go to the first node that owns the radio, in
/// key order (plan C4).
#[test]
fn sta_and_op_class_finds_go_to_the_first_node_that_owns_the_radio() {
    let mut mesh = mesh_with_nodes(&[("NetB", MAC1), ("NetA", MAC2)]);
    let radio_mac = mac(9);
    mesh.put_radio(radio_in("NetB", MAC1, radio_mac)).unwrap();
    mesh.put_radio(radio_in("NetA", MAC2, radio_mac)).unwrap();

    mesh.put_sta(sta_on(mac(40), mac(41), radio_mac)).unwrap();
    let sta_key = StaKey {
        sta: mac(40),
        bssid: mac(41),
        radio: radio_mac,
    };
    assert_eq!(mesh.get_sta(&sta_key).unwrap().at, node_id("NetA", MAC2));
    assert_eq!(mesh.first_sta().unwrap().at, node_id("NetA", MAC2));

    mesh.put_op_class(fixtures::op_class(radio_mac, OpClassType::CURRENT, 81))
        .unwrap();
    let key = OpClassKey {
        ruid: radio_mac,
        kind: OpClassType::CURRENT,
        op_class: 81,
    };
    assert_eq!(mesh.get_op_class(&key).unwrap().at, node_id("NetA", MAC2));
}

/// The op class owner rules of `get_op_class` and `put_op_class`
/// (`dm_easy_mesh_list.cpp:1047-1113, 1177-1253`): radio-scoped types find the radio's node,
/// the other types the node whose AL MAC is the `ruid`, and the fallbacks differ by call.
#[test]
fn op_class_owners_follow_the_type_rules_of_get_and_put() {
    let mut mesh = mesh_with_nodes(&[("NetA", MAC1)]);
    let owner = node_id("NetA", MAC1);
    // `scan_param` is not radio-scoped: the owner is the node whose AL MAC is the ruid.
    let op = fixtures::op_class(MAC1, OpClassType::SCAN_PARAM, 200);
    assert_eq!(mesh.put_op_class(op), Ok(Put::Inserted));
    let key = OpClassKey {
        ruid: MAC1,
        kind: OpClassType::SCAN_PARAM,
        op_class: 200,
    };
    assert_eq!(mesh.get_op_class(&key).unwrap().at, owner);
    // `current` is radio-scoped and the node has no such radio.
    let current = fixtures::op_class(MAC1, OpClassType::CURRENT, 81);
    assert_eq!(mesh.put_op_class(current), Err(not_found(Table::Node)));
    // The broadcast `ruid` is accepted for `anticipated` by both calls.
    let anticipated = fixtures::op_class(MacAddr::BROADCAST, OpClassType::ANTICIPATED, 90);
    assert_eq!(mesh.put_op_class(anticipated), Ok(Put::Inserted));
    // `put` accepts the device MAC for the types from `cac_available` on, `preference` among
    // them; `get` accepts it only for `anticipated`.
    let preference = fixtures::op_class(MAC1, OpClassType::PREFERENCE, 91);
    assert_eq!(mesh.put_op_class(preference), Ok(Put::Inserted));
    let preference_key = OpClassKey {
        ruid: MAC1,
        kind: OpClassType::PREFERENCE,
        op_class: 91,
    };
    assert!(mesh.get_op_class(&preference_key).is_none());
    let preset = OpClassKey {
        ruid: MAC1,
        kind: OpClassType::ANTICIPATED,
        op_class: 83,
    };
    assert_eq!(mesh.get_op_class(&preset).unwrap().at, owner);
    let active = fixtures::op_class(MAC1, OpClassType::CAC_ACTIVE, 120);
    assert_eq!(mesh.put_op_class(active), Ok(Put::Inserted));
}

/// `put_op_class` of an op class no node owns: upstream returns 0, success, and stores nothing
/// (`dm_easy_mesh_list.cpp:1224-1226`, research 7.17); the port reports `NotFound`.
/// intentional-difference:c4-global-finds
#[test]
fn put_op_class_without_an_owner_is_not_found() {
    let mut mesh = mesh_with_nodes(&[("NetA", MAC1)]);
    let op = fixtures::op_class(mac(77), OpClassType::CURRENT, 81);
    assert_eq!(mesh.put_op_class(op), Err(not_found(Table::Node)));
    assert_eq!(
        mesh.get_data_model(&node_id("NetA", MAC1))
            .unwrap()
            .op_classes()
            .len(),
        6
    );
}

// --- plan A9: capacity versus update ---

/// Plan A9, "insert at capacity versus update of an existing row", for the mesh's tables: an
/// insert at capacity is `Full` and changes nothing; an update of a stored row succeeds. Upstream
/// writes past the array in `put_bss` and `put_network_ssid` (0007 defect 1).
/// intentional-difference:c6-capacity
#[test]
fn insert_at_capacity_fails_and_updates_succeed_in_every_mesh_table() {
    let mut mesh = mesh_with_nodes(&[("NetA", MAC1)]);
    let id = node_id("NetA", MAC1);

    // Radios: 3.
    for last in 1..=3 {
        mesh.put_radio(radio_in("NetA", MAC1, mac(last))).unwrap();
    }
    let policies = mesh.get_data_model(&id).unwrap().num_policies();
    assert_eq!(
        mesh.put_radio(radio_in("NetA", MAC1, mac(4))).err(),
        Some(full(Table::Radio, 3))
    );
    assert_eq!(mesh.get_data_model(&id).unwrap().radios().len(), 3);
    assert_eq!(mesh.get_data_model(&id).unwrap().num_policies(), policies);
    let mut update = radio_in("NetA", MAC1, mac(2));
    update.noise = 5;
    assert_eq!(mesh.put_radio(update).unwrap().put.put, Put::Replaced);

    // BSS: 24.
    for last in 0..24 {
        let bss = bss_in("NetA", MAC1, mac(1), mac2_of(last), HaulType::FRONTHAUL);
        assert_eq!(mesh.put_bss(bss), Ok(Put::Inserted));
    }
    let extra = bss_in("NetA", MAC1, mac(1), mac2_of(24), HaulType::FRONTHAUL);
    assert_eq!(mesh.put_bss(extra), Err(full(Table::Bss, 24)));
    let again = bss_in("NetA", MAC1, mac(1), mac2_of(3), HaulType::FRONTHAUL);
    assert_eq!(mesh.put_bss(again), Ok(Put::Replaced));
    assert_eq!(mesh.get_data_model(&id).unwrap().bss_list().len(), 24);

    // Op classes: 64, 6 of them the presets.
    for number in 1..=58 {
        let op = fixtures::op_class(mac(1), OpClassType::CURRENT, number);
        assert_eq!(mesh.put_op_class(op), Ok(Put::Inserted));
    }
    let over = fixtures::op_class(mac(1), OpClassType::CURRENT, 59);
    assert_eq!(mesh.put_op_class(over), Err(full(Table::OpClass, 64)));
    let same = fixtures::op_class(mac(1), OpClassType::CURRENT, 10);
    assert_eq!(mesh.put_op_class(same), Ok(Put::Replaced));
    assert_eq!(mesh.get_data_model(&id).unwrap().op_classes().len(), 64);

    // Network SSIDs: 5.
    for index in 0..5 {
        let ssid = fixtures::network_ssid(&format!("id{index}"), "s", &[]);
        assert_eq!(mesh.put_network_ssid(&net("NetA"), ssid), Ok(Put::Inserted));
    }
    let sixth = fixtures::network_ssid("id5", "s", &[]);
    assert_eq!(
        mesh.put_network_ssid(&net("NetA"), sixth),
        Err(full(Table::NetworkSsid, 5))
    );
    let replace = fixtures::network_ssid("id2", "changed", &[]);
    assert_eq!(
        mesh.put_network_ssid(&net("NetA"), replace),
        Ok(Put::Replaced)
    );
    assert_eq!(
        mesh.get_network(&net("NetA")).unwrap().num_network_ssids(),
        5
    );
}

fn mac2_of(index: u8) -> MacAddr {
    super::fixtures::mac2(1, index)
}

// --- plan A9: failed multi-row puts ---

fn station_scan_fixture() -> (MeshModel, ScanKey) {
    let mut mesh = mesh_with_nodes(&[("NetA", MAC1)]);
    mesh.put_radio(radio_in("NetA", MAC1, mac(1))).unwrap();
    mesh.put_sta(sta_on(mac(40), mac(41), mac(1))).unwrap();
    let key = scan_key_in("NetA", MAC1, mac(40), 81, 6, ScannerType::STA);
    (mesh, key)
}

fn reports_of(mesh: &MeshModel, sta_key: &StaKey) -> usize {
    let node = mesh.get_data_model(&node_id("NetA", MAC1)).unwrap();
    node.sta_info(sta_key, StaMapTarget::Consolidated)
        .unwrap()
        .beacon_reports
        .len()
}

fn sta_key_40() -> StaKey {
    StaKey {
        sta: mac(40),
        bssid: mac(41),
        radio: mac(1),
    }
}

/// Plan A9, failed batches under the C1 row rule, for `put_scan_result`: with 16 neighbors a new
/// BSSID is refused (upstream returns, `:1576-1578`) before any write: the scan result and the
/// station's beacon reports are unchanged and the error names the table. A known BSSID still
/// updates.
#[test]
fn put_scan_result_at_the_neighbor_capacity_fails_and_changes_nothing() {
    let (mut mesh, key) = station_scan_fixture();
    let mut first = scan_in(&key);
    for last in 0..16 {
        first.neighbor.push(neighbor(mac2_of(last))).unwrap();
    }
    mesh.put_scan_result(MacAddr::BROADCAST, first, 0).unwrap();
    let reports = reports_of(&mesh, &sta_key_40());

    let mut incoming = scan_in(&key);
    incoming.neighbor.push(neighbor(mac2_of(99))).unwrap();
    let result = mesh.put_scan_result(mac2_of(99), incoming, 0);
    assert_eq!(result, Err(full(Table::Neighbor, 16)));
    assert_eq!(mesh.get_scan_result(&key).unwrap().neighbor.len(), 16);
    assert!(
        mesh.get_scan_result(&key)
            .unwrap()
            .neighbor
            .iter()
            .all(|n| n.bssid != mac2_of(99))
    );
    assert_eq!(reports_of(&mesh, &sta_key_40()), reports);

    let mut update = scan_in(&key);
    let mut changed = neighbor(mac2_of(3));
    changed.signal_strength = -40;
    update.neighbor.push(changed).unwrap();
    assert_eq!(
        mesh.put_scan_result(mac2_of(3), update, 0),
        Ok(Put::Replaced)
    );
    let stored = mesh.get_scan_result(&key).unwrap();
    assert_eq!(
        stored
            .neighbor
            .iter()
            .find(|n| n.bssid == mac2_of(3))
            .unwrap()
            .signal_strength,
        -40
    );
}

/// A station with 10 beacon reports refuses another one before the scan result is written:
/// upstream writes past `beacon_reports` (`:1596-1604`). Nothing is stored, nothing is half done.
/// intentional-difference:c6-capacity
#[test]
fn put_scan_result_at_the_beacon_report_capacity_fails_and_stores_nothing() {
    let (mut mesh, key) = station_scan_fixture();
    let node = mesh.data_model_mut(&node_id("NetA", MAC1)).unwrap();
    let sta = node
        .sta_info_mut(&sta_key_40(), StaMapTarget::Consolidated)
        .unwrap();
    for _ in 0..10 {
        sta.beacon_reports.push(report(mac(60))).unwrap();
    }
    let mut incoming = scan_in(&key);
    incoming.neighbor.push(neighbor(mac(61))).unwrap();
    assert_eq!(
        mesh.put_scan_result(mac(61), incoming, 0),
        Err(full(Table::BeaconReport, 10))
    );
    assert!(mesh.get_scan_result(&key).is_none());
    assert_eq!(reports_of(&mesh, &sta_key_40()), 10);
}

fn report(bssid: MacAddr) -> BeaconReport {
    BeaconReport {
        op_class: 0,
        channel: 0,
        start_time: 0,
        duration: 0,
        frame_info: 0,
        rcpi: 0,
        rsni: 0,
        bssid,
        antenna: 0,
        tsf: 0,
        wide_bandwidth_channel_present: false,
        wide_bandwidth_channel: WideBwChannel {
            bandwidth: 0,
            center_seg0: 0,
            center_seg1: 0,
        },
        num_repetitions: 0,
    }
}

/// Plan A9, a decoded scan-result fixture: a station scan is inserted, then a second neighbor
/// appended and a known one updated, each beacon report is appended for the station, and
/// `remove_scan_result` removes the record and the report of the BSSID
/// (`dm_easy_mesh_list.cpp:1524-1611, 1453-1522`).
#[test]
fn a_decoded_scan_result_is_inserted_updated_and_removed_with_its_beacon_reports() {
    let (mut mesh, key) = station_scan_fixture();
    let mut decoded = scan_in(&key);
    decoded.neighbor.push(neighbor(mac(51))).unwrap();
    decoded.neighbor.push(neighbor(mac(52))).unwrap();
    assert_eq!(mesh.put_scan_result(mac(51), decoded, 0), Ok(Put::Inserted));
    // Plan A4 / research 7.13: the count is the neighbors stored, not one more.
    assert_eq!(mesh.get_scan_result(&key).unwrap().neighbor.len(), 2);
    assert_eq!(reports_of(&mesh, &sta_key_40()), 1);

    let mut append = scan_in(&key);
    append.neighbor.push(neighbor(mac(53))).unwrap();
    assert_eq!(mesh.put_scan_result(mac(53), append, 0), Ok(Put::Replaced));
    assert_eq!(mesh.get_scan_result(&key).unwrap().neighbor.len(), 3);
    let mut update = scan_in(&key);
    let mut louder = neighbor(mac(52));
    louder.signal_strength = -30;
    update.neighbor.push(louder).unwrap();
    mesh.put_scan_result(mac(52), update, 0).unwrap();
    assert_eq!(mesh.get_scan_result(&key).unwrap().neighbor.len(), 3);
    assert_eq!(reports_of(&mesh, &sta_key_40()), 3);

    // An index beyond the neighbors of the incoming result is an error and changes nothing.
    assert_eq!(
        mesh.put_scan_result(mac(52), scan_in(&key), 4),
        Err(not_found(Table::Neighbor))
    );
    assert_eq!(reports_of(&mesh, &sta_key_40()), 3);

    assert!(mesh.remove_scan_result(&key, mac(52)).is_some());
    assert!(mesh.get_scan_result(&key).is_none());
    assert_eq!(reports_of(&mesh, &sta_key_40()), 2);
    // The report is removed even though the scan result is gone, as upstream.
    assert!(mesh.remove_scan_result(&key, mac(53)).is_none());
    assert_eq!(reports_of(&mesh, &sta_key_40()), 1);
}

/// A radio scan appends no beacon report to any station (`dm_easy_mesh_list.cpp:1590-1592`).
#[test]
fn a_radio_scan_appends_no_beacon_report() {
    let (mut mesh, _) = station_scan_fixture();
    let radio_scan = scan_key_in("NetA", MAC1, mac(40), 81, 6, ScannerType::RADIO);
    mesh.put_scan_result(mac(51), scan_in(&radio_scan), 0)
        .unwrap();
    assert_eq!(reports_of(&mesh, &sta_key_40()), 0);
}

/// Plan A9, failed batch for `put_radio`: the radio lands, and the default policies that the full
/// policy table rejects are reported (`policies_rejected`), not lost silently
/// (`dm_easy_mesh.cpp:3411-3414`). The policy table keeps its 32 rows.
#[test]
fn put_radio_reports_the_default_policies_a_full_policy_table_rejects() {
    let mut mesh = mesh_with_nodes(&[("NetA", MAC1)]);
    let id = node_id("NetA", MAC1);
    for last in 0..24u8 {
        let key = policy_key("NetA", MAC1, mac(100 + last), PolicyIdType::QOS_MGT);
        mesh.put_policy(&key, policy_in(&key)).unwrap();
    }
    assert_eq!(mesh.get_data_model(&id).unwrap().num_policies(), 32);
    let outcome = mesh.put_radio(radio_in("NetA", MAC1, mac(1))).unwrap();
    assert_eq!(outcome.put.put, Put::Inserted);
    assert_eq!(outcome.put.policies_rejected, 2);
    assert_eq!(mesh.get_data_model(&id).unwrap().num_policies(), 32);
    assert!(mesh.get_radio(mac(1)).is_some());
}

/// Plan A6: `put_radio` returns what `create_node` receives, and creates no node
/// (`dm_easy_mesh_list.cpp:470`): the node, the radio's interface MAC, `media_data.band`, profile
/// 3 and the controller service.
#[test]
fn put_radio_returns_the_node_needed_and_creates_no_node() {
    let mut mesh = mesh_with_nodes(&[("NetA", MAC1)]);
    let mut radio = radio_in("NetA", MAC1, mac(1));
    radio.media_data.band = 3;
    radio.band = em_types::FreqBand::BAND_2_4_GHZ;
    let nodes = mesh.num_nodes();
    let outcome = mesh.put_radio(radio).unwrap();
    let needed = outcome.node_needed;
    assert_eq!(needed.node, node_id("NetA", MAC1));
    assert_eq!(needed.radio, mac(1));
    assert_eq!(needed.band, em_types::FreqBand::BAND_6_GHZ);
    assert_eq!(needed.profile, ProfileType::MULTI_AP_PROFILE_3);
    assert_eq!(needed.service, em_types::ServiceType::MULTI_AP_CONTROLLER);
    assert_eq!(mesh.num_nodes(), nodes);
    // The interface the caller reads is the stored radio's.
    let stored = mesh
        .get_data_model(&needed.node)
        .unwrap()
        .radio_by_mac(needed.radio)
        .unwrap();
    assert_eq!(stored.intf.mac, mac(1));
    // The seeded default policies are the node's.
    assert_eq!(
        mesh.get_data_model(&needed.node).unwrap().num_policies(),
        10
    );
}

/// A failed `put_radio` (no node, or no room) returns no `NodeNeeded`.
#[test]
fn a_failed_put_radio_needs_no_node() {
    let mut mesh = mesh_with_nodes(&[("NetA", MAC1)]);
    assert_eq!(
        mesh.put_radio(radio_in("NetZ", MAC1, mac(1))).err(),
        Some(not_found(Table::Node))
    );
    assert_eq!(
        mesh.put_radio(radio_in("NetA", MAC2, mac(1))).err(),
        Some(not_found(Table::Node))
    );
}

// --- STA and frame ownership ---

/// Plan A9: a station put through the mesh moves the boxed row in (no duplicate of the frame);
/// the frame limit holds: 11 426 bytes are stored, 11 427 are refused and the old frame stays
/// (0007 D4).
#[test]
fn a_station_put_moves_the_frame_in_and_the_limit_is_11426() {
    let mut mesh = mesh_with_nodes(&[("NetA", MAC1)]);
    mesh.put_radio(radio_in("NetA", MAC1, mac(1))).unwrap();
    let mut row = sta_on(mac(40), mac(41), mac(1));
    row.set_frame_body(vec![7; 11_426]).unwrap();
    assert_eq!(mesh.put_sta(row), Ok(Put::Inserted));
    let found = mesh.get_sta(&sta_key_40()).unwrap();
    assert_eq!(found.item.frame_body.len(), 11_426);

    let ctx = mesh.node_ctx(&node_id("NetA", MAC1)).unwrap();
    let stored = ctx
        .node
        .sta_info_mut(&sta_key_40(), StaMapTarget::Consolidated)
        .unwrap();
    assert!(stored.set_frame_body(vec![8; 11_427]).is_err());
    assert_eq!(stored.frame_body.len(), 11_426);
    assert!(stored.set_frame_body(vec![9; 11_426]).is_ok());
}

// --- network SSIDs: visibility and the late join ---

/// Plan A9, "network-SSID updates visible to all matching nodes": one put is visible to every
/// node of the network, and to no node of another network.
#[test]
fn a_network_ssid_update_is_visible_to_every_node_of_the_network() {
    let mut mesh = mesh_with_nodes(&[
        ("NetA", MAC1),
        ("NetA", MAC2),
        ("NetA", MAC3),
        ("NetB", MAC1),
    ]);
    mesh.put_network_ssid(
        &net("NetA"),
        fixtures::network_ssid("Front@NetA", "one", &[]),
    )
    .unwrap();
    mesh.put_network_ssid(
        &net("NetA"),
        fixtures::network_ssid("Front@NetA", "two", &[]),
    )
    .unwrap();
    for al in [MAC1, MAC2, MAC3] {
        let view = mesh.node(&node_id("NetA", al)).unwrap();
        assert_eq!(view.network.num_network_ssids(), 1);
        assert_eq!(
            view.network.network_ssid(0).unwrap().ssid.as_bytes(),
            b"two"
        );
    }
    assert_eq!(
        mesh.node(&node_id("NetB", MAC1))
            .unwrap()
            .network
            .num_network_ssids(),
        0
    );
}

/// Plan A9, the late-join scenario (0007 D2): an SSID in network A, then a late node in A and a
/// node of B with the same AL MAC as an A node and a different config. The late A node sees A's
/// SSID with no copy made for it; the B node sees only B's. After A's SSID is removed, A sees
/// none and B's config is still visible. Upstream copies the SSIDs into each node at creation
/// (`dm_easy_mesh_list.cpp:1786-1801`). intentional-difference:u1-network-stored-once
#[test]
fn the_late_join_scenario_sees_the_network_ssid_without_a_copy() {
    let mut mesh = mesh_with_nodes(&[("NetA", MAC1)]);
    mesh.put_network_ssid(
        &net("NetA"),
        fixtures::network_ssid("Front@NetA", "ssid-a", &[]),
    )
    .unwrap();

    mesh.create_data_model(agent("NetA", MAC2)).unwrap();
    mesh.create_data_model(agent("NetB", MAC1)).unwrap();
    mesh.put_network_ssid(
        &net("NetB"),
        fixtures::network_ssid("Front@NetB", "ssid-b", &[]),
    )
    .unwrap();

    let early = mesh.node(&node_id("NetA", MAC1)).unwrap();
    let late = mesh.node(&node_id("NetA", MAC2)).unwrap();
    assert!(
        std::ptr::eq(early.network, late.network),
        "one table, no per-node copy"
    );
    assert_eq!(late.network.num_network_ssids(), 1);
    assert_eq!(
        late.network.network_ssid(0).unwrap().ssid.as_bytes(),
        b"ssid-a"
    );
    let other = mesh.node(&node_id("NetB", MAC1)).unwrap();
    assert_eq!(other.network.num_network_ssids(), 1);
    assert_eq!(
        other.network.network_ssid(0).unwrap().ssid.as_bytes(),
        b"ssid-b"
    );

    assert!(
        mesh.remove_network_ssid(&net("NetA"), &ssid_key("Front@NetA"))
            .is_some()
    );
    for al in [MAC1, MAC2] {
        let view = mesh.node(&node_id("NetA", al)).unwrap();
        assert_eq!(view.network.num_network_ssids(), 0);
    }
    let other = mesh.node(&node_id("NetB", MAC1)).unwrap();
    assert_eq!(
        other.network.network_ssid(0).unwrap().ssid.as_bytes(),
        b"ssid-b"
    );
}

/// `remove_network_ssid` matches the whole id (plan A4): `Front@Net` does not remove
/// `Front@Net10`. Upstream compares a prefix (`dm_easy_mesh_list.cpp:920`, research 7.5).
/// intentional-difference:a4-ssid-identity
#[test]
fn remove_network_ssid_matches_the_whole_id() {
    let mut mesh = mesh_with_nodes(&[("NetA", MAC1)]);
    mesh.put_network_ssid(
        &net("NetA"),
        fixtures::network_ssid("Front@Net10", "s", &[]),
    )
    .unwrap();
    assert!(
        mesh.remove_network_ssid(&net("NetA"), &ssid_key("Front@Net"))
            .is_none()
    );
    assert_eq!(
        mesh.get_network(&net("NetA")).unwrap().num_network_ssids(),
        1
    );
    assert!(mesh.get_network_ssid(&ssid_key("Front@Net10")).is_some());
    assert!(mesh.get_network_ssid(&ssid_key("Front@Net")).is_none());
}

/// `put_network_ssid` of a network with no model is `NotFound`; upstream stores nothing and says
/// nothing (`dm_easy_mesh_list.cpp:958-986`).
#[test]
fn put_network_ssid_without_a_network_is_not_found() {
    let mut mesh = MeshModel::new(None);
    let result = mesh.put_network_ssid(&net("NetA"), fixtures::network_ssid("a", "s", &[]));
    assert_eq!(result, Err(not_found(Table::Network)));
}

/// `get_first_network_ssid` and `get_next_network_ssid` walk the networks in key order and each
/// network's SSIDs in insertion (array) order, as upstream's array walk does
/// (`dm_easy_mesh_list.cpp:812-867`). Upstream returns the last node's slot 0 as the first row
/// because its loop has no `break` (`:812-827`); the port starts at the first network (plan C3,
/// amended by D1a for the order inside a network). intentional-difference:c3-key-order
#[test]
fn network_ssids_are_walked_in_array_order_within_a_network() {
    let mut mesh = mesh_with_nodes(&[("NetA", MAC1), ("NetB", MAC1), ("NetC", MAC1)]);
    for (network, id) in [("NetB", "z"), ("NetB", "m"), ("NetA", "q"), ("NetC", "a")] {
        mesh.put_network_ssid(&net(network), fixtures::network_ssid(id, "s", &[]))
            .unwrap();
    }
    let mut seen = Vec::new();
    let mut cursor = mesh.first_network_ssid();
    while let Some(found) = cursor {
        seen.push((found.at, found.index, found.item.id.as_bytes().to_vec()));
        cursor = mesh.next_network_ssid(&found.at, found.index);
    }
    let expected: Vec<(NetId, usize, Vec<u8>)> = [
        ("NetA", 0, "q"),
        ("NetB", 0, "z"),
        ("NetB", 1, "m"),
        ("NetC", 0, "a"),
    ]
    .iter()
    .map(|(network, index, id)| (net(network), *index, id.as_bytes().to_vec()))
    .collect();
    assert_eq!(seen, expected);
    assert!(mesh.next_network_ssid(&net("NetA"), 9).is_none());
    assert!(mesh.next_network_ssid(&net("Absent"), 0).is_none());
}

// --- U2 removals not covered by the ported tests ---

/// `remove_bss` matches the whole key: upstream matches the BSSID alone (`:640-643`), so a BSS
/// on another radio or haul with the same BSSID would go. intentional-difference:c4-exact-keys
#[test]
fn remove_bss_matches_the_whole_key() {
    let mut mesh = mesh_with_nodes(&[("NetA", MAC1)]);
    mesh.put_bss(bss_in("NetA", MAC1, mac(1), mac(7), HaulType::FRONTHAUL))
        .unwrap();
    mesh.put_bss(bss_in("NetA", MAC1, mac(2), mac(7), HaulType::BACKHAUL))
        .unwrap();
    let id = node_id("NetA", MAC1);
    assert!(
        mesh.remove_bss(&id, &bss_key(mac(2), mac(7), HaulType::BACKHAUL))
            .is_some()
    );
    assert!(
        mesh.get_bss(&id, &bss_key(mac(1), mac(7), HaulType::FRONTHAUL))
            .is_some()
    );
    assert_eq!(mesh.get_data_model(&id).unwrap().bss_list().len(), 1);
}

/// `remove_scan_result` removes the whole record by its key, whatever the neighbor BSSID of the
/// upstream key (`dm_easy_mesh_list.cpp:1478-1482`).
#[test]
fn remove_scan_result_removes_the_whole_record() {
    let mut mesh = mesh_with_nodes(&[("NetA", MAC1)]);
    let key = scan_key_in("NetA", MAC1, mac(40), 81, 6, ScannerType::RADIO);
    let mut result = scan_in(&key);
    result.neighbor.push(neighbor(mac(51))).unwrap();
    result.neighbor.push(neighbor(mac(52))).unwrap();
    mesh.put_scan_result(mac(51), result, 0).unwrap();
    let removed = mesh.remove_scan_result(&key, mac(51)).unwrap();
    assert_eq!(removed.neighbor.len(), 2);
    assert!(mesh.get_scan_result(&key).is_none());
}

/// Removing a radio removes its capability record too, and only the first node's radio in key
/// order.
#[test]
fn remove_radio_takes_the_radio_of_the_first_node_with_that_mac() {
    let mut mesh = mesh_with_nodes(&[("NetA", MAC1), ("NetB", MAC1)]);
    for network in ["NetA", "NetB"] {
        mesh.put_radio(radio_in(network, MAC1, mac(9))).unwrap();
    }
    assert!(mesh.remove_radio(mac(9)).is_some());
    let first_radios = mesh
        .get_data_model(&node_id("NetA", MAC1))
        .unwrap()
        .radios();
    assert_eq!(first_radios.len(), 0);
    assert_eq!(
        mesh.get_data_model(&node_id("NetB", MAC1))
            .unwrap()
            .radios()
            .len(),
        1
    );
}

// --- walks ---

/// The walks cross node boundaries in key order and a cursor that is not a stored row returns
/// `None`, as an unmatched pointer does upstream.
#[test]
fn walks_cross_nodes_in_key_order_and_reject_unknown_cursors() {
    let mut mesh = mesh_with_nodes(&[("NetB", MAC1), ("NetA", MAC1)]);
    for network in ["NetB", "NetA"] {
        mesh.put_radio(radio_in(network, MAC1, mac(9))).unwrap();
        mesh.put_bss(bss_in(network, MAC1, mac(9), mac(20), HaulType::FRONTHAUL))
            .unwrap();
    }
    mesh.put_radio(radio_in("NetA", MAC1, mac(8))).unwrap();
    let id_a = node_id("NetA", MAC1);
    let id_b = node_id("NetB", MAC1);

    let first = mesh.first_radio().unwrap();
    assert_eq!((first.at, first.item.intf.mac), (id_a, mac(9)));
    let second = mesh.next_radio(&first.at, first.index).unwrap();
    assert_eq!((second.at, second.item.intf.mac), (id_a, mac(8)));
    let third = mesh.next_radio(&second.at, second.index).unwrap();
    assert_eq!((third.at, third.item.intf.mac), (id_b, mac(9)));
    assert!(mesh.next_radio(&third.at, third.index).is_none());
    assert!(mesh.next_radio(&id_a, 77).is_none());

    let first_bss = mesh.first_bss().unwrap();
    assert_eq!((first_bss.at, first_bss.index), (id_a, 0));
    let next_bss = mesh.next_bss(&id_a, 0).unwrap();
    assert_eq!((next_bss.at, next_bss.index), (id_b, 0));
    assert!(mesh.next_bss(&id_b, 0).is_none());
    assert!(mesh.next_bss(&id_a, 5).is_none());
}

/// `get_first_bss(al)` and `get_next_bss(al, bss)` use the global network, as upstream hard-codes
/// `GLOBAL_NET_ID` (`dm_easy_mesh_list.cpp:592, 606`).
#[test]
fn the_per_agent_bss_walk_uses_the_global_network() {
    let mut mesh = mesh_with_nodes(&[(NetId::GLOBAL, MAC1), ("NetA", MAC1)]);
    mesh.put_bss(bss_in(
        NetId::GLOBAL,
        MAC1,
        mac(9),
        mac(20),
        HaulType::FRONTHAUL,
    ))
    .unwrap();
    mesh.put_bss(bss_in(
        NetId::GLOBAL,
        MAC1,
        mac(9),
        mac(21),
        HaulType::FRONTHAUL,
    ))
    .unwrap();
    mesh.put_bss(bss_in("NetA", MAC1, mac(9), mac(22), HaulType::FRONTHAUL))
        .unwrap();
    let first = mesh.first_bss_of_agent(MAC1).unwrap();
    assert_eq!(first.item.bssid.mac, mac(20));
    let next = mesh.next_bss_of_agent(MAC1, first.index).unwrap();
    assert_eq!(next.item.bssid.mac, mac(21));
    assert!(mesh.next_bss_of_agent(MAC1, next.index).is_none());
    assert!(mesh.next_bss_of_agent(MAC1, 99).is_none());
    assert!(mesh.first_bss_of_agent(MAC2).is_none());
}

/// The pre-set op class walk reads the first node only; for `anticipated` it skips the radio-level
/// op classes (`dm_easy_mesh_list.cpp:1115-1170`).
#[test]
fn the_pre_set_op_class_walk_reads_the_first_node_and_skips_radio_level_anticipated() {
    let mut mesh = mesh_with_nodes(&[("NetA", MAC1), ("NetB", MAC1)]);
    mesh.put_radio(radio_in("NetA", MAC1, mac(9))).unwrap();
    let mut radio_level = fixtures::op_class(mac(9), OpClassType::ANTICIPATED, 70);
    radio_level.pref_valid = true;
    mesh.put_op_class(radio_level).unwrap();
    let mut seen = Vec::new();
    let mut cursor = mesh.first_pre_set_op_class_by_type(OpClassType::ANTICIPATED);
    while let Some(row) = cursor {
        seen.push(row.item.op_class);
        cursor = mesh.next_pre_set_op_class_by_type(OpClassType::ANTICIPATED, row.index);
    }
    assert_eq!(seen, vec![83, 128, 135]);
}

// --- side maps (C4) ---

/// The radio capability list: insert, update, delete by MAC, in MAC order, with `update_list`'s
/// unchecked update of a missing record reported as `NotFound`
/// (`dm_radio_cap_list.cpp:111-137`).
#[test]
fn the_radio_cap_list_is_keyed_by_mac_and_walks_in_mac_order() {
    let mut mesh = MeshModel::new(None);
    for last in [3, 1, 2] {
        mesh.update_radio_cap_list(ListOp::Insert, fixtures::radio_cap(mac(last)))
            .unwrap();
    }
    assert_eq!(mesh.first_radio_cap_entry().unwrap().ruid.mac, mac(1));
    assert_eq!(mesh.next_radio_cap_entry(mac(1)).unwrap().ruid.mac, mac(2));
    assert!(mesh.next_radio_cap_entry(mac(3)).is_none());
    assert!(mesh.next_radio_cap_entry(mac(9)).is_none());

    let mut changed = fixtures::radio_cap(mac(2));
    changed.ht_cap.ruid = mac(2);
    mesh.update_radio_cap_list(ListOp::Update, changed).unwrap();
    assert!(mesh.radio_cap_entry(mac(2)).is_some());
    assert_eq!(
        mesh.update_radio_cap_list(ListOp::Update, fixtures::radio_cap(mac(9))),
        Err(not_found(Table::RadioCapList))
    );
    mesh.update_radio_cap_list(ListOp::Delete, fixtures::radio_cap(mac(2)))
        .unwrap();
    assert!(mesh.radio_cap_entry(mac(2)).is_none());
    assert_eq!(mesh.delete_radio_cap_list(), 2);
    assert!(mesh.first_radio_cap_entry().is_none());
}

/// The list is global: the same MAC under two networks is one record (plan C4).
#[test]
fn the_side_maps_are_global_not_per_network() {
    let mut mesh = mesh_with_nodes(&[("NetA", MAC1), ("NetB", MAC1)]);
    mesh.update_radio_cap_list(ListOp::Insert, fixtures::radio_cap(mac(1)))
        .unwrap();
    mesh.update_radio_cap_list(ListOp::Insert, fixtures::radio_cap(mac(1)))
        .unwrap();
    assert_eq!(mesh.delete_radio_cap_list(), 1);
}

/// The IEEE 1905 security list: keyed by MAC, `update_list` insert/update/delete and `delete_list`
/// (`dm_ieee_1905_security_list.cpp:127-170`).
#[test]
fn the_ieee_1905_security_list_is_keyed_by_mac() {
    let mut mesh = MeshModel::new(None);
    let record = |last: u8, protocol: u8| crate::Ieee1905SecurityInfo {
        id: mac(last),
        sec_cap: crate::Ieee1905SecurityCap {
            onboarding_proto: protocol,
            integrity_algo: 0,
            encryption_algo: 0,
        },
    };
    mesh.update_ieee_1905_security_list(ListOp::Insert, record(2, 1))
        .unwrap();
    mesh.update_ieee_1905_security_list(ListOp::Insert, record(1, 1))
        .unwrap();
    assert_eq!(mesh.first_ieee_1905_security_entry().unwrap().id, mac(1));
    assert_eq!(
        mesh.next_ieee_1905_security_entry(mac(1)).unwrap().id,
        mac(2)
    );
    assert!(mesh.next_ieee_1905_security_entry(mac(2)).is_none());
    mesh.update_ieee_1905_security_list(ListOp::Update, record(2, 5))
        .unwrap();
    assert_eq!(
        mesh.ieee_1905_security_entry(mac(2))
            .unwrap()
            .sec_cap
            .onboarding_proto,
        5
    );
    assert_eq!(
        mesh.update_ieee_1905_security_list(ListOp::Update, record(9, 1)),
        Err(not_found(Table::Ieee1905SecurityList))
    );
    mesh.update_ieee_1905_security_list(ListOp::Delete, record(1, 0))
        .unwrap();
    assert!(mesh.ieee_1905_security_entry(mac(1)).is_none());
    assert_eq!(mesh.delete_ieee_1905_security_list(), 1);
}

/// The SSID-to-VID map is keyed by SSID and MAC: the same SSID under two MACs is two records.
/// `update_list` returns insert, unchanged or update. For a changed record upstream returns
/// `db_update` and leaves the stored record (`dm_ssid_2_vid_map.cpp:113-116`); the port stores
/// it. intentional-difference:ssid2vid-update-stores
#[test]
fn the_ssid_2_vid_map_is_keyed_by_ssid_and_mac_and_stores_updates() {
    let mut mesh = MeshModel::new(None);
    let record = |ssid: &str, vid: u16| crate::Ssid2VidMapInfo {
        id: crate::CStrBytes::new(ssid.as_bytes()).unwrap(),
        ssid: crate::CStrBytes::new(ssid.as_bytes()).unwrap(),
        vid,
    };
    assert_eq!(
        mesh.update_ssid_2_vid_map(mac(1), record("home", 10)),
        Ssid2VidChange::Inserted
    );
    assert_eq!(
        mesh.update_ssid_2_vid_map(mac(1), record("home", 10)),
        Ssid2VidChange::Unchanged
    );
    assert_eq!(
        mesh.update_ssid_2_vid_map(mac(2), record("home", 20)),
        Ssid2VidChange::Inserted
    );
    assert_eq!(
        mesh.update_ssid_2_vid_map(mac(1), record("home", 11)),
        Ssid2VidChange::Updated
    );
    let key = |last: u8| Ssid2VidKey {
        ssid: crate::CStrBytes::new(b"home").unwrap(),
        mac: mac(last),
    };
    assert_eq!(mesh.ssid_2_vid_entry(&key(1)).unwrap().vid, 11);
    assert_eq!(mesh.ssid_2_vid_entry(&key(2)).unwrap().vid, 20);
    let (first_key, first) = mesh.first_ssid_2_vid_entry().unwrap();
    assert_eq!((first_key, first.vid), (key(1), 11));
    let (second_key, _) = mesh.next_ssid_2_vid_entry(&first_key).unwrap();
    assert_eq!(second_key, key(2));
    assert!(mesh.next_ssid_2_vid_entry(&second_key).is_none());
    assert!(mesh.next_ssid_2_vid_entry(&key(9)).is_none());
}

// --- update_list and delete_list residue ---

fn populated_mesh() -> MeshModel {
    let mut mesh = mesh_with_nodes(&[("NetA", MAC1), ("NetA", MAC2)]);
    mesh.put_radio(radio_in("NetA", MAC1, mac(9))).unwrap();
    mesh.put_bss(bss_in("NetA", MAC1, mac(9), mac(20), HaulType::FRONTHAUL))
        .unwrap();
    mesh.put_sta(sta_on(mac(40), mac(20), mac(9))).unwrap();
    let key = scan_key_in("NetA", MAC1, mac(9), 81, 6, ScannerType::RADIO);
    let mut scan = scan_in(&key);
    scan.neighbor.push(neighbor(mac(51))).unwrap();
    mesh.put_scan_result(mac(51), scan, 0).unwrap();
    mesh.put_network_ssid(&net("NetA"), fixtures::network_ssid("a", "s", &[]))
        .unwrap();
    mesh
}

/// The `update_list` residue dispatches on the operation: insert puts, update merges with the
/// merge rules of plan A4, delete removes (the radio keeps `radio_temp`, the device keeps its
/// runtime metrics, a policy update of an unknown policy inserts, a BSS update replaces the BSS
/// info).
#[test]
fn update_list_merges_with_the_upstream_rules() {
    let mut mesh = populated_mesh();
    let id = node_id("NetA", MAC1);

    // Radio: update keeps radio_temp.
    mesh.data_model_mut(&id)
        .unwrap()
        .radio_by_mac_mut(mac(9))
        .unwrap()
        .radio_temp = 55;
    let mut radio = radio_in("NetA", MAC1, mac(9));
    radio.noise = 3;
    mesh.update_radio_list(ListOp::Update, radio).unwrap();
    let stored = mesh
        .get_data_model(&id)
        .unwrap()
        .radio_by_mac(mac(9))
        .unwrap();
    assert_eq!((stored.noise, stored.radio_temp), (3, 55));
    assert_eq!(
        mesh.update_radio_list(ListOp::Update, radio_in("NetA", MAC1, mac(77))),
        Err(not_found(Table::Radio))
    );

    // Device: update keeps the runtime metrics.
    mesh.data_model_mut(&id).unwrap().device_mut().cpu_load = 9;
    let mut device = device_in("NetA", MAC1);
    device.dfs_enable = true;
    mesh.update_device_list(ListOp::Update, device, false)
        .unwrap();
    let stored = mesh.get_device(&id).unwrap();
    assert!(stored.dfs_enable);
    assert_eq!(stored.cpu_load, 9);

    // Policy: update of an unknown policy falls back to insert.
    let key = policy_key("NetA", MAC1, mac(9), PolicyIdType::QOS_MGT);
    let before = mesh.get_data_model(&id).unwrap().num_policies();
    mesh.update_policy_list(ListOp::Update, policy_in(&key))
        .unwrap();
    assert_eq!(mesh.get_data_model(&id).unwrap().num_policies(), before + 1);
    mesh.update_policy_list(ListOp::Delete, policy_in(&key))
        .unwrap();
    assert_eq!(mesh.get_data_model(&id).unwrap().num_policies(), before);

    // BSS: update replaces the BSS info; a BSS that is not there is reported.
    let mut bss = bss_in("NetA", MAC1, mac(9), mac(20), HaulType::FRONTHAUL);
    bss.vap_index = 4;
    mesh.update_bss_list(ListOp::Update, bss).unwrap();
    let found = mesh
        .get_bss(&id, &bss_key(mac(9), mac(20), HaulType::FRONTHAUL))
        .unwrap();
    assert_eq!(found.vap_index, 4);
    let missing = bss_in("NetA", MAC1, mac(9), mac(21), HaulType::FRONTHAUL);
    assert_eq!(
        mesh.update_bss_list(ListOp::Update, missing),
        Err(not_found(Table::Bss))
    );
}

/// The same for the station, op class, scan result, SSID and network lists.
#[test]
fn update_list_merges_the_remaining_classes() {
    let mut mesh = populated_mesh();
    mesh.update_sta_list(ListOp::Update, sta_on(mac(40), mac(20), mac(9)))
        .unwrap();
    assert_eq!(
        mesh.update_sta_list(ListOp::Update, sta_on(mac(41), mac(20), mac(9))),
        Err(not_found(Table::Sta))
    );
    let op = fixtures::op_class(mac(9), OpClassType::CURRENT, 81);
    mesh.update_op_class_list(ListOp::Insert, op).unwrap();
    let mut changed = fixtures::op_class(mac(9), OpClassType::CURRENT, 81);
    changed.channel = 11;
    mesh.update_op_class_list(ListOp::Update, changed).unwrap();
    let key = OpClassKey {
        ruid: mac(9),
        kind: OpClassType::CURRENT,
        op_class: 81,
    };
    assert_eq!(mesh.get_op_class(&key).unwrap().item.channel, 11);
    mesh.update_op_class_list(
        ListOp::Delete,
        fixtures::op_class(mac(9), OpClassType::CURRENT, 81),
    )
    .unwrap();
    assert!(mesh.get_op_class(&key).is_none());

    let scan_key = scan_key_in("NetA", MAC1, mac(9), 81, 6, ScannerType::RADIO);
    let mut replaced = scan_in(&scan_key);
    replaced.util = 77;
    mesh.update_scan_result_list(ListOp::Update, replaced, SCAN_RESULT_SELF_INDEX)
        .unwrap();
    assert_eq!(mesh.get_scan_result(&scan_key).unwrap().util, 77);
    let unknown = scan_key_in("NetA", MAC1, mac(9), 81, 11, ScannerType::RADIO);
    assert_eq!(
        mesh.update_scan_result_list(ListOp::Update, scan_in(&unknown), SCAN_RESULT_SELF_INDEX),
        Err(not_found(Table::ScanResult))
    );

    mesh.update_network_ssid_list(
        ListOp::Update,
        &net("NetA"),
        fixtures::network_ssid("a", "t", &[]),
    )
    .unwrap();
    assert_eq!(
        mesh.get_network_ssid(&ssid_key("a"))
            .unwrap()
            .ssid
            .as_bytes(),
        b"t"
    );
    mesh.update_network_ssid_list(
        ListOp::Delete,
        &net("NetA"),
        fixtures::network_ssid("a", "t", &[]),
    )
    .unwrap();
    assert!(mesh.get_network_ssid(&ssid_key("a")).is_none());

    let mut record = zeroed_network();
    record.id = net("NetA").as_text();
    record.num_of_devices = 12;
    mesh.update_network_list(ListOp::Update, record).unwrap();
    assert_eq!(
        mesh.get_network(&net("NetA"))
            .unwrap()
            .info()
            .num_of_devices,
        12
    );
    let mut absent = zeroed_network();
    absent.id = net("NetZ").as_text();
    assert_eq!(
        mesh.update_network_list(ListOp::Update, absent),
        Err(not_found(Table::Network))
    );
}

/// `update_list` insert and delete reach the puts and removes of the mesh, including the node
/// creation of `put_device` and `put_network`.
#[test]
fn update_list_insert_and_delete_reach_the_mesh_operations() {
    let mut mesh = MeshModel::new(None);
    let mut record = zeroed_network();
    record.id = net("NetA").as_text();
    record.ctrl_id.mac = MAC1;
    mesh.update_network_list(ListOp::Insert, record).unwrap();
    assert!(mesh.get_data_model(&node_id("NetA", MAC1)).is_some());

    mesh.update_device_list(ListOp::Insert, device_in("NetA", MAC2), false)
        .unwrap();
    assert!(mesh.get_device(&node_id("NetA", MAC2)).is_some());
    mesh.update_radio_list(ListOp::Insert, radio_in("NetA", MAC2, mac(9)))
        .unwrap();
    mesh.update_bss_list(
        ListOp::Insert,
        bss_in("NetA", MAC2, mac(9), mac(20), HaulType::FRONTHAUL),
    )
    .unwrap();
    mesh.update_sta_list(ListOp::Insert, sta_on(mac(40), mac(20), mac(9)))
        .unwrap();
    assert!(mesh.first_sta().is_some());
    mesh.update_sta_list(ListOp::Delete, sta_on(mac(40), mac(20), mac(9)))
        .unwrap();
    assert!(mesh.first_sta().is_none());
    mesh.update_bss_list(
        ListOp::Delete,
        bss_in("NetA", MAC2, mac(9), mac(20), HaulType::FRONTHAUL),
    )
    .unwrap();
    assert!(mesh.first_bss().is_none());
    mesh.update_radio_list(ListOp::Delete, radio_in("NetA", MAC2, mac(9)))
        .unwrap();
    assert!(mesh.first_radio().is_none());
    mesh.update_device_list(ListOp::Delete, device_in("NetA", MAC2), false)
        .unwrap();
    assert!(mesh.get_device(&node_id("NetA", MAC2)).is_none());
    // The network has a node, so the delete is refused.
    let mut record = zeroed_network();
    record.id = net("NetA").as_text();
    assert!(matches!(
        mesh.update_network_list(ListOp::Delete, record),
        Err(ModelError::NetworkInUse { .. })
    ));
}

/// The `delete_list` residue removes every row of its class and returns how many; the scan
/// result list removes each result and the beacon reports of its neighbors.
#[test]
fn delete_list_removes_every_row_of_its_class() {
    let mut mesh = populated_mesh();
    assert_eq!(mesh.delete_sta_list(), 1);
    assert_eq!(mesh.delete_bss_list(), 1);
    assert_eq!(mesh.delete_scan_result_list(), 1);
    assert_eq!(mesh.delete_network_ssid_list(), 1);
    assert_eq!(mesh.delete_radio_list(), 1);
    assert_eq!(mesh.delete_radio_list(), 0);
    // 8 defaults on each node and the 2 seeds of the radio on the first.
    assert_eq!(mesh.delete_policy_list(), 18);
    assert_eq!(mesh.delete_op_class_list(), 12);
    assert_eq!(mesh.delete_network_list(), 0, "NetA has nodes");
    assert_eq!(mesh.delete_device_list(), 2);
    assert_eq!(mesh.delete_network_list(), 1);
    assert_eq!(mesh.num_networks(), 0);
}

/// Dirty marking stays with the caller (plan C5): the mesh's operations mark nothing except the two
/// list-layer marks.
#[test]
fn mesh_operations_do_not_mark_the_dirty_set() {
    let mut mesh = populated_mesh();
    mesh.put_device(
        device_id("NetA", MAC1, MediaType(0)),
        device_in("NetA", MAC1),
        false,
    )
    .unwrap();
    mesh.put_network(net("NetA"), zeroed_network()).unwrap();
    for view in mesh.nodes() {
        assert!(!view.node.dirty().any());
    }
}

/// A `NodeId` lookup of a node that was created with `from_controller_identity` works by the key
/// the mesh stores it under, not by the zero interface MAC `NodeRef::id()` reads.
#[test]
fn walk_cursors_are_the_map_keys_not_the_interface_mac() {
    let mut mesh = mesh_with_nodes(&[("NetA", MAC2)]);
    mesh.data_model_mut(&node_id("NetA", MAC2))
        .unwrap()
        .set_agent_al_interface_mac(MacAddr::ZERO);
    let first = mesh.first_device().unwrap();
    assert_eq!(first.at, node_id("NetA", MAC2));
    assert!(mesh.next_device(&first.at).is_none());
    assert_eq!(
        mesh.next_device(&node_id("NetA", MacAddr::ZERO))
            .map(|row| row.at),
        None
    );
}
