// SPDX-License-Identifier: Apache-2.0

//! `ModelDelta::apply` for every variant: what lands, what is skipped and why (plan C1 row
//! rule), and the frame copies of `StaCapUpdate` (plan C2, F1).

use super::fixtures::{
    affiliated_ap, agent_model, ap_mld, bss, device, mac, mac2, op_class, policy, radio, radio_cap,
    scan_key, scan_result, sta,
};
use crate::bounded::BoundedVec;
use crate::enums::{HaulType, OpClassType, PolicyIdType, ScannerType, VapMode};
use crate::model::delta::{
    CsaChannel, DeltaOutcome, ModelDelta, SkipReason, SkippedRow, StaCapUpdate,
};
use crate::model::error::{ModelError, Table};
use crate::model::keys::{NetId, OpClassKey, StaKey};
use crate::model::node::StaMapTarget::{Assoc, Consolidated};
use crate::model::node::{FrameBody, MAX_BSS, MAX_OP_CLASSES, MAX_POLICIES, MAX_RADIOS};
use crate::records::{AffiliatedStaInfo, AssocStaMldInfo};
use crate::{AgentModel, NetworkInfo, StaInfo, UnassocStaMetricsRsp};

fn key(station: u8, bssid: u8, radio_mac: u8) -> StaKey {
    StaKey {
        sta: mac(station),
        bssid: mac(bssid),
        radio: mac(radio_mac),
    }
}

fn row(station: u8, bssid: u8, radio_mac: u8) -> Box<StaInfo> {
    sta(mac(station), mac(bssid), mac(radio_mac))
}

fn skipped(table: Table, index: usize, reason: SkipReason) -> SkippedRow {
    SkippedRow {
        table,
        index,
        reason,
    }
}

fn not_found(table: Table) -> SkipReason {
    SkipReason::Rejected(ModelError::NotFound { table })
}

fn full(table: Table, capacity: usize) -> SkipReason {
    SkipReason::Rejected(ModelError::Full { table, capacity })
}

fn apply(model: &mut AgentModel, delta: ModelDelta) -> DeltaOutcome {
    delta.apply(&mut model.ctx())
}

// --- CsaChannel ---

#[test]
fn csa_channel_sets_the_channel_of_the_op_class_of_that_radio_and_type() {
    let mut model = agent_model();
    model
        .ctx()
        .node
        .put_op_class(op_class(mac(10), OpClassType::CURRENT, 115))
        .unwrap();
    let outcome = apply(
        &mut model,
        ModelDelta::CsaChannel(CsaChannel {
            ruid: mac(10),
            kind: OpClassType::CURRENT,
            channel: 149,
        }),
    );
    assert_eq!(outcome.applied, 1);
    assert_eq!(outcome.skipped.len(), 0);
    let key = OpClassKey {
        ruid: mac(10),
        kind: OpClassType::CURRENT,
        op_class: 115,
    };
    assert_eq!(
        model.node_model().find_op_class(&key).unwrap().channel,
        149,
        "the write lands in the model, not in a discarded copy"
    );
}

/// Upstream writes a discarded local copy (`src/agent/dm_easy_mesh_agent.cpp:672-674, 764`);
/// the delta writes the model. intentional-difference:c1-csa-lost-update
#[test]
fn csa_channel_for_an_unknown_radio_is_skipped_and_changes_nothing() {
    let mut model = agent_model();
    let outcome = apply(
        &mut model,
        ModelDelta::CsaChannel(CsaChannel {
            ruid: mac(10),
            kind: OpClassType::CURRENT,
            channel: 149,
        }),
    );
    assert_eq!(outcome.applied, 0);
    assert_eq!(
        outcome.skipped,
        vec![skipped(Table::OpClass, 0, not_found(Table::OpClass))]
    );
}

// --- StaLinkMetrics ---

#[test]
fn sta_link_metrics_replaces_existing_rows_and_skips_unknown_keys() {
    let mut model = agent_model();
    model.ctx().node.put_sta(row(30, 20, 10));
    let mut known = row(30, 20, 10);
    known.last_ul_rate = 7;
    let unknown = row(31, 20, 10);
    let outcome = apply(&mut model, ModelDelta::StaLinkMetrics(vec![unknown, known]));
    assert_eq!(outcome.applied, 1);
    assert_eq!(
        outcome.skipped,
        vec![skipped(Table::Sta, 0, not_found(Table::Sta))]
    );
    let node = model.node_model();
    assert_eq!(
        node.sta_info(&key(30, 20, 10), Consolidated)
            .unwrap()
            .last_ul_rate,
        7
    );
    assert!(
        node.sta_info(&key(31, 20, 10), Consolidated).is_none(),
        "not added"
    );
}

