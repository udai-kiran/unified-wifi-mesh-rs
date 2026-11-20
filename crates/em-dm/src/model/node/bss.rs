// SPDX-License-Identifier: Apache-2.0

//! BSS operations of a node. The BSS table is an ordered array upstream (`m_bss[]`, indexed by
//! `get_bss_info(index)`), so it stays ordered here (capacity 24).

use super::{MAX_BSS, NodeModel};
use crate::enums::{HaulType, VapMode};
use crate::model::error::{ModelError, Put, Table};
use crate::model::keys::BssKey;
use crate::records::BssInfo;
use em_types::MacAddr;

impl NodeModel {
    /// `m_bss[0..m_num_bss]` in insertion order (`get_bss_info(index)`, `get_num_bss`).
    #[must_use]
    pub fn bss_list(&self) -> &[Box<BssInfo>] {
        self.bss.as_slice()
    }

    /// `get_bss(index)` / `get_bss_info(index)` (`inc/dm_easy_mesh.h:1337, 1429`): `None` at and
    /// beyond the count (upstream returns a stale or zeroed slot below 24).
    #[must_use]
    pub fn bss(&self, index: usize) -> Option<&BssInfo> {
        self.bss.as_slice().get(index).map(|bss| &**bss)
    }

    /// The BSS at `index`, for the engine's direct writes (`get_bss_by_ref`).
    pub fn bss_mut(&mut self, index: usize) -> Option<&mut BssInfo> {
        self.bss.as_mut_slice().get_mut(index).map(|bss| &mut **bss)
    }

    /// `get_bss(radio, bss)` (`dm_easy_mesh.cpp:2896-2910`): the BSS whose `bssid.mac` is
    /// `bss_mac` and whose `ruid.mac` is `radio_mac`.
    #[must_use]
    pub fn bss_by_radio(&self, radio_mac: MacAddr, bss_mac: MacAddr) -> Option<&BssInfo> {
        self.bss(self.bss_index_by_radio(radio_mac, bss_mac)?)
    }

    /// The position of the BSS that [`NodeModel::bss_by_radio`] finds: the positional form of
    /// upstream `get_bss(ruid, bssid)` (`dm_easy_mesh.cpp:2896-2910`), for the in-place identity
    /// write of `em_configuration.cpp:1625-1638` through [`NodeModel::bss_mut`].
    ///
    /// The match is on the interface MACs (`bssid.mac`, `ruid.mac`), not on the haul type of the
    /// id. A BSS whose `id.haul_type` the engine rewrote is still found here, and
    /// [`NodeModel::find_matching_bss`] finds it by [`BssKey`] with the new haul type, as upstream
    /// `find_matching_bss` does (`:3517-3535`).
    #[must_use]
    pub fn bss_index_by_radio(&self, radio_mac: MacAddr, bss_mac: MacAddr) -> Option<usize> {
        self.bss
            .iter()
            .position(|bss| bss.bssid.mac == bss_mac && bss.ruid.mac == radio_mac)
    }

    /// `get_bss_info_with_mac` (`dm_easy_mesh.cpp:2731-2741`): the first BSS whose `bssid.mac` is
    /// `mac`.
    #[must_use]
    pub fn bss_info_with_mac(&self, mac: MacAddr) -> Option<&BssInfo> {
        self.bss
            .iter()
            .map(|bss| &**bss)
            .find(|bss| bss.bssid.mac == mac)
    }

    /// `find_matching_bss` (`dm_easy_mesh.cpp:3517-3535`): the BSS whose id has this radio, BSSID
    /// and haul type. Upstream also compares the network id by prefix and the device MAC; both
    /// are the node's own here.
    #[must_use]
    pub fn find_matching_bss(&self, key: &BssKey) -> Option<&BssInfo> {
        self.bss
            .iter()
            .map(|bss| &**bss)
            .find(|bss| BssKey::of(bss) == *key)
    }

    /// `get_bsta_bss_info` (`dm_easy_mesh.cpp:2912-2940`): the first enabled backhaul BSS in STA
    /// mode whose radio exists and is enabled.
    #[must_use]
    pub fn bsta_bss_info(&self) -> Option<&BssInfo> {
        self.bss.iter().map(|bss| &**bss).find(|bss| {
            bss.id.haul_type == HaulType::BACKHAUL
                && bss.vap_mode == VapMode::STA
                && self.radio_enabled(bss.ruid.mac)
                && bss.enabled
        })
    }

    /// `get_associated_bsta_bss_info` (`dm_easy_mesh.cpp:2942-2960`): the first enabled
    /// backhaul STA BSS with a non-zero BSSID on an enabled radio; if none, the result of
    /// [`NodeModel::bsta_bss_info`].
    #[must_use]
    pub fn associated_bsta_bss_info(&self) -> Option<&BssInfo> {
        let associated = self.bss.iter().map(|bss| &**bss).find(|bss| {
            bss.id.haul_type == HaulType::BACKHAUL
                && bss.vap_mode == VapMode::STA
                && !bss.bssid.mac.is_zero()
                && self.radio_enabled(bss.ruid.mac)
                && bss.enabled
        });
        associated.or_else(|| self.bsta_bss_info())
    }

