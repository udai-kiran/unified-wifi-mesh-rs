// SPDX-License-Identifier: Apache-2.0

//! Node creation (plan A5), the device merge rules (A4), interfaces and preferences, and the
//! small per-node accessors.

use super::fixtures::{agent_node, device, interface, mac, net, node_init};
use crate::NodeCtx;
use crate::bounded::BoundedVec;
use crate::enums::HaulType;
use crate::model::error::{ModelError, Table};
use crate::model::keys::NetId;
use crate::model::network::NetworkModel;
use crate::model::node::{
    MAX_INTERFACES, MAX_PREFERENCES, MLO_DEFAULT_BAND_BITMAP, NodeInit, NodeModel, NodeRole,
};
use crate::records::{DeviceId, InterfacePreference};
use crate::text::CStrBytes;
use em_types::{Interface, InterfaceName, MediaType, ProfileType};

fn named_interface(name: &str, address: u8) -> Interface {
    Interface {
        name: Some(InterfaceName::try_from(name).unwrap()),
        mac: mac(address),
        media: MediaType(0),
    }
}

fn preference(platform: &str, media: MediaType) -> InterfacePreference {
    InterfacePreference {
        platform: CStrBytes::new(platform.as_bytes()).unwrap(),
        media,
    }
}

// --- creation (dm_easy_mesh_list.cpp:1656-1804) ---

#[test]
fn a_new_node_has_six_op_classes_eight_policies_and_nothing_else() {
    let node = agent_node();
    assert_eq!(node.op_classes().len(), 6);
    assert_eq!(node.num_policies(), 8);
    assert_eq!(node.radios().len(), 0);
    assert_eq!(node.radio_caps().len(), 0);
    assert_eq!(node.bss_list().len(), 0);
    assert_eq!(node.ap_mlds().len(), 0);
    assert_eq!(node.assoc_sta_mlds().len(), 0);
    assert_eq!(node.num_scan_results(), 0);
    assert_eq!(node.interfaces().len(), 0);
    assert_eq!(node.preferences().len(), 0);
    assert!(!node.dirty().any());
}

#[test]
fn a_new_agent_node_takes_its_identity_from_the_init() {
    let mut init = node_init(NodeRole::Agent);
    init.net = net("NetA");
    init.al = mac(7);
    init.profile = ProfileType::MULTI_AP_PROFILE_2;
    init.colocated = true;
    let node = NodeModel::new(init);
    assert!(!node.is_controller());
    assert!(node.is_colocated());
    assert_eq!(node.net(), net("NetA"));
    assert_eq!(node.agent_al_interface_mac(), mac(7));
    assert_eq!(node.device().profile, ProfileType::MULTI_AP_PROFILE_2);
    assert_eq!(node.device().id.net_id.as_str(), "NetA");
    assert!(
        node.device().id.dev_mac.is_zero(),
        "an agent's id.dev_mac stays zero until the device is put"
    );
}

#[test]
fn a_new_controller_node_also_sets_the_device_id_mac() {
    let node = NodeModel::new(node_init(NodeRole::Controller));
    assert!(node.is_controller());
    assert_eq!(node.device().id.dev_mac, mac(1));
    assert_eq!(node.device().id.media, MediaType(0));
    assert_eq!(node.agent_al_interface_mac(), mac(1));
}

#[test]
fn the_mlo_policy_defaults_are_use_tlv_for_fronthaul_only() {
    let node = agent_node();
    let fronthaul = node.mlo_policy(HaulType::FRONTHAUL).unwrap();
    let backhaul = node.mlo_policy(HaulType::BACKHAUL).unwrap();
    assert!(fronthaul.use_mld_tlv);
    assert!(!backhaul.use_mld_tlv);
    assert_eq!(fronthaul.band_bitmap, MLO_DEFAULT_BAND_BITMAP);
    assert_eq!(backhaul.band_bitmap, 0x0b, "2.4, 5 and 6 GHz");
}

/// `get_mlo_policy` answers fronthaul and backhaul only (`dm_easy_mesh.cpp:3620-3627`).
#[test]
fn get_mlo_policy_is_none_for_other_haul_types() {
    let mut node = agent_node();
    assert!(node.mlo_policy(HaulType::IOT).is_none());
    assert!(node.mlo_policy(HaulType(9)).is_none());
    assert!(node.mlo_policy_mut(HaulType::IOT).is_none());
    node.mlo_policy_mut(HaulType::BACKHAUL).unwrap().band_bitmap = 1;
    assert_eq!(node.mlo_policy(HaulType::BACKHAUL).unwrap().band_bitmap, 1);
}

