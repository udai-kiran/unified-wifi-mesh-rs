// SPDX-License-Identifier: Apache-2.0

//! Regression tests of review round 2 (plan rev 3.1, D1-D6): each one fails on the code it was
//! written for. The walk helpers at the top are the only place that knows the cursor shape.

use super::fixtures::{self, agent_model, bss, mac, op_class, radio, radio_cap, sta};
use super::mesh_support::{MAC1, init, node_id, radio_in, scan_in, scan_key_in, sta_on};
use crate::bounded::BoundedVec;
use crate::enums::{HaulType, OpClassType, ScannerType, VapMode};
use crate::model::access::ModelAccess;
use crate::model::dirty::DbCfgType;
use crate::model::keys::{BssKey, NetId, ScanKey, SsidKey, StaKey};
use crate::model::node::StaMapTarget::Assoc;
use crate::model::node::StaMapTarget::Consolidated;
use crate::model::node::{NodeInit, NodeRole};
use crate::records::{AffiliatedStaInfo, AssocStaMldInfo, BeaconReport, Neighbor, WideBwChannel};
use crate::{ListOp, MeshModel, ModelDelta, SCAN_RESULT_SELF_INDEX};
use em_types::{MacAddr, ProfileType};

const GLOBAL: &str = NetId::GLOBAL;
/// More steps than any fixture has rows: a walk that needs more has looped.
const WALK_LIMIT: usize = 20;

