// SPDX-License-Identifier: Apache-2.0

//! BSS, STA, op class, policy and scan result operations of `MeshModel`
//! (`dm_easy_mesh_list.cpp:514-811, 989-1611`).

use super::{Located, MeshModel, Positioned};
use crate::enums::{OpClassType, ScannerType};
use crate::model::error::{ModelError, Put, Table};
use crate::model::keys::{BssKey, NetId, NodeId, OpClassKey, PolicyKey, ScanKey, StaKey};
use crate::model::node::{NodeModel, StaMapTarget};
use crate::records::{
    BeaconReport, BssInfo, OpClassInfo, Policy, ScanResult, StaInfo, WideBwChannel,
};
use em_types::MacAddr;

/// `scan_result_self_index` (`inc/dm_scan_result_list.h:27`): the neighbor index that means "the
/// scan result itself", with a null neighbor.
pub const SCAN_RESULT_SELF_INDEX: usize = 0xFFF;

/// `EM_MAX_BEACON_REPORTS_PER_SCAN` (`inc/em_base.h`): the beacon reports of one station.
const MAX_BEACON_REPORTS: usize = 10;

fn node_not_found() -> ModelError {
    ModelError::NotFound { table: Table::Node }
}

fn node_of_bss(bss: &BssInfo) -> NodeId {
    NodeId {
        net: bss.id.net_id.into(),
        al: bss.id.dev_mac,
    }
}

impl MeshModel {
    // --- BSS ---

    /// `get_bss(key)` (`dm_easy_mesh_list.cpp:571-586`): the BSS with this key in the node `id`
    /// (`find_matching_bss`).
    #[must_use]
    pub fn get_bss(&self, id: &NodeId, key: &BssKey) -> Option<&BssInfo> {
        self.nodes.get(id)?.find_matching_bss(key)
    }

