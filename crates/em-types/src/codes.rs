// SPDX-License-Identifier: Apache-2.0

//! Wire-code newtypes: [`MsgType`], [`TlvType`], [`FreqBand`], [`MediaType`],
//! [`ProfileType`], and [`ServiceType`].
//!
//! Each type is an open newtype over its wire integer.  Unknown values are
//! accepted and round-trip unchanged; the codec in `em-msg` decides policy.
//! See design note 0001 (`docs/0001-em-types-scope.md`), decisions D3 and D4.
//!
//! Entries were derived from `classification.tsv` and `upstream_enums.tsv`
//! via a throwaway script outside the repo.

// Test-only supporting types used by the golden test.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Class {
    Spec,
    Vendor,
}

#[cfg(test)]
pub(crate) struct Entry {
    pub(crate) name: &'static str,
    pub(crate) value: u32,
    pub(crate) class: Class,
    pub(crate) upstream: Option<&'static str>,
    pub(crate) spec_ref: Option<&'static str>,
}

macro_rules! wire_code {
    // --- sub-rules: generate a single associated const ---

    // spec entry with an upstream identifier
    (@const spec $Const:ident, $val:literal, $upstream:literal, $spec_ref:literal) => {
        #[doc = concat!("`", stringify!($Const), "`")]
        #[doc = concat!("Spec: ", $spec_ref)]
        #[doc = concat!("Upstream: `", $upstream, "`")]
        pub const $Const: Self = Self($val);
    };
    // spec entry that has no upstream identifier (an addition)
    (@const spec $Const:ident, $val:literal, none, $spec_ref:literal) => {
        #[doc = concat!("`", stringify!($Const), "`")]
        #[doc = concat!("Spec: ", $spec_ref)]
        /// Not defined upstream (addition)
        pub const $Const: Self = Self($val);
    };
    // vendor entry (no spec_ref)
    (@const vendor $Const:ident, $val:literal, $upstream:literal, _) => {
        #[doc = concat!("`", stringify!($Const), "`")]
        /// Non-standard (vendor)
        #[doc = concat!("Upstream: `", $upstream, "`")]
        pub const $Const: Self = Self($val);
    };

    // --- sub-rules: generate a single test-table Entry ---

    (@entry spec, $Const:ident, $val:literal, $upstream:literal, $spec_ref:literal) => {
        Entry {
            name: stringify!($Const),
            value: $val,
            class: Class::Spec,
            upstream: Some($upstream),
            spec_ref: Some($spec_ref),
        }
    };
    (@entry spec, $Const:ident, $val:literal, none, $spec_ref:literal) => {
        Entry {
            name: stringify!($Const),
            value: $val,
            class: Class::Spec,
            upstream: None,
            spec_ref: Some($spec_ref),
        }
    };
    (@entry vendor, $Const:ident, $val:literal, $upstream:literal, _) => {
        Entry {
            name: stringify!($Const),
            value: $val,
            class: Class::Vendor,
            upstream: Some($upstream),
            spec_ref: None,
        }
    };

    // --- main rule ---
    (
        $( #[$type_meta:meta] )*
        pub struct $Name:ident($Int:ty);
        const HEX_WIDTH: usize = $W:literal;
        entries: [ $( ($Const:ident, $val:literal, $class:ident, $upstream:tt, $spec_ref:tt) ),* $(,)? ]
    ) => {
        $( #[$type_meta] )*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $Name(
            /// The raw wire value.
            pub $Int
        );

        impl $Name {
            $( wire_code!(@const $class $Const, $val, $upstream, $spec_ref); )*

            /// Returns the name of this value if it is a named constant, or `None`
            /// for unknown or reserved values.
            #[must_use]
            pub const fn name(self) -> Option<&'static str> {
                match self.0 {
                    $( $val => Some(stringify!($Const)), )*
                    _ => None,
                }
            }

            /// All named constants for this type, in declaration order.
            pub const ALL: &'static [Self] = &[
                $( Self::$Const, )*
            ];

            #[cfg(test)]
            pub(crate) const TABLE: &'static [Entry] = &[
                $( wire_code!(@entry $class, $Const, $val, $upstream, $spec_ref), )*
            ];
        }

        impl From<$Int> for $Name {
            fn from(v: $Int) -> Self {
                Self(v)
            }
        }

        impl From<$Name> for $Int {
            fn from(v: $Name) -> $Int {
                v.0
            }
        }

        impl core::fmt::Debug for $Name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                match self.name() {
                    Some(n) => write!(f, "{n}(0x{:0>w$X})", self.0, w = $W),
                    None => {
                        let type_name = stringify!($Name);
                        write!(f, "{type_name}(0x{:0>w$X})", self.0, w = $W)
                    }
                }
            }
        }
    };
}

