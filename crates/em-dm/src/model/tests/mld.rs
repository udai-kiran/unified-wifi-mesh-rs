// SPDX-License-Identifier: Apache-2.0

//! AP MLDs, associated-STA MLDs and the backhaul-STA MLD.

use super::fixtures::{affiliated_ap, agent_node, ap_mld, mac};
use crate::bounded::BoundedVec;
use crate::enums::HaulType;
use crate::model::error::{ModelError, Table};
use crate::model::node::{MAX_AP_MLD, MAX_ASSOC_STA_MLD};
use crate::records::{AffiliatedStaInfo, ApMldInfo, AssocStaMldInfo};
use crate::text::CStrBytes;

/// `em_haul_type_max` is 5: the haul types 0 to 4 are valid (`inc/em_base.h`).
fn haul(value: u16) -> HaulType {
    HaulType(value)
}

fn sta_mld(mld_mac: u8, links: &[(u8, u8)]) -> AssocStaMldInfo {
    let affiliated: Vec<AffiliatedStaInfo> = links
        .iter()
        .map(|&(bssid, link)| AffiliatedStaInfo {
            bssid: mac(bssid),
            link_addr: mac(link),
        })
        .collect();
    AssocStaMldInfo {
        mac_addr: mac(mld_mac),
        ap_mld_mac_addr: mac(60),
        str: true,
        nstr: false,
        emlsr: false,
        emlmr: false,
        affiliated_sta: BoundedVec::try_from_vec(affiliated).unwrap(),
    }
}

// --- AP MLD ---

#[test]
fn ap_mld_capacity_is_64() {
    assert_eq!(MAX_AP_MLD, 64, "EM_MAX_AP_MLD, inc/em_base.h:233");
    assert_eq!(
        MAX_ASSOC_STA_MLD, 64,
        "EM_MAX_ASSOC_STA_MLD, inc/em_base.h:234"
    );
}

#[test]
fn update_ap_mld_info_creates_a_new_mld() {
    let mut node = agent_node();
    let skipped = node
        .update_ap_mld_info(ap_mld(
            HaulType::FRONTHAUL,
            mac(40),
            vec![affiliated_ap(mac(10), mac(41), 0)],
        ))
        .unwrap();
    assert_eq!(skipped, Vec::<usize>::new());
    assert_eq!(node.ap_mlds().len(), 1);
    let stored = node.ap_mld(0).unwrap();
    assert_eq!(stored.mac_addr, mac(40));
    assert!(stored.mac_addr_valid);
    assert_eq!(stored.haul_type, HaulType::FRONTHAUL);
    assert_eq!(stored.affiliated_ap.len(), 1);
    assert_eq!(stored.affiliated_ap.as_slice()[0].mac_addr, mac(41));
}

/// AP MLDs are identified by haul type alone (`dm_easy_mesh.cpp:3711`).
#[test]
fn update_ap_mld_info_updates_the_mld_of_the_same_haul_type() {
    let mut node = agent_node();
    node.update_ap_mld_info(ap_mld(
        HaulType::FRONTHAUL,
        mac(40),
        vec![affiliated_ap(mac(10), mac(41), 0)],
    ))
    .unwrap();
    let mut radio_only = affiliated_ap(mac(10), mac(42), 1);
    radio_only.mac_addr_valid = false;
    let mut second = ap_mld(HaulType::FRONTHAUL, mac(50), vec![radio_only]);
    second.nstr = true;
    second.str = false;
    node.update_ap_mld_info(second).unwrap();
    assert_eq!(node.ap_mlds().len(), 1);
    let stored = node.ap_mld(0).unwrap();
    assert_eq!(stored.mac_addr, mac(50));
    assert!(
        stored.nstr && !stored.str,
        "the mode flags are always overwritten"
    );
    assert_eq!(
        stored.affiliated_ap.len(),
        1,
        "matched by radio when the MACs differ"
    );
}

#[test]
fn update_ap_mld_info_keeps_the_mac_and_ssid_when_the_input_has_none() {
    let mut node = agent_node();
    node.update_ap_mld_info(ap_mld(HaulType::FRONTHAUL, mac(40), vec![]))
        .unwrap();
    let mut bare = ap_mld(HaulType::FRONTHAUL, mac(99), vec![]);
    bare.mac_addr_valid = false;
    bare.ssid = CStrBytes::default();
    node.update_ap_mld_info(bare).unwrap();
    let stored = node.ap_mld(0).unwrap();
    assert_eq!(stored.mac_addr, mac(40));
    assert_eq!(stored.ssid.as_bytes(), b"mld-ssid");
}

