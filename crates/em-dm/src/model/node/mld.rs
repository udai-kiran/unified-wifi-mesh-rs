// SPDX-License-Identifier: Apache-2.0

//! MLD operations of a node: AP MLDs, the backhaul-STA MLD and associated-STA MLDs.

use super::{MAX_AP_MLD, MAX_ASSOC_STA_MLD, NodeModel};
use crate::bounded::BoundedVec;
use crate::enums::HaulType;
use crate::model::error::{ModelError, Table};
use crate::records::{AffiliatedApInfo, ApMldInfo, AssocStaMldInfo, BstaMldInfo};
use crate::text::CStrBytes;
use em_types::MacAddr;

/// The first value that is not a haul type: `em_haul_type_max` (`inc/em_base.h`).
const HAUL_TYPE_MAX: u16 = 5;

/// A fresh affiliated AP: the `memset` row of `dm_easy_mesh.cpp:3792`.
fn blank_affiliated_ap() -> AffiliatedApInfo {
    AffiliatedApInfo {
        mac_addr_valid: false,
        link_id_valid: false,
        ruid: crate::records::zeroed_interface(),
        mac_addr: MacAddr::ZERO,
        link_id: 0,
    }
}

impl NodeModel {
    /// `m_ap_mld[0..m_num_ap_mld]` in insertion order.
    #[must_use]
    pub fn ap_mlds(&self) -> &[ApMldInfo] {
        self.ap_mld.as_slice()
    }

    /// `get_ap_mld(index)` / `get_ap_mld_by_ref(index)` (`inc/dm_easy_mesh.h:1635, 1649`): `None`
    /// at and beyond the count. Upstream returns a zeroed slot for any index below 64.
    #[must_use]
    pub fn ap_mld(&self, index: usize) -> Option<&ApMldInfo> {
        self.ap_mld.as_slice().get(index)
    }

    /// The AP MLD at `index`, for the engine's direct writes.
    pub fn ap_mld_mut(&mut self, index: usize) -> Option<&mut ApMldInfo> {
        self.ap_mld.as_mut_slice().get_mut(index)
    }

    /// `get_ap_mld_frm_haul_type` (`dm_easy_mesh.cpp:3629-3644`): the AP MLD of this haul type;
    /// `None` for a haul type at or above `em_haul_type_max`.
    #[must_use]
    pub fn ap_mld_by_haul_type(&self, haul: HaulType) -> Option<&ApMldInfo> {
        if haul.0 >= HAUL_TYPE_MAX {
            return None;
        }
        self.ap_mld.iter().find(|mld| mld.haul_type == haul)
    }

    /// `is_ap_mld_mac` (`dm_easy_mesh.cpp:3646-3666`): true if a valid AP MLD has this MAC.
    #[must_use]
    pub fn is_ap_mld_mac(&self, mac: MacAddr) -> bool {
        self.ap_mld
            .iter()
            .any(|mld| mld.mac_addr_valid && mld.mac_addr == mac)
    }

    /// `resolve_ap_mld_to_fallback_ruid` (`dm_easy_mesh.cpp:3668-3698`): the radio of the first
    /// affiliated AP with a valid MAC of the valid AP MLD that has MAC `ap_mld_mac`. `None` if no
    /// such MLD, or if the first matching MLD has no valid affiliated AP.
    #[must_use]
    pub fn resolve_ap_mld_to_fallback_ruid(&self, ap_mld_mac: MacAddr) -> Option<MacAddr> {
        let mld = self
            .ap_mld
            .iter()
            .find(|mld| mld.mac_addr_valid && mld.mac_addr == ap_mld_mac)?;
        mld.affiliated_ap
            .iter()
            .find(|ap| ap.mac_addr_valid)
            .map(|ap| ap.ruid.mac)
    }

