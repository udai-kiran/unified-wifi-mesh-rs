// SPDX-License-Identifier: Apache-2.0

//! Policies: capacity 32, the insert fallback, the 8 defaults and the `dev_mac` stamp.

use super::fixtures::{agent_node, mac, policy};
use crate::NodeModel;
use crate::enums::{PolicyIdType, SteeringPolicyType};
use crate::model::error::{ModelError, Put, Table};
use crate::model::keys::PolicyKey;
use crate::model::node::MAX_POLICIES;

fn key(kind: PolicyIdType, radio: u8, node: &NodeModel) -> PolicyKey {
    PolicyKey {
        net: node.net(),
        dev_mac: node.agent_al_interface_mac(),
        radio_mac: mac(radio),
        kind,
    }
}

/// A node filled to 32 policies: 8 defaults and 24 per-radio rows.
fn full_node() -> NodeModel {
    let mut node = agent_node();
    for radio in 0..24_u8 {
        node.set_policy(policy(PolicyIdType::STEERING_PARAM, mac(100 + radio)))
            .unwrap();
    }
    assert_eq!(node.num_policies(), 32);
    node
}

// --- defaults (dm_easy_mesh_list.cpp:1664-1700, 1775-1777) ---

#[test]
fn a_new_node_has_the_eight_default_policies() {
    let node = agent_node();
    assert_eq!(node.num_policies(), 8);
    let kinds: Vec<PolicyIdType> = node.policies().map(|p| p.id.kind).collect();
    for kind in [
        PolicyIdType::AP_METRICS_REP,
        PolicyIdType::STEERING_LOCAL,
        PolicyIdType::STEERING_BTM,
        PolicyIdType::CHANNEL_SCAN,
        PolicyIdType::DEFAULT_8021Q_SETTINGS,
        PolicyIdType::UNSUCCESS_ASSOC,
        PolicyIdType::BACKHAUL_BSS_CONFIG,
        PolicyIdType::QOS_MGT,
    ] {
        assert!(kinds.contains(&kind), "{kind:?}");
        assert!(node.has_policy_type(kind));
    }
    assert!(!node.has_policy_type(PolicyIdType::RADIO_METRICS_REP));
}

#[test]
fn the_default_policies_have_their_content() {
    let node = agent_node();
    let find = |kind| node.policies().find(|p| p.id.kind == kind).unwrap();

    assert_eq!(find(PolicyIdType::AP_METRICS_REP).interval, 5);

    for kind in [PolicyIdType::STEERING_LOCAL, PolicyIdType::STEERING_BTM] {
        let steering = find(kind);
        assert_eq!(steering.sta_mac.len(), 1);
        assert!(steering.sta_mac.as_slice()[0].is_zero());
    }

    let q = find(PolicyIdType::DEFAULT_8021Q_SETTINGS);
    assert_eq!(q.def_8021q_settings.primary_vid, 1);
    assert_eq!(q.def_8021q_settings.default_pcp, 2);

    let unsuccess = find(PolicyIdType::UNSUCCESS_ASSOC);
    assert!(unsuccess.report_unassoc_sta);
    assert_eq!(unsuccess.max_reporting_rate, 30);

    for kind in [
        PolicyIdType::CHANNEL_SCAN,
        PolicyIdType::BACKHAUL_BSS_CONFIG,
        PolicyIdType::QOS_MGT,
    ] {
        let plain = find(kind);
        assert_eq!(plain.interval, 0);
        assert_eq!(plain.sta_mac.len(), 0);
        assert!(!plain.report_unassoc_sta);
    }
}

/// Upstream writes the literal `GLOBAL_NET_ID` (`dm_easy_mesh_list.cpp:446, 459`); the port uses
/// the node's network id. intentional-difference:a5-policy-net-id
#[test]
fn the_default_policies_carry_the_nodes_network_id_and_device_mac() {
    let mut init = super::fixtures::node_init(crate::model::node::NodeRole::Agent);
    init.net = super::fixtures::net("Net9");
    let node = NodeModel::new(init);
    assert!(node.policies().all(|p| p.id.net_id.as_str() == "Net9"));
    assert!(
        node.policies()
            .all(|p| p.id.dev_mac == node.agent_al_interface_mac())
    );
}

// --- set_policy (dm_easy_mesh.cpp:3393-3426) ---

/// `set_policy` stamps the device's AL MAC into the policy (`dm_easy_mesh.cpp:3405`).
#[test]
fn set_policy_stamps_the_dev_mac() {
    let mut node = agent_node();
    let mut incoming = policy(PolicyIdType::STEERING_PARAM, mac(10));
    incoming.id.dev_mac = mac(77);
    node.set_policy(incoming).unwrap();
    let stored = node
        .policy(&key(PolicyIdType::STEERING_PARAM, 10, &node))
        .unwrap();
    assert_eq!(stored.id.dev_mac, node.agent_al_interface_mac());
    assert!(
        node.policies()
            .all(|p| p.id.dev_mac == node.agent_al_interface_mac())
    );
}

#[test]
fn set_policy_replaces_the_same_key_and_inserts_a_new_one() {
    let mut node = agent_node();
    assert_eq!(
        node.set_policy(policy(PolicyIdType::STEERING_PARAM, mac(10))),
        Ok(Put::Inserted)
    );
    let mut changed = policy(PolicyIdType::STEERING_PARAM, mac(10));
    changed.util_threshold = 42;
    assert_eq!(node.set_policy(changed), Ok(Put::Replaced));
    assert_eq!(node.num_policies(), 9);
    assert_eq!(
        node.policy(&key(PolicyIdType::STEERING_PARAM, 10, &node))
            .unwrap()
            .util_threshold,
        42
    );
}

