// SPDX-License-Identifier: Apache-2.0

//! `NodeModel`: the data of one node, the port of `dm_easy_mesh_t`'s stored members
//! (`inc/dm_easy_mesh.h:50-101`).
//!
//! The network record and the SSID table are not here: they live once per network in
//! [`NetworkModel`](crate::model::network::NetworkModel) (plan U1, 0007 D2), and the operations that touch
//! both go through [`NodeCtx`](crate::model::access::NodeCtx). Every collection is bounded with the
//! upstream limit (plan C6); maps are `BTreeMap`s, so every "first" and "next" iterates in key
//! order (plan C3), where upstream iterates in hash order.
//!
//! Operations never mark the dirty set (plan C5): the caller marks, as upstream's callers do.
//!
//! # Not stored
//!
//! | Upstream member | Reason |
//! |---|---|
//! | `s_counter`, `m_instance_num`, `set_id`, `get_id` | process-global counter; a node is identified by its `NodeId` |
//! | `m_sta_map_mutex` | one actor owns the model (0004 R3) |
//! | `m_wifi_data` | OneWifi webconfig, outside em-dm (0007 D1) |
//! | `m_dpp` | DPP data belongs to `em-easyconnect` (0007 D1) |
//! | `m_cmd_ctx` | command context belongs to `em-cmd` (0002) |
//! | `m_em` | back pointer (0004 R3) |
//! | `m_network`, `m_network_ssid`, `m_num_net_ssids` | stored once per network (U1) |

mod bss;
mod interfaces;
mod mld;
mod op_class;
pub(crate) mod policy;
mod radio;
mod scan;
mod sta;

pub use op_class::ChannelsListOutcome;
pub use radio::{RadioCapOutcome, RadioCommit, RadioPut};
pub use sta::{FrameBody, StaEntry, StaMapTarget, StaMergeOutcome};

use crate::bounded::BoundedVec;
use crate::enums::HaulType;
use crate::model::dirty::DirtySet;
use crate::model::keys::{NetId, PolicyKey, ScanKey, StaKey};
use crate::records::{
    ApMldInfo, AssocStaMldInfo, BssInfo, BstaMldInfo, CacCompInfo, DeviceInfo,
    Ieee1905SecurityInfo, InterfacePreference, MloPolicy, OpClassInfo, Policy, RadioCapInfo,
    RadioInfo, ScanResult, StaInfo, TidToLinkInfo, UnassocQueryList, UnassocStaMetricEntry,
    UnassocStaMetricsRsp,
};
use em_types::{Interface, MacAddr, MediaType, ProfileType};
use std::collections::BTreeMap;

/// `EM_MAX_PLATFORMS` (`inc/em_base.h:44`).
pub const MAX_PREFERENCES: usize = 5;
/// `EM_MAX_INTERFACES` (`inc/em_base.h:42`).
pub const MAX_INTERFACES: usize = 8;
/// `EM_MAX_BANDS` (`inc/em_base.h:228`).
pub const MAX_RADIOS: usize = 3;
/// `EM_MAX_BSSS` = `EM_MAX_BANDS * 8` (`inc/em_base.h:229`).
pub const MAX_BSS: usize = 24;
/// `EM_MAX_OPCLASS` (`inc/em_base.h:232`).
pub const MAX_OP_CLASSES: usize = 64;
/// `EM_MAX_POLICIES` (`inc/em_base.h:85`).
pub const MAX_POLICIES: usize = 32;
/// `EM_MAX_AP_MLD` (`inc/em_base.h:233`); also the cap on the affiliated APs and STAs of one MLD
/// (`dm_easy_mesh.cpp:3779, 3877`).
pub const MAX_AP_MLD: usize = 64;
/// `EM_MAX_ASSOC_STA_MLD` (`inc/em_base.h:234`).
pub const MAX_ASSOC_STA_MLD: usize = 64;
/// `EM_MAX_UNASSOC_STA` (`inc/em_base.h:650`).
pub const MAX_UNASSOC_STA: usize = 64;
/// `EM_MLO_POLICY_HAUL_COUNT` (`inc/em_base.h:407`).
pub const MLO_POLICY_HAUL_COUNT: usize = 2;
/// `EM_MLO_DEFAULT_BAND_BITMAP` = `EM_RF_24GHZ | EM_RF_50GHZ | EM_RF_6GHZ` (`inc/em_base.h:154-157,
/// 408`).
pub const MLO_DEFAULT_BAND_BITMAP: u8 = 0x01 | 0x02 | 0x08;

