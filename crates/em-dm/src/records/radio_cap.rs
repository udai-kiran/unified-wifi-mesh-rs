// SPDX-License-Identifier: Apache-2.0

//! Records of the `radio_cap` group (see the module docs of `records`).

use crate::bits::UBits;
use crate::bounded::BoundedVec;
use crate::enums::Ieee80211Variant;
#[cfg(test)]
use crate::mapping::Map;
use crate::records::MetricCltnInterval;
use em_types::{Interface, MacAddr};

record! {
    /// C `em_ap_ht_cap_t` (`inc/em_base.h:1881`).
    pub struct ApHtCap = "em_ap_ht_cap_t", copy {
        /// C `ruid`: `mac_address_t`.
        pub ruid: MacAddr = Map::field("ruid"),
        /// C `reserved`: `unsigned char`.
        pub reserved: bool = Map::field("reserved"),
        /// C `ht_sprt_40mhz`: `unsigned char`.
        pub ht_sprt_40mhz: bool = Map::field("ht_sprt_40mhz"),
        /// C `gi_sprt_40mhz`: `unsigned char`.
        pub gi_sprt_40mhz: bool = Map::field("gi_sprt_40mhz"),
        /// C `gi_sprt_20mhz`: `unsigned char`.
        pub gi_sprt_20mhz: bool = Map::field("gi_sprt_20mhz"),
        /// C `max_sprt_rx_streams`: `unsigned char`.
        pub max_sprt_rx_streams: UBits<2> = Map::field("max_sprt_rx_streams"),
        /// C `max_sprt_tx_streams`: `unsigned char`.
        pub max_sprt_tx_streams: UBits<2> = Map::field("max_sprt_tx_streams"),
    }
}

record! {
    /// C `em_ap_vht_cap_t` (`inc/em_base.h:1891`).
    pub struct ApVhtCap = "em_ap_vht_cap_t", copy {
        /// C `ruid`: `mac_address_t`.
        pub ruid: MacAddr = Map::field("ruid"),
        /// C `sprt_tx_mcs`: `unsigned short`.
        pub sprt_tx_mcs: u16 = Map::field("sprt_tx_mcs"),
        /// C `sprt_rx_mcs`: `unsigned short`.
        pub sprt_rx_mcs: u16 = Map::field("sprt_rx_mcs"),
        /// C `gi_sprt_160mhz`: `unsigned char`.
        pub gi_sprt_160mhz: bool = Map::field("gi_sprt_160mhz"),
        /// C `gi_sprt_80mhz`: `unsigned char`.
        pub gi_sprt_80mhz: bool = Map::field("gi_sprt_80mhz"),
        /// C `max_sprt_rx_streams`: `unsigned char`.
        pub max_sprt_rx_streams: UBits<3> = Map::field("max_sprt_rx_streams"),
        /// C `max_sprt_tx_streams`: `unsigned char`.
        pub max_sprt_tx_streams: UBits<3> = Map::field("max_sprt_tx_streams"),
        /// C `reserved`: `unsigned char`.
        pub reserved: UBits<4> = Map::field("reserved"),
        /// C `mu_beamformer_cap`: `unsigned char`.
        pub mu_beamformer_cap: bool = Map::field("mu_beamformer_cap"),
        /// C `su_beamformer_cap`: `unsigned char`.
        pub su_beamformer_cap: bool = Map::field("su_beamformer_cap"),
        /// C `sprt_160mhz`: `unsigned char`.
        pub sprt_160mhz: bool = Map::field("sprt_160mhz"),
        /// C `sprt_80_80_mhz`: `unsigned char`.
        pub sprt_80_80_mhz: bool = Map::field("sprt_80_80_mhz"),
    }
}

record! {
    /// C `em_ap_he_mcs_maps_t` (`inc/em_base.h:1906`).
    pub struct ApHeMcsMaps = "em_ap_he_mcs_maps_t", copy {
        /// C `tx_he_mcs`: `unsigned short`.
        pub tx_he_mcs: u16 = Map::field("tx_he_mcs"),
        /// C `rx_he_mcs`: `unsigned short`.
        pub rx_he_mcs: u16 = Map::field("rx_he_mcs"),
    }
}

