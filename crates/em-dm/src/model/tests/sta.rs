// SPDX-License-Identifier: Apache-2.0

//! Station maps of a node: lookups, puts, merges, frame ownership and the association updates.

use super::fixtures::{agent_node, bss, mac, mac2, radio, sta};
use crate::enums::{HaulType, VapMode};
use crate::model::error::{ModelError, Put, Table};
use crate::model::keys::StaKey;
use crate::model::node::StaMapTarget::{Assoc, Consolidated, Disassoc};
use crate::records::{AffiliatedStaInfo, AssocStaMldInfo};
use crate::{BoundedVec, StaInfo};
use em_types::MacAddr;

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

fn with_frame(mut info: Box<StaInfo>, frame: &[u8]) -> Box<StaInfo> {
    info.set_frame_body(frame.to_vec()).unwrap();
    info
}

// --- find / first / next ---

#[test]
fn find_sta_valid_match() {
    let mut node = agent_node();
    node.put_sta(row(30, 20, 10));
    let found = node.find_sta(mac(30), mac(20)).unwrap();
    assert_eq!(found.radiomac, mac(10));
}

#[test]
fn find_sta_no_entries() {
    assert!(agent_node().find_sta(mac(30), mac(20)).is_none());
}

#[test]
fn find_sta_partial_mismatch_sta_mac() {
    let mut node = agent_node();
    node.put_sta(row(30, 20, 10));
    assert!(node.find_sta(mac(31), mac(20)).is_none());
}

#[test]
fn find_sta_partial_mismatch_bssid() {
    let mut node = agent_node();
    node.put_sta(row(30, 20, 10));
    assert!(node.find_sta(mac(30), mac(21)).is_none());
}

#[test]
fn find_sta_zero_mac_inputs() {
    let mut node = agent_node();
    node.put_sta(row(30, 20, 10));
    assert!(node.find_sta(MacAddr::ZERO, MacAddr::ZERO).is_none());
}

#[test]
fn find_sta_on_any_bss_tries_every_bss() {
    let mut node = agent_node();
    node.put_bss(bss(mac(10), mac(20), HaulType::FRONTHAUL, VapMode::AP))
        .unwrap();
    node.put_bss(bss(mac(10), mac(21), HaulType::FRONTHAUL, VapMode::AP))
        .unwrap();
    node.put_sta(row(30, 21, 10));
    assert_eq!(node.find_sta_on_any_bss(mac(30)).unwrap().bssid, mac(21));
    assert!(node.find_sta_on_any_bss(mac(31)).is_none());
}

#[test]
fn is_sta_associated_needs_the_flag() {
    let mut node = agent_node();
    let mut associated = row(30, 20, 10);
    associated.associated = true;
    node.put_sta(associated);
    node.put_sta(row(31, 20, 10));
    assert!(node.is_sta_associated(mac(20), mac(30)));
    assert!(!node.is_sta_associated(mac(20), mac(31)));
    assert!(!node.is_sta_associated(mac(20), mac(32)));
}

#[test]
fn first_sta_found() {
    let mut node = agent_node();
    node.put_sta(row(31, 20, 10));
    node.put_sta(row(30, 21, 10));
    node.put_sta(row(30, 20, 10));
    // Key order: the BSSID breaks the tie between the two rows of station 30.
    assert_eq!(node.first_sta(mac(30)).unwrap().row.bssid, mac(20));
}

#[test]
fn first_sta_empty_map() {
    assert!(agent_node().first_sta(mac(30)).is_none());
}

#[test]
fn first_sta_mac_not_found() {
    let mut node = agent_node();
    node.put_sta(row(31, 20, 10));
    assert!(node.first_sta(mac(30)).is_none());
}

#[test]
fn first_sta_looks_only_at_the_consolidated_map() {
    let mut node = agent_node();
    node.put_sta_info(row(30, 20, 10), Assoc);
    assert!(node.first_sta(mac(30)).is_none());
}

#[test]
fn next_sta_valid_next() {
    let mut node = agent_node();
    node.put_sta(row(30, 20, 10));
    node.put_sta(row(30, 21, 10));
    let next = node.next_sta(mac(30), &key(30, 20, 10)).unwrap();
    assert_eq!(next.row.bssid, mac(21));
    assert_eq!(next.key, key(30, 21, 10));
}

