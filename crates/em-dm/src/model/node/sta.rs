// SPDX-License-Identifier: Apache-2.0

//! Station maps of a node: the consolidated map `m_sta_map`, the pending association map
//! `m_sta_assoc_map` and the disassociation map `m_sta_dassoc_map` (`inc/dm_easy_mesh.h:81-83`).
//!
//! All three are `BTreeMap<StaKey, Box<StaInfo>>` and unbounded, as upstream's hash maps are
//! (plan C6). "First" and "next" iterate in [`StaKey`] order (plan C3); `next` takes the key of
//! the previous row, where upstream takes a pointer to it (`dm_easy_mesh.cpp:3027-3056`).
//!
//! # Frame bodies
//!
//! A station row owns its association frame body. Rows move into the maps; the only place that
//! copies frame bytes is [`copy_frame_f1`], the F1 boundary (plan F1, 0004 R2): upstream keeps one
//! frame per map entry, so a station that is in the pending and the consolidated map holds two.

use super::NodeModel;
use crate::bounded::BoundedVec;
use crate::limits::EM_MAX_FRAME_BODY_LEN;
use crate::model::error::{ModelError, Put, Table};
use crate::model::keys::StaKey;
use crate::records::{AssocStaMldInfo, LinkReport, StaInfo};
use em_types::MacAddr;
use std::collections::BTreeMap;

/// `em_target_sta_map_t` (`inc/em_base.h:2768-2773`): which of the three maps an operation uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StaMapTarget {
    /// `em_target_sta_map_assoc`: the pending association map, a staging queue that the
    /// database flush persists and then deletes (`dm_easy_mesh_ctrl.cpp:6065-6090`).
    Assoc,
    /// `em_target_sta_map_disassoc`.
    Disassoc,
    /// `em_target_sta_map_consolidated`: the model's station table. Upstream's default for any
    /// other value too (`dm_easy_mesh.cpp:3011-3017`).
    Consolidated,
}

/// A row of a station map together with the key it is stored under.
///
/// The key is the storage location, which a later edit of the row's `id`, `bssid` or `radiomac`
/// through [`NodeModel::sta_info_mut`] does not change (plan D1a): the matching `next_*` takes it
/// as the cursor, as upstream's walks take the row pointer (`dm_easy_mesh.cpp:3027-3056`).
#[derive(Debug, Clone, Copy)]
pub struct StaEntry<'a> {
    /// The key the row is stored under.
    pub key: StaKey,
    /// The row.
    pub row: &'a StaInfo,
}

/// A frame body: at most `EM_MAX_FRAME_BODY_LEN` bytes, as in `StaInfo::frame_body`.
pub type FrameBody = BoundedVec<u8, EM_MAX_FRAME_BODY_LEN>;

/// **F1 boundary.** Copies the bytes of an association frame body.
///
/// This is the only function in `em-dm` that duplicates frame bytes (plan user decision F1,
/// option (a); 0004 R2). The only other sanctioned duplication is the whole-row copy of
/// [`copy_sta_row`] (decision F1b). Upstream stores the Client Capability Report frame in the pending entry
/// and in the consolidated entry, and once per link of an MLD station: up to 2N copies of up to
/// 11 426 bytes (`dm_easy_mesh.cpp:4104-4124`, `dm_sta.cpp:671-701`). The port keeps that state
/// and routes every copy through here, so the duplication is named, counted and greppable.
/// Callers: `StaCapUpdate` (both halves, per link), the pending row of
/// [`NodeModel::apply_sta_mld_association`] and the row copy of an association update.
pub(crate) fn copy_frame_f1(frame: &FrameBody) -> FrameBody {
    // Same capacity on both sides: the copy cannot exceed it, so there is no error to handle.
    frame.copy_of()
}

/// A copy of a `Copy` list with the same capacity.
fn copy_list<T: Copy, const CAP: usize>(list: &BoundedVec<T, CAP>) -> BoundedVec<T, CAP> {
    list.copy_of()
}