/// 0007 defect 3: upstream's converting constructor copies the whole `ctrl_id` interface over
/// the start of `id` (`dm_easy_mesh.cpp:4318-4327`); the port sets each identity field
/// explicitly, including the network id. intentional-difference:defect3-ctor-identity
#[test]
fn the_converting_constructor_sets_the_identity_fields_explicitly() {
    let node = NodeModel::from_controller_identity(net("NetC"), mac(5), MediaType::IEEE_802_3AB);
    let device = node.device();
    assert_eq!(
        device.id.net_id.as_str(),
        "NetC",
        "the network id is not overwritten"
    );
    assert_eq!(device.id.dev_mac, mac(5));
    assert_eq!(device.id.media, MediaType::IEEE_802_3AB);
    assert_eq!(device.backhaul_alid.mac, mac(5));
    assert_eq!(device.backhaul_alid.media, MediaType::IEEE_802_3AB);
    assert_eq!(device.backhaul_mac.mac, mac(5));
    assert_eq!(device.backhaul_mac.media, MediaType::IEEE_802_3AB);
    assert_eq!(node.net(), net("NetC"));
    assert!(!node.is_controller());
}

#[test]
fn the_converting_constructor_leaves_the_tables_empty() {
    let node = NodeModel::from_controller_identity(net("NetC"), mac(5), MediaType(0));
    assert_eq!(node.op_classes().len(), 0);
    assert_eq!(node.num_policies(), 0);
    assert!(node.device().intf.name.is_none(), "names are the adapter's");
}

#[test]
fn the_converting_constructor_of_a_zero_identity_is_zeroed() {
    let node =
        NodeModel::from_controller_identity(NetId::default(), crate::MacAddr::ZERO, MediaType(0));
    assert!(node.device().id.dev_mac.is_zero());
    assert!(node.device().backhaul_alid.mac.is_zero());
    assert!(node.device().backhaul_mac.mac.is_zero());
}

#[test]
fn node_flags_and_timers_round_trip() {
    let mut node = agent_node();
    assert!(!node.topo_state());
    node.set_topo_state(true);
    node.set_ssid_mismatch_check_time(11);
    node.set_last_topo_query_sent_time(12);
    node.set_msg_id(13);
    node.set_colocated(true);
    node.set_controller(true);
    assert!(node.topo_state());
    assert_eq!(node.ssid_mismatch_check_time(), 11);
    assert_eq!(node.last_topo_query_sent_time(), 12);
    assert_eq!(node.msg_id(), 13);
    assert!(node.is_colocated());
    assert!(node.is_controller());
    node.set_controller(false);
    assert!(!node.is_controller());
}

// --- device (dm_easy_mesh_list.cpp:267-281, dm_easy_mesh.cpp:216-222) ---

/// The device update keeps `uptime`, `total_mem`, `free_mem`, `cached_mem`, `cpu_load` and
/// `cpu_temp`
/// (`dm_easy_mesh_list.cpp:272-281`).
#[test]
fn update_device_keeps_the_runtime_metrics() {
    let mut node = agent_node();
    {
        let stored = node.device_mut();
        stored.uptime = 1;
        stored.total_mem = 2;
        stored.free_mem = 3;
        stored.cached_mem = 4;
        stored.cpu_load = 5;
        stored.cpu_temp = 6;
    }
    let mut incoming = device(mac(1));
    incoming.uptime = 100;
    incoming.total_mem = 200;
    incoming.free_mem = 300;
    incoming.cached_mem = 400;
    incoming.cpu_load = 50;
    incoming.cpu_temp = 60;
    incoming.profile = ProfileType::MULTI_AP_PROFILE_1;
    node.update_device(incoming);
    let stored = node.device();
    assert_eq!(
        (
            stored.uptime,
            stored.total_mem,
            stored.free_mem,
            stored.cached_mem,
            stored.cpu_load,
            stored.cpu_temp
        ),
        (1, 2, 3, 4, 5, 6)
    );
    assert_eq!(
        stored.profile,
        ProfileType::MULTI_AP_PROFILE_1,
        "the rest is replaced"
    );
}

#[test]
fn put_device_replaces_the_record_and_takes_the_given_identity() {
    let mut node = agent_node();
    node.device_mut().uptime = 9;
    let id = DeviceId {
        net_id: net("NetZ").as_text(),
        dev_mac: mac(1),
        media: MediaType::IEEE_802_3AB,
    };
    node.put_device(device(mac(1)), id);
    assert_eq!(
        node.device().uptime,
        0,
        "replaced whole, unlike update_device"
    );
    assert_eq!(node.device().id, id);
}

/// `commit_config` keeps `is_emplus_agent` and `extended_info` (`dm_easy_mesh.cpp:218-222`).
#[test]
fn commit_device_keeps_is_emplus_agent_and_extended_info() {
    let mut node = agent_node();
    node.device_mut().is_emplus_agent = 1;
    node.device_mut().extended_info.boot_id = 77;
    let mut incoming = device(mac(1));
    incoming.is_emplus_agent = 0;
    incoming.extended_info.boot_id = 5;
    incoming.uptime = 8;
    node.commit_device(incoming);
    assert_eq!(node.device().is_emplus_agent, 1);
    assert_eq!(node.device().extended_info.boot_id, 77);
    assert_eq!(node.device().uptime, 8);
}

