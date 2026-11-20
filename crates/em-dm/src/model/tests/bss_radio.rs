// SPDX-License-Identifier: Apache-2.0

//! BSS, radio and radio-capability operations of a node (ports of the upstream `dm_easy_mesh_t`
//! tests named in `tests/data/upstream_test_map.tsv`).

use super::fixtures::{agent_node, bss, mac, mac2, radio, radio_cap};
use crate::enums::{HaulType, PolicyIdType, VapMode};
use crate::model::error::{ModelError, Put, Table};
use crate::model::keys::{BssKey, PolicyKey};
use crate::model::node::{MAX_BSS, MAX_RADIOS, RadioCapOutcome, RadioCommit};

fn backhaul_ap(radio_mac: u8, bssid: u8) -> Box<crate::BssInfo> {
    bss(mac(radio_mac), mac(bssid), HaulType::BACKHAUL, VapMode::AP)
}

fn backhaul_sta(radio_mac: u8, bssid: u8) -> Box<crate::BssInfo> {
    bss(mac(radio_mac), mac(bssid), HaulType::BACKHAUL, VapMode::STA)
}

// --- BSS lookups ---

#[test]
fn bss_info_with_mac_valid() {
    let mut node = agent_node();
    node.put_bss(backhaul_ap(10, 20)).unwrap();
    node.put_bss(backhaul_ap(10, 21)).unwrap();
    let found = node.bss_info_with_mac(mac(21)).unwrap();
    assert_eq!(found.bssid.mac, mac(21));
}

#[test]
fn bss_info_with_mac_unknown_mac() {
    let mut node = agent_node();
    node.put_bss(backhaul_ap(10, 20)).unwrap();
    assert!(node.bss_info_with_mac(mac(99)).is_none());
}

#[test]
fn bss_info_with_mac_empty_table() {
    assert!(agent_node().bss_info_with_mac(mac(20)).is_none());
}

#[test]
fn bss_by_radio_needs_the_bss_and_the_radio_mac() {
    let mut node = agent_node();
    node.put_bss(backhaul_ap(10, 20)).unwrap();
    assert!(node.bss_by_radio(mac(10), mac(20)).is_some());
}

#[test]
fn bss_by_radio_wrong_bss_mac() {
    let mut node = agent_node();
    node.put_bss(backhaul_ap(10, 20)).unwrap();
    assert!(node.bss_by_radio(mac(10), mac(21)).is_none());
}

#[test]
fn bss_by_radio_wrong_radio_mac() {
    let mut node = agent_node();
    node.put_bss(backhaul_ap(10, 20)).unwrap();
    assert!(node.bss_by_radio(mac(11), mac(20)).is_none());
}

#[test]
fn bss_by_radio_empty_table() {
    assert!(agent_node().bss_by_radio(mac(10), mac(20)).is_none());
}

#[test]
fn bss_by_index_zero_and_last() {
    let mut node = agent_node();
    for bssid in 20..24 {
        node.put_bss(backhaul_ap(10, bssid)).unwrap();
    }
    assert_eq!(node.bss(0).unwrap().bssid.mac, mac(20));
    assert_eq!(node.bss(3).unwrap().bssid.mac, mac(23));
    assert_eq!(node.bss_list().len(), 4);
}

/// Upstream returns a zeroed slot for an index inside the 24-entry array but beyond
/// `m_num_bss`; the port returns `None` (intentional-difference:index-bounds).
#[test]
fn bss_by_index_beyond_count_is_none() {
    let mut node = agent_node();
    node.put_bss(backhaul_ap(10, 20)).unwrap();
    assert!(node.bss(1).is_none());
    assert!(node.bss(MAX_BSS).is_none());
    assert!(agent_node().bss(0).is_none());
}

#[test]
fn bss_list_keeps_insertion_order_and_content() {
    let mut node = agent_node();
    let bssids = [20, 22, 21];
    for bssid in bssids {
        node.put_bss(backhaul_ap(10, bssid)).unwrap();
    }
    let stored: Vec<_> = node.bss_list().iter().map(|b| b.bssid.mac).collect();
    assert_eq!(stored, vec![mac(20), mac(22), mac(21)]);
}

#[test]
fn find_matching_bss_by_radio_bssid_and_haul() {
    let mut node = agent_node();
    node.put_bss(backhaul_ap(10, 20)).unwrap();
    let key = BssKey {
        ruid: mac(10),
        bssid: mac(20),
        haul_type: HaulType::BACKHAUL,
    };
    assert!(node.find_matching_bss(&key).is_some());
}