wire_code! {
    /// `EasyMesh` / IEEE 1905.1 message type (wire-encoded `u16`).
    ///
    /// Open newtype: unknown values are accepted and round-trip unchanged.
    /// The codec decides how to handle an unrecognised value
    /// (see design note [0001, D3](https://github.com/udai-kiran/unified-wifi-mesh-rs/blob/main/docs/0001-em-types-scope.md)).
    pub struct MsgType(u16);
    const HEX_WIDTH: usize = 4;
    entries: [
        (TOPOLOGY_DISCOVERY, 0x0000, spec, "em_msg_type_topo_disc", "IEEE 1905.1 (unverified: 1905.1 text not available)"),
        (TOPOLOGY_NOTIFICATION, 0x0001, spec, "em_msg_type_topo_notif", "Table 21, §17.1; message §17.1.5"),
        (TOPOLOGY_QUERY, 0x0002, spec, "em_msg_type_topo_query", "Table 21, §17.1; message §17.1.38"),
        (TOPOLOGY_RESPONSE, 0x0003, spec, "em_msg_type_topo_resp", "Table 21, §17.1; message §17.1.4"),
        (VENDOR_SPECIFIC, 0x0004, spec, "em_msg_type_topo_vendor", "IEEE 1905.1 (unverified: 1905.1 text not available)"),
        (LINK_METRIC_QUERY, 0x0005, spec, "em_msg_type_link_metric_query", "IEEE 1905.1 (unverified: 1905.1 text not available)"),
        (LINK_METRIC_RESPONSE, 0x0006, spec, "em_msg_type_link_metric_resp", "IEEE 1905.1 (unverified: 1905.1 text not available)"),
        (AP_AUTOCONFIGURATION_SEARCH, 0x0007, spec, "em_msg_type_autoconf_search", "Table 21, §17.1; message §17.1.1"),
        (AP_AUTOCONFIGURATION_RESPONSE, 0x0008, spec, "em_msg_type_autoconf_resp", "Table 21, §17.1; message §17.1.2"),
        (AP_AUTOCONFIGURATION_WSC, 0x0009, spec, "em_msg_type_autoconf_wsc", "Table 21, §17.1; message §17.1.3"),
        (AP_AUTOCONFIGURATION_RENEW, 0x000A, spec, "em_msg_type_autoconf_renew", "IEEE 1905.1 (unverified: 1905.1 text not available)"),
        (IEEE1905_ACK, 0x8000, spec, "em_msg_type_1905_ack", "Table 21, §17.1; message §17.1.32"),
        (AP_CAPABILITY_QUERY, 0x8001, spec, "em_msg_type_ap_cap_query", "Table 21, §17.1; message §17.1.6"),
        (AP_CAPABILITY_REPORT, 0x8002, spec, "em_msg_type_ap_cap_rprt", "Table 21, §17.1; message §17.1.7"),
        (MULTI_AP_POLICY_CONFIG_REQUEST, 0x8003, spec, "em_msg_type_map_policy_config_req", "Table 21, §17.1; message §17.1.8"),
        (CHANNEL_PREFERENCE_QUERY, 0x8004, spec, "em_msg_type_channel_pref_query", "Table 21, §17.1; message §17.1.9"),
        (CHANNEL_PREFERENCE_REPORT, 0x8005, spec, "em_msg_type_channel_pref_rprt", "Table 21, §17.1; message §17.1.10"),
        (CHANNEL_SELECTION_REQUEST, 0x8006, spec, "em_msg_type_channel_sel_req", "Table 21, §17.1; message §17.1.11"),
        (CHANNEL_SELECTION_RESPONSE, 0x8007, spec, "em_msg_type_channel_sel_rsp", "Table 21, §17.1; message §17.1.12"),
        (OPERATING_CHANNEL_REPORT, 0x8008, spec, "em_msg_type_op_channel_rprt", "Table 21, §17.1; message §17.1.13"),
        (CLIENT_CAPABILITY_QUERY, 0x8009, spec, "em_msg_type_client_cap_query", "Table 21, §17.1; message §17.1.14"),
        (CLIENT_CAPABILITY_REPORT, 0x800A, spec, "em_msg_type_client_cap_rprt", "Table 21, §17.1; message §17.1.15"),
        (AP_METRICS_QUERY, 0x800B, spec, "em_msg_type_ap_metrics_query", "Table 21, §17.1; message §17.1.16"),
        (AP_METRICS_RESPONSE, 0x800C, spec, "em_msg_type_ap_metrics_rsp", "Table 21, §17.1; message §17.1.17"),
        (ASSOCIATED_STA_LINK_METRICS_QUERY, 0x800D, spec, "em_msg_type_assoc_sta_link_metrics_query", "Table 21, §17.1; message §17.1.18"),
        (ASSOCIATED_STA_LINK_METRICS_RESPONSE, 0x800E, spec, "em_msg_type_assoc_sta_link_metrics_rsp", "Table 21, §17.1; message §17.1.19"),
        (UNASSOCIATED_STA_LINK_METRICS_QUERY, 0x800F, spec, "em_msg_type_unassoc_sta_link_metrics_query", "Table 21, §17.1; message §17.1.20"),
        (UNASSOCIATED_STA_LINK_METRICS_RESPONSE, 0x8010, spec, "em_msg_type_unassoc_sta_link_metrics_rsp", "Table 21, §17.1; message §17.1.21"),
        (BEACON_METRICS_QUERY, 0x8011, spec, "em_msg_type_beacon_metrics_query", "Table 21, §17.1; message §17.1.22"),
        (BEACON_METRICS_RESPONSE, 0x8012, spec, "em_msg_type_beacon_metrics_rsp", "Table 21, §17.1; message §17.1.23"),
        (COMBINED_INFRASTRUCTURE_METRICS, 0x8013, spec, "em_msg_type_combined_infra_metrics", "Table 21, §17.1; message §17.1.24"),
        (CLIENT_STEERING_REQUEST, 0x8014, spec, "em_msg_type_client_steering_req", "Table 21, §17.1; message §17.1.25"),
        (CLIENT_STEERING_BTM_REPORT, 0x8015, spec, "em_msg_type_client_steering_btm_rprt", "Table 21, §17.1; message §17.1.26"),
        (CLIENT_ASSOCIATION_CONTROL_REQUEST, 0x8016, spec, "em_msg_type_client_assoc_ctrl_req", "Table 21, §17.1; message §17.1.27"),
        (STEERING_COMPLETED, 0x8017, spec, "em_msg_type_steering_complete", "Table 21, §17.1; message §17.1.28"),
        (HIGHER_LAYER_DATA, 0x8018, spec, "em_msg_type_higher_layer_data", "Table 21, §17.1; message §17.1.31"),
        (BACKHAUL_STEERING_REQUEST, 0x8019, spec, "em_msg_type_bh_steering_req", "Table 21, §17.1; message §17.1.29"),
        (BACKHAUL_STEERING_RESPONSE, 0x801A, spec, "em_msg_type_bh_steering_rsp", "Table 21, §17.1; message §17.1.30"),
        (CHANNEL_SCAN_REQUEST, 0x801B, spec, "em_msg_type_channel_scan_req", "Table 21, §17.1; message §17.1.33"),
        (CHANNEL_SCAN_REPORT, 0x801C, spec, "em_msg_type_channel_scan_rprt", "Table 21, §17.1; message §17.1.34"),
        (DPP_CCE_INDICATION, 0x801D, spec, "em_msg_type_dpp_cce_ind", "Table 21, §17.1; message §17.1.51"),
        (IEEE1905_REKEY_REQUEST, 0x801E, spec, "em_msg_type_1905_rekey_req", "Table 21, §17.1; message §17.1.45"),
        (IEEE1905_DECRYPTION_FAILURE, 0x801F, spec, "em_msg_type_1905_decrypt_fail", "Table 21, §17.1; message §17.1.46"),
        (CAC_REQUEST, 0x8020, spec, "em_msg_type_cac_req", "Table 21, §17.1; message §17.1.35"),
        (CAC_TERMINATION, 0x8021, spec, "em_msg_type_cac_term", "Table 21, §17.1; message §17.1.36"),
        (CLIENT_DISASSOCIATION_STATS, 0x8022, spec, "em_msg_type_client_disassoc_stats", "Table 21, §17.1; message §17.1.41"),
        (SERVICE_PRIORITIZATION_REQUEST, 0x8023, spec, "em_msg_type_svc_prio_req", "Table 21, §17.1; message §17.1.47"),
        (ERROR_RESPONSE, 0x8024, spec, "em_msg_type_err_rsp", "Table 21, §17.1; message §17.1.37"),
        (ASSOCIATION_STATUS_NOTIFICATION, 0x8025, spec, "em_msg_type_assoc_status_notif", "Table 21, §17.1; message §17.1.39"),
        (TUNNELED, 0x8026, spec, "em_msg_type_tunneled", "Table 21, §17.1; message §17.1.40"),
        (BACKHAUL_STA_CAPABILITY_QUERY, 0x8027, spec, "em_msg_type_bh_sta_cap_query", "Table 21, §17.1; message §17.1.42"),
        (BACKHAUL_STA_CAPABILITY_REPORT, 0x8028, spec, "em_msg_type_bh_sta_cap_rprt", "Table 21, §17.1; message §17.1.43"),
        (PROXIED_ENCAP_DPP, 0x8029, spec, "em_msg_type_proxied_encap_dpp", "Table 21, §17.1; message §17.1.48"),
        (DIRECT_ENCAP_DPP, 0x802A, spec, "em_msg_type_direct_encap_dpp", "Table 21, §17.1; message §17.1.56"),
        (RECONFIGURATION_TRIGGER, 0x802B, spec, "em_msg_type_reconfig_trigger", "Table 21, §17.1; message §17.1.57"),
        (BSS_CONFIGURATION_REQUEST, 0x802C, spec, "em_msg_type_bss_config_req", "Table 21, §17.1; message §17.1.53"),
        (BSS_CONFIGURATION_RESPONSE, 0x802D, spec, "em_msg_type_bss_config_rsp", "Table 21, §17.1; message §17.1.54"),
        (BSS_CONFIGURATION_RESULT, 0x802E, spec, "em_msg_type_bss_config_res", "Table 21, §17.1; message §17.1.55"),
        (CHIRP_NOTIFICATION, 0x802F, spec, "em_msg_type_chirp_notif", "Table 21, §17.1; message §17.1.52"),
        (IEEE1905_ENCAP_EAPOL, 0x8030, spec, "em_msg_type_1905_encap_eapol", "Table 21, §17.1; message §17.1.49"),
        (DPP_BOOTSTRAPPING_URI_NOTIFICATION, 0x8031, spec, "em_msg_type_dpp_bootstrap_uri_notif", "Table 21, §17.1; message §17.1.50"),
        (ANTICIPATED_CHANNEL_PREFERENCE, 0x8032, spec, "em_msg_type_anticipated_channel_pref", "Table 21, §17.1; message §17.1.59"),
        (FAILED_CONNECTION, 0x8033, spec, "em_msg_type_failed_conn", "Table 21, §17.1; message §17.1.44"),
        (AGENT_LIST, 0x8035, spec, "em_msg_type_agent_list", "Table 21, §17.1; message §17.1.58"),
        (ANTICIPATED_CHANNEL_USAGE_REPORT, 0x8036, spec, "em_msg_type_anticipated_channel_usage_rprt", "Table 21, §17.1; message §17.1.60"),
        (QOS_MANAGEMENT_NOTIFICATION, 0x8037, spec, "em_msg_type_qos_mgmt_notif", "Table 21, §17.1; message §17.1.61"),
        (EARLY_AP_CAPABILITY_REPORT, 0x8043, spec, "em_msg_type_early_ap_cap_rprt", "Table 21, §17.1; message §17.1.62"),
        (AP_MLD_CONFIGURATION_REQUEST, 0x8044, spec, "em_msg_type_ap_mld_config_req", "Table 21, §17.1; message §17.1.63"),
        (AP_MLD_CONFIGURATION_RESPONSE, 0x8045, spec, "em_msg_type_ap_mld_config_resp", "Table 21, §17.1; message §17.1.64"),
        (BSTA_MLD_CONFIGURATION_REQUEST, 0x8046, spec, "em_msg_type_bsta_mld_config_req", "Table 21, §17.1; message §17.1.65"),
        (BSTA_MLD_CONFIGURATION_RESPONSE, 0x8047, spec, "em_msg_type_bsta_mld_config_resp", "Table 21, §17.1; message §17.1.66"),
        (AVAILABLE_SPECTRUM_INQUIRY, 0x8049, spec, "em_msg_type_avail_spectrum_inquiry", "Table 21, §17.1; message §17.1.67"),
    ]
}