/// `m_is_ctlr` (`inc/dm_easy_mesh.h:55`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeRole {
    /// The controller's own node.
    Controller,
    /// An agent node (colocated or remote).
    Agent,
}

/// Inputs of [`NodeModel::new`]: what upstream's `create_data_model` receives
/// (`dm_easy_mesh_list.cpp:1656`). The host lookup that decides `colocated` stays outside em-dm
/// (0007 D1), so the flag is an input (plan A5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeInit {
    /// Network of the node.
    pub net: NetId,
    /// AL MAC of the node's device.
    pub al: MacAddr,
    /// Multi-AP profile of the device (`dev->m_device_info.profile`).
    pub profile: ProfileType,
    /// Controller or agent.
    pub role: NodeRole,
    /// The agent's AL MAC exists on this host (`dm->set_colocated`, `:1703-1706`).
    pub colocated: bool,
}

/// The data of one node. Neither `Clone` nor `Copy`: it owns every table of the node.
///
/// ```compile_fail
/// fn needs_clone<T: Clone>() {}
/// needs_clone::<em_dm::NodeModel>();
/// ```
///
/// ```compile_fail
/// fn needs_copy<T: Copy>() {}
/// needs_copy::<em_dm::NodeModel>();
/// ```
///
/// A compiling control for the two probes above:
///
/// ```
/// fn needs_clone<T: Clone>() {}
/// needs_clone::<em_dm::MacAddr>();
/// let _: Option<em_dm::NodeModel> = None;
/// ```
#[derive(Debug, PartialEq)]
pub struct NodeModel {
    net: NetId,
    role: NodeRole,
    colocated: bool,
    topo_changed: bool,
    ssid_mismatch_check_time: u32,
    last_topo_query_sent_time: u32,
    msg_id: u16,
    preferences: BoundedVec<InterfacePreference, MAX_PREFERENCES>,
    interfaces: BoundedVec<Interface, MAX_INTERFACES>,
    device: Box<DeviceInfo>,
    ieee_1905_security: Ieee1905SecurityInfo,
    radios: BoundedVec<Box<RadioInfo>, MAX_RADIOS>,
    radio_caps: BoundedVec<Box<RadioCapInfo>, MAX_RADIOS>,
    bss: BoundedVec<Box<BssInfo>, MAX_BSS>,
    op_classes: BoundedVec<OpClassInfo, MAX_OP_CLASSES>,
    policies: BTreeMap<PolicyKey, Box<Policy>>,
    scan_results: BTreeMap<ScanKey, Box<ScanResult>>,
    sta_map: BTreeMap<StaKey, Box<StaInfo>>,
    sta_assoc_map: BTreeMap<StaKey, Box<StaInfo>>,
    sta_dassoc_map: BTreeMap<StaKey, Box<StaInfo>>,
    cac_comp: CacCompInfo,
    dirty: DirtySet,
    ap_mld: BoundedVec<ApMldInfo, MAX_AP_MLD>,
    bsta_mld_present: bool,
    bsta_mld: BstaMldInfo,
    assoc_sta_mld: BoundedVec<AssocStaMldInfo, MAX_ASSOC_STA_MLD>,
    tid_to_link: TidToLinkInfo,
    unassoc_sta_metrics_rsp: UnassocStaMetricsRsp,
    unassoc_query_list: UnassocQueryList,
    unassoc_sta_metrics: BoundedVec<UnassocStaMetricEntry, MAX_UNASSOC_STA>,
    mlo_policy: [MloPolicy; MLO_POLICY_HAUL_COUNT],
}