    /// `update_ap_mld_info` (`dm_easy_mesh.cpp:3700-3820`). The MLD of the same haul type is
    /// updated, or a new one is added; AP MLDs are identified by haul type alone (`:3711`).
    ///
    /// - The MLD MAC and SSID are overwritten only when the input has a valid MAC / a non-empty
    ///   SSID (`:3745-3756`); `str`, `nstr`, `emlsr` and `emlmr` always are.
    /// - Each input affiliated AP is matched to a stored one by link MAC when both are valid,
    ///   otherwise by radio MAC (`:3762-3785`); an unmatched one is added, or skipped at 64
    ///   (`:3787-3791`). `ruid` is always overwritten; the link MAC and link id only when valid.
    ///
    /// Returns the positions, in `input.affiliated_ap`, of the affiliated APs skipped because the
    /// MLD already has 64 (`:3791-3795`); an AP that matches a stored one is still updated.
    ///
    /// Intentional differences: a 32-byte SSID is kept whole where upstream copies 31 bytes
    /// (`:3750`, 0007 amendment A1); the "unchanged" early return (`:3737-3740`) is not ported,
    /// because it changes nothing observable.
    ///
    /// # Errors
    ///
    /// [`ModelError::InvalidHaulType`] for a haul type at or above `em_haul_type_max` (`:3706`);
    /// [`ModelError::Full`] if the MLD is new and 64 are stored (`:3727`). The model is unchanged.
    pub fn update_ap_mld_info(&mut self, input: ApMldInfo) -> Result<Vec<usize>, ModelError> {
        if input.haul_type.0 >= HAUL_TYPE_MAX {
            return Err(ModelError::InvalidHaulType(input.haul_type));
        }
        let position = self
            .ap_mld
            .iter()
            .position(|mld| mld.haul_type == input.haul_type);
        let index = if let Some(index) = position {
            index
        } else {
            let fresh = ApMldInfo {
                mac_addr_valid: false,
                ssid: CStrBytes::default(),
                mac_addr: MacAddr::ZERO,
                str: false,
                nstr: false,
                emlsr: false,
                emlmr: false,
                haul_type: HaulType(0),
                affiliated_ap: BoundedVec::new(),
            };
            self.ap_mld.push(fresh).map_err(|_| ModelError::Full {
                table: Table::ApMld,
                capacity: MAX_AP_MLD,
            })?;
            self.ap_mld.len() - 1
        };
        let target = &mut self.ap_mld.as_mut_slice()[index];

        if input.mac_addr_valid {
            target.mac_addr_valid = true;
            target.mac_addr = input.mac_addr;
        }
        if !input.ssid.is_empty() {
            target.ssid = input.ssid;
        }
        target.haul_type = input.haul_type;
        target.str = input.str;
        target.nstr = input.nstr;
        target.emlsr = input.emlsr;
        target.emlmr = input.emlmr;

        let mut skipped = Vec::new();
        for (position, incoming) in input.affiliated_ap.into_iter().enumerate() {
            let matched = target.affiliated_ap.iter().position(|stored| {
                if incoming.mac_addr_valid && stored.mac_addr_valid {
                    stored.mac_addr == incoming.mac_addr
                } else {
                    stored.ruid.mac == incoming.ruid.mac
                }
            });
            let slot = if let Some(slot) = matched {
                slot
            } else {
                if target.affiliated_ap.push(blank_affiliated_ap()).is_err() {
                    skipped.push(position);
                    continue;
                }
                target.affiliated_ap.len() - 1
            };
            let stored = &mut target.affiliated_ap.as_mut_slice()[slot];
            if incoming.mac_addr_valid {
                stored.mac_addr_valid = true;
                stored.mac_addr = incoming.mac_addr;
            }
            if incoming.link_id_valid {
                stored.link_id_valid = true;
                stored.link_id = incoming.link_id;
            }
            stored.ruid = incoming.ruid;
        }
        Ok(skipped)
    }

    /// `remove_ap_mld_info_by_haul_type` (`dm_easy_mesh.cpp:3897-3925`): removes the AP MLD of
    /// this haul type and shifts the later ones left. `None` if there is none or the haul type is
    /// at or above `em_haul_type_max`.
    pub fn remove_ap_mld_info_by_haul_type(&mut self, haul: HaulType) -> Option<ApMldInfo> {
        if haul.0 >= HAUL_TYPE_MAX {
            return None;
        }
        let index = self.ap_mld.iter().position(|mld| mld.haul_type == haul)?;
        self.ap_mld.remove(index)
    }

    /// `is_bsta_mld_present` (`inc/dm_easy_mesh.h:1658`).
    #[must_use]
    pub fn is_bsta_mld_present(&self) -> bool {
        self.bsta_mld_present
    }

    /// Sets `m_bsta_mld_present`. Nothing at the pin writes it (`m_bsta_mld_present` has only
    /// readers), so the engine sets it together with [`NodeModel::bsta_mld_info_mut`].
    pub fn set_bsta_mld_present(&mut self, present: bool) {
        self.bsta_mld_present = present;
    }

    /// `get_bsta_mld_info` (`inc/dm_easy_mesh.h:1679`).
    #[must_use]
    pub fn bsta_mld_info(&self) -> &BstaMldInfo {
        &self.bsta_mld
    }

