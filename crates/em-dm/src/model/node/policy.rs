// SPDX-License-Identifier: Apache-2.0

//! Policy operations of a node (up to 32 policies keyed by [`PolicyKey`]).

use super::{MAX_POLICIES, NodeModel};
use crate::bounded::BoundedVec;
use crate::enums::{PolicyIdType, SteeringPolicyType};
use crate::float::Float32;
use crate::model::error::{ModelError, Put, Table};
use crate::model::keys::{NetId, PolicyKey};
use crate::records::{
    ClientFiltersCfg, Ieee8021qSettingsPolicy, LinkStatsAlarmCfg, Policy, PolicyId,
    TrafficSeparationPolicy,
};
use crate::text::CStrBytes;
use em_types::MacAddr;

/// A zeroed policy of `kind`: the all-zero `em_policy_t` of the C initialisers.
pub(crate) fn blank_policy(net: NetId, kind: PolicyIdType) -> Policy {
    Policy {
        id: PolicyId {
            net_id: net.as_text(),
            dev_mac: MacAddr::ZERO,
            radio_mac: MacAddr::ZERO,
            kind,
        },
        sta_mac: BoundedVec::new(),
        policy: SteeringPolicyType::DISALLOWED,
        util_threshold: 0,
        rcpi_threshold: 0,
        interval: 0,
        rcpi_hysteresis: 0,
        sta_traffic_stats: false,
        sta_link_metric: false,
        sta_status: false,
        managed_sta_marker: CStrBytes::default(),
        independent_scan_report: false,
        bssid: MacAddr::ZERO,
        profile_1_sta_disallowed: false,
        profile_2_sta_disallowed: false,
        report_unassoc_sta: false,
        max_reporting_rate: 0,
        def_8021q_settings: Ieee8021qSettingsPolicy {
            primary_vid: 0,
            default_pcp: 0,
        },
        traffic_separ: TrafficSeparationPolicy {
            ssid_info: BoundedVec::new(),
        },
        qos_mgt: BoundedVec::new(),
        backhaul_bss_config: BoundedVec::new(),
        link_stats_alarm_cfg: LinkStatsAlarmCfg {
            collection_start_time: CStrBytes::default(),
            reporting_interval: 0,
            link_quality_threshold: Float32::new(0.0),
        },
        client_filters: ClientFiltersCfg {
            sta_mac: MacAddr::ZERO,
            consec_alarm_thres_cnt: 0,
            collect_duration: CStrBytes::default(),
        },
    }
}

impl NodeModel {
    /// Seeds the 8 default device policies (`dm_easy_mesh_list.cpp:1664-1700, 1775-1777`):
    ///
    /// | Type | Non-zero fields |
    /// |---|---|
    /// | `ap_metrics_rep` | `interval` 5 |
    /// | `steering_local`, `steering_btm` | one zero STA MAC |
    /// | `channel_scan` | none |
    /// | `default_8021q_settings` | `primary_vid` 1, `default_pcp` 2 |
    /// | `unsuccess_assoc` | `report_unassoc_sta` true, `max_reporting_rate` 30 |
    /// | `backhaul_bss_config`, `qos_mgt` | none |
    ///
    /// The per-radio policies are not seeded here; `put_radio` does that.
    pub(super) fn seed_default_policies(&mut self) {
        let net = self.net;
        let mut ap_metrics = blank_policy(net, PolicyIdType::AP_METRICS_REP);
        ap_metrics.interval = 5;

        let mut steering_local = blank_policy(net, PolicyIdType::STEERING_LOCAL);
        let _ = steering_local.sta_mac.push(MacAddr::ZERO);
        let mut steering_btm = blank_policy(net, PolicyIdType::STEERING_BTM);
        let _ = steering_btm.sta_mac.push(MacAddr::ZERO);

        let mut default_8021q = blank_policy(net, PolicyIdType::DEFAULT_8021Q_SETTINGS);
        default_8021q.def_8021q_settings = Ieee8021qSettingsPolicy {
            primary_vid: 1,
            default_pcp: 2,
        };

        let mut unsuccess_assoc = blank_policy(net, PolicyIdType::UNSUCCESS_ASSOC);
        unsuccess_assoc.report_unassoc_sta = true;
        unsuccess_assoc.max_reporting_rate = 30;

        let defaults = [
            ap_metrics,
            steering_local,
            steering_btm,
            blank_policy(net, PolicyIdType::CHANNEL_SCAN),
            default_8021q,
            unsuccess_assoc,
            blank_policy(net, PolicyIdType::BACKHAUL_BSS_CONFIG),
            blank_policy(net, PolicyIdType::QOS_MGT),
        ];
        for policy in defaults {
            // The table is empty and the defaults are distinct, so none is rejected.
            let _ = self.set_policy(Box::new(policy));
        }
    }

