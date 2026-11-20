// SPDX-License-Identifier: Apache-2.0

//! Network operations of `MeshModel`: `get_network`, `put_network`, `remove_network` and the
//! network SSIDs (`dm_easy_mesh_list.cpp:48-177, 812-987`).

use super::{MAX_NETWORKS, MeshModel, Positioned};
use crate::model::dirty::DbCfgType;
use crate::model::error::{ModelError, Put, Table};
use crate::model::keys::{NetId, NodeId, SsidKey};
use crate::model::network::NetworkModel;
use crate::model::node::{NodeInit, NodeRole};
use crate::records::{NetworkInfo, NetworkSsidInfo};
use em_types::ProfileType;
use std::ops::Bound;

impl MeshModel {
    /// Number of networks (`m_num_networks`, `dm_easy_mesh_list.h:29`).
    #[must_use]
    pub fn num_networks(&self) -> usize {
        self.networks.len()
    }

    /// `get_network` (`dm_easy_mesh_list.cpp:105-122`): the network with exactly this id.
    ///
    /// Intentional difference (plan C4, research 7.6): upstream compares with `strncmp` over
    /// `strlen(key)`, so `"Network1"` finds `"Network10"` and the empty key finds any network.
    /// The empty id is valid input here and finds only a network with the empty id.
    #[must_use]
    pub fn get_network(&self, key: &NetId) -> Option<&NetworkModel> {
        self.networks.get(key).map(|network| &**network)
    }

    /// The network with this id, for the engine's direct writes.
    pub fn network_mut(&mut self, key: &NetId) -> Option<&mut NetworkModel> {
        self.networks.get_mut(key).map(|network| &mut **network)
    }

    /// `get_first_network` (`dm_easy_mesh_list.cpp:48-69`): the first network in key order.
    /// Upstream returns the network of `m_network_list[0]`, the first one added through
    /// `put_network` (plan C3).
    #[must_use]
    pub fn first_network(&self) -> Option<&NetworkModel> {
        self.networks.values().next().map(|network| &**network)
    }

    /// `get_next_network` (`dm_easy_mesh_list.cpp:71-103`): the network after `net`, in key
    /// order. `None` if `net` is not a stored network or is the last one.
    #[must_use]
    pub fn next_network(&self, net: &NetId) -> Option<&NetworkModel> {
        if !self.networks.contains_key(net) {
            return None;
        }
        self.networks
            .range((Bound::Excluded(*net), Bound::Unbounded))
            .next()
            .map(|(_, network)| &**network)
    }

    /// The network of `key`, created as a default one if there is none (plan C7).
    ///
    /// # Errors
    ///
    /// [`ModelError::Full`] if the network is new and 5 networks are stored.
    pub(super) fn ensure_network(&mut self, key: NetId) -> Result<&mut NetworkModel, ModelError> {
        if !self.networks.contains_key(&key) && self.networks.len() >= MAX_NETWORKS {
            return Err(ModelError::Full {
                table: Table::Network,
                capacity: MAX_NETWORKS,
            });
        }
        Ok(&mut **self
            .networks
            .entry(key)
            .or_insert_with(|| Box::new(NetworkModel::new(key))))
    }

    /// `put_network` (`dm_easy_mesh_list.cpp:129-177`): stores the network record under `key`.
    ///
    /// - The controller-ID mismatch (`:139-142`): for the global network, when the list knows
    ///   the controller's AL MAC and `net.ctrl_id.mac` differs from it, the record's controller
    ///   MAC is replaced by the AL MAC before anything else, and the controller's node is marked
    ///   `network_list_update` dirty after the write (`:172-175`, plan C5).
    /// - If there is no node for `(key, net.ctrl_id.mac)`, the controller's node is created
    ///   (`:154`, role controller, profile 3) and the call returns [`Put::Inserted`]; otherwise
    ///   [`Put::Replaced`].
    ///
    /// Intentional differences: the record is stored once, in the network (plan U1), where
    /// upstream copies it into every node (`:161-167`, which also overwrites the records of other
    /// networks, research 7.4); a sixth network is refused and nothing changes, where upstream
    /// writes past `m_network_list` (`:156-158`, plan C7).
    ///
    /// # Errors
    ///
    /// [`ModelError::Full`] if the network is new and 5 networks are stored.
    pub fn put_network(
        &mut self,
        key: NetId,
        mut net: Box<NetworkInfo>,
    ) -> Result<Put, ModelError> {
        let mismatched_controller = self
            .controller_al_mac
            .filter(|al| key == NetId::global() && !al.is_zero() && net.ctrl_id.mac != *al);
        if let Some(al) = mismatched_controller {
            net.ctrl_id.mac = al;
        }
        let controller_node = NodeId {
            net: key,
            al: net.ctrl_id.mac,
        };
        let put = if self.nodes.contains_key(&controller_node) {
            Put::Replaced
        } else {
            self.create_data_model(NodeInit {
                net: key,
                al: controller_node.al,
                profile: ProfileType::MULTI_AP_PROFILE_3,
                role: NodeRole::Controller,
                colocated: false,
            })?;
            Put::Inserted
        };
        self.ensure_network(key)?.set_info(net);
        if mismatched_controller.is_some() {
            if let Some(node) = self.nodes.get_mut(&controller_node) {
                node.dirty_mut().mark(DbCfgType::NetworkListUpdate);
            }
        }
        Ok(put)
    }

