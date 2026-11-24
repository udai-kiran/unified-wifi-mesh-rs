// SPDX-License-Identifier: Apache-2.0

//! Records of the `device` group (see the module docs of `records`).

use crate::bounded::BoundedVec;
use crate::enums::{DeviceRole, Ieee80211Variant, ProductClass};
use crate::limits::{
    CLIENT_ID_CAP, CLIENT_SECRET_CAP, LONG_STRING_CAP, SMALL_STRING_CAP, STRING_CAP,
    TINY_STRING_CAP,
};
#[cfg(test)]
use crate::mapping::{Map, NO_WIRE};
use crate::secret::Secret;
use crate::text::{CStrBytes, LenBytes, Text};
use em_types::{Interface, MacAddr, MediaType, ProfileType};

record! {
    /// C `em_device_id_t` (`inc/em_base.h:2540`).
    pub struct DeviceId = "em_device_id_t", copy {
        /// C `net_id`: `em_long_string_t`.
        pub net_id: Text<LONG_STRING_CAP> = Map::field("net_id").text("key text parsed with strncpy from the database key string (src/dm/dm_device.cpp:368)").wire(NO_WIRE),
        /// C `dev_mac`: `mac_address_t`.
        pub dev_mac: MacAddr = Map::field("dev_mac"),
        /// C `media`: `em_media_type_t`.
        pub media: MediaType = Map::field("media"),
    }
}

record! {
    /// C `ieee_1905_security_t` (`inc/em_base.h:2533`).
    pub struct Ieee1905Security = "ieee_1905_security_t", copy {
        /// C `auth_flags`: `unsigned short`.
        pub auth_flags: u16 = Map::field("auth_flags"),
        /// C `encr_flags`: `unsigned short`.
        pub encr_flags: u16 = Map::field("encr_flags"),
        /// C `conn_flags`: `unsigned short`.
        pub conn_flags: u16 = Map::field("conn_flags"),
        /// C `cfg_methods`: `unsigned short`.
        pub cfg_methods: u16 = Map::field("cfg_methods"),
    }
}

record! {
    /// Extended information of a device: product identity and client credentials. Large: store
    /// it as `Box<ExtDeviceInfo>` in collections (inline size in the crate README).
    ///
    /// C `em_ext_device_info_t` (`inc/em_base.h:2559`).
    pub struct ExtDeviceInfo = "em_ext_device_info_t", owned {
        /// C `received`: `bool`.
        pub received: bool = Map::field("received"),
        /// C `boot_id`: `unsigned int`.
        pub boot_id: u32 = Map::field("boot_id"),
        /// C `client_id`: `char[256]` and length `client_id_len`. A credential: its `Debug`
        /// output is redacted. The reader appends a NUL, so the payload is 255 bytes.
        pub client_id: Secret<LenBytes<CLIENT_ID_CAP>> = Map::field("client_id").len_bytes("client_id_len").reader_nul("src/em/config/em_configuration.cpp:1308").wire("255 @ src/em/config/em_configuration.cpp:1298"),
        /// C `client_secret`: `char[256]` and length `client_secret_len`. A credential: its
        /// `Debug` output is redacted. The reader appends a NUL, so the payload is 255 bytes.
        pub client_secret: Secret<LenBytes<CLIENT_SECRET_CAP>> = Map::field("client_secret").len_bytes("client_secret_len").reader_nul("src/em/config/em_configuration.cpp:1321").wire("255 @ src/em/config/em_configuration.cpp:1311"),
        /// C `product_class`: `emex_product_class_t`.
        pub product_class: ProductClass = Map::field("product_class"),
        /// C `device_role`: `emex_device_role_t`.
        pub device_role: DeviceRole = Map::field("device_role"),
    }
}