// --- AssocStaMldRefresh ---

fn assoc_mld(mld_mac: u8) -> AssocStaMldInfo {
    AssocStaMldInfo {
        mac_addr: mac(mld_mac),
        ap_mld_mac_addr: mac(60),
        str: false,
        nstr: false,
        emlsr: false,
        emlmr: false,
        affiliated_sta: BoundedVec::try_from_vec(vec![AffiliatedStaInfo {
            bssid: mac(20),
            link_addr: mac(21),
        }])
        .unwrap(),
    }
}

#[test]
fn assoc_sta_mld_refresh_adds_then_replaces() {
    let mut model = agent_model();
    let outcome = apply(&mut model, ModelDelta::AssocStaMldRefresh(assoc_mld(70)));
    assert_eq!(outcome.applied, 1);
    let mut again = assoc_mld(70);
    again.affiliated_sta = BoundedVec::new();
    let outcome = apply(&mut model, ModelDelta::AssocStaMldRefresh(again));
    assert_eq!(outcome.applied, 1);
    assert_eq!(model.node_model().assoc_sta_mlds().len(), 1);
    assert_eq!(
        model.node_model().assoc_sta_mlds()[0].affiliated_sta.len(),
        0
    );
}

/// `dm_easy_mesh.cpp:3947-3950`: the row is skipped at capacity.
#[test]
fn assoc_sta_mld_refresh_at_capacity_skips_the_row() {
    let mut model = agent_model();
    for index in 0..64_u8 {
        apply(&mut model, ModelDelta::AssocStaMldRefresh(assoc_mld(index)));
    }
    let outcome = apply(&mut model, ModelDelta::AssocStaMldRefresh(assoc_mld(200)));
    assert_eq!(outcome.applied, 0);
    assert_eq!(
        outcome.skipped,
        vec![skipped(Table::AssocStaMld, 0, full(Table::AssocStaMld, 64))]
    );
    assert_eq!(model.node_model().assoc_sta_mlds().len(), 64);
}

// --- StaAggregate ---

#[test]
fn sta_aggregate_moves_the_assoc_rows_in_and_removes_the_disassoc_keys() {
    let mut model = agent_model();
    model.ctx().node.put_sta(row(30, 20, 10));
    model.ctx().node.put_sta(row(31, 20, 10));
    let outcome = apply(
        &mut model,
        ModelDelta::StaAggregate {
            assoc: vec![row(32, 20, 10), row(30, 20, 10)],
            disassoc: vec![key(31, 20, 10)],
        },
    );
    assert_eq!(outcome.applied, 3, "one insert, one replace, one removal");
    assert_eq!(outcome.skipped.len(), 0);
    let node = model.node_model();
    assert_eq!(node.num_sta(Consolidated), 2);
    assert!(node.sta_info(&key(32, 20, 10), Consolidated).is_some());
    assert!(node.sta_info(&key(31, 20, 10), Consolidated).is_none());
}

#[test]
fn sta_aggregate_reports_a_disassociation_of_an_unknown_station() {
    let mut model = agent_model();
    model.ctx().node.put_sta(row(30, 20, 10));
    let outcome = apply(
        &mut model,
        ModelDelta::StaAggregate {
            assoc: vec![],
            disassoc: vec![key(99, 20, 10), key(30, 20, 10)],
        },
    );
    assert_eq!(outcome.applied, 1);
    assert_eq!(
        outcome.skipped,
        vec![skipped(Table::Sta, 0, not_found(Table::Sta))]
    );
    assert_eq!(model.node_model().num_sta(Consolidated), 0);
}

// --- UnassocResults ---

#[test]
fn unassoc_results_replace_the_previous_response() {
    let mut model = agent_model();
    let first = UnassocStaMetricsRsp {
        entry: BoundedVec::new(),
    };
    assert_eq!(
        apply(&mut model, ModelDelta::UnassocResults(first)).applied,
        1
    );
    assert_eq!(model.node_model().unassoc_sta_metrics_rsp().entry.len(), 0);
}

