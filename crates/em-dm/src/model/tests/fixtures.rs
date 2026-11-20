// SPDX-License-Identifier: Apache-2.0

//! Builders of zeroed records for the model tests.

use crate::bits::UBits;
use crate::bounded::BoundedVec;
use crate::enums::{HaulType, OpClassType, PolicyIdType, ScannerType, VapMode};
use crate::model::keys::{NetId, ScanKey};
use crate::model::node::policy::blank_policy;
use crate::model::node::{NodeInit, NodeModel, NodeRole};
use crate::records::{
    AffiliatedApInfo, ApMldInfo, BssId, BssInfo, DeviceInfo, EhtOperationsBss, MediaSpecData,
    NetworkSsidInfo, OpClassId, OpClassInfo, Policy, RadioCapInfo, RadioId, RadioInfo, ScanResult,
    StaInfo,
};
use crate::secret::Secret;
use crate::text::CStrBytes;
use crate::zeroed_interface;
use em_types::{FreqBand, Interface, MacAddr, ProfileType};

/// A locally administered MAC with a distinguishing last byte.
pub fn mac(last: u8) -> MacAddr {
    MacAddr::new([0x02, 0, 0, 0, 0, last])
}

/// A MAC with two distinguishing bytes, for "partial match" cases.
pub fn mac2(second_to_last: u8, last: u8) -> MacAddr {
    MacAddr::new([0x02, 0, 0, 0, second_to_last, last])
}

pub fn net(id: &str) -> NetId {
    NetId::new(id).unwrap()
}

pub fn interface(address: MacAddr) -> Interface {
    Interface {
        name: None,
        mac: address,
        media: zeroed_interface().media,
    }
}

pub fn node_init(role: NodeRole) -> NodeInit {
    NodeInit {
        net: NetId::global(),
        al: mac(1),
        profile: ProfileType::MULTI_AP_PROFILE_3,
        role,
        colocated: false,
    }
}

/// An agent node with its defaults.
pub fn agent_node() -> NodeModel {
    NodeModel::new(node_init(NodeRole::Agent))
}

pub fn radio(radio_mac: MacAddr, enabled: bool) -> Box<RadioInfo> {
    Box::new(RadioInfo {
        id: RadioId {
            net_id: NetId::global().as_text(),
            dev_mac: mac(1),
            ruid: radio_mac,
        },
        intf: interface(radio_mac),
        enabled,
        band: FreqBand::BAND_5_GHZ,
        media_data: MediaSpecData {
            media_type: 0,
            media_spec_size: 0,
            network_memb: MacAddr::ZERO,
            role: 0,
            band: 0,
            center_freq_index_1: 0,
            center_freq_index_2: 0,
        },
        number_of_bss: 0,
        number_of_unassoc_sta: 0,
        noise: 0,
        transmit: 0,
        receive_self: 0,
        receive_other: 0,
        utilization: 0,
        traffic_sep_combined_fronthaul: false,
        traffic_sep_combined_backhaul: false,
        steering_policy: 0,
        channel_util_threshold: 0,
        rcpi_steering_threshold: 0,
        sta_reporting_rcpi_threshold: 0,
        sta_reporting_hysteresis_margin_override: 0,
        channel_utilization_reporting_threshold: 0,
        associated_sta_traffic_stats_inclusion_policy: false,
        associated_sta_link_mterics_inclusion_policy: false,
        unassociated_sta_link_mterics_opclass_inclusion_policy: false,
        unassociated_sta_link_mterics_nonopclass_inclusion_policy: false,
        support_rcpi_steering: false,
        chip_vendor: CStrBytes::default(),
        ap_metrics_wifi6: false,
        transmit_power_limit: 0,
        partial_bss_color: 0,
        bss_color: 0,
        hesiga_spatial_reuse_value15_allowed: false,
        srg_information_valid: false,
        non_srg_offset_valid: false,
        psr_disallowed: false,
        init_cfg_done: false,
        non_srg_obsspd_max_offset: 0,
        srg_obsspd_min_offset: 0,
        srg_obsspd_max_offset: 0,
        srg_bss_color_bitmap: [0; 8],
        srg_partial_bssid_bitmap: [0; 8],
        neigh_bss_color_in_use_bitmap: [0; 8],
        radio_temp: 0,
    })
}

pub fn bss(radio_mac: MacAddr, bssid: MacAddr, haul: HaulType, mode: VapMode) -> Box<BssInfo> {
    Box::new(BssInfo {
        vap_index: 0,
        vap_mode: mode,
        id: BssId {
            net_id: NetId::global().as_text(),
            dev_mac: mac(1),
            ruid: radio_mac,
            bssid,
            haul_type: haul,
        },
        bssid: interface(bssid),
        ruid: interface(radio_mac),
        ssid: CStrBytes::default(),
        enabled: true,
        last_change: 0,
        timestamp: CStrBytes::default(),
        numberofsta: 0,
        channel_util: 0,
        unicast_bytes_sent: 0,
        unicast_bytes_rcvd: 0,
        multicast_bytes_sent: 0,
        multicast_bytes_rcvd: 0,
        broadcast_bytes_sent: 0,
        broadcast_bytes_rcvd: 0,
        inc_esp_ac_be: false,
        inc_esp_ac_bk: false,
        inc_esp_ac_vo: false,
        inc_esp_ac_vi: false,
        est_svc_params_be: CStrBytes::default(),
        est_svc_params_bk: CStrBytes::default(),
        est_svc_params_vi: CStrBytes::default(),
        est_svc_params_vo: CStrBytes::default(),
        byte_counter_units: 0,
        fronthaul_akm: BoundedVec::new(),
        backhaul_akm: BoundedVec::new(),
        profile_1b_sta_allowed: false,
        profile_2b_sta_allowed: false,
        assoc_allowed_status: 0,
        backhaul_use: false,
        fronthaul_use: false,
        r1_disallowed: false,
        r2_disallowed: false,
        multi_bssid: false,
        transmitted_bssid: false,
        eht_ops: EhtOperationsBss {
            bssid: MacAddr::ZERO,
            reserved1: UBits::default(),
            group_addr_bu_ind_exp: UBits::default(),
            group_addr_bu_ind_limit: false,
            default_pe_duration: false,
            disabled_subchannel_valid: false,
            op_info_valid: false,
            eht_msc_nss_set: [0; 4],
            control: 0,
            ccfs0: 0,
            ccfs1: 0,
            disabled_subchannel_bitmap: [0; 2],
            reserved2: [0; 16],
        },
        mesh_sta_passphrase: Secret::default(),
        vlan_id: 0,
        mld_mac: MacAddr::ZERO,
        sta_mac: MacAddr::ZERO,
        vendor_elements: BoundedVec::new(),
        connect_status: false,
    })
}

