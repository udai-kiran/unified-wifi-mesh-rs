// SPDX-License-Identifier: Apache-2.0

//! The model residue of the `dm_*_list_t` classes: `update_list` and `delete_list`
//! (`src/dm/dm_*_list.cpp`).
//!
//! Upstream's `update_list` formats a key from the record's own id and dispatches on the
//! database operation type; the port reads the ids off the record and calls the matching
//! `MeshModel` operation, so the merge rules (device keep-fields, radio `radio_temp`, policy
//! fallback to insert, BSS info only, exact SSID id) are those of the operations.
//! `delete_list` upstream walks the rows with `get_first`/`get_next` and removes each one; the
//! port removes every row and returns how many it removed.

use super::records::{
    SCAN_RESULT_SELF_INDEX, remove_neighbor_beacon_report, remove_self_beacon_report,
};
use super::{MeshModel, RadioPutOutcome};
use crate::model::error::{ModelError, Table};
use crate::model::keys::{BssKey, NetId, NodeId, OpClassKey, PolicyKey, ScanKey, SsidKey, StaKey};
use crate::model::node::StaMapTarget;
use crate::records::{
    BssInfo, DeviceInfo, NetworkInfo, NetworkSsidInfo, OpClassInfo, Policy, RadioInfo, ScanResult,
    StaInfo,
};
use em_types::MacAddr;

/// `dm_orch_type_db_insert`, `_update` and `_delete`: the three operations of an `update_list`.
/// `dm_orch_type_none` needs no call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListOp {
    /// `dm_orch_type_db_insert`.
    Insert,
    /// `dm_orch_type_db_update`.
    Update,
    /// `dm_orch_type_db_delete`.
    Delete,
}

fn not_found(table: Table) -> ModelError {
    ModelError::NotFound { table }
}

impl MeshModel {
    /// `dm_network_list_t::update_list` (`dm_network_list.cpp:123-146`), keyed by `net.id`:
    /// insert is [`MeshModel::put_network`]; update replaces the record of the stored network
    /// (`:133-137`); delete is [`MeshModel::remove_network`] (a no-op upstream, plan U2).
    ///
    /// # Errors
    ///
    /// The errors of the operations; an update of a network that is not there is
    /// [`ModelError::NotFound`] (upstream dereferences null, `:134-136`).
    pub fn update_network_list(
        &mut self,
        op: ListOp,
        net: Box<NetworkInfo>,
    ) -> Result<(), ModelError> {
        let key = NetId::from(net.id);
        match op {
            ListOp::Insert => {
                self.put_network(key, net)?;
            }
            ListOp::Update => {
                self.network_mut(&key)
                    .ok_or_else(|| not_found(Table::Network))?
                    .set_info(net);
            }
            ListOp::Delete => {
                self.remove_network(&key)?;
            }
        }
        Ok(())
    }

    /// `dm_network_list_t::delete_list` (`dm_network_list.cpp:148-163`): removes every network
    /// that has no nodes and returns how many; a network with nodes is refused, as
    /// [`MeshModel::remove_network`] refuses it.
    pub fn delete_network_list(&mut self) -> usize {
        let keys: Vec<NetId> = self.networks.keys().copied().collect();
        keys.iter()
            .filter(|key| self.remove_network(key).is_ok())
            .count()
    }

    /// `dm_device_list_t::update_list` (`dm_device_list.cpp:146-172`), keyed by `device.id`:
    /// insert is [`MeshModel::put_device`] (`colocated` is the host lookup for a node it
    /// creates), update is [`MeshModel::update_device`], delete is [`MeshModel::remove_device`].
    ///
    /// # Errors
    ///
    /// The errors of the operations.
    pub fn update_device_list(
        &mut self,
        op: ListOp,
        device: Box<DeviceInfo>,
        colocated: bool,
    ) -> Result<(), ModelError> {
        let key = device.id;
        let node = NodeId {
            net: NetId::from(key.net_id),
            al: key.dev_mac,
        };
        match op {
            ListOp::Insert => {
                self.put_device(key, device, colocated)?;
            }
            ListOp::Update => self.update_device(&node, device)?,
            ListOp::Delete => {
                self.remove_device(&node);
            }
        }
        Ok(())
    }

