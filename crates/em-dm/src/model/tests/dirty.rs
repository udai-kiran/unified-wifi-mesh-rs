// SPDX-License-Identifier: Apache-2.0

//! The typed dirty set (plan C5): bit values, marking, clearing and the BSS-delete filter.

use super::fixtures::{agent_node, bss, mac};
use crate::enums::{HaulType, VapMode};
use crate::model::dirty::{DbCfgType, DirtySet};

/// `db_cfg_type_*` shifts at `inc/em_base.h:3316-3341`.
const UPSTREAM_SHIFTS: [(DbCfgType, u32); 23] = [
    (DbCfgType::NetworkListUpdate, 0),
    (DbCfgType::NetworkListDelete, 1),
    (DbCfgType::DeviceListUpdate, 2),
    (DbCfgType::DeviceListDelete, 3),
    (DbCfgType::RadioListUpdate, 4),
    (DbCfgType::RadioListDelete, 5),
    (DbCfgType::OpClassListUpdate, 6),
    (DbCfgType::OpClassListDelete, 7),
    (DbCfgType::BssListUpdate, 8),
    (DbCfgType::BssListDelete, 9),
    (DbCfgType::StaListUpdate, 10),
    (DbCfgType::StaListDelete, 11),
    (DbCfgType::NetworkSsidListUpdate, 12),
    (DbCfgType::NetworkSsidListDelete, 13),
    (DbCfgType::RadioCapListUpdate, 14),
    (DbCfgType::RadioCapListDelete, 15),
    (DbCfgType::Ieee1905SecurityListUpdate, 16),
    (DbCfgType::Ieee1905SecurityListDelete, 17),
    (DbCfgType::StaMetricsUpdate, 18),
    (DbCfgType::PolicyListUpdate, 19),
    (DbCfgType::PolicyListDelete, 20),
    (DbCfgType::ScanResultListUpdate, 21),
    (DbCfgType::ScanResultListDelete, 22),
];

#[test]
fn every_bit_has_its_upstream_value() {
    for (kind, shift) in UPSTREAM_SHIFTS {
        assert_eq!(kind.bit(), 1_u32 << shift, "{kind:?}");
    }
}

#[test]
fn the_all_table_lists_each_bit_once_in_upstream_order() {
    let listed: Vec<DbCfgType> = UPSTREAM_SHIFTS.iter().map(|(kind, _)| *kind).collect();
    assert_eq!(DbCfgType::ALL.as_slice(), listed.as_slice());
}

#[test]
fn a_new_set_is_clean() {
    let set = DirtySet::new();
    assert!(!set.any());
    assert_eq!(set.bits(), 0);
    assert_eq!(set.bss_delete_filter(), None);
    for kind in DbCfgType::ALL {
        assert!(!set.is_set(kind));
    }
}

#[test]
fn mark_sets_only_that_bit() {
    for kind in DbCfgType::ALL {
        let mut set = DirtySet::new();
        set.mark(kind);
        assert_eq!(set.bits(), kind.bit());
        assert!(set.is_set(kind));
        assert!(set.any());
    }
}

#[test]
fn clear_clears_only_that_bit() {
    let mut set = DirtySet::new();
    set.mark(DbCfgType::StaListUpdate);
    set.mark(DbCfgType::BssListUpdate);
    set.clear(DbCfgType::StaListUpdate);
    assert!(!set.is_set(DbCfgType::StaListUpdate));
    assert!(set.is_set(DbCfgType::BssListUpdate));
    assert!(set.any());
    set.clear(DbCfgType::BssListUpdate);
    assert!(!set.any());
}

#[test]
fn clearing_a_clear_bit_changes_nothing() {
    let mut set = DirtySet::new();
    set.clear(DbCfgType::PolicyListDelete);
    assert_eq!(set, DirtySet::new());
}

