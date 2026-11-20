// SPDX-License-Identifier: Apache-2.0

//! `NetworkModel`: the SSID table (cap 5, exact matching), the record and the shared view.

use super::fixtures::{mac, net, network_ssid};
use crate::NetworkInfo;
use crate::enums::HaulType;
use crate::model::error::{ModelError, Put, Table};
use crate::model::keys::{NetId, SsidKey};
use crate::model::network::{MAX_NET_SSIDS, NetworkModel};
use crate::text::CStrBytes;
use em_types::MediaType;

fn ssid_key(id: &str) -> SsidKey {
    SsidKey::new(id.as_bytes()).unwrap()
}

fn ssid_text(text: &str) -> CStrBytes<32> {
    CStrBytes::new(text.as_bytes()).unwrap()
}

fn full_network() -> NetworkModel {
    let mut network = NetworkModel::new(NetId::global());
    for index in 0..MAX_NET_SSIDS {
        network
            .put_network_ssid(network_ssid(
                &format!("id{index}"),
                "ssid",
                &[HaulType::FRONTHAUL],
            ))
            .unwrap();
    }
    network
}

#[test]
fn a_new_network_model_is_a_zeroed_record_with_its_id_and_no_ssids() {
    let network = NetworkModel::new(net("Net7"));
    assert_eq!(network.net(), net("Net7"));
    assert_eq!(network.info().id.as_str(), "Net7");
    assert!(network.controller_interface_mac().is_zero());
    assert_eq!(network.num_network_ssids(), 0);
    assert_eq!(network.ssids().len(), 0);
}

#[test]
fn ssid_capacity_is_5() {
    assert_eq!(MAX_NET_SSIDS, 5, "EM_MAX_NET_SSIDS, inc/em_base.h:41");
}

/// Upstream writes past `m_network_ssid` on the sixth row (0007 defect 1, research 7.14); the
/// port refuses it and changes nothing. intentional-difference:c6-capacity
#[test]
fn inserting_an_ssid_at_capacity_is_an_error_and_changes_nothing() {
    let mut network = full_network();
    let result = network.put_network_ssid(network_ssid("extra", "ssid", &[]));
    assert_eq!(
        result,
        Err(ModelError::Full {
            table: Table::NetworkSsid,
            capacity: 5
        })
    );
    assert_eq!(network.num_network_ssids(), 5);
    assert!(network.find_network_ssid(&ssid_key("extra")).is_none());
}

#[test]
fn updating_an_ssid_at_capacity_succeeds() {
    let mut network = full_network();
    let result = network.put_network_ssid(network_ssid("id2", "changed", &[HaulType::BACKHAUL]));
    assert_eq!(result, Ok(Put::Replaced));
    assert_eq!(network.num_network_ssids(), 5);
    let stored = network.find_network_ssid(&ssid_key("id2")).unwrap();
    assert_eq!(stored.ssid.as_bytes(), b"changed");
}

#[test]
fn put_network_ssid_replaces_the_row_with_the_same_id_in_place() {
    let mut network = NetworkModel::new(NetId::global());
    assert_eq!(
        network.put_network_ssid(network_ssid("a", "one", &[])),
        Ok(Put::Inserted)
    );
    network
        .put_network_ssid(network_ssid("b", "two", &[]))
        .unwrap();
    assert_eq!(
        network.put_network_ssid(network_ssid("a", "three", &[])),
        Ok(Put::Replaced)
    );
    assert_eq!(network.num_network_ssids(), 2);
    assert_eq!(network.network_ssid(0).unwrap().ssid.as_bytes(), b"three");
    assert_eq!(network.network_ssid(1).unwrap().ssid.as_bytes(), b"two");
}

/// `network_ssid` identity is the full id, matched exactly (A4).
#[test]
fn ssid_lookup_is_an_exact_match_on_the_id() {
    let mut network = NetworkModel::new(NetId::global());
    network
        .put_network_ssid(network_ssid("Ssid1", "one", &[]))
        .unwrap();
    network
        .put_network_ssid(network_ssid("Ssid10", "ten", &[]))
        .unwrap();
    assert_eq!(
        network
            .find_network_ssid(&ssid_key("Ssid1"))
            .unwrap()
            .ssid
            .as_bytes(),
        b"one"
    );
    assert_eq!(
        network
            .find_network_ssid(&ssid_key("Ssid10"))
            .unwrap()
            .ssid
            .as_bytes(),
        b"ten"
    );
    assert!(network.find_network_ssid(&ssid_key("Ssid")).is_none());
    assert!(network.find_network_ssid(&ssid_key("Ssid100")).is_none());
}