#[test]
fn find_matching_device_compares_the_al_mac() {
    let node = agent_node();
    assert!(node.find_matching_device(&device(mac(1))).is_some());
    assert!(node.find_matching_device(&device(mac(2))).is_none());
    assert!(
        node.find_matching_device(&device(crate::MacAddr::ZERO))
            .is_none(),
        "a zero MAC is not the node's"
    );
}

#[test]
fn the_device_text_fields_have_setters_and_getters() {
    let mut node = agent_node();
    node.set_manufacturer(CStrBytes::new(b"acme").unwrap());
    node.set_manufacturer_model(CStrBytes::new(b"m1").unwrap());
    node.set_software_version(CStrBytes::new(b"1.2").unwrap());
    node.set_serial_number(CStrBytes::new(b"sn").unwrap());
    node.set_primary_device_type(CStrBytes::new(b"ap").unwrap());
    node.set_agent_al_interface_name(InterfaceName::try_from("br0").unwrap());
    assert_eq!(node.manufacturer().as_bytes(), b"acme");
    assert_eq!(node.manufacturer_model().as_bytes(), b"m1");
    assert_eq!(node.software_version().as_bytes(), b"1.2");
    assert_eq!(node.serial_number().as_bytes(), b"sn");
    assert_eq!(node.primary_device_type().as_bytes(), b"ap");
    assert_eq!(node.agent_al_interface_name().unwrap().as_str(), "br0");
}

#[test]
fn the_controller_al_writes_update_both_the_node_and_the_network() {
    let mut node = agent_node();
    let mut network = NetworkModel::new(NetId::global());
    {
        let mut ctx = NodeCtx {
            id: crate::NodeId {
                net: NetId::global(),
                al: node.agent_al_interface_mac(),
            },
            network: &mut network,
            node: &mut node,
        };
        ctx.set_ctrl_al_interface_mac(mac(3));
        ctx.set_ctrl_al_interface_name("eth1").unwrap();
        assert!(ctx.set_ctrl_al_interface_name("").is_err());
    }
    assert_eq!(node.agent_al_interface_mac(), mac(3));
    assert_eq!(network.controller_interface_mac(), mac(3));
    assert_eq!(node.agent_al_interface_name().unwrap().as_str(), "eth1");
    assert_eq!(
        network.controller_interface_name().unwrap().as_str(),
        "eth1"
    );
}

// --- interfaces and preferences (dm_easy_mesh.cpp:2177-2261) ---

#[test]
fn preference_capacity_is_5() {
    let mut node = agent_node();
    for _ in 0..MAX_PREFERENCES {
        node.push_preference(preference("p", MediaType::IEEE_802_3AB))
            .unwrap();
    }
    assert_eq!(
        node.push_preference(preference("p", MediaType::IEEE_802_3AB)),
        Err(ModelError::Full {
            table: Table::Preference,
            capacity: 5
        })
    );
    assert_eq!(node.preferences().len(), 5);
}

/// Upstream checks the interface capacity only after the first write (0007 defect 1,
/// `dm_easy_mesh.cpp:2245-2261`); the port checks first. intentional-difference:c6-capacity
#[test]
fn interface_capacity_is_8_and_checked_before_the_write() {
    let mut node = agent_node();
    for index in 0..MAX_INTERFACES {
        node.push_interface(named_interface("eth0", u8::try_from(index).unwrap()))
            .unwrap();
    }
    assert_eq!(
        node.push_interface(named_interface("eth9", 99)),
        Err(ModelError::Full {
            table: Table::Interface,
            capacity: 8
        })
    );
    assert_eq!(node.interfaces().len(), 8);
    assert_eq!(node.interface(7).unwrap().mac, mac(7));
    assert!(node.interface(8).is_none());
}

fn node_with_interfaces() -> NodeModel {
    let mut node = agent_node();
    node.push_interface(named_interface("lo", 1)).unwrap();
    node.push_interface(named_interface("wlan0", 2)).unwrap();
    node.push_interface(named_interface("eth0", 3)).unwrap();
    node.push_preference(preference("rdk", MediaType::IEEE_802_3AB))
        .unwrap();
    node
}

#[test]
fn get_prioritized_interface_picks_the_first_ethernet_name() {
    let node = node_with_interfaces();
    assert_eq!(node.prioritized_interface(b"rdk").unwrap().mac, mac(3));
}

