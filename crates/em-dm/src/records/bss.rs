// SPDX-License-Identifier: Apache-2.0

//! Records of the `bss` group (see the module docs of `records`).

use crate::bounded::BoundedVec;
use crate::enums::{HaulType, VapMode};
use crate::limits::{LONG_STRING_CAP, SHORT_STRING_CAP, SSID_CAP, STRING_CAP};
#[cfg(test)]
use crate::mapping::{Map, NO_WIRE};
use crate::records::EhtOperationsBss;
use crate::secret::Secret;
use crate::text::{CStrBytes, Text};
use em_types::{Interface, MacAddr};

record! {
    /// Identity of a BSS.
    ///
    /// C `em_bss_id_t` (`inc/em_base.h:2775`).
    pub struct BssId = "em_bss_id_t", copy {
        /// C `net_id`: `em_long_string_t`.
        pub net_id: Text<LONG_STRING_CAP> = Map::field("net_id").text("key text parsed with strncpy from the database key string (src/dm/dm_bss.cpp:405)").wire(NO_WIRE),
        /// C `dev_mac`: `mac_address_t`.
        pub dev_mac: MacAddr = Map::field("dev_mac"),
        /// C `ruid`: `mac_address_t`.
        pub ruid: MacAddr = Map::field("ruid"),
        /// C `bssid`: `mac_address_t`.
        pub bssid: MacAddr = Map::field("bssid"),
        /// C `haul_type`: `em_haul_type_t`.
        pub haul_type: HaulType = Map::field("haul_type"),
    }
}

