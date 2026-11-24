// SPDX-License-Identifier: Apache-2.0

//! Typed dirty set of a node (plan C5): `em_db_cfg_param_t` and `db_cfg_type_t`.
//!
//! Upstream keeps a bit mask and one criteria string per bit (`inc/em_base.h:3343-3346`). Only
//! one caller passes a criterion: `"radio=<mac>"` on `bss_list_delete`
//! (`src/orch/em_orch_ctrl.cpp:544-545`), and the flush then deletes only the matching BSS rows
//! (`src/ctrl/dm_easy_mesh_ctrl.cpp:6003-6010`, `src/dm/dm_bss.cpp:375-392`). The set keeps that
//! one criterion as an `Option<MacAddr>` and no strings.
//!
//! Intentional differences, each tested in `model::tests::dirty`:
//!
//! - Clearing a bit also clears its criterion. Upstream's `reset_db_cfg_type` indexes the
//!   criteria with the bit's value instead of its index (`dm_easy_mesh.cpp:4137-4138`), so the
//!   `radio=` string stays and a later mark with `""` still filters.
//! - The unterminated `strncpy` (`:4157`) and the endless loop on type 0 (`:4160-4171`) cannot
//!   occur: the criterion is not a string, and a type is an enum value.

use em_types::MacAddr;

/// `db_cfg_type_t` (`inc/em_base.h:3316-3341`). Each value carries its upstream bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum DbCfgType {
    /// `db_cfg_type_network_list_update` = 1 << 0.
    NetworkListUpdate = 1 << 0,
    /// `db_cfg_type_network_list_delete` = 1 << 1.
    NetworkListDelete = 1 << 1,
    /// `db_cfg_type_device_list_update` = 1 << 2.
    DeviceListUpdate = 1 << 2,
    /// `db_cfg_type_device_list_delete` = 1 << 3.
    DeviceListDelete = 1 << 3,
    /// `db_cfg_type_radio_list_update` = 1 << 4.
    RadioListUpdate = 1 << 4,
    /// `db_cfg_type_radio_list_delete` = 1 << 5.
    RadioListDelete = 1 << 5,
    /// `db_cfg_type_op_class_list_update` = 1 << 6.
    OpClassListUpdate = 1 << 6,
    /// `db_cfg_type_op_class_list_delete` = 1 << 7.
    OpClassListDelete = 1 << 7,
    /// `db_cfg_type_bss_list_update` = 1 << 8.
    BssListUpdate = 1 << 8,
    /// `db_cfg_type_bss_list_delete` = 1 << 9.
    BssListDelete = 1 << 9,
    /// `db_cfg_type_sta_list_update` = 1 << 10.
    StaListUpdate = 1 << 10,
    /// `db_cfg_type_sta_list_delete` = 1 << 11.
    StaListDelete = 1 << 11,
    /// `db_cfg_type_network_ssid_list_update` = 1 << 12.
    NetworkSsidListUpdate = 1 << 12,
    /// `db_cfg_type_network_ssid_list_delete` = 1 << 13.
    NetworkSsidListDelete = 1 << 13,
    /// `db_cfg_type_radio_cap_list_update` = 1 << 14.
    RadioCapListUpdate = 1 << 14,
    /// `db_cfg_type_radio_cap_list_delete` = 1 << 15.
    RadioCapListDelete = 1 << 15,
    /// `db_cfg_type_1905_security_list_update` = 1 << 16.
    Ieee1905SecurityListUpdate = 1 << 16,
    /// `db_cfg_type_1905_security_list_delete` = 1 << 17.
    Ieee1905SecurityListDelete = 1 << 17,
    /// `db_cfg_type_sta_metrics_update` = 1 << 18.
    StaMetricsUpdate = 1 << 18,
    /// `db_cfg_type_policy_list_update` = 1 << 19.
    PolicyListUpdate = 1 << 19,
    /// `db_cfg_type_policy_list_delete` = 1 << 20.
    PolicyListDelete = 1 << 20,
    /// `db_cfg_type_scan_result_list_update` = 1 << 21.
    ScanResultListUpdate = 1 << 21,
    /// `db_cfg_type_scan_result_list_delete` = 1 << 22.
    ScanResultListDelete = 1 << 22,
}