wire_code! {
    /// `EasyMesh` / IEEE 1905.1 TLV type (wire-encoded `u8`).
    ///
    /// Open newtype: unknown values are accepted and round-trip unchanged.
    /// The codec decides how to handle an unrecognised value
    /// (see design note [0001, D3](https://github.com/udai-kiran/unified-wifi-mesh-rs/blob/main/docs/0001-em-types-scope.md)).
    pub struct TlvType(u8);
    const HEX_WIDTH: usize = 2;
    entries: [
        (END_OF_MESSAGE, 0x00, spec, "em_tlv_type_eom", "IEEE 1905.1 (unverified: 1905.1 text not available)"),
        (AL_MAC_ADDRESS, 0x01, spec, "em_tlv_type_al_mac_address", "IEEE 1905.1 (unverified: 1905.1 text not available)"),
        (MAC_ADDRESS, 0x02, spec, "em_tlv_type_mac_address", "IEEE 1905.1 (unverified: 1905.1 text not available)"),
        (DEVICE_INFORMATION, 0x03, spec, "em_tlv_type_device_info", "IEEE 1905.1 (unverified: 1905.1 text not available)"),
        (DEVICE_BRIDGING_CAPABILITY, 0x04, spec, "em_tlv_type_device_bridging_cap", "IEEE 1905.1 (unverified: 1905.1 text not available)"),
        (NON_1905_NEIGHBOR_DEVICE_LIST, 0x06, spec, "em_tlv_type_non1905_neigh_list", "IEEE 1905.1 (unverified: 1905.1 text not available)"),
        (IEEE1905_NEIGHBOR_DEVICE_LIST, 0x07, spec, "em_tlv_type_1905_neigh_list", "IEEE 1905.1 (unverified: 1905.1 text not available)"),
        (LINK_METRIC_QUERY, 0x08, spec, "em_tlv_type_link_metric", "IEEE 1905.1 (unverified: 1905.1 text not available)"),
        (TRANSMITTER_LINK_METRIC, 0x09, spec, "em_tlv_type_transmitter_link_metric", "IEEE 1905.1 (unverified: 1905.1 text not available)"),
        (RECEIVER_LINK_METRIC, 0x0A, spec, "em_tlv_type_receiver_link_metric", "IEEE 1905.1 (unverified: 1905.1 text not available)"),
        (VENDOR_SPECIFIC, 0x0B, spec, "em_tlv_type_vendor_specific", "IEEE 1905.1 (unverified: 1905.1 text not available)"),
        (LINK_METRIC_RESULT_CODE, 0x0C, spec, "em_tlv_type_link_metric_result_code", "IEEE 1905.1 (unverified: 1905.1 text not available)"),
        (SEARCHED_ROLE, 0x0D, spec, "em_tlv_type_searched_role", "IEEE 1905.1 (unverified: 1905.1 text not available)"),
        (AUTOCONFIG_FREQ_BAND, 0x0E, spec, "em_tlv_type_autoconf_freq_band", "IEEE 1905.1 (unverified: 1905.1 text not available)"),
        (SUPPORTED_ROLE, 0x0F, spec, "em_tlv_type_supported_role", "IEEE 1905.1 (unverified: 1905.1 text not available)"),
        (SUPPORTED_FREQ_BAND, 0x10, spec, "em_tlv_type_supported_freq_band", "IEEE 1905.1 (unverified: 1905.1 text not available)"),
        (WSC, 0x11, spec, "em_tlv_type_wsc", "IEEE 1905.1 (unverified: 1905.1 text not available)"),
        (SUPPORTED_SERVICE, 0x80, spec, "em_tlv_type_supported_service", "Table 22, §17.2; TLV §17.2.1"),
        (SEARCHED_SERVICE, 0x81, spec, "em_tlv_type_searched_service", "Table 22, §17.2; TLV §17.2.2"),
        (AP_RADIO_IDENTIFIER, 0x82, spec, "em_tlv_type_radio_id", "Table 22, §17.2; TLV §17.2.3"),
        (AP_OPERATIONAL_BSS, 0x83, spec, "em_tlv_type_operational_bss", "Table 22, §17.2; TLV §17.2.4"),
        (ASSOCIATED_CLIENTS, 0x84, spec, "em_tlv_type_associated_clients", "Table 22, §17.2; TLV §17.2.5"),
        (AP_RADIO_BASIC_CAPABILITIES, 0x85, spec, "em_tlv_type_ap_radio_basic_cap", "Table 22, §17.2; TLV §17.2.7"),
        (AP_HT_CAPABILITIES, 0x86, spec, "em_tlv_type_ht_cap", "Table 22, §17.2; TLV §17.2.8"),
        (AP_VHT_CAPABILITIES, 0x87, spec, "em_tlv_type_vht_cap", "Table 22, §17.2; TLV §17.2.9"),
        (AP_HE_CAPABILITIES, 0x88, spec, "em_tlv_type_he_cap", "Table 22, §17.2; TLV §17.2.10"),
        (STEERING_POLICY, 0x89, spec, "em_tlv_type_steering_policy", "Table 22, §17.2; TLV §17.2.11"),
        (METRIC_REPORTING_POLICY, 0x8A, spec, "em_tlv_type_metric_reporting_policy", "Table 22, §17.2; TLV §17.2.12"),
        (CHANNEL_PREFERENCE, 0x8B, spec, "em_tlv_type_channel_pref", "Table 22, §17.2; TLV §17.2.13"),
        (RADIO_OPERATION_RESTRICTION, 0x8C, spec, "em_tlv_type_radio_op_restriction", "Table 22, §17.2; TLV §17.2.14"),
        (TRANSMIT_POWER_LIMIT, 0x8D, spec, "em_tlv_type_tx_power", "Table 22, §17.2; TLV §17.2.15"),
        (CHANNEL_SELECTION_RESPONSE, 0x8E, spec, "em_tlv_type_channel_sel_resp", "Table 22, §17.2; TLV §17.2.16"),
        (OPERATING_CHANNEL_REPORT, 0x8F, spec, "em_tlv_type_op_channel_report", "Table 22, §17.2; TLV §17.2.17"),
        (CLIENT_INFO, 0x90, spec, "em_tlv_type_client_info", "Table 22, §17.2; TLV §17.2.18"),
        (CLIENT_CAPABILITY_REPORT, 0x91, spec, "em_tlv_type_client_cap_report", "Table 22, §17.2; TLV §17.2.19"),
        (CLIENT_ASSOCIATION_EVENT, 0x92, spec, "em_tlv_type_client_assoc_event", "Table 22, §17.2; TLV §17.2.20"),
        (AP_METRICS_QUERY, 0x93, spec, "em_tlv_type_ap_metrics_query", "Table 22, §17.2; TLV §17.2.21"),
        (AP_METRICS, 0x94, spec, "em_tlv_type_ap_metrics", "Table 22, §17.2; TLV §17.2.22"),
        (STA_MAC_ADDRESS_TYPE, 0x95, spec, "em_tlv_type_sta_mac_addr", "Table 22, §17.2; TLV §17.2.23"),
        (ASSOCIATED_STA_LINK_METRICS, 0x96, spec, "em_tlv_type_assoc_sta_link_metric", "Table 22, §17.2; TLV §17.2.24"),
        (UNASSOCIATED_STA_LINK_METRICS_QUERY, 0x97, spec, "em_tlv_type_unassoc_sta_link_metric_query", "Table 22, §17.2; TLV §17.2.25"),
        (UNASSOCIATED_STA_LINK_METRICS_RESPONSE, 0x98, spec, "em_tlv_type_unassoc_sta_link_metric_rsp", "Table 22, §17.2; TLV §17.2.26"),
        (BEACON_METRICS_QUERY, 0x99, spec, "em_tlv_type_bcon_metric_query", "Table 22, §17.2; TLV §17.2.27"),
        (BEACON_METRICS_RESPONSE, 0x9A, spec, "em_tlv_type_bcon_metric_rsp", "Table 22, §17.2; TLV §17.2.28"),
        (STEERING_REQUEST, 0x9B, spec, "em_tlv_type_steering_request", "Table 22, §17.2; TLV §17.2.29"),
        (STEERING_BTM_REPORT, 0x9C, spec, "em_tlv_type_steering_btm_rprt", "Table 22, §17.2; TLV §17.2.30"),
        (CLIENT_ASSOCIATION_CONTROL_REQUEST, 0x9D, spec, "em_tlv_type_client_assoc_ctrl_req", "Table 22, §17.2; TLV §17.2.31"),
        (BACKHAUL_STEERING_REQUEST, 0x9E, spec, "em_tlv_type_bh_steering_req", "Table 22, §17.2; TLV §17.2.32"),
        (BACKHAUL_STEERING_RESPONSE, 0x9F, spec, "em_tlv_type_bh_steering_rsp", "Table 22, §17.2; TLV §17.2.33"),
        (HIGHER_LAYER_DATA, 0xA0, spec, "em_tlv_type_higher_layer_data", "Table 22, §17.2; TLV §17.2.34"),
        (AP_CAPABILITY, 0xA1, spec, "em_tlv_type_ap_cap", "Table 22, §17.2; TLV §17.2.6"),
        (ASSOCIATED_STA_TRAFFIC_STATS, 0xA2, spec, "em_tlv_type_assoc_sta_traffic_sts", "Table 22, §17.2; TLV §17.2.35"),
        (ERROR_CODE, 0xA3, spec, "em_tlv_type_error_code", "Table 22, §17.2; TLV §17.2.36"),
        (CHANNEL_SCAN_REPORTING_POLICY, 0xA4, spec, "em_tlv_type_channel_scan_rprt_policy", "Table 22, §17.2; TLV §17.2.37"),
        (CHANNEL_SCAN_CAPABILITIES, 0xA5, spec, "em_tlv_type_channel_scan_cap", "Table 22, §17.2; TLV §17.2.38"),
        (CHANNEL_SCAN_REQUEST, 0xA6, spec, "em_tlv_type_channel_scan_req", "Table 22, §17.2; TLV §17.2.39"),
        (CHANNEL_SCAN_RESULT, 0xA7, spec, "em_tlv_type_channel_scan_rslt", "Table 22, §17.2; TLV §17.2.40"),
        (TIMESTAMP, 0xA8, spec, "em_tlv_type_timestamp", "Table 22, §17.2; TLV §17.2.41"),
        (IEEE1905_LAYER_SECURITY_CAPABILITY, 0xA9, spec, "em_tlv_type_1905_layer_security_cap", "Table 22, §17.2; TLV §17.2.67"),
        (AP_WIFI_6_CAPABILITIES, 0xAA, spec, "em_tlv_type_ap_wifi6_cap", "Table 22, §17.2; TLV §17.2.72"),
        (MIC, 0xAB, spec, "em_tlv_type_mic", "Table 22, §17.2; TLV §17.2.68"),
        (ENCRYPTED_PAYLOAD, 0xAC, spec, "em_tlv_type_encrypt_payload", "Table 22, §17.2; TLV §17.2.69"),
        (CAC_REQUEST, 0xAD, spec, "em_tlv_type_cac_req", "Table 22, §17.2; TLV §17.2.42"),
        (CAC_TERMINATION, 0xAE, spec, "em_tlv_type_cac_term", "Table 22, §17.2; TLV §17.2.43"),
        (CAC_COMPLETION_REPORT, 0xAF, spec, "em_tlv_type_cac_cmpltn_rprt", "Table 22, §17.2; TLV §17.2.44"),
        (ASSOCIATED_WIFI_6_STA_STATUS_REPORT, 0xB0, spec, "em_tlv_type_assoc_wifi6_sta_rprt", "Table 22, §17.2; TLV §17.2.73"),
        (CAC_STATUS_REPORT, 0xB1, spec, "em_tlv_type_cac_sts_rprt", "Table 22, §17.2; TLV §17.2.45"),
        (CAC_CAPABILITIES, 0xB2, spec, "em_tlv_type_cac_cap", "Table 22, §17.2; TLV §17.2.46"),
        (MULTI_AP_PROFILE, 0xB3, spec, "em_tlv_type_profile", "Table 22, §17.2; TLV §17.2.47"),
        (PROFILE_2_AP_CAPABILITY, 0xB4, spec, "em_tlv_type_profile_2_ap_cap", "Table 22, §17.2; TLV §17.2.48"),
        (DEFAULT_8021Q_SETTINGS, 0xB5, spec, "em_tlv_type_dflt_8021q_settings", "Table 22, §17.2; TLV §17.2.49"),
        (TRAFFIC_SEPARATION_POLICY, 0xB6, spec, "em_tlv_type_traffic_separation_policy", "Table 22, §17.2; TLV §17.2.50"),
        (BSS_CONFIGURATION_REPORT, 0xB7, spec, "em_tlv_type_bss_conf_rep", "Table 22, §17.2; TLV §17.2.75"),
        (BSSID, 0xB8, spec, "em_tlv_type_bssid", "Table 22, §17.2; TLV §17.2.74"),
        (SERVICE_PRIORITIZATION_RULE, 0xB9, spec, "em_tlv_type_svc_prio_rule", "Table 22, §17.2; TLV §17.2.70"),
        (DSCP_MAPPING_TABLE, 0xBA, spec, "em_tlv_type_dscp_map_tbl", "Table 22, §17.2; TLV §17.2.71"),
        (BSS_CONFIGURATION_REQUEST, 0xBB, spec, "em_tlv_type_bss_conf_req", "Table 22, §17.2; TLV §17.2.84"),
        (PROFILE_2_ERROR_CODE, 0xBC, spec, "em_tlv_type_profile_2_error_code", "Table 22, §17.2; TLV §17.2.51"),
        (BSS_CONFIGURATION_RESPONSE, 0xBD, spec, "em_tlv_type_bss_conf_rsp", "Table 22, §17.2; TLV §17.2.85"),
        (AP_RADIO_ADVANCED_CAPABILITIES, 0xBE, spec, "em_tlv_type_ap_radio_advanced_cap", "Table 22, §17.2; TLV §17.2.52"),
        (ASSOCIATION_STATUS_NOTIFICATION, 0xBF, spec, "em_tlv_type_assoc_sts_notif", "Table 22, §17.2; TLV §17.2.53"),
        (SOURCE_INFO, 0xC0, spec, "em_tlv_type_src_info", "Table 22, §17.2; TLV §17.2.54"),
        (TUNNELED_MESSAGE_TYPE, 0xC1, spec, "em_tlv_type_tunneled_msg_type", "Table 22, §17.2; TLV §17.2.55"),
        (TUNNELED, 0xC2, spec, "em_tlv_type_tunneled", "Table 22, §17.2; TLV §17.2.56"),
        (PROFILE_2_STEERING_REQUEST, 0xC3, spec, "em_tlv_type_profile2_steering_request", "Table 22, §17.2; TLV §17.2.57"),
        (UNSUCCESSFUL_ASSOCIATION_POLICY, 0xC4, spec, "em_tlv_type_unsucc_assoc_policy", "Table 22, §17.2; TLV §17.2.58"),
        (METRIC_COLLECTION_INTERVAL, 0xC5, spec, "em_tlv_type_metric_cltn_interval", "Table 22, §17.2; TLV §17.2.59"),
        (RADIO_METRICS, 0xC6, spec, "em_tlv_type_radio_metric", "Table 22, §17.2; TLV §17.2.60"),
        (AP_EXTENDED_METRICS, 0xC7, spec, "em_tlv_type_ap_ext_metric", "Table 22, §17.2; TLV §17.2.61"),
        (ASSOCIATED_STA_EXTENDED_LINK_METRICS, 0xC8, spec, "em_tlv_type_assoc_sta_ext_link_metric", "Table 22, §17.2; TLV §17.2.62"),
        (STATUS_CODE, 0xC9, spec, "em_tlv_type_status_code", "Table 22, §17.2; TLV §17.2.63"),
        (REASON_CODE, 0xCA, spec, "em_tlv_type_reason_code", "Table 22, §17.2; TLV §17.2.64"),
        (BACKHAUL_STA_RADIO_CAPABILITIES, 0xCB, spec, "em_tlv_type_bh_sta_radio_cap", "Table 22, §17.2; TLV §17.2.65"),
        (AKM_SUITE_CAPABILITIES, 0xCC, spec, "em_tlv_type_akm_suite", "Table 22, §17.2; TLV §17.2.78"),
        (IEEE1905_ENCAP_DPP, 0xCD, spec, "em_tlv_type_1905_encap_dpp", "Table 22, §17.2; TLV §17.2.79"),
        (IEEE1905_ENCAP_EAPOL, 0xCE, spec, "em_tlv_type_1905_encap_eapol", "Table 22, §17.2; TLV §17.2.80"),
        (DPP_BOOTSTRAPPING_URI_NOTIFICATION, 0xCF, spec, "em_tlv_type_dpp_bootstrap_uri_notification", "Table 22, §17.2; TLV §17.2.81"),
        (BACKHAUL_BSS_CONFIGURATION, 0xD0, spec, "em_tlv_type_backhaul_bss_conf", "Table 22, §17.2; TLV §17.2.66"),
        (DPP_MESSAGE, 0xD1, spec, "em_tlv_type_dpp_msg", "Table 22, §17.2; TLV §17.2.86"),
        (DPP_CCE_INDICATION, 0xD2, spec, "em_tlv_type_dpp_cce_indication", "Table 22, §17.2; TLV §17.2.82"),
        (DPP_CHIRP_VALUE, 0xD3, spec, "em_tlv_type_dpp_chirp_value", "Table 22, §17.2; TLV §17.2.83"),
        (DEVICE_INVENTORY, 0xD4, spec, "em_tlv_type_device_inventory", "Table 22, §17.2; TLV §17.2.76"),
        (AGENT_LIST, 0xD5, spec, "em_tlv_type_agent_list", "Table 22, §17.2; TLV §17.2.77"),
        (ANTICIPATED_CHANNEL_PREFERENCE, 0xD6, spec, "em_tlv_type_anticipated_channel_pref", "Table 22, §17.2; TLV §17.2.87"),
        (ANTICIPATED_CHANNEL_USAGE, 0xD7, spec, "em_tlv_type_channel_usage", "Table 22, §17.2; TLV §17.2.88"),
        (SPATIAL_REUSE_REQUEST, 0xD8, spec, "em_tlv_type_spatial_reuse_req", "Table 22, §17.2; TLV §17.2.89"),
        (SPATIAL_REUSE_REPORT, 0xD9, spec, "em_tlv_type_spatial_reuse_rep", "Table 22, §17.2; TLV §17.2.90"),
        (SPATIAL_REUSE_CONFIG_RESPONSE, 0xDA, spec, "em_tlv_type_spatial_reuse_cfg_rsp", "Table 22, §17.2; TLV §17.2.91"),
        (QOS_MANAGEMENT_POLICY, 0xDB, spec, "em_tlv_type_qos_mgmt_policy", "Table 22, §17.2; TLV §17.2.92"),
        (QOS_MANAGEMENT_DESCRIPTOR, 0xDC, spec, "em_tlv_type_qos_mgmt_desc", "Table 22, §17.2; TLV §17.2.93"),
        (CONTROLLER_CAPABILITY, 0xDD, spec, "em_tlv_type_ctrl_cap", "Table 22, §17.2; TLV §17.2.94"),
        (WIFI_7_AGENT_CAPABILITIES, 0xDF, spec, "em_tlv_type_wifi7_agent_cap", "Table 22, §17.2; TLV §17.2.95"),
        (AGENT_AP_MLD_CONFIGURATION, 0xE0, spec, "em_tlv_type_ap_mld_config", "Table 22, §17.2; TLV §17.2.96"),
        (BACKHAUL_STA_MLD_CONFIGURATION, 0xE1, spec, "em_tlv_type_bsta_mld_config", "Table 22, §17.2; TLV §17.2.97"),
        (ASSOCIATED_STA_MLD_CONFIGURATION_REPORT, 0xE2, spec, "em_tlv_type_assoc_sta_mld_conf_rep", "Table 22, §17.2; TLV §17.2.98"),
        (AFFILIATED_STA_METRICS, 0xE4, spec, "em_tlv_type_affiliated_sta_metrics", "Table 22, §17.2; TLV §17.2.100"),
        (TID_TO_LINK_MAPPING_POLICY, 0xE6, spec, "em_tlv_type_tid_to_link_map_policy", "Table 22, §17.2; TLV §17.2.102"),
        (EHT_OPERATIONS, 0xE7, spec, "em_tlv_eht_operations", "Table 22, §17.2; TLV §17.2.103"),
        (AVAILABLE_SPECTRUM_INQUIRY_REQUEST, 0xE8, spec, "em_tlv_type_avail_spectrum_inquiry_reg", "Table 126, §17.2.104"),
        (AVAILABLE_SPECTRUM_INQUIRY_RESPONSE, 0xE9, spec, "em_tlv_type_avail_spectrum_inquiry_rsp", "Table 127, §17.2.105"),
        (VENDOR_OPERATIONAL_BSS, 0xF2, vendor, "em_tlv_type_vendor_operational_bss", _),
        (MLD_STRUCTURE, 0xE3, spec, none, "Table 22, §17.2; TLV §17.2.99"),
        (AFFILIATED_AP_METRICS, 0xE5, spec, none, "Table 22, §17.2; TLV §17.2.101"),
    ]
}

