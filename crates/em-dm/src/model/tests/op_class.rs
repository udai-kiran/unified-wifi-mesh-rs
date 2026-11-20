// SPDX-License-Identifier: Apache-2.0

//! Op classes: capacity 64, `commit_config` rules, `set_channels_list`, the presets and the
//! derived reads.

use super::fixtures::{agent_node, mac, op_class};
use crate::bounded::BoundedVec;
use crate::enums::OpClassType;
use crate::model::error::{ModelError, Put, Table};
use crate::model::keys::OpClassKey;
use crate::model::node::{ChannelsListOutcome, MAX_OP_CLASSES};
use crate::{NodeModel, OpClassInfo};

/// A node with no op classes (the constructor seeds 6 presets).
fn node_without_op_classes() -> NodeModel {
    let mut node = agent_node();
    let keys: Vec<OpClassKey> = node.op_classes().iter().map(OpClassKey::of).collect();
    for key in &keys {
        node.remove_op_class(key).unwrap();
    }
    assert_eq!(node.op_classes().len(), 0);
    node
}

fn node_with_full_table() -> NodeModel {
    let mut node = node_without_op_classes();
    for number in 0..64_u32 {
        let mut info = op_class(mac(10), OpClassType::CAPABILITY, 100 + number);
        // Valid preferences, so that `set_channels_list` cannot reuse the row.
        info.pref_valid = true;
        node.put_op_class(info).unwrap();
    }
    node
}

fn channels(list: &[u32]) -> BoundedVec<u32, 64> {
    BoundedVec::try_from_vec(list.to_vec()).unwrap()
}

// --- presets ---

