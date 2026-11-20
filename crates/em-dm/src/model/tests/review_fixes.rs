// SPDX-License-Identifier: Apache-2.0

//! Regression tests of review round 1 (R1-R10): each one fails on the code it was written for.

use super::fixtures::{
    self, affiliated_ap, agent_model, ap_mld, bss, mac, mac2, op_class, radio, radio_cap,
    scan_result, sta,
};
use super::mesh_support::{
    MAC1, MAC2, MAC3, bss_in, init, node_id, radio_in, scan_in, scan_key_in, zeroed_network,
};
use crate::bounded::BoundedVec;
use crate::enums::{HaulType, OpClassType, ScannerType, VapMode};
use crate::model::access::ModelAccess;
use crate::model::dirty::DbCfgType;
use crate::model::error::{ModelError, Put, Table};
use crate::model::keys::{NetId, NodeId, StaKey};
use crate::model::node::StaMapTarget::{Assoc, Consolidated};
use crate::model::node::{FrameBody, MAX_RADIOS, NodeRole};
use crate::records::{AffiliatedStaInfo, AssocStaMldInfo};
use crate::{
    CsaChannel, DeltaOutcome, ListOp, MeshModel, ModelDelta, Neighbor, SkipReason, SkippedRow,
    StaCapUpdate,
};
use em_types::{MacAddr, ProfileType};

fn agent_in(network: &str, al: MacAddr) -> crate::NodeInit {
    init(
        network,
        al,
        ProfileType::MULTI_AP_PROFILE_3,
        NodeRole::Agent,
    )
}

fn mesh_with_agent() -> (MeshModel, NodeId) {
    let mut mesh = MeshModel::new(None);
    mesh.create_data_model(agent_in("OneWifiMesh", MAC1))
        .unwrap();
    (mesh, node_id("OneWifiMesh", MAC1))
}

fn apply_to_mesh(mesh: &mut MeshModel, id: &NodeId, delta: ModelDelta) -> DeltaOutcome {
    delta.apply(&mut mesh.node_ctx(id).unwrap())
}

fn apply_to_agent(model: &mut crate::AgentModel, delta: ModelDelta) -> DeltaOutcome {
    delta.apply(&mut model.ctx())
}

fn skipped(table: Table, index: usize, reason: SkipReason) -> SkippedRow {
    SkippedRow {
        table,
        index,
        reason,
    }
}

// --- R1: walks over array-backed tables end when two rows share a key ---

/// Visits every BSS with `first_bss` / `next_bss` and returns the positions. Stops after
/// `limit` steps so that a looping walk fails the test instead of hanging it.
fn bss_walk(mesh: &MeshModel, limit: usize) -> Vec<(NodeId, usize)> {
    let mut visited = Vec::new();
    let mut cursor = mesh.first_bss();
    while let Some(row) = cursor {
        visited.push((row.at, row.index));
        if visited.len() >= limit {
            break;
        }
        cursor = mesh.next_bss(&row.at, row.index);
    }
    visited
}

fn op_class_walk(mesh: &MeshModel, limit: usize) -> Vec<(NodeId, usize)> {
    let mut visited = Vec::new();
    let mut cursor = mesh.first_op_class();
    while let Some(row) = cursor {
        visited.push((row.at, row.index));
        if visited.len() >= limit {
            break;
        }
        cursor = mesh.next_op_class(&row.at, row.index);
    }
    visited
}