wire_code! {
    /// `EasyMesh` / IEEE 1905.1 frequency band (wire-encoded `u8`).
    ///
    /// Open newtype: unknown values are accepted and round-trip unchanged.
    /// The codec decides how to handle an unrecognised value
    /// (see design note [0001, D3](https://github.com/udai-kiran/unified-wifi-mesh-rs/blob/main/docs/0001-em-types-scope.md)).
    ///
    /// Sentinel and reserved values (such as `0x04`, used by upstream as "unknown band")
    /// are not named constants; they still round-trip unchanged (see amendment A1).
    pub struct FreqBand(u8);
    const HEX_WIDTH: usize = 2;
    entries: [
        (BAND_2_4_GHZ, 0x00, spec, "em_freq_band_24", "Table 15, §6.1 (extends IEEE 1905.1 Table 6-23)"),
        (BAND_5_GHZ, 0x01, spec, "em_freq_band_5", "Table 15, §6.1 (extends IEEE 1905.1 Table 6-23)"),
        (BAND_60_GHZ, 0x02, spec, "em_freq_band_60", "Table 15, §6.1 (extends IEEE 1905.1 Table 6-23)"),
        (BAND_6_GHZ, 0x03, spec, "em_freq_band_6", "Table 15, §6.1 (extends IEEE 1905.1 Table 6-23)"),
    ]
}