#[test]
fn next_sta_skips_other_stations_and_matches_the_mac() {
    let mut node = agent_node();
    node.put_sta(row(30, 20, 10));
    node.put_sta(row(30, 21, 10));
    node.put_sta(row(30, 22, 10));
    node.put_sta(row(31, 20, 10));
    assert_eq!(
        node.next_sta(mac(30), &key(30, 21, 10)).unwrap().row.bssid,
        mac(22)
    );
}

#[test]
fn next_sta_last_sta() {
    let mut node = agent_node();
    node.put_sta(row(30, 20, 10));
    assert!(node.next_sta(mac(30), &key(30, 20, 10)).is_none());
}

/// A previous row that is not stored (upstream: a null or foreign pointer) matches no row, so
/// there is no next one (`dm_easy_mesh.cpp:3129-3146`).
#[test]
fn next_sta_for_a_previous_key_that_is_not_stored_finds_nothing() {
    let mut node = agent_node();
    node.put_sta(row(30, 20, 10));
    node.put_sta(row(30, 21, 10));
    assert!(node.next_sta(mac(30), &key(30, 22, 10)).is_none());
}

#[test]
fn next_sta_mac_not_found() {
    let mut node = agent_node();
    node.put_sta(row(30, 20, 10));
    node.put_sta(row(30, 21, 10));
    assert!(node.next_sta(mac(99), &key(30, 20, 10)).is_none());
}

#[test]
fn next_sta_empty_sta_map() {
    assert!(agent_node().next_sta(mac(30), &key(30, 20, 10)).is_none());
}

#[test]
fn first_sta_info_per_target() {
    let mut node = agent_node();
    node.put_sta_info(row(30, 20, 10), Consolidated);
    node.put_sta_info(row(31, 20, 10), Assoc);
    node.put_sta_info(row(32, 20, 10), Disassoc);
    assert_eq!(node.first_sta_info(Consolidated).unwrap().row.id, mac(30));
    assert_eq!(node.first_sta_info(Assoc).unwrap().row.id, mac(31));
    assert_eq!(node.first_sta_info(Disassoc).unwrap().row.id, mac(32));
}

#[test]
fn first_sta_info_empty_map() {
    let node = agent_node();
    for target in [Consolidated, Assoc, Disassoc] {
        assert!(node.first_sta_info(target).is_none());
    }
}

#[test]
fn next_sta_info_valid_next() {
    let mut node = agent_node();
    node.put_sta_info(row(30, 20, 10), Consolidated);
    node.put_sta_info(row(31, 20, 10), Consolidated);
    assert_eq!(
        node.next_sta_info(Consolidated, &key(30, 20, 10))
            .unwrap()
            .row
            .id,
        mac(31)
    );
}

#[test]
fn next_sta_info_last_element() {
    let mut node = agent_node();
    node.put_sta_info(row(30, 20, 10), Consolidated);
    assert!(node.next_sta_info(Consolidated, &key(30, 20, 10)).is_none());
}

#[test]
fn next_sta_info_empty_map() {
    assert!(
        agent_node()
            .next_sta_info(Consolidated, &key(30, 20, 10))
            .is_none()
    );
}

/// Upstream searches the map for the pointer; a pointer that is not in it gives null.
#[test]
fn next_sta_info_for_a_key_that_is_not_stored() {
    let mut node = agent_node();
    node.put_sta_info(row(30, 20, 10), Consolidated);
    node.put_sta_info(row(32, 20, 10), Consolidated);
    assert!(node.next_sta_info(Consolidated, &key(31, 20, 10)).is_none());
}

#[test]
fn num_bss_for_associated_sta_counts_the_rows_of_the_station() {
    let mut node = agent_node();
    node.put_sta(row(30, 20, 10));
    node.put_sta(row(30, 21, 10));
    node.put_sta(row(31, 20, 10));
    assert_eq!(node.num_bss_for_associated_sta(mac(30)), 2);
    assert_eq!(node.num_bss_for_associated_sta(mac(32)), 0);
    assert_eq!(agent_node().num_bss_for_associated_sta(mac(30)), 0);
}

#[test]
fn sta_info_assoc_positive_and_negative() {
    let mut node = agent_node();
    node.put_sta_info(row(30, 20, 10), Assoc);
    assert!(node.sta_info(&key(30, 20, 10), Assoc).is_some());
    assert!(
        node.sta_info(&key(30, 21, 10), Assoc).is_none(),
        "wrong bssid"
    );
    assert!(node.sta_info(&key(30, 20, 10), Consolidated).is_none());
}

