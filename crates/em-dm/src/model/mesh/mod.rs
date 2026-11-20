// SPDX-License-Identifier: Apache-2.0

//! `MeshModel`: the controller's model, the port of `dm_easy_mesh_list_t`
//! (`inc/dm_easy_mesh_list.h:28-33`) and the model residue of the `dm_*_list_t` classes.
//!
//! # Storage
//!
//! | Upstream member | Port |
//! |---|---|
//! | `m_list` (`hash_map_t`, key `"net@al_mac"`, `:31`) | `BTreeMap<NodeId, Box<NodeModel>>`: one node per (network, AL MAC); key order replaces hash order (plan C3) |
//! | `m_network_list[EM_MAX_NETWORKS]`, `m_num_networks` (`:28-29`) | `BTreeMap<NetId, Box<NetworkModel>>`, at most 5 entries (plan C7); key order replaces insertion order (plan C3) |
//! | the network record and SSIDs copied into every node (`dm_easy_mesh_list.cpp:1782-1801`, `:962-986`) | stored once in the node's [`NetworkModel`] (plan U1, 0007 D2) |
//! | `m_controller_al_intf` (`:33`) | `controller_al_mac`, only its MAC is read (`:139-142`) |
//! | `m_mgr` (`:32`) | none: [`NodeNeeded`] is returned instead of calling `create_node` (plan A6) |
//! | `dm_radio_cap_list_t::m_list` | `BTreeMap<MacAddr, Box<RadioCapInfo>>` (`dm_radio_cap_list.cpp:120`) |
//! | `dm_ieee_1905_security_list_t::m_list` | `BTreeMap<MacAddr, Ieee1905SecurityInfo>` (`dm_ieee_1905_security_list.cpp:136`) |
//! | `dm_ssid_2_vid_map_t::m_list` | `BTreeMap<Ssid2VidKey, Ssid2VidMapInfo>` (`dm_ssid_2_vid_map.cpp:78`) |
//!
//! The side maps are global, as upstream's (plan C4): their keys carry no network.
//!
//! # Keys and lookups
//!
//! Every lookup is exact (plan C4). Node-scoped operations take the [`NodeId`] that upstream
//! parses out of the key string. The global finds by radio MAC (`get_radio`, `get_sta`,
//! `put_sta`, `get_op_class`, `put_op_class`) return the first match in key order, as upstream's
//! walk returns the first match in hash order.
//!
//! # Storage position is the cursor and the route (plan D1)
//!
//! An operation that holds a storage location does not rebuild it from the record's fields.
//! The walks yield and take the stored map key ([`Located::key`]) or the array position
//! ([`Positioned::index`]), and the bulk operations (`delete_*_list`, `apply_sta_mld_association`)
//! remove from the node and row they are visiting. Editing a key field of a row through a `&mut`
//! accessor does not rekey it: the row stays where it is stored, as upstream's pointer and array
//! storage does. Where upstream rebuilds a key from the record in a bulk operation, the port
//! routes by location instead (`intentional-difference:d1-storage-route`).
//!
//! These APIs receive only a record or a key, as upstream's key-string APIs do, so they find the
//! row from it. That is correct and is not a rebuild of a held location: BSS put and update
//! (the node of the record), the global finds by radio MAC (`get_radio`, `get_sta`, `put_sta`,
//! `get_op_class`, `put_op_class`), policy get, put, remove and update (the node of the key),
//! scan result get, put and update, radio put, update and remove by MAC, `update_device_list`,
//! and `get_bss` and `remove_bss` by `(node, key)`. [`MeshModel::put_device`] counts the node by
//! the key's network, as upstream does (`dm_easy_mesh_list.cpp:259-263`). `set_policy` stamps the
//! node's AL MAC into the policy and the scan-result update overrides the id fields: both are
//! identity writes of upstream (`dm_easy_mesh.cpp:3405, 3601-3610`).
//!
//! Operations do not mark the dirty set (plan C5), except the two list-layer marks
//! (`dm_easy_mesh_list.cpp:174, 1747`) that live in [`MeshModel::put_network`] and
//! [`MeshModel::create_data_model`].
//!
//! # Not stored
//!
//! `debug_probe` (`:894-906`) has no counterpart. The empty constructor and destructor
//! (`:1827-1834`) have none either: [`MeshModel::new`] takes the place of `init`, and the owned
//! maps are dropped with the model. See the method map.

mod lists;
mod networks;
mod nodes;
mod radios;
mod records;
mod side_maps;
mod walk;

pub use lists::ListOp;
pub use radios::{NodeNeeded, RadioPutOutcome};
pub use records::SCAN_RESULT_SELF_INDEX;
pub use side_maps::{Ssid2VidChange, Ssid2VidKey};

use crate::model::access::{ModelAccess, NodeCtx, NodeRef};
use crate::model::keys::{NetId, NodeId};
use crate::model::network::NetworkModel;
use crate::model::node::NodeModel;
use crate::records::{Ieee1905SecurityInfo, RadioCapInfo, Ssid2VidMapInfo};
use em_types::MacAddr;
use std::collections::BTreeMap;