#[test]
fn find_matching_bss_no_match() {
    let mut node = agent_node();
    node.put_bss(backhaul_ap(10, 20)).unwrap();
    let wrong_haul = BssKey {
        ruid: mac(10),
        bssid: mac(20),
        haul_type: HaulType::FRONTHAUL,
    };
    assert!(node.find_matching_bss(&wrong_haul).is_none());
    let wrong_radio = BssKey {
        ruid: mac(11),
        bssid: mac(20),
        haul_type: HaulType::BACKHAUL,
    };
    assert!(node.find_matching_bss(&wrong_radio).is_none());
}

// --- BSS derived reads ---

fn node_with_radio_and_bss(radio_enabled: bool, bss: Box<crate::BssInfo>) -> crate::NodeModel {
    let mut node = agent_node();
    node.put_radio(radio(bss.ruid.mac, radio_enabled)).unwrap();
    node.put_bss(bss).unwrap();
    node
}

#[test]
fn backhaul_bss_info_finds_enabled_backhaul_ap() {
    let node = node_with_radio_and_bss(true, backhaul_ap(10, 20));
    assert_eq!(node.backhaul_bss_info().unwrap().bssid.mac, mac(20));
}

#[test]
fn backhaul_bss_info_empty_table() {
    assert!(agent_node().backhaul_bss_info().is_none());
}

#[test]
fn backhaul_bss_info_ignores_non_backhaul_bss() {
    let fronthaul = bss(mac(10), mac(20), HaulType::FRONTHAUL, VapMode::AP);
    assert!(
        node_with_radio_and_bss(true, fronthaul)
            .backhaul_bss_info()
            .is_none()
    );
}

#[test]
fn backhaul_bss_info_needs_its_radio() {
    let mut node = agent_node();
    node.put_bss(backhaul_ap(10, 20)).unwrap();
    assert!(node.backhaul_bss_info().is_none());
}

#[test]
fn backhaul_bss_info_needs_an_enabled_radio() {
    assert!(
        node_with_radio_and_bss(false, backhaul_ap(10, 20))
            .backhaul_bss_info()
            .is_none()
    );
}

#[test]
fn backhaul_bss_info_needs_an_enabled_bss() {
    let mut disabled = backhaul_ap(10, 20);
    disabled.enabled = false;
    assert!(
        node_with_radio_and_bss(true, disabled)
            .backhaul_bss_info()
            .is_none()
    );
}

#[test]
fn bsta_bss_info_finds_backhaul_sta() {
    let node = node_with_radio_and_bss(true, backhaul_sta(10, 20));
    assert_eq!(node.bsta_bss_info().unwrap().bssid.mac, mac(20));
}

#[test]
fn bsta_bss_info_empty_table() {
    assert!(agent_node().bsta_bss_info().is_none());
}

#[test]
fn bsta_bss_info_not_backhaul() {
    let fronthaul = bss(mac(10), mac(20), HaulType::FRONTHAUL, VapMode::STA);
    assert!(
        node_with_radio_and_bss(true, fronthaul)
            .bsta_bss_info()
            .is_none()
    );
}

#[test]
fn bsta_bss_info_not_sta_mode() {
    assert!(
        node_with_radio_and_bss(true, backhaul_ap(10, 20))
            .bsta_bss_info()
            .is_none()
    );
}

#[test]
fn bsta_bss_info_radio_not_found() {
    let mut node = agent_node();
    node.put_bss(backhaul_sta(10, 20)).unwrap();
    assert!(node.bsta_bss_info().is_none());
}

#[test]
fn bsta_bss_info_radio_disabled() {
    assert!(
        node_with_radio_and_bss(false, backhaul_sta(10, 20))
            .bsta_bss_info()
            .is_none()
    );
}

#[test]
fn bsta_bss_info_bss_disabled() {
    let mut disabled = backhaul_sta(10, 20);
    disabled.enabled = false;
    assert!(
        node_with_radio_and_bss(true, disabled)
            .bsta_bss_info()
            .is_none()
    );
}

#[test]
fn associated_bsta_prefers_a_bss_with_a_non_zero_bssid() {
    let mut node = agent_node();
    node.put_radio(radio(mac(10), true)).unwrap();
    let mut unassociated = backhaul_sta(10, 20);
    unassociated.bssid.mac = em_types::MacAddr::ZERO;
    unassociated.id.bssid = em_types::MacAddr::ZERO;
    node.put_bss(unassociated).unwrap();
    node.put_bss(backhaul_sta(10, 21)).unwrap();
    assert_eq!(node.associated_bsta_bss_info().unwrap().bssid.mac, mac(21));
}

