// SPDX-License-Identifier: Apache-2.0

//! Device, radio and radio-capability operations of a node.

use super::{MAX_RADIOS, NodeModel};
use crate::limits::{LONG_STRING_CAP, SMALL_STRING_CAP, STRING_CAP};
use crate::model::error::{ModelError, Put, Table};
use crate::records::{DeviceId, DeviceInfo, RadioCapInfo, RadioInfo};
use crate::text::CStrBytes;
use em_types::{Interface, InterfaceName, MacAddr};

/// Result of [`NodeModel::put_radio`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadioPut {
    /// Whether the radio was added or replaced.
    pub put: Put,
    /// Default per-radio policies that could not be seeded because the policy table is full
    /// (`set_policy` rejects them, `dm_easy_mesh.cpp:3411-3414`).
    pub policies_rejected: u8,
}

/// What [`NodeModel::commit_radio`] did with the capability record of the delta (plan D3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadioCapOutcome {
    /// A new radio: the supplied record is stored in the radio's slot.
    Stored,
    /// A new radio and no record supplied: the slot holds a zeroed default
    /// (`dm_easy_mesh.cpp:240-247`).
    Defaulted,
    /// A new radio and a record whose `ruid` is not the radio's MAC. Upstream looks the record up
    /// by the radio's MAC (`get_radio_cap(mac)`, `:240`) and would not find it, so the slot holds
    /// a zeroed default and the supplied record is dropped.
    WrongRadio,
    /// An existing radio and a record supplied: upstream replaces the radio and does not look at
    /// the capability (`:227-233`), so the supplied record is dropped.
    Ignored,
    /// An existing radio and no record supplied: nothing to do.
    NotSupplied,
}

/// Result of [`NodeModel::commit_radio`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadioCommit {
    /// Whether the radio was added or replaced.
    pub radio: Put,
    /// What happened to the capability record.
    pub cap: RadioCapOutcome,
}

impl NodeModel {
    /// `get_device_info` / `get_device` / `get_device_by_ref` (`inc/dm_easy_mesh.h:976, 1103,
    /// 1110`).
    #[must_use]
    pub fn device(&self) -> &DeviceInfo {
        &self.device
    }

    /// The device record, for the engine's direct writes.
    pub fn device_mut(&mut self) -> &mut DeviceInfo {
        &mut self.device
    }

    /// `get_agent_al_interface` (`inc/dm_easy_mesh.h:846`).
    #[must_use]
    pub fn agent_al_interface(&self) -> &Interface {
        &self.device.intf
    }

    /// `get_agent_al_interface_mac` (`inc/dm_easy_mesh.h:857`).
    #[must_use]
    pub fn agent_al_interface_mac(&self) -> MacAddr {
        self.device.intf.mac
    }

    /// `get_agent_al_interface_name` (`inc/dm_easy_mesh.h:868`).
    #[must_use]
    pub fn agent_al_interface_name(&self) -> Option<&InterfaceName> {
        self.device.intf.name.as_ref()
    }

    /// `set_agent_al_interface_mac` (`inc/dm_easy_mesh.h:879`).
    pub fn set_agent_al_interface_mac(&mut self, mac: MacAddr) {
        self.device.intf.mac = mac;
    }

    /// `set_agent_al_interface_name` (`inc/dm_easy_mesh.h:890`).
    pub fn set_agent_al_interface_name(&mut self, name: InterfaceName) {
        self.device.intf.name = Some(name);
    }

    /// `get_manufacturer` (`inc/dm_easy_mesh.h:1934`).
    #[must_use]
    pub fn manufacturer(&self) -> &CStrBytes<LONG_STRING_CAP> {
        &self.device.manufacturer
    }

    /// `set_manufacturer` (`inc/dm_easy_mesh.h:1997`).
    pub fn set_manufacturer(&mut self, manufacturer: CStrBytes<LONG_STRING_CAP>) {
        self.device.manufacturer = manufacturer;
    }

    /// `get_manufacturer_model` (`inc/dm_easy_mesh.h:1943`).
    #[must_use]
    pub fn manufacturer_model(&self) -> &CStrBytes<LONG_STRING_CAP> {
        &self.device.manufacturer_model
    }

