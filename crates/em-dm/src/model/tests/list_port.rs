// SPDX-License-Identifier: Apache-2.0

//! The ports of upstream's `tests/test_l1_dm_easy_mesh_list.cpp` (plan A10, list triage in
//! `tests/data/list_test_triage.tsv`). Each test names the upstream test it ports; the mapping
//! with the class and the tag is `tests/data/upstream_test_map.tsv`.
//!
//! The fixture is [`upstream_fixture`](super::mesh_support::upstream_fixture). Tests that assert
//! a change on purpose carry `intentional-difference:<id>` in their doc comment.

use super::fixtures::{self, mac, net};
use super::mesh_support::{
    MAC1, MAC2, MAC3, MAC4, bss_in, bss_key, device_id, device_in, init, node_id, policy_in,
    policy_key, radio_in, scan_in, scan_key_in, sta_on, upstream_fixture, zeroed_network,
};
use crate::enums::{HaulType, OpClassType, PolicyIdType, ScannerType};
use crate::model::access::ModelAccess;
use crate::model::keys::{OpClassKey, SsidKey, StaKey};
use crate::model::node::NodeRole;
use crate::{MeshModel, ModelError, NetId, Put, Table};
use em_types::{MacAddr, MediaType, ProfileType};

fn ssid_key(id: &str) -> SsidKey {
    SsidKey::new(id.as_bytes()).unwrap()
}

fn ssid_text(text: &str) -> crate::CStrBytes<32> {
    crate::CStrBytes::new(text.as_bytes()).unwrap()
}

// --- network_ssid fan-out tests (:196, :232) ---

/// Upstream `network_ssid_update_without_database_reaches_all_models` (:196): two puts of
/// `Fronthaul@Network1`; every Network1 node sees the new SSID and passphrase, the Network2
/// nodes see none. The SSID is stored once in the network, so the nodes read the same row (no
/// per-node copy). intentional-difference:u1-network-stored-once
#[test]
fn network_ssid_update_without_database_reaches_all_models() {
    let mut mesh = upstream_fixture();
    let network1 = net("Network1");
    let mut old = fixtures::network_ssid("Fronthaul@Network1", "old-ssid", &[HaulType::FRONTHAUL]);
    old.pass_phrase = crate::Secret::new(crate::CStrBytes::new(b"old-passphrase").unwrap());
    assert_eq!(mesh.put_network_ssid(&network1, old), Ok(Put::Inserted));
    let mut new = fixtures::network_ssid("Fronthaul@Network1", "new-ssid", &[HaulType::FRONTHAUL]);
    new.pass_phrase = crate::Secret::new(crate::CStrBytes::new(b"new-passphrase").unwrap());
    assert_eq!(mesh.put_network_ssid(&network1, new), Ok(Put::Replaced));

    for node in [node_id("Network1", MAC1), node_id("Network1", MAC2)] {
        let view = mesh.node(&node).unwrap();
        assert_eq!(view.network.num_network_ssids(), 1);
        let ssid = view.network.network_ssid(0).unwrap();
        assert_eq!(ssid.ssid, ssid_text("new-ssid"));
        assert_eq!(ssid.pass_phrase.expose().as_bytes(), b"new-passphrase");
    }
    for node in [node_id("Network2", MAC3), node_id("Network2", MAC4)] {
        assert_eq!(mesh.node(&node).unwrap().network.num_network_ssids(), 0);
    }
}

/// Upstream `network_ssid_propagation_requires_exact_ids` (:232): Network1 and Network10 nodes,
/// `Fronthaul@Network1Extra` seeded on Network1 and `Fronthaul@Network10` on Network10. Two puts
/// of `Fronthaul@Network1` give Network1 two SSIDs (Extra unchanged, the new id updated) and
/// leave Network10 alone. intentional-difference:u1-network-stored-once
#[test]
fn network_ssid_propagation_requires_exact_ids() {
    let mut mesh = MeshModel::new(None);
    for (network, al) in [
        ("Network1", MAC1),
        ("Network1", MAC2),
        ("Network10", MAC3),
        ("Network10", MAC4),
    ] {
        mesh.create_data_model(init(
            network,
            al,
            ProfileType::MULTI_AP_PROFILE_3,
            NodeRole::Agent,
        ))
        .unwrap();
    }
    mesh.put_network_ssid(
        &net("Network1"),
        fixtures::network_ssid("Fronthaul@Network1Extra", "original", &[]),
    )
    .unwrap();
    mesh.put_network_ssid(
        &net("Network10"),
        fixtures::network_ssid("Fronthaul@Network10", "original", &[]),
    )
    .unwrap();
    assert!(
        mesh.get_network_ssid(&ssid_key("Fronthaul@Network1"))
            .is_none()
    );

    let incoming = |text: &str| fixtures::network_ssid("Fronthaul@Network1", text, &[]);
    mesh.put_network_ssid(&net("Network1"), incoming("new-ssid"))
        .unwrap();
    mesh.put_network_ssid(&net("Network1"), incoming("updated-ssid"))
        .unwrap();

    for al in [MAC1, MAC2] {
        let view = mesh.node(&node_id("Network1", al)).unwrap();
        assert_eq!(view.network.num_network_ssids(), 2);
        let first = view.network.network_ssid(0).unwrap();
        assert_eq!(first.id.as_bytes(), b"Fronthaul@Network1Extra");
        assert_eq!(first.ssid, ssid_text("original"));
        let second = view.network.network_ssid(1).unwrap();
        assert_eq!(second.id.as_bytes(), b"Fronthaul@Network1");
        assert_eq!(second.ssid, ssid_text("updated-ssid"));
    }
    for al in [MAC3, MAC4] {
        let view = mesh.node(&node_id("Network10", al)).unwrap();
        assert_eq!(view.network.num_network_ssids(), 1);
        let only = view.network.network_ssid(0).unwrap();
        assert_eq!(only.id.as_bytes(), b"Fronthaul@Network10");
        assert_eq!(only.ssid, ssid_text("original"));
    }
}

// --- create_data_model ---

/// Upstream `create_data_model_valid_data_model_creation` (:305).
#[test]
fn create_data_model_valid_data_model_creation() {
    let mut mesh = upstream_fixture();
    let al = MacAddr::new([0x10, 0x11, 0x12, 0x13, 0x14, 0x15]);
    let created = mesh.create_data_model(init(
        "Network1",
        al,
        ProfileType::MULTI_AP_PROFILE_1,
        NodeRole::Agent,
    ));
    assert_eq!(created, Ok(Put::Inserted));
    let node = mesh.get_data_model(&node_id("Network1", al)).unwrap();
    assert_eq!(node.device().id.net_id.as_str(), "Network1");
    assert_eq!(node.device().profile, ProfileType::MULTI_AP_PROFILE_1);
    assert_eq!(node.device().intf.mac, al);
    assert!(!node.is_colocated());
}

