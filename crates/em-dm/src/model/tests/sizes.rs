// SPDX-License-Identifier: Apache-2.0

//! Criterion 4 of note 0007 (plan A8): every stored value type and every insert and removal
//! handoff type is at most 256 bytes; the large records travel and are stored boxed.

use crate::model::delta::{CsaChannel, DeltaOutcome, SkipReason, SkippedRow, StaCapUpdate};
use crate::model::node::{ChannelsListOutcome, RadioPut, StaMergeOutcome};
use crate::{
    AgentModel, ApMldInfo, AssocStaMldInfo, BssInfo, BssKey, BstaMldInfo, CacCompInfo, DeviceInfo,
    DirtySet, Ieee1905SecurityInfo, InterfacePreference, MloPolicy, ModelDelta, ModelError, NetId,
    NetworkInfo, NetworkModel, NetworkSsidInfo, NodeCtx, NodeId, NodeInit, NodeModel, NodeRef,
    OpClassInfo, OpClassKey, Policy, PolicyKey, Put, RadioCapInfo, RadioInfo, ScanKey, ScanResult,
    SsidKey, StaInfo, StaKey, TidToLinkInfo, UnassocQueryList, UnassocStaMetricEntry,
    UnassocStaMetricsRsp,
};

use em_types::Interface;

const LIMIT: usize = 256;

fn size<T>() -> usize {
    std::mem::size_of::<T>()
}

macro_rules! assert_small {
    ($($ty:ty),+ $(,)?) => {
        $(
            assert!(
                size::<$ty>() <= LIMIT,
                "{} is {} bytes, above {LIMIT}",
                stringify!($ty),
                size::<$ty>()
            );
        )+
    };
}

macro_rules! assert_large {
    ($($ty:ty),+ $(,)?) => {
        $(
            assert!(
                size::<$ty>() > LIMIT,
                "{} is {} bytes: it fits inline, so it needs no box",
                stringify!($ty),
                size::<$ty>()
            );
        )+
    };
}

#[test]
fn model_delta_and_its_parts_are_at_most_256_bytes() {
    assert_small!(
        ModelDelta,
        StaCapUpdate,
        DeltaOutcome,
        SkippedRow,
        SkipReason,
        CsaChannel
    );
}

#[test]
fn the_models_are_small_to_move_because_their_big_parts_are_boxed() {
    assert_small!(AgentModel, NetworkModel, Box<NodeModel>, Box<NetworkModel>);
    // NodeModel is 680 bytes: it is stored boxed in the mesh model (brief 3b) and in
    // `AgentModel`, and the handoff type is the box.
    assert_large!(NodeModel);
}

#[test]
fn the_records_that_exceed_256_bytes_are_handed_off_boxed() {
    assert_large!(
        StaInfo,
        BssInfo,
        RadioInfo,
        RadioCapInfo,
        DeviceInfo,
        Policy,
        ScanResult
    );
    assert_large!(NetworkInfo, NetworkSsidInfo);
    assert_small!(
        Box<StaInfo>,
        Box<BssInfo>,
        Box<RadioInfo>,
        Box<RadioCapInfo>,
        Box<DeviceInfo>,
        Box<Policy>,
        Box<ScanResult>,
        Box<NetworkInfo>,
        Box<NetworkSsidInfo>
    );
}

#[test]
fn the_values_stored_inline_in_the_node_tables_are_at_most_256_bytes() {
    assert_small!(
        OpClassInfo,
        ApMldInfo,
        AssocStaMldInfo,
        BstaMldInfo,
        InterfacePreference,
        Interface,
        MloPolicy,
        CacCompInfo,
        TidToLinkInfo,
        UnassocStaMetricsRsp,
        UnassocQueryList,
        UnassocStaMetricEntry,
        Ieee1905SecurityInfo,
        DirtySet,
    );
}

#[test]
fn the_insert_and_removal_handoff_types_are_at_most_256_bytes() {
    assert_small!(
        // Inserts.
        Box<StaInfo>,
        Vec<Box<StaInfo>>,
        Vec<Box<BssInfo>>,
        Vec<Box<Policy>>,
        Vec<OpClassInfo>,
        OpClassInfo,
        ApMldInfo,
        AssocStaMldInfo,
        NodeInit,
        // Removals and results.
        Option<Box<StaInfo>>,
        Option<Box<BssInfo>>,
        Option<Box<RadioInfo>>,
        Option<Box<Policy>>,
        Option<Box<ScanResult>>,
        Option<Box<NetworkSsidInfo>>,
        Option<OpClassInfo>,
        Option<ApMldInfo>,
        Result<Put, ModelError>,
        Result<RadioPut, ModelError>,
        Result<usize, ModelError>,
        Result<(), ModelError>,
        ModelError,
        Put,
        RadioPut,
        StaMergeOutcome,
        ChannelsListOutcome,
        Vec<Box<StaInfo>>,
    );
}

#[test]
fn the_keys_and_the_access_handles_are_at_most_256_bytes() {
    assert_small!(
        NetId,
        NodeId,
        StaKey,
        ScanKey,
        PolicyKey,
        OpClassKey,
        BssKey,
        SsidKey,
        NodeCtx<'static>,
        NodeRef<'static>,
    );
}

/// Plan A8 for `MeshModel` (brief 3b): the model and its stored value types (the node, network
/// and capability boxes, the small side-map records and keys) and the insert, removal and result
/// types of its operations are at most 256 bytes. `NodeModel` is above the limit and is stored
/// and handed off boxed.
#[test]
fn the_mesh_model_its_stored_values_and_its_handoff_types_are_at_most_256_bytes() {
    use crate::model::keys::NodeId as Id;
    use crate::{
        DeviceId, ListOp, Located, MeshModel, NodeNeeded, Positioned, RadioPutOutcome,
        Ssid2VidChange, Ssid2VidKey, Ssid2VidMapInfo,
    };
    assert_small!(
        MeshModel,
        // Stored values of the three maps and the side maps.
        Box<NodeModel>,
        Box<NetworkModel>,
        Box<RadioCapInfo>,
        Ieee1905SecurityInfo,
        Ssid2VidMapInfo,
        Ssid2VidKey,
        // Inserts.
        DeviceId,
        ListOp,
        // Removals and results.
        Vec<Box<NodeModel>>,
        Option<Box<NodeModel>>,
        Result<Box<NetworkModel>, ModelError>,
        Result<Put, ModelError>,
        Result<RadioPutOutcome, ModelError>,
        RadioPutOutcome,
        NodeNeeded,
        Ssid2VidChange,
        Located<'static, Id, StaInfo, &'static crate::StaKey>,
        Located<'static, Id, ScanResult, &'static crate::ScanKey>,
        Located<'static, Id, Policy, &'static crate::PolicyKey>,
        Located<'static, Id, OpClassInfo>,
        Positioned<'static, NetworkSsidInfo, NetId>,
        crate::StaEntry<'static>,
        Result<crate::RadioCommit, ModelError>,
        Option<(Ssid2VidKey, &'static Ssid2VidMapInfo)>,
    );
    assert_large!(NodeModel);
}