    /// `set_manufacturer_model` (`inc/dm_easy_mesh.h:2008`).
    pub fn set_manufacturer_model(&mut self, model: CStrBytes<LONG_STRING_CAP>) {
        self.device.manufacturer_model = model;
    }

    /// `get_software_version` (`inc/dm_easy_mesh.h:1950`).
    #[must_use]
    pub fn software_version(&self) -> &CStrBytes<STRING_CAP> {
        &self.device.software_ver
    }

    /// `set_software_version` (`inc/dm_easy_mesh.h:2019`).
    pub fn set_software_version(&mut self, version: CStrBytes<STRING_CAP>) {
        self.device.software_ver = version;
    }

    /// `get_serial_number` (`inc/dm_easy_mesh.h:1957`).
    #[must_use]
    pub fn serial_number(&self) -> &CStrBytes<LONG_STRING_CAP> {
        &self.device.serial_number
    }

    /// `set_serial_number` (`inc/dm_easy_mesh.h:2032`).
    pub fn set_serial_number(&mut self, serial: CStrBytes<LONG_STRING_CAP>) {
        self.device.serial_number = serial;
    }

    /// `get_primary_device_type` (`inc/dm_easy_mesh.h:1968`).
    #[must_use]
    pub fn primary_device_type(&self) -> &CStrBytes<SMALL_STRING_CAP> {
        &self.device.primary_device_type
    }

    /// `set_primary_device_type` (`inc/dm_easy_mesh.h:2043`).
    pub fn set_primary_device_type(&mut self, device_type: CStrBytes<SMALL_STRING_CAP>) {
        self.device.primary_device_type = device_type;
    }

    /// `find_matching_device` (`dm_easy_mesh.cpp:2420-2427`): the node's device if its AL
    /// interface MAC equals that of `other`.
    #[must_use]
    pub fn find_matching_device(&self, other: &DeviceInfo) -> Option<&DeviceInfo> {
        if self.device.intf.mac == other.intf.mac {
            Some(&self.device)
        } else {
            None
        }
    }

    /// The device insert of `dm_easy_mesh_list_t::put_device` for an existing node
    /// (`dm_easy_mesh_list.cpp:210-232`): the record is replaced whole and then takes `id`, the
    /// identity the caller derived from the key.
    ///
    /// Upstream also does `num_of_devices++` on every call (`:263`), counting repeated puts of
    /// the same device. That count belongs to the network record; the mesh counts distinct
    /// devices.
    pub fn put_device(&mut self, mut device: Box<DeviceInfo>, id: DeviceId) {
        device.id = id;
        self.device = device;
    }

    /// The device update of `dm_device_list_t::update_list` / `dm_easy_mesh_list_t::update_device`
    /// (`dm_easy_mesh_list.cpp:267-304`): the record is replaced, but the runtime metrics
    /// `uptime`, `total_mem`, `free_mem`, `cached_mem`, `cpu_load` and `cpu_temp` keep their
    /// values (`:272-281`), because a config-driven device never carries them.
    pub fn update_device(&mut self, mut device: Box<DeviceInfo>) {
        device.uptime = self.device.uptime;
        device.total_mem = self.device.total_mem;
        device.free_mem = self.device.free_mem;
        device.cached_mem = self.device.cached_mem;
        device.cpu_load = self.device.cpu_load;
        device.cpu_temp = self.device.cpu_temp;
        self.device = device;
    }

    /// The device part of `commit_config` for the `al` target and for `al_insert`
    /// (`dm_easy_mesh.cpp:216-222, 366-372`): the device is replaced, but `is_emplus_agent` and
    /// `extended_info` keep their values.
    pub fn commit_device(&mut self, mut device: Box<DeviceInfo>) {
        std::mem::swap(&mut device.extended_info, &mut self.device.extended_info);
        device.is_emplus_agent = self.device.is_emplus_agent;
        self.device = device;
    }

    // --- radios ---

    /// `m_radio[0..m_num_radios]` in insertion order (`get_radio_info(index)`,
    /// `get_radio_interface(index)`, `get_num_radios`).
    #[must_use]
    pub fn radios(&self) -> &[Box<RadioInfo>] {
        self.radios.as_slice()
    }