record! {
    /// A device. Large: store it as `Box<DeviceInfo>` in collections (inline size in the crate
    /// README).
    ///
    /// C `em_device_info_t` (`inc/em_base.h:2570`).
    pub struct DeviceInfo = "em_device_info_t", owned {
        /// C `id`: `em_device_id_t`.
        pub id: DeviceId = Map::field("id"),
        /// C `intf`: `em_interface_t`.
        pub intf: Interface = Map::field("intf"),
        /// C `profile`: `em_profile_type_t`.
        pub profile: ProfileType = Map::field("profile"),
        /// C `multi_ap_cap`: `em_long_string_t`.
        pub multi_ap_cap: CStrBytes<LONG_STRING_CAP> = Map::field("multi_ap_cap").cstr().wire(NO_WIRE),
        /// C `coll_interval`: `unsigned int`.
        pub coll_interval: u32 = Map::field("coll_interval"),
        /// C `report_unsuccess_assocs`: `bool`.
        pub report_unsuccess_assocs: bool = Map::field("report_unsuccess_assocs"),
        /// C `max_reporting_rate`: `unsigned short`.
        pub max_reporting_rate: u16 = Map::field("max_reporting_rate"),
        /// C `ap_metrics_reporting_interval`: `unsigned short`.
        pub ap_metrics_reporting_interval: u16 = Map::field("ap_metrics_reporting_interval"),
        /// C `manufacturer`: `em_long_string_t`.
        pub manufacturer: CStrBytes<LONG_STRING_CAP> = Map::field("manufacturer").cstr().wire("consumer: em-msg, src/em/config/em_configuration.cpp:4721"),
        /// C `serial_number`: `em_long_string_t`.
        pub serial_number: CStrBytes<LONG_STRING_CAP> = Map::field("serial_number").cstr().wire("consumer: em-msg, src/em/config/em_configuration.cpp:4732"),
        /// C `manufacturer_model`: `em_long_string_t`.
        pub manufacturer_model: CStrBytes<LONG_STRING_CAP> = Map::field("manufacturer_model").cstr().wire("consumer: em-msg, src/em/config/em_configuration.cpp:4726"),
        /// C `software_ver`: `em_string_t`.
        pub software_ver: CStrBytes<STRING_CAP> = Map::field("software_ver").cstr().wire("31 @ src/em/capability/em_capability.cpp:2031"),
        /// C `exec_env`: `em_string_t`.
        pub exec_env: CStrBytes<STRING_CAP> = Map::field("exec_env").cstr().wire(NO_WIRE),
        /// C `dscp_map`: `em_string_t`.
        pub dscp_map: CStrBytes<STRING_CAP> = Map::field("dscp_map").cstr().wire(NO_WIRE),
        /// C `max_pri_rules`: `unsigned char`.
        pub max_pri_rules: u8 = Map::field("max_pri_rules"),
        /// C `max_vids`: `unsigned char`.
        pub max_vids: u8 = Map::field("max_vids"),
        /// C `country_code`: `em_tiny_string_t`.
        pub country_code: CStrBytes<TINY_STRING_CAP> = Map::field("country_code").cstr().wire("2 @ src/em/capability/em_capability.cpp:1917"),
        /// C `prioritization_sup`: `bool`.
        pub prioritization_sup: bool = Map::field("prioritization_sup"),
        /// C `report_ind_scans`: `bool`.
        pub report_ind_scans: bool = Map::field("report_ind_scans"),
        /// C `traffic_sep_allowed`: `bool`.
        pub traffic_sep_allowed: bool = Map::field("traffic_sep_allowed"),
        /// C `svc_prio_allowed`: `bool`.
        pub svc_prio_allowed: bool = Map::field("svc_prio_allowed"),
        /// C `dfs_enable`: `bool`.
        pub dfs_enable: bool = Map::field("dfs_enable"),
        /// C `max_unsuccessful_assoc_report_rate`: `unsigned short`.
        pub max_unsuccessful_assoc_report_rate: u16 = Map::field("max_unsuccessful_assoc_report_rate"),
        /// C `sta_steer_state`: `bool`.
        pub sta_steer_state: bool = Map::field("sta_steer_state"),
        /// C `coord_cac_allowed`: `bool`.
        pub coord_cac_allowed: bool = Map::field("coord_cac_allowed"),
        /// C `ctrl_operation_mode`: `em_string_t`.
        pub ctrl_operation_mode: CStrBytes<STRING_CAP> = Map::field("ctrl_operation_mode").cstr().wire(NO_WIRE),
        /// C `backhaul_mac`: `em_interface_t`.
        pub backhaul_mac: Interface = Map::field("backhaul_mac"),
        /// C `backhaul_down_mac`: `em_string_t` with count `num_backhaul_down_mac`.
        pub backhaul_down_mac: BoundedVec<CStrBytes<STRING_CAP>, 16> = Map::counted("num_backhaul_down_mac", "backhaul_down_mac").cstr().wire(NO_WIRE),
        /// C `backhaul_media_type`: `wifi_ieee80211Variant_t`.
        pub backhaul_media_type: Ieee80211Variant = Map::field("backhaul_media_type"),
        /// C `backhaul_phyrate`: `unsigned int`.
        pub backhaul_phyrate: u32 = Map::field("backhaul_phyrate"),
        /// C `backhaul_alid`: `em_interface_t`.
        pub backhaul_alid: Interface = Map::field("backhaul_alid"),
        /// C `backhaul_sta`: `mac_address_t`.
        pub backhaul_sta: MacAddr = Map::field("backhaul_sta"),
        /// C `traffic_sep_cap`: `bool`.
        pub traffic_sep_cap: bool = Map::field("traffic_sep_cap"),
        /// C `easy_conn_cap`: `bool`.
        pub easy_conn_cap: bool = Map::field("easy_conn_cap"),
        /// C `test_cap`: `unsigned char`.
        pub test_cap: u8 = Map::field("test_cap"),
        /// C `apmld_maxlinks`: `unsigned char`.
        pub apmld_maxlinks: u8 = Map::field("apmld_maxlinks"),
        /// C `tidlink_map`: `unsigned char`.
        pub tidlink_map: u8 = Map::field("tidlink_map"),
        /// C `assoc_sta_reporting_int`: `unsigned char`.
        pub assoc_sta_reporting_int: u8 = Map::field("assoc_sta_reporting_int"),
        /// C `max_nummlds`: `unsigned char`.
        pub max_nummlds: u8 = Map::field("max_nummlds"),
        /// C `bstamld_maxlinks`: `unsigned char`.
        pub bstamld_maxlinks: u8 = Map::field("bstamld_maxlinks"),
        /// C `environment`: `em_string_t`.
        pub environment: CStrBytes<STRING_CAP> = Map::field("environment").cstr().wire(NO_WIRE),
        /// C `primary_device_type`: `em_small_string_t`.
        pub primary_device_type: CStrBytes<SMALL_STRING_CAP> = Map::field("primary_device_type").cstr().wire(NO_WIRE),
        /// C `secondary_device_type`: `em_small_string_t`.
        pub secondary_device_type: CStrBytes<SMALL_STRING_CAP> = Map::field("secondary_device_type").cstr().wire(NO_WIRE),
        /// C `sec_1905`: `ieee_1905_security_t`.
        pub sec_1905: Ieee1905Security = Map::field("sec_1905"),
        /// C `is_emplus_agent`: `uint8_t`.
        pub is_emplus_agent: u8 = Map::field("is_emplus_agent"),
        /// C `uptime`: `uint32_t`.
        pub uptime: u32 = Map::field("uptime"),
        /// C `total_mem`: `uint32_t`.
        pub total_mem: u32 = Map::field("total_mem"),
        /// C `free_mem`: `uint32_t`.
        pub free_mem: u32 = Map::field("free_mem"),
        /// C `cached_mem`: `uint32_t`.
        pub cached_mem: u32 = Map::field("cached_mem"),
        /// C `cpu_load`: `unsigned char`.
        pub cpu_load: u8 = Map::field("cpu_load"),
        /// C `cpu_temp`: `unsigned char`.
        pub cpu_temp: u8 = Map::field("cpu_temp"),
        /// C `extended_info`: `em_ext_device_info_t`.
        pub extended_info: ExtDeviceInfo = Map::field("extended_info"),
    }
}