    /// `get_first_bss(al)` (`dm_easy_mesh_list.cpp:588-598`): the first BSS of the agent with AL
    /// MAC `al` in the global network. Upstream hard-codes `GLOBAL_NET_ID` (`:592`); that is kept,
    /// because this overload exists for the agent's single network.
    #[must_use]
    pub fn first_bss_of_agent(&self, al: MacAddr) -> Option<Positioned<'_, BssInfo>> {
        self.bss_of_agent_at(al, 0)
    }

    /// `get_next_bss(al, bss)` (`dm_easy_mesh_list.cpp:600-623`): the BSS after the one at
    /// position `index` in the node `(global network, al)` (`:606`). `None` if `index` is not a
    /// stored row or is last. The cursor is the position, not the key, so two BSSs with the same
    /// key are both visited.
    #[must_use]
    pub fn next_bss_of_agent(&self, al: MacAddr, index: usize) -> Option<Positioned<'_, BssInfo>> {
        let node = self.nodes.get(&NodeId {
            net: NetId::global(),
            al,
        })?;
        node.bss(index)?;
        self.bss_of_agent_at(al, index + 1)
    }

    fn bss_of_agent_at(&self, al: MacAddr, index: usize) -> Option<Positioned<'_, BssInfo>> {
        let at = NodeId {
            net: NetId::global(),
            al,
        };
        let item = self.nodes.get(&at)?.bss(index)?;
        Some(Positioned { at, index, item })
    }

    /// `put_bss` (`dm_easy_mesh_list.cpp:648-671`): stores `bss` in the node
    /// `(bss.id.net_id, bss.id.dev_mac)`, replacing the BSS with the same key
    /// ([`NodeModel::put_bss`]). Upstream takes the key as a string; the port derives it from
    /// the record, which is how the database path builds it (`dm_bss_list.cpp:159`).
    ///
    /// # Errors
    ///
    /// [`ModelError::NotFound`] if the node does not exist (upstream returns, `:657-660`);
    /// [`ModelError::Full`] if the BSS is new and 24 are stored (upstream appends past the
    /// array, `:666-667`).
    pub fn put_bss(&mut self, bss: Box<BssInfo>) -> Result<Put, ModelError> {
        self.nodes
            .get_mut(&node_of_bss(&bss))
            .ok_or_else(node_not_found)?
            .put_bss(bss)
    }

    /// `remove_bss` (`dm_easy_mesh_list.cpp:625-646`): removes the first BSS with this key from the node
    /// `id`. Upstream matches the BSSID alone (`:641`); the port matches the whole key. If two
    /// stored rows share the key, only the first is removed (upstream also stops at the first
    /// match).
    pub fn remove_bss(&mut self, id: &NodeId, key: &BssKey) -> Option<Box<BssInfo>> {
        self.nodes.get_mut(id)?.remove_bss(key)
    }

    /// The BSS update of `dm_bss_list_t::update_list` (`dm_bss_list.cpp:149-185`): the stored BSS
    /// with the key of `bss` is replaced, nothing else changes.
    ///
    /// # Errors
    ///
    /// [`ModelError::NotFound`] if the node or the BSS does not exist (upstream logs and
    /// breaks).
    pub fn update_bss(&mut self, bss: Box<BssInfo>) -> Result<(), ModelError> {
        self.nodes
            .get_mut(&node_of_bss(&bss))
            .ok_or_else(node_not_found)?
            .update_bss(bss)
    }

    // --- STA ---

    /// The node that owns the radio `radio`: the first one, in key order, that has it
    /// (`dm_easy_mesh_list.cpp:723-735`, `:775-791`; plan C4).
    fn radio_owner(&self, radio: MacAddr) -> Option<NodeId> {
        self.nodes
            .iter()
            .find(|(_, node)| node.radio_by_mac(radio).is_some())
            .map(|(id, _)| *id)
    }

    /// `get_sta(key)` (`dm_easy_mesh_list.cpp:714-756`): the consolidated row with this key in
    /// the first node that owns `key.radio`.
    #[must_use]
    pub fn get_sta(&self, key: &StaKey) -> Option<Located<'_, NodeId, StaInfo>> {
        let owner = self.radio_owner(key.radio)?;
        let item = self
            .nodes
            .get(&owner)?
            .sta_info(key, StaMapTarget::Consolidated)?;
        Some(Located {
            at: owner,
            key: (),
            item,
        })
    }

    /// `put_sta` (`dm_easy_mesh_list.cpp:763-810`): stores `sta` in the consolidated map of the
    /// first node that owns `sta.radiomac`: a row with the same key is replaced with
    /// `copy_sta_info` semantics ([`NodeModel::put_sta`]).
    ///
    /// # Errors
    ///
    /// [`ModelError::NotFound`] if no node owns the radio (upstream logs and returns, `:793-796`).
    pub fn put_sta(&mut self, sta: Box<StaInfo>) -> Result<Put, ModelError> {
        let owner = self.radio_owner(sta.radiomac).ok_or_else(node_not_found)?;
        Ok(self
            .nodes
            .get_mut(&owner)
            .ok_or_else(node_not_found)?
            .put_sta(sta))
    }

    /// `remove_sta` (`dm_easy_mesh_list.cpp:758-761`): removes the consolidated row with this key
    /// from the first node that owns `key.radio`.
    ///
    /// Intentional difference (plan U2): upstream's body is empty.
    pub fn remove_sta(&mut self, key: &StaKey) -> Option<Box<StaInfo>> {
        let owner = self.radio_owner(key.radio)?;
        self.nodes.get_mut(&owner)?.remove_sta(key)
    }

    /// The station update of `dm_sta_list_t::update_list` (`dm_sta_list.cpp:148-177`): the row
    /// with the key of `sta` in the owner of its radio takes `copy_sta_info` semantics.
    ///
    /// # Errors
    ///
    /// [`ModelError::NotFound`] if no node owns the radio or the row is missing: upstream
    /// dereferences null (`:157-160`).
    pub fn update_sta(&mut self, sta: Box<StaInfo>) -> Result<(), ModelError> {
        let owner = self.radio_owner(sta.radiomac).ok_or_else(node_not_found)?;
        self.nodes
            .get_mut(&owner)
            .ok_or_else(node_not_found)?
            .update_sta(sta)
    }

    // --- op class ---

    /// The node that owns an op class (`get_op_class`, `dm_easy_mesh_list.cpp:1047-1113`, and
    /// `put_op_class`, `:1177-1253`): the first node, in key order, where
    ///
    /// - for the types up to `capability`, `preference` and `anticipated`, a radio has the
    ///   `ruid`; for every other type the device's AL MAC is the `ruid`; or else
    /// - the fallback of the call: `get` accepts the device MAC or the broadcast MAC for an
    ///   `anticipated` class (`:1076-1083`); `put` accepts the device MAC for the types from
    ///   `cac_available` on, and the broadcast MAC for `anticipated` (`:1205-1212`).
    fn op_class_owner(&self, key: &OpClassKey, for_put: bool) -> Option<NodeId> {
        let radio_scoped = key.kind.0 <= OpClassType::CAPABILITY.0
            || key.kind == OpClassType::PREFERENCE
            || key.kind == OpClassType::ANTICIPATED;
        self.nodes
            .iter()
            .find(|(_, node)| {
                let owns = if radio_scoped {
                    node.radio_by_mac(key.ruid).is_some()
                } else {
                    node.agent_al_interface_mac() == key.ruid
                };
                owns || op_class_fallback(node, key, for_put)
            })
            .map(|(id, _)| *id)
    }

    /// `get_op_class(key)` (`dm_easy_mesh_list.cpp:1047-1113`): the op class with this key in
    /// the first node that owns it (plan C4).
    #[must_use]
    pub fn get_op_class(&self, key: &OpClassKey) -> Option<Located<'_, NodeId, OpClassInfo>> {
        let owner = self.op_class_owner(key, false)?;
        let item = self.nodes.get(&owner)?.find_op_class(key)?;
        Some(Located {
            at: owner,
            key: (),
            item,
        })
    }

    /// `put_op_class` (`dm_easy_mesh_list.cpp:1177-1253`): stores `info` in the first node that
    /// owns it, replacing the op class with the same key ([`NodeModel::put_op_class`]).
    ///
    /// # Errors
    ///
    /// [`ModelError::NotFound`] if no node owns it. Intentional difference: upstream returns 0,
    /// success, without storing (`:1224-1226`, research 7.17). [`ModelError::Full`] if it is new
    /// and 64 are stored (upstream returns -1, `:1240`).
    pub fn put_op_class(&mut self, info: OpClassInfo) -> Result<Put, ModelError> {
        let owner = self
            .op_class_owner(&OpClassKey::of(&info), true)
            .ok_or_else(node_not_found)?;
        self.nodes
            .get_mut(&owner)
            .ok_or_else(node_not_found)?
            .put_op_class(info)
    }

    /// `remove_op_class` (`dm_easy_mesh_list.cpp:1172-1175`): removes the op class with this key
    /// from the first node that owns it.
    ///
    /// Intentional difference (plan U2): upstream's body is empty.
    pub fn remove_op_class(&mut self, key: &OpClassKey) -> Option<OpClassInfo> {
        let owner = self.op_class_owner(key, false)?;
        self.nodes.get_mut(&owner)?.remove_op_class(key)
    }

    /// The op-class update of `dm_op_class_list_t::update_list` (`dm_op_class_list.cpp:214-241`)
    /// on the owner of the op class ([`NodeModel::update_op_class`]).
    ///
    /// # Errors
    ///
    /// [`ModelError::NotFound`] if no node owns it or the op class is missing: upstream
    /// dereferences null (`:223-224`).
    pub fn update_op_class(&mut self, info: OpClassInfo) -> Result<(), ModelError> {
        let owner = self
            .op_class_owner(&OpClassKey::of(&info), false)
            .ok_or_else(node_not_found)?;
        self.nodes
            .get_mut(&owner)
            .ok_or_else(node_not_found)?
            .update_op_class(info)
    }

    /// `get_first_pre_set_op_class_by_type` (`dm_easy_mesh_list.cpp:1115-1138`): the first op
    /// class of `kind` of the first node in key order; for `anticipated` only the device-level
    /// ones (`ruid` is the AL MAC), not the radio-level ones. Upstream uses the first node in
    /// hash order (plan C3).
    #[must_use]
    pub fn first_pre_set_op_class_by_type(
        &self,
        kind: OpClassType,
    ) -> Option<Positioned<'_, OpClassInfo>> {
        self.pre_set_op_class_from(kind, 0)
    }

    /// `get_next_pre_set_op_class_by_type` (`dm_easy_mesh_list.cpp:1140-1170`): the next such op
    /// class after the one at position `index`, in the first node. `None` if `index` is not an op
    /// class of `kind` there, or is the last. The cursor is the position, not the key, so two op
    /// classes with the same key are both visited.
    #[must_use]
    pub fn next_pre_set_op_class_by_type(
        &self,
        kind: OpClassType,
        index: usize,
    ) -> Option<Positioned<'_, OpClassInfo>> {
        let node = self.nodes.values().next()?;
        let current = node.op_class(index)?;
        if current.id.kind != kind {
            return None;
        }
        self.pre_set_op_class_from(kind, index + 1)
    }

    /// The first pre-set op class of `kind` at or after position `start` of the first node.
    fn pre_set_op_class_from(
        &self,
        kind: OpClassType,
        start: usize,
    ) -> Option<Positioned<'_, OpClassInfo>> {
        let (at, node) = self.nodes.iter().next()?;
        node.op_classes()
            .iter()
            .enumerate()
            .skip(start)
            .find(|(_, info)| is_pre_set(node, info, kind))
            .map(|(index, item)| Positioned {
                at: *at,
                index,
                item,
            })
    }

    // --- policy ---

    /// `get_policy(key)` (`dm_easy_mesh_list.cpp:1300-1333`): the policy with this key in the node
    /// `(key.net, key.dev_mac)`.
    #[must_use]
    pub fn get_policy(&self, key: &PolicyKey) -> Option<&Policy> {
        self.nodes.get(&policy_node(key))?.policy(key)
    }

    /// `put_policy` (`dm_easy_mesh_list.cpp:1369-1383`): the key selects the node
    /// `(key.net, key.dev_mac)` (`:1376`); the policy is stored by its own id with `dev_mac` set
    /// to the node's AL MAC ([`NodeModel::set_policy`], `dm_easy_mesh.cpp:3393-3426`).
    ///
    /// # Errors
    ///
    /// [`ModelError::NotFound`] if the node does not exist; [`ModelError::Full`] if the policy is
    /// new and 32 are stored.
    pub fn put_policy(&mut self, key: &PolicyKey, policy: Box<Policy>) -> Result<Put, ModelError> {
        self.nodes
            .get_mut(&policy_node(key))
            .ok_or_else(node_not_found)?
            .set_policy(policy)
    }

    /// `remove_policy` (`dm_easy_mesh_list.cpp:1335-1367`): removes the policy with this key from
    /// the node `(key.net, key.dev_mac)`.
    pub fn remove_policy(&mut self, key: &PolicyKey) -> Option<Box<Policy>> {
        self.nodes.get_mut(&policy_node(key))?.remove_policy(key)
    }

    /// The policy update of `dm_policy_list_t::update_list` (`dm_policy_list.cpp:223-256`): the
    /// stored policy with the key of `policy` is replaced; if there is none, the policy is
    /// stored as by [`MeshModel::put_policy`] (`:233-237`).
    ///
    /// # Errors
    ///
    /// The errors of [`MeshModel::put_policy`].
    pub fn update_policy(&mut self, policy: Box<Policy>) -> Result<Put, ModelError> {
        let key = PolicyKey::of(&policy);
        self.nodes
            .get_mut(&policy_node(&key))
            .ok_or_else(node_not_found)?
            .update_policy(policy)
    }

    // --- scan result ---

    /// `get_scan_result(key)` (`dm_easy_mesh_list.cpp:1428-1451`): the scan result with this key
    /// in the node `(key.net, key.dev_mac)`. Upstream's key also carries the neighbor BSSID,
    /// which is dropped for the lookup (`:1445-1448`).
    #[must_use]
    pub fn get_scan_result(&self, key: &ScanKey) -> Option<&ScanResult> {
        self.nodes
            .get(&scan_node(key))?
            .find_matching_scan_result(key)
    }

    /// `put_scan_result` (`dm_easy_mesh_list.cpp:1524-1611`): stores a neighbor of `result` for
    /// the scan with the key of `result.id` in the node `(id.net_id, id.dev_mac)`.
    ///
    /// - No such scan result: `result` is stored as it is (`:1548-1560`). Intentional difference
    ///   (research 7.13): upstream copies the neighbors and then does `num_neighbors++`, so the
    ///   count is one above the neighbors stored; the port's count is the neighbor list's length.
    /// - Otherwise the neighbor with BSSID `bssid` is replaced by `result.neighbor[index]`, or
    ///   that neighbor is appended (`:1565-1587`). An `index` beyond the neighbors of `result`
    ///   reads garbage upstream and is an error here. Intentional difference: for
    ///   [`SCAN_RESULT_SELF_INDEX`] (`scan_result_self_index`, `inc/dm_scan_result_list.h:27`)
    ///   no neighbor is written and the report below is still appended; upstream's branch would
    ///   read `neighbor[0xFFF]`, out of bounds. Upstream appends the report after the neighbor
    ///   code (`:1590-1610`) but returns before it when the existing result has 16 neighbors and
    ///   no neighbor with `bssid` (`:1576-1578`); the port appends it then too.
    /// - For a station scanner, a beacon report with the op class, channel and `bssid` is
    ///   appended to the first consolidated station with the scanner's MAC (`:1592-1604`).
    ///
    /// A failed put changes nothing: capacities are checked before the first write.
    ///
    /// # Errors
    ///
    /// [`ModelError::NotFound`] if the node does not exist, or `index` is neither a neighbor of
    /// `result` nor [`SCAN_RESULT_SELF_INDEX`] for an existing scan result; [`ModelError::Full`]
    /// if the scan result has 16 neighbors and `bssid` is new (upstream returns, `:1576-1578`;
    /// not for [`SCAN_RESULT_SELF_INDEX`], which writes no neighbor), or the station has 10
    /// beacon reports (upstream writes past the array).
    pub fn put_scan_result(
        &mut self,
        bssid: MacAddr,
        result: Box<ScanResult>,
        index: usize,
    ) -> Result<Put, ModelError> {
        let key = ScanKey::of(&result.id);
        let node = self
            .nodes
            .get_mut(&scan_node(&key))
            .ok_or_else(node_not_found)?;
        let station = beacon_station(node, &key);
        if let Some(station_key) = station {
            let reports = node
                .sta_info(&station_key, StaMapTarget::Consolidated)
                .map_or(0, |sta| sta.beacon_reports.len());
            if reports >= MAX_BEACON_REPORTS {
                return Err(ModelError::Full {
                    table: Table::BeaconReport,
                    capacity: MAX_BEACON_REPORTS,
                });
            }
        }
        let incoming = result.neighbor.as_slice().get(index).copied();
        let put = match node.scan_result_mut(&key) {
            None => {
                node.put_scan_result_record(result);
                Put::Inserted
            }
            Some(stored) => {
                if index != SCAN_RESULT_SELF_INDEX {
                    let neighbor = incoming.ok_or(ModelError::NotFound {
                        table: Table::Neighbor,
                    })?;
                    match stored
                        .neighbor
                        .iter_mut()
                        .find(|known| known.bssid == bssid)
                    {
                        Some(slot) => *slot = neighbor,
                        None => stored
                            .neighbor
                            .push(neighbor)
                            .map_err(|_| ModelError::Full {
                                table: Table::Neighbor,
                                capacity: 16,
                            })?,
                    }
                }
                Put::Replaced
            }
        };
        if let Some(station_key) = station {
            if let Some(sta) = node.sta_info_mut(&station_key, StaMapTarget::Consolidated) {
                // Capacity was checked above, so the push cannot fail.
                let _ = sta.beacon_reports.push(beacon_report(&key, bssid));
            }
        }
        Ok(put)
    }

    /// `remove_scan_result` (`dm_easy_mesh_list.cpp:1453-1522`): removes the whole scan result
    /// with this key from the node `(key.net, key.dev_mac)` (`:1478-1482`) and hands it back; for
    /// a station scanner it also removes the beacon report with BSSID `bssid` from the first
    /// consolidated station with the scanner's MAC, whether or not the scan result existed
    /// (`:1493-1520`). Upstream edits the encoded report bytes and re-decodes them; the port
    /// edits the decoded reports.
    pub fn remove_scan_result(&mut self, key: &ScanKey, bssid: MacAddr) -> Option<Box<ScanResult>> {
        let node = self.nodes.get_mut(&scan_node(key))?;
        let removed = node.remove_scan_result(key);
        remove_beacon_report(node, key, bssid);
        removed
    }
}

