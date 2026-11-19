// SPDX-License-Identifier: Apache-2.0

//! Records of the `mld` group (see the module docs of `records`).

use crate::bounded::BoundedVec;
use crate::enums::HaulType;
use crate::limits::SSID_CAP;
#[cfg(test)]
use crate::mapping::Map;
use crate::text::CStrBytes;
use em_types::{Interface, MacAddr};

record! {
    /// One affiliated AP of an AP MLD.
    ///
    /// C `em_affiliated_ap_info_t` (`inc/em_base.h:2849`).
    pub struct AffiliatedApInfo = "em_affiliated_ap_info_t", owned {
        /// C `mac_addr_valid`: `bool`.
        pub mac_addr_valid: bool = Map::field("mac_addr_valid"),
        /// C `link_id_valid`: `bool`.
        pub link_id_valid: bool = Map::field("link_id_valid"),
        /// C `ruid`: `em_interface_t`.
        pub ruid: Interface = Map::field("ruid"),
        /// C `mac_addr`: `mac_address_t`.
        pub mac_addr: MacAddr = Map::field("mac_addr"),
        /// C `link_id`: `unsigned char`.
        pub link_id: u8 = Map::field("link_id"),
    }
}

record! {
    /// An AP MLD.
    ///
    /// C `em_ap_mld_info_t` (`inc/em_base.h:2857`).
    pub struct ApMldInfo = "em_ap_mld_info_t", owned {
        /// C `mac_addr_valid`: `bool`.
        pub mac_addr_valid: bool = Map::field("mac_addr_valid"),
        /// C `ssid`: `ssid_t`.
        /// Wire: the reader accepts `ssid_len` up to 32 (`:2776`) and copies at `:2803`.
        pub ssid: CStrBytes<SSID_CAP> = Map::field("ssid").cstr().wire("32 @ src/em/config/em_configuration.cpp:2776").tag("0007-A1"),
        /// C `mac_addr`: `mac_address_t`.
        pub mac_addr: MacAddr = Map::field("mac_addr"),
        /// C `str`: `bool`.
        pub str: bool = Map::field("str"),
        /// C `nstr`: `bool`.
        pub nstr: bool = Map::field("nstr"),
        /// C `emlsr`: `bool`.
        pub emlsr: bool = Map::field("emlsr"),
        /// C `emlmr`: `bool`.
        pub emlmr: bool = Map::field("emlmr"),
        /// C `haul_type`: `em_haul_type_t`.
        pub haul_type: HaulType = Map::field("haul_type"),
        /// C `affiliated_ap`: `em_affiliated_ap_info_t[EM_MAX_AP_MLD]` with count `num_affiliated_ap`.
        pub affiliated_ap: BoundedVec<AffiliatedApInfo, 64> = Map::counted("num_affiliated_ap", "affiliated_ap"),
    }
}

record! {
    /// One affiliated backhaul STA of a bSTA MLD.
    ///
    /// C `em_affiliated_bsta_info_t` (`inc/em_base.h:2870`).
    pub struct AffiliatedBstaInfo = "em_affiliated_bsta_info_t", owned {
        /// C `mac_addr_valid`: `bool`.
        pub mac_addr_valid: bool = Map::field("mac_addr_valid"),
        /// C `ruid`: `em_interface_t`.
        pub ruid: Interface = Map::field("ruid"),
        /// C `mac_addr`: `mac_address_t`.
        pub mac_addr: MacAddr = Map::field("mac_addr"),
    }
}

record! {
    /// A backhaul STA MLD.
    ///
    /// C `em_bsta_mld_info_t` (`inc/em_base.h:2876`).
    pub struct BstaMldInfo = "em_bsta_mld_info_t", owned {
        /// C `mac_addr_valid`: `bool`.
        pub mac_addr_valid: bool = Map::field("mac_addr_valid"),
        /// C `ap_mld_mac_addr_valid`: `bool`.
        pub ap_mld_mac_addr_valid: bool = Map::field("ap_mld_mac_addr_valid"),
        /// C `mac_addr`: `mac_address_t`.
        pub mac_addr: MacAddr = Map::field("mac_addr"),
        /// C `ap_mld_mac_addr`: `mac_address_t`.
        pub ap_mld_mac_addr: MacAddr = Map::field("ap_mld_mac_addr"),
        /// C `str`: `bool`.
        pub str: bool = Map::field("str"),
        /// C `nstr`: `bool`.
        pub nstr: bool = Map::field("nstr"),
        /// C `emlsr`: `bool`.
        pub emlsr: bool = Map::field("emlsr"),
        /// C `emlmr`: `bool`.
        pub emlmr: bool = Map::field("emlmr"),
        /// C `affiliated_bsta`: `em_affiliated_bsta_info_t[EM_MAX_AP_MLD]` with count `num_affiliated_bsta`.
        pub affiliated_bsta: BoundedVec<AffiliatedBstaInfo, 64> = Map::counted("num_affiliated_bsta", "affiliated_bsta"),
    }
}