/// `dm_easy_mesh_list.cpp:1700-1716`.
#[test]
fn a_new_node_has_the_six_preset_op_classes_with_the_device_mac_as_ruid() {
    let node = agent_node();
    let rows: Vec<(OpClassType, u32, Vec<u32>, u8)> = node
        .op_classes()
        .iter()
        .map(|info| {
            (
                info.id.kind,
                info.op_class,
                info.channels.as_slice().to_vec(),
                info.channel_pref[0],
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            (OpClassType::ANTICIPATED, 83, vec![6], 0xe0),
            (OpClassType::ANTICIPATED, 128, vec![42], 0xe0),
            (OpClassType::ANTICIPATED, 135, vec![7], 0xe0),
            (OpClassType::SCAN_PARAM, 81, vec![3, 6, 9], 0),
            (
                OpClassType::SCAN_PARAM,
                115,
                vec![36, 40, 44, 48, 149, 153, 157, 161, 165],
                0
            ),
            (OpClassType::SCAN_PARAM, 131, vec![1], 0),
        ]
    );
    for info in node.op_classes() {
        assert_eq!(info.id.ruid, node.agent_al_interface_mac());
        assert!(info.pref_valid);
        assert_eq!(info.id.op_class, info.op_class);
    }
}

// --- capacity ---

#[test]
fn op_class_capacity_is_64() {
    assert_eq!(MAX_OP_CLASSES, 64, "EM_MAX_OPCLASS, inc/em_base.h:232");
}

/// Upstream returns -1 at 64 (`dm_easy_mesh_list.cpp:1105-1108`).
#[test]
fn put_op_class_at_capacity_is_an_error_and_changes_nothing() {
    let mut node = node_with_full_table();
    let result = node.put_op_class(op_class(mac(10), OpClassType::CAPABILITY, 999));
    assert_eq!(
        result,
        Err(ModelError::Full {
            table: Table::OpClass,
            capacity: 64
        })
    );
    assert_eq!(node.op_classes().len(), 64);
}

#[test]
fn updating_an_existing_op_class_at_capacity_succeeds() {
    let mut node = node_with_full_table();
    let mut changed = op_class(mac(10), OpClassType::CAPABILITY, 130);
    changed.channel = 77;
    assert_eq!(node.put_op_class(changed), Ok(Put::Replaced));
    assert_eq!(node.op_classes().len(), 64);
    let key = OpClassKey {
        ruid: mac(10),
        kind: OpClassType::CAPABILITY,
        op_class: 130,
    };
    assert_eq!(node.find_op_class(&key).unwrap().channel, 77);
}

#[test]
fn put_op_class_replaces_the_same_key_and_appends_a_new_one() {
    let mut node = node_without_op_classes();
    assert_eq!(
        node.put_op_class(op_class(mac(10), OpClassType::CURRENT, 115)),
        Ok(Put::Inserted)
    );
    assert_eq!(
        node.put_op_class(op_class(mac(10), OpClassType::CURRENT, 115)),
        Ok(Put::Replaced)
    );
    assert_eq!(
        node.put_op_class(op_class(mac(11), OpClassType::CURRENT, 115)),
        Ok(Put::Inserted),
        "another radio is another key"
    );
    assert_eq!(node.op_classes().len(), 2);
}

#[test]
fn update_op_class_of_an_unknown_key_is_not_found() {
    let mut node = node_without_op_classes();
    assert_eq!(
        node.update_op_class(op_class(mac(10), OpClassType::CURRENT, 115)),
        Err(ModelError::NotFound {
            table: Table::OpClass
        })
    );
}

/// Upstream's `remove_op_class` is empty (`dm_easy_mesh_list.cpp:1172-1175`); the port deletes
/// the record. intentional-difference:u2-remove
#[test]
fn remove_op_class_deletes_the_record_and_keeps_the_order() {
    let mut node = node_without_op_classes();
    for number in [1, 2, 3] {
        node.put_op_class(op_class(mac(10), OpClassType::CAPABILITY, number))
            .unwrap();
    }
    let key = OpClassKey {
        ruid: mac(10),
        kind: OpClassType::CAPABILITY,
        op_class: 2,
    };
    assert!(node.remove_op_class(&key).is_some());
    assert!(node.remove_op_class(&key).is_none());
    let numbers: Vec<u32> = node.op_classes().iter().map(|info| info.op_class).collect();
    assert_eq!(numbers, vec![1, 3]);
}

// --- commit_op_class (commit_config radio target, dm_easy_mesh.cpp:252-283) ---

#[test]
fn commit_op_class_replaces_the_same_number_and_type() {
    let mut node = node_without_op_classes();
    node.put_op_class(op_class(mac(10), OpClassType::CAPABILITY, 115))
        .unwrap();
    let mut incoming = op_class(mac(11), OpClassType::CAPABILITY, 115);
    incoming.tx_power = 9;
    assert_eq!(node.commit_op_class(incoming), Ok(Put::Replaced));
    assert_eq!(node.op_classes().len(), 1);
    assert_eq!(node.op_class(0).unwrap().id.ruid, mac(11), "replaced whole");
    assert_eq!(node.op_class(0).unwrap().tx_power, 9);
}

#[test]
fn commit_op_class_appends_a_new_number() {
    let mut node = node_without_op_classes();
    node.put_op_class(op_class(mac(10), OpClassType::CAPABILITY, 115))
        .unwrap();
    assert_eq!(
        node.commit_op_class(op_class(mac(10), OpClassType::CAPABILITY, 128)),
        Ok(Put::Inserted)
    );
    assert_eq!(node.op_classes().len(), 2);
}

/// Type 1 (`current`) is also matched by radio: one current op class per radio
/// (`dm_easy_mesh.cpp:264-267`).
#[test]
fn commit_op_class_of_type_current_replaces_the_current_row_of_the_same_radio() {
    let mut node = node_without_op_classes();
    node.put_op_class(op_class(mac(10), OpClassType::CURRENT, 115))
        .unwrap();
    assert_eq!(
        node.commit_op_class(op_class(mac(10), OpClassType::CURRENT, 128)),
        Ok(Put::Replaced)
    );
    assert_eq!(node.op_classes().len(), 1);
    assert_eq!(node.op_class(0).unwrap().op_class, 128);
}

#[test]
fn commit_op_class_of_type_current_does_not_match_another_radio() {
    let mut node = node_without_op_classes();
    node.put_op_class(op_class(mac(10), OpClassType::CURRENT, 115))
        .unwrap();
    assert_eq!(
        node.commit_op_class(op_class(mac(11), OpClassType::CURRENT, 128)),
        Ok(Put::Inserted)
    );
}

#[test]
fn the_radio_match_applies_to_type_current_only() {
    let mut node = node_without_op_classes();
    node.put_op_class(op_class(mac(10), OpClassType::CAPABILITY, 115))
        .unwrap();
    assert_eq!(
        node.commit_op_class(op_class(mac(10), OpClassType::CAPABILITY, 128)),
        Ok(Put::Inserted),
        "only type 1 matches by radio"
    );
}

/// The first stored row that satisfies either rule wins (`dm_easy_mesh.cpp:257-272`).
#[test]
fn commit_op_class_updates_the_first_row_that_matches_either_rule() {
    let mut node = node_without_op_classes();
    node.put_op_class(op_class(mac(10), OpClassType::CURRENT, 81))
        .unwrap();
    node.put_op_class(op_class(mac(11), OpClassType::CURRENT, 115))
        .unwrap();
    // Same number and type as the second row, same radio as the first: the first row wins.
    let incoming = op_class(mac(10), OpClassType::CURRENT, 115);
    assert_eq!(node.commit_op_class(incoming), Ok(Put::Replaced));
    assert_eq!(node.op_class(0).unwrap().op_class, 115);
    assert_eq!(node.op_class(1).unwrap().id.ruid, mac(11));
}

/// At 64 a new op class is refused (`dm_easy_mesh.cpp:274-275`); updates still work.
#[test]
fn commit_op_class_at_capacity_refuses_a_new_row_and_still_updates_an_existing_one() {
    let mut node = node_with_full_table();
    let refused = node.commit_op_class(op_class(mac(10), OpClassType::CAPABILITY, 999));
    assert_eq!(
        refused,
        Err(ModelError::Full {
            table: Table::OpClass,
            capacity: 64
        })
    );
    assert_eq!(node.op_classes().len(), 64);
    assert_eq!(
        node.commit_op_class(op_class(mac(10), OpClassType::CAPABILITY, 101)),
        Ok(Put::Replaced)
    );
}

// --- set_channels_list (dm_easy_mesh.cpp:3428-3483) ---

#[test]
fn set_channels_list_adds_a_new_op_class() {
    let mut node = node_without_op_classes();
    let mut incoming = op_class(mac(10), OpClassType::PREFERENCE, 115);
    incoming.channels = channels(&[36, 40]);
    let outcome = node.set_channels_list(vec![incoming]);
    assert_eq!(
        outcome,
        ChannelsListOutcome {
            applied: 1,
            skipped_full: 0
        }
    );
    assert_eq!(node.op_classes().len(), 1);
    assert!(node.op_class(0).unwrap().pref_valid);
    assert_eq!(node.op_class(0).unwrap().channels.as_slice(), &[36, 40]);
}

#[test]
fn set_channels_list_replaces_an_existing_op_class() {
    let mut node = node_without_op_classes();
    node.set_channels_list(vec![op_class(mac(10), OpClassType::PREFERENCE, 115)]);
    let mut incoming = op_class(mac(10), OpClassType::PREFERENCE, 115);
    incoming.channels = channels(&[149]);
    node.set_channels_list(vec![incoming]);
    assert_eq!(node.op_classes().len(), 1);
    assert_eq!(node.op_class(0).unwrap().channels.as_slice(), &[149]);
}

#[test]
fn set_channels_list_fills_a_zero_ruid_of_an_anticipated_op_class_with_the_device_mac() {
    let mut node = node_without_op_classes();
    node.set_channels_list(vec![op_class(
        crate::MacAddr::ZERO,
        OpClassType::ANTICIPATED,
        83,
    )]);
    assert_eq!(
        node.op_class(0).unwrap().id.ruid,
        node.agent_al_interface_mac()
    );
}

#[test]
fn set_channels_list_fills_a_zero_ruid_of_a_scan_param_op_class_only_for_those_two_types() {
    let mut node = node_without_op_classes();
    node.set_channels_list(vec![
        op_class(crate::MacAddr::ZERO, OpClassType::SCAN_PARAM, 81),
        op_class(crate::MacAddr::ZERO, OpClassType::PREFERENCE, 82),
    ]);
    assert_eq!(
        node.op_class(0).unwrap().id.ruid,
        node.agent_al_interface_mac()
    );
    assert!(node.op_class(1).unwrap().id.ruid.is_zero());
}

#[test]
fn set_channels_list_with_no_input_changes_nothing() {
    let mut node = agent_node();
    let outcome = node.set_channels_list(Vec::new());
    assert_eq!(outcome, ChannelsListOutcome::default());
    assert_eq!(node.op_classes().len(), 6);
}

#[test]
fn set_channels_list_same_radio_different_op_class_adds_a_row() {
    let mut node = node_without_op_classes();
    node.set_channels_list(vec![
        op_class(mac(10), OpClassType::PREFERENCE, 115),
        op_class(mac(10), OpClassType::PREFERENCE, 128),
    ]);
    assert_eq!(node.op_classes().len(), 2);
}

#[test]
fn set_channels_list_reuses_the_first_row_of_the_radio_and_type_without_valid_preferences() {
    let mut node = node_without_op_classes();
    let mut stale = op_class(mac(10), OpClassType::PREFERENCE, 81);
    stale.pref_valid = false;
    node.put_op_class(stale).unwrap();
    node.set_channels_list(vec![op_class(mac(10), OpClassType::PREFERENCE, 115)]);
    assert_eq!(node.op_classes().len(), 1, "the stale row was reused");
    assert_eq!(node.op_class(0).unwrap().id.op_class, 115);
    assert!(node.op_class(0).unwrap().pref_valid);
}

/// At 64 the row is skipped and the next one is tried (`dm_easy_mesh.cpp:3475-3478`).
#[test]
fn set_channels_list_at_capacity_skips_the_new_rows_and_applies_the_replacing_ones() {
    let mut node = node_with_full_table();
    let outcome = node.set_channels_list(vec![
        op_class(mac(10), OpClassType::CAPABILITY, 999),
        op_class(mac(10), OpClassType::CAPABILITY, 105),
    ]);
    assert_eq!(
        outcome,
        ChannelsListOutcome {
            applied: 1,
            skipped_full: 1
        }
    );
    assert_eq!(node.op_classes().len(), 64);
}

// --- derived reads ---

#[test]
fn update_cac_status_id_sets_the_ruid_of_the_types_above_capability_only() {
    let mut node = node_without_op_classes();
    for (kind, number) in [
        (OpClassType::CURRENT, 1),
        (OpClassType::CAPABILITY, 2),
        (OpClassType::CAC_AVAILABLE, 3),
        (OpClassType::SELECTION, 4),
    ] {
        node.put_op_class(op_class(mac(10), kind, number)).unwrap();
    }
    node.update_cac_status_id(mac(99));
    let ruids: Vec<_> = node.op_classes().iter().map(|info| info.id.ruid).collect();
    assert_eq!(ruids, vec![mac(10), mac(10), mac(99), mac(99)]);
}

#[test]
fn update_cac_status_id_with_no_op_classes_does_nothing() {
    let mut node = node_without_op_classes();
    node.update_cac_status_id(mac(99));
    assert_eq!(node.op_classes().len(), 0);
}

#[test]
fn update_cac_status_id_with_only_the_early_types_changes_nothing() {
    let mut node = node_without_op_classes();
    node.put_op_class(op_class(mac(10), OpClassType::CURRENT, 1))
        .unwrap();
    node.put_op_class(op_class(mac(10), OpClassType::CAPABILITY, 2))
        .unwrap();
    node.update_cac_status_id(mac(99));
    assert!(node.op_classes().iter().all(|info| info.id.ruid == mac(10)));
}

/// The parameter is named `bssid` but is compared with the radio MAC (`dm_easy_mesh.cpp:2986`).
#[test]
fn op_class_info_for_bss_matches_the_radio_mac_only() {
    let mut node = node_without_op_classes();
    node.put_op_class(op_class(mac(10), OpClassType::CURRENT, 115))
        .unwrap();
    node.put_op_class(op_class(mac(10), OpClassType::CAPABILITY, 128))
        .unwrap();
    let first = node.op_class_info_for_bss(mac(10), None).unwrap();
    assert_eq!(first.op_class, 115);
    assert!(node.op_class_info_for_bss(mac(20), None).is_none());
}

#[test]
fn op_class_info_for_bss_with_an_op_class_number_needs_both() {
    let mut node = node_without_op_classes();
    node.put_op_class(op_class(mac(10), OpClassType::CURRENT, 115))
        .unwrap();
    node.put_op_class(op_class(mac(10), OpClassType::CAPABILITY, 128))
        .unwrap();
    assert_eq!(
        node.op_class_info_for_bss(mac(10), Some(128))
            .unwrap()
            .id
            .kind,
        OpClassType::CAPABILITY
    );
    assert!(node.op_class_info_for_bss(mac(10), Some(999)).is_none());
    assert!(node.op_class_info_for_bss(mac(11), Some(128)).is_none());
}

#[test]
fn op_class_info_for_bss_with_no_op_classes_is_none() {
    assert!(
        node_without_op_classes()
            .op_class_info_for_bss(mac(10), None)
            .is_none()
    );
}

#[test]
fn op_class_by_index_beyond_the_count_is_none() {
    let node = agent_node();
    assert!(node.op_class(5).is_some());
    assert!(node.op_class(6).is_none());
    let _: &[OpClassInfo] = node.op_classes();
}