/// `commit_bss` matches the BSSID alone (`dm_easy_mesh.cpp:285-319`): committing a BACKHAUL BSS
/// over a stored FRONTHAUL one with the same BSSID leaves two rows with the same `BssKey` when
/// the first match was not the row that carries that key.
#[test]
fn the_bss_walk_visits_each_row_once_when_commit_bss_leaves_two_rows_with_one_key() {
    let (mut mesh, id) = mesh_with_agent();
    let radio_mac = mac(0x10);
    let bssid = mac(0x20);
    mesh.put_bss(bss_in(
        "OneWifiMesh",
        MAC1,
        radio_mac,
        bssid,
        HaulType::FRONTHAUL,
    ))
    .unwrap();
    mesh.put_bss(bss_in(
        "OneWifiMesh",
        MAC1,
        radio_mac,
        bssid,
        HaulType::BACKHAUL,
    ))
    .unwrap();
    let delta = ModelDelta::EmInsertBss {
        radio: radio_mac,
        bss: vec![bss_in(
            "OneWifiMesh",
            MAC1,
            radio_mac,
            bssid,
            HaulType::BACKHAUL,
        )],
        ap_mld: vec![],
    };
    apply_to_mesh(&mut mesh, &id, delta);

    let stored = mesh.get_data_model(&id).unwrap().bss_list();
    assert_eq!(stored.len(), 2);
    assert_eq!(
        crate::BssKey::of(&stored[0]),
        crate::BssKey::of(&stored[1]),
        "the delta left two rows with one key"
    );
    assert_eq!(bss_walk(&mesh, 10), vec![(id, 0), (id, 1)]);
}

/// `commit_op_class` matches `(op_class, kind)` (`dm_easy_mesh.cpp:257-259`): the op class of
/// radio 2 replaces the first stored class with that number and kind, which is radio 1's, and
/// the table then holds two rows with the key of radio 2.
#[test]
fn the_op_class_walk_visits_each_row_once_when_em_insert_radio_leaves_two_rows_with_one_key() {
    let (mut mesh, id) = mesh_with_agent();
    let (radio_1, radio_2) = (mac(0x11), mac(0x12));
    {
        let node = mesh.data_model_mut(&id).unwrap();
        node.put_radio(radio(radio_2, true)).unwrap();
        node.put_op_class(op_class(radio_1, OpClassType::CAPABILITY, 115))
            .unwrap();
        node.put_op_class(op_class(radio_2, OpClassType::CAPABILITY, 115))
            .unwrap();
    }
    let delta = ModelDelta::EmInsertRadio {
        radio: radio(radio_2, true),
        cap: None,
        op_classes: vec![op_class(radio_2, OpClassType::CAPABILITY, 115)],
    };
    apply_to_mesh(&mut mesh, &id, delta);

    let total = mesh.get_data_model(&id).unwrap().op_classes().len();
    let keys: Vec<_> = mesh
        .get_data_model(&id)
        .unwrap()
        .op_classes()
        .iter()
        .map(crate::OpClassKey::of)
        .collect();
    let radio_2_key = crate::OpClassKey::of(&op_class(radio_2, OpClassType::CAPABILITY, 115));
    let with_that_key = keys.iter().filter(|key| **key == radio_2_key).count();
    assert_eq!(with_that_key, 2, "the delta left two rows with one key");
    let expected: Vec<_> = (0..total).map(|index| (id, index)).collect();
    assert_eq!(op_class_walk(&mesh, total + 5), expected);
}

/// `update_cac_status_id` gives every class above `capability` the AL MAC as `ruid`
/// (`dm_easy_mesh.cpp:1682-1699`), so two CAC rows of the same class collide.
#[test]
fn the_op_class_walk_visits_each_row_once_when_update_cac_status_id_makes_two_keys_equal() {
    let (mut mesh, id) = mesh_with_agent();
    {
        let node = mesh.data_model_mut(&id).unwrap();
        node.put_op_class(op_class(mac(0x11), OpClassType::CAC_AVAILABLE, 121))
            .unwrap();
        node.put_op_class(op_class(mac(0x12), OpClassType::CAC_AVAILABLE, 121))
            .unwrap();
        node.update_cac_status_id(MAC1);
    }
    let node = mesh.get_data_model(&id).unwrap();
    let cac: Vec<_> = node
        .op_classes()
        .iter()
        .filter(|info| info.id.kind == OpClassType::CAC_AVAILABLE)
        .map(crate::OpClassKey::of)
        .collect();
    assert_eq!(cac.len(), 2);
    assert_eq!(cac[0], cac[1], "the rewrite made the keys equal");
    let total = node.op_classes().len();
    let expected: Vec<_> = (0..total).map(|index| (id, index)).collect();
    assert_eq!(op_class_walk(&mesh, total + 5), expected);
}