/// A station row with only its key set.
pub fn sta(station: MacAddr, bssid: MacAddr, radio_mac: MacAddr) -> Box<StaInfo> {
    Box::new(StaInfo {
        id: station,
        bssid,
        radiomac: radio_mac,
        ..StaInfo::default()
    })
}

pub fn op_class(ruid: MacAddr, kind: OpClassType, number: u32) -> OpClassInfo {
    OpClassInfo {
        id: OpClassId {
            ruid,
            kind,
            op_class: number,
        },
        op_class: number,
        channel: 0,
        tx_power: 0,
        max_tx_power: 0,
        channels: BoundedVec::new(),
        channel_pref: [0; 64],
        pref_valid: false,
        mins_since_cac_comp: 0,
        sec_remain_non_occ_dur: 0,
        countdown_cac_comp: 0,
    }
}

pub fn scan_key(scanner: MacAddr, op_class: u8, channel: u8, kind: ScannerType) -> ScanKey {
    ScanKey {
        net: NetId::global(),
        dev_mac: mac(1),
        scanner_mac: scanner,
        op_class,
        channel,
        scanner_type: kind,
    }
}

pub fn scan_result(key: &ScanKey) -> Box<ScanResult> {
    Box::new(ScanResult {
        id: key.to_id(),
        scan_status: 0,
        timestamp: CStrBytes::default(),
        util: 0,
        noise: 0,
        neighbor: BoundedVec::new(),
        aggr_scan_duration: 0,
        scan_type: 0,
    })
}

pub fn radio_cap(radio_mac: MacAddr) -> Box<RadioCapInfo> {
    let mut cap = RadioCapInfo::zeroed();
    cap.ruid = interface(radio_mac);
    cap.ht_cap.ruid = radio_mac;
    cap.vht_cap.ruid = radio_mac;
    cap.he_cap.ruid = radio_mac;
    cap.wifi7_cap.mlo_cap_support.ruid = radio_mac;
    cap.ch_scan.ruid = radio_mac;
    cap.radio_ad_cap.ruid = radio_mac;
    cap.cac_cap.ruid = radio_mac;
    cap
}

/// A zeroed policy of `kind` on `radio_mac`, in the global network.
pub fn policy(kind: PolicyIdType, radio_mac: MacAddr) -> Box<Policy> {
    let mut policy = blank_policy(NetId::global(), kind);
    policy.id.radio_mac = radio_mac;
    Box::new(policy)
}

/// A network SSID row with the given id, SSID text and haul types.
pub fn network_ssid(id: &str, ssid: &str, hauls: &[HaulType]) -> Box<NetworkSsidInfo> {
    Box::new(NetworkSsidInfo {
        id: CStrBytes::new(id.as_bytes()).unwrap(),
        ssid: CStrBytes::new(ssid.as_bytes()).unwrap(),
        pass_phrase: Secret::default(),
        band: BoundedVec::new(),
        enable: true,
        akm: BoundedVec::new(),
        suite_select: CStrBytes::default(),
        advertisement: true,
        mfp: CStrBytes::default(),
        mobility_domain: MacAddr::ZERO,
        haul_type: BoundedVec::try_from_vec(hauls.to_vec()).unwrap(),
        auth_type: CStrBytes::default(),
        vlan_id: 0,
    })
}

/// A device record with the AL MAC `al`.
pub fn device(al: MacAddr) -> Box<DeviceInfo> {
    let mut device = Box::new(DeviceInfo::zeroed());
    device.intf.mac = al;
    device
}

/// An affiliated AP with a valid link MAC.
pub fn affiliated_ap(radio_mac: MacAddr, link_mac: MacAddr, link_id: u8) -> AffiliatedApInfo {
    AffiliatedApInfo {
        mac_addr_valid: true,
        link_id_valid: true,
        ruid: interface(radio_mac),
        mac_addr: link_mac,
        link_id,
    }
}

/// An AP MLD of `haul` with a valid MAC and the given affiliated APs.
pub fn ap_mld(haul: HaulType, mld_mac: MacAddr, aps: Vec<AffiliatedApInfo>) -> ApMldInfo {
    ApMldInfo {
        mac_addr_valid: true,
        ssid: CStrBytes::new(b"mld-ssid").unwrap(),
        mac_addr: mld_mac,
        str: true,
        nstr: false,
        emlsr: false,
        emlmr: false,
        haul_type: haul,
        affiliated_ap: BoundedVec::try_from_vec(aps).unwrap(),
    }
}

/// An agent model with its defaults.
pub fn agent_model() -> crate::AgentModel {
    crate::AgentModel::new(node_init(NodeRole::Agent))
}
