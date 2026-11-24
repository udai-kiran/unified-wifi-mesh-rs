// SPDX-License-Identifier: Apache-2.0

//! Fixtures of the `MeshModel` tests: the four nodes of upstream's list test fixture
//! (`tests/test_l1_dm_easy_mesh_list.cpp:33-134`) and builders of records that carry their own
//! network and device ids, as the list layer reads them.

use super::fixtures::{self, net};
use crate::enums::{HaulType, PolicyIdType, ScannerType, VapMode};
use crate::model::keys::{BssKey, NodeId, PolicyKey, ScanKey};
use crate::model::node::policy::blank_policy;
use crate::model::node::{NodeInit, NodeRole};
use crate::records::{
    BssInfo, DeviceId, DeviceInfo, NetworkInfo, Policy, RadioInfo, ScanResult, StaInfo,
};
use crate::{MeshModel, NetId};
use em_types::{MacAddr, MediaType, ProfileType};

/// `mac1` .. `mac4` of the upstream fixture (`:39-42`).
pub const MAC1: MacAddr = MacAddr::new([0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
pub const MAC2: MacAddr = MacAddr::new([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
pub const MAC3: MacAddr = MacAddr::new([0x10, 0x11, 0x12, 0x13, 0x14, 0x16]);
pub const MAC4: MacAddr = MacAddr::new([0x10, 0x11, 0x12, 0x13, 0x12, 0x15]);

pub fn node_id(network: &str, al: MacAddr) -> NodeId {
    NodeId {
        net: net(network),
        al,
    }
}

pub fn init(network: &str, al: MacAddr, profile: ProfileType, role: NodeRole) -> NodeInit {
    NodeInit {
        net: net(network),
        al,
        profile,
        role,
        colocated: false,
    }
}

/// The four nodes and two `put_network` calls of upstream's `SetUp` (`:109-134`):
/// `dm1` agent (Network1, `mac1`, profile 1), `dm2` controller (Network1, `mac2`, profile 2),
/// `dm3` and `dm4` agents (Network2, `mac3`, `mac4`, profile 3). `put_network` is called with
/// the zeroed network record of `dm1` and `dm3`, whose controller MAC is zero, so it also
/// creates a controller node at the zero MAC in each network, as upstream does (`:154`).
pub fn upstream_fixture() -> MeshModel {
    let mut mesh = MeshModel::new(None);
    let nodes = [
        init(
            "Network1",
            MAC1,
            ProfileType::MULTI_AP_PROFILE_1,
            NodeRole::Agent,
        ),
        init(
            "Network1",
            MAC2,
            ProfileType::MULTI_AP_PROFILE_2,
            NodeRole::Controller,
        ),
        init(
            "Network2",
            MAC3,
            ProfileType::MULTI_AP_PROFILE_3,
            NodeRole::Agent,
        ),
        init(
            "Network2",
            MAC4,
            ProfileType::MULTI_AP_PROFILE_3,
            NodeRole::Agent,
        ),
    ];
    for node in nodes {
        mesh.create_data_model(node).unwrap();
    }
    for network in ["Network1", "Network2"] {
        mesh.put_network(net(network), zeroed_network()).unwrap();
    }
    mesh
}

pub fn zeroed_network() -> Box<NetworkInfo> {
    Box::new(NetworkInfo::zeroed(NetId::default().as_text()))
}

pub fn device_id(network: &str, al: MacAddr, media: MediaType) -> DeviceId {
    DeviceId {
        net_id: net(network).as_text(),
        dev_mac: al,
        media,
    }
}

/// A device whose record carries `network` and `al` as its id and AL interface.
pub fn device_in(network: &str, al: MacAddr) -> Box<DeviceInfo> {
    let mut device = fixtures::device(al);
    device.id = device_id(network, al, MediaType(0));
    device
}

pub fn radio_in(network: &str, dev: MacAddr, radio_mac: MacAddr) -> Box<RadioInfo> {
    let mut radio = fixtures::radio(radio_mac, true);
    radio.id.net_id = net(network).as_text();
    radio.id.dev_mac = dev;
    radio
}

pub fn bss_in(
    network: &str,
    dev: MacAddr,
    radio_mac: MacAddr,
    bssid: MacAddr,
    haul: HaulType,
) -> Box<BssInfo> {
    let mut bss = fixtures::bss(radio_mac, bssid, haul, VapMode::AP);
    bss.id.net_id = net(network).as_text();
    bss.id.dev_mac = dev;
    bss
}

pub fn bss_key(radio_mac: MacAddr, bssid: MacAddr, haul: HaulType) -> BssKey {
    BssKey {
        ruid: radio_mac,
        bssid,
        haul_type: haul,
    }
}

pub fn sta_on(station: MacAddr, bssid: MacAddr, radio_mac: MacAddr) -> Box<StaInfo> {
    fixtures::sta(station, bssid, radio_mac)
}

pub fn scan_key_in(
    network: &str,
    dev: MacAddr,
    scanner: MacAddr,
    op_class: u8,
    channel: u8,
    kind: ScannerType,
) -> ScanKey {
    ScanKey {
        net: net(network),
        dev_mac: dev,
        scanner_mac: scanner,
        op_class,
        channel,
        scanner_type: kind,
    }
}

pub fn scan_in(key: &ScanKey) -> Box<ScanResult> {
    fixtures::scan_result(key)
}

pub fn policy_key(
    network: &str,
    dev: MacAddr,
    radio_mac: MacAddr,
    kind: PolicyIdType,
) -> PolicyKey {
    PolicyKey {
        net: net(network),
        dev_mac: dev,
        radio_mac,
        kind,
    }
}

pub fn policy_in(key: &PolicyKey) -> Box<Policy> {
    let mut policy = blank_policy(key.net, key.kind);
    policy.id.dev_mac = key.dev_mac;
    policy.id.radio_mac = key.radio_mac;
    Box::new(policy)
}