fn op_class_fallback(node: &NodeModel, key: &OpClassKey, for_put: bool) -> bool {
    let is_device = node.agent_al_interface_mac() == key.ruid;
    let anticipated = key.kind == OpClassType::ANTICIPATED;
    if for_put {
        (is_device && key.kind.0 >= OpClassType::CAC_AVAILABLE.0)
            || (key.ruid == MacAddr::BROADCAST && anticipated)
    } else {
        (is_device || key.ruid == MacAddr::BROADCAST) && anticipated
    }
}

fn is_pre_set(node: &NodeModel, info: &OpClassInfo, kind: OpClassType) -> bool {
    info.id.kind == kind
        && (kind != OpClassType::ANTICIPATED || info.id.ruid == node.agent_al_interface_mac())
}

fn policy_node(key: &PolicyKey) -> NodeId {
    NodeId {
        net: key.net,
        al: key.dev_mac,
    }
}

fn scan_node(key: &ScanKey) -> NodeId {
    NodeId {
        net: key.net,
        al: key.dev_mac,
    }
}

/// The consolidated station that a station scan's beacon reports belong to: the first one, in
/// key order, whose MAC is the scanner's (`dm_easy_mesh_list.cpp:1592-1604`). A radio scan has
/// none. The result is the key the row is stored under, so the callers act on the row found,
/// whatever its fields say now (plan D1a; upstream holds the row pointer).
fn beacon_station(node: &NodeModel, key: &ScanKey) -> Option<StaKey> {
    if key.scanner_type == ScannerType::RADIO {
        return None;
    }
    node.first_sta(key.scanner_mac).map(|entry| entry.key)
}

