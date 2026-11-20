// SPDX-License-Identifier: Apache-2.0

//! The `get_first_*` and `get_next_*` walks over the whole mesh (`dm_easy_mesh_list.cpp:178-208,
//! 306-363, 514-569, 673-712, 989-1045, 1255-1298, 1385-1426`).
//!
//! Upstream walks `m_list` in hash order and finds the "next" row by comparing pointers into the
//! nodes. The port walks the nodes in key order (plan C3), and a `get_next_*` takes the cursor
//! of the previous row. A cursor that is not a stored row returns `None`, as an unmatched pointer
//! does upstream.
//!
//! The cursor depends on the storage of the table:
//!
//! - Array-backed tables (radios, BSSs, op classes) can hold two rows with the same key: the
//!   commits match on part of the key (`commit_bss` on the BSSID alone, `commit_op_class` on
//!   `(op_class, kind)`) and `update_cac_status_id` rewrites `ruid`. A key cursor would find the
//!   first of the two rows again and loop forever, so these walks take the position
//!   `(node, index)` ([`Positioned`]) and visit each stored row once. Upstream compares the row
//!   pointers, which is the same thing.
//! - Map-backed tables (stations, policies, scan results) take the key the row is stored under,
//!   which the walk yields in [`Located::key`] (plan D1a). The storage key stays unique, whatever
//!   edits the rows' own fields get through the `&mut` accessors, so the walk never repeats a
//!   row; a key rebuilt from the row could.
//! - The network SSIDs are an array (`m_network_ssid[]`), so their walk takes the position
//!   `(network, index)` ([`Positioned`]): within a network in insertion order, as upstream
//!   (`dm_easy_mesh_list.cpp:812-867`), across networks in `NetId` order (plan C3, amended).

use super::{Located, MeshModel, Positioned};
use crate::model::keys::{NodeId, PolicyKey, ScanKey, StaKey};
use crate::model::node::{NodeModel, StaMapTarget};
use crate::records::{BssInfo, DeviceInfo, OpClassInfo, Policy, RadioInfo, ScanResult, StaInfo};
use std::ops::Bound;

impl MeshModel {
    /// Every row of one kind, node by node in key order.
    fn rows<'a, Row, Rows>(
        &'a self,
        rows_of: impl Fn(&'a NodeModel) -> Rows + 'a,
    ) -> impl Iterator<Item = Located<'a, NodeId, Row>> + 'a
    where
        Row: 'a,
        Rows: Iterator<Item = &'a Row> + 'a,
    {
        self.nodes.iter().flat_map(move |(id, node)| {
            rows_of(node).map(move |item| Located {
                at: *id,
                key: (),
                item,
            })
        })
    }

    /// Every row of a map-backed table with its storage key, node by node in key order.
    fn keyed_rows<'a, Key, Row, Rows>(
        &'a self,
        rows_of: impl Fn(&'a NodeModel) -> Rows + 'a,
    ) -> impl Iterator<Item = Located<'a, NodeId, Row, &'a Key>> + 'a
    where
        Key: 'a,
        Row: 'a,
        Rows: Iterator<Item = (&'a Key, &'a Row)> + 'a,
    {
        self.nodes.iter().flat_map(move |(id, node)| {
            rows_of(node).map(move |(key, item)| Located { at: *id, key, item })
        })
    }

    /// Every row of an array-backed table with its position, node by node in key order.
    fn positioned_rows<'a, Row, Rows>(
        &'a self,
        rows_of: impl Fn(&'a NodeModel) -> Rows + 'a,
    ) -> impl Iterator<Item = Positioned<'a, Row>> + 'a
    where
        Row: 'a,
        Rows: Iterator<Item = &'a Row> + 'a,
    {
        self.nodes.iter().flat_map(move |(id, node)| {
            rows_of(node)
                .enumerate()
                .map(move |(index, item)| Positioned {
                    at: *id,
                    index,
                    item,
                })
        })
    }