#[test]
fn associated_bsta_falls_back_when_all_bssids_are_zero() {
    let mut node = agent_node();
    node.put_radio(radio(mac(10), true)).unwrap();
    let mut unassociated = backhaul_sta(10, 20);
    unassociated.bssid.mac = em_types::MacAddr::ZERO;
    unassociated.id.bssid = em_types::MacAddr::ZERO;
    node.put_bss(unassociated).unwrap();
    let fallback = node.associated_bsta_bss_info().unwrap();
    assert!(
        fallback.bssid.mac.is_zero(),
        "get_bsta_bss_info picks the zero-BSSID STA BSS"
    );
}

// --- BSS mutation ---

#[test]
fn put_bss_replaces_the_same_key_and_appends_a_new_one() {
    let mut node = agent_node();
    assert_eq!(node.put_bss(backhaul_ap(10, 20)), Ok(Put::Inserted));
    let mut changed = backhaul_ap(10, 20);
    changed.vap_index = 7;
    assert_eq!(node.put_bss(changed), Ok(Put::Replaced));
    assert_eq!(node.bss_list().len(), 1);
    assert_eq!(node.bss(0).unwrap().vap_index, 7);
    assert_eq!(node.put_bss(backhaul_ap(10, 21)), Ok(Put::Inserted));
    assert_eq!(node.bss_list().len(), 2);
}

/// Criterion 5 gate: insert at capacity versus update of an existing row. Upstream appends past
/// the 24-entry array (0007 defect 1, `dm_easy_mesh_list.cpp:665-670`).
/// intentional-difference:c6-capacity
#[test]
fn bss_capacity_is_24_and_updating_an_existing_row_still_works() {
    let mut node = agent_node();
    for bssid in 0..24 {
        node.put_bss(backhaul_ap(10, bssid)).unwrap();
    }
    assert_eq!(MAX_BSS, 24);
    let full = node.put_bss(backhaul_ap(10, 99));
    assert_eq!(
        full,
        Err(ModelError::Full {
            table: Table::Bss,
            capacity: 24
        })
    );
    assert_eq!(
        node.bss_list().len(),
        24,
        "the refused insert changed nothing"
    );
    assert_eq!(node.put_bss(backhaul_ap(10, 0)), Ok(Put::Replaced));
    assert!(node.update_bss(backhaul_ap(10, 5)).is_ok());
}

#[test]
fn update_bss_of_an_unknown_bss_is_not_found() {
    let mut node = agent_node();
    assert_eq!(
        node.update_bss(backhaul_ap(10, 20)),
        Err(ModelError::NotFound { table: Table::Bss })
    );
}

#[test]
fn update_bss_replaces_only_the_bss_record() {
    let mut node = agent_node();
    node.put_bss(backhaul_ap(10, 20)).unwrap();
    let mut changed = backhaul_ap(10, 20);
    changed.vlan_id = 42;
    node.update_bss(changed).unwrap();
    assert_eq!(node.bss(0).unwrap().vlan_id, 42);
    assert_eq!(node.bss_list().len(), 1);
}

/// Upstream removes by BSSID alone (`dm_easy_mesh_list.cpp:641`); the port matches the whole
/// key (plan C4). intentional-difference:c4-exact-keys
#[test]
fn remove_bss_matches_the_whole_key() {
    let mut node = agent_node();
    node.put_bss(backhaul_ap(10, 20)).unwrap();
    let other_haul = BssKey {
        ruid: mac(10),
        bssid: mac(20),
        haul_type: HaulType::FRONTHAUL,
    };
    assert!(node.remove_bss(&other_haul).is_none());
    let key = BssKey::of(node.bss(0).unwrap());
    assert!(node.remove_bss(&key).is_some());
    assert_eq!(node.bss_list().len(), 0);
}

#[test]
fn remove_bss_by_index_valid_index_shifts_the_later_bss() {
    let mut node = agent_node();
    for bssid in [20, 21, 22] {
        node.put_bss(backhaul_ap(10, bssid)).unwrap();
    }
    assert!(node.remove_bss_by_index(1).is_some());
    let left: Vec<_> = node.bss_list().iter().map(|b| b.bssid.mac).collect();
    assert_eq!(left, vec![mac(20), mac(22)]);
}