    /// The backhaul-STA MLD record, for the engine's direct writes.
    ///
    /// Upstream's `update_bsta_mld_info` is an empty `TODO` (`dm_easy_mesh.cpp:3822-3826`) and
    /// is not ported; the only upstream writers of the record are direct field writes.
    pub fn bsta_mld_info_mut(&mut self) -> &mut BstaMldInfo {
        &mut self.bsta_mld
    }

    /// `m_assoc_sta_mld[0..m_num_assoc_sta_mld]` in insertion order.
    #[must_use]
    pub fn assoc_sta_mlds(&self) -> &[AssocStaMldInfo] {
        self.assoc_sta_mld.as_slice()
    }

    /// `is_sta_mld` (`dm_easy_mesh.cpp:3233-3242`): true if an associated-STA MLD has this MAC.
    #[must_use]
    pub fn is_sta_mld(&self, mac: MacAddr) -> bool {
        self.assoc_sta_mld.iter().any(|mld| mld.mac_addr == mac)
    }

    /// `update_assoc_sta_mld_info` (`dm_easy_mesh.cpp:3828-3895`): merges `input` into the entry
    /// with the same STA MLD MAC, or adds one. The top-level fields are overwritten; the
    /// affiliated STAs are merged incrementally by BSSID (a new link is added, a known link gets
    /// the new link address, absent links stay). Returns the number of links skipped because the
    /// entry already has 64.
    ///
    /// # Errors
    ///
    /// [`ModelError::Full`] if the MLD is new and 64 are stored (`:3846-3849`).
    pub fn update_assoc_sta_mld_info(
        &mut self,
        input: &AssocStaMldInfo,
    ) -> Result<usize, ModelError> {
        let position = self
            .assoc_sta_mld
            .iter()
            .position(|mld| mld.mac_addr == input.mac_addr);
        let index = if let Some(index) = position {
            index
        } else {
            let fresh = AssocStaMldInfo {
                mac_addr: MacAddr::ZERO,
                ap_mld_mac_addr: MacAddr::ZERO,
                str: false,
                nstr: false,
                emlsr: false,
                emlmr: false,
                affiliated_sta: BoundedVec::new(),
            };
            self.assoc_sta_mld
                .push(fresh)
                .map_err(|_| ModelError::Full {
                    table: Table::AssocStaMld,
                    capacity: MAX_ASSOC_STA_MLD,
                })?;
            self.assoc_sta_mld.len() - 1
        };
        let target = &mut self.assoc_sta_mld.as_mut_slice()[index];
        target.mac_addr = input.mac_addr;
        target.ap_mld_mac_addr = input.ap_mld_mac_addr;
        target.str = input.str;
        target.nstr = input.nstr;
        target.emlsr = input.emlsr;
        target.emlmr = input.emlmr;

        let mut skipped = 0;
        for link in &input.affiliated_sta {
            let matched = target
                .affiliated_sta
                .iter()
                .position(|stored| stored.bssid == link.bssid);
            match matched {
                Some(slot) => {
                    target.affiliated_sta.as_mut_slice()[slot].link_addr = link.link_addr;
                }
                None => {
                    if target.affiliated_sta.push(*link).is_err() {
                        skipped += 1;
                    }
                }
            }
        }
        Ok(skipped)
    }

    /// `replace_assoc_sta_mld_info` (`dm_easy_mesh.cpp:3927-3954`): replaces the entry with the
    /// same STA MLD MAC whole, or adds one.
    ///
    /// Upstream clamps `num_affiliated_sta` to 64 (`:3930-3934`); the port's record cannot hold
    /// more than 64 links, so the over-count is rejected when the record is built, never clamped.
    ///
    /// # Errors
    ///
    /// [`ModelError::Full`] if the MLD is new and 64 are stored (`:3947-3950`); the row is
    /// skipped.
    pub fn replace_assoc_sta_mld_info(&mut self, info: AssocStaMldInfo) -> Result<(), ModelError> {
        let position = self
            .assoc_sta_mld
            .iter()
            .position(|mld| mld.mac_addr == info.mac_addr);
        if let Some(index) = position {
            self.assoc_sta_mld.as_mut_slice()[index] = info;
            return Ok(());
        }
        self.assoc_sta_mld.push(info).map_err(|_| ModelError::Full {
            table: Table::AssocStaMld,
            capacity: MAX_ASSOC_STA_MLD,
        })
    }
}