#[test]
fn get_prioritized_interface_picks_the_first_wlan_name_for_a_wifi_preference() {
    let mut node = node_with_interfaces();
    node.push_preference(preference("wifi", MediaType::IEEE_802_11B_2_4_GHZ))
        .unwrap();
    assert_eq!(node.prioritized_interface(b"wifi").unwrap().mac, mac(2));
}

#[test]
fn get_prioritized_interface_without_a_match_is_none() {
    let mut node = node_with_interfaces();
    assert!(
        node.prioritized_interface(b"other").is_none(),
        "no such platform"
    );
    node.push_preference(preference("odd", MediaType(0x0200)))
        .unwrap();
    assert!(
        node.prioritized_interface(b"odd").is_none(),
        "other media pick nothing"
    );
    let bare = agent_node();
    assert!(bare.prioritized_interface(b"rdk").is_none());
}

/// The argument is a prefix of the stored platform (`strncmp` over `strlen(platform)`).
#[test]
fn get_prioritized_interface_takes_the_argument_as_a_prefix_of_the_platform() {
    let node = node_with_interfaces();
    assert!(node.prioritized_interface(b"rd").is_some());
    assert!(node.prioritized_interface(b"rdkx").is_none());
}

#[test]
fn interfaces_without_a_name_are_never_picked() {
    let mut node = agent_node();
    node.push_interface(interface(mac(4))).unwrap();
    node.push_preference(preference("rdk", MediaType::IEEE_802_3AB))
        .unwrap();
    assert!(node.prioritized_interface(b"rdk").is_none());
}

// --- direct-access records ---

#[test]
fn the_engine_tables_are_reachable_for_direct_writes() {
    let mut node = agent_node();
    node.cac_comp_mut().channel = 36;
    node.tid_to_link_mut().is_bsta_config = true;
    node.ieee_1905_security_mut().id = mac(1);
    node.unassoc_query_list_mut().opclass_list = BoundedVec::new();
    assert_eq!(node.cac_comp().channel, 36);
    assert!(node.tid_to_link().is_bsta_config);
    assert_eq!(node.ieee_1905_security().id, mac(1));
    assert_eq!(node.unassoc_query_list().opclass_list.len(), 0);
    assert_eq!(node.unassoc_sta_metrics().len(), 0);
    assert_eq!(node.unassoc_sta_metrics_rsp().entry.len(), 0);
    assert_eq!(node.unassoc_sta_metrics_mut().len(), 0);
}

#[test]
fn nodes_of_one_init_in_different_roles_differ_only_in_the_role_dependent_fields() {
    let agent = NodeModel::new(NodeInit {
        role: NodeRole::Agent,
        ..node_init(NodeRole::Agent)
    });
    let controller = NodeModel::new(node_init(NodeRole::Controller));
    assert_eq!(agent.op_classes().len(), controller.op_classes().len());
    assert_eq!(agent.num_policies(), controller.num_policies());
    assert_ne!(agent.device().id.dev_mac, controller.device().id.dev_mac);
}

/// Upstream's `set_num_radios`, `set_num_bss`, `set_num_op_class` and `set_num_ap_mld` write a
/// bare count, clamping (radios, `inc/dm_easy_mesh.h:1881`), refusing (op classes,
/// `dm_easy_mesh.cpp:53-60`) or storing any value (bss, `ap_mld`), so the count and the array can
/// disagree. The port has no setter: each count is the length of its collection (plan C6).
/// intentional-difference:c6-count-rule
#[test]
fn counts_are_the_collection_lengths_and_there_is_no_setter() {
    let mut node = agent_node();
    assert_eq!(node.radios().len(), 0);
    node.put_radio(super::fixtures::radio(mac(10), true))
        .unwrap();
    assert_eq!(node.radios().len(), 1);

    node.put_bss(super::fixtures::bss(
        mac(10),
        mac(20),
        HaulType::FRONTHAUL,
        crate::enums::VapMode::AP,
    ))
    .unwrap();
    assert_eq!(node.bss_list().len(), 1);
    node.remove_bss_by_index(0).unwrap();
    assert_eq!(node.bss_list().len(), 0);

    let presets = node.op_classes().len();
    node.put_op_class(super::fixtures::op_class(
        mac(10),
        crate::enums::OpClassType::CURRENT,
        115,
    ))
    .unwrap();
    assert_eq!(node.op_classes().len(), presets + 1);

    node.update_ap_mld_info(super::fixtures::ap_mld(
        HaulType::FRONTHAUL,
        mac(40),
        vec![],
    ))
    .unwrap();
    assert_eq!(node.ap_mlds().len(), 1);

    let mut network = NetworkModel::new(NetId::global());
    network
        .put_network_ssid(super::fixtures::network_ssid("a", "ssid", &[]))
        .unwrap();
    assert_eq!(network.num_network_ssids(), network.ssids().len());
    assert_eq!(network.num_network_ssids(), 1);
}