    /// `get_backhaul_bss_info` (`dm_easy_mesh.cpp:2962-2982`): the first enabled backhaul BSS in
    /// AP mode whose radio exists and is enabled.
    #[must_use]
    pub fn backhaul_bss_info(&self) -> Option<&BssInfo> {
        self.bss.iter().map(|bss| &**bss).find(|bss| {
            bss.id.haul_type == HaulType::BACKHAUL
                && bss.vap_mode == VapMode::AP
                && self.radio_enabled(bss.ruid.mac)
                && bss.enabled
        })
    }

    /// True if a radio with this `intf.mac` exists and is enabled.
    fn radio_enabled(&self, radio_mac: MacAddr) -> bool {
        self.radio_by_mac(radio_mac)
            .is_some_and(|radio| radio.enabled)
    }

    /// The BSS insert-or-replace of `dm_easy_mesh_list_t::put_bss`
    /// (`dm_easy_mesh_list.cpp:648-671`): the BSS with the same [`BssKey`] is replaced, else the
    /// BSS is appended. Upstream appends past the end of the array (0007 defect 1); the port
    /// refuses at 24 and changes nothing.
    ///
    /// # Errors
    ///
    /// [`ModelError::Full`] if the BSS is new and 24 are stored.
    pub fn put_bss(&mut self, bss: Box<BssInfo>) -> Result<Put, ModelError> {
        let key = BssKey::of(&bss);
        let position = self.bss.iter().position(|stored| BssKey::of(stored) == key);
        if let Some(index) = position {
            self.bss.as_mut_slice()[index] = bss;
            return Ok(Put::Replaced);
        }
        self.bss.push(bss).map_err(|_| ModelError::Full {
            table: Table::Bss,
            capacity: MAX_BSS,
        })?;
        Ok(Put::Inserted)
    }

    /// The BSS update of `dm_bss_list_t::update_list` (`dm_bss_list.cpp`, db update): the BSS
    /// info of the stored BSS with the same [`BssKey`] is replaced, nothing else (the C wrapper
    /// holds only that record).
    ///
    /// # Errors
    ///
    /// [`ModelError::NotFound`] if no such BSS is stored (upstream logs and breaks).
    pub fn update_bss(&mut self, bss: Box<BssInfo>) -> Result<(), ModelError> {
        let key = BssKey::of(&bss);
        let stored = self
            .bss
            .iter_mut()
            .find(|stored| BssKey::of(stored) == key)
            .ok_or(ModelError::NotFound { table: Table::Bss })?;
        *stored = bss;
        Ok(())
    }

    /// The BSS remove of `dm_easy_mesh_list_t::remove_bss` (`dm_easy_mesh_list.cpp:625-646`):
    /// removes the BSS with this key. Upstream matches the BSSID alone (`:641`); the port matches
    /// the whole key (plan C4).
    pub fn remove_bss(&mut self, key: &BssKey) -> Option<Box<BssInfo>> {
        let index = self
            .bss
            .iter()
            .position(|stored| BssKey::of(stored) == *key)?;
        self.remove_bss_by_index(index)
    }

    /// `remove_bss_by_index` (`dm_easy_mesh.cpp:3501-3515`): removes the BSS at `index` and
    /// shifts the later ones left; `None` for an index at or beyond the count.
    ///
    /// Intentional difference: upstream leaves the old last element in the vacated slot, past
    /// the new count (`m_bss[n - 1]` keeps a stale copy); the port has no such slot.
    pub fn remove_bss_by_index(&mut self, index: usize) -> Option<Box<BssInfo>> {
        self.bss.remove(index)
    }

    /// The BSS part of `commit_config` for the `bss` target (`dm_easy_mesh.cpp:285-319`; the BSS loop is `:288-307`, replace `:290-297`, append
    /// `:299-305`): the
    /// BSS with the same `bssid.mac` is replaced whole, else the BSS is appended. Upstream
    /// appends past the end of the array (0007 defect 1); the port refuses at 24 and changes
    /// nothing.
    ///
    /// # Errors
    ///
    /// [`ModelError::Full`] if the BSS is new and 24 are stored.
    pub fn commit_bss(&mut self, bss: Box<BssInfo>) -> Result<Put, ModelError> {
        let position = self
            .bss
            .iter()
            .position(|stored| stored.bssid.mac == bss.bssid.mac);
        if let Some(index) = position {
            self.bss.as_mut_slice()[index] = bss;
            return Ok(Put::Replaced);
        }
        self.bss.push(bss).map_err(|_| ModelError::Full {
            table: Table::Bss,
            capacity: MAX_BSS,
        })?;
        Ok(Put::Inserted)
    }

    /// `commit_bss_config` (`dm_easy_mesh.cpp:328-344`): replaces the stored BSS with the same
    /// radio MAC (`ruid.mac`) and BSSID (`bssid.mac`) by `bss`. Returns whether one was replaced;
    /// a BSS that is not stored is not added. Upstream takes the source by index into another
    /// model and refuses an index of `EM_MAX_BSS_PER_RADIO` or more (`:331-334`); the source is
    /// the argument here.
    pub fn commit_bss_config(&mut self, bss: Box<BssInfo>) -> bool {
        let stored = self
            .bss
            .iter_mut()
            .find(|stored| stored.ruid.mac == bss.ruid.mac && stored.bssid.mac == bss.bssid.mac);
        match stored {
            Some(stored) => {
                *stored = bss;
                true
            }
            None => false,
        }
    }
}