    /// Seeds `radio_metrics_rep` and `steering_param` for the radio `radio_mac` if the node has
    /// none of that type for that radio (`dm_easy_mesh_list.cpp:430-466`). Returns how many
    /// seeds the full policy table rejected.
    ///
    /// The seeds use the node's network id; upstream writes the literal `"OneWifiMesh"`
    /// (`:446, 459`), which is the same on the agent's network (plan A5).
    pub(super) fn seed_radio_policies(&mut self, radio_mac: MacAddr) -> u8 {
        let mut rejected = 0;
        let has = |node: &Self, kind: PolicyIdType| {
            node.policies
                .values()
                .any(|policy| policy.id.radio_mac == radio_mac && policy.id.kind == kind)
        };
        if !has(self, PolicyIdType::RADIO_METRICS_REP) {
            let mut seed = blank_policy(self.net, PolicyIdType::RADIO_METRICS_REP);
            seed.id.radio_mac = radio_mac;
            seed.policy = SteeringPolicyType::DISALLOWED;
            seed.util_threshold = 60;
            seed.rcpi_threshold = 120;
            seed.rcpi_hysteresis = 5;
            seed.sta_traffic_stats = true;
            seed.sta_link_metric = true;
            seed.sta_status = true;
            if self.set_policy(Box::new(seed)).is_err() {
                rejected += 1;
            }
        }
        if !has(self, PolicyIdType::STEERING_PARAM) {
            let mut seed = blank_policy(self.net, PolicyIdType::STEERING_PARAM);
            seed.id.radio_mac = radio_mac;
            seed.policy = SteeringPolicyType::RCPI_ALLOWED;
            seed.util_threshold = 60;
            seed.rcpi_threshold = 120;
            if self.set_policy(Box::new(seed)).is_err() {
                rejected += 1;
            }
        }
        rejected
    }

    /// `set_policy` (`dm_easy_mesh.cpp:3393-3426`): the policy's `dev_mac` is set to the device's
    /// AL MAC (`:3405`), then the policy replaces the stored one with the same [`PolicyKey`], or
    /// is added. At 32 policies a new one is rejected and nothing changes (`:3411-3414`).
    ///
    /// # Errors
    ///
    /// [`ModelError::Full`] if the policy is new and 32 are stored.
    pub fn set_policy(&mut self, mut policy: Box<Policy>) -> Result<Put, ModelError> {
        policy.id.dev_mac = self.device.intf.mac;
        let key = PolicyKey::of(&policy);
        if !self.policies.contains_key(&key) && self.policies.len() >= MAX_POLICIES {
            return Err(ModelError::Full {
                table: Table::Policy,
                capacity: MAX_POLICIES,
            });
        }
        let previous = self.policies.insert(key, policy);
        Ok(if previous.is_some() {
            Put::Replaced
        } else {
            Put::Inserted
        })
    }

    /// The policy update of `dm_policy_list_t::update_list` (`dm_policy_list.cpp`, db update): a
    /// stored policy with the key of `policy` (its `dev_mac` as given) is replaced; if there is
    /// none, the policy goes through [`NodeModel::set_policy`] (the insert path), as upstream's
    /// `put_policy` does.
    ///
    /// # Errors
    ///
    /// The errors of [`NodeModel::set_policy`].
    pub fn update_policy(&mut self, policy: Box<Policy>) -> Result<Put, ModelError> {
        let key = PolicyKey::of(&policy);
        match self.policies.get_mut(&key) {
            Some(stored) => {
                *stored = policy;
                Ok(Put::Replaced)
            }
            None => self.set_policy(policy),
        }
    }

    /// Removes the policy with this key. Upstream removes by the formatted key
    /// (`dm_easy_mesh_list.cpp:1335-1367`).
    pub fn remove_policy(&mut self, key: &PolicyKey) -> Option<Box<Policy>> {
        self.policies.remove(key)
    }

    /// The policy with this key.
    #[must_use]
    pub fn policy(&self, key: &PolicyKey) -> Option<&Policy> {
        self.policies.get(key).map(|policy| &**policy)
    }

    /// The policies in key order.
    pub fn policies(&self) -> impl Iterator<Item = &Policy> {
        self.policies.values().map(|policy| &**policy)
    }

    /// The policies with the keys they are stored under, in key order. The walks of the mesh take
    /// these keys as cursors (plan D1a).
    pub fn policy_entries(&self) -> impl Iterator<Item = (&PolicyKey, &Policy)> {
        self.policies.iter().map(|(key, policy)| (key, &**policy))
    }

    /// Removes every policy and returns how many. The bulk clear of the mesh uses it, so the
    /// policies leave the node that holds them (plan D1b).
    pub(crate) fn clear_policies(&mut self) -> usize {
        let removed = self.policies.len();
        self.policies.clear();
        removed
    }

    /// `get_num_policy` (`inc/dm_easy_mesh.h:1488`).
    #[must_use]
    pub fn num_policies(&self) -> usize {
        self.policies.len()
    }

    /// `has_policy_type` (`dm_easy_mesh.cpp:186-204`): true if any stored policy has this type.
    #[must_use]
    pub fn has_policy_type(&self, kind: PolicyIdType) -> bool {
        self.policies.values().any(|policy| policy.id.kind == kind)
    }
}
