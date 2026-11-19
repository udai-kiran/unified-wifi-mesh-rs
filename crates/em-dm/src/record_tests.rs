// SPDX-License-Identifier: Apache-2.0

//! Tests of the large records: the STA frame body, the bounded collections, the string limits of
//! the members with their own capacities, and the redaction of credentials in `Debug` output.
//! Each test first proves, with a typed closure, that the record field has the type under test.

use em_types::{Interface, MacAddr, MediaType};

use crate::limits::{EM_MAX_FRAME_BODY_LEN, LONG_STRING_CAP, SHORT_STRING_CAP, SSID_CAP};
use crate::{
    AffiliatedApInfo, AffiliatedBstaInfo, AffiliatedStaInfo, AlarmSamples, ApMldInfo,
    AssocStaMldInfo, BackhaulBssConfigPolicy, BeaconReport, BoundedVec, BssId, BssInfo,
    BstaMldInfo, CStrBytes, CapacityError, EhtOperationsBss, ExtDeviceInfo, Float32, HaulType,
    LenBytes, LinkReport, LinkStatsAlarmCfg, Neighbor, NetworkSsidInfo, Policy, ProductClass,
    QosMgtPolicy, ScanCapOpClassInfo, ScanResult, ScanResultId, Secret, StaInfo, StringError, Text,
    TidToLinkInfo, TidToLinkMapInfo, TrafficSeparSsidInfo, TrafficSeparationPolicy, UBits,
    UnassocQueryChannel, UnassocQueryList, UnassocQueryOpclass, UnassocStaMetricEntry,
    UnassocStaMetricsRsp, VapMode,
};

// ---------------------------------------------------------------------------
// STA frame body
// ---------------------------------------------------------------------------

#[test]
fn frame_body_cap_is_the_cited_constant() {
    let _: fn(&StaInfo) -> &BoundedVec<u8, EM_MAX_FRAME_BODY_LEN> = |r| &r.frame_body;
    // inc/em_base.h:74: (11454 - 24 - 4).
    assert_eq!(EM_MAX_FRAME_BODY_LEN, 11454 - 24 - 4);
    assert_eq!(EM_MAX_FRAME_BODY_LEN, 11_426);
}

#[test]
fn frame_body_constructor_accepts_the_limit_and_rejects_one_more() {
    let exact = vec![0xa5; 11_426];
    let built = BoundedVec::<u8, EM_MAX_FRAME_BODY_LEN>::try_from_vec(exact).unwrap();
    assert_eq!(built.len(), 11_426);
    assert_eq!(
        BoundedVec::<u8, EM_MAX_FRAME_BODY_LEN>::try_from_vec(vec![0; 11_427]),
        Err(CapacityError {
            capacity: 11_426,
            requested: 11_427
        })
    );
}

#[test]
fn frame_body_setter_accepts_the_limit_and_rejects_one_more() {
    let mut sta = StaInfo::default();
    assert_eq!(sta.set_frame_body(vec![0x5a; 11_426]), Ok(()));
    assert_eq!(sta.frame_body.len(), 11_426);
    assert_eq!(
        sta.set_frame_body(vec![0; 11_427]),
        Err(CapacityError {
            capacity: 11_426,
            requested: 11_427
        })
    );
    assert_eq!(
        sta.frame_body.len(),
        11_426,
        "a rejected set keeps the frame"
    );
}

#[test]
fn rejected_frame_body_update_preserves_the_existing_frame() {
    let mut sta = StaInfo::default();
    sta.set_frame_body(vec![1, 2, 3, 0, 255]).unwrap();
    assert!(sta.set_frame_body(vec![9; 11_427]).is_err());
    assert_eq!(sta.frame_body.as_slice(), &[1, 2, 3, 0, 255]);
    // An empty frame is a valid update and clears the body.
    assert_eq!(sta.set_frame_body(Vec::new()), Ok(()));
    assert!(sta.frame_body.is_empty());
}

// ---------------------------------------------------------------------------
// Bounded collections: push at capacity is an error and changes nothing
// ---------------------------------------------------------------------------

/// Fills the list behind `field` to its capacity, then checks that one more element is refused
/// and that the list is unchanged. `expected` is the C array size.
fn check_list<Owner, T, const CAP: usize>(
    _field: fn(&Owner) -> &BoundedVec<T, CAP>,
    expected: usize,
    make: fn() -> T,
) {
    assert_eq!(CAP, expected, "capacity of the list");
    assert_eq!(BoundedVec::<T, CAP>::CAPACITY, expected);
    let mut list = BoundedVec::<T, CAP>::new();
    for _ in 0..CAP {
        assert_eq!(list.push(make()), Ok(()));
    }
    assert_eq!(
        list.push(make()),
        Err(CapacityError {
            capacity: CAP,
            requested: CAP + 1
        })
    );
    assert_eq!(list.len(), CAP, "a refused push keeps the list");
}