    /// `dm_device_list_t::delete_list` (`dm_device_list.cpp:174-195`): removes the node of every
    /// device and returns how many.
    pub fn delete_device_list(&mut self) -> usize {
        let removed = self.nodes.len();
        self.nodes.clear();
        removed
    }

    /// `dm_radio_list_t::update_list` (`dm_radio_list.cpp:146-179`), keyed by the radio MAC:
    /// insert is [`MeshModel::put_radio`], update is [`MeshModel::update_radio`], delete is
    /// [`MeshModel::remove_radio`].
    ///
    /// Insert returns the [`RadioPutOutcome`] of `put_radio`: the database path reaches
    /// `create_node` too (`sync_db` -> `update_list` -> `put_radio` -> `create_node`,
    /// `dm_radio_list.cpp:146-155, 265-304`, `dm_easy_mesh_list.cpp:470`), and the rejected
    /// policy seeds are part of the same result. The other operations return `None`.
    ///
    /// # Errors
    ///
    /// The errors of the operations.
    pub fn update_radio_list(
        &mut self,
        op: ListOp,
        radio: Box<RadioInfo>,
    ) -> Result<Option<RadioPutOutcome>, ModelError> {
        match op {
            ListOp::Insert => return self.put_radio(radio).map(Some),
            ListOp::Update => self.update_radio(radio)?,
            ListOp::Delete => {
                self.remove_radio(radio.intf.mac);
            }
        }
        Ok(None)
    }

    /// `dm_radio_list_t::delete_list` (`dm_radio_list.cpp:181-199`): removes every radio of
    /// every node, with its capability slot, and returns how many. Each radio is removed from
    /// the node and position being visited, not found again by its MAC (plan D1b,
    /// `intentional-difference:d1-storage-route`: upstream formats the key from the record).
    pub fn delete_radio_list(&mut self) -> usize {
        let mut removed = 0;
        for node in self.nodes.values_mut() {
            while node.remove_radio_at(0).is_some() {
                removed += 1;
            }
        }
        removed
    }

    /// `dm_bss_list_t::update_list` (`dm_bss_list.cpp:149-185`), keyed by `bss.id`: insert is
    /// [`MeshModel::put_bss`], update is [`MeshModel::update_bss`], delete is
    /// [`MeshModel::remove_bss`].
    ///
    /// # Errors
    ///
    /// The errors of the operations.
    pub fn update_bss_list(&mut self, op: ListOp, bss: Box<BssInfo>) -> Result<(), ModelError> {
        match op {
            ListOp::Insert => {
                self.put_bss(bss)?;
            }
            ListOp::Update => self.update_bss(bss)?,
            ListOp::Delete => {
                let node = NodeId {
                    net: NetId::from(bss.id.net_id),
                    al: bss.id.dev_mac,
                };
                self.remove_bss(&node, &BssKey::of(&bss));
            }
        }
        Ok(())
    }

    /// `dm_bss_list_t::delete_list` (`dm_bss_list.cpp:187-207`): removes every BSS of every node
    /// and returns how many.
    pub fn delete_bss_list(&mut self) -> usize {
        let mut removed = 0;
        for node in self.nodes.values_mut() {
            while node.remove_bss_by_index(0).is_some() {
                removed += 1;
            }
        }
        removed
    }

    /// `dm_sta_list_t::update_list` (`dm_sta_list.cpp:148-177`), keyed by the station, BSSID and
    /// radio MAC: insert is [`MeshModel::put_sta`], update is [`MeshModel::update_sta`], delete
    /// is [`MeshModel::remove_sta`].
    ///
    /// # Errors
    ///
    /// The errors of the operations.
    pub fn update_sta_list(&mut self, op: ListOp, sta: Box<StaInfo>) -> Result<(), ModelError> {
        match op {
            ListOp::Insert => {
                self.put_sta(sta)?;
            }
            ListOp::Update => self.update_sta(sta)?,
            ListOp::Delete => {
                self.remove_sta(&StaKey::of(&sta));
            }
        }
        Ok(())
    }

    /// `dm_sta_list_t::delete_list` (`dm_sta_list.cpp:179-200`): removes every consolidated
    /// station of every node and returns how many. Upstream's 2-part key never matches and its
    /// `remove_sta` is empty anyway.
    pub fn delete_sta_list(&mut self) -> usize {
        self.nodes
            .values_mut()
            .map(|node| node.take_sta_rows(StaMapTarget::Consolidated).len())
            .sum()
    }

