// SPDX-License-Identifier: Apache-2.0

//! Scan results: counts, lookup, `create_new_scan_result` and `update_scan_results`.

use super::fixtures::{agent_node, mac, net, node_init, scan_key, scan_result};
use crate::bounded::BoundedVec;
use crate::enums::{ChannelBandwidth, ScannerType};
use crate::model::error::Put;
use crate::model::keys::{NetId, NodeId, ScanKey};
use crate::model::network::NetworkModel;
use crate::model::node::NodeRole;
use crate::text::CStrBytes;
use crate::{Neighbor, NodeCtx, NodeModel};

fn neighbor(last: u8) -> Neighbor {
    Neighbor {
        bssid: mac(last),
        ssid: CStrBytes::default(),
        signal_strength: -50,
        bandwidth: ChannelBandwidth(0),
        bss_color: 0,
        channel_util: 0,
        sta_count: 0,
        bss_load_element_present: 0,
    }
}

// --- counts and reads (get_num_scan_results, get_scan_result) ---

#[test]
fn the_default_node_has_no_scan_results() {
    let node = agent_node();
    assert_eq!(node.num_scan_results(), 0);
    assert_eq!(node.scan_results().count(), 0);
}

#[test]
fn get_num_scan_results_counts_one_entry() {
    let mut node = agent_node();
    node.put_scan_result_record(scan_result(&scan_key(mac(5), 115, 36, ScannerType::RADIO)));
    assert_eq!(node.num_scan_results(), 1);
}

#[test]
fn get_num_scan_results_counts_multiple_entries() {
    let mut node = agent_node();
    for channel in [36, 40, 44] {
        node.put_scan_result_record(scan_result(&scan_key(
            mac(5),
            115,
            channel,
            ScannerType::RADIO,
        )));
    }
    assert_eq!(node.num_scan_results(), 3);
}

/// Research 7.13: upstream's new-record path copies the whole neighbour array and then
/// increments the count, so the count can exceed the stored neighbours
/// (`dm_easy_mesh_list.cpp:1548-1560`). The port's neighbour list has no separate count.
/// intentional-difference:r713-scan-neighbour-count
#[test]
fn put_scan_result_stores_exactly_the_neighbours_it_was_given() {
    let mut node = agent_node();
    let key = scan_key(mac(5), 115, 36, ScannerType::RADIO);
    let mut result = scan_result(&key);
    result.neighbor =
        BoundedVec::try_from_vec(vec![neighbor(1), neighbor(2), neighbor(3)]).unwrap();
    assert_eq!(node.put_scan_result_record(result), Put::Inserted);
    let stored = node.find_matching_scan_result(&key).unwrap();
    assert_eq!(stored.neighbor.len(), 3);
    assert_eq!(stored.neighbor.as_slice()[2].bssid, mac(3));
    assert_eq!(node.num_scan_results(), 1, "one result, not one more");
}

#[test]
fn put_scan_result_with_the_same_key_replaces_it_and_keeps_the_count() {
    let mut node = agent_node();
    let key = scan_key(mac(5), 115, 36, ScannerType::RADIO);
    node.put_scan_result_record(scan_result(&key));
    let mut again = scan_result(&key);
    again.util = 77;
    assert_eq!(node.put_scan_result_record(again), Put::Replaced);
    assert_eq!(node.num_scan_results(), 1);
    assert_eq!(node.find_matching_scan_result(&key).unwrap().util, 77);
}

/// Upstream indexes by hash iteration order (`dm_easy_mesh.cpp:3562-3577`); the port by key order.
/// intentional-difference:c3-key-order
#[test]
fn get_scan_result_by_index_follows_key_order() {
    let mut node = agent_node();
    for channel in [44, 36, 40] {
        node.put_scan_result_record(scan_result(&scan_key(
            mac(5),
            115,
            channel,
            ScannerType::RADIO,
        )));
    }
    let channels: Vec<u8> = (0..3)
        .map(|index| node.scan_result(index).unwrap().id.channel)
        .collect();
    assert_eq!(
        channels,
        vec![36, 40, 44],
        "key order replaces hash order (C3)"
    );
}