record! {
    /// A BSS. Large: store it as `Box<BssInfo>` in collections (inline size in the crate README).
    ///
    /// C `em_bss_info_t` (`inc/em_base.h:2783`).
    pub struct BssInfo = "em_bss_info_t", owned {
        /// C `vap_index`: `unsigned int`.
        pub vap_index: u32 = Map::field("vap_index"),
        /// C `vap_mode`: `em_vap_mode_t`.
        pub vap_mode: VapMode = Map::field("vap_mode"),
        /// C `id`: `em_bss_id_t`.
        pub id: BssId = Map::field("id"),
        /// C `bssid`: `em_interface_t`.
        pub bssid: Interface = Map::field("bssid"),
        /// C `ruid`: `em_interface_t`.
        pub ruid: Interface = Map::field("ruid"),
        /// C `ssid`: `ssid_t`.
        /// Wire: the reader clamps the SSID length to 31 bytes (`:2271`) and copies at `:2277`.
        pub ssid: CStrBytes<SSID_CAP> = Map::field("ssid").cstr().wire("31 @ src/em/config/em_configuration.cpp:2271").tag("0007-A1"),
        /// C `enabled`: `bool`.
        pub enabled: bool = Map::field("enabled"),
        /// C `last_change`: `unsigned int`.
        pub last_change: u32 = Map::field("last_change"),
        /// C `timestamp`: `em_long_string_t`.
        pub timestamp: CStrBytes<LONG_STRING_CAP> = Map::field("timestamp").cstr().wire(NO_WIRE),
        /// C `numberofsta`: `unsigned int`.
        pub numberofsta: u32 = Map::field("numberofsta"),
        /// C `channel_util`: `unsigned int`.
        pub channel_util: u32 = Map::field("channel_util"),
        /// C `unicast_bytes_sent`: `unsigned int`.
        pub unicast_bytes_sent: u32 = Map::field("unicast_bytes_sent"),
        /// C `unicast_bytes_rcvd`: `unsigned int`.
        pub unicast_bytes_rcvd: u32 = Map::field("unicast_bytes_rcvd"),
        /// C `multicast_bytes_sent`: `unsigned int`.
        pub multicast_bytes_sent: u32 = Map::field("multicast_bytes_sent"),
        /// C `multicast_bytes_rcvd`: `unsigned int`.
        pub multicast_bytes_rcvd: u32 = Map::field("multicast_bytes_rcvd"),
        /// C `broadcast_bytes_sent`: `unsigned int`.
        pub broadcast_bytes_sent: u32 = Map::field("broadcast_bytes_sent"),
        /// C `broadcast_bytes_rcvd`: `unsigned int`.
        pub broadcast_bytes_rcvd: u32 = Map::field("broadcast_bytes_rcvd"),
        /// C `inc_esp_ac_be`: `bool`.
        pub inc_esp_ac_be: bool = Map::field("inc_esp_ac_be"),
        /// C `inc_esp_ac_bk`: `bool`.
        pub inc_esp_ac_bk: bool = Map::field("inc_esp_ac_bk"),
        /// C `inc_esp_ac_vo`: `bool`.
        pub inc_esp_ac_vo: bool = Map::field("inc_esp_ac_vo"),
        /// C `inc_esp_ac_vi`: `bool`.
        pub inc_esp_ac_vi: bool = Map::field("inc_esp_ac_vi"),
        /// C `est_svc_params_be`: `em_string_t`.
        pub est_svc_params_be: CStrBytes<STRING_CAP> = Map::field("est_svc_params_be").cstr().wire(NO_WIRE),
        /// C `est_svc_params_bk`: `em_string_t`.
        pub est_svc_params_bk: CStrBytes<STRING_CAP> = Map::field("est_svc_params_bk").cstr().wire(NO_WIRE),
        /// C `est_svc_params_vi`: `em_string_t`.
        pub est_svc_params_vi: CStrBytes<STRING_CAP> = Map::field("est_svc_params_vi").cstr().wire(NO_WIRE),
        /// C `est_svc_params_vo`: `em_string_t`.
        pub est_svc_params_vo: CStrBytes<STRING_CAP> = Map::field("est_svc_params_vo").cstr().wire(NO_WIRE),
        /// C `byte_counter_units`: `unsigned int`.
        pub byte_counter_units: u32 = Map::field("byte_counter_units"),
        /// C `fronthaul_akm`: `em_short_string_t` with count `num_fronthaul_akms`.
        /// Wire: AKM Suite Capabilities (`em_configuration.cpp:5098`, count clamped to 10 at :5094) and WSC M1 auth-type flags (`:4709`, count not clamped, upstream defect 1); the consumer must clamp or reject above 10.
        pub fronthaul_akm: BoundedVec<CStrBytes<SHORT_STRING_CAP>, 10> = Map::counted("num_fronthaul_akms", "fronthaul_akm").cstr().wire("consumer: em-msg, src/em/config/em_configuration.cpp:5098"),
        /// C `backhaul_akm`: `em_short_string_t` with count `num_backhaul_akms`.
        /// Wire: AKM Suite Capabilities (`em_configuration.cpp:5108`, count clamped to 10 at :5104) and WSC M1 auth-type flags (`:4710`, count not clamped, upstream defect 1); the consumer must clamp or reject above 10.
        pub backhaul_akm: BoundedVec<CStrBytes<SHORT_STRING_CAP>, 10> = Map::counted("num_backhaul_akms", "backhaul_akm").cstr().wire("consumer: em-msg, src/em/config/em_configuration.cpp:5108"),
        /// C `profile_1b_sta_allowed`: `bool`.
        pub profile_1b_sta_allowed: bool = Map::field("profile_1b_sta_allowed"),
        /// C `profile_2b_sta_allowed`: `bool`.
        pub profile_2b_sta_allowed: bool = Map::field("profile_2b_sta_allowed"),
        /// C `assoc_allowed_status`: `unsigned int`.
        pub assoc_allowed_status: u32 = Map::field("assoc_allowed_status"),
        /// C `backhaul_use`: `bool`.
        pub backhaul_use: bool = Map::field("backhaul_use"),
        /// C `fronthaul_use`: `bool`.
        pub fronthaul_use: bool = Map::field("fronthaul_use"),
        /// C `r1_disallowed`: `bool`.
        pub r1_disallowed: bool = Map::field("r1_disallowed"),
        /// C `r2_disallowed`: `bool`.
        pub r2_disallowed: bool = Map::field("r2_disallowed"),
        /// C `multi_bssid`: `bool`.
        pub multi_bssid: bool = Map::field("multi_bssid"),
        /// C `transmitted_bssid`: `bool`.
        pub transmitted_bssid: bool = Map::field("transmitted_bssid"),
        /// C `eht_ops`: `em_eht_operations_bss_t`.
        pub eht_ops: EhtOperationsBss = Map::field("eht_ops"),
        /// C `mesh_sta_passphrase`: `em_short_string_t`. A credential: its `Debug` output is redacted.
        pub mesh_sta_passphrase: Secret<CStrBytes<SHORT_STRING_CAP>> = Map::field("mesh_sta_passphrase").cstr().wire(NO_WIRE),
        /// C `vlan_id`: `unsigned int`.
        pub vlan_id: u32 = Map::field("vlan_id"),
        /// C `mld_mac`: `mac_address_t`.
        pub mld_mac: MacAddr = Map::field("mld_mac"),
        /// C `sta_mac`: `mac_address_t`.
        pub sta_mac: MacAddr = Map::field("sta_mac"),
        /// C `vendor_elements`: `unsigned char[WIFI_AP_MAX_VENDOR_IE_LEN]` with count `vendor_elements_len`.
        pub vendor_elements: BoundedVec<u8, 2310> = Map::counted("vendor_elements_len", "vendor_elements"),
        /// C `connect_status`: `bool`.
        pub connect_status: bool = Map::field("connect_status"),
    }
}