// --- ScanResults ---

#[test]
fn scan_results_go_through_update_scan_results_with_its_overrides() {
    let mut model = agent_model();
    let key = scan_key(mac(5), 115, 36, ScannerType(2));
    let outcome = apply(&mut model, ModelDelta::ScanResults(scan_result(&key)));
    assert_eq!(outcome.applied, 1);
    let stored = model.node_model().scan_result(0).unwrap();
    assert_eq!(stored.id.scanner_type, ScannerType::RADIO);
    assert_eq!(
        stored.id.dev_mac,
        model.node_model().agent_al_interface_mac()
    );
}

// --- AlInsert ---

#[test]
fn al_insert_keeps_the_emplus_flag_and_the_extended_info() {
    let mut model = agent_model();
    model.ctx().node.device_mut().is_emplus_agent = 1;
    model.ctx().node.device_mut().extended_info.boot_id = 77;
    let mut incoming = device(mac(1));
    incoming.uptime = 5;
    let outcome = apply(
        &mut model,
        ModelDelta::AlInsert {
            network: None,
            device: incoming,
        },
    );
    assert_eq!(outcome.applied, 1);
    let device = model.node_model().device();
    assert_eq!(device.uptime, 5);
    assert_eq!(device.is_emplus_agent, 1);
    assert_eq!(device.extended_info.boot_id, 77);
}

/// The `al` target also copies the network record (`dm_easy_mesh.cpp:217`); the write is counted
/// as its own applied row (plan D3).
#[test]
fn al_insert_with_a_network_replaces_the_network_record() {
    let mut model = agent_model();
    let mut record = Box::new(NetworkInfo::zeroed(NetId::global().as_text()));
    record.ctrl_id.mac = mac(9);
    let outcome = apply(
        &mut model,
        ModelDelta::AlInsert {
            network: Some(record),
            device: device(mac(1)),
        },
    );
    assert_eq!(model.network().controller_interface_mac(), mac(9));
    assert_eq!(outcome.applied, 2, "the network write and the device");
}

// --- EmInsertRadio (commit_config radio target, dm_easy_mesh.cpp:223-284) ---

// The per-path table of plan D3: radio, capability slot, report, count.

fn insert_radio(radio_mac: u8, cap: bool, op_classes: Vec<crate::OpClassInfo>) -> ModelDelta {
    ModelDelta::EmInsertRadio {
        radio: radio(mac(radio_mac), true),
        cap: cap.then(|| radio_cap(mac(radio_mac))),
        op_classes,
    }
}

#[test]
fn em_insert_radio_adds_a_new_radio_with_its_capability_at_the_same_index() {
    let mut model = agent_model();
    let outcome = apply(&mut model, insert_radio(10, true, vec![]));
    assert_eq!(outcome.applied, 2);
    let node = model.node_model();
    assert_eq!(node.radios().len(), 1);
    assert_eq!(node.radio_caps().len(), 1);
    assert_eq!(node.radio_cap_at(0).unwrap().ruid.mac, mac(10));
}

/// `m_radio_cap[m_num_radios].init()` (`dm_easy_mesh.cpp:240-247`): a new radio without a
/// capability record gets a zeroed default at its index, so the tables keep their parity.
#[test]
fn em_insert_radio_without_a_capability_stores_a_default_one_at_the_same_index() {
    let mut model = agent_model();
    apply(&mut model, insert_radio(10, true, vec![]));
    let outcome = apply(&mut model, insert_radio(11, false, vec![]));
    assert_eq!(outcome.applied, 2);
    let node = model.node_model();
    assert_eq!(node.radios().len(), 2);
    assert_eq!(node.radio_caps().len(), 2, "index parity with the radios");
    assert_eq!(node.radio_cap_at(0).unwrap().ruid.mac, mac(10));
    let default = node.radio_cap_at(1).unwrap();
    assert!(default.ruid.mac.is_zero());
    assert_eq!(default.num_op_classes, 0);
}