fn short_text() -> CStrBytes<SHORT_STRING_CAP> {
    CStrBytes::new(b"SAE").unwrap()
}

fn long_text() -> CStrBytes<LONG_STRING_CAP> {
    CStrBytes::new(b"vendor").unwrap()
}

fn mac() -> MacAddr {
    MacAddr::ZERO
}

fn interface() -> Interface {
    Interface {
        name: None,
        mac: MacAddr::ZERO,
        media: MediaType::IEEE_802_3AB,
    }
}

#[test]
fn bss_collections_hold_their_c_array_sizes() {
    check_list(|r: &BssInfo| &r.fronthaul_akm, 10, short_text);
    check_list(|r: &BssInfo| &r.backhaul_akm, 10, short_text);
    check_list(|r: &BssInfo| &r.vendor_elements, 2310, || 0u8);
}

#[test]
fn policy_collections_hold_their_c_array_sizes() {
    check_list(|r: &Policy| &r.sta_mac, 16, mac);
    check_list(
        |r: &Policy| &r.qos_mgt,
        256,
        || QosMgtPolicy {
            msc_mac: BoundedVec::new(),
            sc_mac: BoundedVec::new(),
        },
    );
    check_list(
        |r: &Policy| &r.backhaul_bss_config,
        24,
        || BackhaulBssConfigPolicy {
            bssid: MacAddr::ZERO,
            b_profile_1_sta_disallowed: false,
            b_profile_2_sta_disallowed: false,
        },
    );
    check_list(|r: &QosMgtPolicy| &r.msc_mac, 16, mac);
    check_list(|r: &QosMgtPolicy| &r.sc_mac, 16, mac);
    check_list(
        |r: &TrafficSeparationPolicy| &r.ssid_info,
        5,
        || TrafficSeparSsidInfo {
            ssid: LenBytes::default(),
            vlan_id: 0,
        },
    );
}

#[test]
fn sta_collections_hold_their_c_array_sizes() {
    check_list(|r: &StaInfo| &r.vendor_info, 5, long_text);
    check_list(|r: &StaInfo| &r.beacon_reports, 10, beacon_report);
    check_list(|r: &StaInfo| &r.beacon_report_elem, 400, || 0u8);
    check_list(
        |r: &LinkReport| &r.alarm_sample,
        10,
        || AlarmSamples {
            link_quality_score: Float32::new(0.0),
            reporting_time: CStrBytes::default(),
            snr: Float32::new(0.0),
            per: Float32::new(0.0),
            phy: Float32::new(0.0),
        },
    );
}

#[test]
fn scan_and_mld_collections_hold_their_c_array_sizes() {
    check_list(
        |r: &ScanResult| &r.neighbor,
        16,
        || Neighbor {
            bssid: MacAddr::ZERO,
            ssid: CStrBytes::default(),
            signal_strength: 0,
            bandwidth: crate::ChannelBandwidth::MHZ_20,
            bss_color: 0,
            channel_util: 0,
            sta_count: 0,
            bss_load_element_present: 0,
        },
    );
    check_list(
        |r: &ApMldInfo| &r.affiliated_ap,
        64,
        || AffiliatedApInfo {
            mac_addr_valid: false,
            link_id_valid: false,
            ruid: interface(),
            mac_addr: MacAddr::ZERO,
            link_id: 0,
        },
    );
    check_list(
        |r: &BstaMldInfo| &r.affiliated_bsta,
        64,
        || AffiliatedBstaInfo {
            mac_addr_valid: false,
            ruid: interface(),
            mac_addr: MacAddr::ZERO,
        },
    );
    check_list(
        |r: &AssocStaMldInfo| &r.affiliated_sta,
        64,
        || AffiliatedStaInfo {
            bssid: MacAddr::ZERO,
            link_addr: MacAddr::ZERO,
        },
    );
    check_list(|r: &TidToLinkInfo| &r.tid_to_link_mapping, 64, tid_map);
}

