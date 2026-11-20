// SPDX-License-Identifier: Apache-2.0

//! The global side maps of `MeshModel` (plan C4): the radio capability list, the IEEE 1905
//! security list and the SSID-to-VID map, with the model residue of their `dm_*_list_t` classes
//! (`update_list`, `delete_list`, the inline `get_first` and `get_next`).

use super::{ListOp, MeshModel};
use crate::limits::SSID_CAP;
use crate::model::error::{ModelError, Table};
use crate::records::{Ieee1905SecurityInfo, RadioCapInfo, Ssid2VidMapInfo};
use crate::text::CStrBytes;
use em_types::MacAddr;
use std::cmp::Ordering;
use std::ops::Bound;

/// Key of the SSID-to-VID map: the SSID and the MAC, upstream's `"ssid@mac"`
/// (`dm_ssid_2_vid_map.cpp:78`). The MAC is the device the JSON entry belongs to; the record does
/// not carry it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ssid2VidKey {
    /// The SSID.
    pub ssid: CStrBytes<SSID_CAP>,
    /// The MAC of the device.
    pub mac: MacAddr,
}

impl PartialOrd for Ssid2VidKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Ssid2VidKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.ssid
            .as_bytes()
            .cmp(other.ssid.as_bytes())
            .then(self.mac.cmp(&other.mac))
    }
}

/// What [`MeshModel::update_ssid_2_vid_map`] did, upstream's `dm_orch_type_t` result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ssid2VidChange {
    /// The same record was stored (`dm_orch_type_none`).
    Unchanged,
    /// A different record was stored under the key (`dm_orch_type_db_update`).
    Updated,
    /// A new record was added (`dm_orch_type_db_insert`).
    Inserted,
}

impl MeshModel {
    // --- radio capability list ---

    /// The record of `dm_radio_cap_list_t::m_list` with this radio MAC
    /// (`dm_radio_cap_list.cpp:120`).
    #[must_use]
    pub fn radio_cap_entry(&self, mac: MacAddr) -> Option<&RadioCapInfo> {
        self.radio_caps.get(&mac).map(|cap| &**cap)
    }

    /// `dm_radio_cap_list_t::get_first` (`inc/dm_radio_cap_list.h:58`): the first record, in MAC
    /// order.
    #[must_use]
    pub fn first_radio_cap_entry(&self) -> Option<&RadioCapInfo> {
        self.radio_caps.values().next().map(|cap| &**cap)
    }

    /// `dm_radio_cap_list_t::get_next` (`inc/dm_radio_cap_list.h:72`): the record after the one
    /// with MAC `after`. `None` if `after` is not stored or is the last.
    #[must_use]
    pub fn next_radio_cap_entry(&self, after: MacAddr) -> Option<&RadioCapInfo> {
        if !self.radio_caps.contains_key(&after) {
            return None;
        }
        self.radio_caps
            .range((Bound::Excluded(after), Bound::Unbounded))
            .next()
            .map(|(_, cap)| &**cap)
    }

    /// `dm_radio_cap_list_t::update_list` (`dm_radio_cap_list.cpp:111-137`), keyed by
    /// `cap.ruid.mac`: insert stores (replacing a record with that key, as `hash_map_put`),
    /// update replaces, delete removes.
    ///
    /// # Errors
    ///
    /// [`ModelError::NotFound`] for an update of a record that is not there; upstream
    /// dereferences null (`:124-127`).
    pub fn update_radio_cap_list(
        &mut self,
        op: ListOp,
        cap: Box<RadioCapInfo>,
    ) -> Result<(), ModelError> {
        let mac = cap.ruid.mac;
        match op {
            ListOp::Insert => {
                self.radio_caps.insert(mac, cap);
            }
            ListOp::Update => {
                let stored = self.radio_caps.get_mut(&mac).ok_or(ModelError::NotFound {
                    table: Table::RadioCapList,
                })?;
                *stored = cap;
            }
            ListOp::Delete => {
                self.radio_caps.remove(&mac);
            }
        }
        Ok(())
    }

    /// `dm_radio_cap_list_t::delete_list` (`dm_radio_cap_list.cpp:139-154`): removes every record
    /// and returns how many there were.
    pub fn delete_radio_cap_list(&mut self) -> usize {
        let removed = self.radio_caps.len();
        self.radio_caps.clear();
        removed
    }

    // --- IEEE 1905 security list ---

    /// The record of `dm_ieee_1905_security_list_t::m_list` with this MAC
    /// (`dm_ieee_1905_security_list.cpp:136`).
    #[must_use]
    pub fn ieee_1905_security_entry(&self, mac: MacAddr) -> Option<&Ieee1905SecurityInfo> {
        self.security_1905.get(&mac)
    }