    /// `dm_op_class_list_t::update_list` (`dm_op_class_list.cpp:214-241`), keyed by `info.id`:
    /// insert is [`MeshModel::put_op_class`], update is [`MeshModel::update_op_class`], delete is
    /// [`MeshModel::remove_op_class`].
    ///
    /// # Errors
    ///
    /// The errors of the operations.
    pub fn update_op_class_list(
        &mut self,
        op: ListOp,
        info: OpClassInfo,
    ) -> Result<(), ModelError> {
        match op {
            ListOp::Insert => {
                self.put_op_class(info)?;
            }
            ListOp::Update => self.update_op_class(info)?,
            ListOp::Delete => {
                self.remove_op_class(&OpClassKey::of(&info));
            }
        }
        Ok(())
    }

    /// `dm_op_class_list_t::delete_list` (`dm_op_class_list.cpp:243-263`): removes every op class
    /// of every node and returns how many. Each op class is removed by position from the node
    /// being visited, so two rows with the same key are both removed (plan D1b,
    /// `intentional-difference:d1-storage-route`).
    pub fn delete_op_class_list(&mut self) -> usize {
        let mut removed = 0;
        for node in self.nodes.values_mut() {
            while node.remove_op_class_at(0).is_some() {
                removed += 1;
            }
        }
        removed
    }

    /// `dm_policy_list_t::update_list` (`dm_policy_list.cpp:223-256`), keyed by `policy.id`:
    /// insert is [`MeshModel::put_policy`], update is [`MeshModel::update_policy`] (falling back
    /// to insert), delete is [`MeshModel::remove_policy`].
    ///
    /// # Errors
    ///
    /// The errors of the operations.
    pub fn update_policy_list(
        &mut self,
        op: ListOp,
        policy: Box<Policy>,
    ) -> Result<(), ModelError> {
        let key = PolicyKey::of(&policy);
        match op {
            ListOp::Insert => {
                self.put_policy(&key, policy)?;
            }
            ListOp::Update => {
                self.update_policy(policy)?;
            }
            ListOp::Delete => {
                self.remove_policy(&key);
            }
        }
        Ok(())
    }

    /// `dm_policy_list_t::delete_list` (`dm_policy_list.cpp:258-276`): removes every policy of
    /// every node and returns how many. The node being visited is cleared of its policies, with no
    /// key rebuilt from the record (plan D1b, `intentional-difference:d1-storage-route`).
    pub fn delete_policy_list(&mut self) -> usize {
        self.nodes
            .values_mut()
            .map(|node| node.clear_policies())
            .sum()
    }

    /// `dm_scan_result_list_t::update_list` (`dm_scan_result_list.cpp:167-205`): insert is
    /// [`MeshModel::put_scan_result`] with the BSSID of `result.neighbor[index]` (zero for
    /// [`SCAN_RESULT_SELF_INDEX`], the null neighbor, `:138`);
    /// update replaces the stored scan result of that key whole (`:190-192`); delete is
    /// [`MeshModel::remove_scan_result`]. Intentional difference (plan D5(b),
    /// `intentional-difference:d5-self-report-cleanup`): a delete at [`SCAN_RESULT_SELF_INDEX`]
    /// removes the zero-BSSID report with the op class and channel of `result`; upstream matches
    /// the zero BSSID alone and removes the first one (`dm_easy_mesh_list.cpp:1493-1520`).
    ///
    /// # Errors
    ///
    /// The errors of the operations; an update of a scan result that is not there is
    /// [`ModelError::NotFound`] (upstream dereferences null). An `index` that is neither a
    /// neighbor of `result` nor [`SCAN_RESULT_SELF_INDEX`] is [`ModelError::NotFound`] for
    /// [`Table::Neighbor`] and nothing changes (upstream reads garbage).
    pub fn update_scan_result_list(
        &mut self,
        op: ListOp,
        result: Box<ScanResult>,
        index: usize,
    ) -> Result<(), ModelError> {
        let key = ScanKey::of(&result.id);
        let bssid = if index == SCAN_RESULT_SELF_INDEX {
            MacAddr::ZERO
        } else {
            result
                .neighbor
                .as_slice()
                .get(index)
                .map(|neighbor| neighbor.bssid)
                .ok_or(ModelError::NotFound {
                    table: Table::Neighbor,
                })?
        };
        match op {
            ListOp::Insert => {
                self.put_scan_result(bssid, result, index)?;
            }
            ListOp::Update => {
                let node = NodeId {
                    net: key.net,
                    al: key.dev_mac,
                };
                let stored = self
                    .nodes
                    .get_mut(&node)
                    .and_then(|node| node.scan_result_mut(&key))
                    .ok_or_else(|| not_found(Table::ScanResult))?;
                *stored = *result;
            }
            ListOp::Delete if index == SCAN_RESULT_SELF_INDEX => {
                // The self report is matched by op class and channel, as the bulk clear does.
                if let Some(node) = self.nodes.get_mut(&NodeId {
                    net: key.net,
                    al: key.dev_mac,
                }) {
                    node.remove_scan_result(&key);
                    remove_self_beacon_report(node, &key);
                }
            }
            ListOp::Delete => {
                self.remove_scan_result(&key, bssid);
            }
        }
        Ok(())
    }

