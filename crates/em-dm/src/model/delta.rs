// SPDX-License-Identifier: Apache-2.0

//! `ModelDelta`: the model changes that cross an actor or a command boundary (plan C1, C2, F1).
//!
//! Every variant is the native em-dm value of one upstream write; decoding JSON, wire and bus
//! structs stays with the consumers (0007 D1). [`ModelDelta::apply`] takes the delta by value,
//! because the records are neither `Clone` nor `Copy`: a delta that rides in a `Command` sits in
//! an `Option` slot and is moved out when applied (0002 D3).
//!
//! # Row rule (plan C1)
//!
//! Within one delta, the rows that land and the rows that are skipped match upstream. A skipped
//! row is reported in the [`DeltaOutcome`] and is never written partially. Where upstream clamps
//! a row the port rejects and reports it (C6); where upstream returns before any write the port
//! does too.
//!
//! There is no variant for `set_ssid`, `set_radio` or `dev_test` (plan C1).

use crate::bounded::BoundedVec;
use crate::enums::OpClassType;
use crate::model::access::NodeCtx;
use crate::model::error::{ModelError, Put, Table};
use crate::model::keys::StaKey;
use crate::model::node::{FrameBody, RadioCapOutcome};
use crate::records::{
    ApMldInfo, AssocStaMldInfo, BssInfo, DeviceInfo, NetworkInfo, OpClassInfo, Policy,
    RadioCapInfo, RadioInfo, ScanResult, StaInfo, UnassocStaMetricsRsp,
};
use em_types::MacAddr;

/// Why a row of a delta was not written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// The model refused the row: a full table, or a row that is not there.
    Rejected(ModelError),
    /// Upstream returns before reaching the row: the radio table was full
    /// (`dm_easy_mesh.cpp:235-238`), so the radio's op classes are not committed. The same
    /// reason marks a capability record supplied with a radio that is already stored: upstream
    /// replaces that radio and never reads the record (`dm_easy_mesh.cpp:227-233`). It also marks
    /// each affiliated AP of an AP MLD that the model rejected: the APs are written only through
    /// their MLD, so none was tried. Their rows are in [`Table::AffiliatedAp`], indexed in the
    /// MLD's `affiliated_ap` list.
    NotAttempted,
    /// The row belongs to another radio than the delta's and upstream's loop does not select it
    /// (`dm_easy_mesh.cpp:253-254`).
    WrongRadio,
    /// An AP MLD without affiliated APs (`dm_easy_mesh.cpp:312-316`).
    NoAffiliatedAps,
    /// The affiliated AP is new and its AP MLD already has 64 (`dm_easy_mesh.cpp:3791-3795`).
    /// The row's table is [`Table::AffiliatedAp`], its `index` is the position of the AP in the
    /// `affiliated_ap` list of the delta's AP MLD, and `ap_mld` is the position of that MLD in the
    /// delta. The MLD itself and the other affiliated APs are written.
    AffiliatedApFull {
        /// Position of the AP MLD in the delta's `ap_mld` list.
        ap_mld: usize,
    },
}

/// A row that was not written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkippedRow {
    /// The table of the row.
    pub table: Table,
    /// Position of the row in its list in the delta.
    pub index: usize,
    /// Why it was not written.
    pub reason: SkipReason,
}

/// Result of [`ModelDelta::apply`].
#[derive(Debug, PartialEq, Eq, Default)]
pub struct DeltaOutcome {
    /// Rows written: inserted, replaced, or removed. Each write counts once, so an input that
    /// repeats a row counts again for each time it is written. A companion row written by the same
    /// operation counts too: a new radio counts twice, for the
    /// radio and its capability slot; `AlInsert` counts its network record and its device
    /// separately; `StaCapUpdate` counts the pending row of each link plus its consolidated row
    /// when that exists (`dm_easy_mesh.cpp:4104-4124`); `EmInsertBss` counts each AP MLD plus each
    /// affiliated AP written into it (`:3762-3795`), where a skipped affiliated AP is listed in
    /// `skipped` and not counted.
    pub applied: usize,
    /// Rows not written, with their reasons. Their tables are unchanged.
    pub skipped: Vec<SkippedRow>,
    /// For `StaCapUpdate`: the number of links whose maps were written. The caller marks
    /// `sta_list_update` as upstream does: for an MLD station only if this is above zero
    /// (`em_ctrl.cpp:359-361`), for a single link always (`:383`).
    pub links_updated: usize,
}

impl DeltaOutcome {
    fn skip(&mut self, table: Table, index: usize, reason: SkipReason) {
        self.skipped.push(SkippedRow {
            table,
            index,
            reason,
        });
    }

    fn skip_rejected(&mut self, table: Table, index: usize, error: ModelError) {
        self.skip(table, index, SkipReason::Rejected(error));
    }
}