record! {
    /// C `em_ap_he_cap_t` (`inc/em_base.h:1911`).
    pub struct ApHeCap = "em_ap_he_cap_t", copy {
        /// C `ruid`: `mac_address_t`.
        pub ruid: MacAddr = Map::field("ruid"),
        /// C `sprt_mcs_len`: `unsigned char`.
        pub sprt_mcs_len: u8 = Map::field("sprt_mcs_len"),
        /// C `sprt_tx_rx_mcs`: `em_ap_he_mcs_maps_t[MAX_MCS]`.
        pub sprt_tx_rx_mcs: [ApHeMcsMaps; 3] = Map::field("sprt_tx_rx_mcs"),
        /// C `sprt_160mhz`: `unsigned char`.
        pub sprt_160mhz: bool = Map::field("sprt_160mhz"),
        /// C `sprt_80_80_mhz`: `unsigned char`.
        pub sprt_80_80_mhz: bool = Map::field("sprt_80_80_mhz"),
        /// C `max_sprt_rx_streams`: `unsigned char`.
        pub max_sprt_rx_streams: UBits<3> = Map::field("max_sprt_rx_streams"),
        /// C `max_sprt_tx_streams`: `unsigned char`.
        pub max_sprt_tx_streams: UBits<3> = Map::field("max_sprt_tx_streams"),
        /// C `reserved`: `unsigned char`.
        pub reserved: bool = Map::field("reserved"),
        /// C `dl_ofdma_cap`: `unsigned char`.
        pub dl_ofdma_cap: bool = Map::field("dl_ofdma_cap"),
        /// C `ul_ofdma_cap`: `unsigned char`.
        pub ul_ofdma_cap: bool = Map::field("ul_ofdma_cap"),
        /// C `dl_mimo_ofdma_cap`: `unsigned char`.
        pub dl_mimo_ofdma_cap: bool = Map::field("dl_mimo_ofdma_cap"),
        /// C `ul_mimo_ofdma_cap`: `unsigned char`.
        pub ul_mimo_ofdma_cap: bool = Map::field("ul_mimo_ofdma_cap"),
        /// C `ul_mimo_cap`: `unsigned char`.
        pub ul_mimo_cap: bool = Map::field("ul_mimo_cap"),
        /// C `mu_beamformer_cap`: `unsigned char`.
        pub mu_beamformer_cap: bool = Map::field("mu_beamformer_cap"),
        /// C `su_beamformer_cap`: `unsigned char`.
        pub su_beamformer_cap: bool = Map::field("su_beamformer_cap"),
    }
}

record! {
    /// C `em_wifi6_cap_role_head_tlv_t` (`inc/em_base.h:1929`).
    pub struct Wifi6CapRoleHead = "em_wifi6_cap_role_head_tlv_t", copy {
        /// C `mcs_nss_num`: `unsigned char`.
        pub mcs_nss_num: UBits<4> = Map::field("mcs_nss_num"),
        /// C `he_8080`: `unsigned char`.
        pub he_8080: bool = Map::field("he_8080"),
        /// C `he_160`: `unsigned char`.
        pub he_160: bool = Map::field("he_160"),
        /// C `agent_role`: `unsigned char`.
        pub agent_role: UBits<2> = Map::field("agent_role"),
    }
}