    /// `get_radio(index)` (`dm_easy_mesh.cpp:2354-2361`): the radio at `index`. Upstream returns
    /// a slot for any index below `EM_MAX_BANDS`, a zeroed one beyond the count; the port returns
    /// `None` at and beyond the count.
    #[must_use]
    pub fn radio(&self, index: usize) -> Option<&RadioInfo> {
        self.radios.as_slice().get(index).map(|radio| &**radio)
    }

    /// The radio at `index`, for the engine's direct writes (`get_radio_by_ref`).
    ///
    /// Editing `intf.mac` keeps the tables consistent (plan D3): [`NodeModel::remove_radio`]
    /// removes the radio and its capability slot by index, and [`NodeModel::radio_cap`] matches
    /// `cap.ruid.mac`, not the radio's field, as upstream's `get_radio_cap(mac)` does
    /// (`dm_easy_mesh.cpp:2374-2384`).
    pub fn radio_mut(&mut self, index: usize) -> Option<&mut RadioInfo> {
        self.radios
            .as_mut_slice()
            .get_mut(index)
            .map(|radio| &mut **radio)
    }

    /// `get_radio(mac)` (`dm_easy_mesh.cpp:2363-2372`): the radio whose `intf.mac` is `mac`.
    #[must_use]
    pub fn radio_by_mac(&self, mac: MacAddr) -> Option<&RadioInfo> {
        self.radios
            .iter()
            .map(|radio| &**radio)
            .find(|radio| radio.intf.mac == mac)
    }

    /// The radio whose `intf.mac` is `mac`, for the engine's direct writes.
    pub fn radio_by_mac_mut(&mut self, mac: MacAddr) -> Option<&mut RadioInfo> {
        self.radios
            .iter_mut()
            .map(|radio| &mut **radio)
            .find(|radio| radio.intf.mac == mac)
    }

    /// `find_matching_radio` (`dm_easy_mesh.cpp:2401-2410`): the stored radio with the same
    /// `intf.mac` as `other`.
    #[must_use]
    pub fn find_matching_radio(&self, other: &RadioInfo) -> Option<&RadioInfo> {
        self.radio_by_mac(other.intf.mac)
    }

    /// The radio insert-or-replace of `dm_easy_mesh_list_t::put_radio`
    /// (`dm_easy_mesh_list.cpp:399-474`) for an existing node.
    ///
    /// The radio is replaced whole when its `intf.mac` is known, and appended otherwise; at the
    /// capacity of 3 the call fails and changes nothing (upstream returns before any write too,
    /// `:419-422`). Then the default per-radio policies `radio_metrics_rep` and `steering_param`
    /// are seeded for the radio if absent (`:430-466`). The mesh decides the "node needed" result
    /// for the radio (`:470`, plan A6); this method never creates a node.
    ///
    /// # Errors
    ///
    /// [`ModelError::Full`] if the radio is new and 3 radios are stored.
    pub fn put_radio(&mut self, radio: Box<RadioInfo>) -> Result<RadioPut, ModelError> {
        let radio_mac = radio.intf.mac;
        let position = self
            .radios
            .iter()
            .position(|stored| stored.intf.mac == radio_mac);
        let put = if let Some(index) = position {
            self.radios.as_mut_slice()[index] = radio;
            Put::Replaced
        } else {
            self.radios.push(radio).map_err(|_| ModelError::Full {
                table: Table::Radio,
                capacity: MAX_RADIOS,
            })?;
            Put::Inserted
        };
        let policies_rejected = self.seed_radio_policies(radio_mac);
        Ok(RadioPut {
            put,
            policies_rejected,
        })
    }

