// SPDX-License-Identifier: Apache-2.0

//! Radio operations of `MeshModel` (`dm_easy_mesh_list.cpp:306-512`).

use super::{Located, MeshModel, Positioned};
use crate::model::error::{ModelError, Table};
use crate::model::keys::NodeId;
use crate::model::node::RadioPut;
use crate::records::RadioInfo;
use em_types::{FreqBand, MacAddr, ProfileType, ServiceType};

/// What upstream's `put_radio` passes to `m_mgr->create_node` (`dm_easy_mesh_list.cpp:470`,
/// plan A6): a node (`em_t`) is needed for this radio. The mesh creates none (0002 D8).
///
/// `create_node(&radio.intf, band, dm, false, profile_3, service_ctrl)`: `interface` is the
/// stored radio's `intf`, read through `radio` (an `Interface` is not `Clone`, and the caller
/// resolves the interface name from the MAC, `em_mgr.cpp:207`); `dm` is `node`; `is_al_mac` is
/// always false.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeNeeded {
    /// The node (`dm`) the radio belongs to.
    pub node: NodeId,
    /// The MAC of the radio's interface (`&pradio->m_radio_info.intf`): read the interface from
    /// the stored radio of `node` with this MAC.
    pub radio: MacAddr,
    /// `media_data.band` of the radio, as `em_freq_band_t` (`:470`).
    pub band: FreqBand,
    /// Always `em_profile_type_3`.
    pub profile: ProfileType,
    /// Always `em_service_type_ctrl`.
    pub service: ServiceType,
}

/// Result of [`MeshModel::put_radio`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadioPutOutcome {
    /// What the node did with the radio and the seeded policies.
    pub put: RadioPut,
    /// The `create_node` call upstream makes after every successful put.
    pub node_needed: NodeNeeded,
}

impl MeshModel {
    /// `get_radio(key)` (`dm_easy_mesh_list.cpp:365-392`): the first radio, in node key order,
    /// whose `intf.mac` is `mac`, whatever its network (plan C4, research 7.7).
    #[must_use]
    pub fn get_radio(&self, mac: MacAddr) -> Option<Located<'_, NodeId, RadioInfo>> {
        self.nodes.iter().find_map(|(id, node)| {
            node.radio_by_mac(mac).map(|item| Located {
                at: *id,
                key: (),
                item,
            })
        })
    }

    /// `get_first_radio(net, al)` (`dm_easy_mesh_list.cpp:476-486`): the first radio of the node
    /// `id`.
    #[must_use]
    pub fn first_radio_of(&self, id: &NodeId) -> Option<Positioned<'_, RadioInfo>> {
        let item = self.nodes.get(id)?.radio(0)?;
        Some(Positioned {
            at: *id,
            index: 0,
            item,
        })
    }

    /// `get_next_radio(net, al, radio)` (`dm_easy_mesh_list.cpp:488-512`): the radio after the one
    /// at position `index` of the node `id`. `None` if `index` is not a stored radio or is the
    /// last. The cursor is the position, as for every array-backed walk.
    #[must_use]
    pub fn next_radio_of(&self, id: &NodeId, index: usize) -> Option<Positioned<'_, RadioInfo>> {
        let node = self.nodes.get(id)?;
        node.radio(index)?;
        let next = index + 1;
        let item = node.radio(next)?;
        Some(Positioned {
            at: *id,
            index: next,
            item,
        })
    }

    /// `put_radio` (`dm_easy_mesh_list.cpp:399-474`): stores `radio` in the node
    /// `(radio.id.net_id, radio.id.dev_mac)` ([`NodeModel::put_radio`](crate::NodeModel::put_radio):
    /// replace by MAC, else append at most 3, seed the two default policies) and returns the
    /// [`NodeNeeded`] of `create_node` (`:470`), creating no node. The key argument of upstream
    /// is the radio's own MAC.
    ///
    /// Intentional difference (research 7.7): upstream looks the radio up in every node
    /// (`get_radio(key)`, `:418`), so a radio with the same MAC in another network is
    /// overwritten and the target node's count is unchanged (`:428`). The port replaces only
    /// within the target node.
    ///
    /// # Errors
    ///
    /// [`ModelError::NotFound`] if the node does not exist (upstream returns, `:407-413`);
    /// [`ModelError::Full`] if the radio is new and the node has 3 radios.
    pub fn put_radio(&mut self, radio: Box<RadioInfo>) -> Result<RadioPutOutcome, ModelError> {
        let node_id = NodeId {
            net: radio.id.net_id.into(),
            al: radio.id.dev_mac,
        };
        let radio_mac = radio.intf.mac;
        let band = FreqBand(radio.media_data.band);
        let node = self
            .nodes
            .get_mut(&node_id)
            .ok_or(ModelError::NotFound { table: Table::Node })?;
        let put = node.put_radio(radio)?;
        Ok(RadioPutOutcome {
            put,
            node_needed: NodeNeeded {
                node: node_id,
                radio: radio_mac,
                band,
                profile: ProfileType::MULTI_AP_PROFILE_3,
                service: ServiceType::MULTI_AP_CONTROLLER,
            },
        })
    }

    /// `remove_radio` (`dm_easy_mesh_list.cpp:394-397`): removes the first radio (node key
    /// order) whose `intf.mac` is `mac`, with its capability record.
    ///
    /// Intentional difference (plan U2): upstream's body is empty.
    pub fn remove_radio(&mut self, mac: MacAddr) -> Option<Box<RadioInfo>> {
        self.nodes
            .values_mut()
            .find(|node| node.radio_by_mac(mac).is_some())?
            .remove_radio(mac)
    }

    /// The radio update of `dm_radio_list_t::update_list` (`dm_radio_list.cpp:146-171`) on the
    /// first radio with the MAC of `radio`: `radio_temp` keeps its value
    /// ([`NodeModel::update_radio`](crate::NodeModel::update_radio)).
    ///
    /// # Errors
    ///
    /// [`ModelError::NotFound`] if no node has the radio: upstream dereferences null.
    pub fn update_radio(&mut self, radio: Box<RadioInfo>) -> Result<(), ModelError> {
        self.nodes
            .values_mut()
            .find(|node| node.radio_by_mac(radio.intf.mac).is_some())
            .ok_or(ModelError::NotFound {
                table: Table::Radio,
            })?
            .update_radio(radio)
    }
}