#[test]
fn em_insert_radio_replaces_an_existing_radio_and_leaves_its_capability_alone() {
    let mut model = agent_model();
    apply(&mut model, insert_radio(10, true, vec![]));
    let mut replacement = radio(mac(10), false);
    replacement.noise = 9;
    let outcome = apply(
        &mut model,
        ModelDelta::EmInsertRadio {
            radio: replacement,
            cap: None,
            op_classes: vec![],
        },
    );
    assert_eq!(outcome.applied, 1);
    let node = model.node_model();
    assert_eq!(node.radios().len(), 1);
    assert_eq!(node.radio(0).unwrap().noise, 9);
    assert_eq!(node.radio_caps().len(), 1);
    assert_eq!(
        node.radio_cap_at(0).unwrap().ruid.mac,
        mac(10),
        "not replaced by a default"
    );
}

/// D3 table, row 2: a new radio with a capability for another radio is committed, its slot holds
/// the zeroed default (`get_radio_cap(mac)` misses, `dm_easy_mesh.cpp:240-247`) and the record is
/// reported as `WrongRadio`; the radio and the default slot count 2.
#[test]
fn em_insert_radio_with_a_capability_of_another_radio_stores_the_default_and_reports_it() {
    let mut model = agent_model();
    let outcome = apply(
        &mut model,
        ModelDelta::EmInsertRadio {
            radio: radio(mac(10), true),
            cap: Some(radio_cap(mac(11))),
            op_classes: vec![],
        },
    );
    assert_eq!(outcome.applied, 2);
    assert_eq!(
        outcome.skipped,
        vec![skipped(Table::RadioCap, 0, SkipReason::WrongRadio)]
    );
    let node = model.node_model();
    assert_eq!(node.radios().len(), 1);
    assert_eq!(node.radio_caps().len(), 1);
    assert!(node.radio_cap_at(0).unwrap().ruid.mac.is_zero());
    assert!(node.radio_cap(mac(11)).is_none());
}

/// D3 table, row 3: an existing radio is replaced, the supplied capability is ignored as upstream
/// ignores it (`:227-233`) and reported as `NotAttempted`; the count is 1.
#[test]
fn em_insert_radio_for_an_existing_radio_reports_the_ignored_capability() {
    let mut model = agent_model();
    apply(&mut model, insert_radio(10, true, vec![]));
    let mut other = radio_cap(mac(10));
    other.num_op_classes = 7;
    let outcome = apply(
        &mut model,
        ModelDelta::EmInsertRadio {
            radio: radio(mac(10), false),
            cap: Some(other),
            op_classes: vec![],
        },
    );
    assert_eq!(outcome.applied, 1);
    assert_eq!(
        outcome.skipped,
        vec![skipped(Table::RadioCap, 0, SkipReason::NotAttempted)]
    );
    assert_eq!(
        model.node_model().radio_cap_at(0).unwrap().num_op_classes,
        0
    );
}

/// D3 table, row 1: a new radio with its own capability reports nothing and counts 2.
#[test]
fn em_insert_radio_with_its_own_capability_reports_nothing() {
    let mut model = agent_model();
    let outcome = apply(&mut model, insert_radio(10, true, vec![]));
    assert_eq!(outcome.applied, 2);
    assert_eq!(outcome.skipped.len(), 0);
}

/// `put_radio` appends radios without a capability slot (`dm_easy_mesh_list.cpp:399-474`), so
/// removing such a radio must not take the slot of another one (plan D3).
#[test]
fn remove_radio_removes_the_capability_slot_only_if_it_exists() {
    let mut model = agent_model();
    apply(&mut model, insert_radio(10, true, vec![]));
    model.ctx().node.put_radio(radio(mac(11), true)).unwrap();
    assert_eq!(model.node_model().radio_caps().len(), 1);
    assert!(model.ctx().node.remove_radio(mac(11)).is_some());
    assert_eq!(model.node_model().radio_caps().len(), 1);
    assert_eq!(
        model.node_model().radio_cap_at(0).unwrap().ruid.mac,
        mac(10)
    );
}

/// A commit-config radio seeds no policies, unlike `put_radio` (`dm_easy_mesh.cpp:223-251`).
#[test]
fn em_insert_radio_seeds_no_policies() {
    let mut model = agent_model();
    let before = model.node_model().num_policies();
    apply(&mut model, insert_radio(10, true, vec![]));
    assert_eq!(model.node_model().num_policies(), before);
}

