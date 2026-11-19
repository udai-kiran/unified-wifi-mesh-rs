// SPDX-License-Identifier: Apache-2.0

//! The 70 upstream validation tables, as static data.
//!
//! Each table is one `em_msg_t::<name>()` function of `em_msg.cpp` (between
//! `autoconfig_search` and `i1905_ack`), in the order of the constructor's
//! `switch`. The rows were generated from the golden fixture
//! `tests/data/upstream_msg_tables.tsv`, and a test checks that they still
//! equal it. Do not edit the rows by hand.

use em_types::{MsgType, ProfileType, TlvType};

/// How much a message needs a TLV.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requirement {
    /// The TLV must be present, and long enough.
    Mandatory,
    /// The TLV may be present. Its length is not checked.
    Optional,
    /// The TLV must not be present.
    Bad,
}

/// The requirement of a row, which may depend on the Multi-AP profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// The same requirement for every profile.
    Fixed(Requirement),
    /// `yes` if the profile number is greater than `level`, else `no`.
    IfProfileAbove {
        /// Profile number to compare against (1 or 2).
        level: u8,
        /// Requirement when the profile number is greater than `level`.
        yes: Requirement,
        /// Requirement otherwise.
        no: Requirement,
    },
}

impl Rule {
    /// The requirement for `profile`.
    ///
    /// Like upstream, this compares the raw profile number: profile 0 behaves
    /// like 1, and 4 or more like 3.
    #[must_use]
    pub fn resolve(self, profile: ProfileType) -> Requirement {
        match self {
            Self::Fixed(requirement) => requirement,
            Self::IfProfileAbove { level, yes, no } => {
                if profile.0 > level {
                    yes
                } else {
                    no
                }
            }
        }
    }
}

/// Which WSC message a table is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WscKind {
    /// WSC M1 (message type 4).
    M1,
    /// WSC M2 (message type 5).
    M2,
}

/// One table row (upstream `em_tlv_member_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    /// TLV type the row is about.
    pub tlv_type: TlvType,
    /// Requirement, by profile.
    pub rule: Rule,
    /// Minimum length, counting the 3-byte TLV header.
    pub min_len: u16,
    /// Specification reference, as upstream words it.
    pub spec_ref: &'static str,
}

/// The rows of one message type (upstream table function).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Table {
    /// Name of the upstream function that fills the table.
    pub upstream_name: &'static str,
    /// Message type the table is for.
    pub msg_type: MsgType,
    /// For AP-Autoconfiguration WSC: which of the two tables this is.
    pub wsc: Option<WscKind>,
    /// Rows, in upstream order. Duplicates are kept.
    pub rows: &'static [Row],
}

// Short names for the generated rows below.
const M: Requirement = Requirement::Mandatory;
const O: Requirement = Requirement::Optional;
const B: Requirement = Requirement::Bad;

const fn fixed(requirement: Requirement) -> Rule {
    Rule::Fixed(requirement)
}

const fn above(level: u8, yes: Requirement, no: Requirement) -> Rule {
    Rule::IfProfileAbove { level, yes, no }
}

const fn row(tlv_type: TlvType, rule: Rule, min_len: u16, spec_ref: &'static str) -> Row {
    Row {
        tlv_type,
        rule,
        min_len,
        spec_ref,
    }
}

