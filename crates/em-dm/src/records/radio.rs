// SPDX-License-Identifier: Apache-2.0

//! Records of the `radio` group (see the module docs of `records`).

use crate::bounded::BoundedVec;
use crate::enums::OpClassType;
use crate::limits::LONG_STRING_CAP;
#[cfg(test)]
use crate::mapping::{Map, NO_WIRE};
use crate::text::{CStrBytes, Text};
use em_types::{FreqBand, Interface, MacAddr};

record! {
    /// C `em_radio_id_t` (`inc/em_base.h:2926`).
    pub struct RadioId = "em_radio_id_t", copy {
        /// C `net_id`: `em_long_string_t`.
        pub net_id: Text<LONG_STRING_CAP> = Map::field("net_id").text("key text parsed with strncpy from the database key string (src/dm/dm_radio.cpp:260)").wire(NO_WIRE),
        /// C `dev_mac`: `mac_address_t`.
        pub dev_mac: MacAddr = Map::field("dev_mac"),
        /// C `ruid`: `mac_address_t`.
        pub ruid: MacAddr = Map::field("ruid"),
    }
}

record! {
    /// C `em_media_spec_data_t` (`inc/em_base.h:1561`).
    pub struct MediaSpecData = "em_media_spec_data_t", copy {
        /// C `media_type`: `unsigned short`.
        pub media_type: u16 = Map::field("media_type"),
        /// C `media_spec_size`: `unsigned char`.
        pub media_spec_size: u8 = Map::field("media_spec_size"),
        /// C `network_memb`: `mac_address_t`.
        pub network_memb: MacAddr = Map::field("network_memb"),
        /// C `role`: `unsigned char`.
        pub role: u8 = Map::field("role"),
        /// C `band`: `unsigned char`.
        pub band: u8 = Map::field("band"),
        /// C `center_freq_index_1`: `unsigned char`.
        pub center_freq_index_1: u8 = Map::field("center_freq_index_1"),
        /// C `center_freq_index_2`: `unsigned char`.
        pub center_freq_index_2: u8 = Map::field("center_freq_index_2"),
    }
}