/// `dm_easy_mesh.cpp:235-238`: with 3 radios a new one returns before the op classes.
#[test]
fn em_insert_radio_with_full_radios_skips_the_radio_and_all_its_op_classes() {
    let mut model = agent_model();
    for index in 0..3 {
        apply(&mut model, insert_radio(10 + index, true, vec![]));
    }
    let before = model.node_model().op_classes().len();
    let outcome = apply(
        &mut model,
        insert_radio(
            20,
            true,
            vec![
                op_class(mac(20), OpClassType::CAPABILITY, 1),
                op_class(mac(20), OpClassType::CAPABILITY, 2),
            ],
        ),
    );
    assert_eq!(outcome.applied, 0);
    assert_eq!(
        outcome.skipped,
        vec![
            skipped(Table::Radio, 0, full(Table::Radio, MAX_RADIOS)),
            skipped(Table::RadioCap, 0, SkipReason::NotAttempted),
            skipped(Table::OpClass, 0, SkipReason::NotAttempted),
            skipped(Table::OpClass, 1, SkipReason::NotAttempted),
        ]
    );
    assert_eq!(model.node_model().radios().len(), 3);
    assert_eq!(model.node_model().radio_caps().len(), 3);
    assert_eq!(model.node_model().op_classes().len(), before);
}

#[test]
fn em_insert_radio_commits_only_the_op_classes_of_its_radio() {
    let mut model = agent_model();
    let before = model.node_model().op_classes().len();
    let outcome = apply(
        &mut model,
        insert_radio(
            10,
            true,
            vec![
                op_class(mac(10), OpClassType::CAPABILITY, 115),
                op_class(mac(99), OpClassType::CAPABILITY, 116),
            ],
        ),
    );
    assert_eq!(outcome.applied, 3, "radio, capability, one op class");
    assert_eq!(
        outcome.skipped,
        vec![skipped(Table::OpClass, 1, SkipReason::WrongRadio)]
    );
    assert_eq!(model.node_model().op_classes().len(), before + 1);
}

/// `dm_easy_mesh.cpp:274-275`: an op class at capacity is skipped and the rest are committed.
#[test]
fn em_insert_radio_skips_an_op_class_at_capacity_and_commits_the_rest() {
    let mut model = agent_model();
    {
        let node = model.ctx().node;
        let have = node.op_classes().len();
        for number in 0..(MAX_OP_CLASSES - have) {
            node.put_op_class(op_class(
                mac(10),
                OpClassType::CAPABILITY,
                1000 + u32::try_from(number).unwrap(),
            ))
            .unwrap();
        }
        assert_eq!(node.op_classes().len(), MAX_OP_CLASSES);
    }
    let outcome = apply(
        &mut model,
        insert_radio(
            10,
            true,
            vec![
                op_class(mac(10), OpClassType::CAPABILITY, 5000),
                op_class(mac(10), OpClassType::CAPABILITY, 1001),
            ],
        ),
    );
    assert_eq!(
        outcome.applied, 3,
        "radio, capability and the replacing op class"
    );
    assert_eq!(
        outcome.skipped,
        vec![skipped(
            Table::OpClass,
            0,
            full(Table::OpClass, MAX_OP_CLASSES)
        )]
    );
    assert_eq!(model.node_model().op_classes().len(), MAX_OP_CLASSES);
}

// --- EmInsertBss (commit_config bss target, dm_easy_mesh.cpp:285-319) ---

#[allow(clippy::vec_box, reason = "ModelDelta::EmInsertBss holds boxed rows")]
fn insert_bss(
    radio_mac: u8,
    rows: Vec<Box<crate::BssInfo>>,
    ap_mld: Vec<crate::ApMldInfo>,
) -> ModelDelta {
    ModelDelta::EmInsertBss {
        radio: mac(radio_mac),
        bss: rows,
        ap_mld,
    }
}

fn fronthaul(radio_mac: u8, bssid: u8) -> Box<crate::BssInfo> {
    bss(mac(radio_mac), mac(bssid), HaulType::FRONTHAUL, VapMode::AP)
}

