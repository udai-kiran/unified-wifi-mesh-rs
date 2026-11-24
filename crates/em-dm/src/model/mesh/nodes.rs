// SPDX-License-Identifier: Apache-2.0

//! Node creation, removal and device operations of `MeshModel`
//! (`dm_easy_mesh_list.cpp:178-304, 1613-1817`).

use super::MeshModel;
use crate::model::dirty::DbCfgType;
use crate::model::error::{ModelError, Put, Table};
use crate::model::keys::{NetId, NodeId};
use crate::model::node::{NodeInit, NodeModel, NodeRole};
use crate::records::{DeviceId, DeviceInfo};

fn node_not_found() -> ModelError {
    ModelError::NotFound { table: Table::Node }
}

impl MeshModel {
    /// Number of nodes.
    #[must_use]
    pub fn num_nodes(&self) -> usize {
        self.nodes.len()
    }

    /// `get_data_model` (`dm_easy_mesh_list.cpp:1806-1817`): the node with this network and AL
    /// MAC. The empty network id is valid input and finds only a node with the empty id.
    #[must_use]
    pub fn get_data_model(&self, id: &NodeId) -> Option<&NodeModel> {
        self.nodes.get(id).map(|node| &**node)
    }

    /// The node with this id, for the engine's direct writes. Dirty marking goes through it.
    pub fn data_model_mut(&mut self, id: &NodeId) -> Option<&mut NodeModel> {
        self.nodes.get_mut(id).map(|node| &mut **node)
    }

    /// `create_data_model` (`dm_easy_mesh_list.cpp:1656-1804`): creates the node of `init` with
    /// [`NodeModel::new`] and stores it under `NodeId { init.net, init.al }`. The host lookup
    /// that decides `init.colocated` (`:1728`) and the JSON side effect (`:1763`) stay outside
    /// em-dm. Returns [`Put::Replaced`] if a node with that id was there: upstream's
    /// `hash_map_put` replaces it too (`:1801`).
    ///
    /// - Default network (plan C7): a node whose network has no model gets a default
    ///   [`NetworkModel`](crate::NetworkModel) with that id. Upstream leaves the node's network
    ///   zeroed (`:1781-1798`). At 5 networks the call fails and changes nothing, where upstream
    ///   has no limit.
    /// - No network or SSID copy into the node (`:1782-1801`): the network is stored once
    ///   (plan U1).
    /// - Co-located agent (`:1736-1747`): when the network already has a model, the colocated
    ///   agent MAC of the one record becomes `init.al` and the controller node of that same
    ///   network is marked `network_list_update` dirty (plan C5): the controller node stored
    ///   under the record's controller MAC, else the first controller node of the network in key
    ///   order (plan D4(b), `intentional-difference:d4-controller-by-role`). Upstream finds the
    ///   controller's node under `GLOBAL_NET_ID` (`:1738`), writes its own copy of the record and
    ///   marks that node (`:1740-1747`); the port keeps one record per network (plan U1), so the
    ///   node to mark is a controller of the network whose record changed. If there is none,
    ///   upstream asserts (`:1739`) and the port writes the MAC and marks nothing.
    ///
    /// # Errors
    ///
    /// [`ModelError::Full`] if the network is new and 5 networks are stored.
    pub fn create_data_model(&mut self, init: NodeInit) -> Result<Put, ModelError> {
        let network_known = self.networks.contains_key(&init.net);
        self.ensure_network(init.net)?;
        if network_known && init.role == NodeRole::Agent && init.colocated {
            self.record_colocated_agent(init.net, init.al);
        }
        let id = NodeId {
            net: init.net,
            al: init.al,
        };
        let previous = self.nodes.insert(id, Box::new(NodeModel::new(init)));
        Ok(if previous.is_some() {
            Put::Replaced
        } else {
            Put::Inserted
        })
    }

    /// The colocated-agent write of `create_data_model` (`dm_easy_mesh_list.cpp:1742-1747`).
    fn record_colocated_agent(&mut self, net: NetId, agent_al: em_types::MacAddr) {
        let Some(network) = self.networks.get_mut(&net) else {
            return;
        };
        network.set_colocated_agent_interface_mac(agent_al);
        let controller_mac = network.controller_interface_mac();
        if let Some(controller) = self.controller_node_of(net, controller_mac) {
            if let Some(node) = self.nodes.get_mut(&controller) {
                node.dirty_mut().mark(DbCfgType::NetworkListUpdate);
            }
        }
    }

    /// The controller node of network `net` that the co-located write marks (plan D4(b)): the
    /// controller node stored under `(net, controller_mac)`, the network record's controller MAC;
    /// else the first controller node of `net` in key order; else none.
    ///
    /// Upstream finds the node by `get_data_model(GLOBAL_NET_ID, ctrl_id.mac)` and asserts
    /// (`dm_easy_mesh_list.cpp:1738-1739`). Intentional difference
    /// (`intentional-difference:d4-controller-by-role`): the port keeps one network record
    /// (plan U1), and that record's controller MAC and the controller node's key can part ways
    /// ([`NetworkModel::set_controller_id`](crate::NetworkModel::set_controller_id),
    /// [`NetworkModel::info_mut`](crate::NetworkModel::info_mut), an `update_network_list`
    /// update, `AlInsert` with a network, `NodeCtx::set_ctrl_al_interface_mac`), so the lookup
    /// falls back to the role instead of losing the mark.
    fn controller_node_of(&self, net: NetId, controller_mac: em_types::MacAddr) -> Option<NodeId> {
        let by_key = NodeId {
            net,
            al: controller_mac,
        };
        if self
            .nodes
            .get(&by_key)
            .is_some_and(|node| node.is_controller())
        {
            return Some(by_key);
        }
        self.nodes
            .iter()
            .find(|(id, node)| id.net == net && node.is_controller())
            .map(|(id, _)| *id)
    }