#[test]
fn remove_bss_by_index_invalid_index() {
    let mut node = agent_node();
    node.put_bss(backhaul_ap(10, 20)).unwrap();
    assert!(node.remove_bss_by_index(5).is_none());
    assert_eq!(node.bss_list().len(), 1);
}

#[test]
fn remove_bss_by_index_last_index() {
    let mut node = agent_node();
    for bssid in [20, 21] {
        node.put_bss(backhaul_ap(10, bssid)).unwrap();
    }
    assert!(node.remove_bss_by_index(1).is_some());
    assert_eq!(node.bss_list().len(), 1);
    assert_eq!(node.bss(0).unwrap().bssid.mac, mac(20));
}

#[test]
fn remove_bss_by_index_empty_list() {
    let mut node = agent_node();
    assert!(node.remove_bss_by_index(0).is_none());
}

/// Upstream copies `m_bss[i + 1]` down and leaves the old last element in the vacated slot, still
/// readable by index (`dm_easy_mesh.cpp:3501-3515`); the port has no such slot.
/// intentional-difference:remove-bss-stale-slot
#[test]
fn remove_bss_by_index_leaves_no_stale_last_slot() {
    let mut node = agent_node();
    for bssid in [20, 21] {
        node.put_bss(backhaul_ap(10, bssid)).unwrap();
    }
    node.remove_bss_by_index(0).unwrap();
    assert!(
        node.bss(1).is_none(),
        "upstream still shows BSS 21 at index 1"
    );
}

#[test]
fn commit_bss_config_replaces_the_bss_with_the_same_radio_and_bssid() {
    let mut node = agent_node();
    node.put_bss(backhaul_ap(10, 20)).unwrap();
    let mut changed = backhaul_ap(10, 20);
    changed.vlan_id = 9;
    assert!(node.commit_bss_config(changed));
    assert_eq!(node.bss(0).unwrap().vlan_id, 9);
}

#[test]
fn commit_bss_config_only_touches_the_matching_bssid() {
    let mut node = agent_node();
    node.put_bss(backhaul_ap(10, 20)).unwrap();
    node.put_bss(backhaul_ap(10, 21)).unwrap();
    let mut changed = backhaul_ap(10, 21);
    changed.vlan_id = 9;
    assert!(node.commit_bss_config(changed));
    assert_eq!(node.bss(0).unwrap().vlan_id, 0);
    assert_eq!(node.bss(1).unwrap().vlan_id, 9);
}

/// Upstream refuses a source index of `EM_MAX_BSS_PER_RADIO` or more; the source is the argument
/// here, so the matching case is a BSS that is not stored: nothing is added.
/// intentional-difference:commit-bss-source-by-value
#[test]
fn commit_bss_config_for_an_unknown_bss_changes_nothing() {
    let mut node = agent_node();
    node.put_bss(backhaul_ap(10, 20)).unwrap();
    assert!(!node.commit_bss_config(backhaul_ap(10, 77)));
    assert_eq!(node.bss_list().len(), 1);
}

// --- radios ---

/// Upstream returns a zeroed slot for an index below 3 (`dm_easy_mesh.cpp:2354-2361`); the port
/// returns `None` at and beyond the count. intentional-difference:index-bounds
#[test]
fn radio_by_index_valid_and_beyond_the_count() {
    let mut node = agent_node();
    node.put_radio(radio(mac(10), true)).unwrap();
    assert_eq!(node.radio(0).unwrap().intf.mac, mac(10));
    assert!(
        node.radio(1).is_none(),
        "upstream returns a zeroed slot below 3"
    );
    assert!(node.radio(MAX_RADIOS).is_none());
}

#[test]
fn radio_by_mac_valid_and_unknown() {
    let mut node = agent_node();
    node.put_radio(radio(mac(10), true)).unwrap();
    assert_eq!(node.radio_by_mac(mac(10)).unwrap().intf.mac, mac(10));
    assert!(node.radio_by_mac(mac(11)).is_none());
}

#[test]
fn find_matching_radio_exact_match() {
    let mut node = agent_node();
    node.put_radio(radio(mac(10), true)).unwrap();
    assert!(node.find_matching_radio(&radio(mac(10), false)).is_some());
}

#[test]
fn find_matching_radio_no_match() {
    let mut node = agent_node();
    node.put_radio(radio(mac(10), true)).unwrap();
    assert!(node.find_matching_radio(&radio(mac(11), true)).is_none());
}