    /// `get_first_dm` (`inc/dm_easy_mesh_list.h:128`): the first node in key order.
    #[must_use]
    pub fn first_dm(&self) -> Option<Located<'_, NodeId, NodeModel>> {
        self.nodes.iter().next().map(|(id, node)| Located {
            at: *id,
            key: (),
            item: &**node,
        })
    }

    /// `get_next_dm` (`inc/dm_easy_mesh_list.h:144`): the node after `after`. `None` if `after`
    /// is not a node or is the last.
    #[must_use]
    pub fn next_dm(&self, after: &NodeId) -> Option<Located<'_, NodeId, NodeModel>> {
        if !self.nodes.contains_key(after) {
            return None;
        }
        self.nodes
            .range((Bound::Excluded(*after), Bound::Unbounded))
            .next()
            .map(|(id, node)| Located {
                at: *id,
                key: (),
                item: &**node,
            })
    }

    /// `get_first_device` (`dm_easy_mesh_list.cpp:178-188`): the device of the first node.
    #[must_use]
    pub fn first_device(&self) -> Option<Located<'_, NodeId, DeviceInfo>> {
        self.rows(|node| std::iter::once(node.device())).next()
    }

    /// `get_next_device` (`dm_easy_mesh_list.cpp:190-208`): the device of the node after `after`.
    #[must_use]
    pub fn next_device(&self, after: &NodeId) -> Option<Located<'_, NodeId, DeviceInfo>> {
        self.rows(|node| std::iter::once(node.device()))
            .skip_while(|row| row.at != *after)
            .nth(1)
    }

    /// `get_first_radio` (`dm_easy_mesh_list.cpp:306-322`): the first radio of the first node
    /// that has one.
    #[must_use]
    pub fn first_radio(&self) -> Option<Positioned<'_, RadioInfo>> {
        self.positioned_rows(|node| node.radios().iter().map(|radio| &**radio))
            .next()
    }

    /// `get_next_radio` (`dm_easy_mesh_list.cpp:324-363`): the radio after the one at position
    /// `index` of the node `at`, continuing into the following nodes.
    #[must_use]
    pub fn next_radio(&self, at: &NodeId, index: usize) -> Option<Positioned<'_, RadioInfo>> {
        self.positioned_rows(|node| node.radios().iter().map(|radio| &**radio))
            .skip_while(|row| !(row.at == *at && row.index == index))
            .nth(1)
    }

    /// `get_first_bss` (`dm_easy_mesh_list.cpp:514-531`): the first BSS of the first node that
    /// has one.
    #[must_use]
    pub fn first_bss(&self) -> Option<Positioned<'_, BssInfo>> {
        self.positioned_rows(|node| node.bss_list().iter().map(|bss| &**bss))
            .next()
    }

    /// `get_next_bss` (`dm_easy_mesh_list.cpp:533-569`): the BSS after the one at position
    /// `index` of the node `at`, continuing into the following nodes. Two BSSs with the same key
    /// are two rows (`commit_bss` matches the BSSID alone); each is visited once.
    #[must_use]
    pub fn next_bss(&self, at: &NodeId, index: usize) -> Option<Positioned<'_, BssInfo>> {
        self.positioned_rows(|node| node.bss_list().iter().map(|bss| &**bss))
            .skip_while(|row| !(row.at == *at && row.index == index))
            .nth(1)
    }

    /// `get_first_sta` (`dm_easy_mesh_list.cpp:673-688`): the first consolidated station of the
    /// first node that has one, with the key it is stored under.
    #[must_use]
    pub fn first_sta(&self) -> Option<Located<'_, NodeId, StaInfo, &'_ StaKey>> {
        self.keyed_rows(|node| node.sta_entries(StaMapTarget::Consolidated))
            .next()
    }

    /// `get_next_sta` (`dm_easy_mesh_list.cpp:690-712`): the consolidated station after the one
    /// stored under `after` in the node `at`, continuing into the following nodes. `at` and
    /// `after` are the [`Located::at`] and [`Located::key`] of the previous row.
    #[must_use]
    pub fn next_sta(
        &self,
        at: &NodeId,
        after: &StaKey,
    ) -> Option<Located<'_, NodeId, StaInfo, &'_ StaKey>> {
        self.keyed_rows(|node| node.sta_entries(StaMapTarget::Consolidated))
            .skip_while(|row| !(row.at == *at && row.key == after))
            .nth(1)
    }

    /// `get_first_op_class` (`dm_easy_mesh_list.cpp:989-1006`): the first op class of the first
    /// node that has one.
    #[must_use]
    pub fn first_op_class(&self) -> Option<Positioned<'_, OpClassInfo>> {
        self.positioned_rows(|node| node.op_classes().iter()).next()
    }

    /// `get_next_op_class` (`dm_easy_mesh_list.cpp:1008-1045`): the op class after the one at
    /// position `index` of the node `at`, continuing into the following nodes. Two op classes
    /// with the same key are two rows (`commit_op_class`, `update_cac_status_id`); each is
    /// visited once.
    #[must_use]
    pub fn next_op_class(&self, at: &NodeId, index: usize) -> Option<Positioned<'_, OpClassInfo>> {
        self.positioned_rows(|node| node.op_classes().iter())
            .skip_while(|row| !(row.at == *at && row.index == index))
            .nth(1)
    }

    /// `get_first_policy` (`dm_easy_mesh_list.cpp:1255-1275`): the first policy, in key order, of
    /// the first node that has one, with the key it is stored under.
    #[must_use]
    pub fn first_policy(&self) -> Option<Located<'_, NodeId, Policy, &'_ PolicyKey>> {
        self.keyed_rows(NodeModel::policy_entries).next()
    }

    /// `get_next_policy` (`dm_easy_mesh_list.cpp:1277-1298`): the policy after the one stored
    /// under `after` in the node `at`, continuing into the following nodes.
    #[must_use]
    pub fn next_policy(
        &self,
        at: &NodeId,
        after: &PolicyKey,
    ) -> Option<Located<'_, NodeId, Policy, &'_ PolicyKey>> {
        self.keyed_rows(NodeModel::policy_entries)
            .skip_while(|row| !(row.at == *at && row.key == after))
            .nth(1)
    }

    /// `get_first_scan_result` (`dm_easy_mesh_list.cpp:1385-1401`): the first scan result, in key
    /// order, of the first node that has one, with the key it is stored under.
    #[must_use]
    pub fn first_scan_result(&self) -> Option<Located<'_, NodeId, ScanResult, &'_ ScanKey>> {
        self.keyed_rows(NodeModel::scan_result_entries).next()
    }

    /// `get_next_scan_result` (`dm_easy_mesh_list.cpp:1403-1426`): the scan result after the one
    /// stored under `after` in the node `at`, continuing into the following nodes.
    #[must_use]
    pub fn next_scan_result(
        &self,
        at: &NodeId,
        after: &ScanKey,
    ) -> Option<Located<'_, NodeId, ScanResult, &'_ ScanKey>> {
        self.keyed_rows(NodeModel::scan_result_entries)
            .skip_while(|row| !(row.at == *at && row.key == after))
            .nth(1)
    }
}
