// SPDX-License-Identifier: Apache-2.0

//! Typed keys of the model collections (note 0007 plan C4).
//!
//! Upstream formats every key as a string (`"net@mac"`, `"sta@bssid@radio"`, ...) and matches
//! some of them with `strncmp` over the shorter string, so `"Network1"` matches `"Network10"`
//! (research 7.6). The keys here compare exactly, are `Copy`, and are never formatted, so the
//! 64- and 256-byte truncation of the formatted keys (`dm_easy_mesh_list.cpp:1643-1660`) has no
//! counterpart either.

use crate::enums::{HaulType, OpClassType, PolicyIdType, ScannerType};
use crate::limits::LONG_STRING_CAP;
use crate::records::{BssInfo, NetworkSsidInfo, OpClassInfo, Policy, ScanResultId};
use crate::text::{CStrBytes, StringError, Text};
use em_types::MacAddr;
use std::cmp::Ordering;

/// Network id: the `em_long_string_t` id of `em_network_info_t` and of every key that starts with
/// a network (`"OneWifiMesh"` is the agent's, `GLOBAL_NET_ID`, `inc/dm_easy_mesh.h:45`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NetId(Text<LONG_STRING_CAP>);

impl NetId {
    /// `GLOBAL_NET_ID` (`inc/dm_easy_mesh.h:45`), the network id of every agent.
    pub const GLOBAL: &'static str = "OneWifiMesh";

    /// Creates a network id.
    ///
    /// # Errors
    ///
    /// [`StringError`] if `id` is longer than 127 bytes or contains NUL.
    pub fn new(id: &str) -> Result<Self, StringError> {
        Ok(Self(Text::new(id)?))
    }

    /// The agent's network id, `"OneWifiMesh"`.
    #[must_use]
    pub fn global() -> Self {
        Self(Text::new(Self::GLOBAL).unwrap_or_default())
    }

    /// The id as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// The id as the record field type.
    #[must_use]
    pub fn as_text(&self) -> Text<LONG_STRING_CAP> {
        self.0
    }
}

impl From<Text<LONG_STRING_CAP>> for NetId {
    fn from(text: Text<LONG_STRING_CAP>) -> Self {
        Self(text)
    }
}

impl Default for NetId {
    /// The empty id: a valid input that finds nothing.
    fn default() -> Self {
        Self(Text::default())
    }
}

impl PartialOrd for NetId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for NetId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.as_bytes().cmp(other.0.as_bytes())
    }
}

/// Identity of a node: its network and its AL MAC. The same AL MAC can exist in two networks
/// (upstream key `"net@mac"`, `dm_easy_mesh_list.cpp:1643-1660`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId {
    /// Network the node belongs to.
    pub net: NetId,
    /// AL MAC of the node's device.
    pub al: MacAddr,
}

/// Key of a STA map entry: upstream `"sta@bssid@radio"` (`dm_easy_mesh.cpp:3148-3180`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StaKey {
    /// Station MAC (`em_sta_info_t.id`).
    pub sta: MacAddr,
    /// BSSID the station is on.
    pub bssid: MacAddr,
    /// Radio the BSS is on (`radiomac`).
    pub radio: MacAddr,
}

impl StaKey {
    /// The key of `info`'s own row.
    #[must_use]
    pub fn of(info: &crate::records::StaInfo) -> Self {
        Self {
            sta: info.id,
            bssid: info.bssid,
            radio: info.radiomac,
        }
    }
}

/// Key of a scan result: upstream `"net@dev@scanner@op_class@channel@scanner_type"`
/// (`dm_easy_mesh.cpp:3537-3560`). It is the `em_scan_result_id_t` itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ScanKey {
    /// Network of the scan (the agent's network id for decoded scans).
    pub net: NetId,
    /// AL MAC of the scanning device.
    pub dev_mac: MacAddr,
    /// Radio or station that scanned.
    pub scanner_mac: MacAddr,
    /// Operating class.
    pub op_class: u8,
    /// Channel.
    pub channel: u8,
    /// Scanner kind.
    pub scanner_type: ScannerType,
}