#[test]
fn get_scan_result_single_entry_and_out_of_range() {
    let mut node = agent_node();
    assert!(node.scan_result(0).is_none(), "empty map");
    node.put_scan_result_record(scan_result(&scan_key(mac(5), 115, 36, ScannerType::RADIO)));
    assert!(node.scan_result(0).is_some());
    assert!(node.scan_result(1).is_none());
}

// --- find_matching_scan_result ---

#[test]
fn find_matching_scan_result_needs_the_whole_key() {
    let mut node = agent_node();
    let key = scan_key(mac(5), 115, 36, ScannerType::RADIO);
    node.put_scan_result_record(scan_result(&key));
    assert!(node.find_matching_scan_result(&key).is_some());
    for other in [
        ScanKey { channel: 40, ..key },
        ScanKey {
            op_class: 81,
            ..key
        },
        ScanKey {
            scanner_mac: mac(6),
            ..key
        },
        ScanKey {
            dev_mac: mac(9),
            ..key
        },
        ScanKey {
            scanner_type: ScannerType(2),
            ..key
        },
    ] {
        assert!(
            node.find_matching_scan_result(&other).is_none(),
            "{other:?}"
        );
    }
}

/// Upstream compares the network id by prefix (`dm_easy_mesh.cpp:3579-3597`).
/// intentional-difference:c4-exact-keys
#[test]
fn find_matching_scan_result_compares_the_network_id_exactly() {
    let mut node = agent_node();
    let key = ScanKey {
        net: net("Network1"),
        ..scan_key(mac(5), 115, 36, ScannerType::RADIO)
    };
    node.put_scan_result_record(scan_result(&key));
    let longer = ScanKey {
        net: net("Network10"),
        ..key
    };
    assert!(node.find_matching_scan_result(&longer).is_none());
    assert!(node.find_matching_scan_result(&key).is_some());
}

// --- create_new_scan_result (dm_easy_mesh.cpp:3537-3560) ---

#[test]
fn create_new_scan_result_stores_an_empty_result_with_the_id() {
    let mut node = agent_node();
    let key = scan_key(mac(5), 115, 36, ScannerType::RADIO);
    let created = node.create_new_scan_result(key);
    assert_eq!(ScanKey::of(&created.id), key);
    assert_eq!(created.neighbor.len(), 0);
    assert_eq!(created.util, 0);
    assert_eq!(node.num_scan_results(), 1);
}

#[test]
fn create_new_scan_result_keeps_every_scanner_type() {
    let mut node = agent_node();
    for kind in [ScannerType::RADIO, ScannerType(2), ScannerType(3)] {
        node.create_new_scan_result(scan_key(mac(5), 115, 36, kind));
    }
    assert_eq!(node.num_scan_results(), 3);
    assert_eq!(
        node.find_matching_scan_result(&scan_key(mac(5), 115, 36, ScannerType(2)))
            .unwrap()
            .id
            .scanner_type,
        ScannerType(2)
    );
}

#[test]
fn create_new_scan_result_over_an_existing_key_replaces_it() {
    let mut node = agent_node();
    let key = scan_key(mac(5), 115, 36, ScannerType::RADIO);
    node.create_new_scan_result(key).util = 9;
    let again = node.create_new_scan_result(key);
    assert_eq!(again.util, 0);
    assert_eq!(node.num_scan_results(), 1);
}

#[test]
fn remove_scan_result_deletes_by_key() {
    let mut node = agent_node();
    let key = scan_key(mac(5), 115, 36, ScannerType::RADIO);
    node.put_scan_result_record(scan_result(&key));
    assert!(node.remove_scan_result(&key).is_some());
    assert!(node.remove_scan_result(&key).is_none());
    assert_eq!(node.num_scan_results(), 0);
}