record! {
    /// C `em_wifi6_cap_role_tail_tlv_t` (`inc/em_base.h:1936`).
    pub struct Wifi6CapRoleTail = "em_wifi6_cap_role_tail_tlv_t", copy {
        /// C `dl_ofdma`: `unsigned char`.
        pub dl_ofdma: bool = Map::field("dl_ofdma"),
        /// C `ul_ofdma`: `unsigned char`.
        pub ul_ofdma: bool = Map::field("ul_ofdma"),
        /// C `ul_mumimo`: `unsigned char`.
        pub ul_mumimo: bool = Map::field("ul_mumimo"),
        /// C `beam_formee_sts_g80`: `unsigned char`.
        pub beam_formee_sts_g80: bool = Map::field("beam_formee_sts_g80"),
        /// C `beam_formee_sts_l80`: `unsigned char`.
        pub beam_formee_sts_l80: bool = Map::field("beam_formee_sts_l80"),
        /// C `mu_beam_former`: `unsigned char`.
        pub mu_beam_former: bool = Map::field("mu_beam_former"),
        /// C `su_beam_formee`: `unsigned char`.
        pub su_beam_formee: bool = Map::field("su_beam_formee"),
        /// C `su_beam_former`: `unsigned char`.
        pub su_beam_former: bool = Map::field("su_beam_former"),
        /// C `max_ul_mumimo_rx`: `unsigned char`.
        pub max_ul_mumimo_rx: UBits<4> = Map::field("max_ul_mumimo_rx"),
        /// C `max_dl_mumimo_tx`: `unsigned char`.
        pub max_dl_mumimo_tx: UBits<4> = Map::field("max_dl_mumimo_tx"),
        /// C `max_dl_ofdma_tx`: `unsigned char`.
        pub max_dl_ofdma_tx: u8 = Map::field("max_dl_ofdma_tx"),
        /// C `max_ul_ofdma_rx`: `unsigned char`.
        pub max_ul_ofdma_rx: u8 = Map::field("max_ul_ofdma_rx"),
        /// C `anticipated_channel_usage`: `unsigned char`.
        pub anticipated_channel_usage: bool = Map::field("anticipated_channel_usage"),
        /// C `spatial_reuse`: `unsigned char`.
        pub spatial_reuse: bool = Map::field("spatial_reuse"),
        /// C `twt_resp`: `unsigned char`.
        pub twt_resp: bool = Map::field("twt_resp"),
        /// C `twt_req`: `unsigned char`.
        pub twt_req: bool = Map::field("twt_req"),
        /// C `mu_edca`: `unsigned char`.
        pub mu_edca: bool = Map::field("mu_edca"),
        /// C `multi_bssid`: `unsigned char`.
        pub multi_bssid: bool = Map::field("multi_bssid"),
        /// C `mu_rts`: `unsigned char`.
        pub mu_rts: bool = Map::field("mu_rts"),
        /// C `rts`: `unsigned char`.
        pub rts: bool = Map::field("rts"),
    }
}

record! {
    /// C `em_wifi6_role_wire_t` (`inc/em_base.h:1959`).
    pub struct Wifi6RoleWire = "em_wifi6_role_wire_t", copy {
        /// C `role_head`: `em_wifi6_cap_role_head_tlv_t`.
        pub role_head: Wifi6CapRoleHead = Map::field("role_head"),
        /// C `sprt_tx_rx_mcs`: `em_ap_he_mcs_maps_t[MAX_MCS]`.
        pub sprt_tx_rx_mcs: [ApHeMcsMaps; 3] = Map::field("sprt_tx_rx_mcs"),
        /// C `role_tail`: `em_wifi6_cap_role_tail_tlv_t`.
        pub role_tail: Wifi6CapRoleTail = Map::field("role_tail"),
    }
}

record! {
    /// C `em_radio_wifi6_cap_data_t` (`inc/em_base.h:1965`).
    pub struct RadioWifi6CapData = "em_radio_wifi6_cap_data_t", owned {
        /// C `roles`: `em_wifi6_role_wire_t` with count `num_role`.
        pub roles: BoundedVec<Wifi6RoleWire, 2> = Map::counted("num_role", "roles"),
    }
}

