// SPDX-License-Identifier: Apache-2.0

//! `NetworkModel`: one network's record and its SSID table (plan U1, 0007 D2).
//!
//! Upstream keeps a copy of both in every node's `dm_easy_mesh_t` and fans updates out
//! (`dm_easy_mesh_list.cpp:959-986, 1782-1801`); here they live once, so an SSID update is one
//! write and every node of the network sees it.

use crate::bounded::BoundedVec;
use crate::enums::HaulType;
use crate::limits::SSID_CAP;
use crate::model::error::{ModelError, Put, Table};
use crate::model::keys::{NetId, SsidKey};
use crate::records::{NetworkInfo, NetworkSsidInfo};
use crate::text::CStrBytes;
use em_types::{Interface, InterfaceName, InterfaceNameError, MacAddr, MediaType};

/// `EM_MAX_NET_SSIDS` (`inc/em_base.h:41`).
pub const MAX_NET_SSIDS: usize = 5;

/// A network record and its SSID configuration.
///
/// ```compile_fail
/// fn needs_clone<T: Clone>() {}
/// needs_clone::<em_dm::NetworkModel>();
/// ```
///
/// ```compile_fail
/// fn needs_copy<T: Copy>() {}
/// needs_copy::<em_dm::NetworkModel>();
/// ```
///
/// A compiling control for the two probes above:
///
/// ```
/// fn needs_clone<T: Clone>() {}
/// needs_clone::<em_dm::MacAddr>();
/// let _: Option<em_dm::NetworkModel> = None;
/// ```
#[derive(Debug, PartialEq)]
pub struct NetworkModel {
    net: NetId,
    info: Box<NetworkInfo>,
    ssids: BoundedVec<Box<NetworkSsidInfo>, MAX_NET_SSIDS>,
}

impl NetworkModel {
    /// The default network of plan C7: a zeroed record whose id is `net`, and no SSIDs. A node
    /// created in a network that has no model gets this one (upstream leaves the node's network
    /// zeroed, `dm_easy_mesh_list.cpp:1781-1798`).
    #[must_use]
    pub fn new(net: NetId) -> Self {
        Self {
            net,
            info: Box::new(NetworkInfo::zeroed(net.as_text())),
            ssids: BoundedVec::new(),
        }
    }

    /// The id the network is stored under.
    #[must_use]
    pub fn net(&self) -> NetId {
        self.net
    }

    /// `get_network_info` / `get_network` / `get_network_by_ref` (`inc/dm_easy_mesh.h:1026,
    /// 1004, 1015`).
    #[must_use]
    pub fn info(&self) -> &NetworkInfo {
        &self.info
    }

    /// The network record, for the engine's direct writes (the key `net` does not follow
    /// `info.id`).
    pub fn info_mut(&mut self) -> &mut NetworkInfo {
        &mut self.info
    }

    /// `set_network` (`inc/dm_easy_mesh.h:770`): replaces the record whole.
    pub fn set_info(&mut self, info: Box<NetworkInfo>) {
        self.info = info;
    }

    /// `get_controller_interface` / `get_ctrl_al_interface` (`inc/dm_easy_mesh.h:782, 917`).
    #[must_use]
    pub fn controller_interface(&self) -> &Interface {
        &self.info.ctrl_id
    }

    /// `get_controller_interface_mac` / `get_ctrl_al_interface_mac` / `get_ctl_mac`
    /// (`inc/dm_easy_mesh.h:792, 926, 1052`).
    #[must_use]
    pub fn controller_interface_mac(&self) -> MacAddr {
        self.info.ctrl_id.mac
    }

    /// `get_ctrl_al_interface_name` (`inc/dm_easy_mesh.h:803`).
    #[must_use]
    pub fn controller_interface_name(&self) -> Option<&InterfaceName> {
        self.info.ctrl_id.name.as_ref()
    }

    /// `dm_network_t::set_controller_id` (`inc/dm_network.h:126`): the controller MAC.
    pub fn set_controller_id(&mut self, mac: MacAddr) {
        self.info.ctrl_id.mac = mac;
    }

    /// `set_controller_intf_media` (`inc/dm_easy_mesh.h:836`).
    pub fn set_controller_intf_media(&mut self, media: MediaType) {
        self.info.media = media;
    }

    /// `dm_network_t::get_colocated_agent_interface` (`inc/dm_network.h:151`).
    #[must_use]
    pub fn colocated_agent_interface(&self) -> &Interface {
        &self.info.colocated_agent_id
    }

    /// `set_colocated_agent_interface_mac` (`inc/dm_network.h:182`). The caller marks
    /// `network_list_update` for the colocated-agent write of `dm_easy_mesh_list.cpp:1742-1747`
    /// (plan C5).
    pub fn set_colocated_agent_interface_mac(&mut self, mac: MacAddr) {
        self.info.colocated_agent_id.mac = mac;
    }

    /// `set_colocated_agent_interface_name` (`inc/dm_network.h:193`).
    ///
    /// # Errors
    ///
    /// [`InterfaceNameError`] if `name` is not a valid interface name.
    pub fn set_colocated_agent_interface_name(
        &mut self,
        name: &str,
    ) -> Result<(), InterfaceNameError> {
        self.info.colocated_agent_id.name = Some(InterfaceName::try_from(name)?);
        Ok(())
    }

    // --- SSID table ---

    /// `m_network_ssid[0..m_num_net_ssids]` in insertion order.
    #[must_use]
    pub fn ssids(&self) -> &[Box<NetworkSsidInfo>] {
        self.ssids.as_slice()
    }