    /// `dm_scan_result_list_t::delete_list` (`dm_scan_result_list.cpp:207-235`): removes every
    /// scan result of every node, and the beacon reports of its station scanner, and returns how
    /// many scan results it removed. Each result is removed once, whether or not it has
    /// neighbors; the neighbor loop only cleans up the beacon reports. Upstream's loop advances
    /// before it reads and then uses freed records (research 7.9); the port does what the loop
    /// intends.
    ///
    /// - Intentional difference (plan D1b, `intentional-difference:d1-storage-route`): each result
    ///   is taken out of the node and map entry that holds it, and its reports are cleaned in that
    ///   node under the key it was stored with. Upstream rebuilds the node key and the scan key
    ///   from the record (`:211-230`). The same applies to the neighbor reports: each is the first
    ///   with the neighbor's BSSID and the op class and channel of the removed result, where
    ///   upstream's BSSID-only match can take the report of another channel (`:1503-1511`).
    /// - Intentional difference (plan D5(b), `intentional-difference:d5-self-report-cleanup`): a
    ///   station scan's self row appends a zero-BSSID report (`put_scan_result` with
    ///   [`SCAN_RESULT_SELF_INDEX`]). For each removed result of a
    ///   station scanner the clear also removes that station's zero-BSSID report with the op class
    ///   and channel of the result, once; the neighbor loop skips zero BSSIDs so that this is the
    ///   only place that removes one. Upstream would remove it only through the zeroed neighbor
    ///   of research 7.13, by BSSID alone, if its loop worked (7.9).
    pub fn delete_scan_result_list(&mut self) -> usize {
        let mut removed = 0;
        for node in self.nodes.values_mut() {
            for (key, result) in node.take_scan_results() {
                removed += 1;
                for neighbor in &result.neighbor {
                    if neighbor.bssid != MacAddr::ZERO {
                        remove_neighbor_beacon_report(node, &key, neighbor.bssid);
                    }
                }
                remove_self_beacon_report(node, &key);
            }
        }
        removed
    }

    /// `dm_network_ssid_list_t::update_list` (`dm_network_ssid_list.cpp:174-191`): insert and
    /// update are both [`MeshModel::put_network_ssid`] in network `net` (`:181`); delete is
    /// [`MeshModel::remove_network_ssid`].
    ///
    /// # Errors
    ///
    /// The errors of the operations.
    pub fn update_network_ssid_list(
        &mut self,
        op: ListOp,
        net: &NetId,
        ssid: Box<NetworkSsidInfo>,
    ) -> Result<(), ModelError> {
        match op {
            ListOp::Insert | ListOp::Update => {
                self.put_network_ssid(net, ssid)?;
            }
            ListOp::Delete => {
                self.remove_network_ssid(net, &SsidKey::of(&ssid));
            }
        }
        Ok(())
    }

    /// `dm_network_ssid_list_t::delete_list` (`dm_network_ssid_list.cpp:193-205`): removes every
    /// SSID of every network and returns how many. Each row is removed by position from the
    /// network being visited, so rows that share an id are all removed (plan D1b,
    /// `intentional-difference:d1-storage-route`).
    pub fn delete_network_ssid_list(&mut self) -> usize {
        let mut removed = 0;
        for network in self.networks.values_mut() {
            while network.remove_network_ssid_at(0).is_some() {
                removed += 1;
            }
        }
        removed
    }
}