#[test]
fn update_ap_mld_info_matches_an_affiliated_ap_by_link_mac_when_both_are_valid() {
    let mut node = agent_node();
    node.update_ap_mld_info(ap_mld(
        HaulType::FRONTHAUL,
        mac(40),
        vec![affiliated_ap(mac(10), mac(41), 0)],
    ))
    .unwrap();
    // Same link MAC on another radio: the same affiliated AP.
    node.update_ap_mld_info(ap_mld(
        HaulType::FRONTHAUL,
        mac(40),
        vec![affiliated_ap(mac(11), mac(41), 2)],
    ))
    .unwrap();
    let aps = node.ap_mld(0).unwrap().affiliated_ap.as_slice();
    assert_eq!(aps.len(), 1);
    assert_eq!(aps[0].ruid.mac, mac(11), "ruid is always overwritten");
    assert_eq!(aps[0].link_id, 2);
    // A different link MAC adds a second one.
    node.update_ap_mld_info(ap_mld(
        HaulType::FRONTHAUL,
        mac(40),
        vec![affiliated_ap(mac(10), mac(43), 3)],
    ))
    .unwrap();
    assert_eq!(node.ap_mld(0).unwrap().affiliated_ap.len(), 2);
}

/// `dm_easy_mesh.cpp:3787-3791`: an affiliated AP beyond 64 is skipped.
#[test]
fn update_ap_mld_info_skips_affiliated_aps_beyond_64() {
    let mut node = agent_node();
    let first: Vec<_> = (0..64_u8)
        .map(|index| affiliated_ap(mac(index), mac2_link(index), index))
        .collect();
    assert_eq!(
        node.update_ap_mld_info(ap_mld(HaulType::FRONTHAUL, mac(40), first)),
        Ok(Vec::new())
    );
    let skipped = node
        .update_ap_mld_info(ap_mld(
            HaulType::FRONTHAUL,
            mac(40),
            vec![affiliated_ap(mac(200), mac(201), 0)],
        ))
        .unwrap();
    assert_eq!(skipped, vec![0]);
    assert_eq!(node.ap_mld(0).unwrap().affiliated_ap.len(), 64);
}

fn mac2_link(index: u8) -> crate::MacAddr {
    super::fixtures::mac2(1, index)
}

/// `dm_easy_mesh.cpp:3706`: a haul type at or above `em_haul_type_max` is refused.
#[test]
fn update_ap_mld_info_refuses_an_invalid_haul_type() {
    let mut node = agent_node();
    let result = node.update_ap_mld_info(ap_mld(haul(5), mac(40), vec![]));
    assert_eq!(result, Err(ModelError::InvalidHaulType(haul(5))));
    assert_eq!(node.ap_mlds().len(), 0);
}

/// intentional-difference:c6-capacity
/// `dm_easy_mesh.cpp:3727`: the table holds at most 64 AP MLDs. Rows are identified by haul
/// type and only the five haul types below `em_haul_type_max` are accepted, so the cap cannot
/// be reached through `update_ap_mld_info`; the bound is the type's capacity.
#[test]
fn the_ap_mld_table_holds_one_row_per_valid_haul_type_and_is_bounded_at_64() {
    let mut node = agent_node();
    for index in 0..5_u16 {
        node.update_ap_mld_info(ap_mld(haul(index), mac(40), vec![]))
            .unwrap();
    }
    assert_eq!(node.ap_mlds().len(), 5);
    assert_eq!(
        node.update_ap_mld_info(ap_mld(haul(5), mac(40), vec![])),
        Err(ModelError::InvalidHaulType(haul(5)))
    );
    assert_eq!(node.ap_mlds().len(), 5);
    assert_eq!(BoundedVec::<ApMldInfo, MAX_AP_MLD>::CAPACITY, 64);
}

/// Upstream returns a zeroed slot for any index below 64 (`inc/dm_easy_mesh.h:1635`) and null
/// beyond; the port returns `None` at and beyond the count.
/// intentional-difference:index-bounds
#[test]
fn get_ap_mld_by_index_and_beyond_the_count() {
    let mut node = agent_node();
    assert!(node.ap_mld(0).is_none(), "no AP MLD configured");
    node.update_ap_mld_info(ap_mld(HaulType::FRONTHAUL, mac(40), vec![]))
        .unwrap();
    assert!(node.ap_mld(0).is_some());
    assert!(node.ap_mld(1).is_none());
    assert!(node.ap_mld(64).is_none());
}

