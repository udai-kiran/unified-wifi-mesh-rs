// SPDX-License-Identifier: Apache-2.0

//! Op-class operations of a node (ordered array of up to 64 records).

use super::{MAX_OP_CLASSES, NodeModel};
use crate::bounded::BoundedVec;
use crate::enums::OpClassType;
use crate::model::error::{ModelError, Put, Table};
use crate::model::keys::OpClassKey;
use crate::records::{OpClassId, OpClassInfo};
use em_types::MacAddr;

/// Result of [`NodeModel::set_channels_list`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChannelsListOutcome {
    /// Rows written (replaced, reused or appended).
    pub applied: usize,
    /// Rows skipped because the table was full (`dm_easy_mesh.cpp:3475-3478`).
    pub skipped_full: usize,
}

/// One preset op class with a zero `ruid` (`dm_easy_mesh_list.cpp:1700-1716`).
fn preset(
    kind: OpClassType,
    op_class: u32,
    channels: BoundedVec<u32, 64>,
    first_pref: u8,
) -> OpClassInfo {
    let mut channel_pref = [0_u8; 64];
    channel_pref[0] = first_pref;
    OpClassInfo {
        id: OpClassId {
            ruid: MacAddr::ZERO,
            kind,
            op_class,
        },
        op_class,
        channel: 0,
        tx_power: 0,
        max_tx_power: 0,
        channels,
        channel_pref,
        pref_valid: true,
        mins_since_cac_comp: 0,
        sec_remain_non_occ_dur: 0,
        countdown_cac_comp: 0,
    }
}

impl NodeModel {
    /// Seeds the 6 preset op classes through [`NodeModel::set_channels_list`]
    /// (`dm_easy_mesh_list.cpp:1700-1716, 1773`). Their `ruid` is zero in the table; the call
    /// replaces it with the device MAC (`dm_easy_mesh.cpp:3407-3412`).
    pub(super) fn seed_preset_op_classes(&mut self) {
        // Anticipated rows carry the first channel preference 0xe0, scan rows 0
        // (`dm_easy_mesh_list.cpp:1700-1716`).
        let rows = vec![
            preset(
                OpClassType::ANTICIPATED,
                83,
                BoundedVec::from_array([6]),
                0xe0,
            ),
            preset(
                OpClassType::ANTICIPATED,
                128,
                BoundedVec::from_array([42]),
                0xe0,
            ),
            preset(
                OpClassType::ANTICIPATED,
                135,
                BoundedVec::from_array([7]),
                0xe0,
            ),
            preset(
                OpClassType::SCAN_PARAM,
                81,
                BoundedVec::from_array([3, 6, 9]),
                0,
            ),
            preset(
                OpClassType::SCAN_PARAM,
                115,
                BoundedVec::from_array([36, 40, 44, 48, 149, 153, 157, 161, 165]),
                0,
            ),
            preset(OpClassType::SCAN_PARAM, 131, BoundedVec::from_array([1]), 0),
        ];
        self.set_channels_list(rows);
    }

    /// `m_op_class[0..m_num_opclass]` in insertion order.
    #[must_use]
    pub fn op_classes(&self) -> &[OpClassInfo] {
        self.op_classes.as_slice()
    }

    /// `get_op_class_info(index)` / `get_curr_op_class(index)` (`inc/dm_easy_mesh.h:1229`,
    /// `dm_easy_mesh.cpp:2412-2418`): `None` at and beyond the count (upstream: a zeroed slot
    /// below 64, null from 64).
    #[must_use]
    pub fn op_class(&self, index: usize) -> Option<&OpClassInfo> {
        self.op_classes.as_slice().get(index)
    }

    /// The op class at `index`, for the engine's direct writes (`get_op_class_by_ref`).
    pub fn op_class_mut(&mut self, index: usize) -> Option<&mut OpClassInfo> {
        self.op_classes.as_mut_slice().get_mut(index)
    }

    /// The op class with this key.
    #[must_use]
    pub fn find_op_class(&self, key: &OpClassKey) -> Option<&OpClassInfo> {
        self.op_classes
            .iter()
            .find(|info| OpClassKey::of(info) == *key)
    }

    /// The op class with this key, for the engine's direct writes (the CSA delta uses it).
    pub fn find_op_class_mut(&mut self, key: &OpClassKey) -> Option<&mut OpClassInfo> {
        self.op_classes
            .iter_mut()
            .find(|info| OpClassKey::of(info) == *key)
    }

    /// `get_opclass_info_for_bss` (`dm_easy_mesh.cpp:2984-3004`): the first op class whose
    /// `id.ruid` equals `bssid`, and whose `op_class` equals `op_class` when one is given.
    ///
    /// The upstream parameter is named `bssid` but is compared with the op class's `id.ruid`, so
    /// only a BSSID equal to the radio MAC matches (`:2986`). That is kept.
    #[must_use]
    pub fn op_class_info_for_bss(
        &self,
        bssid: MacAddr,
        op_class: Option<u32>,
    ) -> Option<&OpClassInfo> {
        self.op_classes.iter().find(|info| {
            info.id.ruid == bssid && op_class.is_none_or(|wanted| info.op_class == wanted)
        })
    }

    /// The op-class insert-or-replace of `dm_easy_mesh_list_t::put_op_class`
    /// (`dm_easy_mesh_list.cpp:1047-1113`) for the node that owns the op class: replaced when the
    /// [`OpClassKey`] is stored, appended otherwise. At 64 the call fails and changes nothing
    /// (upstream returns -1, `:1105-1108`).
    ///
    /// # Errors
    ///
    /// [`ModelError::Full`] if the op class is new and 64 are stored.
    pub fn put_op_class(&mut self, info: OpClassInfo) -> Result<Put, ModelError> {
        let key = OpClassKey::of(&info);
        let position = self
            .op_classes
            .iter()
            .position(|stored| OpClassKey::of(stored) == key);
        if let Some(index) = position {
            self.op_classes.as_mut_slice()[index] = info;
            return Ok(Put::Replaced);
        }
        self.op_classes.push(info).map_err(|_| ModelError::Full {
            table: Table::OpClass,
            capacity: MAX_OP_CLASSES,
        })?;
        Ok(Put::Inserted)
    }