    /// The radio part of `commit_config` for the `radio` target (`dm_easy_mesh.cpp:223-251`): the
    /// radio with the same `intf.mac` is replaced whole (`:227-233`) and its capability record is
    /// left alone; a record supplied with it is dropped ([`RadioCapOutcome::Ignored`]), as
    /// upstream does not look at it. A new radio is appended (`:239`) with its capability record
    /// at the same index, so the radio and capability tables keep their index parity (plan D3):
    ///
    /// - the supplied `cap` if its `ruid` is the radio's MAC ([`RadioCapOutcome::Stored`]);
    /// - otherwise a zeroed default (`:240-247`, upstream's `m_radio_cap[m_num_radios].init()`),
    ///   with the zero `ruid` that `init()` leaves: [`RadioCapOutcome::Defaulted`] if none was
    ///   supplied, [`RadioCapOutcome::WrongRadio`] if the supplied one is for another radio
    ///   (upstream finds the record by the radio's MAC and would not find it).
    ///
    /// Unlike [`NodeModel::put_radio`] it seeds no policies. At 3 radios a new one is refused
    /// before any write (`:235-238`).
    ///
    /// # Errors
    ///
    /// [`ModelError::Full`] if the radio is new and 3 radios are stored.
    pub fn commit_radio(
        &mut self,
        radio: Box<RadioInfo>,
        cap: Option<Box<RadioCapInfo>>,
    ) -> Result<RadioCommit, ModelError> {
        let position = self
            .radios
            .iter()
            .position(|stored| stored.intf.mac == radio.intf.mac);
        if let Some(index) = position {
            self.radios.as_mut_slice()[index] = radio;
            let cap = if cap.is_some() {
                RadioCapOutcome::Ignored
            } else {
                RadioCapOutcome::NotSupplied
            };
            return Ok(RadioCommit {
                radio: Put::Replaced,
                cap,
            });
        }
        let new_index = self.radios.len();
        if new_index >= MAX_RADIOS {
            return Err(ModelError::Full {
                table: Table::Radio,
                capacity: MAX_RADIOS,
            });
        }
        let (slot, cap_outcome) = match cap {
            Some(cap) if cap.ruid.mac == radio.intf.mac => (cap, RadioCapOutcome::Stored),
            Some(_) => (RadioCapInfo::zeroed(), RadioCapOutcome::WrongRadio),
            None => (RadioCapInfo::zeroed(), RadioCapOutcome::Defaulted),
        };
        self.store_cap_at(new_index, slot)?;
        self.radios.push(radio).map_err(|_| ModelError::Full {
            table: Table::Radio,
            capacity: MAX_RADIOS,
        })?;
        Ok(RadioCommit {
            radio: Put::Inserted,
            cap: cap_outcome,
        })
    }

    /// Stores `cap` at `index` of the capability table, as upstream writes the array slot
    /// `m_radio_cap[index]`: a gap before `index` is filled with zeroed placeholders, and the slot
    /// is overwritten if it exists.
    ///
    /// Slot `index` must be the slot of the radio the capability belongs to, or a slot no stored
    /// radio owns. Both callers satisfy this: [`NodeModel::commit_radio`] writes the slot of the
    /// radio it is about to append (`index == radios.len()`), and
    /// [`NodeModel::put_radio_cap`] writes the slot of a stored radio. No caller parks a
    /// capability for an unstored radio, so the table never holds more slots than radios and a
    /// slot is never taken from another radio.
    fn store_cap_at(&mut self, index: usize, cap: Box<RadioCapInfo>) -> Result<Put, ModelError> {
        let full = |_| ModelError::Full {
            table: Table::RadioCap,
            capacity: MAX_RADIOS,
        };
        while self.radio_caps.len() < index {
            self.radio_caps.push(RadioCapInfo::zeroed()).map_err(full)?;
        }
        if let Some(slot) = self.radio_caps.as_mut_slice().get_mut(index) {
            *slot = cap;
            return Ok(Put::Replaced);
        }
        self.radio_caps.push(cap).map_err(full)?;
        Ok(Put::Inserted)
    }

    /// The radio update of `dm_radio_list_t::update_list` (`dm_radio_list.cpp:146-171`): the
    /// record is replaced, but `radio_temp` keeps its value (`:161-167`), because a config-driven
    /// radio never carries it. Upstream dereferences a missing radio; the port reports it.
    ///
    /// # Errors
    ///
    /// [`ModelError::NotFound`] if no stored radio has the `intf.mac` of `radio`.
    pub fn update_radio(&mut self, mut radio: Box<RadioInfo>) -> Result<(), ModelError> {
        let radio_mac = radio.intf.mac;
        let stored = self
            .radio_by_mac_mut(radio_mac)
            .ok_or(ModelError::NotFound {
                table: Table::Radio,
            })?;
        radio.radio_temp = stored.radio_temp;
        *stored = *radio;
        Ok(())
    }