/// `get_ap_mld_by_ref(index)` out of range: no row (the ambiguous triage row).
/// intentional-difference:index-bounds
#[test]
fn ap_mld_by_ref_index_beyond_the_count_is_none() {
    let mut node = agent_node();
    assert!(node.ap_mld_mut(0).is_none());
    node.update_ap_mld_info(ap_mld(HaulType::FRONTHAUL, mac(40), vec![]))
        .unwrap();
    assert!(node.ap_mld_mut(0).is_some());
    assert!(node.ap_mld_mut(1).is_none());
}

#[test]
fn ap_mld_by_haul_type_and_the_invalid_haul_types() {
    let mut node = agent_node();
    node.update_ap_mld_info(ap_mld(HaulType::BACKHAUL, mac(40), vec![]))
        .unwrap();
    assert!(node.ap_mld_by_haul_type(HaulType::BACKHAUL).is_some());
    assert!(node.ap_mld_by_haul_type(HaulType::FRONTHAUL).is_none());
    assert!(node.ap_mld_by_haul_type(haul(5)).is_none());
}

#[test]
fn remove_ap_mld_info_by_haul_type_shifts_the_later_ones() {
    let mut node = agent_node();
    for (kind, last) in [
        (HaulType::FRONTHAUL, 40),
        (HaulType::BACKHAUL, 41),
        (HaulType::IOT, 42),
    ] {
        node.update_ap_mld_info(ap_mld(kind, mac(last), vec![]))
            .unwrap();
    }
    assert!(
        node.remove_ap_mld_info_by_haul_type(HaulType::BACKHAUL)
            .is_some()
    );
    assert!(
        node.remove_ap_mld_info_by_haul_type(HaulType::BACKHAUL)
            .is_none()
    );
    assert!(node.remove_ap_mld_info_by_haul_type(haul(5)).is_none());
    let hauls: Vec<_> = node.ap_mlds().iter().map(|mld| mld.haul_type).collect();
    assert_eq!(hauls, vec![HaulType::FRONTHAUL, HaulType::IOT]);
}

#[test]
fn is_ap_mld_mac_needs_a_valid_mld_with_that_mac() {
    let mut node = agent_node();
    let mut invalid = ap_mld(HaulType::BACKHAUL, mac(41), vec![]);
    invalid.mac_addr_valid = false;
    node.update_ap_mld_info(ap_mld(HaulType::FRONTHAUL, mac(40), vec![]))
        .unwrap();
    node.update_ap_mld_info(invalid).unwrap();
    assert!(node.is_ap_mld_mac(mac(40)));
    assert!(
        !node.is_ap_mld_mac(mac(41)),
        "the MAC of an invalid MLD is not an MLD MAC"
    );
    assert!(!node.is_ap_mld_mac(mac(42)));
}

#[test]
fn resolve_ap_mld_to_fallback_ruid_takes_the_first_valid_affiliated_ap() {
    let mut node = agent_node();
    let mut invalid_link = affiliated_ap(mac(9), mac(8), 0);
    invalid_link.mac_addr_valid = false;
    node.update_ap_mld_info(ap_mld(
        HaulType::FRONTHAUL,
        mac(40),
        vec![invalid_link, affiliated_ap(mac(10), mac(41), 1)],
    ))
    .unwrap();
    assert_eq!(node.resolve_ap_mld_to_fallback_ruid(mac(40)), Some(mac(10)));
    assert_eq!(node.resolve_ap_mld_to_fallback_ruid(mac(99)), None);
}

// --- associated-STA MLD ---

#[test]
fn update_assoc_sta_mld_info_adds_a_new_entry() {
    let mut node = agent_node();
    let skipped = node
        .update_assoc_sta_mld_info(&sta_mld(70, &[(20, 71), (21, 72)]))
        .unwrap();
    assert_eq!(skipped, 0);
    assert_eq!(node.assoc_sta_mlds().len(), 1);
    assert_eq!(node.assoc_sta_mlds()[0].affiliated_sta.len(), 2);
    assert!(node.is_sta_mld(mac(70)));
    assert!(!node.is_sta_mld(mac(71)));
}