record! {
    /// C `em_wifi7_mlo_cap_support_tlv_t` (`inc/em_base.h:1990`).
    pub struct Wifi7MloCapSupport = "em_wifi7_mlo_cap_support_tlv_t", copy {
        /// C `ruid`: `mac_address_t`.
        pub ruid: MacAddr = Map::field("ruid"),
        /// C `reserved3`: `unsigned char[24]`.
        pub reserved3: [u8; 24] = Map::field("reserved3"),
        /// C `reserved4`: `unsigned char`.
        pub reserved4: UBits<4> = Map::field("reserved4"),
        /// C `ap_emlmr_support`: `unsigned char`.
        pub ap_emlmr_support: bool = Map::field("ap_emlmr_support"),
        /// C `ap_emlsr_support`: `unsigned char`.
        pub ap_emlsr_support: bool = Map::field("ap_emlsr_support"),
        /// C `ap_nstr_support`: `unsigned char`.
        pub ap_nstr_support: bool = Map::field("ap_nstr_support"),
        /// C `ap_str_support`: `unsigned char`.
        pub ap_str_support: bool = Map::field("ap_str_support"),
        /// C `reserved5`: `unsigned char`.
        pub reserved5: UBits<4> = Map::field("reserved5"),
        /// C `bsta_emlmr_support`: `unsigned char`.
        pub bsta_emlmr_support: bool = Map::field("bsta_emlmr_support"),
        /// C `bsta_emlsr_support`: `unsigned char`.
        pub bsta_emlsr_support: bool = Map::field("bsta_emlsr_support"),
        /// C `bsta_nstr_support`: `unsigned char`.
        pub bsta_nstr_support: bool = Map::field("bsta_nstr_support"),
        /// C `bsta_str_support`: `unsigned char`.
        pub bsta_str_support: bool = Map::field("bsta_str_support"),
    }
}

record! {
    /// C `em_wifi7_freq_record_tlv_t` (`inc/em_base.h:1970`).
    pub struct Wifi7FreqRecord = "em_wifi7_freq_record_tlv_t", copy {
        /// C `ruid`: `mac_address_t`.
        pub ruid: MacAddr = Map::field("ruid"),
        /// C `reserved`: `unsigned char`.
        pub reserved: UBits<3> = Map::field("reserved"),
        /// C `freq_sep`: `unsigned char`.
        pub freq_sep: UBits<5> = Map::field("freq_sep"),
    }
}

record! {
    /// C `em_wifi7_freq_records_t` (`inc/em_base.h:1976`).
    pub struct Wifi7FreqRecords = "em_wifi7_freq_records_t", owned {
        /// C `records`: `em_wifi7_freq_record_tlv_t` with count `num_records`.
        pub records: BoundedVec<Wifi7FreqRecord, 8> = Map::counted("num_records", "records"),
    }
}

record! {
    /// C `em_wifi7_mlo_cap_records_t` (`inc/em_base.h:2005`).
    pub struct Wifi7MloCapRecords = "em_wifi7_mlo_cap_records_t", owned {
        /// C `ap_str`: `em_wifi7_freq_records_t`.
        pub ap_str: Wifi7FreqRecords = Map::field("ap_str"),
        /// C `ap_nstr`: `em_wifi7_freq_records_t`.
        pub ap_nstr: Wifi7FreqRecords = Map::field("ap_nstr"),
        /// C `ap_emlsr`: `em_wifi7_freq_records_t`.
        pub ap_emlsr: Wifi7FreqRecords = Map::field("ap_emlsr"),
        /// C `ap_emlmr`: `em_wifi7_freq_records_t`.
        pub ap_emlmr: Wifi7FreqRecords = Map::field("ap_emlmr"),
        /// C `bsta_str`: `em_wifi7_freq_records_t`.
        pub bsta_str: Wifi7FreqRecords = Map::field("bsta_str"),
        /// C `bsta_nstr`: `em_wifi7_freq_records_t`.
        pub bsta_nstr: Wifi7FreqRecords = Map::field("bsta_nstr"),
        /// C `bsta_emlsr`: `em_wifi7_freq_records_t`.
        pub bsta_emlsr: Wifi7FreqRecords = Map::field("bsta_emlsr"),
        /// C `bsta_emlmr`: `em_wifi7_freq_records_t`.
        pub bsta_emlmr: Wifi7FreqRecords = Map::field("bsta_emlmr"),
    }
}

record! {
    /// C `em_wifi7_agent_cap_t` (`inc/em_base.h:2016`).
    pub struct Wifi7AgentCap = "em_wifi7_agent_cap_t", owned {
        /// C `mlo_cap_support`: `em_wifi7_mlo_cap_support_tlv_t`.
        pub mlo_cap_support: Wifi7MloCapSupport = Map::field("mlo_cap_support"),
        /// C `mlo_cap_records`: `em_wifi7_mlo_cap_records_t`.
        pub mlo_cap_records: Wifi7MloCapRecords = Map::field("mlo_cap_records"),
    }
}

