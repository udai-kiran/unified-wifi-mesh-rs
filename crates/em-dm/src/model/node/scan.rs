// SPDX-License-Identifier: Apache-2.0

//! Scan-result operations of a node. Results are keyed by [`ScanKey`] and iterate in key order
//! (plan C3); the upstream index in `get_scan_result(index)` is the position in that order.

use super::NodeModel;
use crate::bounded::BoundedVec;
use crate::limits::LONG_STRING_CAP;
use crate::model::error::Put;
use crate::model::keys::ScanKey;
use crate::records::ScanResult;
use crate::text::CStrBytes;
use std::collections::btree_map::Entry;

impl NodeModel {
    /// The scan results in key order (`hash_map_get_first`/`next` over `m_scan_result_map`).
    pub fn scan_results(&self) -> impl Iterator<Item = &ScanResult> {
        self.scan_results.values().map(|result| &**result)
    }

    /// The scan results with the keys they are stored under, in key order. The walks of the mesh
    /// take these keys as cursors (plan D1a).
    pub fn scan_result_entries(&self) -> impl Iterator<Item = (&ScanKey, &ScanResult)> {
        self.scan_results
            .iter()
            .map(|(key, result)| (key, &**result))
    }

    /// `get_num_scan_results` (`inc/dm_easy_mesh.h:1541`).
    #[must_use]
    pub fn num_scan_results(&self) -> usize {
        self.scan_results.len()
    }

    /// `get_scan_result(index)` (`dm_easy_mesh.cpp:3562-3577`): the result at position `index` in
    /// key order, `None` at and beyond the count.
    #[must_use]
    pub fn scan_result(&self, index: usize) -> Option<&ScanResult> {
        self.scan_results
            .values()
            .nth(index)
            .map(|result| &**result)
    }

    /// `find_matching_scan_result` (`dm_easy_mesh.cpp:3579-3597`): the result with exactly this
    /// key. Upstream compares the network id by prefix (`strncmp` over the shorter string).
    #[must_use]
    pub fn find_matching_scan_result(&self, key: &ScanKey) -> Option<&ScanResult> {
        self.scan_results.get(key).map(|result| &**result)
    }

    /// The result with this key, for the engine's direct writes. Editing the result's `id` does
    /// not move it: it stays under its storage key, as upstream's pointer storage does (plan D1).
    pub fn scan_result_mut(&mut self, key: &ScanKey) -> Option<&mut ScanResult> {
        self.scan_results.get_mut(key).map(|result| &mut **result)
    }

    /// `create_new_scan_result` (`dm_easy_mesh.cpp:3537-3560`): stores an empty result with the
    /// id of `key` and returns it. A result already stored under the key is replaced (upstream's
    /// `hash_map_put` also replaces and leaks the old one). Upstream returns null for a null id;
    /// the port has no null.
    pub fn create_new_scan_result(&mut self, key: ScanKey) -> &mut ScanResult {
        let empty = Box::new(ScanResult {
            id: key.to_id(),
            scan_status: 0,
            timestamp: CStrBytes::<LONG_STRING_CAP>::default(),
            util: 0,
            noise: 0,
            neighbor: BoundedVec::new(),
            aggr_scan_duration: 0,
            scan_type: 0,
        });
        match self.scan_results.entry(key) {
            Entry::Occupied(mut occupied) => {
                occupied.insert(empty);
                occupied.into_mut()
            }
            Entry::Vacant(vacant) => vacant.insert(empty),
        }
    }

    /// Stores `result` under the key of its own id, replacing a result with that key.
    pub fn put_scan_result_record(&mut self, result: Box<ScanResult>) -> Put {
        let key = ScanKey::of(&result.id);
        if self.scan_results.insert(key, result).is_some() {
            Put::Replaced
        } else {
            Put::Inserted
        }
    }

    /// Removes the result with this key.
    pub fn remove_scan_result(&mut self, key: &ScanKey) -> Option<Box<ScanResult>> {
        self.scan_results.remove(key)
    }

    /// Takes every result out of the node with the key it was stored under, in key order. The
    /// bulk clear of the mesh uses it, so each result leaves the node that holds it (plan D1b).
    pub(crate) fn take_scan_results(&mut self) -> Vec<(ScanKey, Box<ScanResult>)> {
        std::mem::take(&mut self.scan_results).into_iter().collect()
    }
}