record! {
    /// One affiliated STA of an associated STA MLD.
    ///
    /// C `em_affiliated_sta_info_t` (`inc/em_base.h:2889`).
    pub struct AffiliatedStaInfo = "em_affiliated_sta_info_t", copy {
        /// C `bssid`: `mac_address_t`.
        pub bssid: MacAddr = Map::field("bssid"),
        /// C `link_addr`: `mac_address_t`.
        pub link_addr: MacAddr = Map::field("link_addr"),
    }
}

record! {
    /// An associated STA MLD.
    ///
    /// C `em_assoc_sta_mld_info_t` (`inc/em_base.h:2894`).
    pub struct AssocStaMldInfo = "em_assoc_sta_mld_info_t", owned {
        /// C `mac_addr`: `mac_address_t`.
        pub mac_addr: MacAddr = Map::field("mac_addr"),
        /// C `ap_mld_mac_addr`: `mac_address_t`.
        pub ap_mld_mac_addr: MacAddr = Map::field("ap_mld_mac_addr"),
        /// C `str`: `bool`.
        pub str: bool = Map::field("str"),
        /// C `nstr`: `bool`.
        pub nstr: bool = Map::field("nstr"),
        /// C `emlsr`: `bool`.
        pub emlsr: bool = Map::field("emlsr"),
        /// C `emlmr`: `bool`.
        pub emlmr: bool = Map::field("emlmr"),
        /// C `affiliated_sta`: `em_affiliated_sta_info_t[EM_MAX_AP_MLD]` with count `num_affiliated_sta`.
        pub affiliated_sta: BoundedVec<AffiliatedStaInfo, 64> = Map::counted("num_affiliated_sta", "affiliated_sta"),
    }
}

record! {
    /// One TID-to-link mapping.
    ///
    /// C `em_tid_to_link_map_info_t` (`inc/em_base.h:2905`).
    pub struct TidToLinkMapInfo = "em_tid_to_link_map_info_t", copy {
        /// C `add_remove`: `bool`.
        pub add_remove: bool = Map::field("add_remove"),
        /// C `sta_mld_mac_addr`: `mac_address_t`.
        pub sta_mld_mac_addr: MacAddr = Map::field("sta_mld_mac_addr"),
        /// C `direction`: `bool`.
        pub direction: bool = Map::field("direction"),
        /// C `default_link_map`: `bool`.
        pub default_link_map: bool = Map::field("default_link_map"),
        /// C `map_switch_time_present`: `bool`.
        pub map_switch_time_present: bool = Map::field("map_switch_time_present"),
        /// C `expected_dur_present`: `bool`.
        pub expected_dur_present: bool = Map::field("expected_dur_present"),
        /// C `link_map_size`: `bool`.
        pub link_map_size: bool = Map::field("link_map_size"),
        /// C `link_map_presence_ind`: `unsigned char`.
        pub link_map_presence_ind: u8 = Map::field("link_map_presence_ind"),
        /// C `expected_dur`: `unsigned char[3]`.
        pub expected_dur: [u8; 3] = Map::field("expected_dur"),
        /// C `tid_to_link_map`: `unsigned char`.
        pub tid_to_link_map: u8 = Map::field("tid_to_link_map"),
    }
}

record! {
    /// TID-to-link mapping configuration of an MLD.
    ///
    /// C `em_tid_to_link_info_t` (`inc/em_base.h:2918`).
    pub struct TidToLinkInfo = "em_tid_to_link_info_t", owned {
        /// C `is_bsta_config`: `bool`.
        pub is_bsta_config: bool = Map::field("is_bsta_config"),
        /// C `mld_mac_addr`: `mac_address_t`.
        pub mld_mac_addr: MacAddr = Map::field("mld_mac_addr"),
        /// C `tid_to_link_map_neg`: `bool`.
        pub tid_to_link_map_neg: bool = Map::field("tid_to_link_map_neg"),
        /// C `tid_to_link_mapping`: `em_tid_to_link_map_info_t[EM_MAX_AP_MLD]` with count `num_mapping`.
        pub tid_to_link_mapping: BoundedVec<TidToLinkMapInfo, 64> = Map::counted("num_mapping", "tid_to_link_mapping"),
    }
}

record! {
    /// Multi-link operation policy.
    ///
    /// C `em_mlo_policy_t` (`inc/em_base.h:516`).
    pub struct MloPolicy = "em_mlo_policy_t", copy {
        /// C `use_mld_tlv`: `bool`.
        pub use_mld_tlv: bool = Map::field("use_mld_tlv"),
        /// C `band_bitmap`: `uint8_t`.
        pub band_bitmap: u8 = Map::field("band_bitmap"),
    }
}