record! {
    /// A radio. Large: store it as `Box<RadioInfo>` in collections (inline size in the crate
    /// README).
    ///
    /// C `em_radio_info_t` (`inc/em_base.h:2932`).
    pub struct RadioInfo = "em_radio_info_t", owned {
        /// C `id`: `em_radio_id_t`.
        pub id: RadioId = Map::field("id"),
        /// C `intf`: `em_interface_t`.
        pub intf: Interface = Map::field("intf"),
        /// C `enabled`: `bool`.
        pub enabled: bool = Map::field("enabled"),
        /// C `band`: `em_freq_band_t`.
        pub band: FreqBand = Map::field("band"),
        /// C `media_data`: `em_media_spec_data_t`.
        pub media_data: MediaSpecData = Map::field("media_data"),
        /// C `number_of_bss`: `unsigned int`.
        pub number_of_bss: u32 = Map::field("number_of_bss"),
        /// C `number_of_unassoc_sta`: `unsigned int`.
        pub number_of_unassoc_sta: u32 = Map::field("number_of_unassoc_sta"),
        /// C `noise`: `int`.
        pub noise: i32 = Map::field("noise"),
        /// C `transmit`: `unsigned char`.
        pub transmit: u8 = Map::field("transmit"),
        /// C `receive_self`: `unsigned char`.
        pub receive_self: u8 = Map::field("receive_self"),
        /// C `receive_other`: `unsigned char`.
        pub receive_other: u8 = Map::field("receive_other"),
        /// C `utilization`: `unsigned short`.
        pub utilization: u16 = Map::field("utilization"),
        /// C `traffic_sep_combined_fronthaul`: `bool`.
        pub traffic_sep_combined_fronthaul: bool = Map::field("traffic_sep_combined_fronthaul"),
        /// C `traffic_sep_combined_backhaul`: `bool`.
        pub traffic_sep_combined_backhaul: bool = Map::field("traffic_sep_combined_backhaul"),
        /// C `steering_policy`: `unsigned int`.
        pub steering_policy: u32 = Map::field("steering_policy"),
        /// C `channel_util_threshold`: `unsigned int`.
        pub channel_util_threshold: u32 = Map::field("channel_util_threshold"),
        /// C `rcpi_steering_threshold`: `unsigned int`.
        pub rcpi_steering_threshold: u32 = Map::field("rcpi_steering_threshold"),
        /// C `sta_reporting_rcpi_threshold`: `unsigned int`.
        pub sta_reporting_rcpi_threshold: u32 = Map::field("sta_reporting_rcpi_threshold"),
        /// C `sta_reporting_hysteresis_margin_override`: `unsigned int`.
        pub sta_reporting_hysteresis_margin_override: u32 = Map::field("sta_reporting_hysteresis_margin_override"),
        /// C `channel_utilization_reporting_threshold`: `unsigned int`.
        pub channel_utilization_reporting_threshold: u32 = Map::field("channel_utilization_reporting_threshold"),
        /// C `associated_sta_traffic_stats_inclusion_policy`: `bool`.
        pub associated_sta_traffic_stats_inclusion_policy: bool = Map::field("associated_sta_traffic_stats_inclusion_policy"),
        /// C `associated_sta_link_mterics_inclusion_policy`: `bool`.
        pub associated_sta_link_mterics_inclusion_policy: bool = Map::field("associated_sta_link_mterics_inclusion_policy"),
        /// C `unassociated_sta_link_mterics_opclass_inclusion_policy`: `bool`.
        pub unassociated_sta_link_mterics_opclass_inclusion_policy: bool = Map::field("unassociated_sta_link_mterics_opclass_inclusion_policy"),
        /// C `unassociated_sta_link_mterics_nonopclass_inclusion_policy`: `bool`.
        pub unassociated_sta_link_mterics_nonopclass_inclusion_policy: bool = Map::field("unassociated_sta_link_mterics_nonopclass_inclusion_policy"),
        /// C `support_rcpi_steering`: `bool`.
        pub support_rcpi_steering: bool = Map::field("support_rcpi_steering"),
        /// C `chip_vendor`: `em_long_string_t`.
        pub chip_vendor: CStrBytes<LONG_STRING_CAP> = Map::field("chip_vendor").cstr().wire(NO_WIRE),
        /// C `ap_metrics_wifi6`: `bool`.
        pub ap_metrics_wifi6: bool = Map::field("ap_metrics_wifi6"),
        /// C `transmit_power_limit`: `int`.
        pub transmit_power_limit: i32 = Map::field("transmit_power_limit"),
        /// C `partial_bss_color`: `unsigned char`.
        pub partial_bss_color: u8 = Map::field("partial_bss_color"),
        /// C `bss_color`: `unsigned char`.
        pub bss_color: u8 = Map::field("bss_color"),
        /// C `hesiga_spatial_reuse_value15_allowed`: `bool`.
        pub hesiga_spatial_reuse_value15_allowed: bool = Map::field("hesiga_spatial_reuse_value15_allowed"),
        /// C `srg_information_valid`: `bool`.
        pub srg_information_valid: bool = Map::field("srg_information_valid"),
        /// C `non_srg_offset_valid`: `bool`.
        pub non_srg_offset_valid: bool = Map::field("non_srg_offset_valid"),
        /// C `psr_disallowed`: `bool`.
        pub psr_disallowed: bool = Map::field("psr_disallowed"),
        /// C `init_cfg_done`: `bool`.
        pub init_cfg_done: bool = Map::field("init_cfg_done"),
        /// C `non_srg_obsspd_max_offset`: `unsigned char`.
        pub non_srg_obsspd_max_offset: u8 = Map::field("non_srg_obsspd_max_offset"),
        /// C `srg_obsspd_min_offset`: `unsigned char`.
        pub srg_obsspd_min_offset: u8 = Map::field("srg_obsspd_min_offset"),
        /// C `srg_obsspd_max_offset`: `unsigned char`.
        pub srg_obsspd_max_offset: u8 = Map::field("srg_obsspd_max_offset"),
        /// C `srg_bss_color_bitmap`: `unsigned char[8]`.
        pub srg_bss_color_bitmap: [u8; 8] = Map::field("srg_bss_color_bitmap"),
        /// C `srg_partial_bssid_bitmap`: `unsigned char[8]`.
        pub srg_partial_bssid_bitmap: [u8; 8] = Map::field("srg_partial_bssid_bitmap"),
        /// C `neigh_bss_color_in_use_bitmap`: `unsigned char[8]`.
        pub neigh_bss_color_in_use_bitmap: [u8; 8] = Map::field("neigh_bss_color_in_use_bitmap"),
        /// C `radio_temp`: `unsigned char`.
        pub radio_temp: u8 = Map::field("radio_temp"),
    }
}