#[test]
fn scan_capability_channels_cannot_exceed_the_c_array() {
    // The channel count `num` is the list length: a count above 64 has no representation.
    check_list(|r: &ScanCapOpClassInfo| &r.channels, 64, || 0u8);
    assert_eq!(
        BoundedVec::<u8, 64>::try_from_vec(vec![36; 65]),
        Err(CapacityError {
            capacity: 64,
            requested: 65
        })
    );
    let full = ScanCapOpClassInfo {
        op_class: 115,
        channels: BoundedVec::try_from_vec(vec![36; 64]).unwrap(),
    };
    assert_eq!(full.channels.len(), 64);
}

#[test]
fn unassoc_collections_hold_their_c_array_sizes() {
    check_list(|r: &UnassocQueryChannel| &r.sta_list, 64, mac);
    check_list(
        |r: &UnassocQueryOpclass| &r.channel_list,
        16,
        || UnassocQueryChannel {
            channel: 0,
            sta_list: BoundedVec::new(),
        },
    );
    check_list(
        |r: &UnassocQueryList| &r.opclass_list,
        64,
        || UnassocQueryOpclass {
            op_class: 0,
            channel_list: BoundedVec::new(),
        },
    );
    check_list(
        |r: &UnassocStaMetricsRsp| &r.entry,
        64,
        || UnassocStaMetricEntry {
            sta_mac: MacAddr::ZERO,
            channel: 0,
            op_class: 0,
            rcpi: 0,
            time_delta: 0,
        },
    );
}

fn beacon_report() -> BeaconReport {
    BeaconReport {
        op_class: 0,
        channel: 0,
        start_time: 0,
        duration: 0,
        frame_info: 0,
        rcpi: 0,
        rsni: 0,
        bssid: MacAddr::ZERO,
        antenna: 0,
        tsf: 0,
        wide_bandwidth_channel_present: false,
        wide_bandwidth_channel: crate::WideBwChannel {
            bandwidth: 0,
            center_seg0: 0,
            center_seg1: 0,
        },
        num_repetitions: 0,
    }
}

fn tid_map() -> TidToLinkMapInfo {
    TidToLinkMapInfo {
        add_remove: false,
        sta_mld_mac_addr: MacAddr::ZERO,
        direction: false,
        default_link_map: false,
        map_switch_time_present: false,
        expected_dur_present: false,
        link_map_size: false,
        link_map_presence_ind: 0,
        expected_dur: [0; 3],
        tid_to_link_map: 0,
    }
}

// ---------------------------------------------------------------------------
// String limits of the members with their own capacities
// ---------------------------------------------------------------------------

#[test]
fn traffic_separation_ssid_keeps_the_upstream_33_byte_member() {
    let _: fn(&TrafficSeparSsidInfo) -> &LenBytes<33> = |r| &r.ssid;
    // Length-delimited: NUL and non-UTF-8 bytes are payload, 33 bytes fit, 34 do not.
    let mut full = [0u8; 33];
    full[0] = 0xff;
    assert_eq!(LenBytes::<33>::new(&full).map(|s| s.len()), Ok(33));
    assert_eq!(
        LenBytes::<33>::new(&[1u8; 34]),
        Err(StringError::TooLong {
            len: 34,
            capacity: 33
        })
    );
}

#[test]
fn ssid_t_members_are_32_bytes_and_the_sta_ssid_is_127() {
    let _: fn(&BssInfo) -> &CStrBytes<SSID_CAP> = |r| &r.ssid;
    let _: fn(&Neighbor) -> &CStrBytes<SSID_CAP> = |r| &r.ssid;
    let _: fn(&ApMldInfo) -> &CStrBytes<SSID_CAP> = |r| &r.ssid;
    let _: fn(&StaInfo) -> &CStrBytes<LONG_STRING_CAP> = |r| &r.ssid;
    assert!(CStrBytes::<SSID_CAP>::new(&[b's'; 32]).is_ok());
    assert!(CStrBytes::<SSID_CAP>::new(&[b's'; 33]).is_err());
    assert!(CStrBytes::<LONG_STRING_CAP>::new(&[b's'; 127]).is_ok());
    assert_eq!(
        CStrBytes::<LONG_STRING_CAP>::new(b"a\0b"),
        Err(StringError::EmbeddedNul { position: 1 })
    );
}

#[test]
fn new_ids_carry_the_network_id_as_text() {
    let _: fn(&BssId) -> &Text<LONG_STRING_CAP> = |r| &r.net_id;
    let _: fn(&ScanResultId) -> &Text<LONG_STRING_CAP> = |r| &r.net_id;
    let _: fn(&crate::PolicyId) -> &Text<LONG_STRING_CAP> = |r| &r.net_id;
    assert!(Text::<LONG_STRING_CAP>::new(&"n".repeat(127)).is_ok());
    assert!(Text::<LONG_STRING_CAP>::new(&"n".repeat(128)).is_err());
}