    /// `dm_ieee_1905_security_list_t::get_first` (`inc/dm_ieee_1905_security_list.h:67`): the
    /// first record, in MAC order.
    #[must_use]
    pub fn first_ieee_1905_security_entry(&self) -> Option<&Ieee1905SecurityInfo> {
        self.security_1905.values().next()
    }

    /// `dm_ieee_1905_security_list_t::get_next` (`inc/dm_ieee_1905_security_list.h:82`): the
    /// record after the one with MAC `after`. `None` if `after` is not stored or is the last.
    #[must_use]
    pub fn next_ieee_1905_security_entry(&self, after: MacAddr) -> Option<&Ieee1905SecurityInfo> {
        if !self.security_1905.contains_key(&after) {
            return None;
        }
        self.security_1905
            .range((Bound::Excluded(after), Bound::Unbounded))
            .next()
            .map(|(_, record)| record)
    }

    /// `dm_ieee_1905_security_list_t::update_list` (`dm_ieee_1905_security_list.cpp:127-154`),
    /// keyed by `record.id`: insert stores, update replaces, delete removes.
    ///
    /// # Errors
    ///
    /// [`ModelError::NotFound`] for an update of a record that is not there; upstream
    /// dereferences null (`:140-141`).
    pub fn update_ieee_1905_security_list(
        &mut self,
        op: ListOp,
        record: Ieee1905SecurityInfo,
    ) -> Result<(), ModelError> {
        let mac = record.id;
        match op {
            ListOp::Insert => {
                self.security_1905.insert(mac, record);
            }
            ListOp::Update => {
                let stored = self
                    .security_1905
                    .get_mut(&mac)
                    .ok_or(ModelError::NotFound {
                        table: Table::Ieee1905SecurityList,
                    })?;
                *stored = record;
            }
            ListOp::Delete => {
                self.security_1905.remove(&mac);
            }
        }
        Ok(())
    }

    /// `dm_ieee_1905_security_list_t::delete_list` (`dm_ieee_1905_security_list.cpp:156-170`):
    /// removes every record and returns how many there were.
    pub fn delete_ieee_1905_security_list(&mut self) -> usize {
        let removed = self.security_1905.len();
        self.security_1905.clear();
        removed
    }

    // --- SSID-to-VID map ---

    /// The record of `dm_ssid_2_vid_map_t::m_list` with this key.
    #[must_use]
    pub fn ssid_2_vid_entry(&self, key: &Ssid2VidKey) -> Option<&Ssid2VidMapInfo> {
        self.ssid_2_vid.get(key)
    }

    /// `dm_ssid_2_vid_map_t::get_first` (`inc/dm_ssid_2_vid_map.h:69`): the first record, in key
    /// order.
    #[must_use]
    pub fn first_ssid_2_vid_entry(&self) -> Option<(Ssid2VidKey, &Ssid2VidMapInfo)> {
        self.ssid_2_vid
            .iter()
            .next()
            .map(|(key, record)| (*key, record))
    }

    /// `dm_ssid_2_vid_map_t::get_next` (`inc/dm_ssid_2_vid_map.h:83`): the record after the one
    /// with this key. `None` if `after` is not stored or is the last.
    #[must_use]
    pub fn next_ssid_2_vid_entry(
        &self,
        after: &Ssid2VidKey,
    ) -> Option<(Ssid2VidKey, &Ssid2VidMapInfo)> {
        if !self.ssid_2_vid.contains_key(after) {
            return None;
        }
        self.ssid_2_vid
            .range((Bound::Excluded(*after), Bound::Unbounded))
            .next()
            .map(|(key, record)| (*key, record))
    }

    /// `dm_ssid_2_vid_map_t::update_list` (`dm_ssid_2_vid_map.cpp:94-123`): the record is keyed by
    /// its SSID and the device MAC `mac`; the same record stored is [`Ssid2VidChange::Unchanged`],
    /// a new one is stored ([`Ssid2VidChange::Inserted`]).
    ///
    /// Intentional difference: for a stored record that differs, upstream returns
    /// `dm_orch_type_db_update` and leaves the stored record as it was (`:113-116`), so the list
    /// disagrees with the database it was just told to update. The port stores the new record
    /// ([`Ssid2VidChange::Updated`]). Upstream also compares the id by prefix (`:101`); the key
    /// here is exact.
    pub fn update_ssid_2_vid_map(&mut self, mac: MacAddr, info: Ssid2VidMapInfo) -> Ssid2VidChange {
        let key = Ssid2VidKey {
            ssid: info.ssid,
            mac,
        };
        match self.ssid_2_vid.get_mut(&key) {
            Some(stored) if *stored == info => Ssid2VidChange::Unchanged,
            Some(stored) => {
                *stored = info;
                Ssid2VidChange::Updated
            }
            None => {
                self.ssid_2_vid.insert(key, info);
                Ssid2VidChange::Inserted
            }
        }
    }
}