/// The agent-scoped BSS walk is positional too.
#[test]
fn the_per_agent_bss_walk_ends_over_duplicate_keys() {
    let mut mesh = MeshModel::new(None);
    mesh.create_data_model(agent_in(NetId::GLOBAL, MAC1))
        .unwrap();
    let id = node_id(NetId::GLOBAL, MAC1);
    let (radio_mac, bssid) = (mac(0x10), mac(0x20));
    mesh.put_bss(bss_in(
        NetId::GLOBAL,
        MAC1,
        radio_mac,
        bssid,
        HaulType::FRONTHAUL,
    ))
    .unwrap();
    mesh.put_bss(bss_in(
        NetId::GLOBAL,
        MAC1,
        radio_mac,
        bssid,
        HaulType::BACKHAUL,
    ))
    .unwrap();
    let delta = ModelDelta::EmInsertBss {
        radio: radio_mac,
        bss: vec![bss_in(
            NetId::GLOBAL,
            MAC1,
            radio_mac,
            bssid,
            HaulType::BACKHAUL,
        )],
        ap_mld: vec![],
    };
    apply_to_mesh(&mut mesh, &id, delta);

    let first = mesh.first_bss_of_agent(MAC1).unwrap();
    assert_eq!(first.index, 0);
    let second = mesh.next_bss_of_agent(MAC1, first.index).unwrap();
    assert_eq!(second.index, 1);
    assert!(mesh.next_bss_of_agent(MAC1, second.index).is_none());
}

#[test]
fn the_pre_set_op_class_walk_visits_each_row_once_over_duplicate_keys() {
    let (mut mesh, id) = mesh_with_agent();
    let node = mesh.data_model_mut(&id).unwrap();
    node.put_op_class(op_class(mac(0x11), OpClassType::CAC_AVAILABLE, 121))
        .unwrap();
    node.put_op_class(op_class(mac(0x12), OpClassType::CAC_AVAILABLE, 121))
        .unwrap();
    node.update_cac_status_id(MAC1);
    let mut visited = Vec::new();
    let mut cursor = mesh.first_pre_set_op_class_by_type(OpClassType::CAC_AVAILABLE);
    while let Some(row) = cursor {
        visited.push(row.index);
        assert!(visited.len() < 10, "the walk does not end");
        cursor = mesh.next_pre_set_op_class_by_type(OpClassType::CAC_AVAILABLE, row.index);
    }
    assert_eq!(visited.len(), 2);
}

// --- R2: the list entry point returns the node needed ---

/// Upstream's database path reaches `create_node` through `update_list` -> `put_radio`
/// (`dm_radio_list.cpp:146-155`, `dm_easy_mesh_list.cpp:470`).
#[test]
fn the_radio_list_insert_returns_the_node_needed_and_the_policy_seeds() {
    let (mut mesh, id) = mesh_with_agent();
    let outcome = mesh
        .update_radio_list(ListOp::Insert, radio_in("OneWifiMesh", MAC1, mac(9)))
        .unwrap()
        .expect("an insert returns the outcome of put_radio");
    assert_eq!(outcome.node_needed.node, id);
    assert_eq!(outcome.node_needed.radio, mac(9));
    assert_eq!(outcome.put.put, Put::Inserted);
    assert_eq!(outcome.put.policies_rejected, 0);

    let update = mesh
        .update_radio_list(ListOp::Update, radio_in("OneWifiMesh", MAC1, mac(9)))
        .unwrap();
    assert!(update.is_none());
    let delete = mesh
        .update_radio_list(ListOp::Delete, radio_in("OneWifiMesh", MAC1, mac(9)))
        .unwrap();
    assert!(delete.is_none());
}

// --- R3: the capability table keeps index parity ---

fn cap_macs(node: &crate::NodeModel) -> Vec<MacAddr> {
    node.radio_caps().iter().map(|cap| cap.ruid.mac).collect()
}