wire_code! {
    /// `EasyMesh` / IEEE 1905.1 media type (wire-encoded `u16`).
    ///
    /// Open newtype: unknown values are accepted and round-trip unchanged.
    /// The codec decides how to handle an unrecognised value
    /// (see design note [0001, D3](https://github.com/udai-kiran/unified-wifi-mesh-rs/blob/main/docs/0001-em-types-scope.md)).
    pub struct MediaType(u16);
    const HEX_WIDTH: usize = 4;
    entries: [
        (IEEE_802_3AB, 0x0001, spec, "em_media_type_ieee8023ab", "IEEE 1905.1 media type (unverified: 1905.1 text not available)"),
        (IEEE_802_11B_2_4_GHZ, 0x0100, spec, "em_media_type_ieee80211b_24", "IEEE 1905.1 media type (unverified: 1905.1 text not available)"),
        (IEEE_802_11G_2_4_GHZ, 0x0101, spec, "em_media_type_ieee80211g_24", "IEEE 1905.1 media type (unverified: 1905.1 text not available)"),
        (IEEE_802_11A_5_GHZ, 0x0102, spec, "em_media_type_ieee80211a_5", "IEEE 1905.1 media type (unverified: 1905.1 text not available)"),
        (IEEE_802_11N_2_4_GHZ, 0x0103, spec, "em_media_type_ieee80211n_24", "IEEE 1905.1 media type (unverified: 1905.1 text not available)"),
        (IEEE_802_11N_5_GHZ, 0x0104, spec, "em_media_type_ieee80211n_5", "IEEE 1905.1 media type (unverified: 1905.1 text not available)"),
        (IEEE_802_11AC_5_GHZ, 0x0105, spec, "em_media_type_ieee80211ac_5", "IEEE 1905.1 media type (unverified: 1905.1 text not available)"),
        (IEEE_802_11AD_60_GHZ, 0x0106, spec, "em_media_type_ieee80211ad_60", "IEEE 1905.1 media type (unverified: 1905.1 text not available)"),
        (IEEE_802_11AF, 0x0107, spec, "em_media_type_ieee80211af", "IEEE 1905.1 media type (unverified: 1905.1 text not available)"),
    ]
}