record! {
    /// C `em_metric_cltn_interval_t` (`inc/em_base.h:2105`).
    pub struct MetricCltnInterval = "em_metric_cltn_interval_t", copy {
        /// C `metric_cltn_interval`: `unsigned int`.
        pub metric_cltn_interval: u32 = Map::field("metric_cltn_interval"),
    }
}

record! {
    /// C `em_op_class_id_t` (`inc/em_base.h:2678`).
    pub struct OpClassId = "em_op_class_id_t", copy {
        /// C `ruid`: `mac_address_t`.
        pub ruid: MacAddr = Map::field("ruid"),
        /// C `type`: `em_op_class_type_t` (renamed: `type` is a Rust keyword).
        pub kind: OpClassType = Map::field("type"),
        /// C `op_class`: `unsigned int`.
        pub op_class: u32 = Map::field("op_class"),
    }
}

record! {
    /// C `em_op_class_info_t` (`inc/em_base.h:2684`).
    pub struct OpClassInfo = "em_op_class_info_t", owned {
        /// C `id`: `em_op_class_id_t`.
        pub id: OpClassId = Map::field("id"),
        /// C `op_class`: `unsigned int`.
        pub op_class: u32 = Map::field("op_class"),
        /// C `channel`: `unsigned int`.
        pub channel: u32 = Map::field("channel"),
        /// C `tx_power`: `int`.
        pub tx_power: i32 = Map::field("tx_power"),
        /// C `max_tx_power`: `int`.
        pub max_tx_power: i32 = Map::field("max_tx_power"),
        /// C `channels`: `unsigned int` with count `num_channels`.
        pub channels: BoundedVec<u32, 64> = Map::counted("num_channels", "channels"),
        /// C `channel_pref`: `unsigned char[EM_MAX_CHANNELS_IN_LIST]`.
        pub channel_pref: [u8; 64] = Map::field("channel_pref"),
        /// C `pref_valid`: `bool`.
        pub pref_valid: bool = Map::field("pref_valid"),
        /// C `mins_since_cac_comp`: `unsigned short`.
        pub mins_since_cac_comp: u16 = Map::field("mins_since_cac_comp"),
        /// C `sec_remain_non_occ_dur`: `unsigned short`.
        pub sec_remain_non_occ_dur: u16 = Map::field("sec_remain_non_occ_dur"),
        /// C `countdown_cac_comp`: `unsigned int`.
        pub countdown_cac_comp: u32 = Map::field("countdown_cac_comp"),
    }
}

record! {
    /// C `em_cac_comp_rprt_pair_t` (`inc/em_base.h:1508`).
    pub struct CacCompRprtPair = "em_cac_comp_rprt_pair_t", copy {
        /// C `op_class`: `unsigned char`.
        pub op_class: u8 = Map::field("op_class"),
        /// C `channel`: `unsigned char`.
        pub channel: u8 = Map::field("channel"),
    }
}

record! {
    /// C `em_cac_comp_info_t` (`inc/em_base.h:2699`).
    pub struct CacCompInfo = "em_cac_comp_info_t", owned {
        /// C `ruid`: `mac_address_t`.
        pub ruid: MacAddr = Map::field("ruid"),
        /// C `op_class`: `unsigned char`.
        pub op_class: u8 = Map::field("op_class"),
        /// C `channel`: `unsigned char`.
        pub channel: u8 = Map::field("channel"),
        /// C `status`: `unsigned char`.
        pub status: u8 = Map::field("status"),
        /// C `detected_pairs`: `em_cac_comp_rprt_pair_t` with count `detected_pairs_num`.
        pub detected_pairs: BoundedVec<CacCompRprtPair, 4> = Map::counted("detected_pairs_num", "detected_pairs"),
    }
}