/// `commit_radio(A, None)` stores a placeholder at slot 0; the capability of A then takes that
/// slot, so the later commit of B does not overwrite it.
#[test]
fn a_capability_put_after_commit_radio_fills_the_placeholder_and_survives_the_next_commit() {
    let mut node = fixtures::agent_node();
    let (a, b) = (mac(0x50), mac(0x51));
    node.commit_radio(radio(a, true), None).unwrap();
    assert_eq!(node.put_radio_cap(radio_cap(a)), Ok(Put::Replaced));
    assert_eq!(cap_macs(&node), vec![a]);
    node.commit_radio(radio(b, true), None).unwrap();
    assert!(node.radio_cap(a).is_some(), "the capability of A is kept");
    assert_eq!(node.radio_cap_at(0).unwrap().ruid.mac, a);
    assert!(node.radio_cap_at(1).unwrap().ruid.mac.is_zero());
}

/// Radios put without a capability slot and a third committed with one: the capability of the
/// first radio goes to slot 0 instead of failing with `Full`.
#[test]
fn a_capability_put_for_a_stored_radio_writes_its_slot_when_the_table_is_padded() {
    let mut node = fixtures::agent_node();
    let (a, b, c) = (mac(0x60), mac(0x61), mac(0x62));
    node.put_radio(radio(a, true)).unwrap();
    node.put_radio(radio(b, true)).unwrap();
    node.commit_radio(radio(c, true), Some(radio_cap(c)))
        .unwrap();
    assert_eq!(cap_macs(&node), vec![MacAddr::ZERO, MacAddr::ZERO, c]);
    assert_eq!(node.put_radio_cap(radio_cap(a)), Ok(Put::Replaced));
    assert_eq!(node.put_radio_cap(radio_cap(b)), Ok(Put::Replaced));
    assert_eq!(cap_macs(&node), vec![a, b, c]);
    assert_eq!(node.radio_caps().len(), MAX_RADIOS);
}

/// The rule for a capability whose radio is not stored: it is refused and nothing changes.
#[test]
fn a_capability_for_a_radio_that_is_not_stored_is_refused() {
    let mut node = fixtures::agent_node();
    node.commit_radio(radio(mac(0x50), true), None).unwrap();
    assert_eq!(
        node.put_radio_cap(radio_cap(mac(0x77))),
        Err(ModelError::NotFound {
            table: Table::Radio
        })
    );
    assert_eq!(cap_macs(&node), vec![MacAddr::ZERO]);
}

// --- R4: every disassoc entry is applied or skipped, once ---

#[allow(clippy::vec_box, reason = "the delta's rows are boxed (0007 D3)")]
fn aggregate(assoc: Vec<Box<crate::StaInfo>>, disassoc: Vec<StaKey>) -> ModelDelta {
    ModelDelta::StaAggregate { assoc, disassoc }
}

fn sta_key(station: u8) -> StaKey {
    StaKey {
        sta: mac(station),
        bssid: mac(0x41),
        radio: mac(0x42),
    }
}

#[test]
fn a_key_in_both_aggregate_lists_is_added_then_removed_and_not_skipped() {
    let mut model = agent_model();
    let key = sta_key(0x40);
    let outcome = apply_to_agent(
        &mut model,
        aggregate(vec![sta(key.sta, key.bssid, key.radio)], vec![key]),
    );
    assert_eq!(outcome.applied, 2, "one insert and one removal");
    assert_eq!(outcome.skipped.len(), 0);
    assert!(model.node_model().sta_info(&key, Consolidated).is_none());
}

#[test]
fn a_duplicate_disassoc_key_is_removed_once_and_skipped_once() {
    let mut model = agent_model();
    let key = sta_key(0x40);
    model.ctx().node.put_sta(sta(key.sta, key.bssid, key.radio));
    let outcome = apply_to_agent(&mut model, aggregate(vec![], vec![key, key]));
    assert_eq!(outcome.applied, 1);
    assert_eq!(
        outcome.skipped,
        vec![skipped(
            Table::Sta,
            1,
            SkipReason::Rejected(ModelError::NotFound { table: Table::Sta })
        )]
    );
}