record! {
    /// C `em_ieee_1905_security_cap_t` (`inc/em_base.h:2629`).
    pub struct Ieee1905SecurityCap = "em_ieee_1905_security_cap_t", copy {
        /// C `onboarding_proto`: `unsigned char`.
        pub onboarding_proto: u8 = Map::field("onboarding_proto"),
        /// C `integrity_algo`: `unsigned char`.
        pub integrity_algo: u8 = Map::field("integrity_algo"),
        /// C `encryption_algo`: `unsigned char`.
        pub encryption_algo: u8 = Map::field("encryption_algo"),
    }
}

record! {
    /// C `em_ieee_1905_security_info_t` (`inc/em_base.h:2635`).
    pub struct Ieee1905SecurityInfo = "em_ieee_1905_security_info_t", copy {
        /// C `id`: `mac_address_t`.
        pub id: MacAddr = Map::field("id"),
        /// C `sec_cap`: `em_ieee_1905_security_cap_t`.
        pub sec_cap: Ieee1905SecurityCap = Map::field("sec_cap"),
    }
}

impl ExtDeviceInfo {
    /// The zeroed C record (`received` false, empty credentials, codes 0).
    #[must_use]
    pub fn zeroed() -> Self {
        Self {
            received: false,
            boot_id: 0,
            client_id: Secret::default(),
            client_secret: Secret::default(),
            product_class: ProductClass(0),
            device_role: DeviceRole(0),
        }
    }
}