#[test]
fn floats_compare_by_bit_pattern_inside_records() {
    let config = |threshold: f32| LinkStatsAlarmCfg {
        collection_start_time: CStrBytes::default(),
        reporting_interval: 60,
        link_quality_threshold: Float32::new(threshold),
    };
    assert_eq!(config(0.5), config(0.5));
    assert_ne!(config(0.5), config(0.25));
}

// ---------------------------------------------------------------------------
// Credentials never appear in Debug output
// ---------------------------------------------------------------------------

fn secret_bytes(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

#[test]
fn passphrase_is_redacted_in_network_ssid_debug() {
    let _: fn(&NetworkSsidInfo) -> &Secret<CStrBytes<LONG_STRING_CAP>> = |r| &r.pass_phrase;
    let record = NetworkSsidInfo {
        id: CStrBytes::default(),
        ssid: CStrBytes::new(b"homenet").unwrap(),
        pass_phrase: Secret::new(CStrBytes::new(b"correct-horse-battery").unwrap()),
        band: BoundedVec::new(),
        enable: true,
        akm: BoundedVec::new(),
        suite_select: CStrBytes::default(),
        advertisement: true,
        mfp: CStrBytes::default(),
        mobility_domain: MacAddr::ZERO,
        haul_type: BoundedVec::new(),
        auth_type: CStrBytes::default(),
        vlan_id: 0,
    };
    let shown = format!("{record:?} {record:#?}");
    assert!(!shown.contains("correct-horse-battery"));
    assert!(!shown.contains("99, 111, 114"), "no byte list either");
    assert!(shown.contains("redacted"));
    assert!(shown.contains("homenet"), "the SSID is not a credential");
    assert_eq!(
        record.pass_phrase.expose().as_bytes(),
        &secret_bytes("correct-horse-battery")[..]
    );
}

#[test]
fn client_credentials_are_redacted_in_ext_device_debug() {
    let record = ExtDeviceInfo {
        received: true,
        boot_id: 7,
        client_id: Secret::new(LenBytes::new(b"client-id-0123").unwrap()),
        client_secret: Secret::new(LenBytes::new(b"s3cr3t-value").unwrap()),
        product_class: ProductClass::GW,
        device_role: crate::DeviceRole::from(0),
    };
    let shown = format!("{record:?} {record:#?}");
    assert!(!shown.contains("client-id-0123"));
    assert!(!shown.contains("s3cr3t-value"));
    assert_eq!(shown.matches("redacted").count(), 4);
}

fn eht_ops_bss() -> EhtOperationsBss {
    EhtOperationsBss {
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
    }
}

fn bss_with_passphrase(passphrase: &[u8]) -> BssInfo {
    BssInfo {
        vap_index: 1,
        vap_mode: VapMode::from(0),
        id: BssId {
            net_id: Text::default(),
            dev_mac: MacAddr::ZERO,
            ruid: MacAddr::ZERO,
            bssid: MacAddr::ZERO,
            haul_type: HaulType::FRONTHAUL,
        },
        bssid: interface(),
        ruid: interface(),
        ssid: CStrBytes::new(b"meshnet").unwrap(),
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
        eht_ops: eht_ops_bss(),
        mesh_sta_passphrase: Secret::new(CStrBytes::new(passphrase).unwrap()),
        vlan_id: 0,
        mld_mac: MacAddr::ZERO,
        sta_mac: MacAddr::ZERO,
        vendor_elements: BoundedVec::new(),
        connect_status: false,
    }
}

#[test]
fn mesh_sta_passphrase_is_redacted_in_bss_debug() {
    let _: fn(&BssInfo) -> &Secret<CStrBytes<SHORT_STRING_CAP>> = |r| &r.mesh_sta_passphrase;
    let record = bss_with_passphrase(b"backhaul-psk-value");
    let shown = format!("{record:?} {record:#?}");
    assert!(!shown.contains("backhaul-psk-value"));
    assert!(shown.contains("redacted"));
    assert!(shown.contains("meshnet"));
    // 63 payload bytes fit char[64]; the Secret wrapper does not change the limit.
    assert!(CStrBytes::<SHORT_STRING_CAP>::new(&[b'p'; 63]).is_ok());
    assert!(CStrBytes::<SHORT_STRING_CAP>::new(&[b'p'; 64]).is_err());
}