impl DbCfgType {
    /// Every type, in bit order.
    pub const ALL: [Self; 23] = [
        Self::NetworkListUpdate,
        Self::NetworkListDelete,
        Self::DeviceListUpdate,
        Self::DeviceListDelete,
        Self::RadioListUpdate,
        Self::RadioListDelete,
        Self::OpClassListUpdate,
        Self::OpClassListDelete,
        Self::BssListUpdate,
        Self::BssListDelete,
        Self::StaListUpdate,
        Self::StaListDelete,
        Self::NetworkSsidListUpdate,
        Self::NetworkSsidListDelete,
        Self::RadioCapListUpdate,
        Self::RadioCapListDelete,
        Self::Ieee1905SecurityListUpdate,
        Self::Ieee1905SecurityListDelete,
        Self::StaMetricsUpdate,
        Self::PolicyListUpdate,
        Self::PolicyListDelete,
        Self::ScanResultListUpdate,
        Self::ScanResultListDelete,
    ];

    /// The upstream bit value.
    #[must_use]
    pub const fn bit(self) -> u32 {
        self as u32
    }
}

/// The dirty bits of one node and the one criterion upstream callers pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DirtySet {
    bits: u32,
    bss_delete_radio: Option<MacAddr>,
}

impl DirtySet {
    /// An empty set (`db_cfg_type_none`).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bits: 0,
            bss_delete_radio: None,
        }
    }

    /// Marks `kind` dirty (`set_db_cfg_param`, `dm_easy_mesh.cpp:4142-4158`). It carries no
    /// criterion; use [`DirtySet::mark_bss_delete`] for `bss_list_delete`.
    pub fn mark(&mut self, kind: DbCfgType) {
        self.bits |= kind.bit();
    }

    /// Marks `bss_list_delete`, optionally limited to the BSSs of one radio
    /// (`"radio=<mac>"`, `em_orch_ctrl.cpp:544-545`).
    ///
    /// `None` after an earlier `Some` on the same pending mark keeps the filter, as upstream
    /// does: `strncpy` with the length of the empty criterion copies nothing
    /// (`dm_easy_mesh.cpp:4157`). A mark after [`DirtySet::clear`] starts clean.
    pub fn mark_bss_delete(&mut self, radio: Option<MacAddr>) {
        self.bits |= DbCfgType::BssListDelete.bit();
        if radio.is_some() {
            self.bss_delete_radio = radio;
        }
    }

    /// Clears `kind` and its criterion (`reset_db_cfg_type`, `dm_easy_mesh.cpp:4125-4140`; the
    /// criterion is cleared as well, see the module docs).
    pub fn clear(&mut self, kind: DbCfgType) {
        self.bits &= !kind.bit();
        if kind == DbCfgType::BssListDelete {
            self.bss_delete_radio = None;
        }
    }

    /// True if `kind` is marked (`db_cfg_type_is_set(type)`, `inc/dm_easy_mesh.h:2552`).
    #[must_use]
    pub const fn is_set(&self, kind: DbCfgType) -> bool {
        self.bits & kind.bit() != 0
    }

    /// True if any bit is marked (`db_cfg_type_is_set()`, `inc/dm_easy_mesh.h:2574`).
    #[must_use]
    pub const fn any(&self) -> bool {
        self.bits > 0
    }

    /// The radio filter of `bss_list_delete` (`db_cfg_type_get_criteria`,
    /// `dm_easy_mesh.cpp:4160-4171`); `None` deletes every BSS row of the node.
    #[must_use]
    pub const fn bss_delete_filter(&self) -> Option<MacAddr> {
        self.bss_delete_radio
    }

    /// The raw bit mask (`em_db_cfg_param_t.db_cfg_type`).
    #[must_use]
    pub const fn bits(&self) -> u32 {
        self.bits
    }
}