#[test]
fn a_missing_disassoc_key_is_skipped_at_its_position() {
    let mut model = agent_model();
    let present = sta_key(0x40);
    let missing = sta_key(0x41);
    model
        .ctx()
        .node
        .put_sta(sta(present.sta, present.bssid, present.radio));
    let outcome = apply_to_agent(&mut model, aggregate(vec![], vec![present, missing]));
    assert_eq!(outcome.applied, 1);
    assert_eq!(
        outcome.skipped,
        vec![skipped(
            Table::Sta,
            1,
            SkipReason::Rejected(ModelError::NotFound { table: Table::Sta })
        )]
    );
}

// --- R5: NodeRef::id is the storage key ---

#[test]
fn node_ids_survive_a_rewrite_of_the_al_mac_through_the_context() {
    let mut mesh = MeshModel::new(None);
    mesh.create_data_model(agent_in("NetA", MAC1)).unwrap();
    mesh.create_data_model(agent_in("NetA", MAC2)).unwrap();
    let rewritten = node_id("NetA", MAC1);
    mesh.node_ctx(&rewritten)
        .unwrap()
        .set_ctrl_al_interface_mac(MAC3);
    assert_eq!(
        mesh.node(&rewritten).unwrap().node.agent_al_interface_mac(),
        MAC3
    );

    let ids: Vec<NodeId> = mesh.nodes().map(|node| node.id()).collect();
    assert_eq!(ids, vec![node_id("NetA", MAC1), node_id("NetA", MAC2)]);
    for id in &ids {
        assert_eq!(mesh.node(id).unwrap().id(), *id);
    }
    assert_eq!(mesh.node_ctx(&rewritten).unwrap().id, rewritten);
}

#[test]
fn the_agent_node_id_survives_a_rewrite_of_the_al_mac() {
    let mut model = agent_model();
    let before = model.nodes().next().unwrap().id();
    model.ctx().set_ctrl_al_interface_mac(MAC3);
    model.ctx().node.set_agent_al_interface_mac(MAC2);
    let after = model.nodes().next().unwrap().id();
    assert_eq!(after, before);
    assert_eq!(model.node(&after).unwrap().id(), before);
}

// --- R6: the colocated-agent mark goes on the controller of the same network ---

#[test]
fn a_colocated_agent_marks_the_controller_of_its_own_network_not_the_global_one() {
    let mut mesh = MeshModel::new(None);
    let mut global = zeroed_network();
    global.ctrl_id.mac = MAC1;
    mesh.put_network(NetId::global(), global).unwrap();
    let mut net_a = zeroed_network();
    net_a.ctrl_id.mac = MAC2;
    mesh.put_network(fixtures::net("NetA"), net_a).unwrap();
    let global_controller = node_id(NetId::GLOBAL, MAC1);
    let net_a_controller = node_id("NetA", MAC2);
    let marked = |mesh: &MeshModel, id: &NodeId| {
        mesh.get_data_model(id)
            .unwrap()
            .dirty()
            .is_set(DbCfgType::NetworkListUpdate)
    };
    let global_before = marked(&mesh, &global_controller);
    assert!(!marked(&mesh, &net_a_controller));

    let mut colocated = agent_in("NetA", MAC3);
    colocated.colocated = true;
    mesh.create_data_model(colocated).unwrap();

    assert_eq!(
        mesh.get_network(&fixtures::net("NetA"))
            .unwrap()
            .colocated_agent_interface()
            .mac,
        MAC3
    );
    assert!(marked(&mesh, &net_a_controller));
    assert_eq!(marked(&mesh, &global_controller), global_before);
}

// --- R7: affiliated APs skipped at capacity are reported ---