/// Upstream `create_data_model_loop_profile_types` (:428): profiles 0 to 3 on one node id; each
/// create replaces the node, as `hash_map_put` does.
#[test]
fn create_data_model_loop_profile_types() {
    let mut mesh = upstream_fixture();
    let al = MacAddr::new([0x50, 0x51, 0x52, 0x53, 0x54, 0x55]);
    for profile in 0u8..=3 {
        let put = mesh
            .create_data_model(init(
                "Network4",
                al,
                ProfileType::from(profile),
                NodeRole::Agent,
            ))
            .unwrap();
        assert_eq!(
            put,
            if profile == 0 {
                Put::Inserted
            } else {
                Put::Replaced
            }
        );
        let node = mesh.get_data_model(&node_id("Network4", al)).unwrap();
        assert_eq!(node.device().id.net_id.as_str(), "Network4");
        assert_eq!(node.device().profile, ProfileType::from(profile));
        assert_eq!(node.device().intf.mac, al);
        assert!(!node.is_colocated());
    }
}

/// Upstream `create_data_model_default_unsuccess_assoc_policy` (:462): the default
/// `unsuccess_assoc` policy reports unassociated stations at rate 30.
#[test]
fn create_data_model_default_unsuccess_assoc_policy() {
    let mesh = upstream_fixture();
    let node = mesh.get_data_model(&node_id("Network1", MAC1)).unwrap();
    let policy = node
        .policies()
        .find(|policy| policy.id.kind == PolicyIdType::UNSUCCESS_ASSOC)
        .unwrap();
    assert!(policy.report_unassoc_sta);
    assert_eq!(policy.max_reporting_rate, 30);
}

// --- debug_probe, delete_data_model, constructor and destructor ---

/// Upstream `delete_data_model_existent` (:530), a smoke test: the node is gone afterwards and
/// handed back.
#[test]
fn delete_data_model_existent() {
    let mut mesh = upstream_fixture();
    let al = MacAddr::new([0x12, 0x23, 0x33, 0x44, 0x55, 0x66]);
    mesh.create_data_model(init(
        "Network1",
        al,
        ProfileType::MULTI_AP_PROFILE_1,
        NodeRole::Controller,
    ))
    .unwrap();
    let removed = mesh.delete_data_model(&node_id("Network1", al)).unwrap();
    assert!(removed.is_controller());
    assert!(mesh.get_data_model(&node_id("Network1", al)).is_none());
}

/// Upstream `delete_data_model_non_existent` (:572), which expects a throw (upstream
/// dereferences the null result, `dm_easy_mesh_list.cpp:1649-1652`): the port returns `None` and
/// the model is unchanged.
#[test]
fn delete_data_model_non_existent() {
    let mut mesh = upstream_fixture();
    let before = mesh.num_nodes();
    let al = MacAddr::new([0x00, 0x1A, 0x2B, 0x3C, 0x4D, 0x5E]);
    assert!(
        mesh.delete_data_model(&node_id("non_existent", al))
            .is_none()
    );
    assert_eq!(mesh.num_nodes(), before);
}

/// Upstream `delete_data_model_empty_netid` (:669), a smoke test with the empty network id: it
/// is valid input that finds no node.
#[test]
fn delete_data_model_empty_netid() {
    let mut mesh = upstream_fixture();
    let before = mesh.num_nodes();
    let al = MacAddr::new([0x40, 0x41, 0x42, 0x43, 0x44, 0x45]);
    assert!(mesh.delete_data_model(&node_id("", al)).is_none());
    assert_eq!(mesh.num_nodes(), before);
}

/// Upstream `dm_easy_mesh_list_t_defaultConstructor_singleInstance` (:702): a new mesh is empty.
#[test]
fn dm_easy_mesh_list_t_default_constructor_single_instance() {
    let mesh = MeshModel::new(None);
    assert_eq!(mesh.num_nodes(), 0);
    assert_eq!(mesh.num_networks(), 0);
    assert!(mesh.first_dm().is_none());
}

/// Upstream `dm_easy_mesh_list_t_valid_destruction` (:4901): a mesh with nodes drops cleanly and
/// a new one is independent of it.
#[test]
fn dm_easy_mesh_list_t_valid_destruction() {
    let mesh = upstream_fixture();
    assert_eq!(mesh.num_nodes(), 6);
    drop(mesh);
    assert_eq!(MeshModel::new(None).num_nodes(), 0);
}

/// Upstream `init_valid_non_null_mgr` (:2893), a smoke test of `init`: the controller MAC
/// given to the new mesh is the one `put_network` reads, and the mesh starts empty.
#[test]
fn init_valid_non_null_mgr() {
    let mesh = MeshModel::new(Some(MAC1));
    assert_eq!(mesh.controller_al_mac(), Some(MAC1));
    assert_eq!(mesh.num_nodes(), 0);
}

// --- get_bss, get_data_model, get_device ---

/// Upstream `get_bss_key_not_exists` (:728).
#[test]
fn get_bss_key_not_exists() {
    let mesh = upstream_fixture();
    let key = bss_key(
        MacAddr::new([0x88; 6]),
        MacAddr::new([0x77; 6]),
        HaulType::FRONTHAUL,
    );
    assert!(
        mesh.get_bss(&node_id("Network1", MacAddr::new([0x99; 6])), &key)
            .is_none()
    );
}

/// Upstream `get_bss_empty_key` (:788): a key with the empty network id finds nothing.
#[test]
fn get_bss_empty_key() {
    let mesh = upstream_fixture();
    let key = bss_key(
        MacAddr::new([0x88; 6]),
        MacAddr::new([0x77; 6]),
        HaulType::FRONTHAUL,
    );
    assert!(
        mesh.get_bss(
            &node_id("", MacAddr::new([0x89, 0x99, 0x99, 0x99, 0x99, 0x99])),
            &key
        )
        .is_none()
    );
}

/// Upstream `get_data_model_ExistentNetworkID` (:823).
#[test]
fn get_data_model_existent_network_id() {
    let mesh = upstream_fixture();
    let node = mesh.get_data_model(&node_id("Network2", MAC3)).unwrap();
    assert_eq!(node.device().id.net_id.as_str(), "Network2");
    assert_eq!(node.device().intf.mac, MAC3);
}