    /// `get_num_network_ssid` (`inc/dm_easy_mesh.h:1145`).
    #[must_use]
    pub fn num_network_ssids(&self) -> usize {
        self.ssids.len()
    }

    /// `get_network_ssid(index)` (`inc/dm_easy_mesh.h:1172`): `None` at and beyond the count.
    #[must_use]
    pub fn network_ssid(&self, index: usize) -> Option<&NetworkSsidInfo> {
        self.ssids.as_slice().get(index).map(|ssid| &**ssid)
    }

    /// The row with this id, matched exactly (`dm_easy_mesh_list.cpp:869-885`).
    #[must_use]
    pub fn find_network_ssid(&self, key: &SsidKey) -> Option<&NetworkSsidInfo> {
        self.ssids
            .iter()
            .map(|ssid| &**ssid)
            .find(|ssid| SsidKey::of(ssid) == *key)
    }

    /// The row with this id, for the engine's direct writes. Editing the row's `id` does not move
    /// it: it stays at its position, as upstream's array slot does (plan D1).
    pub fn network_ssid_mut(&mut self, key: &SsidKey) -> Option<&mut NetworkSsidInfo> {
        self.ssids
            .iter_mut()
            .map(|ssid| &mut **ssid)
            .find(|ssid| SsidKey::of(ssid) == *key)
    }

    /// The SSID put of `dm_easy_mesh_list_t::put_network_ssid` and `update_list`
    /// (`dm_easy_mesh_list.cpp:937-987`): the row with the same id replaces the stored one, a new
    /// id is added. Upstream does this in every node of the network; here the one table is the
    /// network's (plan U1). At 5 rows a new id is refused and nothing changes (upstream writes
    /// past the array, 0007 defect 1).
    ///
    /// # Errors
    ///
    /// [`ModelError::Full`] if the id is new and 5 rows are stored.
    pub fn put_network_ssid(&mut self, ssid: Box<NetworkSsidInfo>) -> Result<Put, ModelError> {
        let key = SsidKey::of(&ssid);
        let position = self
            .ssids
            .iter()
            .position(|stored| SsidKey::of(stored) == key);
        if let Some(index) = position {
            self.ssids.as_mut_slice()[index] = ssid;
            return Ok(Put::Replaced);
        }
        self.ssids.push(ssid).map_err(|_| ModelError::Full {
            table: Table::NetworkSsid,
            capacity: MAX_NET_SSIDS,
        })?;
        Ok(Put::Inserted)
    }

    /// The SSID remove of `dm_easy_mesh_list_t::remove_network_ssid`
    /// (`dm_easy_mesh_list.cpp:908-935`): removes the row with this id, matched exactly. Upstream
    /// matches a prefix and leaves a zeroed hole in the array while the count drops
    /// (`:925-928`); the port removes the row and keeps the order of the others.
    pub fn remove_network_ssid(&mut self, key: &SsidKey) -> Option<Box<NetworkSsidInfo>> {
        let index = self
            .ssids
            .iter()
            .position(|stored| SsidKey::of(stored) == *key)?;
        self.ssids.remove(index)
    }

    /// `get_first_network_ssid` (`dm_easy_mesh_list.cpp:812-827`): slot 0 of the table. The
    /// table is in insertion order, as upstream's array is.
    #[must_use]
    pub fn first_network_ssid(&self) -> Option<&NetworkSsidInfo> {
        self.network_ssid(0)
    }

    /// `get_next_network_ssid` (`dm_easy_mesh_list.cpp:829-867`): the row after the one at
    /// position `index`; `None` if `index` is not a stored row or is the last. The cursor is the
    /// position, as for every array-backed table: two rows can come to share an id when the
    /// engine edits one through [`NetworkModel::network_ssid_mut`], and each is still visited
    /// (plan D1a).
    #[must_use]
    pub fn next_network_ssid(&self, index: usize) -> Option<&NetworkSsidInfo> {
        self.network_ssid(index)?;
        self.network_ssid(index + 1)
    }

    /// Removes the row at `index`, keeping the order of the others. The bulk clear of the mesh
    /// removes by position (plan D1b).
    pub(crate) fn remove_network_ssid_at(&mut self, index: usize) -> Option<Box<NetworkSsidInfo>> {
        self.ssids.remove(index)
    }

    /// `is_ssid_match` (`dm_easy_mesh.cpp:2841-2851`): true if a row has exactly this SSID.
    #[must_use]
    pub fn is_ssid_match(&self, ssid: &CStrBytes<SSID_CAP>) -> bool {
        self.ssids.iter().any(|stored| stored.ssid == *ssid)
    }

    /// `get_network_ssid_info_by_haul_type` (`dm_easy_mesh.cpp:2853-2874`): the first row that
    /// carries this haul type.
    #[must_use]
    pub fn network_ssid_by_haul_type(&self, haul: HaulType) -> Option<&NetworkSsidInfo> {
        self.ssids
            .iter()
            .map(|ssid| &**ssid)
            .find(|ssid| ssid.haul_type.iter().any(|stored| *stored == haul))
    }

    /// `get_haul_type_from_ssid` (`dm_easy_mesh.cpp:2876-2894`): the first haul type of the first
    /// row with this SSID and at least one haul type. `None` for an empty SSID.
    #[must_use]
    pub fn haul_type_from_ssid(&self, ssid: &CStrBytes<SSID_CAP>) -> Option<HaulType> {
        if ssid.is_empty() {
            return None;
        }
        self.ssids
            .iter()
            .filter(|stored| stored.ssid == *ssid)
            .find_map(|stored| stored.haul_type.as_slice().first().copied())
    }
}