#[test]
fn em_insert_bss_reports_each_affiliated_ap_skipped_at_capacity() {
    let mut model = agent_model();
    let full_list: Vec<_> = (0..64_u8)
        .map(|index| affiliated_ap(mac(index), mac2(1, index), index))
        .collect();
    model
        .ctx()
        .node
        .update_ap_mld_info(ap_mld(HaulType::FRONTHAUL, mac(40), full_list))
        .unwrap();

    // One AP that matches a stored one (still updated), one new AP (no room), one more new AP.
    let incoming = vec![
        affiliated_ap(mac(5), mac2(1, 5), 5),
        affiliated_ap(mac(200), mac(201), 0),
        affiliated_ap(mac(202), mac(203), 1),
    ];
    let outcome = apply_to_agent(
        &mut model,
        ModelDelta::EmInsertBss {
            radio: mac(10),
            bss: vec![],
            ap_mld: vec![ap_mld(HaulType::FRONTHAUL, mac(40), incoming)],
        },
    );
    assert_eq!(
        outcome.applied, 2,
        "the MLD row and the matched affiliated AP are written"
    );
    assert_eq!(
        outcome.skipped,
        vec![
            skipped(
                Table::AffiliatedAp,
                1,
                SkipReason::AffiliatedApFull { ap_mld: 0 }
            ),
            skipped(
                Table::AffiliatedAp,
                2,
                SkipReason::AffiliatedApFull { ap_mld: 0 }
            ),
        ]
    );
    assert_eq!(
        model.node_model().ap_mld(0).unwrap().affiliated_ap.len(),
        64
    );
}

// --- R8: a scan result without neighbors is deleted too ---

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

#[test]
fn delete_scan_result_list_removes_results_with_and_without_neighbors() {
    let (mut mesh, id) = mesh_with_agent();
    let scanner = mac(0x30);
    let empty_key = scan_key_in("OneWifiMesh", MAC1, scanner, 81, 6, ScannerType::RADIO);
    let busy_key = scan_key_in("OneWifiMesh", MAC1, scanner, 115, 36, ScannerType::RADIO);
    mesh.put_scan_result(MacAddr::ZERO, scan_in(&empty_key), 0)
        .unwrap();
    let mut busy = scan_result(&busy_key);
    busy.neighbor.push(neighbor(mac(0x31))).unwrap();
    busy.neighbor.push(neighbor(mac(0x32))).unwrap();
    mesh.put_scan_result(mac(0x31), busy, 0).unwrap();
    assert_eq!(mesh.get_data_model(&id).unwrap().num_scan_results(), 2);

    assert_eq!(mesh.delete_scan_result_list(), 2);
    assert_eq!(mesh.get_data_model(&id).unwrap().num_scan_results(), 0);
    assert_eq!(mesh.delete_scan_result_list(), 0);
}

// --- R9: gate mutants ---

fn frame_of(bytes: &[u8]) -> FrameBody {
    BoundedVec::try_from_vec(bytes.to_vec()).unwrap()
}

fn links(keys: &[StaKey]) -> BoundedVec<StaKey, 64> {
    BoundedVec::try_from_vec(keys.to_vec()).unwrap()
}

/// An association request: 4 fixed bytes, an SSID element "mesh" and a Supported Rates element.
const ASSOC_FRAME: [u8; 14] = [
    0x01, 0x04, 0x0a, 0x00, 0x00, 0x04, b'm', b'e', b's', b'h', 0x01, 0x02, 0x82, 0x84,
];

/// `apply_sta_cap_to_maps` re-derives the IE text fields of the consolidated row from the new
/// frame (`dm_easy_mesh.cpp:4119-4123`, `decode_sta_capability`).
#[test]
fn sta_cap_update_re_derives_the_ie_text_of_the_consolidated_row() {
    let mut model = agent_model();
    let key = sta_key(0x40);
    let mut existing = sta(key.sta, key.bssid, key.radio);
    existing.ssid = crate::CStrBytes::new(b"stale").unwrap();
    existing.last_dl_rate = 7;
    model.ctx().node.put_sta(existing);
    let update = StaCapUpdate::new(links(&[key]), frame_of(&ASSOC_FRAME));
    apply_to_agent(&mut model, ModelDelta::StaCapUpdate(update));
    let stored = model.node_model().sta_info(&key, Consolidated).unwrap();
    assert_eq!(stored.ssid.as_bytes(), b"mesh");
    assert_eq!(stored.supp_rates.as_bytes(), b"8284");
    assert_eq!(stored.frame_body.as_slice(), ASSOC_FRAME);
    assert_eq!(stored.last_dl_rate, 7);
}