/// Upstream `get_data_model_NonExistentNetworkID` (:875).
#[test]
fn get_data_model_non_existent_network_id() {
    let mesh = upstream_fixture();
    let al = MacAddr::new([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
    assert!(mesh.get_data_model(&node_id("nonexistent", al)).is_none());
}

/// Upstream `get_device_valid_existing_device` (:970): the device of (Network1, `mac2`); the key
/// carries a media (`0x0100`), which the lookup ignores.
#[test]
fn get_device_valid_existing_device() {
    let mesh = upstream_fixture();
    let device = mesh.get_device(&node_id("Network1", MAC2)).unwrap();
    assert_eq!(device.id.net_id.as_str(), "Network1");
    assert_eq!(device.intf.mac, MAC2);
    assert_eq!(device.profile, ProfileType::MULTI_AP_PROFILE_2);
}

/// Upstream `get_device_valid_non_existing_device` (:1003).
#[test]
fn get_device_valid_non_existing_device() {
    let mesh = upstream_fixture();
    let al = MacAddr::new([0x00, 0x22, 0x33, 0x44, 0x55, 0x66]);
    assert!(mesh.get_device(&node_id("net2", al)).is_none());
}

/// Upstream `get_device_empty_key` (:1062): the empty network id finds no device.
#[test]
fn get_device_empty_key() {
    let mesh = upstream_fixture();
    assert!(mesh.get_device(&node_id("", MacAddr::ZERO)).is_none());
}

/// Upstream `get_device_improper_formatted_key` (:1091): a malformed key string cannot be built
/// from a [`NodeId`]; the nearest case is a node id that names nothing.
#[test]
fn get_device_improper_formatted_key() {
    let mesh = upstream_fixture();
    assert!(
        mesh.get_device(&node_id("invalidkeyformat", MacAddr::ZERO))
            .is_none()
    );
}

/// Upstream `get_device_extra_whitespace` (:1123): the whitespace is part of a typed network id
/// and matches exactly, so a padded id finds nothing.
#[test]
fn get_device_extra_whitespace() {
    let mesh = upstream_fixture();
    assert!(mesh.get_device(&node_id(" net1", MAC1)).is_none());
    assert!(mesh.get_device(&node_id(" Network1 ", MAC1)).is_none());
}

// --- get_first_* ---

/// Upstream `get_first_bss_returns_NULL_when_empty` (:1155).
#[test]
fn get_first_bss_returns_null_when_empty() {
    assert!(upstream_fixture().first_bss().is_none());
}

/// Upstream `get_first_device_returns_valid_pointer` (:1191): upstream's first device in hash
/// order is on Network2 with profile 3; the port's first node in key order is the controller
/// node `put_network` created at the zero MAC of Network1 (profile 3).
/// intentional-difference:c3-key-order
#[test]
fn get_first_device_returns_valid_pointer() {
    let mesh = upstream_fixture();
    let first = mesh.first_device().unwrap();
    assert_eq!(first.at, node_id("Network1", MacAddr::ZERO));
    assert_eq!(first.item.id.net_id.as_str(), "Network1");
    assert_eq!(first.item.profile, ProfileType::MULTI_AP_PROFILE_3);
}

/// Upstream `get_first_dm_valid` (:1226): the first node has 8 policies; which node it is, is
/// the key order's. intentional-difference:c3-key-order
#[test]
fn get_first_dm_valid() {
    let mesh = upstream_fixture();
    let first = mesh.first_dm().unwrap();
    assert_eq!(first.at, node_id("Network1", MacAddr::ZERO));
    assert_eq!(first.item.device().id.net_id.as_str(), "Network1");
    assert_eq!(first.item.device().profile, ProfileType::MULTI_AP_PROFILE_3);
    assert_eq!(first.item.num_policies(), 8);
}

/// Upstream `get_first_network_returns_NULL_when_no_networks_present` (:1256). Upstream's
/// fixture leaves the node networks with an empty id, so the first network is never found; the
/// port's networks carry their ids (plan C7), so the empty mesh is the case with no network.
/// intentional-difference:c7-network-lifecycle
#[test]
fn get_first_network_returns_null_when_no_networks_present() {
    assert!(MeshModel::new(None).first_network().is_none());
    assert_eq!(
        upstream_fixture().first_network().unwrap().net(),
        net("Network1")
    );
}

/// Upstream `get_first_network_ssid_returns_null` (:1285).
#[test]
fn get_first_network_ssid_returns_null() {
    assert!(upstream_fixture().first_network_ssid().is_none());
}

/// Upstream `get_first_op_class_valid` (:1315): the first op class is a preset (83, 128 or 135).
#[test]
fn get_first_op_class_valid() {
    let mesh = upstream_fixture();
    let first = mesh.first_op_class().unwrap();
    assert!([83, 128, 135].contains(&first.item.op_class));
}

/// Upstream `get_first_policy_empty_policy_list` (:1350): the first policy is a default one of
/// the first node. Upstream asserts the literal `OneWifiMesh` id and `ap_metrics_rep` (hash
/// order); the port seeds the node's network id (plan A5) and iterates in key order, whose first
/// type is `steering_local`. intentional-difference:a5-policy-net-id
#[test]
fn get_first_policy_empty_policy_list() {
    let mesh = upstream_fixture();
    let first = mesh.first_policy().unwrap();
    assert_eq!(first.item.id.net_id.as_str(), "Network1");
    assert_eq!(first.item.id.kind, PolicyIdType::STEERING_LOCAL);
}

/// Upstream `get_first_pre_set_op_class_by_type_NoPreConfiguredOperationClassPresent` (:1383).
#[test]
fn get_first_pre_set_op_class_by_type_no_pre_configured_operation_class_present() {
    assert!(
        upstream_fixture()
            .first_pre_set_op_class_by_type(OpClassType(0))
            .is_none()
    );
}

/// Upstream `get_first_radio_returns_valid_pointer_when_multiple_radios_exist` (:1411): the
/// fixture has no radio, so there is none.
#[test]
fn get_first_radio_returns_valid_pointer_when_multiple_radios_exist() {
    assert!(upstream_fixture().first_radio().is_none());
}

/// Upstream `get_first_radio_returns_NULL_when_net_id_does_not_match` (:1439).
#[test]
fn get_first_radio_returns_null_when_net_id_does_not_match() {
    assert!(
        upstream_fixture()
            .first_radio_of(&node_id("Network4", MAC1))
            .is_none()
    );
}

/// Upstream `get_first_radio_handles_null_mac` (:1468): the zero MAC names a node (the
/// controller node of the fixture's `put_network`) with no radio.
#[test]
fn get_first_radio_handles_null_mac() {
    assert!(
        upstream_fixture()
            .first_radio_of(&node_id("Network1", MacAddr::ZERO))
            .is_none()
    );
}

/// Upstream `get_first_radio_empty_netid_returns_NULL` (:1533): the empty network id finds no
/// node.
#[test]
fn get_first_radio_empty_netid_returns_null() {
    assert!(
        upstream_fixture()
            .first_radio_of(&node_id("", MAC2))
            .is_none()
    );
}

/// Upstream `get_first_scan_result_ReturnNullWhenNoScanResultsExist` (:1566).
#[test]
fn get_first_scan_result_return_null_when_no_scan_results_exist() {
    assert!(upstream_fixture().first_scan_result().is_none());
}

/// Upstream `get_first_sta_empty_network_list` (:1594).
#[test]
fn get_first_sta_empty_network_list() {
    assert!(upstream_fixture().first_sta().is_none());
}

// --- get_network, get_network_ssid ---

/// Upstream `get_network_nonExistentKey` (:1622).
#[test]
fn get_network_non_existent_key() {
    assert!(
        upstream_fixture()
            .get_network(&net("nonExistentKey"))
            .is_none()
    );
}

/// Upstream `get_network_emptyKey` (:1650) asserts a network is found for the empty key, because
/// the prefix match of `strncmp(…, strlen(key) = 0)` matches any network
/// (`dm_easy_mesh_list.cpp:114`, research 7.6). The port matches exactly: the empty id names no
/// stored network. intentional-difference:c4-exact-keys
#[test]
fn get_network_empty_key() {
    let mesh = upstream_fixture();
    assert!(mesh.get_network(&NetId::default()).is_none());
    assert!(mesh.get_network(&net("Network1")).is_some());
}

/// Upstream `get_network_ssid_Negative_nonexistent_key` (:1707).
#[test]
fn get_network_ssid_negative_nonexistent_key() {
    assert!(
        upstream_fixture()
            .get_network_ssid(&ssid_key("nonexistent_key"))
            .is_none()
    );
}

/// Upstream `get_network_ssid_Negative_empty_string_key` (:1765): the empty id finds nothing.
#[test]
fn get_network_ssid_negative_empty_string_key() {
    let mut mesh = upstream_fixture();
    mesh.put_network_ssid(
        &net("Network1"),
        fixtures::network_ssid("Fronthaul@Network1", "s", &[]),
    )
    .unwrap();
    assert!(mesh.get_network_ssid(&ssid_key("")).is_none());
}

/// Upstream `get_network_ssid_Positive_special_character_key_notexists` (:1799).
#[test]
fn get_network_ssid_positive_special_character_key_notexists() {
    assert!(
        upstream_fixture()
            .get_network_ssid(&ssid_key("n@work#1"))
            .is_none()
    );
}

// --- get_next_* ---

/// Upstream `get_next_bss_valid` (:1831): two BSSs are put, then the first and the next are read.
/// Upstream's keys carry the ids and its records are zeroed; the port derives the key from the
/// record, so the records carry the ids. intentional-difference:c4-keys-from-records
#[test]
fn get_next_bss_valid() {
    let mut mesh = upstream_fixture();
    mesh.put_bss(bss_in("Network1", MAC1, MAC2, MAC3, HaulType::FRONTHAUL))
        .unwrap();
    mesh.put_bss(bss_in("Network1", MAC1, MAC2, MAC4, HaulType::FRONTHAUL))
        .unwrap();
    let first = mesh.first_bss().unwrap();
    assert_eq!(first.at, node_id("Network1", MAC1));
    let next = mesh.next_bss(&first.at, first.index).unwrap();
    assert_ne!(next.item.bssid.mac, first.item.bssid.mac);
}

/// Upstream `GetNextDevice_Positive` (:1902).
#[test]
fn get_next_device_positive() {
    let mesh = upstream_fixture();
    let first = mesh.first_device().unwrap();
    let next = mesh.next_device(&first.at).unwrap();
    assert_ne!(next.at, first.at);
    assert!(mesh.next_device(&node_id("Network2", MAC3)).is_none());
}

/// Upstream `get_next_dm_Positive` (:1964).
#[test]
fn get_next_dm_positive() {
    let mesh = upstream_fixture();
    let first = mesh.first_dm().unwrap();
    let next = mesh.next_dm(&first.at).unwrap();
    assert!(first.at < next.at);
}

/// Upstream `get_next_op_class_valid` (:2078).
#[test]
fn get_next_op_class_valid() {
    let mesh = upstream_fixture();
    let first = mesh.first_op_class().unwrap();
    let next = mesh.next_op_class(&first.at, first.index).unwrap();
    assert_ne!(OpClassKey::of(next.item), OpClassKey::of(first.item));
}

/// Upstream `get_next_policy_positive` (:2135).
#[test]
fn get_next_policy_positive() {
    let mesh = upstream_fixture();
    let first = mesh.first_policy().unwrap();
    let next = mesh
        .next_policy(&first.at, &crate::PolicyKey::of(first.item))
        .unwrap();
    assert!(crate::PolicyKey::of(first.item) < crate::PolicyKey::of(next.item));
}

/// Upstream `get_next_pre_set_op_class_by_type_invalid_opclasstype` (:2225): a type above
/// `scan_param`, with an op class that is not stored.
#[test]
fn get_next_pre_set_op_class_by_type_invalid_opclasstype() {
    let mesh = upstream_fixture();
    let kind = OpClassType(OpClassType::SCAN_PARAM.0 + 2);
    assert!(mesh.next_pre_set_op_class_by_type(kind, 999).is_none());
}

/// Upstream `get_next_radio_valid_next_radio_available` (:2258). Upstream puts zeroed radios,
/// which no node owns, so its assertions never run; the port puts two real radios in `dm1` and
/// reads the second after the first. intentional-difference:vacuous-upstream-test
#[test]
fn get_next_radio_valid_next_radio_available() {
    let mut mesh = upstream_fixture();
    mesh.put_radio(radio_in("Network1", MAC1, mac(1))).unwrap();
    mesh.put_radio(radio_in("Network1", MAC1, mac(2))).unwrap();
    let id = node_id("Network1", MAC1);
    let first = mesh.first_radio_of(&id).unwrap();
    assert_eq!(first.item.intf.mac, mac(1));
    let next = mesh.next_radio_of(&id, first.index).unwrap();
    assert_eq!(next.item.intf.mac, mac(2));
}

/// Upstream `get_next_radio_no_next_radio_available` (:2294): a node that does not exist.
#[test]
fn get_next_radio_no_next_radio_available() {
    let mesh = upstream_fixture();
    let al = MacAddr::new([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
    assert!(mesh.next_radio_of(&node_id("mesh_net_1", al), 0).is_none());
}

/// Upstream `get_next_radio_null_net_id_provided` (:2338): after the last radio there is no
/// next one (upstream's assertion is inside an `if` that its zeroed radios never reach).
/// intentional-difference:vacuous-upstream-test
#[test]
fn get_next_radio_null_net_id_provided() {
    let mut mesh = upstream_fixture();
    mesh.put_radio(radio_in("Network1", MAC1, mac(1))).unwrap();
    let id = node_id("Network1", MAC1);
    assert!(mesh.next_radio_of(&id, 0).is_none());
}

/// Upstream `get_next_scan_result_positive` (:2438): two scan results, the next after the
/// first, in key order.
#[test]
fn get_next_scan_result_positive() {
    let mut mesh = upstream_fixture();
    let first_key = scan_key_in("Network1", MAC1, MAC2, 0x81, 6, ScannerType::RADIO);
    let second_key = scan_key_in("Network1", MAC1, MAC2, 0x81, 11, ScannerType::RADIO);
    mesh.put_scan_result(MacAddr::BROADCAST, scan_in(&first_key), 0)
        .unwrap();
    mesh.put_scan_result(MacAddr::BROADCAST, scan_in(&second_key), 1)
        .unwrap();
    let first = mesh.first_scan_result().unwrap();
    assert_eq!(crate::ScanKey::of(&first.item.id), first_key);
    let next = mesh
        .next_scan_result(&first.at, &crate::ScanKey::of(&first.item.id))
        .unwrap();
    assert_eq!(crate::ScanKey::of(&next.item.id), second_key);
    mesh.remove_scan_result(&first_key, MacAddr::BROADCAST);
    mesh.remove_scan_result(&second_key, MacAddr::BROADCAST);
    assert!(mesh.first_scan_result().is_none());
}

// --- get_op_class, get_policy, get_radio, get_scan_result, get_sta ---

/// Upstream `get_op_class_non_existent_key` (:2546).
#[test]
fn get_op_class_non_existent_key() {
    let key = OpClassKey {
        ruid: MAC1,
        kind: OpClassType::CURRENT,
        op_class: 0x81,
    };
    assert!(upstream_fixture().get_op_class(&key).is_none());
}

/// Upstream `get_policy_non_existent_key` (:2606).
#[test]
fn get_policy_non_existent_key() {
    let key = policy_key(
        "non_existing_key",
        MAC1,
        MAC2,
        PolicyIdType::DEFAULT_8021Q_SETTINGS,
    );
    assert!(upstream_fixture().get_policy(&key).is_none());
}

/// Upstream `get_radio_non_existent_radio_key_returns_null` (:2667).
#[test]
fn get_radio_non_existent_radio_key_returns_null() {
    assert!(
        upstream_fixture()
            .get_radio(MacAddr::new([0xEE; 6]))
            .is_none()
    );
}

/// Upstream `get_scan_result_validKey_returnsCorrespondingScanResult` (:2730).
#[test]
fn get_scan_result_valid_key_returns_corresponding_scan_result() {
    let mut mesh = upstream_fixture();
    let key = scan_key_in("Network1", MAC1, MAC2, 0x81, 6, ScannerType::RADIO);
    mesh.put_scan_result(MacAddr::BROADCAST, scan_in(&key), 0)
        .unwrap();
    let found = mesh.get_scan_result(&key).unwrap();
    assert_eq!(crate::ScanKey::of(&found.id), key);
    mesh.remove_scan_result(&key, MacAddr::BROADCAST);
    assert!(mesh.get_scan_result(&key).is_none());
}

/// Upstream `get_scan_result_nonexistentKey_returnsNull` (:2773).
#[test]
fn get_scan_result_nonexistent_key_returns_null() {
    let key = scan_key_in("nonexistent_key", MAC3, MAC4, 0x81, 6, ScannerType::RADIO);
    assert!(upstream_fixture().get_scan_result(&key).is_none());
}

/// Upstream `get_sta_valid_non_existing_sta` (:2830).
#[test]
fn get_sta_valid_non_existing_sta() {
    let key = StaKey {
        sta: MAC4,
        bssid: MAC2,
        radio: MAC3,
    };
    assert!(upstream_fixture().get_sta(&key).is_none());
}

// --- put_* ---

/// Upstream `put_bss_valid_insertion` (:2952), a smoke test: the BSS is stored in the node of
/// its id and can be read back.
#[test]
fn put_bss_valid_insertion() {
    let mut mesh = upstream_fixture();
    let bss = bss_in("Network1", MAC1, MAC2, MAC3, HaulType::FRONTHAUL);
    assert_eq!(mesh.put_bss(bss), Ok(Put::Inserted));
    let key = bss_key(MAC2, MAC3, HaulType::FRONTHAUL);
    assert!(mesh.get_bss(&node_id("Network1", MAC1), &key).is_some());
    assert_eq!(
        mesh.get_data_model(&node_id("Network1", MAC1))
            .unwrap()
            .bss_list()
            .len(),
        1
    );
}

/// Upstream `put_bss_empty_key` (:3012), which expects a throw: a BSS of the empty network id
/// has no node; the port returns `NotFound` and changes nothing.
#[test]
fn put_bss_empty_key() {
    let mut mesh = upstream_fixture();
    let bss = bss_in("", MAC1, MAC2, MAC3, HaulType::FRONTHAUL);
    assert_eq!(
        mesh.put_bss(bss),
        Err(ModelError::NotFound { table: Table::Node })
    );
    assert!(mesh.first_bss().is_none());
}

/// Upstream `put_bss_special_char_key` (:3071), which expects a throw because the key
/// `mesh@bss#004$%…` breaks upstream's string parsing. A typed key is not parsed: a network id
/// with `@`, `#` and `$` is ordinary text, so the BSS lands in a node of that network, and a BSS
/// of a network without a node is `NotFound`. intentional-difference:c4-typed-keys
#[test]
fn put_bss_special_char_key() {
    let mut mesh = upstream_fixture();
    let special = "mesh@bss#004$%";
    let bss = || bss_in(special, MAC1, MAC2, MAC3, HaulType::FRONTHAUL);
    assert_eq!(
        mesh.put_bss(bss()),
        Err(ModelError::NotFound { table: Table::Node })
    );
    mesh.create_data_model(init(
        special,
        MAC1,
        ProfileType::MULTI_AP_PROFILE_3,
        NodeRole::Agent,
    ))
    .unwrap();
    assert_eq!(mesh.put_bss(bss()), Ok(Put::Inserted));
    let key = bss_key(MAC2, MAC3, HaulType::FRONTHAUL);
    assert!(mesh.get_bss(&node_id(special, MAC1), &key).is_some());
}

/// Upstream `put_device_valid_insertion` (:3102), a smoke test: the device replaces that of the
/// node, takes the key as its id (with the key's media), and the network counts its devices.
#[test]
fn put_device_valid_insertion() {
    let mut mesh = upstream_fixture();
    let key = device_id("Network1", MAC1, MediaType::IEEE_802_11B_2_4_GHZ);
    let put = mesh.put_device(key, device_in("Network1", MAC1), false);
    assert_eq!(put, Ok(Put::Replaced));
    let stored = mesh.get_device(&node_id("Network1", MAC1)).unwrap();
    assert_eq!(stored.id, key);
    assert_eq!(
        mesh.get_network(&net("Network1"))
            .unwrap()
            .info()
            .num_of_devices,
        3
    );
}

/// Upstream `put_network_valid_network_insertion` (:3192), a smoke test: the fixture's
/// `put_network` already created the controller node at the zero MAC, so this put replaces the
/// record and creates no node.
#[test]
fn put_network_valid_network_insertion() {
    let mut mesh = upstream_fixture();
    let nodes = mesh.num_nodes();
    let mut record = zeroed_network();
    record.timestamp = crate::CStrBytes::new(b"stamp").unwrap();
    assert_eq!(mesh.put_network(net("Network1"), record), Ok(Put::Replaced));
    assert_eq!(mesh.num_nodes(), nodes);
    let stored = mesh.get_network(&net("Network1")).unwrap();
    assert_eq!(stored.info().timestamp.as_bytes(), b"stamp");
}

/// Upstream `put_network_ssid_valid_new_ssid_insertion` (:3281), a smoke test with the key
/// `SSID1`: the SSID is stored in the network and found by its id.
#[test]
fn put_network_ssid_valid_new_ssid_insertion() {
    let mut mesh = upstream_fixture();
    let ssid = fixtures::network_ssid("SSID1", "ssid-one", &[]);
    assert_eq!(
        mesh.put_network_ssid(&net("Network1"), ssid),
        Ok(Put::Inserted)
    );
    let found = mesh.get_network_ssid(&ssid_key("SSID1")).unwrap();
    assert_eq!(found.ssid, ssid_text("ssid-one"));
}

/// Upstream `put_network_ssid_negative_empty_key` (:3369), which expects a throw: a network id
/// that does not exist is `NotFound` and nothing is stored.
#[test]
fn put_network_ssid_negative_empty_key() {
    let mut mesh = upstream_fixture();
    let ssid = fixtures::network_ssid("", "ssid", &[]);
    assert_eq!(
        mesh.put_network_ssid(&NetId::default(), ssid),
        Err(ModelError::NotFound {
            table: Table::Network
        })
    );
    assert!(mesh.first_network_ssid().is_none());
}

/// A radio of `dm1` whose MAC is `mac1`, so that `mac1` is also an op class `ruid` or a station
/// radio of the fixture. Upstream's tests reach this by a key that names `mac1`; its owner
/// lookup finds no node there, so those tests store nothing.
fn fixture_with_radio_mac1() -> MeshModel {
    let mut mesh = upstream_fixture();
    mesh.put_radio(radio_in("Network1", MAC1, MAC1)).unwrap();
    mesh
}

fn op_class_key(ruid: MacAddr, kind: OpClassType, op_class: u32) -> OpClassKey {
    OpClassKey {
        ruid,
        kind,
        op_class,
    }
}

/// Upstream `put_op_class_valid_input` (:3398), a smoke test. Upstream's lookup of the node that
/// owns `mac1` finds none and returns 0 without storing (`dm_easy_mesh_list.cpp:1224-1226`); here
/// `dm1` owns the radio `mac1`, and the op class is stored and read back.
/// intentional-difference:c4-global-finds
#[test]
fn put_op_class_valid_input() {
    let mut mesh = fixture_with_radio_mac1();
    let mut op = fixtures::op_class(MAC1, OpClassType::CURRENT, 100);
    op.channel = 5;
    op.tx_power = 15;
    assert_eq!(mesh.put_op_class(op), Ok(Put::Inserted));
    let key = op_class_key(MAC1, OpClassType::CURRENT, 100);
    let found = mesh.get_op_class(&key).unwrap();
    assert_eq!(found.at, node_id("Network1", MAC1));
    assert_eq!(found.item.channel, 5);
}

/// Upstream `put_op_class_empty_key` (:3509), which expects a throw: the empty `ruid` (zero MAC)
/// is valid input. For a `cac_available` op class the owner is the first node whose AL MAC is
/// that MAC, which is the fixture's controller node at the zero MAC.
#[test]
fn put_op_class_empty_key() {
    let mut mesh = upstream_fixture();
    let op = fixtures::op_class(MacAddr::ZERO, OpClassType::CAC_AVAILABLE, 300);
    assert_eq!(mesh.put_op_class(op), Ok(Put::Inserted));
    let key = op_class_key(MacAddr::ZERO, OpClassType::CAC_AVAILABLE, 300);
    assert_eq!(
        mesh.get_op_class(&key).unwrap().at,
        node_id("Network1", MacAddr::ZERO)
    );
}

/// Upstream `put_policy_valid_policy_insertion` (:3550), a smoke test: a policy for a new radio
/// is stored in the node of the key and takes the node's AL MAC.
#[test]
fn put_policy_valid_policy_insertion() {
    let mut mesh = upstream_fixture();
    let key = policy_key("Network2", MAC3, MAC4, PolicyIdType::DEFAULT_8021Q_SETTINGS);
    assert_eq!(mesh.put_policy(&key, policy_in(&key)), Ok(Put::Inserted));
    let stored = mesh.get_policy(&key).unwrap();
    assert_eq!(stored.id.dev_mac, MAC3);
    assert_eq!(
        mesh.get_data_model(&node_id("Network2", MAC3))
            .unwrap()
            .num_policies(),
        9
    );
}

/// Upstream `put_policy_empty_key` (:3644), which expects a throw: the empty network id has no
/// node.
#[test]
fn put_policy_empty_key() {
    let mut mesh = upstream_fixture();
    let key = policy_key("", MAC1, MAC2, PolicyIdType::DEFAULT_8021Q_SETTINGS);
    assert_eq!(
        mesh.put_policy(&key, policy_in(&key)),
        Err(ModelError::NotFound { table: Table::Node })
    );
}

/// Upstream `put_radio_valid_radio_valid_key` (:3674), a smoke test. Upstream's radio is zeroed,
/// so no node is found and nothing is stored; the port stores a radio of `dm2` and returns the
/// `create_node` arguments of `dm_easy_mesh_list.cpp:470` (plan A6).
/// intentional-difference:a6-node-needed
#[test]
fn put_radio_valid_radio_valid_key() {
    let mut mesh = upstream_fixture();
    let mut radio = radio_in("Network1", MAC2, MAC2);
    radio.media_data.band = 1;
    let outcome = mesh.put_radio(radio).unwrap();
    assert_eq!(outcome.put.put, Put::Inserted);
    assert_eq!(outcome.node_needed.node, node_id("Network1", MAC2));
    assert_eq!(outcome.node_needed.radio, MAC2);
    assert_eq!(outcome.node_needed.band, em_types::FreqBand::BAND_5_GHZ);
    assert_eq!(outcome.node_needed.profile, ProfileType::MULTI_AP_PROFILE_3);
    assert_eq!(
        outcome.node_needed.service,
        em_types::ServiceType::MULTI_AP_CONTROLLER
    );
    assert_eq!(mesh.get_radio(MAC2).unwrap().at, node_id("Network1", MAC2));
}

/// Upstream `put_radio_empty_key_valid_radio` (:3770), which expects a throw: a zeroed radio
/// (empty network id) has no node; nothing is stored and no node is needed.
#[test]
fn put_radio_empty_key_valid_radio() {
    let mut mesh = upstream_fixture();
    let radio = radio_in("", MacAddr::ZERO, MacAddr::ZERO);
    assert_eq!(
        mesh.put_radio(radio).err(),
        Some(ModelError::NotFound { table: Table::Node })
    );
    assert!(mesh.first_radio().is_none());
}

/// Upstream `put_scan_result_valid_insertion_standard` (:3801), a smoke test: a new scan result
/// is stored under the key of its id.
#[test]
fn put_scan_result_valid_insertion_standard() {
    let mut mesh = upstream_fixture();
    let key = scan_key_in("Network1", MAC1, MAC2, 0x81, 6, ScannerType::RADIO);
    assert_eq!(
        mesh.put_scan_result(MacAddr::BROADCAST, scan_in(&key), 0),
        Ok(Put::Inserted)
    );
    assert!(mesh.get_scan_result(&key).is_some());
}

/// Upstream `put_scan_result_valid_insertion_non_zero_index` (:3834), a smoke test with index 5:
/// the index is read only for an existing scan result, so a new one is stored.
#[test]
fn put_scan_result_valid_insertion_non_zero_index() {
    let mut mesh = upstream_fixture();
    let key = scan_key_in("Network1", MAC1, MAC2, 0x81, 6, ScannerType::RADIO);
    assert_eq!(
        mesh.put_scan_result(MacAddr::BROADCAST, scan_in(&key), 5),
        Ok(Put::Inserted)
    );
    assert!(mesh.get_scan_result(&key).is_some());
}

/// Upstream `put_scan_result_empty_key` (:3935), which expects a throw: the empty network id has
/// no node.
#[test]
fn put_scan_result_empty_key() {
    let mut mesh = upstream_fixture();
    let key = scan_key_in("", MAC1, MAC2, 0x81, 6, ScannerType::RADIO);
    assert_eq!(
        mesh.put_scan_result(MacAddr::BROADCAST, scan_in(&key), 1),
        Err(ModelError::NotFound { table: Table::Node })
    );
    assert!(mesh.first_scan_result().is_none());
}

/// Upstream `put_sta_validKey_added` (:3971), a smoke test. Upstream puts the radio `mac3` into
/// a local node that is not in the list, so no node owns it and nothing is stored; the port gives
/// `dm1` the radio `mac3` and stores the station there.
/// intentional-difference:vacuous-upstream-test
#[test]
fn put_sta_valid_key_added() {
    let mut mesh = upstream_fixture();
    mesh.put_radio(radio_in("Network1", MAC1, MAC3)).unwrap();
    assert_eq!(mesh.put_sta(sta_on(MAC1, MAC2, MAC3)), Ok(Put::Inserted));
    let key = StaKey {
        sta: MAC1,
        bssid: MAC2,
        radio: MAC3,
    };
    assert_eq!(mesh.get_sta(&key).unwrap().at, node_id("Network1", MAC1));
}

// --- remove_* ---

/// Upstream `remove_bss_valid_key_existing` (:4062).
#[test]
fn remove_bss_valid_key_existing() {
    let mut mesh = upstream_fixture();
    mesh.put_bss(bss_in("Network1", MAC1, MAC2, MAC3, HaulType::FRONTHAUL))
        .unwrap();
    let id = node_id("Network1", MAC1);
    let key = bss_key(MAC2, MAC3, HaulType::FRONTHAUL);
    assert!(mesh.remove_bss(&id, &key).is_some());
    assert!(mesh.get_bss(&id, &key).is_none());
}

/// Upstream `remove_bss_non_existent_key_no_change` (:4125), which expects a throw: no such BSS
/// is `None` and the node's BSSs are unchanged.
#[test]
fn remove_bss_non_existent_key_no_change() {
    let mut mesh = upstream_fixture();
    mesh.put_bss(bss_in("Network1", MAC1, MAC2, MAC3, HaulType::FRONTHAUL))
        .unwrap();
    let id = node_id("Network1", MAC1);
    assert!(
        mesh.remove_bss(&id, &bss_key(MAC4, MAC4, HaulType::BACKHAUL))
            .is_none()
    );
    assert!(
        mesh.remove_bss(
            &node_id("non_existent_key", MAC1),
            &bss_key(MAC2, MAC3, HaulType::FRONTHAUL)
        )
        .is_none()
    );
    assert_eq!(mesh.get_data_model(&id).unwrap().bss_list().len(), 1);
}

/// Upstream `remove_device_existing_valid_key` (:4158): upstream's remove uses the 3-part key
/// `net@mac@media`, which never matches the node key, so the device stays
/// (`dm_easy_mesh_list.cpp:226-233`); the port removes it. intentional-difference:u2-removes-implemented
#[test]
fn remove_device_existing_valid_key() {
    let mut mesh = upstream_fixture();
    let key = device_id("Network1", MAC1, MediaType::IEEE_802_11B_2_4_GHZ);
    mesh.put_device(key, device_in("Network1", MAC1), false)
        .unwrap();
    let id = node_id("Network1", MAC1);
    assert!(mesh.remove_device(&id).is_some());
    assert!(mesh.get_device(&id).is_none());
}

/// Upstream `remove_device_non_existent_key` (:4194): a key with the empty network id.
#[test]
fn remove_device_non_existent_key() {
    let mut mesh = upstream_fixture();
    let nodes = mesh.num_nodes();
    assert!(mesh.remove_device(&node_id("", MAC1)).is_none());
    assert_eq!(mesh.num_nodes(), nodes);
}

/// Upstream `remove_network_existing_valid_key` (:4252): upstream's body is empty. The port
/// refuses while the network has nodes and removes it after they are gone (plan U2, C7).
/// intentional-difference:u2-removes-implemented
#[test]
fn remove_network_existing_valid_key() {
    let mut mesh = upstream_fixture();
    mesh.put_network(net("Network1"), zeroed_network()).unwrap();
    assert_eq!(
        mesh.remove_network(&net("Network1")).err(),
        Some(ModelError::NetworkInUse { nodes: 3 })
    );
    assert!(mesh.get_network(&net("Network1")).is_some());
    for al in [MacAddr::ZERO, MAC1, MAC2] {
        assert!(mesh.delete_data_model(&node_id("Network1", al)).is_some());
    }
    assert!(mesh.remove_network(&net("Network1")).is_ok());
    assert!(mesh.get_network(&net("Network1")).is_none());
}

/// Upstream `remove_network_non_existent_key` (:4284), which expects a throw.
#[test]
fn remove_network_non_existent_key() {
    let mut mesh = upstream_fixture();
    assert_eq!(
        mesh.remove_network(&net("non_existing_network")).err(),
        Some(ModelError::NotFound {
            table: Table::Network
        })
    );
    assert_eq!(mesh.num_networks(), 2);
}

/// Upstream `remove_network_ssid_valid_existing_key` (:4347).
#[test]
fn remove_network_ssid_valid_existing_key() {
    let mut mesh = upstream_fixture();
    mesh.put_network_ssid(&net("Network1"), fixtures::network_ssid("SSID1", "s", &[]))
        .unwrap();
    assert!(
        mesh.remove_network_ssid(&net("Network1"), &ssid_key("SSID1"))
            .is_some()
    );
    assert!(mesh.get_network_ssid(&ssid_key("SSID1")).is_none());
}

/// Upstream `remove_network_ssid_nonexistent_key` (:4379), which expects a throw: `None`, and
/// the stored SSIDs are unchanged.
#[test]
fn remove_network_ssid_nonexistent_key() {
    let mut mesh = upstream_fixture();
    mesh.put_network_ssid(&net("Network1"), fixtures::network_ssid("SSID1", "s", &[]))
        .unwrap();
    assert!(
        mesh.remove_network_ssid(&net("Network1"), &ssid_key("nonExistentSSID"))
            .is_none()
    );
    assert_eq!(
        mesh.get_network(&net("Network1"))
            .unwrap()
            .num_network_ssids(),
        1
    );
}

/// Upstream `remove_op_class_valid_key_present` (:4439): upstream's body is empty
/// (`dm_easy_mesh_list.cpp:1172-1175`); the port removes the op class.
/// intentional-difference:u2-removes-implemented
#[test]
fn remove_op_class_valid_key_present() {
    let mut mesh = fixture_with_radio_mac1();
    mesh.put_op_class(fixtures::op_class(MAC1, OpClassType::CURRENT, 0x81))
        .unwrap();
    let key = op_class_key(MAC1, OpClassType::CURRENT, 0x81);
    assert!(mesh.remove_op_class(&key).is_some());
    assert!(mesh.get_op_class(&key).is_none());
}

/// Upstream `remove_op_class_valid_key_not_present` (:4470), which expects a throw.
#[test]
fn remove_op_class_valid_key_not_present() {
    let mut mesh = fixture_with_radio_mac1();
    let key = op_class_key(MAC1, OpClassType::CURRENT, 0x81);
    assert!(mesh.remove_op_class(&key).is_none());
}

/// Upstream `remove_policy_existing_policy_key` (:4524).
#[test]
fn remove_policy_existing_policy_key() {
    let mut mesh = upstream_fixture();
    let key = policy_key("Network1", MAC1, MAC2, PolicyIdType::DEFAULT_8021Q_SETTINGS);
    mesh.put_policy(&key, policy_in(&key)).unwrap();
    assert!(mesh.remove_policy(&key).is_some());
    assert!(mesh.get_policy(&key).is_none());
    assert_eq!(
        mesh.get_data_model(&node_id("Network1", MAC1))
            .unwrap()
            .num_policies(),
        8
    );
}

/// Upstream `remove_policy_non_existent_policy_key` (:4555), which expects a throw.
#[test]
fn remove_policy_non_existent_policy_key() {
    let mut mesh = upstream_fixture();
    let key = policy_key(
        "non_existent_policy_key",
        MAC1,
        MAC2,
        PolicyIdType::DEFAULT_8021Q_SETTINGS,
    );
    assert!(mesh.remove_policy(&key).is_none());
}

/// Upstream `remove_radio_existingRadio_validKey` (:4611): upstream's body is empty
/// (`dm_easy_mesh_list.cpp:394-397`); the port removes the radio.
/// intentional-difference:u2-removes-implemented
#[test]
fn remove_radio_existing_radio_valid_key() {
    let mut mesh = fixture_with_radio_mac1();
    assert!(mesh.remove_radio(MAC1).is_some());
    assert!(mesh.get_radio(MAC1).is_none());
}

/// Upstream `remove_radio_nonExistentKey` (:4641), which expects a throw.
#[test]
fn remove_radio_non_existent_key() {
    let mut mesh = upstream_fixture();
    assert!(mesh.remove_radio(MAC2).is_none());
}

/// Upstream `remove_scan_result_existing_valid_key` (:4698).
#[test]
fn remove_scan_result_existing_valid_key() {
    let mut mesh = upstream_fixture();
    let key = scan_key_in("Network1", MAC1, MAC2, 0x81, 6, ScannerType::RADIO);
    mesh.put_scan_result(MacAddr::BROADCAST, scan_in(&key), 0)
        .unwrap();
    assert!(mesh.remove_scan_result(&key, MacAddr::BROADCAST).is_some());
    assert!(mesh.get_scan_result(&key).is_none());
}

/// Upstream `remove_scan_result_non_existent_key` (:4735).
#[test]
fn remove_scan_result_non_existent_key() {
    let mut mesh = upstream_fixture();
    let key = scan_key_in("Network2", MAC3, MAC4, 0x81, 6, ScannerType::RADIO);
    assert!(mesh.remove_scan_result(&key, MacAddr::BROADCAST).is_none());
}

/// Upstream `remove_sta_existing_sta_valid` (:4802): upstream's body is empty
/// (`dm_easy_mesh_list.cpp:758-761`) and its station was never stored; the port stores the
/// station and removes it. intentional-difference:u2-removes-implemented
#[test]
fn remove_sta_existing_sta_valid() {
    let mut mesh = upstream_fixture();
    mesh.put_radio(radio_in("Network1", MAC1, MAC3)).unwrap();
    mesh.put_sta(sta_on(MAC1, MAC2, MAC3)).unwrap();
    let key = StaKey {
        sta: MAC1,
        bssid: MAC2,
        radio: MAC3,
    };
    assert!(mesh.remove_sta(&key).is_some());
    assert!(mesh.get_sta(&key).is_none());
}

/// Upstream `remove_sta_non_existing_sta` (:4839), which expects a throw.
#[test]
fn remove_sta_non_existing_sta() {
    let mut mesh = upstream_fixture();
    let key = StaKey {
        sta: MAC2,
        bssid: MAC3,
        radio: MAC4,
    };
    assert!(mesh.remove_sta(&key).is_none());
}

// --- delete_all_data_models, later additions ---

/// Upstream `delete_all_data_models_DeleteNonEmptyDataModels` (:4941), a smoke test: every node
/// but the controllers goes (`dm_easy_mesh_list.cpp:1622-1625`); creating the four nodes again
/// gives the fixture back.
#[test]
fn delete_all_data_models_delete_non_empty_data_models() {
    let mut mesh = upstream_fixture();
    let removed = mesh.delete_all_data_models();
    assert_eq!(removed.len(), 3);
    assert!(removed.iter().all(|node| !node.is_controller()));
    assert_eq!(mesh.num_nodes(), 3);
    assert!(mesh.get_data_model(&node_id("Network1", MAC2)).is_some());
    for (network, al, role) in [
        ("Network1", MAC1, NodeRole::Agent),
        ("Network1", MAC2, NodeRole::Controller),
        ("Network2", MAC3, NodeRole::Agent),
        ("Network2", MAC4, NodeRole::Agent),
    ] {
        mesh.create_data_model(init(network, al, ProfileType::MULTI_AP_PROFILE_3, role))
            .unwrap();
    }
    assert_eq!(mesh.num_nodes(), 6);
}

/// Upstream `get_next_pre_set_op_class_by_type_positive` (:4982): the first `scan_param` preset
/// and the one after it, in the first node.
#[test]
fn get_next_pre_set_op_class_by_type_positive() {
    let mesh = upstream_fixture();
    let first = mesh
        .first_pre_set_op_class_by_type(OpClassType::SCAN_PARAM)
        .unwrap();
    assert_eq!(first.item.op_class, 81);
    let next = mesh
        .next_pre_set_op_class_by_type(OpClassType::SCAN_PARAM, first.index)
        .unwrap();
    assert_eq!(next.item.op_class, 115);
}

/// Upstream `get_bss_returns_valid_bss` (:5016).
#[test]
fn get_bss_returns_valid_bss() {
    let mut mesh = upstream_fixture();
    let bssid = MacAddr::new([0, 0, 0, 0, 0, 1]);
    mesh.put_bss(bss_in(
        "Network1",
        MAC1,
        MacAddr::ZERO,
        bssid,
        HaulType::BACKHAUL,
    ))
    .unwrap();
    let key = bss_key(MacAddr::ZERO, bssid, HaulType::BACKHAUL);
    let found = mesh.get_bss(&node_id("Network1", MAC1), &key).unwrap();
    assert_eq!(found.bssid.mac, bssid);
}

/// Upstream `get_next_network_ssid_positive` (:5061): two SSIDs, the first and the next differ.
/// Upstream stores both rows with the id `Network1` and tells them apart only by the put key,
/// which it compares with the stored id (`dm_easy_mesh_list.cpp:966-972`); the port's identity is
/// the record's own id, so the rows carry distinct ids. intentional-difference:a4-ssid-identity
#[test]
fn get_next_network_ssid_positive() {
    let mut mesh = upstream_fixture();
    mesh.put_network_ssid(
        &net("Network1"),
        fixtures::network_ssid("Fronthaul@Network1", "TestSSID1", &[]),
    )
    .unwrap();
    mesh.put_network_ssid(
        &net("Network1"),
        fixtures::network_ssid("Backhaul@Network1", "TestSSID2", &[]),
    )
    .unwrap();
    let first = mesh.first_network_ssid().unwrap();
    let next = mesh.next_network_ssid(&first.at, first.index).unwrap();
    assert_ne!(first.item.ssid, next.item.ssid);
    assert!(mesh.next_network_ssid(&next.at, next.index).is_none());
}

/// Upstream `update_device_existing_device_updated` (:5117): `dfs_enable` is updated and read back;
/// the runtime metrics keep their values (`dm_easy_mesh_list.cpp:272-281`).
#[test]
fn update_device_existing_device_updated() {
    let mut mesh = upstream_fixture();
    let key = device_id("Network1", MAC1, MediaType::IEEE_802_11B_2_4_GHZ);
    mesh.put_device(key, device_in("Network1", MAC1), false)
        .unwrap();
    let id = node_id("Network1", MAC1);
    mesh.data_model_mut(&id).unwrap().device_mut().uptime = 42;
    let mut updated = device_in("Network1", MAC1);
    updated.dfs_enable = true;
    assert_eq!(mesh.update_device(&id, updated), Ok(()));
    let stored = mesh.get_device(&id).unwrap();
    assert!(stored.dfs_enable);
    assert_eq!(stored.uptime, 42);
}

/// Upstream `update_device_non_existing_device_should_throw` (:5170): no such node is `NotFound`
/// and nothing changes.
#[test]
fn update_device_non_existing_device_should_throw() {
    let mut mesh = upstream_fixture();
    let nodes = mesh.num_nodes();
    let result = mesh.update_device(&node_id("Network9", MAC1), device_in("Network9", MAC1));
    assert_eq!(result, Err(ModelError::NotFound { table: Table::Node }));
    assert_eq!(mesh.num_nodes(), nodes);
}