impl NodeModel {
    /// A node with empty tables and zeroed records: upstream's `dm_easy_mesh_t()` followed by
    /// `init()` (`dm_easy_mesh.cpp:4241-4276, 4330-4343`), with the MLO policy defaults of
    /// `init()` (`:4266-4271`).
    fn empty(net: NetId, role: NodeRole) -> Self {
        let mut device = Box::new(DeviceInfo::zeroed());
        device.id.net_id = net.as_text();
        Self {
            net,
            role,
            colocated: false,
            topo_changed: false,
            ssid_mismatch_check_time: 0,
            last_topo_query_sent_time: 0,
            msg_id: 0,
            preferences: BoundedVec::new(),
            interfaces: BoundedVec::new(),
            device,
            ieee_1905_security: Ieee1905SecurityInfo {
                id: MacAddr::ZERO,
                sec_cap: crate::records::Ieee1905SecurityCap {
                    onboarding_proto: 0,
                    integrity_algo: 0,
                    encryption_algo: 0,
                },
            },
            radios: BoundedVec::new(),
            radio_caps: BoundedVec::new(),
            bss: BoundedVec::new(),
            op_classes: BoundedVec::new(),
            policies: BTreeMap::new(),
            scan_results: BTreeMap::new(),
            sta_map: BTreeMap::new(),
            sta_assoc_map: BTreeMap::new(),
            sta_dassoc_map: BTreeMap::new(),
            cac_comp: CacCompInfo {
                ruid: MacAddr::ZERO,
                op_class: 0,
                channel: 0,
                status: 0,
                detected_pairs: BoundedVec::new(),
            },
            dirty: DirtySet::new(),
            ap_mld: BoundedVec::new(),
            bsta_mld_present: false,
            bsta_mld: BstaMldInfo {
                mac_addr_valid: false,
                ap_mld_mac_addr_valid: false,
                mac_addr: MacAddr::ZERO,
                ap_mld_mac_addr: MacAddr::ZERO,
                str: false,
                nstr: false,
                emlsr: false,
                emlmr: false,
                affiliated_bsta: BoundedVec::new(),
            },
            assoc_sta_mld: BoundedVec::new(),
            tid_to_link: TidToLinkInfo {
                is_bsta_config: false,
                mld_mac_addr: MacAddr::ZERO,
                tid_to_link_map_neg: false,
                tid_to_link_mapping: BoundedVec::new(),
            },
            unassoc_sta_metrics_rsp: UnassocStaMetricsRsp {
                entry: BoundedVec::new(),
            },
            unassoc_query_list: UnassocQueryList {
                opclass_list: BoundedVec::new(),
            },
            unassoc_sta_metrics: BoundedVec::new(),
            mlo_policy: [
                MloPolicy {
                    use_mld_tlv: true,
                    band_bitmap: MLO_DEFAULT_BAND_BITMAP,
                },
                MloPolicy {
                    use_mld_tlv: false,
                    band_bitmap: MLO_DEFAULT_BAND_BITMAP,
                },
            ],
        }
    }

    /// Creates a node the way `create_data_model` does (`dm_easy_mesh_list.cpp:1656-1804`),
    /// without the parts that belong to the mesh: the default network (plan C7), the network SSID
    /// copy (U1) and the controller-side network update on a colocated agent.
    ///
    /// - The device gets the AL MAC, the network id and the profile (`:1736-1759`). A controller
    ///   also gets `id.dev_mac` and the media of the network record, which is zero at this point
    ///   upstream (`:1743-1744`).
    /// - The 6 preset op classes and the 8 default policies are seeded (`:1773-1777`,
    ///   `:1664-1700`). Upstream writes the literal `"OneWifiMesh"` in every default policy; the
    ///   port uses the node's network id, which is the same on the agent's network (plan A5).
    #[must_use]
    pub fn new(init: NodeInit) -> Self {
        let mut node = Self::empty(init.net, init.role);
        node.colocated = init.colocated;
        node.device.intf.mac = init.al;
        node.device.profile = init.profile;
        if init.role == NodeRole::Controller {
            node.device.id.dev_mac = init.al;
            node.device.id.media = MediaType(0);
            node.device.backhaul_mac.media = MediaType(0);
        }
        node.seed_preset_op_classes();
        node.seed_default_policies();
        node
    }