/// Removes the first beacon report with BSSID `bssid` from the station of the scan `key`, if the
/// scan has a station (`dm_easy_mesh_list.cpp:1493-1520`). Upstream matches by BSSID alone, so
/// this is the cleanup of the single-row [`MeshModel::remove_scan_result`].
pub(super) fn remove_beacon_report(node: &mut NodeModel, key: &ScanKey, bssid: MacAddr) {
    remove_first_beacon_report(node, key, |report| report.bssid == bssid);
}

/// Removes the first beacon report with BSSID `bssid`, and the op class and channel of `key`,
/// from the station of the scan `key`: the report that [`MeshModel::put_scan_result`] appended
/// for that neighbor. Not upstream, which matches by BSSID alone and so can remove the report of
/// another channel (plan D1b, `intentional-difference:d1-storage-route`: the bulk clear acts on
/// the stored key). Used by [`MeshModel::delete_scan_result_list`].
pub(super) fn remove_neighbor_beacon_report(node: &mut NodeModel, key: &ScanKey, bssid: MacAddr) {
    remove_first_beacon_report(node, key, |report| {
        report.bssid == bssid && report.op_class == key.op_class && report.channel == key.channel
    });
}

/// Removes the self report that [`MeshModel::put_scan_result`] appended for the scan `key`: the
/// first zero-BSSID report with the op class and channel of `key` (`beacon_report(key, ZERO)`).
/// Not upstream: its single-row removal matches by BSSID alone (plan D5(b),
/// `intentional-difference:d5-self-report-cleanup`).
pub(super) fn remove_self_beacon_report(node: &mut NodeModel, key: &ScanKey) {
    remove_first_beacon_report(node, key, |report| {
        report.bssid == MacAddr::ZERO
            && report.op_class == key.op_class
            && report.channel == key.channel
    });
}

fn remove_first_beacon_report(
    node: &mut NodeModel,
    key: &ScanKey,
    matches: impl Fn(&BeaconReport) -> bool,
) {
    let Some(station_key) = beacon_station(node, key) else {
        return;
    };
    let Some(sta) = node.sta_info_mut(&station_key, StaMapTarget::Consolidated) else {
        return;
    };
    let position = sta.beacon_reports.iter().position(matches);
    if let Some(position) = position {
        sta.beacon_reports.remove(position);
    }
}

/// The report appended for a station scan: op class, channel and BSSID, everything else zero.
fn beacon_report(key: &ScanKey, bssid: MacAddr) -> BeaconReport {
    BeaconReport {
        op_class: key.op_class,
        channel: key.channel,
        start_time: 0,
        duration: 0,
        frame_info: 0,
        rcpi: 0,
        rsni: 0,
        bssid,
        antenna: 0,
        tsf: 0,
        wide_bandwidth_channel_present: false,
        wide_bandwidth_channel: WideBwChannel {
            bandwidth: 0,
            center_seg0: 0,
            center_seg1: 0,
        },
        num_repetitions: 0,
    }
}