/// **F1b boundary.** Copies a station row: every field, and the frame body through
/// [`copy_frame_f1`].
///
/// Sanctioned by user decision F1b (2026-11-20; 0004 R2): upstream's
/// `apply_sta_association_row` keeps the consolidated row and puts a full copy into the pending
/// staging map (`dm_easy_mesh.cpp:3966-3989`), so a move is impossible. Only the association path
/// calls it.
///
/// The row models upstream's `dm_sta_t(*sta)` and `copy_sta_info(&dst, &src)` where the source
/// stays in its map (`dm_sta.cpp:704-733`), used when a pending row is built from the
/// consolidated row (`dm_easy_mesh.cpp:3925-3940`).
fn copy_sta_row(src: &StaInfo) -> StaInfo {
    StaInfo {
        id: src.id,
        bssid: src.bssid,
        radiomac: src.radiomac,
        associated: src.associated,
        sta_client_type: src.sta_client_type,
        timestamp: src.timestamp,
        last_ul_rate: src.last_ul_rate,
        last_dl_rate: src.last_dl_rate,
        est_ul_rate: src.est_ul_rate,
        est_dl_rate: src.est_dl_rate,
        last_conn_time: src.last_conn_time,
        retrans_count: src.retrans_count,
        signal_strength: src.signal_strength,
        rcpi: src.rcpi,
        util_tx: src.util_tx,
        util_rx: src.util_rx,
        pkts_tx: src.pkts_tx,
        pkts_rx: src.pkts_rx,
        bytes_tx: src.bytes_tx,
        bytes_rx: src.bytes_rx,
        errors_tx: src.errors_tx,
        errors_rx: src.errors_rx,
        frame_body: copy_frame_f1(&src.frame_body),
        multi_band_cap: src.multi_band_cap,
        beacon_query_sent_time: src.beacon_query_sent_time,
        beacon_report_elem: copy_list(&src.beacon_report_elem),
        delta_ms: src.delta_ms,
        cap: src.cap,
        ht_cap: src.ht_cap,
        vht_cap: src.vht_cap,
        he_cap: src.he_cap,
        wifi6_cap: src.wifi6_cap,
        wifi7_cap: src.wifi7_cap,
        cellular_data_pref: src.cellular_data_pref,
        listen_interval: src.listen_interval,
        ssid: src.ssid,
        supp_rates: src.supp_rates,
        power_cap: src.power_cap,
        supp_channels: src.supp_channels,
        rsn_info: src.rsn_info,
        ext_supp_rates: src.ext_supp_rates,
        supp_op_classes: src.supp_op_classes,
        ext_cap: src.ext_cap,
        rm_cap: src.rm_cap,
        multi_link: src.multi_link,
        vendor_info: copy_list(&src.vendor_info),
        beacon_reports: copy_list(&src.beacon_reports),
        link_stats_report: LinkReport {
            sta_mac: src.link_stats_report.sta_mac,
            reporting_timestamp: src.link_stats_report.reporting_timestamp,
            link_quality_threshold: src.link_stats_report.link_quality_threshold,
            alarm_triggered: src.link_stats_report.alarm_triggered,
            alarm_sample: copy_list(&src.link_stats_report.alarm_sample),
        },
        reason_code: src.reason_code,
    }
}

/// Result of [`NodeModel::merge_assoc_sta_updates`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StaMergeOutcome {
    /// Rows added to the consolidated map.
    pub inserted: usize,
    /// Rows that replaced a consolidated row.
    pub replaced: usize,
    /// Consolidated rows removed because the station disassociated.
    pub removed: usize,
}

impl NodeModel {
    fn sta_table(&self, target: StaMapTarget) -> &BTreeMap<StaKey, Box<StaInfo>> {
        match target {
            StaMapTarget::Assoc => &self.sta_assoc_map,
            StaMapTarget::Disassoc => &self.sta_dassoc_map,
            StaMapTarget::Consolidated => &self.sta_map,
        }
    }

    fn sta_table_mut(&mut self, target: StaMapTarget) -> &mut BTreeMap<StaKey, Box<StaInfo>> {
        match target {
            StaMapTarget::Assoc => &mut self.sta_assoc_map,
            StaMapTarget::Disassoc => &mut self.sta_dassoc_map,
            StaMapTarget::Consolidated => &mut self.sta_map,
        }
    }

    /// The rows of one map in key order.
    pub fn sta_rows(&self, target: StaMapTarget) -> impl Iterator<Item = &StaInfo> {
        self.sta_table(target).values().map(|sta| &**sta)
    }