#[test]
fn find_matching_radio_partial_match_is_no_match() {
    let mut node = agent_node();
    node.put_radio(radio(mac2(1, 10), true)).unwrap();
    assert!(
        node.find_matching_radio(&radio(mac2(2, 10), true))
            .is_none()
    );
}

#[test]
fn radio_cap_by_mac_valid_and_unknown() {
    let mut node = agent_node();
    node.put_radio(radio(mac(10), true)).unwrap();
    node.put_radio_cap(radio_cap(mac(10))).unwrap();
    assert!(node.radio_cap(mac(10)).is_some());
    assert!(node.radio_cap(mac(11)).is_none());
    assert!(node.radio_cap_at(0).is_some());
    assert!(node.radio_cap_at(1).is_none());
}

/// Criterion 5 gate for radios: 3 radios, the fourth is refused, an update still works.
/// intentional-difference:c6-capacity
#[test]
fn radio_capacity_is_3_and_updating_an_existing_row_still_works() {
    let mut node = agent_node();
    for radio_mac in [10, 11, 12] {
        node.put_radio(radio(mac(radio_mac), true)).unwrap();
    }
    assert_eq!(
        node.put_radio(radio(mac(13), true)),
        Err(ModelError::Full {
            table: Table::Radio,
            capacity: 3
        })
    );
    assert_eq!(node.radios().len(), 3);
    let again = node.put_radio(radio(mac(11), false)).unwrap();
    assert_eq!(again.put, Put::Replaced);
    assert!(!node.radio_by_mac(mac(11)).unwrap().enabled);
    assert!(node.update_radio(radio(mac(12), true)).is_ok());
}

/// A capability record is written to the slot of its radio, so there are as many slots as
/// radios (at most 3); a record for a radio that is not stored is refused.
#[test]
fn radio_cap_capacity_is_3_one_slot_per_stored_radio() {
    let mut node = agent_node();
    for radio_mac in [10, 11, 12] {
        node.put_radio(radio(mac(radio_mac), true)).unwrap();
        node.put_radio_cap(radio_cap(mac(radio_mac))).unwrap();
    }
    assert_eq!(
        node.put_radio_cap(radio_cap(mac(13))),
        Err(ModelError::NotFound {
            table: Table::Radio
        })
    );
    assert_eq!(node.radio_caps().len(), 3);
    assert_eq!(node.put_radio_cap(radio_cap(mac(10))), Ok(Put::Replaced));
}

#[test]
fn put_radio_seeds_the_per_radio_policies_once() {
    let mut node = agent_node();
    let before = node.num_policies();
    let put = node.put_radio(radio(mac(10), true)).unwrap();
    assert_eq!(put.policies_rejected, 0);
    assert_eq!(node.num_policies(), before + 2);
    let key = |kind| PolicyKey {
        net: node.net(),
        dev_mac: node.agent_al_interface_mac(),
        radio_mac: mac(10),
        kind,
    };
    let metrics = node.policy(&key(PolicyIdType::RADIO_METRICS_REP)).unwrap();
    assert_eq!(
        (
            metrics.util_threshold,
            metrics.rcpi_threshold,
            metrics.rcpi_hysteresis
        ),
        (60, 120, 5)
    );
    assert!(metrics.sta_traffic_stats && metrics.sta_link_metric && metrics.sta_status);
    let steering = node.policy(&key(PolicyIdType::STEERING_PARAM)).unwrap();
    assert_eq!(
        (steering.util_threshold, steering.rcpi_threshold),
        (60, 120)
    );
    node.put_radio(radio(mac(10), false)).unwrap();
    assert_eq!(
        node.num_policies(),
        before + 2,
        "a second put seeds nothing"
    );
}

/// The seeded policies use the node's network id; upstream writes the literal `"OneWifiMesh"`
/// (`dm_easy_mesh_list.cpp:446, 459`). intentional-difference:policy-net-id
#[test]
fn seeded_policies_use_the_nodes_network_id() {
    let mut init = super::fixtures::node_init(crate::NodeRole::Agent);
    init.net = super::fixtures::net("OtherNet");
    let mut node = crate::NodeModel::new(init);
    node.put_radio(radio(mac(10), true)).unwrap();
    assert!(
        node.policies()
            .all(|policy| policy.id.net_id.as_str() == "OtherNet")
    );
}