/// The channel of one op class after a CSA beacon: the update that upstream writes to a
/// discarded local copy of the model (`src/agent/dm_easy_mesh_agent.cpp:672-674, 764`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CsaChannel {
    /// Radio of the op class (`id.ruid`).
    pub ruid: MacAddr,
    /// Op class type; the CSA path uses `current` (`:723`).
    pub kind: OpClassType,
    /// The new channel.
    pub channel: u32,
}

/// A Client Capability Report for a station: the frame body and the resolved link keys
/// (plan C2, F1). The caller resolves the MLD links (`em_ctrl.cpp:328-363`): one key for a
/// non-MLD station, one per resolved link for an MLD station.
///
/// ```compile_fail
/// fn needs_clone<T: Clone>() {}
/// needs_clone::<em_dm::StaCapUpdate>();
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct StaCapUpdate {
    links: BoundedVec<StaKey, 64>,
    frame: FrameBody,
}

impl StaCapUpdate {
    /// Creates the update. The frame body is bounded by its type (11 426 bytes); the caller
    /// rejects an over-long report before building it (0007 D4).
    #[must_use]
    pub fn new(links: BoundedVec<StaKey, 64>, frame: FrameBody) -> Self {
        Self { links, frame }
    }

    /// The link keys.
    #[must_use]
    pub fn links(&self) -> &[StaKey] {
        self.links.as_slice()
    }

    /// The frame body.
    #[must_use]
    pub fn frame(&self) -> &[u8] {
        self.frame.as_slice()
    }
}

/// A model change as a value.
///
/// ```compile_fail
/// fn needs_clone<T: Clone>() {}
/// needs_clone::<em_dm::ModelDelta>();
/// ```
///
/// ```compile_fail
/// fn needs_copy<T: Copy>() {}
/// needs_copy::<em_dm::ModelDelta>();
/// ```
///
/// A compiling control for the two probes above:
///
/// ```
/// fn needs_clone<T: Clone>() {}
/// needs_clone::<em_dm::MacAddr>();
/// let _: Option<em_dm::ModelDelta> = None;
/// ```
#[derive(Debug, PartialEq)]
pub enum ModelDelta {
    /// `CsaChannel`: the op-class channel by radio and type; fixes the lost update upstream.
    CsaChannel(CsaChannel),
    /// `merge_sta_link_metrics` (`dm_easy_mesh.cpp:4215-4227`): rows whose key is in the
    /// consolidated map replace it whole; unknown keys are skipped.
    StaLinkMetrics(
        #[allow(
            clippy::vec_box,
            reason = "a StaInfo is 2728 bytes: boxed so every handoff value stays small (0007 D3)"
        )]
        Vec<Box<StaInfo>>,
    ),
    /// `replace_assoc_sta_mld_info` (`dm_easy_mesh.cpp:3927-3954`).
    AssocStaMldRefresh(AssocStaMldInfo),
    /// `merge_assoc_sta_updates` (`dm_easy_mesh.cpp:4173-4213`): `assoc` rows move into the
    /// consolidated map, `disassoc` keys are removed from it.
    StaAggregate {
        /// Rows to add or replace.
        #[allow(
            clippy::vec_box,
            reason = "a StaInfo is 2728 bytes: boxed so every handoff value stays small (0007 D3)"
        )]
        assoc: Vec<Box<StaInfo>>,
        /// Keys to remove.
        disassoc: Vec<StaKey>,
    },
    /// The unassociated-STA metrics response: memset, then fill
    /// (`src/agent/dm_easy_mesh_agent.cpp:937-981`).
    UnassocResults(UnassocStaMetricsRsp),
    /// `update_scan_results` (`dm_easy_mesh.cpp:3599-3614`).
    ScanResults(Box<ScanResult>),
    /// `commit_config` for the `al` target and for `al_insert` (`dm_easy_mesh.cpp:207-321`,
    /// `:364-372`): the device, with `is_emplus_agent` and `extended_info` preserved; and, for the
    /// `al` target, the network record.
    AlInsert {
        /// The network record (`m_network = dm.m_network`); `None` for the command path.
        network: Option<Box<NetworkInfo>>,
        /// The device record.
        device: Box<DeviceInfo>,
    },
    /// `commit_config` for the `radio` target and for `em_insert` (`dm_easy_mesh.cpp:207-321`):
    /// one radio, its capability record and its op classes.
    EmInsertRadio {
        /// The radio.
        radio: Box<RadioInfo>,
        /// The radio's capability record; stored only when the radio is new and the record's
        /// `ruid` is the radio's MAC. A new radio otherwise gets a zeroed default
        /// (`dm_easy_mesh.cpp:240-247`); a record that is not stored is reported as a skipped
        /// [`Table::RadioCap`] row (plan D3).
        cap: Option<Box<RadioCapInfo>>,
        /// The radio's op classes.
        op_classes: Vec<OpClassInfo>,
    },
    /// `commit_config` for the `bss` target (`dm_easy_mesh.cpp:207-321`): the BSSs of one radio
    /// and the AP MLDs.
    EmInsertBss {
        /// The radio whose BSSs are committed.
        radio: MacAddr,
        /// The BSSs.
        #[allow(
            clippy::vec_box,
            reason = "a BssInfo is 768 bytes: boxed so every handoff value stays small (0007 D3)"
        )]
        bss: Vec<Box<BssInfo>>,
        /// The AP MLDs; those without affiliated APs are skipped.
        ap_mld: Vec<ApMldInfo>,
    },
    /// `set_policy` for each policy (`dm_easy_mesh.cpp:3393-3426`, used at
    /// `src/orch/em_orch_ctrl.cpp:282, 564`).
    Policies(
        #[allow(
            clippy::vec_box,
            reason = "a Policy is 464 bytes: boxed so every handoff value stays small (0007 D3)"
        )]
        Vec<Box<Policy>>,
    ),
    /// `apply_sta_cap_to_maps` for each link (`dm_easy_mesh.cpp:4104-4124`, plan C2).
    StaCapUpdate(StaCapUpdate),
}