    /// The converting constructor `dm_easy_mesh_t(const dm_network_t&)`
    /// (`dm_easy_mesh.cpp:4318-4327`): a node whose device identity is the controller's.
    ///
    /// `id.net_id`, `id.dev_mac` and `id.media`, `backhaul_alid` and `backhaul_mac` all come
    /// explicitly from the arguments. Upstream copies the whole `ctrl_id` interface over the start
    /// of `id`, which overlays the wrong type (0007 defect 3, `:4320`); that copy is not
    /// reproduced. Interface names stay empty: resolving a name from a MAC reads the host
    /// (`name_from_mac_address`, 0007 D1) and the adapter sets it through
    /// [`NodeModel::device_mut`]. The node has no op classes and no policies, as upstream.
    #[must_use]
    pub fn from_controller_identity(net: NetId, ctrl_mac: MacAddr, ctrl_media: MediaType) -> Self {
        let mut node = Self::empty(net, NodeRole::Agent);
        node.device.id.dev_mac = ctrl_mac;
        node.device.id.media = ctrl_media;
        node.device.backhaul_alid.mac = ctrl_mac;
        node.device.backhaul_alid.media = ctrl_media;
        node.device.backhaul_mac.mac = ctrl_mac;
        node.device.backhaul_mac.media = ctrl_media;
        node
    }

    /// Network the node belongs to.
    #[must_use]
    pub fn net(&self) -> NetId {
        self.net
    }

    /// `is_controller` (`inc/dm_easy_mesh.h:2619`).
    #[must_use]
    pub fn is_controller(&self) -> bool {
        self.role == NodeRole::Controller
    }

    /// `set_controller` (`inc/dm_easy_mesh.h:2618`).
    pub fn set_controller(&mut self, controller: bool) {
        self.role = if controller {
            NodeRole::Controller
        } else {
            NodeRole::Agent
        };
    }

    /// `get_colocated` (`inc/dm_easy_mesh.h:2616`).
    #[must_use]
    pub fn is_colocated(&self) -> bool {
        self.colocated
    }

    /// `set_colocated` (`inc/dm_easy_mesh.h:2606`).
    pub fn set_colocated(&mut self, colocated: bool) {
        self.colocated = colocated;
    }

    /// `get_topo_state` (`inc/dm_easy_mesh.h:105`).
    #[must_use]
    pub fn topo_state(&self) -> bool {
        self.topo_changed
    }

    /// `set_topo_state` (`inc/dm_easy_mesh.h:106`).
    pub fn set_topo_state(&mut self, state: bool) {
        self.topo_changed = state;
    }

    /// `get_ssid_mismatch_check_time` (`inc/dm_easy_mesh.h:110`).
    #[must_use]
    pub fn ssid_mismatch_check_time(&self) -> u32 {
        self.ssid_mismatch_check_time
    }

    /// `set_ssid_mismatch_check_time` (`inc/dm_easy_mesh.h:109`).
    pub fn set_ssid_mismatch_check_time(&mut self, time: u32) {
        self.ssid_mismatch_check_time = time;
    }

    /// `get_last_topo_query_sent_time` (`inc/dm_easy_mesh.h:112`).
    #[must_use]
    pub fn last_topo_query_sent_time(&self) -> u32 {
        self.last_topo_query_sent_time
    }

    /// `set_last_topo_query_sent_time` (`inc/dm_easy_mesh.h:111`).
    pub fn set_last_topo_query_sent_time(&mut self, time: u32) {
        self.last_topo_query_sent_time = time;
    }

    /// `get_msg_id` (`inc/dm_easy_mesh.h:1976`).
    #[must_use]
    pub fn msg_id(&self) -> u16 {
        self.msg_id
    }

    /// `set_msg_id` (`inc/dm_easy_mesh.h:1985`).
    pub fn set_msg_id(&mut self, id: u16) {
        self.msg_id = id;
    }

    /// The dirty set of the node (`m_db_cfg_param`, plan C5).
    #[must_use]
    pub fn dirty(&self) -> &DirtySet {
        &self.dirty
    }