record! {
    /// C `em_eht_operations_bss_t` (`inc/em_base.h:2027`).
    pub struct EhtOperationsBss = "em_eht_operations_bss_t", copy {
        /// C `bssid`: `mac_address_t`.
        pub bssid: MacAddr = Map::field("bssid"),
        /// C `reserved1`: `unsigned char`.
        pub reserved1: UBits<2> = Map::field("reserved1"),
        /// C `group_addr_bu_ind_exp`: `unsigned char`.
        pub group_addr_bu_ind_exp: UBits<2> = Map::field("group_addr_bu_ind_exp"),
        /// C `group_addr_bu_ind_limit`: `unsigned char`.
        pub group_addr_bu_ind_limit: bool = Map::field("group_addr_bu_ind_limit"),
        /// C `default_pe_duration`: `unsigned char`.
        pub default_pe_duration: bool = Map::field("default_pe_duration"),
        /// C `disabled_subchannel_valid`: `unsigned char`.
        pub disabled_subchannel_valid: bool = Map::field("disabled_subchannel_valid"),
        /// C `op_info_valid`: `unsigned char`.
        pub op_info_valid: bool = Map::field("op_info_valid"),
        /// C `eht_msc_nss_set`: `unsigned char[4]`.
        pub eht_msc_nss_set: [u8; 4] = Map::field("eht_msc_nss_set"),
        /// C `control`: `unsigned char`.
        pub control: u8 = Map::field("control"),
        /// C `ccfs0`: `unsigned char`.
        pub ccfs0: u8 = Map::field("ccfs0"),
        /// C `ccfs1`: `unsigned char`.
        pub ccfs1: u8 = Map::field("ccfs1"),
        /// C `disabled_subchannel_bitmap`: `unsigned char[2]`.
        pub disabled_subchannel_bitmap: [u8; 2] = Map::field("disabled_subchannel_bitmap"),
        /// C `reserved2`: `unsigned char[16]`.
        pub reserved2: [u8; 16] = Map::field("reserved2"),
    }
}

record! {
    /// C `em_eht_operations_radio_t` (`inc/em_base.h:2043`).
    pub struct EhtOperationsRadio = "em_eht_operations_radio_t", owned {
        /// C `ruid`: `mac_address_t`.
        pub ruid: MacAddr = Map::field("ruid"),
        /// C `bss`: `em_eht_operations_bss_t` with count `bss_num`.
        pub bss: BoundedVec<EhtOperationsBss, 16> = Map::counted("bss_num", "bss"),
        /// C `reserved`: `unsigned char[25]`.
        pub reserved: [u8; 25] = Map::field("reserved"),
    }
}

record! {
    /// C `em_eht_operations_t` (`inc/em_base.h:2050`).
    pub struct EhtOperations = "em_eht_operations_t", owned {
        /// C `radios`: `em_eht_operations_radio_t` with count `radios_num`.
        pub radios: BoundedVec<EhtOperationsRadio, 4> = Map::counted("radios_num", "radios"),
    }
}

record! {
    /// Channels of one scan-capable operating class. Not `Copy`: it holds a `Vec`.
    ///
    /// C `em_scan_cap_op_class_info_t` (`inc/em_base.h:536`), with its member
    /// `em_per_op_class_channels_list_t` (`inc/em_base.h:525`, a struct whose only field is
    /// `channel[EM_MAX_CHANNELS_IN_LIST]`) and the count `num` folded into one bounded list.
    /// Upstream clamps `num` to the array size when it reads the TLV
    /// (`src/em/capability/em_capability.cpp:1448-1453`) and loops over `num` when it writes
    /// (`src/em/em.cpp:1757-1761`), so `num > 64` has no meaning and cannot be represented here.
    pub struct ScanCapOpClassInfo = "em_scan_cap_op_class_info_t", owned {
        /// C `op_class`: `unsigned char`.
        pub op_class: u8 = Map::field("op_class"),
        /// C `channels.channel` with count `num`: `unsigned char[EM_MAX_CHANNELS_IN_LIST]`.
        pub channels: BoundedVec<u8, 64> = Map::counted("num", "channels").cross_fold("em_per_op_class_channels_list_t", "channel"),
    }
}