    /// `remove_network` (`dm_easy_mesh_list.cpp:124-127`): removes the network.
    ///
    /// Intentional difference (plan U2, C7): upstream's body is empty. A network that still has
    /// nodes is refused and nothing changes; remove the nodes first.
    ///
    /// # Errors
    ///
    /// [`ModelError::NotFound`] if there is no such network; [`ModelError::NetworkInUse`] if it
    /// has nodes.
    pub fn remove_network(&mut self, key: &NetId) -> Result<Box<NetworkModel>, ModelError> {
        if !self.networks.contains_key(key) {
            return Err(ModelError::NotFound {
                table: Table::Network,
            });
        }
        let nodes = self.nodes.keys().filter(|id| id.net == *key).count();
        if nodes > 0 {
            return Err(ModelError::NetworkInUse { nodes });
        }
        self.networks.remove(key).ok_or(ModelError::NotFound {
            table: Table::Network,
        })
    }

    // --- network SSIDs ---

    /// `get_network_ssid` (`dm_easy_mesh_list.cpp:869-892`): the SSID row with exactly this id in
    /// the first network (key order) that has one. Upstream searches the nodes in hash order and
    /// matches the whole id with `strcmp`.
    #[must_use]
    pub fn get_network_ssid(&self, key: &SsidKey) -> Option<&NetworkSsidInfo> {
        self.networks
            .values()
            .find_map(|network| network.find_network_ssid(key))
    }

    /// `put_network_ssid` (`dm_easy_mesh_list.cpp:937-987`): the row with the same id replaces
    /// the stored one, a new id is added, in the SSID table of network `net` (the network id
    /// that upstream parses from the key suffix, `:944-954`).
    ///
    /// Intentional differences: one write into the network's table, visible to every node of
    /// the network, replaces the fan-out into each node (plan U1, 0007 D2); a sixth SSID is
    /// refused (upstream writes past the array, `:979-981`); a network that does not exist is an
    /// error (upstream, finding no node, stores nothing and reports nothing).
    ///
    /// # Errors
    ///
    /// [`ModelError::NotFound`] if there is no network `net`; [`ModelError::Full`] if the id is
    /// new and 5 rows are stored.
    pub fn put_network_ssid(
        &mut self,
        net: &NetId,
        ssid: Box<NetworkSsidInfo>,
    ) -> Result<Put, ModelError> {
        self.networks
            .get_mut(net)
            .ok_or(ModelError::NotFound {
                table: Table::Network,
            })?
            .put_network_ssid(ssid)
    }

    /// `remove_network_ssid` (`dm_easy_mesh_list.cpp:908-935`): removes the row with exactly this
    /// id from network `net`. Intentional difference: upstream matches a prefix, leaves a zeroed
    /// hole and lowers the count on every node (research 7.5).
    pub fn remove_network_ssid(
        &mut self,
        net: &NetId,
        key: &SsidKey,
    ) -> Option<Box<NetworkSsidInfo>> {
        self.networks.get_mut(net)?.remove_network_ssid(key)
    }

    /// `get_first_network_ssid` (`dm_easy_mesh_list.cpp:812-827`): slot 0 of the first network
    /// (key order) that has an SSID. Upstream returns the slot 0 of the last node with SSIDs,
    /// because its loop has no `break`; the port takes the first network in key order (plan C3).
    #[must_use]
    pub fn first_network_ssid(&self) -> Option<Positioned<'_, NetworkSsidInfo, NetId>> {
        self.first_ssid_from(Bound::Unbounded)
    }

    /// `get_next_network_ssid` (`dm_easy_mesh_list.cpp:829-867`): the SSID after the row at
    /// position `index` of network `net`, in insertion order within a network and in `NetId`
    /// order across networks (plan C3, amended by D1a). `None` if the row is not stored or is
    /// the last. Upstream also walks the empty slots of the array (`:849-856`); the port has none.
    #[must_use]
    pub fn next_network_ssid(
        &self,
        net: &NetId,
        index: usize,
    ) -> Option<Positioned<'_, NetworkSsidInfo, NetId>> {
        let network = self.networks.get(net)?;
        network.network_ssid(index)?;
        if let Some(item) = network.next_network_ssid(index) {
            return Some(Positioned {
                at: *net,
                index: index + 1,
                item,
            });
        }
        self.first_ssid_from(Bound::Excluded(*net))
    }

    /// Slot 0 of the first network at or after `from` that has an SSID.
    fn first_ssid_from(
        &self,
        from: Bound<NetId>,
    ) -> Option<Positioned<'_, NetworkSsidInfo, NetId>> {
        self.networks
            .range((from, Bound::Unbounded))
            .find_map(|(net, network)| {
                network.first_network_ssid().map(|item| Positioned {
                    at: *net,
                    index: 0,
                    item,
                })
            })
    }
}
