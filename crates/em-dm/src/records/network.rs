// SPDX-License-Identifier: Apache-2.0

//! Records of the `network` group (see the module docs of `records`).

use crate::bounded::BoundedVec;
use crate::enums::HaulType;
use crate::limits::{LONG_STRING_CAP, SHORT_STRING_CAP, SSID_CAP, STRING_CAP};
#[cfg(test)]
use crate::mapping::{Map, NO_WIRE};
use crate::secret::Secret;
use crate::text::{CStrBytes, Text};
use em_types::{Interface, MacAddr, MediaType};

record! {
    /// C `em_interface_preference_t` (`inc/em_base.h:2507`).
    pub struct InterfacePreference = "em_interface_preference_t", copy {
        /// C `platform`: `em_short_string_t`.
        pub platform: CStrBytes<SHORT_STRING_CAP> = Map::field("platform").cstr().wire(NO_WIRE),
        /// C `media`: `em_media_type_t`.
        pub media: MediaType = Map::field("media"),
    }
}

record! {
    /// A network. Large: store it as `Box<NetworkInfo>` in collections (inline size in the crate
    /// README).
    ///
    /// C `em_network_info_t` (`inc/em_base.h:2520`).
    pub struct NetworkInfo = "em_network_info_t", owned {
        /// C `id`: `em_long_string_t`.
        pub id: Text<LONG_STRING_CAP> = Map::field("id").text("copied with %s from a JSON string (src/dm/dm_easy_mesh.cpp:1019)").wire(NO_WIRE),
        /// C `num_of_devices`: `unsigned short`.
        pub num_of_devices: u16 = Map::field("num_of_devices"),
        /// C `timestamp`: `em_long_string_t`.
        pub timestamp: CStrBytes<LONG_STRING_CAP> = Map::field("timestamp").cstr().wire(NO_WIRE),
        /// C `ctrl_id`: `em_interface_t`.
        pub ctrl_id: Interface = Map::field("ctrl_id"),
        /// C `mscs_disallowed_sta`: `em_string_t` with count `num_mscs_disallowed_sta`.
        pub mscs_disallowed_sta: BoundedVec<CStrBytes<STRING_CAP>, 10> = Map::counted("num_mscs_disallowed_sta", "mscs_disallowed_sta").cstr().wire(NO_WIRE),
        /// C `scs_disallowed_sta`: `em_string_t` with count `num_scs_disallowed_sta`.
        pub scs_disallowed_sta: BoundedVec<CStrBytes<STRING_CAP>, 10> = Map::counted("num_scs_disallowed_sta", "scs_disallowed_sta").cstr().wire(NO_WIRE),
        /// C `colocated_agent_id`: `em_interface_t`.
        pub colocated_agent_id: Interface = Map::field("colocated_agent_id"),
        /// C `media`: `em_media_type_t`.
        pub media: MediaType = Map::field("media"),
    }
}

record! {
    /// The SSID settings of one network. Large: store it as `Box<NetworkSsidInfo>` in collections
    /// (inline size in the crate README).
    ///
    /// C `em_network_ssid_info_t` (`inc/em_base.h:2646`).
    pub struct NetworkSsidInfo = "em_network_ssid_info_t", owned {
        /// C `id`: `em_long_string_t`.
        pub id: CStrBytes<LONG_STRING_CAP> = Map::field("id").cstr().wire(NO_WIRE),
        /// C `ssid`: `ssid_t`.
        pub ssid: CStrBytes<SSID_CAP> = Map::field("ssid").cstr().wire("consumer: em-msg, src/em/config/em_configuration.cpp:4921").tag("0007-A1"),
        /// C `pass_phrase`: `em_long_string_t`. A credential: its `Debug` output is redacted.
        pub pass_phrase: Secret<CStrBytes<LONG_STRING_CAP>> = Map::field("pass_phrase").cstr().wire("consumer: em-msg, src/em/config/em_configuration.cpp:4934"),
        /// C `band`: `em_short_string_t` with count `num_bands`.
        pub band: BoundedVec<CStrBytes<SHORT_STRING_CAP>, 3> = Map::counted("num_bands", "band").cstr().wire(NO_WIRE),
        /// C `enable`: `bool`.
        pub enable: bool = Map::field("enable"),
        /// C `akm`: `em_short_string_t` with count `num_akms`.
        pub akm: BoundedVec<CStrBytes<SHORT_STRING_CAP>, 10> = Map::counted("num_akms", "akm").cstr().wire(NO_WIRE),
        /// C `suite_select`: `em_string_t`.
        pub suite_select: CStrBytes<STRING_CAP> = Map::field("suite_select").cstr().wire(NO_WIRE),
        /// C `advertisement`: `bool`.
        pub advertisement: bool = Map::field("advertisement"),
        /// C `mfp`: `em_string_t`.
        pub mfp: CStrBytes<STRING_CAP> = Map::field("mfp").cstr().wire(NO_WIRE),
        /// C `mobility_domain`: `mac_address_t`.
        pub mobility_domain: MacAddr = Map::field("mobility_domain"),
        /// C `haul_type`: `em_haul_type_t` with count `num_hauls`.
        pub haul_type: BoundedVec<HaulType, 8> = Map::counted("num_hauls", "haul_type"),
        /// C `auth_type`: `em_string_t`.
        pub auth_type: CStrBytes<STRING_CAP> = Map::field("auth_type").cstr().wire(NO_WIRE),
        /// C `vlan_id`: `unsigned short`.
        pub vlan_id: u16 = Map::field("vlan_id"),
    }
}

record! {
    /// C `em_ssid_2_vid_map_info_t` (`inc/em_base.h:2640`).
    pub struct Ssid2VidMapInfo = "em_ssid_2_vid_map_info_t", copy {
        /// C `id`: `em_long_string_t`.
        pub id: CStrBytes<LONG_STRING_CAP> = Map::field("id").cstr().wire(NO_WIRE),
        /// C `ssid`: `ssid_t`.
        pub ssid: CStrBytes<SSID_CAP> = Map::field("ssid").cstr().wire(NO_WIRE).tag("0007-A1"),
        /// C `vid`: `unsigned short`.
        pub vid: u16 = Map::field("vid"),
    }
}

/// An interface with no name, a zero MAC and media type 0: the zeroed C `em_interface_t`.
///
/// `em_types::Interface` has no `Default`, so the zeroed records of the model build it here.
#[must_use]
pub fn zeroed_interface() -> Interface {
    Interface {
        name: None,
        mac: MacAddr::ZERO,
        media: MediaType(0),
    }
}

impl NetworkInfo {
    /// The zeroed C record with `id` set: what upstream's `memset` and `init()` leave.
    #[must_use]
    pub fn zeroed(id: Text<LONG_STRING_CAP>) -> Self {
        Self {
            id,
            num_of_devices: 0,
            timestamp: CStrBytes::default(),
            ctrl_id: zeroed_interface(),
            mscs_disallowed_sta: BoundedVec::new(),
            scs_disallowed_sta: BoundedVec::new(),
            colocated_agent_id: zeroed_interface(),
            media: MediaType(0),
        }
    }
}