#[test]
fn em_insert_bss_adds_new_bss_and_replaces_known_ones_by_bssid() {
    let mut model = agent_model();
    let outcome = apply(
        &mut model,
        insert_bss(10, vec![fronthaul(10, 20), fronthaul(10, 21)], vec![]),
    );
    assert_eq!(outcome.applied, 2);
    let mut changed = fronthaul(10, 20);
    changed.vap_index = 3;
    let outcome = apply(&mut model, insert_bss(10, vec![changed], vec![]));
    assert_eq!(outcome.applied, 1);
    let node = model.node_model();
    assert_eq!(node.bss_list().len(), 2);
    assert_eq!(node.bss(0).unwrap().vap_index, 3);
}

#[test]
fn em_insert_bss_skips_the_bss_of_other_radios() {
    let mut model = agent_model();
    let outcome = apply(
        &mut model,
        insert_bss(10, vec![fronthaul(11, 20), fronthaul(10, 21)], vec![]),
    );
    assert_eq!(outcome.applied, 1);
    assert_eq!(
        outcome.skipped,
        vec![skipped(Table::Bss, 0, SkipReason::WrongRadio)]
    );
    assert_eq!(model.node_model().bss_list().len(), 1);
}

/// Upstream appends past the 24-entry array (0007 defect 1); the port skips the row.
/// intentional-difference:c6-capacity
#[test]
fn em_insert_bss_at_capacity_skips_the_new_row_and_still_replaces_a_known_one() {
    let mut model = agent_model();
    for index in 0..24_u8 {
        model.ctx().node.put_bss(fronthaul(10, 20 + index)).unwrap();
    }
    let outcome = apply(
        &mut model,
        insert_bss(10, vec![fronthaul(10, 100), fronthaul(10, 20)], vec![]),
    );
    assert_eq!(outcome.applied, 1);
    assert_eq!(
        outcome.skipped,
        vec![skipped(Table::Bss, 0, full(Table::Bss, MAX_BSS))]
    );
    assert_eq!(model.node_model().bss_list().len(), 24);
    assert!(model.node_model().bss_info_with_mac(mac(100)).is_none());
}

#[test]
fn em_insert_bss_commits_ap_mlds_with_affiliated_aps_and_skips_the_others() {
    let mut model = agent_model();
    let outcome = apply(
        &mut model,
        insert_bss(
            10,
            vec![],
            vec![
                ap_mld(HaulType::FRONTHAUL, mac(40), vec![]),
                ap_mld(
                    HaulType::BACKHAUL,
                    mac(41),
                    vec![affiliated_ap(mac(10), mac(42), 0)],
                ),
            ],
        ),
    );
    assert_eq!(outcome.applied, 2, "the AP MLD and its affiliated AP");
    assert_eq!(
        outcome.skipped,
        vec![skipped(Table::ApMld, 0, SkipReason::NoAffiliatedAps)]
    );
    assert_eq!(model.node_model().ap_mlds().len(), 1);
    assert_eq!(
        model.node_model().ap_mld(0).unwrap().haul_type,
        HaulType::BACKHAUL
    );
}

/// The AP MLD and each affiliated AP written count as applied rows; an affiliated AP that does
/// not fit is skipped (plan rev 3.1 D3).
#[test]
fn em_insert_bss_counts_each_affiliated_ap_written() {
    let mut model = agent_model();
    let first = ap_mld(
        HaulType::FRONTHAUL,
        mac(40),
        vec![affiliated_ap(mac(10), mac(41), 0)],
    );
    apply(&mut model, insert_bss(10, vec![], vec![first]));
    let full = ap_mld(
        HaulType::FRONTHAUL,
        mac(40),
        vec![
            affiliated_ap(mac(10), mac(41), 0),
            affiliated_ap(mac(11), mac(42), 1),
        ],
    );
    let outcome = apply(&mut model, insert_bss(10, vec![], vec![full]));
    assert_eq!(outcome.applied, 3, "the MLD, the matched AP and the new AP");
    assert_eq!(outcome.skipped.len(), 0);
}