/// A radio update keeps `radio_temp` (`dm_radio_list.cpp:161-167`).
#[test]
fn update_radio_keeps_radio_temp() {
    let mut node = agent_node();
    let mut first = radio(mac(10), true);
    first.radio_temp = 55;
    node.put_radio(first).unwrap();
    let mut update = radio(mac(10), false);
    update.radio_temp = 0;
    node.update_radio(update).unwrap();
    let stored = node.radio_by_mac(mac(10)).unwrap();
    assert_eq!(stored.radio_temp, 55);
    assert!(!stored.enabled, "every other field is replaced");
}

#[test]
fn update_radio_of_an_unknown_radio_is_not_found() {
    assert_eq!(
        agent_node().update_radio(radio(mac(10), true)),
        Err(ModelError::NotFound {
            table: Table::Radio
        })
    );
}

#[test]
fn commit_radio_seeds_no_policies() {
    let mut node = agent_node();
    let before = node.num_policies();
    node.commit_radio(radio(mac(10), true), None).unwrap();
    assert_eq!(node.num_policies(), before);
}

/// Upstream's `remove_radio` is empty (`dm_easy_mesh_list.cpp:394-397`); the port deletes the
/// record and its capability record. intentional-difference:u2-remove
#[test]
fn remove_radio_deletes_the_radio_and_its_capability() {
    let mut node = agent_node();
    node.put_radio(radio(mac(10), true)).unwrap();
    node.put_radio(radio(mac(11), true)).unwrap();
    node.put_radio_cap(radio_cap(mac(10))).unwrap();
    assert!(node.remove_radio(mac(10)).is_some());
    assert!(node.radio_by_mac(mac(10)).is_none());
    assert!(node.radio_cap(mac(10)).is_none());
    assert_eq!(node.radios().len(), 1);
    assert!(node.remove_radio(mac(10)).is_none());
}

/// `m_radio_cap[m_num_radios].init()` (`dm_easy_mesh.cpp:240-247`): a new radio committed
/// without a capability record stores a zeroed default at its index.
#[test]
fn commit_radio_of_a_new_radio_stores_a_default_capability_at_the_same_index() {
    let mut node = agent_node();
    node.commit_radio(radio(mac(10), true), Some(radio_cap(mac(10))))
        .unwrap();
    assert_eq!(
        node.commit_radio(radio(mac(11), true), None),
        Ok(RadioCommit {
            radio: Put::Inserted,
            cap: RadioCapOutcome::Defaulted
        })
    );
    assert_eq!(node.radios().len(), 2);
    assert_eq!(node.radio_caps().len(), 2);
    assert_eq!(node.radio_cap_at(0).unwrap().ruid.mac, mac(10));
    assert!(node.radio_cap_at(1).unwrap().ruid.mac.is_zero());
}

#[test]
fn commit_radio_of_an_existing_radio_keeps_its_capability() {
    let mut node = agent_node();
    node.commit_radio(radio(mac(10), true), Some(radio_cap(mac(10))))
        .unwrap();
    assert_eq!(
        node.commit_radio(radio(mac(10), false), None),
        Ok(RadioCommit {
            radio: Put::Replaced,
            cap: RadioCapOutcome::NotSupplied
        })
    );
    assert_eq!(node.radio_caps().len(), 1);
    assert_eq!(node.radio_cap(mac(10)).unwrap().ruid.mac, mac(10));
}

/// Upstream returns before any write when the radios are full (`dm_easy_mesh.cpp:235-238`): no
/// radio and no capability record is added.
#[test]
fn commit_radio_at_capacity_changes_neither_table() {
    let mut node = agent_node();
    for last in 10..13 {
        node.commit_radio(radio(mac(last), true), None).unwrap();
    }
    let result = node.commit_radio(radio(mac(20), true), Some(radio_cap(mac(20))));
    assert_eq!(
        result,
        Err(ModelError::Full {
            table: Table::Radio,
            capacity: MAX_RADIOS
        })
    );
    assert_eq!(node.radios().len(), 3);
    assert_eq!(node.radio_caps().len(), 3);
    assert!(node.radio_cap(mac(20)).is_none());
}

#[test]
fn remove_radio_also_removes_its_default_capability_at_the_same_index() {
    let mut node = agent_node();
    node.commit_radio(radio(mac(10), true), Some(radio_cap(mac(10))))
        .unwrap();
    node.commit_radio(radio(mac(11), true), None).unwrap();
    assert!(node.remove_radio(mac(11)).is_some());
    assert_eq!(node.radios().len(), 1);
    assert_eq!(node.radio_caps().len(), 1, "no orphaned default capability");
    assert_eq!(node.radio_cap_at(0).unwrap().ruid.mac, mac(10));
}