/// All 70 tables, in the order of upstream's `switch`.
pub const TABLES: &[Table] = &[
    // autoconfig_search
    Table {
        upstream_name: "autoconfig_search",
        msg_type: MsgType::AP_AUTOCONFIGURATION_SEARCH,
        wsc: None,
        rows: &[
            row(
                TlvType::AL_MAC_ADDRESS,
                fixed(M),
                9,
                "table 6-8 of IEEE-1905-1",
            ),
            row(
                TlvType::SEARCHED_ROLE,
                fixed(M),
                4,
                "table 6-22 of IEEE-1905-1",
            ),
            row(
                TlvType::AUTOCONFIG_FREQ_BAND,
                fixed(M),
                4,
                "table 6-23 of IEEE-1905-1",
            ),
            row(
                TlvType::SUPPORTED_SERVICE,
                fixed(O),
                5,
                "17.2.1 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::SEARCHED_SERVICE,
                fixed(O),
                5,
                "17.2.2 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::MULTI_AP_PROFILE,
                above(1, M, B),
                4,
                "17.2.47 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::DPP_CHIRP_VALUE,
                above(2, O, B),
                4,
                "17.2.83 of Wi-Fi Easy Mesh 5.0",
            ),
        ],
    },
    // autoconfig_resp
    Table {
        upstream_name: "autoconfig_resp",
        msg_type: MsgType::AP_AUTOCONFIGURATION_RESPONSE,
        wsc: None,
        rows: &[
            row(
                TlvType::SUPPORTED_ROLE,
                fixed(M),
                4,
                "table 6-24 of IEEE-1905-1",
            ),
            row(
                TlvType::SUPPORTED_FREQ_BAND,
                fixed(M),
                4,
                "table 6-25 of IEEE-1905-1",
            ),
            row(
                TlvType::IEEE1905_LAYER_SECURITY_CAPABILITY,
                above(2, M, B),
                6,
                "17.2.67 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::SUPPORTED_SERVICE,
                fixed(O),
                5,
                "17.2.1 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::MULTI_AP_PROFILE,
                above(1, M, B),
                4,
                "17.2.47 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::DPP_CHIRP_VALUE,
                above(2, O, B),
                4,
                "17.2.83 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::CONTROLLER_CAPABILITY,
                fixed(O),
                3,
                "17.2.94 of Wi-Fi Easy Mesh 5.0",
            ),
        ],
    },
    // autoconfig_wsc_m1
    Table {
        upstream_name: "autoconfig_wsc_m1",
        msg_type: MsgType::AP_AUTOCONFIGURATION_WSC,
        wsc: Some(WscKind::M1),
        rows: &[
            row(
                TlvType::AP_RADIO_BASIC_CAPABILITIES,
                fixed(M),
                2,
                "17.2.7 of Wi-Fi Easy Mesh 5.0",
            ),
            row(TlvType::WSC, fixed(M), 264, "table 8 of WSC v2.0.7"),
            row(
                TlvType::PROFILE_2_AP_CAPABILITY,
                above(1, M, B),
                2,
                "17.2.48 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::AP_RADIO_ADVANCED_CAPABILITIES,
                above(1, M, B),
                2,
                "17.2.52 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::VENDOR_SPECIFIC,
                fixed(O),
                16,
                "table 6-7 of IEEE-1905-1",
            ),
        ],
    },
    // autoconfig_wsc_m2
    Table {
        upstream_name: "autoconfig_wsc_m2",
        msg_type: MsgType::AP_AUTOCONFIGURATION_WSC,
        wsc: Some(WscKind::M2),
        rows: &[
            row(
                TlvType::AP_RADIO_IDENTIFIER,
                fixed(M),
                4,
                "17.2.3 of Wi-Fi Easy Mesh 5.0",
            ),
            row(TlvType::WSC, fixed(M), 264, "table 9 of WSC v2.0.7"),
            row(
                TlvType::DEFAULT_8021Q_SETTINGS,
                above(1, O, B),
                2,
                "17.2.49 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::TRAFFIC_SEPARATION_POLICY,
                above(1, O, B),
                2,
                "17.2.50 of Wi-Fi Easy Mesh 5.0",
            ),
        ],
    },
    // topo_disc
    Table {
        upstream_name: "topo_disc",
        msg_type: MsgType::TOPOLOGY_DISCOVERY,
        wsc: None,
        rows: &[
            row(
                TlvType::AL_MAC_ADDRESS,
                fixed(M),
                9,
                "table 6-8 of IEEE-1905-1",
            ),
            row(
                TlvType::MAC_ADDRESS,
                fixed(M),
                9,
                "table 6-9 of IEEE-1905-1",
            ),
        ],
    },
    // topo_notif
    Table {
        upstream_name: "topo_notif",
        msg_type: MsgType::TOPOLOGY_NOTIFICATION,
        wsc: None,
        rows: &[
            row(
                TlvType::AL_MAC_ADDRESS,
                fixed(M),
                9,
                "table 6-8 of IEEE-1905-1",
            ),
            row(
                TlvType::CLIENT_ASSOCIATION_EVENT,
                fixed(O),
                15,
                "17.2.20 of Wi-Fi Easy Mesh 5.0",
            ),
        ],
    },
    // topo_query
    Table {
        upstream_name: "topo_query",
        msg_type: MsgType::TOPOLOGY_QUERY,
        wsc: None,
        rows: &[row(
            TlvType::MULTI_AP_PROFILE,
            above(1, M, B),
            4,
            "17.2.47 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // topo_resp
    Table {
        upstream_name: "topo_resp",
        msg_type: MsgType::TOPOLOGY_RESPONSE,
        wsc: None,
        rows: &[
            row(
                TlvType::DEVICE_INFORMATION,
                fixed(M),
                19,
                "table 6-10 of IEEE-1905-1",
            ),
            row(
                TlvType::AP_OPERATIONAL_BSS,
                fixed(M),
                18,
                "17.2.4 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::SUPPORTED_SERVICE,
                fixed(O),
                5,
                "17.2.1 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::ASSOCIATED_CLIENTS,
                fixed(O),
                20,
                "17.2.5 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::MULTI_AP_PROFILE,
                above(1, M, B),
                4,
                "17.2.47 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::BSS_CONFIGURATION_REPORT,
                above(2, M, B),
                17,
                "17.2.75 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::AGENT_AP_MLD_CONFIGURATION,
                fixed(O),
                64,
                "17.2.96 of Wi-Fi Easy Mesh 6.0",
            ),
            row(
                TlvType::BACKHAUL_STA_MLD_CONFIGURATION,
                fixed(O),
                64,
                "17.2.97 of Wi-Fi Easy Mesh 6.0",
            ),
            row(
                TlvType::ASSOCIATED_STA_MLD_CONFIGURATION_REPORT,
                fixed(O),
                64,
                "17.2.98 of Wi-Fi Easy Mesh 6.0",
            ),
            row(
                TlvType::TID_TO_LINK_MAPPING_POLICY,
                fixed(O),
                64,
                "17.2.97 of Wi-Fi Easy Mesh 6.0",
            ),
            row(
                TlvType::DEVICE_BRIDGING_CAPABILITY,
                fixed(O),
                11,
                "table 6-11 of IEEE-1905-1",
            ),
            row(
                TlvType::NON_1905_NEIGHBOR_DEVICE_LIST,
                fixed(O),
                15,
                "table 6-14 of IEEE-1905-1",
            ),
            row(
                TlvType::IEEE1905_NEIGHBOR_DEVICE_LIST,
                fixed(O),
                15,
                "table 6-15 of IEEE-1905-1",
            ),
        ],
    },
    // topo_vendor
    Table {
        upstream_name: "topo_vendor",
        msg_type: MsgType::VENDOR_SPECIFIC,
        wsc: None,
        rows: &[row(
            TlvType::VENDOR_SPECIFIC,
            fixed(M),
            6,
            "table 6-7 of IEEE-1905-1",
        )],
    },
    // link_metric_query
    Table {
        upstream_name: "link_metric_query",
        msg_type: MsgType::LINK_METRIC_QUERY,
        wsc: None,
        rows: &[row(
            TlvType::LINK_METRIC_QUERY,
            fixed(M),
            11,
            "table 6-16 of IEEE-1905-1",
        )],
    },
    // link_metric_resp
    Table {
        upstream_name: "link_metric_resp",
        msg_type: MsgType::LINK_METRIC_RESPONSE,
        wsc: None,
        rows: &[
            row(
                TlvType::TRANSMITTER_LINK_METRIC,
                fixed(O),
                15,
                "table 6-17 of IEEE-1905-1",
            ),
            row(
                TlvType::RECEIVER_LINK_METRIC,
                fixed(O),
                15,
                "table 6-19 of IEEE-1905-1",
            ),
            row(
                TlvType::LINK_METRIC_RESULT_CODE,
                fixed(O),
                4,
                "table 6-21 of IEEE-1905-1",
            ),
        ],
    },
    // autoconfig_renew
    Table {
        upstream_name: "autoconfig_renew",
        msg_type: MsgType::AP_AUTOCONFIGURATION_RENEW,
        wsc: None,
        rows: &[
            row(
                TlvType::AL_MAC_ADDRESS,
                fixed(M),
                9,
                "table 6-8 of IEEE-1905-1",
            ),
            row(
                TlvType::SUPPORTED_ROLE,
                fixed(M),
                4,
                "table 6-24 of IEEE-1905-1",
            ),
            row(
                TlvType::SUPPORTED_FREQ_BAND,
                fixed(M),
                4,
                "table 6-25 of IEEE-1905-1",
            ),
        ],
    },
    // ap_cap_query
    Table {
        upstream_name: "ap_cap_query",
        msg_type: MsgType::AP_CAPABILITY_QUERY,
        wsc: None,
        rows: &[],
    },
    // ap_cap_rprt
    Table {
        upstream_name: "ap_cap_rprt",
        msg_type: MsgType::AP_CAPABILITY_REPORT,
        wsc: None,
        rows: &[
            row(
                TlvType::AP_CAPABILITY,
                fixed(M),
                3,
                "17.2.6 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::AP_RADIO_BASIC_CAPABILITIES,
                fixed(M),
                15,
                "17.2.7 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::AP_HT_CAPABILITIES,
                fixed(O),
                9,
                "17.2.8 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::AP_VHT_CAPABILITIES,
                fixed(O),
                13,
                "17.2.9 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::AP_HE_CAPABILITIES,
                fixed(O),
                10,
                "17.2.10 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::AP_WIFI_6_CAPABILITIES,
                above(2, O, B),
                24,
                "17.2.72 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::CHANNEL_SCAN_CAPABILITIES,
                above(1, M, B),
                17,
                "17.2.38 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::IEEE1905_LAYER_SECURITY_CAPABILITY,
                above(2, M, B),
                6,
                "17.2.67 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::CAC_CAPABILITIES,
                fixed(O),
                21,
                "17.2.46 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::PROFILE_2_AP_CAPABILITY,
                above(1, M, B),
                6,
                "17.2.48 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::METRIC_COLLECTION_INTERVAL,
                above(1, M, B),
                7,
                "17.2.59 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::DEVICE_INVENTORY,
                fixed(O),
                270,
                "17.2.76 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::AP_RADIO_ADVANCED_CAPABILITIES,
                fixed(O),
                9,
                "17.2.52 of Wi-Fi Easy Mesh 5.0",
            ),
        ],
    },
    // early_ap_cap_rprt
    Table {
        upstream_name: "early_ap_cap_rprt",
        msg_type: MsgType::EARLY_AP_CAPABILITY_REPORT,
        wsc: None,
        rows: &[
            row(
                TlvType::AP_CAPABILITY,
                fixed(M),
                3,
                "17.2.6 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::AP_RADIO_BASIC_CAPABILITIES,
                fixed(M),
                15,
                "17.2.7 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::AKM_SUITE_CAPABILITIES,
                fixed(M),
                5,
                "17.2.78 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::AP_HT_CAPABILITIES,
                fixed(O),
                9,
                "17.2.8 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::AP_VHT_CAPABILITIES,
                fixed(O),
                13,
                "17.2.9 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::AP_HE_CAPABILITIES,
                fixed(O),
                10,
                "17.2.10 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::AP_WIFI_6_CAPABILITIES,
                above(2, O, B),
                24,
                "17.2.72 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::WIFI_7_AGENT_CAPABILITIES,
                above(2, O, B),
                4,
                "17.2.95 of Wi-Fi Easy Mesh 6.0",
            ),
            row(
                TlvType::EHT_OPERATIONS,
                above(2, O, B),
                4,
                "17.2.103 of Wi-Fi Easy Mesh 6.0",
            ),
            row(
                TlvType::IEEE1905_LAYER_SECURITY_CAPABILITY,
                above(2, M, B),
                6,
                "17.2.67 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::CAC_CAPABILITIES,
                fixed(O),
                21,
                "17.2.46 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::PROFILE_2_AP_CAPABILITY,
                above(1, M, B),
                6,
                "17.2.48 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::METRIC_COLLECTION_INTERVAL,
                above(1, O, B),
                7,
                "17.2.59 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::DEVICE_INVENTORY,
                fixed(O),
                270,
                "17.2.76 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::AP_RADIO_ADVANCED_CAPABILITIES,
                fixed(O),
                9,
                "17.2.52 of Wi-Fi Easy Mesh 5.0",
            ),
        ],
    },
    // policy_config_req
    Table {
        upstream_name: "policy_config_req",
        msg_type: MsgType::MULTI_AP_POLICY_CONFIG_REQUEST,
        wsc: None,
        rows: &[
            row(
                TlvType::STEERING_POLICY,
                fixed(O),
                27,
                "17.2.11 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::METRIC_REPORTING_POLICY,
                fixed(O),
                13,
                "17.2.12 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::DEFAULT_8021Q_SETTINGS,
                above(1, O, B),
                5,
                "17.2.49 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::TRAFFIC_SEPARATION_POLICY,
                above(1, O, B),
                7,
                "17.2.50 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::CHANNEL_SCAN_REPORTING_POLICY,
                above(1, O, B),
                3,
                "17.2.37 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::UNSUCCESSFUL_ASSOCIATION_POLICY,
                above(1, O, B),
                7,
                "17.2.58 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::BACKHAUL_BSS_CONFIGURATION,
                above(1, O, B),
                9,
                "17.2.66 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::QOS_MANAGEMENT_POLICY,
                fixed(O),
                37,
                "17.2.92 of Wi-Fi Easy Mesh 5.0",
            ),
        ],
    },
    // channel_pref_query
    Table {
        upstream_name: "channel_pref_query",
        msg_type: MsgType::CHANNEL_PREFERENCE_QUERY,
        wsc: None,
        rows: &[],
    },
    // channel_pref_rprt
    Table {
        upstream_name: "channel_pref_rprt",
        msg_type: MsgType::CHANNEL_PREFERENCE_REPORT,
        wsc: None,
        rows: &[
            row(
                TlvType::CHANNEL_PREFERENCE,
                fixed(O),
                12,
                "17.2.13 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::RADIO_OPERATION_RESTRICTION,
                fixed(O),
                14,
                "17.2.14 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::CAC_COMPLETION_REPORT,
                above(1, O, B),
                16,
                "17.2.44 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::CAC_STATUS_REPORT,
                above(1, M, B),
                19,
                "17.2.45 of Wi-Fi Easy Mesh 5.0",
            ),
        ],
    },
    // channel_sel_req
    Table {
        upstream_name: "channel_sel_req",
        msg_type: MsgType::CHANNEL_SELECTION_REQUEST,
        wsc: None,
        rows: &[
            row(
                TlvType::CHANNEL_PREFERENCE,
                fixed(O),
                12,
                "17.2.13 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::TRANSMIT_POWER_LIMIT,
                fixed(O),
                10,
                "17.2.15 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::SPATIAL_REUSE_REQUEST,
                fixed(O),
                9,
                "17.2.89 of Wi-Fi Easy Mesh 5.0",
            ),
        ],
    },
    // channel_sel_rsp
    Table {
        upstream_name: "channel_sel_rsp",
        msg_type: MsgType::CHANNEL_SELECTION_RESPONSE,
        wsc: None,
        rows: &[
            row(
                TlvType::CHANNEL_SELECTION_RESPONSE,
                fixed(M),
                10,
                "17.2.16 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::SPATIAL_REUSE_CONFIG_RESPONSE,
                fixed(O),
                10,
                "17.2.91 of Wi-Fi Easy Mesh 5.0",
            ),
        ],
    },
    // op_channel_rprt
    Table {
        upstream_name: "op_channel_rprt",
        msg_type: MsgType::OPERATING_CHANNEL_REPORT,
        wsc: None,
        rows: &[
            row(
                TlvType::OPERATING_CHANNEL_REPORT,
                fixed(M),
                13,
                "17.2.17 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::SPATIAL_REUSE_REPORT,
                fixed(O),
                38,
                "17.2.90 of Wi-Fi Easy Mesh 5.0",
            ),
        ],
    },
    // client_cap_query
    Table {
        upstream_name: "client_cap_query",
        msg_type: MsgType::CLIENT_CAPABILITY_QUERY,
        wsc: None,
        rows: &[row(
            TlvType::CLIENT_INFO,
            fixed(M),
            15,
            "17.2.18 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // client_steering_req
    Table {
        upstream_name: "client_steering_req",
        msg_type: MsgType::CLIENT_STEERING_REQUEST,
        wsc: None,
        rows: &[
            row(
                TlvType::STEERING_REQUEST,
                above(1, O, B),
                15,
                "17.2.29 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::PROFILE_2_STEERING_REQUEST,
                above(1, O, B),
                15,
                "17.2.57 of Wi-Fi Easy Mesh 5.0",
            ),
        ],
    },
    // client_steering_btm_rprt
    Table {
        upstream_name: "client_steering_btm_rprt",
        msg_type: MsgType::CLIENT_STEERING_BTM_REPORT,
        wsc: None,
        rows: &[row(
            TlvType::STEERING_BTM_REPORT,
            fixed(M),
            16,
            "17.2.30 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // client_assoc_ctrl_req
    Table {
        upstream_name: "client_assoc_ctrl_req",
        msg_type: MsgType::CLIENT_ASSOCIATION_CONTROL_REQUEST,
        wsc: None,
        rows: &[row(
            TlvType::CLIENT_ASSOCIATION_CONTROL_REQUEST,
            fixed(M),
            19,
            "17.2.31 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // steering_complete
    Table {
        upstream_name: "steering_complete",
        msg_type: MsgType::STEERING_COMPLETED,
        wsc: None,
        rows: &[],
    },
    // higher_layer_data
    Table {
        upstream_name: "higher_layer_data",
        msg_type: MsgType::HIGHER_LAYER_DATA,
        wsc: None,
        rows: &[row(
            TlvType::HIGHER_LAYER_DATA,
            fixed(M),
            4,
            "17.2.34 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // bh_steering_req
    Table {
        upstream_name: "bh_steering_req",
        msg_type: MsgType::BACKHAUL_STEERING_REQUEST,
        wsc: None,
        rows: &[row(
            TlvType::BACKHAUL_STEERING_REQUEST,
            fixed(M),
            17,
            "17.2.32 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // bh_steering_rsp
    Table {
        upstream_name: "bh_steering_rsp",
        msg_type: MsgType::BACKHAUL_STEERING_RESPONSE,
        wsc: None,
        rows: &[
            row(
                TlvType::ERROR_CODE,
                fixed(O),
                10,
                "17.2.36 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::BACKHAUL_STEERING_RESPONSE,
                fixed(O),
                15,
                "17.2.33 of Wi-Fi Easy Mesh 5.0",
            ),
        ],
    },
    // client_cap_rprt
    Table {
        upstream_name: "client_cap_rprt",
        msg_type: MsgType::CLIENT_CAPABILITY_REPORT,
        wsc: None,
        rows: &[
            row(
                TlvType::CLIENT_INFO,
                fixed(M),
                15,
                "17.2.18 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::CLIENT_CAPABILITY_REPORT,
                fixed(M),
                4,
                "17.2.19 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::ERROR_CODE,
                fixed(O),
                10,
                "17.2.36 of Wi-Fi Easy Mesh 5.0",
            ),
        ],
    },
    // ap_metrics_query
    Table {
        upstream_name: "ap_metrics_query",
        msg_type: MsgType::AP_METRICS_QUERY,
        wsc: None,
        rows: &[
            row(
                TlvType::AP_METRICS_QUERY,
                fixed(M),
                10,
                "17.2.21 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::AP_RADIO_IDENTIFIER,
                above(1, O, B),
                9,
                "17.2.3 of Wi-Fi Easy Mesh 5.0",
            ),
        ],
    },
    // ap_metrics_rsp
    Table {
        upstream_name: "ap_metrics_rsp",
        msg_type: MsgType::AP_METRICS_RESPONSE,
        wsc: None,
        rows: &[
            row(
                TlvType::AP_METRICS,
                fixed(M),
                16,
                "17.2.22 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::AP_EXTENDED_METRICS,
                above(1, M, B),
                33,
                "17.2.61 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::RADIO_METRICS,
                above(1, O, B),
                13,
                "17.2.60 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::ASSOCIATED_STA_TRAFFIC_STATS,
                fixed(O),
                37,
                "17.2.35 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::ASSOCIATED_STA_LINK_METRICS,
                fixed(O),
                29,
                "17.2.24 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::ASSOCIATED_STA_EXTENDED_LINK_METRICS,
                above(1, O, B),
                32,
                "17.2.62 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::ASSOCIATED_WIFI_6_STA_STATUS_REPORT,
                above(2, O, B),
                12,
                "17.2.73 of Wi-Fi Easy Mesh 5.0",
            ),
        ],
    },
    // sta_link_metrics_query
    Table {
        upstream_name: "sta_link_metrics_query",
        msg_type: MsgType::ASSOCIATED_STA_LINK_METRICS_QUERY,
        wsc: None,
        rows: &[row(
            TlvType::STA_MAC_ADDRESS_TYPE,
            fixed(M),
            9,
            "17.2.23 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // sta_link_metrics_rsp
    Table {
        upstream_name: "sta_link_metrics_rsp",
        msg_type: MsgType::ASSOCIATED_STA_LINK_METRICS_RESPONSE,
        wsc: None,
        rows: &[
            row(
                TlvType::ASSOCIATED_STA_LINK_METRICS,
                fixed(M),
                29,
                "17.2.24 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::ERROR_CODE,
                fixed(O),
                10,
                "17.2.36 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::ASSOCIATED_STA_EXTENDED_LINK_METRICS,
                above(1, M, B),
                32,
                "17.2.62 of Wi-Fi Easy Mesh 5.0",
            ),
        ],
    },
    // unassoc_sta_link_metrics_query
    Table {
        upstream_name: "unassoc_sta_link_metrics_query",
        msg_type: MsgType::UNASSOCIATED_STA_LINK_METRICS_QUERY,
        wsc: None,
        rows: &[row(
            TlvType::UNASSOCIATED_STA_LINK_METRICS_QUERY,
            fixed(M),
            13,
            "17.2.25 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // unassoc_sta_link_metrics_rsp
    Table {
        upstream_name: "unassoc_sta_link_metrics_rsp",
        msg_type: MsgType::UNASSOCIATED_STA_LINK_METRICS_RESPONSE,
        wsc: None,
        rows: &[row(
            TlvType::UNASSOCIATED_STA_LINK_METRICS_RESPONSE,
            fixed(M),
            5,
            "17.2.26 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // beacon_metrics_query
    Table {
        upstream_name: "beacon_metrics_query",
        msg_type: MsgType::BEACON_METRICS_QUERY,
        wsc: None,
        rows: &[row(
            TlvType::BEACON_METRICS_QUERY,
            fixed(M),
            23,
            "17.2.27 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // beacon_metrics_rsp
    Table {
        upstream_name: "beacon_metrics_rsp",
        msg_type: MsgType::BEACON_METRICS_RESPONSE,
        wsc: None,
        rows: &[row(
            TlvType::BEACON_METRICS_RESPONSE,
            fixed(M),
            11,
            "17.2.28 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // combined_infra_metrics
    Table {
        upstream_name: "combined_infra_metrics",
        msg_type: MsgType::COMBINED_INFRASTRUCTURE_METRICS,
        wsc: None,
        rows: &[
            row(
                TlvType::AP_METRICS,
                fixed(M),
                24,
                "17.2.22 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::TRANSMITTER_LINK_METRIC,
                fixed(M),
                50,
                "section 6.4.11 of IEEE-1905-1",
            ),
            row(
                TlvType::TRANSMITTER_LINK_METRIC,
                fixed(M),
                50,
                "section 6.4.11 of IEEE-1905-1",
            ),
            row(
                TlvType::RECEIVER_LINK_METRIC,
                fixed(M),
                38,
                "section 6.4.12 of IEEE-1905-1",
            ),
            row(
                TlvType::RECEIVER_LINK_METRIC,
                fixed(M),
                38,
                "section 6.4.12 of IEEE-1905-1",
            ),
        ],
    },
    // channel_scan_req
    Table {
        upstream_name: "channel_scan_req",
        msg_type: MsgType::CHANNEL_SCAN_REQUEST,
        wsc: None,
        rows: &[row(
            TlvType::CHANNEL_SCAN_REQUEST,
            fixed(M),
            13,
            "17.2.39 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // qos_mgmt_notif
    Table {
        upstream_name: "qos_mgmt_notif",
        msg_type: MsgType::QOS_MANAGEMENT_NOTIFICATION,
        wsc: None,
        rows: &[row(
            TlvType::QOS_MANAGEMENT_DESCRIPTOR,
            fixed(M),
            17,
            "17.2.93 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // anticipated_channel_usage_rprt
    Table {
        upstream_name: "anticipated_channel_usage_rprt",
        msg_type: MsgType::ANTICIPATED_CHANNEL_USAGE_REPORT,
        wsc: None,
        rows: &[row(
            TlvType::ANTICIPATED_CHANNEL_USAGE,
            fixed(M),
            41,
            "17.2.88 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // anticipated_channel_pref
    Table {
        upstream_name: "anticipated_channel_pref",
        msg_type: MsgType::ANTICIPATED_CHANNEL_PREFERENCE,
        wsc: None,
        rows: &[row(
            TlvType::ANTICIPATED_CHANNEL_PREFERENCE,
            fixed(M),
            8,
            "17.2.87 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // agent_list
    Table {
        upstream_name: "agent_list",
        msg_type: MsgType::AGENT_LIST,
        wsc: None,
        rows: &[row(
            TlvType::AGENT_LIST,
            above(2, M, B),
            12,
            "17.2.77 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // failed_conn
    Table {
        upstream_name: "failed_conn",
        msg_type: MsgType::FAILED_CONNECTION,
        wsc: None,
        rows: &[
            row(
                TlvType::BSSID,
                above(2, M, B),
                9,
                "17.2.74 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::STA_MAC_ADDRESS_TYPE,
                fixed(M),
                9,
                "17.2.23 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::STATUS_CODE,
                fixed(M),
                5,
                "17.2.63 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::REASON_CODE,
                fixed(O),
                5,
                "17.2.64 of Wi-Fi Easy Mesh 5.0",
            ),
        ],
    },
    // dpp_bootstrap_uri_notif
    Table {
        upstream_name: "dpp_bootstrap_uri_notif",
        msg_type: MsgType::DPP_BOOTSTRAPPING_URI_NOTIFICATION,
        wsc: None,
        rows: &[row(
            TlvType::DPP_BOOTSTRAPPING_URI_NOTIFICATION,
            above(2, M, B),
            21,
            "17.2.81 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // i1905_encap_eapol
    Table {
        upstream_name: "i1905_encap_eapol",
        msg_type: MsgType::IEEE1905_ENCAP_EAPOL,
        wsc: None,
        rows: &[row(
            TlvType::IEEE1905_ENCAP_EAPOL,
            above(2, M, B),
            3,
            "17.2.80 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // chirp_notif
    Table {
        upstream_name: "chirp_notif",
        msg_type: MsgType::CHIRP_NOTIFICATION,
        wsc: None,
        rows: &[row(
            TlvType::DPP_CHIRP_VALUE,
            above(2, M, B),
            4,
            "17.2.83 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // bss_config_res
    Table {
        upstream_name: "bss_config_res",
        msg_type: MsgType::BSS_CONFIGURATION_RESULT,
        wsc: None,
        rows: &[row(
            TlvType::BSS_CONFIGURATION_REPORT,
            above(2, M, B),
            17,
            "17.2.75 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // bss_config_rsp
    Table {
        upstream_name: "bss_config_rsp",
        msg_type: MsgType::BSS_CONFIGURATION_RESPONSE,
        wsc: None,
        rows: &[
            row(
                TlvType::BSS_CONFIGURATION_RESPONSE,
                above(2, M, B),
                3,
                "17.2.85 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::DEFAULT_8021Q_SETTINGS,
                fixed(O),
                5,
                "17.2.49 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::TRAFFIC_SEPARATION_POLICY,
                fixed(O),
                7,
                "17.2.50 of Wi-Fi Easy Mesh 5.0",
            ),
        ],
    },
    // bss_config_req
    Table {
        upstream_name: "bss_config_req",
        msg_type: MsgType::BSS_CONFIGURATION_REQUEST,
        wsc: None,
        rows: &[
            row(
                TlvType::MULTI_AP_PROFILE,
                above(2, M, B),
                4,
                "17.2.47 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::SUPPORTED_SERVICE,
                fixed(M),
                5,
                "17.2.1 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::AP_RADIO_BASIC_CAPABILITIES,
                fixed(M),
                15,
                "17.2.7 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::PROFILE_2_AP_CAPABILITY,
                fixed(M),
                7,
                "17.2.48 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::AP_RADIO_ADVANCED_CAPABILITIES,
                fixed(M),
                10,
                "17.2.52 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::BSS_CONFIGURATION_REQUEST,
                above(2, M, B),
                3,
                "17.2.84 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::AKM_SUITE_CAPABILITIES,
                above(2, M, B),
                5,
                "17.2.78 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::BACKHAUL_STA_RADIO_CAPABILITIES,
                fixed(O),
                15,
                "17.2.65 of Wi-Fi Easy Mesh 5.0",
            ),
        ],
    },
    // channel_scan_rprt
    Table {
        upstream_name: "channel_scan_rprt",
        msg_type: MsgType::CHANNEL_SCAN_REPORT,
        wsc: None,
        rows: &[
            row(
                TlvType::TIMESTAMP,
                fixed(M),
                4,
                "17.2.41 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::CHANNEL_SCAN_RESULT,
                fixed(M),
                32,
                "17.2.40 of Wi-Fi Easy Mesh 5.0",
            ),
        ],
    },
    // dpp_cce_ind
    Table {
        upstream_name: "dpp_cce_ind",
        msg_type: MsgType::DPP_CCE_INDICATION,
        wsc: None,
        rows: &[row(
            TlvType::DPP_CCE_INDICATION,
            above(2, M, B),
            4,
            "17.2.82 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // i1905_rekey_req
    Table {
        upstream_name: "i1905_rekey_req",
        msg_type: MsgType::IEEE1905_REKEY_REQUEST,
        wsc: None,
        rows: &[],
    },
    // i1905_decrypt_fail
    Table {
        upstream_name: "i1905_decrypt_fail",
        msg_type: MsgType::IEEE1905_DECRYPTION_FAILURE,
        wsc: None,
        rows: &[row(
            TlvType::AL_MAC_ADDRESS,
            above(2, M, B),
            9,
            "table 6.4.3 of IEEE 1905.1",
        )],
    },
    // cac_term
    Table {
        upstream_name: "cac_term",
        msg_type: MsgType::CAC_TERMINATION,
        wsc: None,
        rows: &[row(
            TlvType::CAC_TERMINATION,
            fixed(M),
            4,
            "17.2.43 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // client_disassoc_stats
    Table {
        upstream_name: "client_disassoc_stats",
        msg_type: MsgType::CLIENT_DISASSOCIATION_STATS,
        wsc: None,
        rows: &[
            row(
                TlvType::STA_MAC_ADDRESS_TYPE,
                fixed(M),
                9,
                "17.2.23 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::REASON_CODE,
                fixed(M),
                5,
                "17.2.64 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::ASSOCIATED_STA_TRAFFIC_STATS,
                fixed(M),
                37,
                "17.2.35 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::AFFILIATED_STA_METRICS,
                fixed(O),
                26,
                "17.2.100 of Wi-Fi Easy Mesh 6.0",
            ),
        ],
    },
    // svc_prio_req
    Table {
        upstream_name: "svc_prio_req",
        msg_type: MsgType::SERVICE_PRIORITIZATION_REQUEST,
        wsc: None,
        rows: &[
            row(
                TlvType::SERVICE_PRIORITIZATION_RULE,
                above(2, O, B),
                9,
                "17.2.70 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::DSCP_MAPPING_TABLE,
                above(2, O, B),
                67,
                "17.2.71 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::QOS_MANAGEMENT_DESCRIPTOR,
                fixed(O),
                17,
                "17.2.93 of Wi-Fi Easy Mesh 5.0",
            ),
        ],
    },
    // err_rsp
    Table {
        upstream_name: "err_rsp",
        msg_type: MsgType::ERROR_RESPONSE,
        wsc: None,
        rows: &[row(
            TlvType::PROFILE_2_ERROR_CODE,
            fixed(M),
            4,
            "17.2.51 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // assoc_status_notif
    Table {
        upstream_name: "assoc_status_notif",
        msg_type: MsgType::ASSOCIATION_STATUS_NOTIFICATION,
        wsc: None,
        rows: &[row(
            TlvType::ASSOCIATION_STATUS_NOTIFICATION,
            fixed(M),
            11,
            "17.2.53 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // tunneled
    Table {
        upstream_name: "tunneled",
        msg_type: MsgType::TUNNELED,
        wsc: None,
        rows: &[
            row(
                TlvType::SOURCE_INFO,
                fixed(M),
                9,
                "17.2.54 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::TUNNELED_MESSAGE_TYPE,
                fixed(M),
                4,
                "17.2.55 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::TUNNELED,
                fixed(M),
                3,
                "17.2.56 of Wi-Fi Easy Mesh 5.0",
            ),
        ],
    },
    // bh_sta_cap_query
    Table {
        upstream_name: "bh_sta_cap_query",
        msg_type: MsgType::BACKHAUL_STA_CAPABILITY_QUERY,
        wsc: None,
        rows: &[],
    },
    // bh_sta_cap_rprt
    Table {
        upstream_name: "bh_sta_cap_rprt",
        msg_type: MsgType::BACKHAUL_STA_CAPABILITY_REPORT,
        wsc: None,
        rows: &[row(
            TlvType::BACKHAUL_STA_RADIO_CAPABILITIES,
            fixed(O),
            15,
            "17.2.65 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // proxied_encap_dpp
    Table {
        upstream_name: "proxied_encap_dpp",
        msg_type: MsgType::PROXIED_ENCAP_DPP,
        wsc: None,
        rows: &[
            row(
                TlvType::IEEE1905_ENCAP_DPP,
                above(2, M, B),
                12,
                "17.2.79 of Wi-Fi Easy Mesh 5.0",
            ),
            row(
                TlvType::DPP_CHIRP_VALUE,
                above(2, O, B),
                4,
                "17.2.83 of Wi-Fi Easy Mesh 5.0",
            ),
        ],
    },
    // direct_encap_dpp
    Table {
        upstream_name: "direct_encap_dpp",
        msg_type: MsgType::DIRECT_ENCAP_DPP,
        wsc: None,
        rows: &[row(
            TlvType::DPP_MESSAGE,
            above(2, M, B),
            3,
            "17.2.86 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // reconfig_trigger
    Table {
        upstream_name: "reconfig_trigger",
        msg_type: MsgType::RECONFIGURATION_TRIGGER,
        wsc: None,
        rows: &[],
    },
    // cac_req
    Table {
        upstream_name: "cac_req",
        msg_type: MsgType::CAC_REQUEST,
        wsc: None,
        rows: &[row(
            TlvType::CAC_REQUEST,
            fixed(M),
            12,
            "17.2.42 of Wi-Fi Easy Mesh 5.0",
        )],
    },
    // ap_mld_config_req
    Table {
        upstream_name: "ap_mld_config_req",
        msg_type: MsgType::AP_MLD_CONFIGURATION_REQUEST,
        wsc: None,
        rows: &[row(
            TlvType::AGENT_AP_MLD_CONFIGURATION,
            fixed(M),
            4,
            "17.2.96 of Wi-Fi Easy Mesh 6.0",
        )],
    },
    // ap_mld_config_rsp
    Table {
        upstream_name: "ap_mld_config_rsp",
        msg_type: MsgType::AP_MLD_CONFIGURATION_RESPONSE,
        wsc: None,
        rows: &[row(
            TlvType::AGENT_AP_MLD_CONFIGURATION,
            fixed(M),
            4,
            "17.2.96 of Wi-Fi Easy Mesh 6.0",
        )],
    },
    // i1905_ack
    Table {
        upstream_name: "i1905_ack",
        msg_type: MsgType::IEEE1905_ACK,
        wsc: None,
        rows: &[row(
            TlvType::ERROR_CODE,
            fixed(O),
            10,
            "17.2.36 of Wi-Fi Easy Mesh 5.0",
        )],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/data/upstream_msg_tables.tsv");
    const CLASSIFICATION: &str = include_str!("../../em-types/tests/data/classification.tsv");

    /// Counts found by `compare`: tables, rows, empty tables.
    #[derive(Debug, PartialEq, Eq)]
    struct Counts {
        tables: usize,
        rows: usize,
        empty: usize,
    }

    /// One fixture line, split into its columns.
    struct Line<'a> {
        msg_type: &'a str,
        wsc: &'a str,
        table: &'a str,
        row: &'a str,
        tlv_type: &'a str,
        rule: &'a str,
        min_len: &'a str,
        spec: &'a str,
    }

    fn data_lines(text: &str) -> Vec<&str> {
        let mut lines = Vec::new();
        for line in text.lines() {
            if !line.starts_with('#') && !line.is_empty() {
                lines.push(line);
            }
        }
        lines
    }

    fn rust_name<'a>(classification: &'a str, upstream: &str) -> Result<&'a str, String> {
        for line in data_lines(classification) {
            let columns: Vec<&str> = line.split('\t').collect();
            if columns.len() > 3 && columns[1] == upstream {
                return Ok(columns[3]);
            }
        }
        Err(format!("{upstream} is not in the classification"))
    }

    fn msg_type_of(classification: &str, upstream: &str) -> Result<MsgType, String> {
        let name = rust_name(classification, upstream)?;
        for candidate in MsgType::ALL {
            if candidate.name() == Some(name) {
                return Ok(*candidate);
            }
        }
        Err(format!("no MsgType named {name} for {upstream}"))
    }

    fn tlv_type_of(classification: &str, upstream: &str) -> Result<TlvType, String> {
        let name = rust_name(classification, upstream)?;
        for candidate in TlvType::ALL {
            if candidate.name() == Some(name) {
                return Ok(*candidate);
            }
        }
        Err(format!("no TlvType named {name} for {upstream}"))
    }

    fn requirement_of(word: &str) -> Result<Requirement, String> {
        match word {
            "mandatory" => Ok(Requirement::Mandatory),
            "optional" => Ok(Requirement::Optional),
            "bad" => Ok(Requirement::Bad),
            other => Err(format!("unknown requirement {other}")),
        }
    }

    /// Parses `mandatory`, `optional`, `bad` or `profile>N?A:B`.
    fn rule_of(text: &str) -> Result<Rule, String> {
        let Some(conditional) = text.strip_prefix("profile>") else {
            return Ok(Rule::Fixed(requirement_of(text)?));
        };
        let Some((level, branches)) = conditional.split_once('?') else {
            return Err(format!("bad rule {text}"));
        };
        let Some((yes, no)) = branches.split_once(':') else {
            return Err(format!("bad rule {text}"));
        };
        let level = level
            .parse::<u8>()
            .map_err(|e| format!("bad level in {text}: {e}"))?;
        Ok(Rule::IfProfileAbove {
            level,
            yes: requirement_of(yes)?,
            no: requirement_of(no)?,
        })
    }

    fn wsc_of(text: &str) -> Result<Option<WscKind>, String> {
        match text {
            "-" => Ok(None),
            "m1" => Ok(Some(WscKind::M1)),
            "m2" => Ok(Some(WscKind::M2)),
            other => Err(format!("unknown wsc column {other}")),
        }
    }

    fn split_line(line: &str) -> Result<Line<'_>, String> {
        let columns: Vec<&str> = line.split('\t').collect();
        if columns.len() != 8 {
            return Err(format!(
                "expected 8 columns, found {}: {line}",
                columns.len()
            ));
        }
        Ok(Line {
            msg_type: columns[0],
            wsc: columns[1],
            table: columns[2],
            row: columns[3],
            tlv_type: columns[4],
            rule: columns[5],
            min_len: columns[6],
            spec: columns[7],
        })
    }

    /// Compares `tables` with the fixture text, table for table and row for row.
    fn compare(tables: &[Table], fixture: &str, classification: &str) -> Result<Counts, String> {
        let mut counts = Counts {
            tables: 0,
            rows: 0,
            empty: 0,
        };
        let mut next_row = 0;
        let mut current: Option<usize> = None;
        let mut previous_name: Option<&str> = None;
        let mut lines = data_lines(fixture).into_iter();
        // The first data line is the column header.
        lines.next();
        for text in lines {
            let line = split_line(text)?;
            let is_new_table = previous_name != Some(line.table);
            if is_new_table {
                let index = current.map_or(0, |i| i + 1);
                let Some(table) = tables.get(index) else {
                    return Err(format!(
                        "the fixture has more tables than TABLES: {}",
                        line.table
                    ));
                };
                check_table_header(table, &line, classification)?;
                if let Some(previous) = current {
                    check_row_count(&tables[previous], next_row)?;
                }
                counts.tables += 1;
                current = Some(index);
                previous_name = Some(line.table);
                next_row = 0;
                if line.row == "-" {
                    counts.empty += 1;
                }
            }
            if line.row == "-" {
                continue;
            }
            let index = current.ok_or("no table")?;
            check_row(&tables[index], next_row, &line, classification)?;
            next_row += 1;
            counts.rows += 1;
        }
        if let Some(previous) = current {
            check_row_count(&tables[previous], next_row)?;
        }
        if counts.tables != tables.len() {
            return Err(format!(
                "TABLES has {} tables, the fixture has {}",
                tables.len(),
                counts.tables
            ));
        }
        Ok(counts)
    }

    fn check_table_header(
        table: &Table,
        line: &Line<'_>,
        classification: &str,
    ) -> Result<(), String> {
        let name = table.upstream_name;
        if name != line.table {
            return Err(format!("table {name}: the fixture has {} here", line.table));
        }
        let msg_type = msg_type_of(classification, line.msg_type)?;
        if table.msg_type != msg_type {
            return Err(format!(
                "table {name}: message type {:?}, fixture {msg_type:?}",
                table.msg_type
            ));
        }
        let wsc = wsc_of(line.wsc)?;
        if table.wsc != wsc {
            return Err(format!(
                "table {name}: wsc {:?}, fixture {wsc:?}",
                table.wsc
            ));
        }
        Ok(())
    }

    fn check_row_count(table: &Table, fixture_rows: usize) -> Result<(), String> {
        if table.rows.len() != fixture_rows {
            return Err(format!(
                "table {}: {} rows, fixture has {fixture_rows}",
                table.upstream_name,
                table.rows.len()
            ));
        }
        Ok(())
    }

    fn check_row(
        table: &Table,
        index: usize,
        line: &Line<'_>,
        classification: &str,
    ) -> Result<(), String> {
        let name = table.upstream_name;
        if line.row.parse::<usize>() != Ok(index) {
            return Err(format!(
                "table {name}: fixture row number {} where {index} was expected",
                line.row
            ));
        }
        let Some(row) = table.rows.get(index) else {
            return Err(format!("table {name}: row {index} is missing from TABLES"));
        };
        let tlv_type = tlv_type_of(classification, line.tlv_type)?;
        if row.tlv_type != tlv_type {
            return Err(format!(
                "table {name} row {index}: TLV {:?}, fixture {tlv_type:?}",
                row.tlv_type
            ));
        }
        let rule = rule_of(line.rule)?;
        if row.rule != rule {
            return Err(format!(
                "table {name} row {index}: rule {:?}, fixture {rule:?}",
                row.rule
            ));
        }
        let min_len = line
            .min_len
            .parse::<u16>()
            .map_err(|e| format!("table {name} row {index}: {e}"))?;
        if row.min_len != min_len {
            return Err(format!(
                "table {name} row {index}: min_len {}, fixture {min_len}",
                row.min_len
            ));
        }
        if row.spec_ref != line.spec {
            return Err(format!(
                "table {name} row {index}: spec {:?}, fixture {:?}",
                row.spec_ref, line.spec
            ));
        }
        Ok(())
    }

    #[test]
    fn tables_equal_the_fixture() {
        let counts = compare(TABLES, FIXTURE, CLASSIFICATION).unwrap();
        assert_eq!(
            counts,
            Counts {
                tables: 70,
                rows: 179,
                empty: 6
            }
        );
        assert_eq!(TABLES.len(), 70);
    }

    #[test]
    fn a_changed_rule_is_found() {
        let changed = FIXTURE.replacen("\tmandatory\t9\ttable 6-8", "\toptional\t9\ttable 6-8", 1);
        assert_ne!(changed, FIXTURE);
        let error = compare(TABLES, &changed, CLASSIFICATION).unwrap_err();
        assert!(
            error.starts_with("table autoconfig_search row 0: rule"),
            "{error}"
        );
    }

    #[test]
    fn a_changed_profile_branch_is_found() {
        let changed = FIXTURE.replacen("profile>1?mandatory:bad", "profile>1?bad:mandatory", 1);
        assert_ne!(changed, FIXTURE);
        let error = compare(TABLES, &changed, CLASSIFICATION).unwrap_err();
        assert!(
            error.starts_with("table autoconfig_search row 5: rule"),
            "{error}"
        );
    }

    #[test]
    fn a_changed_profile_level_is_found() {
        let changed = FIXTURE.replacen("profile>2?optional:bad", "profile>1?optional:bad", 1);
        assert_ne!(changed, FIXTURE);
        let error = compare(TABLES, &changed, CLASSIFICATION).unwrap_err();
        assert!(
            error.starts_with("table autoconfig_search row 6: rule"),
            "{error}"
        );
    }

    #[test]
    fn a_changed_min_len_is_found() {
        let changed =
            FIXTURE.replacen("\tmandatory\t9\ttable 6-8", "\tmandatory\t10\ttable 6-8", 1);
        let error = compare(TABLES, &changed, CLASSIFICATION).unwrap_err();
        assert!(
            error.starts_with("table autoconfig_search row 0: min_len"),
            "{error}"
        );
    }

    #[test]
    fn a_changed_tlv_type_is_found() {
        let changed =
            FIXTURE.replacen("em_tlv_type_al_mac_address", "em_tlv_type_searched_role", 1);
        let error = compare(TABLES, &changed, CLASSIFICATION).unwrap_err();
        assert!(
            error.starts_with("table autoconfig_search row 0: TLV"),
            "{error}"
        );
    }

    #[test]
    fn a_changed_spec_text_is_found() {
        let changed = FIXTURE.replacen("table 6-8 of IEEE-1905-1", "table 6-9 of IEEE-1905-1", 1);
        let error = compare(TABLES, &changed, CLASSIFICATION).unwrap_err();
        assert!(
            error.starts_with("table autoconfig_search row 0: spec"),
            "{error}"
        );
    }

    #[test]
    fn a_dropped_fixture_row_is_found() {
        let mut kept = String::new();
        for line in FIXTURE.lines() {
            if !line.contains("\tautoconfig_search\t6\t") {
                kept.push_str(line);
                kept.push('\n');
            }
        }
        let error = compare(TABLES, &kept, CLASSIFICATION).unwrap_err();
        assert_eq!(error, "table autoconfig_search: 7 rows, fixture has 6");
    }

    #[test]
    fn rule_resolution_compares_the_raw_profile_number() {
        let rule = Rule::IfProfileAbove {
            level: 1,
            yes: Requirement::Mandatory,
            no: Requirement::Bad,
        };
        assert_eq!(rule.resolve(ProfileType(0)), Requirement::Bad);
        assert_eq!(rule.resolve(ProfileType(1)), Requirement::Bad);
        assert_eq!(rule.resolve(ProfileType(2)), Requirement::Mandatory);
        assert_eq!(rule.resolve(ProfileType(4)), Requirement::Mandatory);
        let fixed = Rule::Fixed(Requirement::Optional);
        assert_eq!(fixed.resolve(ProfileType(0)), Requirement::Optional);
    }
}