record! {
    /// C `em_channel_scan_cap_radio_t` (`inc/em_base.h:2063`).
    pub struct ChannelScanCapRadio = "em_channel_scan_cap_radio_t", owned {
        /// C `ruid`: `mac_address_t`.
        pub ruid: MacAddr = Map::field("ruid"),
        /// C `reserved`: `unsigned char`.
        pub reserved: UBits<5> = Map::field("reserved"),
        /// C `scan_impact`: `unsigned char`.
        pub scan_impact: UBits<2> = Map::field("scan_impact"),
        /// C `boot_only`: `unsigned char`.
        pub boot_only: bool = Map::field("boot_only"),
        /// C `min_scan_interval`: `unsigned int`.
        pub min_scan_interval: u32 = Map::field("min_scan_interval"),
        /// C `op_classes`: `em_scan_cap_op_class_info_t` with count `op_classes_num`.
        pub op_classes: BoundedVec<ScanCapOpClassInfo, 64> = Map::counted("op_classes_num", "op_classes"),
    }
}

record! {
    /// C `em_ap_radio_advanced_cap_t` (`inc/em_base.h:2218`).
    pub struct ApRadioAdvancedCap = "em_ap_radio_advanced_cap_t", copy {
        /// C `ruid`: `mac_address_t`.
        pub ruid: MacAddr = Map::field("ruid"),
        /// C `reserved`: `unsigned char`.
        pub reserved: bool = Map::field("reserved"),
        /// C `qm_scs_traffic_description`: `unsigned char`.
        pub qm_scs_traffic_description: bool = Map::field("qm_scs_traffic_description"),
        /// C `dscp_policy`: `unsigned char`.
        pub dscp_policy: bool = Map::field("dscp_policy"),
        /// C `qos_map`: `unsigned char`.
        pub qos_map: bool = Map::field("qos_map"),
        /// C `scs`: `unsigned char`.
        pub scs: bool = Map::field("scs"),
        /// C `mscs`: `unsigned char`.
        pub mscs: bool = Map::field("mscs"),
        /// C `comp_prof1_prof2`: `unsigned char`.
        pub comp_prof1_prof2: bool = Map::field("comp_prof1_prof2"),
        /// C `comb_front_back`: `unsigned char`.
        pub comb_front_back: bool = Map::field("comb_front_back"),
    }
}

record! {
    /// C `em_profile_2_ap_cap_t` (`inc/em_base.h:1750`).
    pub struct Profile2ApCap = "em_profile_2_ap_cap_t", copy {
        /// C `max_prior_rule`: `unsigned char`.
        pub max_prior_rule: u8 = Map::field("max_prior_rule"),
        /// C `reserved1`: `unsigned char`.
        pub reserved1: u8 = Map::field("reserved1"),
        /// C `reserved2`: `unsigned char`.
        pub reserved2: UBits<3> = Map::field("reserved2"),
        /// C `traffic_separation`: `unsigned char`.
        pub traffic_separation: bool = Map::field("traffic_separation"),
        /// C `dpp_onboarding`: `unsigned char`.
        pub dpp_onboarding: bool = Map::field("dpp_onboarding"),
        /// C `prioritization`: `unsigned char`.
        pub prioritization: bool = Map::field("prioritization"),
        /// C `byte_counter_units`: `unsigned char`.
        pub byte_counter_units: UBits<2> = Map::field("byte_counter_units"),
        /// C `max_vid_count`: `unsigned char`.
        pub max_vid_count: u8 = Map::field("max_vid_count"),
    }
}

record! {
    /// C `em_cac_op_class_t` (`inc/em_base.h:546`).
    pub struct CacOpClass = "em_cac_op_class_t", owned {
        /// C `op_class`: `unsigned char`.
        pub op_class: u8 = Map::field("op_class"),
        /// C `channels`: `unsigned char` with count `num`.
        pub channels: BoundedVec<u8, 16> = Map::counted("num", "channels"),
    }
}