impl DeviceInfo {
    /// The zeroed C record (`memset(&m_device_info, 0, ...)`, `dm_easy_mesh.cpp:4301`): every
    /// number 0, every string and list empty.
    #[must_use]
    pub fn zeroed() -> Self {
        Self {
            id: DeviceId {
                net_id: Text::default(),
                dev_mac: MacAddr::ZERO,
                media: MediaType(0),
            },
            intf: crate::records::zeroed_interface(),
            profile: ProfileType(0),
            multi_ap_cap: CStrBytes::default(),
            coll_interval: 0,
            report_unsuccess_assocs: false,
            max_reporting_rate: 0,
            ap_metrics_reporting_interval: 0,
            manufacturer: CStrBytes::default(),
            serial_number: CStrBytes::default(),
            manufacturer_model: CStrBytes::default(),
            software_ver: CStrBytes::default(),
            exec_env: CStrBytes::default(),
            dscp_map: CStrBytes::default(),
            max_pri_rules: 0,
            max_vids: 0,
            country_code: CStrBytes::default(),
            prioritization_sup: false,
            report_ind_scans: false,
            traffic_sep_allowed: false,
            svc_prio_allowed: false,
            dfs_enable: false,
            max_unsuccessful_assoc_report_rate: 0,
            sta_steer_state: false,
            coord_cac_allowed: false,
            ctrl_operation_mode: CStrBytes::default(),
            backhaul_mac: crate::records::zeroed_interface(),
            backhaul_down_mac: BoundedVec::new(),
            backhaul_media_type: Ieee80211Variant(0),
            backhaul_phyrate: 0,
            backhaul_alid: crate::records::zeroed_interface(),
            backhaul_sta: MacAddr::ZERO,
            traffic_sep_cap: false,
            easy_conn_cap: false,
            test_cap: 0,
            apmld_maxlinks: 0,
            tidlink_map: 0,
            assoc_sta_reporting_int: 0,
            max_nummlds: 0,
            bstamld_maxlinks: 0,
            environment: CStrBytes::default(),
            primary_device_type: CStrBytes::default(),
            secondary_device_type: CStrBytes::default(),
            sec_1905: Ieee1905Security {
                auth_flags: 0,
                encr_flags: 0,
                conn_flags: 0,
                cfg_methods: 0,
            },
            is_emplus_agent: 0,
            uptime: 0,
            total_mem: 0,
            free_mem: 0,
            cached_mem: 0,
            cpu_load: 0,
            cpu_temp: 0,
            extended_info: ExtDeviceInfo::zeroed(),
        }
    }
}