/// `EM_MAX_NETWORKS` (`inc/em_base.h:40`): the most networks a mesh holds (plan C7).
pub const MAX_NETWORKS: usize = 5;

/// A row together with where it was found, as the `get_first_*` and `get_next_*` walks return it.
///
/// `at` is the [`NodeId`] of the owning node. `key` is a reference to the key the row is stored
/// under, for the map-backed tables (stations, policies, scan results): the matching `get_next_*`
/// takes `at` and `key` as the cursor. It is a reference because the scan and policy keys carry
/// a network id, which would take the walk results over 256 bytes (0007 criterion 4). It is the storage location, not a key derived from the row's fields, so
/// a later edit of a key field through `&mut` does not break the walk (plan D1a). Walks of other
/// tables have no key, and `Key` is `()`.
#[derive(Debug)]
pub struct Located<'a, Where, Row, Key = ()> {
    /// The owner of the row.
    pub at: Where,
    /// The key the row is stored under; `()` for a table that has none.
    pub key: Key,
    /// The row.
    pub item: &'a Row,
}

/// A row of an array-backed table (radios, BSSs, op classes, network SSIDs) together with its
/// position, as the array walks return it.
///
/// `at` is the [`NodeId`] of the owning node, or the [`NetId`] of the owning network for SSIDs.
/// Those tables can hold two rows with the same key (for example two op classes that
/// `update_cac_status_id` gave the same `ruid`), so the cursor of the matching `get_next_*` is the
/// position `(at, index)`, not the key: every stored row is visited exactly once and the walk
/// ends. The position is the location of the row: an array table has no other stored key (plan
/// D1). The map-backed walks (station, policy, scan result) take the stored map key instead,
/// see [`Located`].
#[derive(Debug)]
pub struct Positioned<'a, Row, Where = NodeId> {
    /// The owner of the row.
    pub at: Where,
    /// The index of the row in the owner's table: the cursor the matching `get_next_*` takes.
    pub index: usize,
    /// The row.
    pub item: &'a Row,
}

/// The controller's model: every node and network of the mesh, and the global side maps.
///
/// ```compile_fail
/// fn needs_clone<T: Clone>() {}
/// needs_clone::<em_dm::MeshModel>();
/// ```
///
/// ```compile_fail
/// fn needs_copy<T: Copy>() {}
/// needs_copy::<em_dm::MeshModel>();
/// ```
///
/// A compiling control for the two probes above:
///
/// ```
/// fn needs_clone<T: Clone>() {}
/// needs_clone::<em_dm::MacAddr>();
/// let _: Option<em_dm::MeshModel> = None;
/// ```
#[derive(Debug, PartialEq, Default)]
pub struct MeshModel {
    controller_al_mac: Option<MacAddr>,
    networks: BTreeMap<NetId, Box<NetworkModel>>,
    nodes: BTreeMap<NodeId, Box<NodeModel>>,
    radio_caps: BTreeMap<MacAddr, Box<RadioCapInfo>>,
    security_1905: BTreeMap<MacAddr, Ieee1905SecurityInfo>,
    ssid_2_vid: BTreeMap<Ssid2VidKey, Ssid2VidMapInfo>,
}

impl MeshModel {
    /// An empty mesh: `init(mgr, controller_al_intf)` (`dm_easy_mesh_list.cpp:1819-1825`). The
    /// manager is not stored (see the table above). `controller_al_mac` is the MAC of
    /// `m_controller_al_intf`, `None` for a null pointer; [`MeshModel::put_network`] reads it.
    #[must_use]
    pub fn new(controller_al_mac: Option<MacAddr>) -> Self {
        Self {
            controller_al_mac,
            ..Self::default()
        }
    }

    /// The MAC of `m_controller_al_intf`.
    #[must_use]
    pub fn controller_al_mac(&self) -> Option<MacAddr> {
        self.controller_al_mac
    }

    /// Replaces the MAC of `m_controller_al_intf`, which upstream reads through a pointer the
    /// controller may update.
    pub fn set_controller_al_mac(&mut self, mac: Option<MacAddr>) {
        self.controller_al_mac = mac;
    }
}

impl ModelAccess for MeshModel {
    fn node_ctx(&mut self, id: &NodeId) -> Option<NodeCtx<'_>> {
        // The two maps are separate fields, so both borrows are live together.
        let node = self.nodes.get_mut(id)?;
        let network = self.networks.get_mut(&id.net)?;
        Some(NodeCtx {
            id: *id,
            network,
            node,
        })
    }

    fn node(&self, id: &NodeId) -> Option<NodeRef<'_>> {
        let node = self.nodes.get(id)?;
        let network = self.networks.get(&id.net)?;
        Some(NodeRef {
            id: *id,
            network,
            node,
        })
    }

    fn nodes(&self) -> Box<dyn Iterator<Item = NodeRef<'_>> + '_> {
        Box::new(self.nodes.iter().filter_map(|(id, node)| {
            let network = self.networks.get(&id.net)?;
            Some(NodeRef {
                id: *id,
                network,
                node,
            })
        }))
    }
}