#[test]
fn mark_bss_delete_with_a_radio_sets_the_bit_and_the_filter() {
    let mut set = DirtySet::new();
    set.mark_bss_delete(Some(mac(10)));
    assert!(set.is_set(DbCfgType::BssListDelete));
    assert_eq!(set.bss_delete_filter(), Some(mac(10)));
}

#[test]
fn mark_bss_delete_without_a_radio_sets_the_bit_and_no_filter() {
    let mut set = DirtySet::new();
    set.mark_bss_delete(None);
    assert!(set.is_set(DbCfgType::BssListDelete));
    assert_eq!(set.bss_delete_filter(), None);
}

/// Upstream resets the criteria slot with the bit's value instead of its index
/// (`dm_easy_mesh.cpp:4138`), so a stale `radio=` string survives the clear and a later mark
/// with `""` still filters (`strncpy` of zero bytes, `:4157`). The port clears the filter with
/// the bit. intentional-difference:c5-clear-clears-criterion
#[test]
fn clear_clears_the_bss_delete_filter() {
    let mut set = DirtySet::new();
    set.mark_bss_delete(Some(mac(10)));
    set.clear(DbCfgType::BssListDelete);
    assert_eq!(set.bss_delete_filter(), None);
    set.mark_bss_delete(None);
    assert_eq!(
        set.bss_delete_filter(),
        None,
        "a later unfiltered mark must not inherit the old radio"
    );
}

/// Marking another bit leaves the criterion of the BSS-delete bit alone (upstream keeps one
/// criteria slot per bit, `inc/em_base.h:3343-3346`).
#[test]
fn the_bss_delete_filter_survives_other_marks() {
    let mut set = DirtySet::new();
    set.mark_bss_delete(Some(mac(10)));
    set.mark(DbCfgType::StaListUpdate);
    set.mark(DbCfgType::BssListUpdate);
    set.clear(DbCfgType::StaListUpdate);
    assert_eq!(set.bss_delete_filter(), Some(mac(10)));
}

/// `strncpy(criteria, "", strlen(""))` copies nothing (`dm_easy_mesh.cpp:4157`): an unfiltered
/// mark made while a filter is set keeps the filter. Parity.
#[test]
fn an_unfiltered_mark_keeps_a_filter_that_is_still_set() {
    let mut set = DirtySet::new();
    set.mark_bss_delete(Some(mac(10)));
    set.mark_bss_delete(None);
    assert_eq!(set.bss_delete_filter(), Some(mac(10)));
}

#[test]
fn a_later_filtered_mark_replaces_the_radio() {
    let mut set = DirtySet::new();
    set.mark_bss_delete(Some(mac(10)));
    set.mark_bss_delete(Some(mac(11)));
    assert_eq!(set.bss_delete_filter(), Some(mac(11)));
}

/// `dm_bss_t::match_criteria` (`dm_bss.cpp:375-392`): the empty criterion matches every BSS.
#[test]
fn no_filter_matches_every_bss() {
    let row = bss(mac(10), mac(20), HaulType::FRONTHAUL, VapMode::AP);
    assert!(row.matches_radio_filter(None));
}

#[test]
fn a_radio_filter_matches_only_the_bss_of_that_radio() {
    let row = bss(mac(10), mac(20), HaulType::FRONTHAUL, VapMode::AP);
    assert!(row.matches_radio_filter(Some(mac(10))));
    assert!(!row.matches_radio_filter(Some(mac(11))));
    assert!(
        !row.matches_radio_filter(Some(mac(20))),
        "the BSSID is not the radio"
    );
}

#[test]
fn a_node_starts_with_a_clean_dirty_set_and_marking_is_the_callers() {
    let mut node = agent_node();
    assert!(!node.dirty().any());
    node.put_bss(bss(mac(10), mac(20), HaulType::FRONTHAUL, VapMode::AP))
        .unwrap();
    assert!(
        !node.dirty().any(),
        "operations do not mark (plan C5, research 3.4)"
    );
    node.dirty_mut().mark(DbCfgType::BssListUpdate);
    assert!(node.dirty().is_set(DbCfgType::BssListUpdate));
}