/// The affiliated STAs merge incrementally by BSSID (`dm_easy_mesh.cpp:3828-3895`).
#[test]
fn update_assoc_sta_mld_info_merges_the_links_by_bssid() {
    let mut node = agent_node();
    node.update_assoc_sta_mld_info(&sta_mld(70, &[(20, 71), (21, 72)]))
        .unwrap();
    node.update_assoc_sta_mld_info(&sta_mld(70, &[(21, 99), (22, 73)]))
        .unwrap();
    let stored = &node.assoc_sta_mlds()[0];
    let links: Vec<_> = stored
        .affiliated_sta
        .iter()
        .map(|link| (link.bssid, link.link_addr))
        .collect();
    assert_eq!(
        links,
        vec![(mac(20), mac(71)), (mac(21), mac(99)), (mac(22), mac(73))],
        "a known link takes the new address, an absent link stays, a new one is added"
    );
}

/// `dm_easy_mesh.cpp:3846-3849`: a new entry at 64 is refused, an existing one still updates.
#[test]
fn update_assoc_sta_mld_info_at_capacity_refuses_a_new_entry_and_updates_an_existing_one() {
    let mut node = agent_node();
    for index in 0..64_u8 {
        node.update_assoc_sta_mld_info(&sta_mld(index, &[]))
            .unwrap();
    }
    assert_eq!(
        node.update_assoc_sta_mld_info(&sta_mld(200, &[])),
        Err(ModelError::Full {
            table: Table::AssocStaMld,
            capacity: 64
        })
    );
    assert_eq!(node.assoc_sta_mlds().len(), 64);
    assert_eq!(
        node.update_assoc_sta_mld_info(&sta_mld(3, &[(20, 71)])),
        Ok(0)
    );
    assert_eq!(node.assoc_sta_mlds().len(), 64);
    assert_eq!(node.assoc_sta_mlds()[3].affiliated_sta.len(), 1);
}

#[test]
fn update_assoc_sta_mld_info_skips_links_beyond_64() {
    let mut node = agent_node();
    let links: Vec<(u8, u8)> = (0..64_u8).map(|index| (index, index)).collect();
    node.update_assoc_sta_mld_info(&sta_mld(70, &links))
        .unwrap();
    let skipped = node
        .update_assoc_sta_mld_info(&sta_mld(70, &[(200, 201)]))
        .unwrap();
    assert_eq!(skipped, 1);
    assert_eq!(node.assoc_sta_mlds()[0].affiliated_sta.len(), 64);
}

#[test]
fn replace_assoc_sta_mld_info_replaces_the_entry_whole() {
    let mut node = agent_node();
    node.update_assoc_sta_mld_info(&sta_mld(70, &[(20, 71), (21, 72)]))
        .unwrap();
    node.replace_assoc_sta_mld_info(sta_mld(70, &[(22, 73)]))
        .unwrap();
    assert_eq!(node.assoc_sta_mlds().len(), 1);
    let links: Vec<_> = node.assoc_sta_mlds()[0]
        .affiliated_sta
        .iter()
        .map(|link| link.bssid)
        .collect();
    assert_eq!(
        links,
        vec![mac(22)],
        "absent links are dropped, unlike the merge"
    );
}

/// `dm_easy_mesh.cpp:3947-3950`: at 64 the new row is skipped.
#[test]
fn replace_assoc_sta_mld_info_at_capacity_refuses_a_new_entry() {
    let mut node = agent_node();
    for index in 0..64_u8 {
        node.replace_assoc_sta_mld_info(sta_mld(index, &[]))
            .unwrap();
    }
    assert_eq!(
        node.replace_assoc_sta_mld_info(sta_mld(200, &[])),
        Err(ModelError::Full {
            table: Table::AssocStaMld,
            capacity: 64
        })
    );
    node.replace_assoc_sta_mld_info(sta_mld(5, &[(20, 71)]))
        .unwrap();
    assert_eq!(node.assoc_sta_mlds().len(), 64);
    assert_eq!(node.assoc_sta_mlds()[5].affiliated_sta.len(), 1);
}

/// `update_bsta_mld_info` is an empty `TODO` upstream (`dm_easy_mesh.cpp:3822-3826`); the port
/// records it as not ported (method map) and exposes the record for the engine's direct writes,
/// as upstream's only writers do. intentional-difference:a4-update-bsta-mld-not-ported
#[test]
fn the_bsta_mld_record_is_written_through_direct_access() {
    let mut node = agent_node();
    assert!(!node.is_bsta_mld_present());
    node.set_bsta_mld_present(true);
    node.bsta_mld_info_mut().mac_addr = mac(80);
    node.bsta_mld_info_mut().mac_addr_valid = true;
    assert!(node.is_bsta_mld_present());
    assert_eq!(node.bsta_mld_info().mac_addr, mac(80));
}