/// An affiliated AP that does not fit is reported and not counted.
#[test]
fn em_insert_bss_does_not_count_a_skipped_affiliated_ap() {
    let mut model = agent_model();
    let fill = (0..64)
        .map(|index| affiliated_ap(mac(10), mac2(2, index), index))
        .collect();
    apply(
        &mut model,
        insert_bss(10, vec![], vec![ap_mld(HaulType::FRONTHAUL, mac(40), fill)]),
    );
    let more = ap_mld(
        HaulType::FRONTHAUL,
        mac(40),
        vec![
            affiliated_ap(mac(10), mac2(2, 0), 0),
            affiliated_ap(mac(11), mac(99), 1),
        ],
    );
    let outcome = apply(&mut model, insert_bss(10, vec![], vec![more]));
    assert_eq!(outcome.applied, 2, "the MLD and the matched AP");
    assert_eq!(
        outcome.skipped,
        vec![skipped(
            Table::AffiliatedAp,
            1,
            SkipReason::AffiliatedApFull { ap_mld: 0 }
        )]
    );
}

#[test]
fn em_insert_bss_reports_an_ap_mld_with_an_invalid_haul_type() {
    let mut model = agent_model();
    let outcome = apply(
        &mut model,
        insert_bss(
            10,
            vec![],
            vec![ap_mld(
                HaulType(7),
                mac(40),
                vec![affiliated_ap(mac(10), mac(42), 0)],
            )],
        ),
    );
    assert_eq!(outcome.applied, 0);
    assert_eq!(
        outcome.skipped,
        vec![
            skipped(
                Table::ApMld,
                0,
                SkipReason::Rejected(ModelError::InvalidHaulType(HaulType(7)))
            ),
            skipped(Table::AffiliatedAp, 0, SkipReason::NotAttempted),
        ]
    );
    assert_eq!(model.node_model().ap_mlds().len(), 0);
}

// --- Policies ---

#[test]
fn policies_apply_each_row_and_skip_those_at_capacity() {
    let mut model = agent_model();
    let room = MAX_POLICIES - model.node_model().num_policies();
    let rows: Vec<Box<crate::Policy>> = (0..=room)
        .map(|index| {
            policy(
                PolicyIdType::STEERING_PARAM,
                mac2(1, u8::try_from(index).unwrap()),
            )
        })
        .collect();
    let outcome = apply(&mut model, ModelDelta::Policies(rows));
    assert_eq!(outcome.applied, room);
    assert_eq!(
        outcome.skipped,
        vec![skipped(
            Table::Policy,
            room,
            full(Table::Policy, MAX_POLICIES)
        )]
    );
    assert_eq!(model.node_model().num_policies(), MAX_POLICIES);
}

#[test]
fn policies_replace_a_stored_policy_even_at_capacity() {
    let mut model = agent_model();
    let room = MAX_POLICIES - model.node_model().num_policies();
    for index in 0..room {
        model
            .ctx()
            .node
            .set_policy(policy(
                PolicyIdType::STEERING_PARAM,
                mac2(1, u8::try_from(index).unwrap()),
            ))
            .unwrap();
    }
    let outcome = apply(
        &mut model,
        ModelDelta::Policies(vec![policy(PolicyIdType::STEERING_PARAM, mac2(1, 0))]),
    );
    assert_eq!(outcome.applied, 1);
    assert_eq!(outcome.skipped.len(), 0);
}

// --- StaCapUpdate (plan C2, F1) ---

fn frame_of(bytes: &[u8]) -> FrameBody {
    BoundedVec::try_from_vec(bytes.to_vec()).unwrap()
}

fn links(keys: &[StaKey]) -> BoundedVec<StaKey, 64> {
    BoundedVec::try_from_vec(keys.to_vec()).unwrap()
}

#[test]
fn sta_cap_update_of_one_link_stages_a_pending_row_with_the_frame() {
    let mut model = agent_model();
    let update = StaCapUpdate::new(links(&[key(30, 20, 10)]), frame_of(&[1, 2, 3]));
    let outcome = apply(&mut model, ModelDelta::StaCapUpdate(update));
    assert_eq!(outcome.links_updated, 1);
    assert_eq!(outcome.applied, 1);
    assert_eq!(outcome.skipped.len(), 0);
    let node = model.node_model();
    let pending = node.sta_info(&key(30, 20, 10), Assoc).unwrap();
    assert!(pending.associated);
    assert_eq!(pending.frame_body.as_slice(), &[1, 2, 3]);
    assert!(
        node.sta_info(&key(30, 20, 10), Consolidated).is_none(),
        "the consolidated half only updates an existing row"
    );
}