    /// Number of rows in one map.
    #[must_use]
    pub fn num_sta(&self, target: StaMapTarget) -> usize {
        self.sta_table(target).len()
    }

    /// The rows of one map with their storage keys, in key order.
    pub fn sta_entries(&self, target: StaMapTarget) -> impl Iterator<Item = (&StaKey, &StaInfo)> {
        self.sta_table(target)
            .iter()
            .map(|(key, sta)| (key, &**sta))
    }

    /// `get_first_sta_info` (`dm_easy_mesh.cpp:3006-3025`): the first row of the map, in key order,
    /// with the key it is stored under.
    #[must_use]
    pub fn first_sta_info(&self, target: StaMapTarget) -> Option<StaEntry<'_>> {
        self.sta_entries(target)
            .next()
            .map(|(key, row)| StaEntry { key: *key, row })
    }

    /// `get_next_sta_info` (`dm_easy_mesh.cpp:3027-3056`): the row after the row stored under
    /// `after`, in key order. `after` is the key an earlier `first_sta_info` or `next_sta_info`
    /// returned, not a key derived from the row's fields (plan D1a). `None` if `after` is not in
    /// the map or is the last row (upstream: the pointer is not found, or is the last).
    #[must_use]
    pub fn next_sta_info(&self, target: StaMapTarget, after: &StaKey) -> Option<StaEntry<'_>> {
        let table = self.sta_table(target);
        if !table.contains_key(after) {
            return None;
        }
        table
            .range((
                std::ops::Bound::Excluded(*after),
                std::ops::Bound::Unbounded,
            ))
            .next()
            .map(|(key, row)| StaEntry { key: *key, row })
    }

    /// `get_sta_info` (`dm_easy_mesh.cpp:3148-3181`): the row with this key in the chosen map.
    #[must_use]
    pub fn sta_info(&self, key: &StaKey, target: StaMapTarget) -> Option<&StaInfo> {
        self.sta_table(target).get(key).map(|sta| &**sta)
    }

    /// The row with this key, for the engine's direct writes. Editing a field that makes up the
    /// key (`id`, `bssid`, `radiomac`) does not move the row: it stays under its storage key, as
    /// upstream's pointer storage does (plan D1). The walks and the removals go by the storage
    /// key, so they still visit and remove the row once.
    pub fn sta_info_mut(&mut self, key: &StaKey, target: StaMapTarget) -> Option<&mut StaInfo> {
        self.sta_table_mut(target)
            .get_mut(key)
            .map(|sta| &mut **sta)
    }

    /// `put_sta_info` (`dm_easy_mesh.cpp:3183-3214`): adds `info` to the chosen map unless a row
    /// with its key is there, in which case the stored row is kept (`:3195-3198`). Returns
    /// whether the row was added; a rejected row is dropped.
    pub fn put_sta_info(&mut self, info: Box<StaInfo>, target: StaMapTarget) -> bool {
        let key = StaKey::of(&info);
        let table = self.sta_table_mut(target);
        if table.contains_key(&key) {
            return false;
        }
        table.insert(key, info);
        true
    }

    /// The station put of `dm_easy_mesh_list_t::put_sta` for the node that owns the radio
    /// (`dm_easy_mesh_list.cpp:714-756`): into the consolidated map, replacing the row with the
    /// same key with `copy_sta_info` semantics ([`StaInfo::copy_sta_info`]).
    ///
    /// The frame body is bounded by its type, so the limit of 11 426 bytes cannot be exceeded
    /// here; an over-long frame is refused when the `StaInfo` is built
    /// ([`StaInfo::set_frame_body`], 0007 D4).
    pub fn put_sta(&mut self, info: Box<StaInfo>) -> Put {
        let key = StaKey::of(&info);
        if let Some(stored) = self.sta_map.get_mut(&key) {
            stored.copy_sta_info(*info);
            Put::Replaced
        } else {
            self.sta_map.insert(key, info);
            Put::Inserted
        }
    }

    /// The station update of `dm_sta_list_t::update_list` (db update): the consolidated row with
    /// the key of `info` is replaced with `copy_sta_info` semantics. Upstream dereferences a
    /// missing row; the port reports it.
    ///
    /// # Errors
    ///
    /// [`ModelError::NotFound`] if there is no such row.
    pub fn update_sta(&mut self, info: Box<StaInfo>) -> Result<(), ModelError> {
        let key = StaKey::of(&info);
        let stored = self
            .sta_map
            .get_mut(&key)
            .ok_or(ModelError::NotFound { table: Table::Sta })?;
        stored.copy_sta_info(*info);
        Ok(())
    }

    /// Removes the consolidated row with this key. Upstream's `remove_sta` is empty
    /// (`dm_easy_mesh_list.cpp:758-761`; plan U2).
    pub fn remove_sta(&mut self, key: &StaKey) -> Option<Box<StaInfo>> {
        self.sta_map.remove(key)
    }

    /// Takes every row out of one map, in key order. The database flush of the pending map
    /// persists the rows and then drops them (`dm_easy_mesh_ctrl.cpp:6065-6090`).
    pub fn take_sta_rows(&mut self, target: StaMapTarget) -> Vec<Box<StaInfo>> {
        std::mem::take(self.sta_table_mut(target))
            .into_values()
            .collect()
    }

    /// `find_sta(sta_mac, bssid)` (`dm_easy_mesh.cpp:3082-3096`): the first consolidated row, in
    /// key order, of this station on this BSSID (whatever the radio).
    #[must_use]
    pub fn find_sta(&self, sta: MacAddr, bssid: MacAddr) -> Option<&StaInfo> {
        self.sta_map
            .values()
            .map(|row| &**row)
            .find(|row| row.id == sta && row.bssid == bssid)
    }

    /// `find_sta(sta_mac)` (`dm_easy_mesh.cpp:3098-3112`): the first consolidated row of this
    /// station on any BSS of the node, trying the BSSs in table order.
    #[must_use]
    pub fn find_sta_on_any_bss(&self, sta: MacAddr) -> Option<&StaInfo> {
        self.bss
            .iter()
            .find_map(|bss| self.find_sta(sta, bss.bssid.mac))
    }

    /// `is_sta_associated` (`dm_easy_mesh.cpp:3076-3080`).
    #[must_use]
    pub fn is_sta_associated(&self, bssid: MacAddr, sta: MacAddr) -> bool {
        self.find_sta(sta, bssid).is_some_and(|row| row.associated)
    }

    /// `get_first_sta` (`dm_easy_mesh.cpp:3114-3127`): the first consolidated row of this
    /// station, in key order, with the key it is stored under.
    #[must_use]
    pub fn first_sta(&self, sta: MacAddr) -> Option<StaEntry<'_>> {
        self.sta_map
            .iter()
            .find(|(_, row)| row.id == sta)
            .map(|(key, row)| StaEntry { key: *key, row })
    }

    /// `get_next_sta` (`dm_easy_mesh.cpp:3129-3146`): the next consolidated row of this station
    /// after the row stored under `after`, in key order. `None` if `after` is not a stored key.
    #[must_use]
    pub fn next_sta(&self, sta: MacAddr, after: &StaKey) -> Option<StaEntry<'_>> {
        if !self.sta_map.contains_key(after) {
            return None;
        }
        self.sta_map
            .range((
                std::ops::Bound::Excluded(*after),
                std::ops::Bound::Unbounded,
            ))
            .find(|(_, row)| row.id == sta)
            .map(|(key, row)| StaEntry { key: *key, row })
    }

    /// `has_at_least_one_associated_sta` (`dm_easy_mesh.cpp:3060-3074`).
    #[must_use]
    pub fn has_at_least_one_associated_sta(&self) -> bool {
        self.sta_map.values().any(|row| row.associated)
    }

    /// `get_num_bss_for_associated_sta` (`dm_easy_mesh.cpp:3216-3231`): the number of
    /// consolidated rows of this station (one per BSS it is known on, associated or not).
    #[must_use]
    pub fn num_bss_for_associated_sta(&self, sta: MacAddr) -> usize {
        self.sta_map.values().filter(|row| row.id == sta).count()
    }

    /// `merge_assoc_sta_updates` (`dm_easy_mesh.cpp:4173-4213`) with the source maps given as
    /// values: every row of `assoc` is moved into the consolidated map, replacing the row with
    /// its key (`copy_sta_info` semantics) or added; then every key of `disassoc` that is in the
    /// consolidated map is removed.
    ///
    /// Upstream copies the source rows and leaves them in the source model. Here the `assoc` rows
    /// are moved, and `disassoc` carries only the keys, so the disassociated station records stay
    /// with whoever owns them (plan C1; which side owns the `sta_list` stations is left to the
    /// em-cmd note).
    pub fn merge_assoc_sta_updates(
        &mut self,
        assoc: Vec<Box<StaInfo>>,
        disassoc: &[StaKey],
    ) -> StaMergeOutcome {
        let mut outcome = StaMergeOutcome::default();
        for row in assoc {
            match self.put_sta(row) {
                Put::Inserted => outcome.inserted += 1,
                Put::Replaced => outcome.replaced += 1,
            }
        }
        for key in disassoc {
            if self.sta_map.remove(key).is_some() {
                outcome.removed += 1;
            }
        }
        outcome
    }

    /// `merge_sta_link_metrics` (`dm_easy_mesh.cpp:4215-4227`): every row of `rows` whose key is
    /// in the consolidated map replaces it whole (`copy_sta_info` semantics); a row with an
    /// unknown key is not added. Returns the number of replaced rows.
    pub fn merge_sta_link_metrics(&mut self, rows: Vec<Box<StaInfo>>) -> usize {
        let mut replaced = 0;
        for row in rows {
            if self.update_sta(row).is_ok() {
                replaced += 1;
            }
        }
        replaced
    }

    /// The per-link write of `apply_sta_cap_to_maps` (`dm_easy_mesh.cpp:4104-4124`), the two
    /// halves of plan C2.
    ///
    /// - Pending half: the pending map gets a new row for `key` (`associated`, the key fields and
    ///   the frame), replacing the one with that key whole (`copy_sta_info` semantics, `:4108-4113`).
    /// - Consolidated half: if the consolidated map has the key, only its frame body is replaced
    ///   and `decode_sta_capability` re-derives the IE text fields; every other field is kept
    ///   (`:4119-4123`).
    ///
    /// Both halves take a copy of the frame through [`copy_frame_f1`]. The pending half always
    /// writes a row; returns whether the consolidated half wrote one too.
    pub(crate) fn apply_sta_cap_to_maps(&mut self, key: &StaKey, frame: &FrameBody) -> bool {
        let pending = StaInfo {
            id: key.sta,
            bssid: key.bssid,
            radiomac: key.radio,
            associated: true,
            frame_body: copy_frame_f1(frame),
            ..StaInfo::default()
        };
        self.sta_assoc_map.insert(*key, Box::new(pending));

        if let Some(consolidated) = self.sta_map.get_mut(key) {
            consolidated.frame_body = copy_frame_f1(frame);
            consolidated.decode_sta_capability();
            return true;
        }
        false
    }

    /// `apply_sta_association_row` (`dm_easy_mesh.cpp:3958-3978`), for a row that is only a key
    /// and an `associated` flag.
    ///
    /// If the consolidated map has the row, its `associated` flag is set and the pending map gets
    /// a copy of the whole consolidated row (a new row, or `copy_sta_info` over the pending one).
    /// If it does not, the pending map gets a new row with only the key and the flag, or the
    /// existing pending row has its flag set.
    fn apply_sta_association_row(&mut self, key: &StaKey, associated: bool) {
        if let Some(existing) = self.sta_map.get_mut(key) {
            existing.associated = associated;
            let copy = copy_sta_row(existing);
            match self.sta_assoc_map.get_mut(key) {
                Some(pending) => pending.copy_sta_info(copy),
                None => {
                    self.sta_assoc_map.insert(*key, Box::new(copy));
                }
            }
            return;
        }
        if let Some(pending) = self.sta_assoc_map.get_mut(key) {
            pending.associated = associated;
        } else {
            let row = StaInfo {
                id: key.sta,
                bssid: key.bssid,
                radiomac: key.radio,
                associated,
                ..StaInfo::default()
            };
            self.sta_assoc_map.insert(*key, Box::new(row));
        }
    }

    /// The radio that holds the BSS `bssid`, found through the radios in table order
    /// (`get_bss(get_radio_info(r)->id.ruid, bssid)` loops in `dm_easy_mesh.cpp:4012-4021,
    /// 4062-4071` and `em_ctrl.cpp:337-345`).
    #[must_use]
    pub fn radio_of_bss(&self, bssid: MacAddr) -> Option<MacAddr> {
        self.radios
            .iter()
            .find_map(|radio| self.bss_by_radio(radio.id.ruid, bssid))
            .map(|bss| bss.ruid.mac)
    }

    /// `apply_sta_mld_association` (`dm_easy_mesh.cpp:3991-4052`): the station `info.mac_addr`
    /// associated over the links of the MLD `info`.
    ///
    /// 1. The frame body of the station is found: the first non-empty one among its rows in the
    ///    pending map, then in the consolidated map.
    /// 2. Every consolidated row of the station that is associated is marked not associated and
    ///    staged in the pending map (`apply_sta_association_row` semantics).
    /// 3. For every link whose BSSID resolves to a radio ([`NodeModel::radio_of_bss`]), the row
    ///    (station, BSSID, radio) is staged as associated, and a pending row without a frame body
    ///    takes a copy of the one found in step 1.
    ///
    /// Returns whether any row was updated; upstream then marks `sta_list_update`
    /// (`:4048-4050`). Marking is the caller's (plan C5).
    pub fn apply_sta_mld_association(&mut self, info: &AssocStaMldInfo) -> bool {
        let mut updated = false;
        let station = info.mac_addr;

        let cached = [&self.sta_assoc_map, &self.sta_map]
            .into_iter()
            .flat_map(|table| table.values())
            .find(|row| row.id == station && !row.frame_body.is_empty())
            .map(|row| copy_frame_f1(&row.frame_body));

        // The stored keys, not keys rebuilt from the rows (plan D1b; upstream formats them from the
        // row, `dm_easy_mesh.cpp:3967-3975, 4007-4016`).
        let associated_keys: Vec<StaKey> = self
            .sta_map
            .iter()
            .filter(|(_, row)| row.id == station && row.associated)
            .map(|(key, _)| *key)
            .collect();
        for key in &associated_keys {
            self.apply_sta_association_row(key, false);
            updated = true;
        }

        for link in &info.affiliated_sta {
            let Some(radio) = self.radio_of_bss(link.bssid) else {
                continue;
            };
            let key = StaKey {
                sta: station,
                bssid: link.bssid,
                radio,
            };
            self.apply_sta_association_row(&key, true);
            if let (Some(pending), Some(frame)) = (self.sta_assoc_map.get_mut(&key), &cached) {
                if pending.frame_body.is_empty() {
                    pending.frame_body = copy_frame_f1(frame);
                }
            }
            updated = true;
        }
        updated
    }

    /// `apply_sta_disassociation` (`dm_easy_mesh.cpp:4054-4102`) for a disassociation event of
    /// station `sta` on `bssid`.
    ///
    /// If `sta` is an associated MLD station, every link that resolves to a radio is staged as
    /// not associated. If no link was staged, the row (`sta`, `bssid`, radio) is staged, with the
    /// radio taken from the BSS with that BSSID, or, for an AP MLD address, from the first valid
    /// affiliated AP ([`NodeModel::resolve_ap_mld_to_fallback_ruid`]); otherwise the radio is
    /// zero. Always returns `true`: upstream marks `sta_list_update` in every case (`:4101`);
    /// marking is the caller's (plan C5).
    pub fn apply_sta_disassociation(&mut self, sta: MacAddr, bssid: MacAddr) -> bool {
        let links: Vec<MacAddr> = self
            .assoc_sta_mld
            .iter()
            .find(|mld| mld.mac_addr == sta)
            .map(|mld| mld.affiliated_sta.iter().map(|link| link.bssid).collect())
            .unwrap_or_default();
        let mut updated = false;
        for link_bssid in links {
            if let Some(radio) = self.radio_of_bss(link_bssid) {
                let key = StaKey {
                    sta,
                    bssid: link_bssid,
                    radio,
                };
                self.apply_sta_association_row(&key, false);
                updated = true;
            }
        }
        if !updated {
            let radio = match self.bss_info_with_mac(bssid) {
                Some(bss) => bss.ruid.mac,
                None if self.is_ap_mld_mac(bssid) => self
                    .resolve_ap_mld_to_fallback_ruid(bssid)
                    .unwrap_or(MacAddr::ZERO),
                None => MacAddr::ZERO,
            };
            self.apply_sta_association_row(&StaKey { sta, bssid, radio }, false);
        }
        true
    }
}