    /// `delete_data_model` (`dm_easy_mesh_list.cpp:1639-1654`): removes the node and hands it
    /// back. `None` if there is no such node: upstream dereferences the null result
    /// (`:1649-1652`). The network stays, even with no node left.
    pub fn delete_data_model(&mut self, id: &NodeId) -> Option<Box<NodeModel>> {
        self.nodes.remove(id)
    }

    /// `delete_all_data_models` (`dm_easy_mesh_list.cpp:1613-1637`): removes every node except
    /// the controller's (`:1622-1625`) and hands them back in key order.
    pub fn delete_all_data_models(&mut self) -> Vec<Box<NodeModel>> {
        let removable: Vec<NodeId> = self
            .nodes
            .iter()
            .filter(|(_, node)| !node.is_controller())
            .map(|(id, _)| *id)
            .collect();
        removable
            .into_iter()
            .filter_map(|id| self.nodes.remove(&id))
            .collect()
    }

    /// `get_device` (`dm_easy_mesh_list.cpp:210-224`): the device of the node `id`. Upstream
    /// parses `"net@mac@media"`; the media is ignored (`:216`).
    #[must_use]
    pub fn get_device(&self, id: &NodeId) -> Option<&DeviceInfo> {
        self.nodes.get(id).map(|node| node.device())
    }

    /// `put_device` (`dm_easy_mesh_list.cpp:235-265`): stores `device` in the node of `key`.
    ///
    /// - A controller node with `(key.net_id, device.intf.mac)` takes the device, and the stored
    ///   `id.dev_mac` becomes that node's AL MAC (`:241-246`).
    /// - Otherwise the node `(key.net_id, key.dev_mac)` takes it; if there is none, an agent node
    ///   is created from the device (`:250`, network `device.id.net_id`, AL MAC
    ///   `device.intf.mac`, profile `device.profile`; `colocated` is the host lookup result).
    /// - The stored record is `device` with `id = key` (`:256-257`).
    /// - `num_of_devices` of the node's network is set to the number of nodes (devices) of that
    ///   network. Intentional difference: upstream does `num_of_devices++` on every call, so a
    ///   repeated put counts the device again (`:263`, research 7.22).
    ///
    /// Returns [`Put::Inserted`] if a node was created.
    ///
    /// # Errors
    ///
    /// [`ModelError::Full`] if a node is created in a new network and 5 networks are stored.
    pub fn put_device(
        &mut self,
        key: DeviceId,
        device: Box<DeviceInfo>,
        colocated: bool,
    ) -> Result<Put, ModelError> {
        let net = NetId::from(key.net_id);
        let mut id = key;
        let controller_id = NodeId {
            net,
            al: device.intf.mac,
        };
        let by_key = NodeId {
            net,
            al: key.dev_mac,
        };
        let controller_reused = self
            .nodes
            .get(&controller_id)
            .filter(|node| node.is_controller())
            .map(|node| node.agent_al_interface_mac());
        let mut put = Put::Replaced;
        let target = if let Some(al) = controller_reused {
            id.dev_mac = al;
            controller_id
        } else if self.nodes.contains_key(&by_key) {
            by_key
        } else {
            let created = NodeInit {
                net: NetId::from(device.id.net_id),
                al: device.intf.mac,
                profile: device.profile,
                role: NodeRole::Agent,
                colocated,
            };
            self.create_data_model(created)?;
            put = Put::Inserted;
            NodeId {
                net: created.net,
                al: created.al,
            }
        };
        if let Some(node) = self.nodes.get_mut(&target) {
            node.put_device(device, id);
        }
        self.count_devices(NetId::from(id.net_id));
        Ok(put)
    }

    /// Sets `num_of_devices` of the network to the number of its nodes.
    fn count_devices(&mut self, net: NetId) {
        let devices = self.nodes.keys().filter(|id| id.net == net).count();
        if let Some(network) = self.networks.get_mut(&net) {
            network.info_mut().num_of_devices = u16::try_from(devices).unwrap_or(u16::MAX);
        }
    }

    /// `update_device` (`dm_easy_mesh_list.cpp:267-304`): replaces the device of the node `id`
    /// with `device`, keeping `uptime`, `total_mem`, `free_mem`, `cached_mem`, `cpu_load` and
    /// `cpu_temp` (`:272-281`, [`NodeModel::update_device`]).
    ///
    /// # Errors
    ///
    /// [`ModelError::NotFound`] if there is no such node: upstream does nothing and reports
    /// nothing.
    pub fn update_device(
        &mut self,
        id: &NodeId,
        device: Box<DeviceInfo>,
    ) -> Result<(), ModelError> {
        self.nodes
            .get_mut(id)
            .ok_or_else(node_not_found)?
            .update_device(device);
        Ok(())
    }

    /// `remove_device` (`dm_easy_mesh_list.cpp:226-233`): removes the node `id`, the owner of the
    /// device, and hands it back.
    ///
    /// Intentional difference (plan U2): upstream removes with the 3-part key
    /// `"net@mac@media"`, which never matches the 2-part node key, so nothing is removed
    /// (research 7.2).
    pub fn remove_device(&mut self, id: &NodeId) -> Option<Box<NodeModel>> {
        self.nodes.remove(id)
    }
}
