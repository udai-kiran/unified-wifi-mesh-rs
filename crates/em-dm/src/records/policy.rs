// SPDX-License-Identifier: Apache-2.0

//! Records of the `policy` group (see the module docs of `records`).

use crate::bounded::BoundedVec;
use crate::enums::{PolicyIdType, SteeringPolicyType};
use crate::float::Float32;
use crate::limits::{LONG_STRING_CAP, SMALL_STRING_CAP, STRING_CAP};
#[cfg(test)]
use crate::mapping::{Map, NO_WIRE};
use crate::text::{CStrBytes, LenBytes, Text};
use em_types::MacAddr;

record! {
    /// Identity of a policy.
    ///
    /// C `em_policy_id_t` (`inc/em_base.h:3778`).
    pub struct PolicyId = "em_policy_id_t", copy {
        /// C `net_id`: `em_long_string_t`.
        pub net_id: Text<LONG_STRING_CAP> = Map::field("net_id").text("key text parsed with strncpy from the database key string (src/dm/dm_policy.cpp:298)").wire(NO_WIRE),
        /// C `dev_mac`: `mac_address_t`.
        pub dev_mac: MacAddr = Map::field("dev_mac"),
        /// C `radio_mac`: `mac_address_t`.
        pub radio_mac: MacAddr = Map::field("radio_mac"),
        /// C `type`: `em_policy_id_type_t`.
        pub kind: PolicyIdType = Map::field("type"),
    }
}

record! {
    /// Default 802.1Q settings policy.
    ///
    /// C `em_8021q_settings_policy_t` (`inc/em_base.h:3792`).
    pub struct Ieee8021qSettingsPolicy = "em_8021q_settings_policy_t", copy {
        /// C `primary_vid`: `unsigned short`.
        pub primary_vid: u16 = Map::field("primary_vid"),
        /// C `default_pcp`: `unsigned char`.
        pub default_pcp: u8 = Map::field("default_pcp"),
    }
}

record! {
    /// One SSID of the traffic separation policy. The 33-byte member keeps its upstream capacity; the 32-byte SSID rule (0007 A1) is for `ssid_t` only.
    ///
    /// C `em_ssid_info_traffic_separ_pol_t` (`inc/em_base.h:3797`).
    pub struct TrafficSeparSsidInfo = "em_ssid_info_traffic_separ_pol_t", copy {
        /// C `ssid`: `char[MAX_WIFI_SSID_LEN]` and length `ssid_len`.
        /// Wire: none at the pin; `em_policy_t.traffic_separ` is never written. The Traffic Separation
        /// Policy reader (`src/em/policy_cfg/em_policy_cfg.cpp:804`) fills the same-shaped
        /// `em_traffic_sep_policy_ssid_t` instead, accepting up to 33 bytes and writing a NUL one past
        /// the member at 33 (upstream defect); a consumer storing it here keeps the 33-byte capacity.
        pub ssid: LenBytes<33> = Map::field("ssid").len_bytes("ssid_len").wire(NO_WIRE),
        /// C `vlan_id`: `unsigned short`.
        pub vlan_id: u16 = Map::field("vlan_id"),
    }
}

record! {
    /// Traffic separation policy.
    ///
    /// C `em_traffic_separation_policy_t` (`inc/em_base.h:3803`).
    pub struct TrafficSeparationPolicy = "em_traffic_separation_policy_t", owned {
        /// C `ssid_info`: `em_ssid_info_traffic_separ_pol_t[em_haul_type_max]` with count `num_ssids`.
        pub ssid_info: BoundedVec<TrafficSeparSsidInfo, 5> = Map::counted("num_ssids", "ssid_info"),
    }
}

record! {
    /// Quality-of-service management policy of one STA group.
    ///
    /// C `em_qos_mgt_policy_t` (`inc/em_base.h:3808`).
    pub struct QosMgtPolicy = "em_qos_mgt_policy_t", owned {
        /// C `msc_mac`: `mac_address_t[EM_MAX_MSC_PER_TRAFFIC_SEPAR]` with count `num_mscs`.
        pub msc_mac: BoundedVec<MacAddr, 16> = Map::counted("num_mscs", "msc_mac"),
        /// C `sc_mac`: `mac_address_t[EM_MAX_SCS_PER_TRAFFIC_SEPAR]` with count `num_scs`.
        pub sc_mac: BoundedVec<MacAddr, 16> = Map::counted("num_scs", "sc_mac"),
    }
}

record! {
    /// Backhaul BSS configuration policy of one BSS.
    ///
    /// C `em_backhaul_bss_config_policy_t` (`inc/em_base.h:3815`).
    pub struct BackhaulBssConfigPolicy = "em_backhaul_bss_config_policy_t", copy {
        /// C `bssid`: `bssid_t`.
        pub bssid: MacAddr = Map::field("bssid"),
        /// C `b_profile_1_sta_disallowed`: `bool`.
        pub b_profile_1_sta_disallowed: bool = Map::field("b_profile_1_sta_disallowed"),
        /// C `b_profile_2_sta_disallowed`: `bool`.
        pub b_profile_2_sta_disallowed: bool = Map::field("b_profile_2_sta_disallowed"),
    }
}