#[test]
fn policy_capacity_is_32() {
    assert_eq!(MAX_POLICIES, 32, "EM_MAX_POLICIES, inc/em_base.h:85");
}

/// `dm_easy_mesh.cpp:3411-3414`: the 33rd policy is rejected and nothing changes.
#[test]
fn set_policy_at_capacity_is_an_error_and_changes_nothing() {
    let mut node = full_node();
    let result = node.set_policy(policy(PolicyIdType::STEERING_PARAM, mac(200)));
    assert_eq!(
        result,
        Err(ModelError::Full {
            table: Table::Policy,
            capacity: 32
        })
    );
    assert_eq!(node.num_policies(), 32);
    assert!(
        node.policy(&key(PolicyIdType::STEERING_PARAM, 200, &node))
            .is_none()
    );
}

#[test]
fn updating_an_existing_policy_at_capacity_succeeds() {
    let mut node = full_node();
    let mut changed = policy(PolicyIdType::STEERING_PARAM, mac(100));
    changed.rcpi_threshold = 9;
    assert_eq!(node.set_policy(changed), Ok(Put::Replaced));
    assert_eq!(node.num_policies(), 32);
}

// --- update_policy ---

#[test]
fn update_policy_replaces_a_stored_policy_with_the_row_as_given() {
    let mut node = agent_node();
    node.set_policy(policy(PolicyIdType::STEERING_PARAM, mac(10)))
        .unwrap();
    let mut changed = policy(PolicyIdType::STEERING_PARAM, mac(10));
    changed.id.dev_mac = node.agent_al_interface_mac();
    changed.interval = 11;
    assert_eq!(node.update_policy(changed), Ok(Put::Replaced));
    assert_eq!(node.num_policies(), 9);
}

/// `update_list` falls back to the insert path when the policy is not stored (A4).
#[test]
fn update_policy_falls_back_to_insert() {
    let mut node = agent_node();
    let incoming = policy(PolicyIdType::STEERING_PARAM, mac(10));
    assert_eq!(node.update_policy(incoming), Ok(Put::Inserted));
    assert_eq!(node.num_policies(), 9);
    let stored = node
        .policy(&key(PolicyIdType::STEERING_PARAM, 10, &node))
        .unwrap();
    assert_eq!(
        stored.id.dev_mac,
        node.agent_al_interface_mac(),
        "the insert path stamps the dev_mac"
    );
}

#[test]
fn the_insert_fallback_of_update_policy_respects_the_capacity() {
    let mut node = full_node();
    assert_eq!(
        node.update_policy(policy(PolicyIdType::STEERING_PARAM, mac(200))),
        Err(ModelError::Full {
            table: Table::Policy,
            capacity: 32
        })
    );
    assert_eq!(node.num_policies(), 32);
}

// --- remove / read ---

/// Upstream removes by a formatted key (`dm_easy_mesh_list.cpp:1335-1367`); the port by the
/// typed key. intentional-difference:c4-exact-keys
#[test]
fn remove_policy_matches_the_whole_key() {
    let mut node = agent_node();
    node.set_policy(policy(PolicyIdType::STEERING_PARAM, mac(10)))
        .unwrap();
    assert!(
        node.remove_policy(&key(PolicyIdType::STEERING_PARAM, 11, &node))
            .is_none()
    );
    assert!(
        node.remove_policy(&key(PolicyIdType::STEERING_PARAM, 10, &node))
            .is_some()
    );
    assert_eq!(node.num_policies(), 8);
}

#[test]
fn policy_keys_differ_by_radio_and_type() {
    let mut node = agent_node();
    node.set_policy(policy(PolicyIdType::STEERING_PARAM, mac(10)))
        .unwrap();
    node.set_policy(policy(PolicyIdType::RADIO_METRICS_REP, mac(10)))
        .unwrap();
    node.set_policy(policy(PolicyIdType::STEERING_PARAM, mac(11)))
        .unwrap();
    assert_eq!(node.num_policies(), 11);
}

#[test]
fn has_policy_type_reports_stored_types_only() {
    let mut node = agent_node();
    assert!(!node.has_policy_type(PolicyIdType::TRAFFIC_SEPARATION));
    node.set_policy(policy(
        PolicyIdType::TRAFFIC_SEPARATION,
        crate::MacAddr::ZERO,
    ))
    .unwrap();
    assert!(node.has_policy_type(PolicyIdType::TRAFFIC_SEPARATION));
}

#[test]
fn the_per_radio_seed_policies_have_their_content() {
    let mut node = agent_node();
    node.put_radio(super::fixtures::radio(mac(10), true))
        .unwrap();
    let metrics = node
        .policy(&key(PolicyIdType::RADIO_METRICS_REP, 10, &node))
        .unwrap();
    assert_eq!(metrics.util_threshold, 60);
    assert_eq!(metrics.rcpi_threshold, 120);
    assert_eq!(metrics.rcpi_hysteresis, 5);
    assert!(metrics.sta_traffic_stats && metrics.sta_link_metric && metrics.sta_status);
    let steering = node
        .policy(&key(PolicyIdType::STEERING_PARAM, 10, &node))
        .unwrap();
    assert_eq!(steering.policy, SteeringPolicyType::RCPI_ALLOWED);
    assert_eq!(steering.util_threshold, 60);
    assert_eq!(steering.rcpi_threshold, 120);
}