wire_code! {
    /// `EasyMesh` Multi-AP profile type (wire-encoded `u8`).
    ///
    /// Open newtype: unknown values are accepted and round-trip unchanged.
    /// The codec decides how to handle an unrecognised value
    /// (see design note [0001, D3](https://github.com/udai-kiran/unified-wifi-mesh-rs/blob/main/docs/0001-em-types-scope.md)).
    ///
    /// Sentinel and reserved values (such as `0x00`, reserved per Table 69, and `0x04`,
    /// the upstream upper-bound marker) are not named constants;
    /// they still round-trip unchanged (see amendment A1).
    pub struct ProfileType(u8);
    const HEX_WIDTH: usize = 2;
    entries: [
        (MULTI_AP_PROFILE_1, 0x01, spec, "em_profile_type_1", "Table 69, §17.2.47"),
        (MULTI_AP_PROFILE_2, 0x02, spec, "em_profile_type_2", "Table 69, §17.2.47"),
        (MULTI_AP_PROFILE_3, 0x03, spec, "em_profile_type_3", "Table 69, §17.2.47"),
    ]
}

wire_code! {
    /// `EasyMesh` service type (wire-encoded `u8`); used in Supported/Searched-Service TLVs.
    ///
    /// Open newtype: unknown values are accepted and round-trip unchanged.
    /// The codec decides how to handle an unrecognised value
    /// (see design note [0001, D3](https://github.com/udai-kiran/unified-wifi-mesh-rs/blob/main/docs/0001-em-types-scope.md)).
    ///
    /// Internal and sentinel values (such as `0x02`, reserved per Table 23, and `0xA2`,
    /// upstream's "none" marker) are not named constants;
    /// they still round-trip unchanged (see amendment A1).
    pub struct ServiceType(u8);
    const HEX_WIDTH: usize = 2;
    entries: [
        (MULTI_AP_CONTROLLER, 0x00, spec, "em_service_type_ctrl", "Table 23, §17.2.1"),
        (MULTI_AP_AGENT, 0x01, spec, "em_service_type_agent", "Table 23, §17.2.1"),
        (EMPLUS_CONTROLLER, 0xA0, vendor, "em_service_type_emplus_ctrl", _),
        (EMPLUS_AGENT, 0xA1, vendor, "em_service_type_emplus_agent", _),
    ]
}

#[cfg(test)]
mod golden {
    use super::{Class, Entry, FreqBand, MediaType, MsgType, ProfileType, ServiceType, TlvType};

    // Every internal/sentinel row in classification.tsv that is NOT in the Rust API.
    // Columns: (c_enum, upstream_identifier, class, reason)
    const EXCLUDED: &[(&str, &str, &str, &str)] = &[
        (
            "em_tlv_type_t",
            "em_tlv_type_max",
            "sentinel",
            "upper bound 0xF3",
        ),
        (
            "em_freq_band_t",
            "em_freq_band_unknown",
            "sentinel",
            "0x04 reserved per Table 15",
        ),
        (
            "em_media_type_t",
            "em_media_type_max",
            "sentinel",
            "upper bound 0x0108",
        ),
        (
            "em_profile_type_t",
            "em_profile_type_reserved",
            "sentinel",
            "0x00 reserved per Table 69",
        ),
        (
            "em_profile_type_t",
            "em_profile_type_max",
            "sentinel",
            "upper bound 0x04",
        ),
        (
            "em_service_type_t",
            "em_service_type_cli",
            "internal",
            "IPC endpoint, reserved on wire per Table 23",
        ),
        (
            "em_service_type_t",
            "em_service_type_none",
            "sentinel",
            "upstream none-marker 0xA2",
        ),
    ];