    /// Removes the radio with `intf.mac` equal to `mac` and the capability slot at its index,
    /// keeping the order of the others. The slot is removed only if it exists: `put_radio`
    /// appends radios without one (plan D3). Upstream's `remove_radio` is empty
    /// (`dm_easy_mesh_list.cpp:394-397`; plan U2): in the port every remove deletes the record.
    pub fn remove_radio(&mut self, mac: MacAddr) -> Option<Box<RadioInfo>> {
        let index = self.radios.iter().position(|radio| radio.intf.mac == mac)?;
        self.remove_radio_at(index)
    }

    /// Removes the radio at `index` and the capability slot at the same index if it exists. The
    /// bulk clear of the mesh removes by position (plan D1b).
    pub(crate) fn remove_radio_at(&mut self, index: usize) -> Option<Box<RadioInfo>> {
        let radio = self.radios.remove(index)?;
        self.radio_caps.remove(index);
        Some(radio)
    }

    // --- radio capabilities ---

    /// `m_radio_cap[]` in insertion order.
    #[must_use]
    pub fn radio_caps(&self) -> &[Box<RadioCapInfo>] {
        self.radio_caps.as_slice()
    }

    /// `get_radio_cap(mac)` (`dm_easy_mesh.cpp:2374-2384`): the capability record whose
    /// `ruid.mac` is `mac`. Upstream looks only at the first `m_num_radios` slots; the port
    /// looks at every stored record (there are at most 3).
    #[must_use]
    pub fn radio_cap(&self, mac: MacAddr) -> Option<&RadioCapInfo> {
        self.radio_caps
            .iter()
            .map(|cap| &**cap)
            .find(|cap| cap.ruid.mac == mac)
    }

    /// `get_radio_cap(index)` and `get_radio_cap_info(index)` (`dm_easy_mesh.cpp:2386-2399`): the
    /// record at `index`, `None` beyond the stored records (upstream: beyond `EM_MAX_BANDS`).
    #[must_use]
    pub fn radio_cap_at(&self, index: usize) -> Option<&RadioCapInfo> {
        self.radio_caps.as_slice().get(index).map(|cap| &**cap)
    }

    /// The capability record whose `ruid.mac` is `mac`, for the engine's direct writes.
    pub fn radio_cap_mut(&mut self, mac: MacAddr) -> Option<&mut RadioCapInfo> {
        self.radio_caps
            .iter_mut()
            .map(|cap| &mut **cap)
            .find(|cap| cap.ruid.mac == mac)
    }

    /// Stores a capability record in the slot of its radio. The capability table keeps the index
    /// parity of upstream (`dm_easy_mesh.cpp:240-247`): the capability of the radio at index `i`
    /// is slot `i`. The slot is written whether it holds that radio's earlier record or the
    /// zeroed placeholder that [`NodeModel::commit_radio`] stored; slots before it that do not
    /// exist yet are filled with placeholders. The radio is found by `cap.ruid.mac`.
    ///
    /// A capability for a radio that is not stored is refused. Upstream has no such write: its
    /// capability arrays are written only together with the radio (`:239-247`), and a record
    /// kept outside the radio's slot would break the parity. Store the radio first.
    ///
    /// # Errors
    ///
    /// [`ModelError::NotFound`] if no stored radio has the MAC `cap.ruid.mac`;
    /// [`ModelError::Full`] if the table cannot hold the slot (not reachable while it has no
    /// more slots than the 3 radios).
    pub fn put_radio_cap(&mut self, cap: Box<RadioCapInfo>) -> Result<Put, ModelError> {
        let index = self
            .radios
            .iter()
            .position(|radio| radio.intf.mac == cap.ruid.mac)
            .ok_or(ModelError::NotFound {
                table: Table::Radio,
            })?;
        self.store_cap_at(index, cap)
    }
}
