// SPDX-License-Identifier: Apache-2.0

//! Typed keys (plan C4): exact matching and network scoping.

use super::fixtures::{agent_node, mac, net, node_init};
use crate::enums::HaulType;
use crate::model::keys::{BssKey, NetId, NodeId, OpClassKey, ScanKey, SsidKey, StaKey};
use crate::model::node::{NodeModel, NodeRole};
use std::collections::BTreeMap;

/// Upstream matches network ids with `strncmp` over the shorter string, so "Network1" matches
/// "Network10" (research 7.6). intentional-difference:c4-exact-keys
#[test]
fn network_ids_compare_exactly() {
    assert_ne!(net("Network1"), net("Network10"));
    assert_eq!(net("Network1"), net("Network1"));
}

#[test]
fn a_map_keyed_by_node_id_does_not_confuse_a_prefix_network() {
    let key_short = NodeId {
        net: net("Network1"),
        al: mac(1),
    };
    let key_long = NodeId {
        net: net("Network10"),
        al: mac(1),
    };
    let mut nodes: BTreeMap<NodeId, u8> = BTreeMap::new();
    nodes.insert(key_short, 1);
    assert!(!nodes.contains_key(&key_long));
    nodes.insert(key_long, 2);
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[&key_short], 1);
}

#[test]
fn the_same_mac_in_different_networks_is_two_node_ids() {
    let in_a = NodeId {
        net: net("A"),
        al: mac(1),
    };
    let in_b = NodeId {
        net: net("B"),
        al: mac(1),
    };
    assert_ne!(in_a, in_b);
    let mut init_a = node_init(NodeRole::Agent);
    init_a.net = net("A");
    let mut init_b = node_init(NodeRole::Agent);
    init_b.net = net("B");
    let node_a = NodeModel::new(init_a);
    let node_b = NodeModel::new(init_b);
    assert_eq!(
        node_a.agent_al_interface_mac(),
        node_b.agent_al_interface_mac()
    );
    assert_ne!(node_a.net(), node_b.net());
    assert_eq!(node_a.device().id.net_id.as_str(), "A");
    assert_eq!(node_b.device().id.net_id.as_str(), "B");
}

#[test]
fn node_ids_order_by_network_then_mac() {
    let mut ids = [
        NodeId {
            net: net("B"),
            al: mac(1),
        },
        NodeId {
            net: net("A"),
            al: mac(2),
        },
        NodeId {
            net: net("A"),
            al: mac(1),
        },
    ];
    ids.sort();
    assert_eq!(ids[0].al, mac(1));
    assert_eq!(ids[1].al, mac(2));
    assert_eq!(ids[2].net, net("B"));
}

#[test]
fn the_global_network_id_is_one_wifi_mesh() {
    assert_eq!(NetId::global().as_str(), "OneWifiMesh");
    assert_eq!(NetId::GLOBAL, "OneWifiMesh");
}

/// Upstream truncates formatted keys to 64 or 256 bytes (`dm_easy_mesh_list.cpp:1643-1660`);
/// the typed key holds the whole id. intentional-difference:c4-no-key-truncation
#[test]
fn a_127_byte_network_id_is_kept_whole_and_distinct() {
    let long_a = "a".repeat(127);
    let mut long_b = "a".repeat(126);
    long_b.push('b');
    assert_ne!(net(&long_a), net(&long_b));
    assert!(NetId::new(&"a".repeat(128)).is_err());
}

#[test]
fn sta_key_is_the_triple_and_each_part_matters() {
    let base = StaKey {
        sta: mac(1),
        bssid: mac(2),
        radio: mac(3),
    };
    assert_ne!(
        base,
        StaKey {
            radio: mac(4),
            ..base
        }
    );
    assert_ne!(
        base,
        StaKey {
            bssid: mac(4),
            ..base
        }
    );
    assert_ne!(
        base,
        StaKey {
            sta: mac(4),
            ..base
        }
    );
    let mut row = super::fixtures::sta(mac(1), mac(2), mac(3));
    assert_eq!(StaKey::of(&row), base);
    row.associated = true;
    assert_eq!(
        StaKey::of(&row),
        base,
        "non-key fields are not part of the key"
    );
}

#[test]
fn bss_key_includes_the_haul_type() {
    let row = super::fixtures::bss(
        mac(10),
        mac(20),
        HaulType::BACKHAUL,
        crate::enums::VapMode::AP,
    );
    let key = BssKey::of(&row);
    assert_eq!(
        key,
        BssKey {
            ruid: mac(10),
            bssid: mac(20),
            haul_type: HaulType::BACKHAUL
        }
    );
    assert_ne!(
        key,
        BssKey {
            haul_type: HaulType::FRONTHAUL,
            ..key
        }
    );
}

#[test]
fn op_class_key_is_radio_type_and_number() {
    let info = super::fixtures::op_class(mac(10), crate::enums::OpClassType::CURRENT, 115);
    let key = OpClassKey::of(&info);
    assert_eq!(key.ruid, mac(10));
    assert_eq!(key.kind, crate::enums::OpClassType::CURRENT);
    assert_eq!(key.op_class, 115);
}

#[test]
fn scan_key_round_trips_through_the_scan_result_id() {
    let key = super::fixtures::scan_key(mac(5), 115, 36, crate::enums::ScannerType::RADIO);
    assert_eq!(ScanKey::of(&key.to_id()), key);
    assert_ne!(key, ScanKey { channel: 40, ..key });
    assert_ne!(
        key,
        ScanKey {
            net: net("other"),
            ..key
        }
    );
}

/// Upstream finds a network SSID by prefix (`dm_easy_mesh_list.cpp:869-885, 925`); the key is
/// matched exactly. intentional-difference:c4-exact-keys
#[test]
fn ssid_keys_compare_exactly() {
    let short = SsidKey::new(b"Ssid1").unwrap();
    let long = SsidKey::new(b"Ssid10").unwrap();
    assert_ne!(short, long);
    assert!(short < long);
    assert_eq!(short.as_bytes(), b"Ssid1");
}

#[test]
fn keys_are_copy_and_small() {
    fn assert_copy<T: Copy>() {}
    assert_copy::<NetId>();
    assert_copy::<NodeId>();
    assert_copy::<StaKey>();
    assert_copy::<ScanKey>();
    assert_copy::<BssKey>();
    assert_copy::<OpClassKey>();
    assert_copy::<SsidKey>();
    assert_eq!(agent_node().net(), NetId::global());
}