#[test]
fn sta_info_consolidated_positive_and_negative() {
    let mut node = agent_node();
    node.put_sta_info(row(30, 20, 10), Consolidated);
    assert!(node.sta_info(&key(30, 20, 10), Consolidated).is_some());
    assert!(node.sta_info(&key(31, 20, 10), Consolidated).is_none());
}

#[test]
fn has_at_least_one_associated_sta_positive_negative_and_empty() {
    let mut node = agent_node();
    assert!(!node.has_at_least_one_associated_sta());
    node.put_sta(row(30, 20, 10));
    assert!(!node.has_at_least_one_associated_sta());
    let mut associated = row(31, 20, 10);
    associated.associated = true;
    node.put_sta(associated);
    assert!(node.has_at_least_one_associated_sta());
}

#[test]
fn put_sta_info_into_the_assoc_and_consolidated_maps() {
    let mut node = agent_node();
    assert!(node.put_sta_info(row(30, 20, 10), Assoc));
    assert!(node.put_sta_info(row(31, 20, 10), Consolidated));
    assert_eq!(node.num_sta(Assoc), 1);
    assert_eq!(node.num_sta(Consolidated), 1);
    assert_eq!(node.num_sta(Disassoc), 0);
}

#[test]
fn put_sta_info_duplicate_keeps_the_stored_row() {
    let mut node = agent_node();
    let mut first = row(30, 20, 10);
    first.rcpi = 1;
    assert!(node.put_sta_info(first, Consolidated));
    let mut second = row(30, 20, 10);
    second.rcpi = 2;
    assert!(!node.put_sta_info(second, Consolidated));
    assert_eq!(
        node.sta_info(&key(30, 20, 10), Consolidated).unwrap().rcpi,
        1
    );
}

// --- list-level puts (copy_sta_info semantics) ---

#[test]
fn put_sta_replaces_a_row_with_copy_sta_info_semantics() {
    let mut node = agent_node();
    let mut first = row(30, 20, 10);
    first.rcpi = 1;
    first.ssid = crate::CStrBytes::new(b"old").unwrap();
    assert_eq!(node.put_sta(first), Put::Inserted);
    let mut second = row(30, 20, 10);
    second.rcpi = 2;
    assert_eq!(node.put_sta(second), Put::Replaced);
    let stored = node.sta_info(&key(30, 20, 10), Consolidated).unwrap();
    assert_eq!(stored.rcpi, 2);
    assert!(
        stored.ssid.is_empty(),
        "every field is replaced, not only the new ones"
    );
}

#[test]
fn update_sta_of_an_unknown_row_is_not_found() {
    let mut node = agent_node();
    assert_eq!(
        node.update_sta(row(30, 20, 10)),
        Err(ModelError::NotFound { table: Table::Sta })
    );
}

/// Upstream's `remove_sta` is empty (`dm_easy_mesh_list.cpp:758-761`).
/// intentional-difference:u2-remove
#[test]
fn remove_sta_deletes_the_row() {
    let mut node = agent_node();
    node.put_sta(row(30, 20, 10));
    assert!(node.remove_sta(&key(30, 20, 10)).is_some());
    assert!(node.remove_sta(&key(30, 20, 10)).is_none());
    assert_eq!(node.num_sta(Consolidated), 0);
}

/// Criterion 5 gate: the frame body limit is 11 426 accepted, 11 427 rejected, and a rejected
/// update keeps the old frame (0007 D4). Upstream clamps and stores a prefix (`dm_sta.cpp:671`).
/// intentional-difference:frame-limit-reject
#[test]
fn frame_body_accepts_11426_bytes_rejects_11427_and_keeps_the_old_frame() {
    let mut node = agent_node();
    node.put_sta(with_frame(row(30, 20, 10), &[7; 11_426]));
    let station = node.sta_info_mut(&key(30, 20, 10), Consolidated).unwrap();
    assert_eq!(station.frame_body.len(), 11_426);
    assert!(station.set_frame_body(vec![8; 11_427]).is_err());
    assert_eq!(station.frame_body.len(), 11_426);
    assert_eq!(station.frame_body.as_slice()[0], 7, "the old frame is kept");
    assert!(station.set_frame_body(vec![9; 11_426]).is_ok());
    assert_eq!(station.frame_body.as_slice()[0], 9);
}