    /// The op-class part of `commit_config` for the `radio` target (`dm_easy_mesh.cpp:252-283`).
    /// A stored op class is replaced whole if it has the same `op_class` number and `id.kind`
    /// (`:257-259`), or the same `ruid` when the kind is `current` (type 1, `:264-267`); the first stored
    /// one that satisfies either rule wins. Otherwise the op class is appended; at 64 the row is
    /// refused and nothing changes (the max-limit message, `:274-275`; the append, `:276-281`).
    ///
    /// # Errors
    ///
    /// [`ModelError::Full`] if the op class is new and 64 are stored.
    pub fn commit_op_class(&mut self, info: OpClassInfo) -> Result<Put, ModelError> {
        let position = self.op_classes.iter().position(|stored| {
            let same_class = stored.op_class == info.op_class && stored.id.kind == info.id.kind;
            let same_current_radio = stored.id.kind == info.id.kind
                && info.id.kind == OpClassType::CURRENT
                && stored.id.ruid == info.id.ruid;
            same_class || same_current_radio
        });
        if let Some(index) = position {
            self.op_classes.as_mut_slice()[index] = info;
            return Ok(Put::Replaced);
        }
        self.op_classes.push(info).map_err(|_| ModelError::Full {
            table: Table::OpClass,
            capacity: MAX_OP_CLASSES,
        })?;
        Ok(Put::Inserted)
    }

    /// The op-class update of `dm_op_class_list_t::update_list` (db update): replaces the stored
    /// op class with the same key. Upstream dereferences a missing one; the port reports it.
    ///
    /// # Errors
    ///
    /// [`ModelError::NotFound`] if no op class has the key of `info`.
    pub fn update_op_class(&mut self, info: OpClassInfo) -> Result<(), ModelError> {
        let key = OpClassKey::of(&info);
        let stored = self.find_op_class_mut(&key).ok_or(ModelError::NotFound {
            table: Table::OpClass,
        })?;
        *stored = info;
        Ok(())
    }

    /// Removes the op class with this key, keeping the order of the others. Upstream's
    /// `remove_op_class` is empty (`dm_easy_mesh_list.cpp:1172-1175`; plan U2).
    pub fn remove_op_class(&mut self, key: &OpClassKey) -> Option<OpClassInfo> {
        let index = self
            .op_classes
            .iter()
            .position(|stored| OpClassKey::of(stored) == *key)?;
        self.op_classes.remove(index)
    }

    /// Removes the op class at `index`, keeping the order of the others. The bulk clear of the
    /// mesh removes by position, so two rows with the same key are both removed (plan D1b).
    pub(crate) fn remove_op_class_at(&mut self, index: usize) -> Option<OpClassInfo> {
        self.op_classes.remove(index)
    }

    /// `set_channels_list` (`dm_easy_mesh.cpp:3428-3483`). For each input op class, in order:
    ///
    /// 1. An `anticipated` or `scan_param` op class with a zero `ruid` takes the device MAC.
    /// 2. A stored op class with the same `ruid`, `id.kind` and `id.op_class` is replaced.
    /// 3. Otherwise the first stored op class of the same `ruid` and kind whose `pref_valid` is
    ///    false is reused.
    /// 4. Otherwise the op class is appended; at 64 the row is skipped and the next is tried
    ///    (`:3475-3478`).
    ///
    /// Every written row gets `pref_valid = true` (`EM_CH_PREF_ENTRY_VALID`, `inc/em_base.h:217`).
    pub fn set_channels_list(&mut self, op_classes: Vec<OpClassInfo>) -> ChannelsListOutcome {
        let device_mac = self.device.intf.mac;
        let mut outcome = ChannelsListOutcome::default();
        for mut incoming in op_classes {
            let needs_ruid = incoming.id.kind == OpClassType::ANTICIPATED
                || incoming.id.kind == OpClassType::SCAN_PARAM;
            if needs_ruid && incoming.id.ruid.is_zero() {
                incoming.id.ruid = device_mac;
            }
            incoming.pref_valid = true;

            let matching = self.op_classes.iter().position(|stored| {
                stored.id.ruid == incoming.id.ruid
                    && stored.id.kind == incoming.id.kind
                    && stored.id.op_class == incoming.id.op_class
            });
            let reusable = || {
                self.op_classes.iter().position(|stored| {
                    !stored.pref_valid
                        && stored.id.kind == incoming.id.kind
                        && stored.id.ruid == incoming.id.ruid
                })
            };
            if let Some(index) = matching.or_else(reusable) {
                self.op_classes.as_mut_slice()[index] = incoming;
                outcome.applied += 1;
            } else if self.op_classes.push(incoming).is_ok() {
                outcome.applied += 1;
            } else {
                outcome.skipped_full += 1;
            }
        }
        outcome
    }

    /// `update_cac_status_id` (`dm_easy_mesh.cpp:1682-1699`): every op class of a type above
    /// `capability` (CAC and later) takes `al_mac` as its `ruid`.
    pub fn update_cac_status_id(&mut self, al_mac: MacAddr) {
        for info in &mut self.op_classes {
            if info.id.kind.0 > OpClassType::CAPABILITY.0 {
                info.id.ruid = al_mac;
            }
        }
    }
}