// --- update_scan_results (dm_easy_mesh.cpp:3599-3614) ---

fn with_ctx<R>(net_id: NetId, body: impl FnOnce(&mut NodeCtx<'_>) -> R) -> (NodeModel, R) {
    let mut init = node_init(NodeRole::Agent);
    init.net = net_id;
    let mut node = NodeModel::new(init);
    let mut network = NetworkModel::new(net_id);
    let result = {
        let mut ctx = NodeCtx {
            id: NodeId {
                net: net_id,
                al: node.agent_al_interface_mac(),
            },
            network: &mut network,
            node: &mut node,
        };
        body(&mut ctx)
    };
    (node, result)
}

#[test]
fn update_scan_results_creates_a_new_result() {
    let key = scan_key(mac(5), 115, 36, ScannerType::RADIO);
    let (node, put) = with_ctx(NetId::global(), |ctx| {
        ctx.update_scan_results(scan_result(&key))
    });
    assert_eq!(put, Put::Inserted);
    assert_eq!(node.num_scan_results(), 1);
}

#[test]
fn update_scan_results_replaces_an_existing_result() {
    let key = scan_key(mac(5), 115, 36, ScannerType::RADIO);
    let (node, (first, second)) = with_ctx(NetId::global(), |ctx| {
        let first = ctx.update_scan_results(scan_result(&key));
        let mut again = scan_result(&key);
        again.util = 55;
        (first, ctx.update_scan_results(again))
    });
    assert_eq!((first, second), (Put::Inserted, Put::Replaced));
    assert_eq!(node.num_scan_results(), 1);
    assert_eq!(node.scan_result(0).unwrap().util, 55);
}

/// Upstream overrides `net_id` with `GLOBAL_NET_ID`, `dev_mac` with the device's AL MAC and
/// `scanner_type` with radio before the lookup (`dm_easy_mesh.cpp:3599-3614`; the net id is the
/// agent's network id). Parity for `dev_mac` and `scanner_type`.
#[test]
fn update_scan_results_overrides_the_net_id_dev_mac_and_scanner_type() {
    let key = ScanKey {
        net: net("elsewhere"),
        dev_mac: mac(99),
        scanner_type: ScannerType(3),
        ..scan_key(mac(5), 115, 36, ScannerType::RADIO)
    };
    let (node, ()) = with_ctx(NetId::global(), |ctx| {
        ctx.update_scan_results(scan_result(&key));
    });
    let stored = node.scan_result(0).unwrap();
    assert_eq!(stored.id.net_id.as_str(), "OneWifiMesh");
    assert_eq!(stored.id.dev_mac, node.agent_al_interface_mac());
    assert_eq!(stored.id.scanner_type, ScannerType::RADIO);
    assert_eq!(
        stored.id.scanner_mac,
        mac(5),
        "the other key fields are kept"
    );
}

/// The net id override is the id of the context's network: the agent's network.
#[test]
fn update_scan_results_uses_the_network_of_the_context() {
    let key = scan_key(mac(5), 115, 36, ScannerType::RADIO);
    let (node, ()) = with_ctx(net("AgentNet"), |ctx| {
        ctx.update_scan_results(scan_result(&key));
    });
    assert_eq!(node.scan_result(0).unwrap().id.net_id.as_str(), "AgentNet");
}

/// The overrides make results that differ only in those fields collide, as upstream's do.
#[test]
fn update_scan_results_treats_results_that_differ_only_in_the_overridden_fields_as_one() {
    let key = scan_key(mac(5), 115, 36, ScannerType::RADIO);
    let other = ScanKey {
        dev_mac: mac(77),
        scanner_type: ScannerType(2),
        ..key
    };
    let (node, ()) = with_ctx(NetId::global(), |ctx| {
        ctx.update_scan_results(scan_result(&key));
        ctx.update_scan_results(scan_result(&other));
    });
    assert_eq!(node.num_scan_results(), 1);
}