impl ModelDelta {
    /// Applies the delta to a node and its network, consuming it. Each variant calls the node or
    /// context methods with the upstream semantics cited on the variant.
    pub fn apply(self, ctx: &mut NodeCtx<'_>) -> DeltaOutcome {
        let mut outcome = DeltaOutcome::default();
        match self {
            Self::CsaChannel(csa) => apply_csa(ctx, &csa, &mut outcome),
            Self::StaLinkMetrics(rows) => apply_link_metrics(ctx, rows, &mut outcome),
            Self::AssocStaMldRefresh(info) => match ctx.node.replace_assoc_sta_mld_info(info) {
                Ok(()) => outcome.applied += 1,
                Err(error) => outcome.skip_rejected(Table::AssocStaMld, 0, error),
            },
            Self::StaAggregate { assoc, disassoc } => {
                apply_aggregate(ctx, assoc, &disassoc, &mut outcome);
            }
            Self::UnassocResults(rsp) => {
                ctx.node.set_unassoc_sta_metrics_rsp(rsp);
                outcome.applied += 1;
            }
            Self::ScanResults(scan) => {
                ctx.update_scan_results(scan);
                outcome.applied += 1;
            }
            Self::AlInsert { network, device } => {
                if let Some(network) = network {
                    ctx.network.set_info(network);
                    outcome.applied += 1;
                }
                ctx.node.commit_device(device);
                outcome.applied += 1;
            }
            Self::EmInsertRadio {
                radio,
                cap,
                op_classes,
            } => apply_radio(ctx, radio, cap, op_classes, &mut outcome),
            Self::EmInsertBss { radio, bss, ap_mld } => {
                apply_bss(ctx, radio, bss, ap_mld, &mut outcome);
            }
            Self::Policies(policies) => {
                for (index, policy) in policies.into_iter().enumerate() {
                    match ctx.node.set_policy(policy) {
                        Ok(_) => outcome.applied += 1,
                        Err(error) => outcome.skip_rejected(Table::Policy, index, error),
                    }
                }
            }
            Self::StaCapUpdate(update) => {
                for key in &update.links {
                    let consolidated_written = ctx.node.apply_sta_cap_to_maps(key, &update.frame);
                    outcome.links_updated += 1;
                    outcome.applied += if consolidated_written { 2 } else { 1 };
                }
            }
        }
        outcome
    }
}

fn apply_csa(ctx: &mut NodeCtx<'_>, csa: &CsaChannel, outcome: &mut DeltaOutcome) {
    let target = ctx
        .node
        .op_classes()
        .iter()
        .position(|info| info.id.ruid == csa.ruid && info.id.kind == csa.kind);
    let Some(index) = target else {
        outcome.skip_rejected(
            Table::OpClass,
            0,
            ModelError::NotFound {
                table: Table::OpClass,
            },
        );
        return;
    };
    if let Some(info) = ctx.node.op_class_mut(index) {
        info.channel = csa.channel;
        outcome.applied += 1;
    }
}

#[allow(clippy::vec_box, reason = "see ModelDelta::StaLinkMetrics")]
fn apply_link_metrics(ctx: &mut NodeCtx<'_>, rows: Vec<Box<StaInfo>>, outcome: &mut DeltaOutcome) {
    for (index, row) in rows.into_iter().enumerate() {
        match ctx.node.update_sta(row) {
            Ok(()) => outcome.applied += 1,
            Err(error) => outcome.skip_rejected(Table::Sta, index, error),
        }
    }
}