// --- merges ---

#[test]
fn merge_assoc_sta_updates_adds_replaces_and_removes() {
    let mut node = agent_node();
    let mut old = row(30, 20, 10);
    old.rcpi = 1;
    node.put_sta(old);
    node.put_sta(row(31, 20, 10));
    let mut replacement = row(30, 20, 10);
    replacement.rcpi = 5;
    let outcome = node.merge_assoc_sta_updates(
        vec![replacement, row(32, 20, 10)],
        &[key(31, 20, 10), key(99, 20, 10)],
    );
    assert_eq!(
        (outcome.inserted, outcome.replaced, outcome.removed),
        (1, 1, 1)
    );
    assert_eq!(
        node.sta_info(&key(30, 20, 10), Consolidated).unwrap().rcpi,
        5
    );
    assert!(node.sta_info(&key(32, 20, 10), Consolidated).is_some());
    assert!(node.sta_info(&key(31, 20, 10), Consolidated).is_none());
}

#[test]
fn merge_sta_link_metrics_replaces_existing_rows_only() {
    let mut node = agent_node();
    node.put_sta(row(30, 20, 10));
    let mut metrics = row(30, 20, 10);
    metrics.last_ul_rate = 99;
    let mut unknown = row(31, 20, 10);
    unknown.last_ul_rate = 98;
    assert_eq!(node.merge_sta_link_metrics(vec![metrics, unknown]), 1);
    assert_eq!(
        node.sta_info(&key(30, 20, 10), Consolidated)
            .unwrap()
            .last_ul_rate,
        99
    );
    assert!(node.sta_info(&key(31, 20, 10), Consolidated).is_none());
}

// --- association updates ---

fn node_with_two_radios() -> crate::NodeModel {
    let mut node = agent_node();
    node.put_radio(radio(mac(10), true)).unwrap();
    node.put_radio(radio(mac(11), true)).unwrap();
    node.put_bss(bss(mac(10), mac(20), HaulType::FRONTHAUL, VapMode::AP))
        .unwrap();
    node.put_bss(bss(mac(11), mac(21), HaulType::FRONTHAUL, VapMode::AP))
        .unwrap();
    node
}

fn mld(station: u8, links: &[u8]) -> AssocStaMldInfo {
    let affiliated: Vec<_> = links
        .iter()
        .map(|&bssid| AffiliatedStaInfo {
            bssid: mac(bssid),
            link_addr: mac2(bssid, 1),
        })
        .collect();
    AssocStaMldInfo {
        mac_addr: mac(station),
        ap_mld_mac_addr: mac(60),
        str: false,
        nstr: false,
        emlsr: false,
        emlmr: false,
        affiliated_sta: BoundedVec::try_from_vec(affiliated).unwrap(),
    }
}

#[test]
fn apply_sta_mld_association_stages_resolved_links_with_the_cached_frame() {
    let mut node = node_with_two_radios();
    let mut cached = with_frame(row(30, 20, 10), &[1, 2, 3]);
    cached.associated = true;
    node.put_sta_info(cached, Assoc);
    let link_bssids = [20, 21, 77];
    let updated = node.apply_sta_mld_association(&mld(30, &link_bssids));
    assert!(updated);
    let first = node.sta_info(&key(30, 20, 10), Assoc).unwrap();
    assert!(first.associated);
    assert_eq!(first.frame_body.as_slice(), &[1, 2, 3]);
    let second = node.sta_info(&key(30, 21, 11), Assoc).unwrap();
    assert!(second.associated);
    assert_eq!(
        second.frame_body.as_slice(),
        &[1, 2, 3],
        "a copy for every link (F1)"
    );
    assert_eq!(
        node.num_sta(Assoc),
        2,
        "the unresolved link 77 stages nothing"
    );
}

#[test]
fn apply_sta_mld_association_with_no_resolved_link_updates_nothing() {
    let mut node = node_with_two_radios();
    assert!(!node.apply_sta_mld_association(&mld(30, &[77])));
    assert_eq!(node.num_sta(Assoc), 0);
}