fn mesh_with_agent() -> MeshModel {
    let mut mesh = MeshModel::new(None);
    mesh.create_data_model(init(
        GLOBAL,
        MAC1,
        ProfileType::MULTI_AP_PROFILE_3,
        NodeRole::Agent,
    ))
    .unwrap();
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

fn ssid_key(id: &str) -> SsidKey {
    SsidKey::new(id.as_bytes()).unwrap()
}

/// Number of rows `first_scan_result` / `next_scan_result` visit, at most `WALK_LIMIT + 1`.
fn scan_walk_len(mesh: &MeshModel) -> usize {
    let mut steps = 0;
    let mut cursor = mesh.first_scan_result().map(|row| (row.at, *row.key));
    while let Some((at, key)) = cursor {
        steps += 1;
        if steps > WALK_LIMIT {
            break;
        }
        cursor = mesh
            .next_scan_result(&at, &key)
            .map(|row| (row.at, *row.key));
    }
    steps
}

/// Number of rows `first_sta` / `next_sta` of the mesh visit.
fn sta_walk_len(mesh: &MeshModel) -> usize {
    let mut steps = 0;
    let mut cursor = mesh.first_sta().map(|row| (row.at, *row.key));
    while let Some((at, key)) = cursor {
        steps += 1;
        if steps > WALK_LIMIT {
            break;
        }
        cursor = mesh.next_sta(&at, &key).map(|row| (row.at, *row.key));
    }
    steps
}

/// Number of rows `first_sta_info` / `next_sta_info` of one node visit.
fn node_sta_info_walk_len(node: &crate::NodeModel) -> usize {
    let mut steps = 0;
    let mut cursor = node.first_sta_info(Consolidated).map(|entry| entry.key);
    while let Some(key) = cursor {
        steps += 1;
        if steps > WALK_LIMIT {
            break;
        }
        cursor = node
            .next_sta_info(Consolidated, &key)
            .map(|entry| entry.key);
    }
    steps
}

/// Number of rows `first_network_ssid` / `next_network_ssid` of the mesh visit.
fn ssid_walk_len(mesh: &MeshModel) -> usize {
    let mut steps = 0;
    let mut cursor = mesh.first_network_ssid().map(|row| (row.at, row.index));
    while let Some((at, index)) = cursor {
        steps += 1;
        if steps > WALK_LIMIT {
            break;
        }
        cursor = mesh
            .next_network_ssid(&at, index)
            .map(|row| (row.at, row.index));
    }
    steps
}

// --- D1a: walks follow the stored location, not the record's fields ---

/// Editing the id of a scan result through `scan_result_mut` does not move the row, so the walk
/// still visits each stored row once. Upstream walks pointers (`dm_easy_mesh_list.cpp:1403-1426`).
#[test]
fn mesh_scan_walk_visits_each_row_once_after_a_key_field_edit() {
    let mut mesh = mesh_with_agent();
    let id = node_id(GLOBAL, MAC1);
    let first = scan_key_in(GLOBAL, MAC1, mac(0x21), 1, 1, ScannerType::RADIO);
    let second = scan_key_in(GLOBAL, MAC1, mac(0x21), 1, 6, ScannerType::RADIO);
    assert!(first < second);
    let node = mesh.data_model_mut(&id).unwrap();
    node.put_scan_result_record(scan_in(&first));
    node.put_scan_result_record(scan_in(&second));
    node.scan_result_mut(&first).unwrap().id = second.to_id();
    assert_eq!(scan_walk_len(&mesh), 2);
}

/// The same for the consolidated station map of the mesh walk.
#[test]
fn mesh_sta_walk_visits_each_row_once_after_a_key_field_edit() {
    let mut mesh = mesh_with_agent();
    let id = node_id(GLOBAL, MAC1);
    let node = mesh.data_model_mut(&id).unwrap();
    node.put_sta(sta(mac(0x31), mac(0x41), mac(0x51)));
    node.put_sta(sta(mac(0x32), mac(0x41), mac(0x51)));
    let first = StaKey {
        sta: mac(0x31),
        bssid: mac(0x41),
        radio: mac(0x51),
    };
    node.sta_info_mut(&first, Consolidated).unwrap().id = mac(0x32);
    assert_eq!(sta_walk_len(&mesh), 2);
}

/// The node-level walk too (`get_first_sta_info` / `get_next_sta_info`,
/// `dm_easy_mesh.cpp:3006-3056`).
#[test]
fn node_sta_info_walk_visits_each_row_once_after_a_key_field_edit() {
    let mut node = fixtures::agent_node();
    node.put_sta(sta(mac(0x31), mac(0x41), mac(0x51)));
    node.put_sta(sta(mac(0x32), mac(0x41), mac(0x51)));
    let first = StaKey {
        sta: mac(0x31),
        bssid: mac(0x41),
        radio: mac(0x51),
    };
    node.sta_info_mut(&first, Consolidated).unwrap().id = mac(0x32);
    assert_eq!(node_sta_info_walk_len(&node), 2);
}

/// Two SSID rows that end up with the same id are both walked: the SSID walk is positional,
/// like the BSS and op-class walks (`dm_easy_mesh_list.cpp:829-867`).
#[test]
fn network_ssid_walk_visits_two_rows_with_the_same_id() {
    let mut mesh = mesh_with_agent();
    let net = NetId::global();
    for id in ["a", "b"] {
        mesh.put_network_ssid(&net, fixtures::network_ssid(id, "s", &[]))
            .unwrap();
    }
    let network = mesh.network_mut(&net).unwrap();
    let first_id = network.network_ssid(0).unwrap().id;
    network.network_ssid_mut(&ssid_key("b")).unwrap().id = first_id;
    assert_eq!(ssid_walk_len(&mesh), 2);
}

// --- D3: capability slots follow the radios ---

/// A capability for another radio is not stored in the radio's slot, so removing the radio
/// leaves no orphan capability and a later radio does not inherit one
/// (`dm_easy_mesh.cpp:239-247`).
#[test]
fn a_mismatched_capability_does_not_outlive_its_radio() {
    let mut model = agent_model();
    let (radio_a, radio_b, radio_c) = (mac(0x61), mac(0x62), mac(0x63));
    ModelDelta::EmInsertRadio {
        radio: radio(radio_a, true),
        cap: Some(radio_cap(radio_b)),
        op_classes: vec![],
    }
    .apply(&mut model.ctx());
    assert!(model.ctx().node.remove_radio(radio_a).is_some());
    assert_eq!(model.node_model().radio_caps().len(), 0);

    model.ctx().node.put_radio(radio(radio_c, true)).unwrap();
    let node = model.node_model();
    assert!(node.radio_caps().len() <= node.radios().len());
    assert!(node.radio_cap(radio_b).is_none());
}

/// `radio_mut` can rewrite `intf.mac`; removal is by the radio's index, so its capability slot
/// goes with it.
#[test]
fn removing_a_radio_whose_mac_was_rewritten_removes_its_capability_slot() {
    let mut model = agent_model();
    let (old, new) = (mac(0x61), mac(0x6f));
    ModelDelta::EmInsertRadio {
        radio: radio(old, true),
        cap: Some(radio_cap(old)),
        op_classes: vec![],
    }
    .apply(&mut model.ctx());
    model.ctx().node.radio_mut(0).unwrap().intf.mac = new;
    assert!(model.ctx().node.remove_radio(new).is_some());
    let node = model.node_model();
    assert_eq!(node.radios().len(), 0);
    assert_eq!(node.radio_caps().len(), 0);
}

// --- D1b: bulk operations route through the visited node ---

/// A node whose AL MAC was rewritten stays stored under its original key. A scan result put
/// through it carries the new AL MAC in its id, and the bulk clear still finds and removes it
/// (`dm_scan_result_list.cpp:207-235` walks the nodes it holds).
/// intentional-difference:d1-storage-route
#[test]
fn delete_scan_result_list_removes_a_result_of_a_node_with_a_rewritten_al_mac() {
    let mut mesh = mesh_with_agent();
    let id = node_id(GLOBAL, MAC1);
    {
        let mut ctx = mesh.node_ctx(&id).unwrap();
        ctx.node.set_agent_al_interface_mac(mac(0xEE));
        let key = scan_key_in(GLOBAL, MAC1, mac(0x21), 1, 1, ScannerType::RADIO);
        let outcome = ModelDelta::ScanResults(scan_in(&key)).apply(&mut ctx);
        assert_eq!(outcome.applied, 1);
    }
    assert_eq!(mesh.get_data_model(&id).unwrap().num_scan_results(), 1);
    assert_eq!(mesh.delete_scan_result_list(), 1);
    assert_eq!(mesh.get_data_model(&id).unwrap().num_scan_results(), 0);
}

// --- D4(b): the co-located write marks the controller of the network ---

fn controller_init(al: MacAddr) -> NodeInit {
    init(
        GLOBAL,
        al,
        ProfileType::MULTI_AP_PROFILE_3,
        NodeRole::Controller,
    )
}

fn colocated_agent_init(al: MacAddr) -> NodeInit {
    NodeInit {
        colocated: true,
        ..init(GLOBAL, al, ProfileType::MULTI_AP_PROFILE_3, NodeRole::Agent)
    }
}

fn marked(mesh: &MeshModel, al: MacAddr) -> bool {
    mesh.get_data_model(&node_id(GLOBAL, al))
        .unwrap()
        .dirty()
        .is_set(DbCfgType::NetworkListUpdate)
}

/// After `set_ctrl_al_interface_mac` the network's controller MAC is no node key. The node is
/// still the controller of the network, so it is marked.
/// intentional-difference:d4-controller-by-role
#[test]
fn colocated_agent_marks_the_controller_after_its_mac_was_rewritten() {
    let mut mesh = MeshModel::new(None);
    mesh.create_data_model(controller_init(mac(0xA1))).unwrap();
    let ctrl = node_id(GLOBAL, mac(0xA1));
    mesh.node_ctx(&ctrl)
        .unwrap()
        .set_ctrl_al_interface_mac(mac(0xB2));
    mesh.create_data_model(colocated_agent_init(mac(0xC3)))
        .unwrap();
    let network = mesh.get_network(&NetId::global()).unwrap();
    assert_eq!(network.colocated_agent_interface().mac, mac(0xC3));
    assert!(marked(&mesh, mac(0xA1)));
}

/// With two controllers the one whose key is the network's controller MAC is marked, and only
/// that one.
/// intentional-difference:d4-controller-by-role
#[test]
fn colocated_agent_marks_the_controller_whose_key_matches_the_network() {
    let mut mesh = MeshModel::new(None);
    mesh.create_data_model(controller_init(mac(0xA1))).unwrap();
    mesh.create_data_model(controller_init(mac(0xA2))).unwrap();
    mesh.network_mut(&NetId::global())
        .unwrap()
        .set_controller_id(mac(0xA2));
    mesh.create_data_model(colocated_agent_init(mac(0xC3)))
        .unwrap();
    assert!(marked(&mesh, mac(0xA2)));
    assert!(!marked(&mesh, mac(0xA1)));
}

/// With no key match, the first controller of the network in key order is marked.
/// intentional-difference:d4-controller-by-role
#[test]
fn colocated_agent_marks_the_first_controller_when_no_key_matches() {
    let mut mesh = MeshModel::new(None);
    mesh.create_data_model(controller_init(mac(0xA2))).unwrap();
    mesh.create_data_model(controller_init(mac(0xA1))).unwrap();
    mesh.network_mut(&NetId::global())
        .unwrap()
        .set_controller_id(mac(0xEE));
    mesh.create_data_model(colocated_agent_init(mac(0xC3)))
        .unwrap();
    assert!(marked(&mesh, mac(0xA1)));
    assert!(!marked(&mesh, mac(0xA2)));
}

/// A network without a controller node records the MAC and marks nothing; an agent is never
/// chosen.
/// intentional-difference:d4-controller-by-role
#[test]
fn colocated_agent_without_a_controller_node_marks_nothing() {
    let mut mesh = mesh_with_agent();
    mesh.create_data_model(colocated_agent_init(mac(0xC3)))
        .unwrap();
    let network = mesh.get_network(&NetId::global()).unwrap();
    assert_eq!(network.colocated_agent_interface().mac, mac(0xC3));
    assert!(
        !mesh
            .get_data_model(&node_id(GLOBAL, MAC1))
            .unwrap()
            .dirty()
            .any()
    );
}

// --- D5(b): the self report of a station scan ---

fn station_with_scanner() -> (MeshModel, ScanKey) {
    let mut mesh = mesh_with_agent();
    mesh.put_radio(radio_in(GLOBAL, MAC1, mac(1))).unwrap();
    mesh.put_sta(sta_on(mac(40), mac(41), mac(1))).unwrap();
    let key = scan_key_in(GLOBAL, MAC1, mac(40), 81, 6, ScannerType::STA);
    (mesh, key)
}

fn report_count(mesh: &MeshModel) -> usize {
    mesh.get_data_model(&node_id(GLOBAL, MAC1))
        .unwrap()
        .sta_rows(Consolidated)
        .map(|row| row.beacon_reports.len())
        .sum()
}

fn report_of(bssid: MacAddr) -> BeaconReport {
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

/// Insert and clear of a neighbor-less station scan, repeated past the 10 reports of a station:
/// the clear removes the zero-BSSID report the insert appended.
/// intentional-difference:d5-self-report-cleanup
#[test]
fn clearing_scan_results_removes_the_self_report_of_a_station_scan() {
    let (mut mesh, key) = station_with_scanner();
    for _ in 0..12 {
        mesh.update_scan_result_list(ListOp::Insert, scan_in(&key), SCAN_RESULT_SELF_INDEX)
            .unwrap();
        assert_eq!(report_count(&mesh), 1);
        assert_eq!(mesh.delete_scan_result_list(), 1);
        assert_eq!(report_count(&mesh), 0);
    }
}

/// The cleanup is once per removed result: a report of another BSSID stays.
/// intentional-difference:d5-self-report-cleanup
#[test]
fn clearing_scan_results_keeps_the_reports_of_other_bssids() {
    let (mut mesh, key) = station_with_scanner();
    let node = mesh.data_model_mut(&node_id(GLOBAL, MAC1)).unwrap();
    let station = StaKey {
        sta: mac(40),
        bssid: mac(41),
        radio: mac(1),
    };
    let row = node.sta_info_mut(&station, Consolidated).unwrap();
    row.beacon_reports.push(report_of(mac(77))).unwrap();
    mesh.update_scan_result_list(ListOp::Insert, scan_in(&key), SCAN_RESULT_SELF_INDEX)
        .unwrap();
    assert_eq!(report_count(&mesh), 2);
    assert_eq!(mesh.delete_scan_result_list(), 1);
    assert_eq!(report_count(&mesh), 1);
}

/// A scan result with neighbors still removes each neighbor's report and the result once.
/// intentional-difference:d5-self-report-cleanup
#[test]
fn clearing_a_station_scan_with_neighbors_removes_every_report() {
    let (mut mesh, key) = station_with_scanner();
    let mut result = scan_in(&key);
    for last in [51, 52] {
        result.neighbor.push(neighbor(mac(last))).unwrap();
    }
    mesh.put_scan_result(mac(51), result, 0).unwrap();
    assert_eq!(report_count(&mesh), 1);
    assert_eq!(mesh.delete_scan_result_list(), 1);
    assert_eq!(report_count(&mesh), 0);
}

// --- D1a: beacon_station acts on the row it found ---

fn drifted_station() -> (MeshModel, ScanKey) {
    let (mut mesh, key) = station_with_scanner();
    let node = mesh.data_model_mut(&node_id(GLOBAL, MAC1)).unwrap();
    let stored = StaKey {
        sta: mac(40),
        bssid: mac(41),
        radio: mac(1),
    };
    // The row stays under `stored` while its BSSID field changes.
    node.sta_info_mut(&stored, Consolidated).unwrap().bssid = mac(99);
    (mesh, key)
}

/// `put_scan_result` appends the report to the station row it found, whatever its BSSID field
/// says now (`dm_easy_mesh_list.cpp:1590-1610` holds the row pointer).
#[test]
fn put_scan_result_reports_on_a_station_row_whose_key_field_was_edited() {
    let (mut mesh, key) = drifted_station();
    let mut result = scan_in(&key);
    result.neighbor.push(neighbor(mac(51))).unwrap();
    mesh.put_scan_result(mac(51), result, 0).unwrap();
    assert_eq!(report_count(&mesh), 1);
}

/// `remove_scan_result` removes the report from the same row.
#[test]
fn remove_scan_result_cleans_a_station_row_whose_key_field_was_edited() {
    let (mut mesh, key) = drifted_station();
    let mut result = scan_in(&key);
    result.neighbor.push(neighbor(mac(51))).unwrap();
    mesh.put_scan_result(mac(51), result, 0).unwrap();
    assert_eq!(report_count(&mesh), 1);
    assert!(mesh.remove_scan_result(&key, mac(51)).is_some());
    assert_eq!(report_count(&mesh), 0);
}

// --- missing walk tests ---

/// A walk over two nodes visits every op class of both once (kills a `next_op_class` that
/// ignores the node of the cursor).
#[test]
fn op_class_walk_across_two_nodes_visits_each_row_once() {
    let mut mesh = mesh_with_agent();
    mesh.create_data_model(init(
        GLOBAL,
        mac(0xF0),
        ProfileType::MULTI_AP_PROFILE_3,
        NodeRole::Agent,
    ))
    .unwrap();
    let total: usize = mesh.nodes().map(|node| node.node.op_classes().len()).sum();
    assert!(total > 0);
    let mut visited = Vec::new();
    let mut cursor = mesh.first_op_class().map(|row| (row.at, row.index));
    while let Some((at, index)) = cursor {
        assert!(visited.len() <= total, "the walk does not end");
        visited.push((at, index));
        cursor = mesh
            .next_op_class(&at, index)
            .map(|row| (row.at, row.index));
    }
    assert_eq!(visited.len(), total);
}

/// A cursor that is an op class of another kind is not a cursor: `None`
/// (`dm_easy_mesh_list.cpp:1140-1170` finds no pointer match).
#[test]
fn next_pre_set_op_class_with_a_cursor_of_another_kind_is_none() {
    let mesh = mesh_with_agent();
    let node = mesh.get_data_model(&node_id(GLOBAL, MAC1)).unwrap();
    // The presets are three `anticipated` rows, then three `scan_param` rows.
    assert_eq!(node.op_class(0).unwrap().id.kind, OpClassType::ANTICIPATED);
    assert_eq!(node.op_class(3).unwrap().id.kind, OpClassType::SCAN_PARAM);
    assert!(
        mesh.next_pre_set_op_class_by_type(OpClassType::ANTICIPATED, 3)
            .is_none()
    );
    // Control: a cursor of the asked kind continues.
    let next = mesh
        .next_pre_set_op_class_by_type(OpClassType::SCAN_PARAM, 3)
        .unwrap();
    assert_eq!(next.index, 4);
}

// --- D2: the engine finds a BSS by (radio, BSSID) and rewrites its identity in place ---

/// The engine finds the BSS by (ruid, bssid) and rewrites `id.haul_type` through the index
/// (`em_configuration.cpp:1625-1638`). The (ruid, bssid) find still hits afterwards, `BssKey`
/// finds the row by the new haul type and no longer by the old one, and the walk visits the row
/// once.
#[test]
fn a_bss_whose_haul_type_was_rewritten_in_place_is_still_found_and_walked_once() {
    let mut mesh = mesh_with_agent();
    let id = node_id(GLOBAL, MAC1);
    let (ruid, bssid) = (mac(0x51), mac(0x61));
    let node = mesh.data_model_mut(&id).unwrap();
    node.put_bss(bss(ruid, bssid, HaulType::FRONTHAUL, VapMode::AP))
        .unwrap();

    let index = node.bss_index_by_radio(ruid, bssid).unwrap();
    node.bss_mut(index).unwrap().id.haul_type = HaulType::BACKHAUL;

    assert_eq!(node.bss_index_by_radio(ruid, bssid), Some(index));
    assert!(node.bss_by_radio(ruid, bssid).is_some());
    let old_key = BssKey {
        ruid,
        bssid,
        haul_type: HaulType::FRONTHAUL,
    };
    let new_key = BssKey {
        haul_type: HaulType::BACKHAUL,
        ..old_key
    };
    assert!(node.find_matching_bss(&old_key).is_none());
    assert!(node.find_matching_bss(&new_key).is_some());
    assert!(node.bss_index_by_radio(mac(0x52), bssid).is_none());
    assert!(node.bss_index_by_radio(ruid, mac(0x62)).is_none());

    let mut visited = 0;
    let mut cursor = mesh.first_bss().map(|row| (row.at, row.index));
    while let Some((at, position)) = cursor {
        visited += 1;
        assert!(visited <= WALK_LIMIT, "the walk does not end");
        cursor = mesh.next_bss(&at, position).map(|row| (row.at, row.index));
    }
    assert_eq!(visited, 1);
}

// --- D1a: the walks yield the key a row is stored under ---

/// After the engine edits a key field of a row, the walk still yields the key the row is stored
/// under, which is the cursor it takes next.
#[test]
fn walks_yield_the_storage_key_not_the_key_of_the_edited_row() {
    let mut mesh = mesh_with_agent();
    let id = node_id(GLOBAL, MAC1);
    let node = mesh.data_model_mut(&id).unwrap();
    node.put_sta(sta(mac(0x31), mac(0x41), mac(0x51)));
    let stored = StaKey {
        sta: mac(0x31),
        bssid: mac(0x41),
        radio: mac(0x51),
    };
    node.sta_info_mut(&stored, Consolidated).unwrap().id = mac(0x77);

    let first = mesh.first_sta().unwrap();
    assert_eq!(*first.key, stored);
    assert_eq!(first.item.id, mac(0x77));
    let entry = mesh
        .get_data_model(&id)
        .unwrap()
        .first_sta_info(Consolidated)
        .unwrap();
    assert_eq!(entry.key, stored);
}

/// Policies cannot drift (no `&mut`), but the walk takes the stored key like the others: it
/// visits every policy of the node once, in key order.
#[test]
fn policy_walk_yields_the_stored_key_and_visits_each_policy_once() {
    let mesh = mesh_with_agent();
    let node = mesh.get_data_model(&node_id(GLOBAL, MAC1)).unwrap();
    let mut keys = Vec::new();
    let mut cursor = mesh.first_policy().map(|row| (row.at, *row.key));
    while let Some((at, key)) = cursor {
        assert!(keys.len() <= node.num_policies(), "the walk does not end");
        keys.push(key);
        cursor = mesh.next_policy(&at, &key).map(|row| (row.at, *row.key));
    }
    let stored: Vec<_> = node.policy_entries().map(|(key, _)| *key).collect();
    assert_eq!(keys, stored);
    assert_eq!(keys.len(), node.num_policies());
}

// --- D1b: the bulk deletes remove from the node and position they visit ---

/// `intentional-difference:d1-storage-route`: two radios that share a MAC after an engine edit
/// are both removed, each with its capability slot.
#[test]
fn delete_radio_list_removes_every_radio_and_capability_slot_by_position() {
    let mut mesh = mesh_with_agent();
    let id = node_id(GLOBAL, MAC1);
    let node = mesh.data_model_mut(&id).unwrap();
    node.commit_radio(radio(mac(0x10), true), Some(radio_cap(mac(0x10))))
        .unwrap();
    node.commit_radio(radio(mac(0x11), true), Some(radio_cap(mac(0x11))))
        .unwrap();
    node.radio_mut(1).unwrap().intf.mac = mac(0x10);
    assert_eq!(mesh.delete_radio_list(), 2);
    let node = mesh.get_data_model(&id).unwrap();
    assert_eq!(node.radios().len(), 0);
    assert_eq!(node.radio_caps().len(), 0);
}

/// `intentional-difference:d1-storage-route`: two op classes that share a key are both removed,
/// and so is every preset.
#[test]
fn delete_op_class_list_removes_rows_that_share_a_key() {
    let mut mesh = mesh_with_agent();
    let id = node_id(GLOBAL, MAC1);
    let node = mesh.data_model_mut(&id).unwrap();
    node.commit_op_class(op_class(mac(0x10), OpClassType::CAPABILITY, 115))
        .unwrap();
    node.commit_op_class(op_class(mac(0x10), OpClassType::CAPABILITY, 116))
        .unwrap();
    let total = node.op_classes().len();
    node.op_class_mut(total - 1).unwrap().id.op_class = 115;
    assert_eq!(mesh.delete_op_class_list(), total);
    assert_eq!(mesh.get_data_model(&id).unwrap().op_classes().len(), 0);
}

/// `intentional-difference:d1-storage-route`: the policies of every node are cleared and
/// counted.
#[test]
fn delete_policy_list_clears_the_policies_of_every_node() {
    let mut mesh = mesh_with_agent();
    mesh.create_data_model(init(
        GLOBAL,
        mac(0xF0),
        ProfileType::MULTI_AP_PROFILE_3,
        NodeRole::Agent,
    ))
    .unwrap();
    let total: usize = mesh.nodes().map(|node| node.node.num_policies()).sum();
    assert!(total > 0);
    assert_eq!(mesh.delete_policy_list(), total);
    assert!(mesh.nodes().all(|node| node.node.num_policies() == 0));
}

/// `intentional-difference:d1-storage-route`: two SSID rows that share an id after an engine
/// edit are both removed.
#[test]
fn delete_network_ssid_list_removes_rows_that_share_an_id() {
    let mut mesh = mesh_with_agent();
    let net = NetId::global();
    for id in ["a", "b"] {
        mesh.put_network_ssid(&net, fixtures::network_ssid(id, "s", &[]))
            .unwrap();
    }
    let network = mesh.network_mut(&net).unwrap();
    let first_id = network.network_ssid(0).unwrap().id;
    network.network_ssid_mut(&ssid_key("b")).unwrap().id = first_id;
    assert_eq!(mesh.delete_network_ssid_list(), 2);
    assert_eq!(mesh.get_network(&net).unwrap().num_network_ssids(), 0);
}

/// `intentional-difference:d1-storage-route`: `apply_sta_mld_association` stages the rows it
/// finds in the consolidated map under their stored keys, whatever their fields say now
/// (upstream formats the key from the row, `dm_easy_mesh.cpp:3967-3975, 4007-4016`).
#[test]
fn apply_sta_mld_association_uses_the_stored_keys_of_the_consolidated_map() {
    let mut node = fixtures::agent_node();
    let stored = StaKey {
        sta: mac(30),
        bssid: mac(20),
        radio: mac(10),
    };
    let mut row = sta(mac(30), mac(20), mac(10));
    row.associated = true;
    node.put_sta(row);
    node.sta_info_mut(&stored, Consolidated).unwrap().bssid = mac(99);

    let mld = AssocStaMldInfo {
        mac_addr: mac(30),
        ap_mld_mac_addr: mac(60),
        str: false,
        nstr: false,
        emlsr: false,
        emlmr: false,
        affiliated_sta: BoundedVec::<AffiliatedStaInfo, 64>::new(),
    };
    assert!(node.apply_sta_mld_association(&mld));
    assert!(!node.sta_info(&stored, Consolidated).unwrap().associated);
    assert!(node.sta_info(&stored, Assoc).is_some());
    assert_eq!(node.num_sta(Assoc), 1);
}