fn two_radio_node() -> crate::NodeModel {
    let mut node = fixtures::agent_node();
    for (radio_last, bssid_last) in [(10, 20), (11, 21)] {
        node.put_radio(radio(mac(radio_last), true)).unwrap();
        node.put_bss(bss(
            mac(radio_last),
            mac(bssid_last),
            HaulType::FRONTHAUL,
            VapMode::AP,
        ))
        .unwrap();
    }
    node
}

/// `apply_sta_association_row`: the pending row becomes a copy of the whole consolidated row,
/// not just its `associated` flag (`dm_easy_mesh.cpp:3958-3978`).
#[test]
fn a_known_station_disassociation_copies_the_whole_consolidated_row_over_the_pending_one() {
    let mut node = two_radio_node();
    let key = StaKey {
        sta: mac(30),
        bssid: mac(20),
        radio: mac(10),
    };
    let mut consolidated = sta(key.sta, key.bssid, key.radio);
    consolidated.associated = true;
    consolidated.rcpi = 33;
    consolidated.last_dl_rate = 1234;
    consolidated.ssid = crate::CStrBytes::new(b"consolidated").unwrap();
    consolidated.set_frame_body(vec![4, 5, 6]).unwrap();
    node.put_sta(consolidated);
    let mut pending = sta(key.sta, key.bssid, key.radio);
    pending.rcpi = 1;
    pending.last_dl_rate = 2;
    pending.ssid = crate::CStrBytes::new(b"pending").unwrap();
    node.put_sta_info(pending, Assoc);

    node.apply_sta_disassociation(key.sta, key.bssid);

    let stored = node.sta_info(&key, Consolidated).unwrap();
    assert!(!stored.associated);
    let staged = node.sta_info(&key, Assoc).unwrap();
    assert_eq!(staged, stored, "the whole row, not only the flag");
    assert_eq!(staged.rcpi, 33);
    assert_eq!(staged.last_dl_rate, 1234);
    assert_eq!(staged.ssid.as_bytes(), b"consolidated");
    assert_eq!(staged.frame_body.as_slice(), &[4, 5, 6]);
}

fn mld_info(station: MacAddr, link_bssids: &[u8]) -> AssocStaMldInfo {
    let affiliated: Vec<_> = link_bssids
        .iter()
        .map(|&bssid| AffiliatedStaInfo {
            bssid: mac(bssid),
            link_addr: mac2(bssid, 1),
        })
        .collect();
    AssocStaMldInfo {
        mac_addr: station,
        ap_mld_mac_addr: mac(60),
        str: false,
        nstr: false,
        emlsr: false,
        emlmr: false,
        affiliated_sta: BoundedVec::try_from_vec(affiliated).unwrap(),
    }
}

/// A pending row that has its own frame keeps it: the cached frame of the station fills only a
/// pending row without one (`dm_easy_mesh.cpp:4036-4041`).
#[test]
fn an_mld_association_keeps_the_own_frame_of_a_pending_row() {
    let mut node = two_radio_node();
    let station = mac(30);
    // The pending map is searched in key order, so the row on BSSID 19 supplies the cached
    // frame; the row on BSSID 20 has its own frame and must keep it.
    let mut cached_row = sta(station, mac(19), mac(10));
    cached_row.set_frame_body(vec![1, 1]).unwrap();
    node.put_sta_info(cached_row, Assoc);
    let mut own_row = sta(station, mac(20), mac(10));
    own_row.set_frame_body(vec![2, 2, 2]).unwrap();
    node.put_sta_info(own_row, Assoc);

    assert!(node.apply_sta_mld_association(&mld_info(station, &[20])));
    let key = StaKey {
        sta: station,
        bssid: mac(20),
        radio: mac(10),
    };
    let staged = node.sta_info(&key, Assoc).unwrap();
    assert!(staged.associated);
    assert_eq!(staged.frame_body.as_slice(), &[2, 2, 2]);
    // A pending row without a frame still takes the cached one.
    assert!(node.apply_sta_mld_association(&mld_info(station, &[21])));
    let key = StaKey {
        sta: station,
        bssid: mac(21),
        radio: mac(11),
    };
    assert_eq!(
        node.sta_info(&key, Assoc).unwrap().frame_body.as_slice(),
        &[1, 1]
    );
}