record! {
    /// C `em_cac_cap_method_t` (`inc/em_base.h:2086`).
    pub struct CacCapMethod = "em_cac_cap_method_t", owned {
        /// C `cac_method`: `unsigned char`.
        pub cac_method: u8 = Map::field("cac_method"),
        /// C `cac_duration`: `unsigned int`.
        pub cac_duration: UBits<24> = Map::field("cac_duration"),
        /// C `op_classes`: `em_cac_op_class_t` with count `op_classes_num`.
        pub op_classes: BoundedVec<CacOpClass, 48> = Map::counted("op_classes_num", "op_classes"),
    }
}

record! {
    /// C `em_cac_cap_radio_t` (`inc/em_base.h:2093`).
    pub struct CacCapRadio = "em_cac_cap_radio_t", owned {
        /// C `ruid`: `mac_address_t`.
        pub ruid: MacAddr = Map::field("ruid"),
        /// C `cac_methods`: `em_cac_cap_method_t` with count `cac_methods_num`.
        pub cac_methods: BoundedVec<CacCapMethod, 4> = Map::counted("cac_methods_num", "cac_methods"),
    }
}

record! {
    /// The capabilities of one radio. Large: store it as `Box<RadioCapInfo>` in collections
    /// (inline size in the crate README).
    ///
    /// C `em_radio_cap_info_t` (`inc/em_base.h:2977`).
    pub struct RadioCapInfo = "em_radio_cap_info_t", owned {
        /// C `ruid`: `em_interface_t`.
        pub ruid: Interface = Map::field("ruid"),
        /// C `mode`: `wifi_ieee80211Variant_t`.
        pub mode: Ieee80211Variant = Map::field("mode"),
        /// C `ht_cap`: `em_ap_ht_cap_t`.
        pub ht_cap: ApHtCap = Map::field("ht_cap"),
        /// C `vht_cap`: `em_ap_vht_cap_t`.
        pub vht_cap: ApVhtCap = Map::field("vht_cap"),
        /// C `he_cap`: `em_ap_he_cap_t`.
        pub he_cap: ApHeCap = Map::field("he_cap"),
        /// C `wifi6_cap`: `em_radio_wifi6_cap_data_t`.
        pub wifi6_cap: RadioWifi6CapData = Map::field("wifi6_cap"),
        /// C `wifi7_cap`: `em_wifi7_agent_cap_t`.
        pub wifi7_cap: Wifi7AgentCap = Map::field("wifi7_cap"),
        /// C `eht_ops`: `em_eht_operations_t`.
        pub eht_ops: EhtOperations = Map::field("eht_ops"),
        /// C `ch_scan`: `em_channel_scan_cap_radio_t`.
        pub ch_scan: ChannelScanCapRadio = Map::field("ch_scan"),
        /// C `radio_ad_cap`: `em_ap_radio_advanced_cap_t`.
        pub radio_ad_cap: ApRadioAdvancedCap = Map::field("radio_ad_cap"),
        /// C `prof_2_ap_cap`: `em_profile_2_ap_cap_t`.
        pub prof_2_ap_cap: Profile2ApCap = Map::field("prof_2_ap_cap"),
        /// C `cac_cap`: `em_cac_cap_radio_t`.
        pub cac_cap: CacCapRadio = Map::field("cac_cap"),
        /// C `metric_interval`: `em_metric_cltn_interval_t`.
        pub metric_interval: MetricCltnInterval = Map::field("metric_interval"),
        /// C `num_op_classes`: `unsigned int`.
        pub num_op_classes: u32 = Map::field("num_op_classes"),
    }
}