#[test]
fn apply_sta_mld_association_clears_the_old_links_and_stages_a_copy_of_the_row() {
    let mut node = node_with_two_radios();
    let mut old = with_frame(row(30, 21, 11), &[4, 5]);
    old.associated = true;
    old.rcpi = 33;
    node.put_sta(old);
    assert!(node.apply_sta_mld_association(&mld(30, &[20])));
    let consolidated = node.sta_info(&key(30, 21, 11), Consolidated).unwrap();
    assert!(!consolidated.associated);
    let staged = node.sta_info(&key(30, 21, 11), Assoc).unwrap();
    assert!(!staged.associated);
    assert_eq!(
        staged.rcpi, 33,
        "the pending row is a copy of the whole row"
    );
    assert_eq!(staged.frame_body.as_slice(), &[4, 5]);
    let new_link = node.sta_info(&key(30, 20, 10), Assoc).unwrap();
    assert_eq!(
        new_link.frame_body.as_slice(),
        &[4, 5],
        "the cached frame follows the station"
    );
}

#[test]
fn apply_sta_disassociation_of_a_single_link_station() {
    let mut node = node_with_two_radios();
    assert!(node.apply_sta_disassociation(mac(30), mac(20)));
    let staged = node.sta_info(&key(30, 20, 10), Assoc).unwrap();
    assert!(!staged.associated);
}

#[test]
fn apply_sta_disassociation_of_an_unknown_bss_stages_a_zero_radio() {
    let mut node = node_with_two_radios();
    assert!(node.apply_sta_disassociation(mac(30), mac(77)));
    assert!(
        node.sta_info(
            &StaKey {
                sta: mac(30),
                bssid: mac(77),
                radio: MacAddr::ZERO
            },
            Assoc
        )
        .is_some()
    );
}

#[test]
fn apply_sta_disassociation_of_an_mld_station_stages_every_resolved_link() {
    let mut node = node_with_two_radios();
    node.update_assoc_sta_mld_info(&mld(30, &[20, 21])).unwrap();
    assert!(node.apply_sta_disassociation(mac(30), mac(60)));
    assert!(node.sta_info(&key(30, 20, 10), Assoc).is_some());
    assert!(node.sta_info(&key(30, 21, 11), Assoc).is_some());
    assert_eq!(node.num_sta(Assoc), 2, "no row for the MLD address itself");
}

#[test]
fn apply_sta_disassociation_resolves_an_ap_mld_address_through_its_first_link() {
    use crate::records::{AffiliatedApInfo, ApMldInfo};
    let mut node = agent_node();
    let affiliated = AffiliatedApInfo {
        mac_addr_valid: true,
        link_id_valid: true,
        ruid: super::fixtures::interface(mac(10)),
        mac_addr: mac(20),
        link_id: 0,
    };
    let ap_mld = ApMldInfo {
        mac_addr_valid: true,
        ssid: crate::CStrBytes::default(),
        mac_addr: mac(60),
        str: false,
        nstr: false,
        emlsr: false,
        emlmr: false,
        haul_type: HaulType::FRONTHAUL,
        affiliated_ap: BoundedVec::try_from_vec(vec![affiliated]).unwrap(),
    };
    node.update_ap_mld_info(ap_mld).unwrap();
    assert!(node.apply_sta_disassociation(mac(30), mac(60)));
    assert!(node.sta_info(&key(30, 60, 10), Assoc).is_some());
}

/// After a disassociation the consolidated row exists, so the pending row is a full copy and the
/// consolidated flag is cleared too.
#[test]
fn apply_sta_disassociation_of_a_known_station_updates_both_maps() {
    let mut node = node_with_two_radios();
    let mut known = with_frame(row(30, 20, 10), &[6]);
    known.associated = true;
    node.put_sta(known);
    node.apply_sta_disassociation(mac(30), mac(20));
    assert!(
        !node
            .sta_info(&key(30, 20, 10), Consolidated)
            .unwrap()
            .associated
    );
    let staged = node.sta_info(&key(30, 20, 10), Assoc).unwrap();
    assert!(!staged.associated);
    assert_eq!(staged.frame_body.as_slice(), &[6]);
}

#[test]
fn take_sta_rows_drains_the_pending_map() {
    let mut node = agent_node();
    node.put_sta_info(row(30, 20, 10), Assoc);
    node.put_sta_info(row(31, 20, 10), Assoc);
    let rows = node.take_sta_rows(Assoc);
    assert_eq!(rows.len(), 2);
    assert_eq!(node.num_sta(Assoc), 0);
}
