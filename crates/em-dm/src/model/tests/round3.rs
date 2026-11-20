// SPDX-License-Identifier: Apache-2.0

//! Regression tests of review round 3 (plan rev 3.1, D5(b) and the scan list index rules): each
//! one fails on the code it was written for.

use super::fixtures::mac;
use super::mesh_support::{MAC1, init, node_id, radio_in, scan_in, scan_key_in, sta_on};
use crate::enums::ScannerType;
use crate::model::error::{ModelError, Table};
use crate::model::keys::{NetId, ScanKey};
use crate::model::node::NodeRole;
use crate::model::node::StaMapTarget::Consolidated;
use crate::records::{BeaconReport, Neighbor, WideBwChannel};
use crate::{ListOp, MeshModel, SCAN_RESULT_SELF_INDEX};
use em_types::{MacAddr, ProfileType};

const GLOBAL: &str = NetId::GLOBAL;

/// A node with one radio, a station `mac(40)` and the key of a station scan of it on channel 6.
fn station_with_scanner() -> (MeshModel, ScanKey) {
    let mut mesh = MeshModel::new(None);
    mesh.create_data_model(init(
        GLOBAL,
        MAC1,
        ProfileType::MULTI_AP_PROFILE_3,
        NodeRole::Agent,
    ))
    .unwrap();
    mesh.put_radio(radio_in(GLOBAL, MAC1, mac(1))).unwrap();
    mesh.put_sta(sta_on(mac(40), mac(41), mac(1))).unwrap();
    let key = scan_key_in(GLOBAL, MAC1, mac(40), 81, 6, ScannerType::STA);
    (mesh, key)
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

fn report(channel: u8, bssid: MacAddr) -> BeaconReport {
    BeaconReport {
        op_class: 81,
        channel,
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

/// The (channel, BSSID) of every beacon report of the station, in order.
fn reports(mesh: &MeshModel) -> Vec<(u8, MacAddr)> {
    mesh.get_data_model(&node_id(GLOBAL, MAC1))
        .unwrap()
        .sta_rows(Consolidated)
        .flat_map(|row| row.beacon_reports.iter())
        .map(|report| (report.channel, report.bssid))
        .collect()
}

fn num_scans(mesh: &MeshModel) -> usize {
    mesh.get_data_model(&node_id(GLOBAL, MAC1))
        .unwrap()
        .num_scan_results()
}

fn add_report(mesh: &mut MeshModel, report: BeaconReport) {
    let node = mesh.data_model_mut(&node_id(GLOBAL, MAC1)).unwrap();
    let station = node.first_sta(mac(40)).unwrap().key;
    let row = node.sta_info_mut(&station, Consolidated).unwrap();
    row.beacon_reports.push(report).unwrap();
}

// --- DM3-01: the self report of a removed result is matched by op class and channel ---

/// Codex repro: a zero-BSSID report of channel 11 and a self scan on channel 6; the clear removes
/// the channel-6 report and keeps the other.
/// intentional-difference:d5-self-report-cleanup
#[test]
fn clearing_scan_results_removes_the_self_report_of_its_own_channel() {
    let (mut mesh, key) = station_with_scanner();
    add_report(&mut mesh, report(11, MacAddr::ZERO));
    mesh.update_scan_result_list(ListOp::Insert, scan_in(&key), SCAN_RESULT_SELF_INDEX)
        .unwrap();
    assert_eq!(reports(&mesh).len(), 2);
    assert_eq!(mesh.delete_scan_result_list(), 1);
    assert_eq!(reports(&mesh), vec![(11, MacAddr::ZERO)]);
}

/// Two zero-BSSID self reports, for the results on channels 7 and 6; only channel 6 is stored
/// when the clear runs, so exactly the channel-7 report remains.
/// intentional-difference:d5-self-report-cleanup
#[test]
fn clearing_one_of_two_self_reports_leaves_exactly_one() {
    let (mut mesh, key) = station_with_scanner();
    let other = scan_key_in(GLOBAL, MAC1, mac(40), 81, 7, ScannerType::STA);
    mesh.update_scan_result_list(ListOp::Insert, scan_in(&other), SCAN_RESULT_SELF_INDEX)
        .unwrap();
    mesh.update_scan_result_list(ListOp::Insert, scan_in(&key), SCAN_RESULT_SELF_INDEX)
        .unwrap();
    let node = mesh.data_model_mut(&node_id(GLOBAL, MAC1)).unwrap();
    assert!(node.remove_scan_result(&other).is_some());
    assert_eq!(mesh.delete_scan_result_list(), 1);
    assert_eq!(reports(&mesh), vec![(7, MacAddr::ZERO)]);
}

/// The neighbor loop skips a zero BSSID: the zero report is the self cleanup's, removed once.
/// intentional-difference:d5-self-report-cleanup
#[test]
fn clearing_a_result_with_a_zero_bssid_neighbor_removes_one_zero_report() {
    let (mut mesh, key) = station_with_scanner();
    add_report(&mut mesh, report(11, MacAddr::ZERO));
    let mut result = scan_in(&key);
    result.neighbor.push(neighbor(MacAddr::ZERO)).unwrap();
    mesh.put_scan_result(MacAddr::ZERO, result, 0).unwrap();
    assert_eq!(reports(&mesh).len(), 2);
    assert_eq!(mesh.delete_scan_result_list(), 1);
    assert_eq!(reports(&mesh), vec![(11, MacAddr::ZERO)]);
}

// --- DM3-02: scan list indices ---

fn mesh_with_one_neighbor() -> (MeshModel, ScanKey) {
    let (mut mesh, key) = station_with_scanner();
    let mut result = scan_in(&key);
    result.neighbor.push(neighbor(mac(51))).unwrap();
    mesh.put_scan_result(mac(51), result, 0).unwrap();
    (mesh, key)
}

/// An index that is neither a neighbor nor the self row is an error for every operation, and
/// nothing changes.
#[test]
fn update_scan_result_list_rejects_an_index_that_is_not_a_neighbor() {
    for op in [ListOp::Insert, ListOp::Update, ListOp::Delete] {
        let (mut mesh, key) = mesh_with_one_neighbor();
        let result = scan_in(&key);
        let outcome = mesh.update_scan_result_list(op, result, 1);
        assert_eq!(
            outcome,
            Err(ModelError::NotFound {
                table: Table::Neighbor
            }),
            "{op:?}"
        );
        assert_eq!(num_scans(&mesh), 1, "{op:?}");
        assert_eq!(reports(&mesh).len(), 1, "{op:?}");
    }
}

/// The invalid delete of the Codex repro: an empty neighbor list and index 1.
#[test]
fn update_scan_result_list_invalid_delete_leaves_the_scan() {
    let (mut mesh, key) = station_with_scanner();
    mesh.put_scan_result(MacAddr::ZERO, scan_in(&key), 0)
        .unwrap();
    let outcome = mesh.update_scan_result_list(ListOp::Delete, scan_in(&key), 1);
    assert!(outcome.is_err());
    assert_eq!(num_scans(&mesh), 1);
}

/// A self row on an existing result writes no neighbor and appends the self report.
#[test]
fn self_row_insert_on_an_existing_result_appends_only_the_self_report() {
    let (mut mesh, key) = mesh_with_one_neighbor();
    let outcome =
        mesh.update_scan_result_list(ListOp::Insert, scan_in(&key), SCAN_RESULT_SELF_INDEX);
    assert!(outcome.is_ok(), "{outcome:?}");
    let node = mesh.get_data_model(&node_id(GLOBAL, MAC1)).unwrap();
    let stored = node.scan_result(0).unwrap();
    assert_eq!(stored.neighbor.len(), 1);
    assert_eq!(stored.neighbor.as_slice()[0].bssid, mac(51));
    assert_eq!(
        reports(&mesh),
        vec![(6, mac(51)), (6, MacAddr::ZERO)],
        "one zero-BSSID report was appended"
    );
}

// --- Rev 3.1 round 2: DM3-07, ADV4-01, ADV4-02, ADV4-03 ---

/// Codex repro (DM3-07): a station has a channel-11 report for BSSID X, and a channel-6 scan with
/// neighbor X adds a second one; the clear removes the channel-6 report and keeps channel 11.
/// intentional-difference:d1-storage-route
#[test]
fn clearing_scan_results_removes_the_neighbor_report_of_its_own_channel() {
    let (mut mesh, key) = station_with_scanner();
    add_report(&mut mesh, report(11, mac(51)));
    let mut result = scan_in(&key);
    result.neighbor.push(neighbor(mac(51))).unwrap();
    mesh.put_scan_result(mac(51), result, 0).unwrap();
    assert_eq!(reports(&mesh), vec![(11, mac(51)), (6, mac(51))]);
    assert_eq!(mesh.delete_scan_result_list(), 1);
    assert_eq!(reports(&mesh), vec![(11, mac(51))]);
}

/// ADV4-01: a self delete of channel 6 removes the channel-6 self report, not the first zero
/// report; the later clear then leaves no report.
/// intentional-difference:d5-self-report-cleanup
#[test]
fn self_index_delete_removes_the_self_report_of_its_own_channel() {
    let (mut mesh, key) = station_with_scanner();
    let other = scan_key_in(GLOBAL, MAC1, mac(40), 81, 7, ScannerType::STA);
    mesh.update_scan_result_list(ListOp::Insert, scan_in(&other), SCAN_RESULT_SELF_INDEX)
        .unwrap();
    mesh.update_scan_result_list(ListOp::Insert, scan_in(&key), SCAN_RESULT_SELF_INDEX)
        .unwrap();
    mesh.update_scan_result_list(ListOp::Delete, scan_in(&key), SCAN_RESULT_SELF_INDEX)
        .unwrap();
    assert_eq!(reports(&mesh), vec![(7, MacAddr::ZERO)]);
    assert_eq!(mesh.delete_scan_result_list(), 1);
    assert_eq!(reports(&mesh), vec![]);
}

/// ADV4-02: the self report is matched by op class too; a class-115 zero report on the same
/// channel stays.
/// intentional-difference:d5-self-report-cleanup
#[test]
fn clearing_scan_results_keeps_a_self_report_of_another_op_class() {
    let (mut mesh, key) = station_with_scanner();
    let mut other_class = report(6, MacAddr::ZERO);
    other_class.op_class = 115;
    add_report(&mut mesh, other_class);
    mesh.update_scan_result_list(ListOp::Insert, scan_in(&key), SCAN_RESULT_SELF_INDEX)
        .unwrap();
    assert_eq!(mesh.delete_scan_result_list(), 1);
    let node = mesh.get_data_model(&node_id(GLOBAL, MAC1)).unwrap();
    let remaining: Vec<u8> = node
        .sta_rows(Consolidated)
        .flat_map(|row| row.beacon_reports.iter())
        .map(|report| report.op_class)
        .collect();
    assert_eq!(remaining, vec![115]);
}

/// ADV4-03: the single-row removal matches by BSSID alone and removes the first report
/// (`dm_easy_mesh_list.cpp:1503-1511`).
#[test]
fn remove_scan_result_removes_the_first_report_with_the_bssid() {
    let (mut mesh, key) = station_with_scanner();
    add_report(&mut mesh, report(1, mac(51)));
    add_report(&mut mesh, report(6, mac(51)));
    mesh.remove_scan_result(&key, mac(51));
    assert_eq!(reports(&mesh), vec![(6, mac(51))]);
}