/// Upstream removes by prefix and leaves a zeroed hole while the count drops
/// (`dm_easy_mesh_list.cpp:925-928`). intentional-difference:c4-exact-keys
#[test]
fn removing_an_ssid_matches_exactly_and_keeps_the_order_of_the_others() {
    let mut network = NetworkModel::new(NetId::global());
    for (id, ssid) in [("Ssid1", "one"), ("Ssid10", "ten"), ("Ssid2", "two")] {
        network
            .put_network_ssid(network_ssid(id, ssid, &[]))
            .unwrap();
    }
    assert!(network.remove_network_ssid(&ssid_key("Ssid")).is_none());
    let removed = network.remove_network_ssid(&ssid_key("Ssid1")).unwrap();
    assert_eq!(removed.ssid.as_bytes(), b"one");
    assert_eq!(network.num_network_ssids(), 2);
    assert_eq!(network.network_ssid(0).unwrap().ssid.as_bytes(), b"ten");
    assert_eq!(network.network_ssid(1).unwrap().ssid.as_bytes(), b"two");
}

#[test]
fn ssid_by_index_beyond_the_count_is_none() {
    let network = full_network();
    assert!(network.network_ssid(4).is_some());
    assert!(network.network_ssid(5).is_none());
}

#[test]
fn is_ssid_match_is_exact() {
    let mut network = NetworkModel::new(NetId::global());
    network
        .put_network_ssid(network_ssid("a", "HomeNet", &[]))
        .unwrap();
    assert!(network.is_ssid_match(&ssid_text("HomeNet")));
    assert!(!network.is_ssid_match(&ssid_text("HomeNe")));
    assert!(!network.is_ssid_match(&ssid_text("HomeNet2")));
    assert!(!NetworkModel::new(NetId::global()).is_ssid_match(&ssid_text("HomeNet")));
}

#[test]
fn ssid_by_haul_type_finds_the_first_row_that_carries_it() {
    let mut network = NetworkModel::new(NetId::global());
    network
        .put_network_ssid(network_ssid("a", "front", &[HaulType::FRONTHAUL]))
        .unwrap();
    network
        .put_network_ssid(network_ssid(
            "b",
            "both",
            &[HaulType::FRONTHAUL, HaulType::BACKHAUL],
        ))
        .unwrap();
    assert_eq!(
        network
            .network_ssid_by_haul_type(HaulType::FRONTHAUL)
            .unwrap()
            .ssid
            .as_bytes(),
        b"front"
    );
    assert_eq!(
        network
            .network_ssid_by_haul_type(HaulType::BACKHAUL)
            .unwrap()
            .ssid
            .as_bytes(),
        b"both"
    );
    assert!(network.network_ssid_by_haul_type(HaulType::IOT).is_none());
}

#[test]
fn haul_type_from_ssid_reads_the_first_haul_type_of_the_first_row_with_one() {
    let mut network = NetworkModel::new(NetId::global());
    network
        .put_network_ssid(network_ssid("a", "net", &[]))
        .unwrap();
    network
        .put_network_ssid(network_ssid(
            "b",
            "net",
            &[HaulType::BACKHAUL, HaulType::IOT],
        ))
        .unwrap();
    assert_eq!(
        network.haul_type_from_ssid(&ssid_text("net")),
        Some(HaulType::BACKHAUL)
    );
    assert_eq!(network.haul_type_from_ssid(&ssid_text("other")), None);
    assert_eq!(network.haul_type_from_ssid(&ssid_text("")), None);
}

#[test]
fn the_controller_writes_change_the_one_network_record() {
    let mut network = NetworkModel::new(NetId::global());
    network.set_controller_id(mac(9));
    network.set_controller_intf_media(MediaType::IEEE_802_3AB);
    network.set_colocated_agent_interface_mac(mac(8));
    network.set_colocated_agent_interface_name("eth0").unwrap();
    assert_eq!(network.controller_interface_mac(), mac(9));
    assert_eq!(network.info().media, MediaType::IEEE_802_3AB);
    assert_eq!(network.colocated_agent_interface().mac, mac(8));
    assert_eq!(
        network
            .colocated_agent_interface()
            .name
            .as_ref()
            .unwrap()
            .as_str(),
        "eth0"
    );
    assert!(network.set_colocated_agent_interface_name("").is_err());
}

#[test]
fn set_info_replaces_the_record_whole() {
    let mut network = NetworkModel::new(net("A"));
    let mut record = Box::new(NetworkInfo::zeroed(net("A").as_text()));
    record.ctrl_id.mac = mac(3);
    network.set_info(record);
    assert_eq!(network.controller_interface_mac(), mac(3));
}