record! {
    /// Link statistics alarm configuration.
    ///
    /// C `em_link_stats_alarm_cfg_t` (`inc/em_base.h:2168`).
    pub struct LinkStatsAlarmCfg = "em_link_stats_alarm_cfg_t", copy {
        /// C `collection_start_time`: `em_string_t`.
        /// Wire: the reader copies the whole 32-byte array with no NUL check (into a local struct, not stored at the pin).
        pub collection_start_time: CStrBytes<STRING_CAP> = Map::field("collection_start_time").cstr().wire("32 @ src/em/policy_cfg/em_policy_cfg.cpp:923"),
        /// C `reporting_interval`: `unsigned int`.
        pub reporting_interval: u32 = Map::field("reporting_interval"),
        /// C `link_quality_threshold`: `float`.
        pub link_quality_threshold: Float32 = Map::field("link_quality_threshold"),
    }
}

record! {
    /// Client filter configuration.
    ///
    /// C `em_client_filters_cfg_t` (`inc/em_base.h:2174`).
    pub struct ClientFiltersCfg = "em_client_filters_cfg_t", copy {
        /// C `sta_mac`: `mac_addr_t`.
        pub sta_mac: MacAddr = Map::field("sta_mac"),
        /// C `consec_alarm_thres_cnt`: `unsigned int`.
        pub consec_alarm_thres_cnt: u32 = Map::field("consec_alarm_thres_cnt"),
        /// C `collect_duration`: `em_small_string_t`.
        /// Wire: the reader copies the whole 16-byte array with no NUL check (into a local struct, not stored at the pin).
        pub collect_duration: CStrBytes<SMALL_STRING_CAP> = Map::field("collect_duration").cstr().wire("16 @ src/em/policy_cfg/em_policy_cfg.cpp:935"),
    }
}

record! {
    /// The policy of one device or radio. Large: store it as `Box<Policy>` in collections (inline size in the crate README). The 256 quality-of-service entries are a `BoundedVec`, not an inline array.
    ///
    /// C `em_policy_t` (`inc/em_base.h:3821`).
    pub struct Policy = "em_policy_t", owned {
        /// C `id`: `em_policy_id_t`.
        pub id: PolicyId = Map::field("id"),
        /// C `sta_mac`: `mac_address_t[EM_MAX_STA_PER_STEER_POLICY]` with count `num_sta`.
        pub sta_mac: BoundedVec<MacAddr, 16> = Map::counted("num_sta", "sta_mac"),
        /// C `policy`: `em_steering_policy_type_t`.
        pub policy: SteeringPolicyType = Map::field("policy"),
        /// C `util_threshold`: `unsigned short`.
        pub util_threshold: u16 = Map::field("util_threshold"),
        /// C `rcpi_threshold`: `unsigned short`.
        pub rcpi_threshold: u16 = Map::field("rcpi_threshold"),
        /// C `interval`: `unsigned short`.
        pub interval: u16 = Map::field("interval"),
        /// C `rcpi_hysteresis`: `unsigned short`.
        pub rcpi_hysteresis: u16 = Map::field("rcpi_hysteresis"),
        /// C `sta_traffic_stats`: `bool`.
        pub sta_traffic_stats: bool = Map::field("sta_traffic_stats"),
        /// C `sta_link_metric`: `bool`.
        pub sta_link_metric: bool = Map::field("sta_link_metric"),
        /// C `sta_status`: `bool`.
        pub sta_status: bool = Map::field("sta_status"),
        /// C `managed_sta_marker`: `em_long_string_t`.
        /// Wire: the reader accepts a marker shorter than 32 bytes (into a local struct, not stored at the pin).
        pub managed_sta_marker: CStrBytes<LONG_STRING_CAP> = Map::field("managed_sta_marker").cstr().wire("31 @ src/em/policy_cfg/em_policy_cfg.cpp:910"),
        /// C `independent_scan_report`: `bool`.
        pub independent_scan_report: bool = Map::field("independent_scan_report"),
        /// C `bssid`: `bssid_t`.
        pub bssid: MacAddr = Map::field("bssid"),
        /// C `profile_1_sta_disallowed`: `bool`.
        pub profile_1_sta_disallowed: bool = Map::field("profile_1_sta_disallowed"),
        /// C `profile_2_sta_disallowed`: `bool`.
        pub profile_2_sta_disallowed: bool = Map::field("profile_2_sta_disallowed"),
        /// C `report_unassoc_sta`: `bool`.
        pub report_unassoc_sta: bool = Map::field("report_unassoc_sta"),
        /// C `max_reporting_rate`: `unsigned int`.
        pub max_reporting_rate: u32 = Map::field("max_reporting_rate"),
        /// C `def_8021q_settings`: `em_8021q_settings_policy_t`.
        pub def_8021q_settings: Ieee8021qSettingsPolicy = Map::field("def_8021q_settings"),
        /// C `traffic_separ`: `em_traffic_separation_policy_t`.
        pub traffic_separ: TrafficSeparationPolicy = Map::field("traffic_separ"),
        /// C `qos_mgt`: `em_qos_mgt_policy_t[EM_MAX_STA_PER_AGENT]` with count `num_qos_mgt`.
        pub qos_mgt: BoundedVec<QosMgtPolicy, 256> = Map::counted("num_qos_mgt", "qos_mgt"),
        /// C `backhaul_bss_config`: `em_backhaul_bss_config_policy_t[EM_MAX_BSSS]` with count `num_backhaul_bss_config`.
        pub backhaul_bss_config: BoundedVec<BackhaulBssConfigPolicy, 24> = Map::counted("num_backhaul_bss_config", "backhaul_bss_config"),
        /// C `link_stats_alarm_cfg`: `em_link_stats_alarm_cfg_t`.
        pub link_stats_alarm_cfg: LinkStatsAlarmCfg = Map::field("link_stats_alarm_cfg"),
        /// C `client_filters`: `em_client_filters_cfg_t`.
        pub client_filters: ClientFiltersCfg = Map::field("client_filters"),
    }
}