    // Spec entries that upstream lacks; appear in the Rust API but not in the fixture.
    // Columns: (rust_type_name, const_name, value, spec_ref)
    const ADDITIONS: &[(&str, &str, u32, &str)] = &[
        (
            "TlvType",
            "MLD_STRUCTURE",
            0xE3,
            "Table 22, \u{00a7}17.2; TLV \u{00a7}17.2.99",
        ),
        (
            "TlvType",
            "AFFILIATED_AP_METRICS",
            0xE5,
            "Table 22, \u{00a7}17.2; TLV \u{00a7}17.2.101",
        ),
    ];

    fn all_tables() -> Vec<(&'static str, &'static [Entry])> {
        vec![
            ("em_msg_type_t", MsgType::TABLE),
            ("em_tlv_type_t", TlvType::TABLE),
            ("em_freq_band_t", FreqBand::TABLE),
            ("em_media_type_t", MediaType::TABLE),
            ("em_profile_type_t", ProfileType::TABLE),
            ("em_service_type_t", ServiceType::TABLE),
        ]
    }

    fn c_enum_to_rust_type(c_enum: &str) -> &'static str {
        match c_enum {
            "em_msg_type_t" => "MsgType",
            "em_tlv_type_t" => "TlvType",
            "em_freq_band_t" => "FreqBand",
            "em_media_type_t" => "MediaType",
            "em_profile_type_t" => "ProfileType",
            "em_service_type_t" => "ServiceType",
            other => panic!("unknown c_enum: {other}"),
        }
    }

    fn parse_classification() -> Vec<[String; 6]> {
        let raw = include_str!("../tests/data/classification.tsv");
        let mut seen: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();
        raw.lines()
            .filter(|l| !l.starts_with('#'))
            .skip(1) // skip header
            .filter(|l| !l.is_empty())
            .map(|l| {
                let cols: Vec<&str> = l.split('\t').collect();
                assert!(cols.len() >= 6, "unexpected column count: {l}");
                let pair = (cols[0].to_owned(), cols[1].to_owned());
                assert!(
                    seen.insert(pair),
                    "duplicate (c_enum, identifier) in classification.tsv: {l}"
                );
                std::array::from_fn(|i| cols[i].to_owned())
            })
            .collect()
    }

    fn parse_fixture() -> std::collections::HashMap<(String, String), u32> {
        let raw = include_str!("../tests/data/upstream_enums.tsv");
        let mut map = std::collections::HashMap::new();
        for l in raw
            .lines()
            .filter(|l| !l.starts_with('#'))
            .skip(1) // skip header
            .filter(|l| !l.is_empty())
        {
            let cols: Vec<&str> = l.split('\t').collect();
            assert!(cols.len() >= 3, "unexpected column count: {l}");
            let key = (cols[0].to_owned(), cols[1].to_owned());
            let val = u32::from_str_radix(cols[2].trim_start_matches("0x"), 16)
                .expect("bad hex value in fixture");
            assert!(map.insert(key, val).is_none(), "duplicate fixture key: {l}");
        }
        map
    }

    #[test]
    fn spec_vendor_rows_match_tables() {
        let rows = parse_classification();
        let fixture = parse_fixture();
        let tables = all_tables();

        for row in &rows {
            let (c_enum, identifier, class, rust_name, spec_reference, _reason) =
                (&row[0], &row[1], &row[2], &row[3], &row[4], &row[5]);

            if class != "spec" && class != "vendor" {
                continue;
            }

            let table: &[Entry] = tables
                .iter()
                .find(|(ce, _)| *ce == c_enum.as_str())
                .map_or_else(|| panic!("no table for {c_enum}"), |(_, t)| *t);

            let entry = table
                .iter()
                .find(|e| e.upstream == Some(identifier.as_str()))
                .unwrap_or_else(|| {
                    panic!("no TABLE entry for upstream identifier {identifier:?} in {c_enum}")
                });

            assert_eq!(
                entry.name,
                rust_name.as_str(),
                "rust_name mismatch for {identifier}"
            );

            let fix_val = fixture
                .get(&(c_enum.clone(), identifier.clone()))
                .copied()
                .unwrap_or_else(|| panic!("missing fixture for {c_enum}/{identifier}"));

            assert_eq!(
                entry.value, fix_val,
                "value mismatch for {identifier}: table={} fixture={fix_val}",
                entry.value
            );

            match class.as_str() {
                "spec" => {
                    assert_eq!(entry.class, Class::Spec, "class mismatch for {identifier}");
                    assert_eq!(
                        entry.spec_ref,
                        Some(spec_reference.as_str()),
                        "spec_ref mismatch for {identifier}"
                    );
                }
                "vendor" => {
                    assert_eq!(
                        entry.class,
                        Class::Vendor,
                        "class mismatch for {identifier}"
                    );
                    assert_eq!(
                        entry.spec_ref, None,
                        "vendor entry should have spec_ref=None for {identifier}"
                    );
                }
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn internal_sentinel_rows_in_excluded() {
        let rows = parse_classification();
        let fixture = parse_fixture();
        let tables = all_tables();

        let excluded_set: std::collections::HashSet<(&str, &str)> =
            EXCLUDED.iter().map(|(c, id, _, _)| (*c, *id)).collect();

        let mut excluded_in_rows: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();

        for row in &rows {
            let (c_enum, identifier, class) = (&row[0], &row[1], &row[2]);
            if class != "internal" && class != "sentinel" {
                continue;
            }
            excluded_in_rows.insert((c_enum.clone(), identifier.clone()));

            assert!(
                excluded_set.contains(&(c_enum.as_str(), identifier.as_str())),
                "internal/sentinel row not in EXCLUDED: {c_enum} / {identifier}"
            );

            if let Some((_, table)) = tables.iter().find(|(ce, _)| *ce == c_enum.as_str()) {
                for entry in *table {
                    assert_ne!(
                        entry.upstream,
                        Some(identifier.as_str()),
                        "excluded identifier {identifier} appears in TABLE"
                    );
                }
            }

            assert!(
                fixture.contains_key(&(c_enum.clone(), identifier.clone())),
                "excluded row {c_enum}/{identifier} not in fixture"
            );

            // For each internal/sentinel row, check the EXCLUDED entry
            if let Some((_, _, excl_class, excl_reason)) = EXCLUDED
                .iter()
                .find(|(c, id, _, _)| c == c_enum && id == identifier)
            {
                assert_eq!(
                    *excl_class,
                    class.as_str(),
                    "EXCLUDED class mismatch for {c_enum}/{identifier}"
                );
                assert!(
                    !excl_reason.is_empty(),
                    "EXCLUDED reason is empty for {c_enum}/{identifier}"
                );
                assert_ne!(
                    row[5].as_str(),
                    "-",
                    "classification.tsv reason is '-' for {c_enum}/{identifier}"
                );
            }
        }

        for (c_enum, identifier, _, _) in EXCLUDED {
            assert!(
                excluded_in_rows.contains(&((*c_enum).to_owned(), (*identifier).to_owned())),
                "EXCLUDED entry ({c_enum}, {identifier}) not found as internal/sentinel row"
            );
        }

        assert_eq!(
            EXCLUDED.len(),
            7,
            "expected exactly 7 EXCLUDED entries, got {}",
            EXCLUDED.len()
        );
    }

    #[test]
    fn additions_are_complete_and_valid() {
        let fixture = parse_fixture();
        let tables = all_tables();

        let mut found_additions: std::collections::HashSet<(&str, &str)> =
            std::collections::HashSet::new();

        for (c_enum, table) in &tables {
            let rust_type = c_enum_to_rust_type(c_enum);
            for entry in *table {
                if entry.upstream.is_some() {
                    continue;
                }
                found_additions.insert((rust_type, entry.name));

                let add = ADDITIONS
                    .iter()
                    .find(|(rt, name, _, _)| *rt == rust_type && *name == entry.name)
                    .unwrap_or_else(|| {
                        panic!("addition {rust_type}/{} not in ADDITIONS const", entry.name)
                    });

                for ((fe_type, _), fe_val) in &fixture {
                    if fe_type.as_str() == *c_enum {
                        assert_ne!(
                            entry.value, *fe_val,
                            "addition {} has same value as fixture entry",
                            entry.name
                        );
                    }
                }

                assert_eq!(
                    entry.spec_ref,
                    Some(add.3),
                    "spec_ref mismatch for addition {}",
                    entry.name
                );
                assert_eq!(
                    entry.value, add.2,
                    "value mismatch for addition {}",
                    entry.name
                );
            }
        }

        for (rust_type, name, _, _) in ADDITIONS {
            assert!(
                found_additions.contains(&(*rust_type, *name)),
                "ADDITIONS entry ({rust_type}, {name}) not found in TABLE"
            );
        }

        assert_eq!(
            ADDITIONS.len(),
            2,
            "expected exactly 2 ADDITIONS, got {}",
            ADDITIONS.len()
        );
    }

    #[test]
    fn no_duplicate_names_or_values() {
        for (c_enum, table) in all_tables() {
            let mut names = std::collections::HashSet::new();
            let mut values = std::collections::HashSet::new();
            for entry in table {
                assert!(
                    names.insert(entry.name),
                    "duplicate name {:?} in {c_enum}",
                    entry.name
                );
                assert!(
                    values.insert(entry.value),
                    "duplicate value {} in {c_enum}",
                    entry.value
                );
            }
        }
    }

    #[test]
    fn all_slice_matches_table() {
        assert_eq!(MsgType::ALL.len(), MsgType::TABLE.len());
        assert_eq!(TlvType::ALL.len(), TlvType::TABLE.len());
        assert_eq!(FreqBand::ALL.len(), FreqBand::TABLE.len());
        assert_eq!(MediaType::ALL.len(), MediaType::TABLE.len());
        assert_eq!(ProfileType::ALL.len(), ProfileType::TABLE.len());
        assert_eq!(ServiceType::ALL.len(), ServiceType::TABLE.len());

        for (entry, v) in MsgType::TABLE.iter().zip(MsgType::ALL) {
            assert_eq!(v.name(), Some(entry.name));
        }
        for (entry, v) in TlvType::TABLE.iter().zip(TlvType::ALL) {
            assert_eq!(v.name(), Some(entry.name));
        }
        for (entry, v) in FreqBand::TABLE.iter().zip(FreqBand::ALL) {
            assert_eq!(v.name(), Some(entry.name));
        }
        for (entry, v) in MediaType::TABLE.iter().zip(MediaType::ALL) {
            assert_eq!(v.name(), Some(entry.name));
        }
        for (entry, v) in ProfileType::TABLE.iter().zip(ProfileType::ALL) {
            assert_eq!(v.name(), Some(entry.name));
        }
        for (entry, v) in ServiceType::TABLE.iter().zip(ServiceType::ALL) {
            assert_eq!(v.name(), Some(entry.name));
        }
    }

    #[test]
    fn fixture_and_classification_keys_match() {
        let fixture = parse_fixture();
        let rows = parse_classification();

        let fixture_keys: std::collections::HashSet<(String, String)> =
            fixture.keys().cloned().collect();
        let class_keys: std::collections::HashSet<(String, String)> =
            rows.iter().map(|r| (r[0].clone(), r[1].clone())).collect();

        assert_eq!(
            fixture.len(),
            220,
            "expected exactly 220 fixture entries, got {}",
            fixture.len()
        );

        if fixture_keys != class_keys {
            let mut only_in_fixture: Vec<_> = fixture_keys.difference(&class_keys).collect();
            only_in_fixture.sort();
            let mut only_in_class: Vec<_> = class_keys.difference(&fixture_keys).collect();
            only_in_class.sort();
            panic!(
                "fixture/classification key mismatch\n  only in fixture: {only_in_fixture:?}\n  only in classification: {only_in_class:?}"
            );
        }
    }

    #[test]
    fn totals() {
        let all_entries: Vec<&Entry> = MsgType::TABLE
            .iter()
            .chain(TlvType::TABLE)
            .chain(FreqBand::TABLE)
            .chain(MediaType::TABLE)
            .chain(ProfileType::TABLE)
            .chain(ServiceType::TABLE)
            .collect();

        let upstream_backed = all_entries.iter().filter(|e| e.upstream.is_some()).count();
        let additions = all_entries.iter().filter(|e| e.upstream.is_none()).count();
        let spec_count = all_entries
            .iter()
            .filter(|e| e.upstream.is_some() && e.class == Class::Spec)
            .count();
        let vendor_count = all_entries
            .iter()
            .filter(|e| e.upstream.is_some() && e.class == Class::Vendor)
            .count();

        assert_eq!(
            upstream_backed, 213,
            "expected 213 upstream-backed entries, got {upstream_backed}"
        );
        assert_eq!(additions, 2, "expected 2 additions, got {additions}");
        assert_eq!(
            spec_count, 210,
            "expected 210 spec entries, got {spec_count}"
        );
        assert_eq!(
            vendor_count, 3,
            "expected 3 vendor entries, got {vendor_count}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{FreqBand, MediaType, MsgType, ProfileType, ServiceType, TlvType};

    #[test]
    fn unknown_round_trips() {
        let v = MsgType::from(0x1234_u16);
        assert_eq!(u16::from(v), 0x1234_u16);
        assert_eq!(v.name(), None);
    }

    #[test]
    fn debug_known_msg_type() {
        // AP_AUTOCONFIGURATION_SEARCH is upstream em_msg_type_autoconf_search = 0x0007
        assert_eq!(
            format!("{:?}", MsgType::AP_AUTOCONFIGURATION_SEARCH),
            "AP_AUTOCONFIGURATION_SEARCH(0x0007)"
        );
    }

    #[test]
    fn debug_unknown_msg_type() {
        assert_eq!(
            format!("{:?}", MsgType::from(0x1234_u16)),
            "MsgType(0x1234)"
        );
    }

    #[test]
    fn debug_known_tlv_type() {
        // SUPPORTED_SERVICE is upstream em_tlv_type_supported_service = 0x80
        assert_eq!(
            format!("{:?}", TlvType::SUPPORTED_SERVICE),
            "SUPPORTED_SERVICE(0x80)"
        );
    }

    #[test]
    fn debug_unknown_tlv_type() {
        // 0xF3 is em_tlv_type_max (sentinel), excluded from named consts
        assert_eq!(format!("{:?}", TlvType::from(0xF3_u8)), "TlvType(0xF3)");
    }

    #[test]
    fn freq_band_reserved_round_trips() {
        // 0x04 is em_freq_band_unknown (sentinel), not named, but still round-trips
        let v = FreqBand::from(0x04_u8);
        assert_eq!(v.name(), None);
        assert_eq!(u8::from(v), 0x04);
    }

    #[test]
    fn service_type_reserved_round_trips() {
        // 0x02 is em_service_type_cli (internal), not in API
        let v = ServiceType::from(0x02_u8);
        assert_eq!(v.name(), None);
        assert_eq!(u8::from(v), 0x02);
    }

    #[test]
    fn profile_type_reserved_round_trips() {
        // 0x00 is em_profile_type_reserved (sentinel), not named
        let v = ProfileType::from(0x00_u8);
        assert_eq!(v.name(), None);
        assert_eq!(u8::from(v), 0x00);
    }

    #[test]
    fn media_type_unknown_round_trips() {
        let v = MediaType::from(0xFFFF_u16);
        assert_eq!(v.name(), None);
        assert_eq!(u16::from(v), 0xFFFF_u16);
    }
}