#[allow(clippy::vec_box, reason = "see ModelDelta::StaAggregate")]
fn apply_aggregate(
    ctx: &mut NodeCtx<'_>,
    assoc: Vec<Box<StaInfo>>,
    disassoc: &[StaKey],
    outcome: &mut DeltaOutcome,
) {
    let merged = ctx.node.merge_assoc_sta_updates(assoc, &[]);
    outcome.applied += merged.inserted + merged.replaced;
    // Each disassoc entry is decided by its own removal, after the assoc rows are merged: a key
    // that was just added is removed and counted applied, a second entry for a removed key or a
    // key that is not stored is skipped. Every entry lands in exactly one of the two.
    for (index, key) in disassoc.iter().enumerate() {
        if ctx.node.remove_sta(key).is_some() {
            outcome.applied += 1;
        } else {
            outcome.skip_rejected(
                Table::Sta,
                index,
                ModelError::NotFound { table: Table::Sta },
            );
        }
    }
}

fn apply_radio(
    ctx: &mut NodeCtx<'_>,
    radio: Box<RadioInfo>,
    cap: Option<Box<RadioCapInfo>>,
    op_classes: Vec<OpClassInfo>,
    outcome: &mut DeltaOutcome,
) {
    let radio_mac = radio.intf.mac;
    let cap_supplied = cap.is_some();
    match ctx.node.commit_radio(radio, cap) {
        Ok(commit) => {
            // A new radio also stores a capability slot (a zeroed default if the record was not
            // usable), so it counts twice.
            outcome.applied += match commit.radio {
                Put::Inserted => 2,
                Put::Replaced => 1,
            };
            match commit.cap {
                RadioCapOutcome::WrongRadio => {
                    outcome.skip(Table::RadioCap, 0, SkipReason::WrongRadio);
                }
                RadioCapOutcome::Ignored => {
                    outcome.skip(Table::RadioCap, 0, SkipReason::NotAttempted);
                }
                RadioCapOutcome::Stored
                | RadioCapOutcome::Defaulted
                | RadioCapOutcome::NotSupplied => {}
            }
        }
        Err(error) => {
            outcome.skip_rejected(Table::Radio, 0, error);
            if cap_supplied {
                outcome.skip(Table::RadioCap, 0, SkipReason::NotAttempted);
            }
            for index in 0..op_classes.len() {
                outcome.skip(Table::OpClass, index, SkipReason::NotAttempted);
            }
            return;
        }
    }
    for (index, info) in op_classes.into_iter().enumerate() {
        if info.id.ruid != radio_mac {
            outcome.skip(Table::OpClass, index, SkipReason::WrongRadio);
            continue;
        }
        match ctx.node.commit_op_class(info) {
            Ok(_) => outcome.applied += 1,
            Err(error) => outcome.skip_rejected(Table::OpClass, index, error),
        }
    }
}

#[allow(clippy::vec_box, reason = "see ModelDelta::EmInsertBss")]
fn apply_bss(
    ctx: &mut NodeCtx<'_>,
    radio: MacAddr,
    bss: Vec<Box<BssInfo>>,
    ap_mld: Vec<ApMldInfo>,
    outcome: &mut DeltaOutcome,
) {
    for (index, row) in bss.into_iter().enumerate() {
        if row.ruid.mac != radio {
            outcome.skip(Table::Bss, index, SkipReason::WrongRadio);
            continue;
        }
        match ctx.node.commit_bss(row) {
            Ok(_) => outcome.applied += 1,
            Err(error) => outcome.skip_rejected(Table::Bss, index, error),
        }
    }
    for (index, mld) in ap_mld.into_iter().enumerate() {
        if mld.affiliated_ap.is_empty() {
            outcome.skip(Table::ApMld, index, SkipReason::NoAffiliatedAps);
            continue;
        }
        let offered_aps = mld.affiliated_ap.len();
        match ctx.node.update_ap_mld_info(mld) {
            Ok(skipped_aps) => {
                // The AP MLD row, and each affiliated AP that was written into it.
                outcome.applied += 1 + offered_aps - skipped_aps.len();
                for ap_index in skipped_aps {
                    outcome.skip(
                        Table::AffiliatedAp,
                        ap_index,
                        SkipReason::AffiliatedApFull { ap_mld: index },
                    );
                }
            }
            Err(error) => {
                outcome.skip_rejected(Table::ApMld, index, error);
                // The affiliated APs are written only through their AP MLD.
                for ap_index in 0..offered_aps {
                    outcome.skip(Table::AffiliatedAp, ap_index, SkipReason::NotAttempted);
                }
            }
        }
    }
}