impl RadioCapInfo {
    /// The zeroed C record: upstream's `dm_radio_cap_t::init()` (`dm_radio_cap.cpp:44`) is a
    /// `memset` to zero. The model uses it for a new radio whose capability record is missing or
    /// is for another radio (`dm_easy_mesh.cpp:240-247`), and for the placeholders that fill
    /// earlier slots when a capability is stored (`store_cap_at`).
    #[must_use]
    pub fn zeroed() -> Box<Self> {
        let he_maps = [ApHeMcsMaps {
            tx_he_mcs: 0,
            rx_he_mcs: 0,
        }; 3];
        Box::new(Self {
            ruid: crate::records::zeroed_interface(),
            mode: Ieee80211Variant(0),
            ht_cap: ApHtCap {
                ruid: MacAddr::ZERO,
                reserved: false,
                ht_sprt_40mhz: false,
                gi_sprt_40mhz: false,
                gi_sprt_20mhz: false,
                max_sprt_rx_streams: UBits::default(),
                max_sprt_tx_streams: UBits::default(),
            },
            vht_cap: ApVhtCap {
                ruid: MacAddr::ZERO,
                sprt_tx_mcs: 0,
                sprt_rx_mcs: 0,
                gi_sprt_160mhz: false,
                gi_sprt_80mhz: false,
                max_sprt_rx_streams: UBits::default(),
                max_sprt_tx_streams: UBits::default(),
                reserved: UBits::default(),
                mu_beamformer_cap: false,
                su_beamformer_cap: false,
                sprt_160mhz: false,
                sprt_80_80_mhz: false,
            },
            he_cap: ApHeCap {
                ruid: MacAddr::ZERO,
                sprt_mcs_len: 0,
                sprt_tx_rx_mcs: he_maps,
                sprt_160mhz: false,
                sprt_80_80_mhz: false,
                max_sprt_rx_streams: UBits::default(),
                max_sprt_tx_streams: UBits::default(),
                reserved: false,
                dl_ofdma_cap: false,
                ul_ofdma_cap: false,
                dl_mimo_ofdma_cap: false,
                ul_mimo_ofdma_cap: false,
                ul_mimo_cap: false,
                mu_beamformer_cap: false,
                su_beamformer_cap: false,
            },
            wifi6_cap: RadioWifi6CapData {
                roles: BoundedVec::new(),
            },
            wifi7_cap: wifi7_cap(),
            eht_ops: EhtOperations {
                radios: BoundedVec::new(),
            },
            ch_scan: ChannelScanCapRadio {
                ruid: MacAddr::ZERO,
                reserved: UBits::default(),
                scan_impact: UBits::default(),
                boot_only: false,
                min_scan_interval: 0,
                op_classes: BoundedVec::new(),
            },
            radio_ad_cap: ApRadioAdvancedCap {
                ruid: MacAddr::ZERO,
                reserved: false,
                qm_scs_traffic_description: false,
                dscp_policy: false,
                qos_map: false,
                scs: false,
                mscs: false,
                comp_prof1_prof2: false,
                comb_front_back: false,
            },
            prof_2_ap_cap: Profile2ApCap {
                max_prior_rule: 0,
                reserved1: 0,
                reserved2: UBits::default(),
                traffic_separation: false,
                dpp_onboarding: false,
                prioritization: false,
                byte_counter_units: UBits::default(),
                max_vid_count: 0,
            },
            cac_cap: CacCapRadio {
                ruid: MacAddr::ZERO,
                cac_methods: BoundedVec::new(),
            },
            metric_interval: MetricCltnInterval {
                metric_cltn_interval: 0,
            },
            num_op_classes: 0,
        })
    }
}

fn wifi7_cap() -> Wifi7AgentCap {
    Wifi7AgentCap {
        mlo_cap_support: Wifi7MloCapSupport {
            ruid: MacAddr::ZERO,
            reserved3: [0; 24],
            reserved4: UBits::default(),
            ap_emlmr_support: false,
            ap_emlsr_support: false,
            ap_nstr_support: false,
            ap_str_support: false,
            reserved5: UBits::default(),
            bsta_emlmr_support: false,
            bsta_emlsr_support: false,
            bsta_nstr_support: false,
            bsta_str_support: false,
        },
        mlo_cap_records: Wifi7MloCapRecords {
            ap_str: freq_records(),
            ap_nstr: freq_records(),
            ap_emlsr: freq_records(),
            ap_emlmr: freq_records(),
            bsta_str: freq_records(),
            bsta_nstr: freq_records(),
            bsta_emlsr: freq_records(),
            bsta_emlmr: freq_records(),
        },
    }
}

fn freq_records() -> Wifi7FreqRecords {
    Wifi7FreqRecords {
        records: BoundedVec::new(),
    }
}