/// Only the op class of the right radio and type changes.
#[test]
fn csa_channel_changes_only_the_op_class_of_the_right_type() {
    let mut model = agent_model();
    let radio_mac = mac(10);
    {
        let node = model.ctx().node;
        for (kind, number) in [
            (OpClassType::CAPABILITY, 115),
            (OpClassType::PREFERENCE, 115),
            (OpClassType::CURRENT, 115),
        ] {
            let mut info = op_class(radio_mac, kind, number);
            info.channel = 36;
            node.put_op_class(info).unwrap();
        }
    }
    let outcome = apply_to_agent(
        &mut model,
        ModelDelta::CsaChannel(CsaChannel {
            ruid: radio_mac,
            kind: OpClassType::CURRENT,
            channel: 149,
        }),
    );
    assert_eq!(outcome.applied, 1);
    let channels: Vec<(OpClassType, u32)> = model
        .node_model()
        .op_classes()
        .iter()
        .filter(|info| info.id.ruid == radio_mac)
        .map(|info| (info.id.kind, info.channel))
        .collect();
    assert_eq!(
        channels,
        vec![
            (OpClassType::CAPABILITY, 36),
            (OpClassType::PREFERENCE, 36),
            (OpClassType::CURRENT, 149),
        ]
    );
}

// --- R10: frames are moved, not copied (gate A9) ---

fn frame_ptr(info: &crate::StaInfo) -> *const u8 {
    info.frame_body.as_slice().as_ptr()
}

#[test]
fn put_sta_moves_the_frame_into_the_stored_row() {
    let mut node = fixtures::agent_node();
    let mut first = sta(mac(30), mac(20), mac(10));
    first.set_frame_body(vec![1; 64]).unwrap();
    let before = frame_ptr(&first);
    assert_eq!(node.put_sta(first), Put::Inserted);
    let key = sta_key_of(30, 20, 10);
    assert_eq!(
        frame_ptr(node.sta_info(&key, Consolidated).unwrap()),
        before
    );

    // The replace path moves the incoming frame in as well.
    let mut second = sta(mac(30), mac(20), mac(10));
    second.set_frame_body(vec![2; 64]).unwrap();
    let before = frame_ptr(&second);
    assert_eq!(node.put_sta(second), Put::Replaced);
    assert_eq!(
        frame_ptr(node.sta_info(&key, Consolidated).unwrap()),
        before
    );
}

#[test]
fn a_sta_aggregate_assoc_row_moves_its_frame_into_the_consolidated_map() {
    let mut model = agent_model();
    let mut row = sta(mac(30), mac(20), mac(10));
    row.set_frame_body(vec![3; 64]).unwrap();
    let before = frame_ptr(&row);
    let outcome = apply_to_agent(&mut model, aggregate(vec![row], vec![]));
    assert_eq!(outcome.applied, 1);
    let key = sta_key_of(30, 20, 10);
    assert_eq!(
        frame_ptr(model.node_model().sta_info(&key, Consolidated).unwrap()),
        before
    );
}

fn sta_key_of(station: u8, bssid: u8, radio_mac: u8) -> StaKey {
    StaKey {
        sta: mac(station),
        bssid: mac(bssid),
        radio: mac(radio_mac),
    }
}