    /// The dirty set, for marking and clearing.
    pub fn dirty_mut(&mut self) -> &mut DirtySet {
        &mut self.dirty
    }

    /// `get_mlo_policy` (`dm_easy_mesh.cpp:3620-3627`): the policy of the fronthaul or backhaul;
    /// `None` for every other haul type.
    #[must_use]
    pub fn mlo_policy(&self, haul: HaulType) -> Option<&MloPolicy> {
        if haul == HaulType::FRONTHAUL || haul == HaulType::BACKHAUL {
            self.mlo_policy.get(usize::from(haul.0))
        } else {
            None
        }
    }

    /// The MLO policy of `haul`, for the engine's direct writes; `None` as for
    /// [`NodeModel::mlo_policy`].
    pub fn mlo_policy_mut(&mut self, haul: HaulType) -> Option<&mut MloPolicy> {
        if haul == HaulType::FRONTHAUL || haul == HaulType::BACKHAUL {
            self.mlo_policy.get_mut(usize::from(haul.0))
        } else {
            None
        }
    }

    /// `m_cac_comp` (`inc/dm_easy_mesh.h:84`).
    #[must_use]
    pub fn cac_comp(&self) -> &CacCompInfo {
        &self.cac_comp
    }

    /// `m_cac_comp`, for the engine's direct writes.
    pub fn cac_comp_mut(&mut self) -> &mut CacCompInfo {
        &mut self.cac_comp
    }

    /// `m_tid_to_link` (`inc/dm_easy_mesh.h:95`).
    #[must_use]
    pub fn tid_to_link(&self) -> &TidToLinkInfo {
        &self.tid_to_link
    }

    /// `m_tid_to_link`, for the engine's direct writes.
    pub fn tid_to_link_mut(&mut self) -> &mut TidToLinkInfo {
        &mut self.tid_to_link
    }

    /// `m_ieee_1905_security` (`inc/dm_easy_mesh.h:67`); `get_ieee_1905_security_info` and
    /// `get_ieee_1905_security_cap` read its `id` and `sec_cap`.
    #[must_use]
    pub fn ieee_1905_security(&self) -> &Ieee1905SecurityInfo {
        &self.ieee_1905_security
    }

    /// `m_ieee_1905_security`, for the engine's direct writes.
    pub fn ieee_1905_security_mut(&mut self) -> &mut Ieee1905SecurityInfo {
        &mut self.ieee_1905_security
    }

    /// `m_unassoc_sta_metrics_rsp` (`inc/dm_easy_mesh.h:96`).
    #[must_use]
    pub fn unassoc_sta_metrics_rsp(&self) -> &UnassocStaMetricsRsp {
        &self.unassoc_sta_metrics_rsp
    }

    /// Replaces `m_unassoc_sta_metrics_rsp`: the memset and refill of
    /// `src/agent/dm_easy_mesh_agent.cpp:937-981`.
    pub fn set_unassoc_sta_metrics_rsp(&mut self, rsp: UnassocStaMetricsRsp) {
        self.unassoc_sta_metrics_rsp = rsp;
    }

    /// `m_unassoc_query_list` (`inc/dm_easy_mesh.h:97`).
    #[must_use]
    pub fn unassoc_query_list(&self) -> &UnassocQueryList {
        &self.unassoc_query_list
    }

    /// `m_unassoc_query_list`, for the engine's direct writes.
    pub fn unassoc_query_list_mut(&mut self) -> &mut UnassocQueryList {
        &mut self.unassoc_query_list
    }

    /// `m_unassoc_sta_metrics[]` and `m_num_unassoc_sta_metrics` (`inc/dm_easy_mesh.h:99-100`).
    #[must_use]
    pub fn unassoc_sta_metrics(&self) -> &[UnassocStaMetricEntry] {
        self.unassoc_sta_metrics.as_slice()
    }

    /// The unassociated-STA metrics table, for the engine's direct writes (capacity 64).
    pub fn unassoc_sta_metrics_mut(
        &mut self,
    ) -> &mut BoundedVec<UnassocStaMetricEntry, MAX_UNASSOC_STA> {
        &mut self.unassoc_sta_metrics
    }
}