impl ScanKey {
    /// The key of a scan result id.
    #[must_use]
    pub fn of(id: &ScanResultId) -> Self {
        Self {
            net: NetId::from(id.net_id),
            dev_mac: id.dev_mac,
            scanner_mac: id.scanner_mac,
            op_class: id.op_class,
            channel: id.channel,
            scanner_type: id.scanner_type,
        }
    }

    /// The scan result id with this key's fields.
    #[must_use]
    pub fn to_id(&self) -> ScanResultId {
        ScanResultId {
            net_id: self.net.as_text(),
            dev_mac: self.dev_mac,
            scanner_mac: self.scanner_mac,
            op_class: self.op_class,
            channel: self.channel,
            scanner_type: self.scanner_type,
        }
    }
}

/// Key of a policy: upstream `"net@dev@radio@type"` (`dm_easy_mesh.cpp:167-184`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PolicyKey {
    /// Network id.
    pub net: NetId,
    /// Device (AL) MAC.
    pub dev_mac: MacAddr,
    /// Radio MAC; zero for device-wide policies.
    pub radio_mac: MacAddr,
    /// Policy type.
    pub kind: PolicyIdType,
}

impl PolicyKey {
    /// The key of `policy`'s own id.
    #[must_use]
    pub fn of(policy: &Policy) -> Self {
        Self {
            net: NetId::from(policy.id.net_id),
            dev_mac: policy.id.dev_mac,
            radio_mac: policy.id.radio_mac,
            kind: policy.id.kind,
        }
    }
}

/// Key of an op class: upstream `"ruid@type@op_class"` (`dm_op_class_list.cpp`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OpClassKey {
    /// Radio MAC (or the AL MAC for device-scope types).
    pub ruid: MacAddr,
    /// Op class type.
    pub kind: OpClassType,
    /// Op class number.
    pub op_class: u32,
}

impl OpClassKey {
    /// The key of `info`'s own id.
    #[must_use]
    pub fn of(info: &OpClassInfo) -> Self {
        Self {
            ruid: info.id.ruid,
            kind: info.id.kind,
            op_class: info.id.op_class,
        }
    }
}

/// Key of a BSS within one node: the node-scope part of upstream's
/// `"net@dev@ruid@bssid@haul"` (`dm_bss_list.cpp`, `find_matching_bss` at
/// `dm_easy_mesh.cpp:3517-3535`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BssKey {
    /// Radio MAC (`em_bss_id_t.ruid`).
    pub ruid: MacAddr,
    /// BSSID (`em_bss_id_t.bssid`).
    pub bssid: MacAddr,
    /// Haul type.
    pub haul_type: HaulType,
}

impl BssKey {
    /// The key of `bss`'s own id.
    #[must_use]
    pub fn of(bss: &BssInfo) -> Self {
        Self {
            ruid: bss.id.ruid,
            bssid: bss.id.bssid,
            haul_type: bss.id.haul_type,
        }
    }
}

/// Identity of a network SSID row: the full `id` string, matched exactly
/// (`dm_easy_mesh_list.cpp:869-885`; upstream's `remove_network_ssid` matches a prefix,
/// `:908-935`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SsidKey(CStrBytes<LONG_STRING_CAP>);

impl SsidKey {
    /// The key of `ssid`'s own id.
    #[must_use]
    pub fn of(ssid: &NetworkSsidInfo) -> Self {
        Self(ssid.id)
    }

    /// Creates a key from an id string.
    ///
    /// # Errors
    ///
    /// [`StringError`] if `id` is longer than 127 bytes or contains NUL.
    pub fn new(id: &[u8]) -> Result<Self, StringError> {
        Ok(Self(CStrBytes::new(id)?))
    }

    /// The id bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl PartialOrd for SsidKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SsidKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.as_bytes().cmp(other.0.as_bytes())
    }
}
