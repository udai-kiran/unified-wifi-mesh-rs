// SPDX-License-Identifier: Apache-2.0

//! Errors, results and outcomes of the model operations.

use crate::enums::HaulType;
use std::fmt;

/// A bounded table of the model, named in errors and in delta outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Table {
    /// `m_preference[EM_MAX_PLATFORMS]`.
    Preference,
    /// `m_interfaces[EM_MAX_INTERFACES]`.
    Interface,
    /// `m_radio[EM_MAX_BANDS]`.
    Radio,
    /// `m_radio_cap[EM_MAX_BANDS]`.
    RadioCap,
    /// `m_bss[EM_MAX_BSSS]`.
    Bss,
    /// `m_op_class[EM_MAX_OPCLASS]`.
    OpClass,
    /// `m_policy_map` (`EM_MAX_POLICIES`).
    Policy,
    /// `m_ap_mld[EM_MAX_AP_MLD]`.
    ApMld,
    /// `m_assoc_sta_mld[EM_MAX_ASSOC_STA_MLD]`.
    AssocStaMld,
    /// `em_ap_mld_info_t.affiliated_ap[EM_MAX_AP_MLD]`.
    AffiliatedAp,
    /// `em_assoc_sta_mld_info_t.affiliated_sta[EM_MAX_AP_MLD]`.
    AffiliatedSta,
    /// `m_unassoc_sta_metrics[EM_MAX_UNASSOC_STA]`.
    UnassocSta,
    /// `m_network_ssid[EM_MAX_NET_SSIDS]`.
    NetworkSsid,
    /// `m_network_list[EM_MAX_NETWORKS]`.
    Network,
    /// Station rows (consolidated, pending or disassociated map).
    Sta,
    /// Scan results.
    ScanResult,
    /// `em_scan_result_t.neighbor[EM_MAX_NEIGHBORS]`.
    Neighbor,
    /// `wifi_BeaconReport_t beacon_reports[EM_MAX_BEACON_REPORTS_PER_SCAN]` of a station.
    BeaconReport,
    /// Nodes of the mesh.
    Node,
    /// `dm_radio_cap_list_t::m_list`, keyed by radio MAC (`dm_radio_cap_list.cpp:120`).
    RadioCapList,
    /// `dm_ieee_1905_security_list_t::m_list`, keyed by MAC (`dm_ieee_1905_security_list.cpp:136`).
    Ieee1905SecurityList,
    /// `dm_ssid_2_vid_map_t::m_list`, keyed by SSID and MAC (`dm_ssid_2_vid_map.cpp:78`).
    Ssid2VidMap,
}

/// Why a model operation changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelError {
    /// An insert at the capacity of the table (plan C6). The model is unchanged.
    Full {
        /// The table that is full.
        table: Table,
        /// Its capacity.
        capacity: usize,
    },
    /// An update or remove of a row that is not there.
    NotFound {
        /// The table that was searched.
        table: Table,
    },
    /// An AP MLD haul type that is not below `em_haul_type_max`
    /// (`dm_easy_mesh.cpp:3706-3709`).
    InvalidHaulType(HaulType),
    /// A network that still has nodes cannot be removed (plan C7); remove the nodes first.
    NetworkInUse {
        /// How many nodes the network has.
        nodes: usize,
    },
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full { table, capacity } => {
                write!(f, "{table:?} table is full (capacity {capacity})")
            }
            Self::NotFound { table } => write!(f, "row not found in the {table:?} table"),
            Self::InvalidHaulType(haul) => write!(f, "invalid haul type {}", haul.0),
            Self::NetworkInUse { nodes } => {
                write!(
                    f,
                    "the network still has {nodes} node(s); remove them first"
                )
            }
        }
    }
}

impl std::error::Error for ModelError {}

/// What an upsert did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Put {
    /// A new row was added.
    Inserted,
    /// An existing row was replaced.
    Replaced,
}