#[test]
fn sta_cap_update_replaces_only_the_frame_of_an_existing_consolidated_row() {
    let mut model = agent_model();
    let mut existing = row(30, 20, 10);
    existing.last_dl_rate = 42;
    existing.associated = false;
    model.ctx().node.put_sta(existing);
    let update = StaCapUpdate::new(links(&[key(30, 20, 10)]), frame_of(&[9, 9]));
    apply(&mut model, ModelDelta::StaCapUpdate(update));
    let stored = model
        .node_model()
        .sta_info(&key(30, 20, 10), Consolidated)
        .unwrap();
    assert_eq!(stored.frame_body.as_slice(), &[9, 9]);
    assert_eq!(stored.last_dl_rate, 42, "every other field is kept");
    assert!(!stored.associated);
}

/// F1: upstream holds the frame once in the pending entry and once in the consolidated entry
/// per link: 2N copies. The port keeps that state through the one named copy function.
#[test]
fn sta_cap_update_holds_the_frame_twice_per_link() {
    let mut model = agent_model();
    let link_keys = [key(30, 20, 10), key(30, 21, 11), key(30, 22, 12)];
    for link in &link_keys {
        model
            .ctx()
            .node
            .put_sta(row(30, link.bssid_last(), link.radio_last()));
    }
    let frame = [7_u8; 32];
    let update = StaCapUpdate::new(links(&link_keys), frame_of(&frame));
    assert_eq!(update.links().len(), 3);
    assert_eq!(update.frame(), &frame);
    let outcome = apply(&mut model, ModelDelta::StaCapUpdate(update));
    assert_eq!(outcome.links_updated, 3);
    let node = model.node_model();
    let holders = node
        .sta_rows(Assoc)
        .chain(node.sta_rows(Consolidated))
        .filter(|sta| sta.frame_body.as_slice() == frame)
        .count();
    assert_eq!(holders, 6, "2N copies for N = 3 links");
}

#[test]
fn sta_cap_update_with_no_resolved_links_updates_nothing() {
    let mut model = agent_model();
    let update = StaCapUpdate::new(links(&[]), frame_of(&[1]));
    let outcome = apply(&mut model, ModelDelta::StaCapUpdate(update));
    assert_eq!(
        outcome.links_updated, 0,
        "the caller then does not mark (em_ctrl.cpp:359-361)"
    );
    assert_eq!(outcome.applied, 0);
    assert_eq!(model.node_model().num_sta(Assoc), 0);
}

#[test]
fn sta_cap_update_replaces_a_pending_row_whole() {
    let mut model = agent_model();
    let mut earlier = row(30, 20, 10);
    earlier.last_ul_rate = 5;
    model.ctx().node.put_sta_info(earlier, Assoc);
    let update = StaCapUpdate::new(links(&[key(30, 20, 10)]), frame_of(&[4]));
    apply(&mut model, ModelDelta::StaCapUpdate(update));
    let pending = model
        .node_model()
        .sta_info(&key(30, 20, 10), Assoc)
        .unwrap();
    assert_eq!(
        pending.last_ul_rate, 0,
        "rebuilt from the key, the flag and the frame"
    );
    assert_eq!(pending.frame_body.as_slice(), &[4]);
    assert_eq!(model.node_model().num_sta(Assoc), 1);
}

/// Both maps are written when the consolidated row exists, so two rows are applied for one link
/// (plan rev 3.1 D3); `links_updated` stays the link count that marks the model dirty.
#[test]
fn sta_cap_update_counts_the_pending_and_the_consolidated_row() {
    let mut model = agent_model();
    model.ctx().node.put_sta(row(30, 20, 10));
    let update = StaCapUpdate::new(links(&[key(30, 20, 10)]), frame_of(&[1]));
    let outcome = apply(&mut model, ModelDelta::StaCapUpdate(update));
    assert_eq!(outcome.applied, 2);
    assert_eq!(outcome.links_updated, 1);
}

trait KeyParts {
    fn bssid_last(&self) -> u8;
    fn radio_last(&self) -> u8;
}

impl KeyParts for StaKey {
    fn bssid_last(&self) -> u8 {
        self.bssid.octets()[5]
    }
    fn radio_last(&self) -> u8 {
        self.radio.octets()[5]
    }
}
