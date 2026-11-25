// SPDX-License-Identifier: Apache-2.0

//! The controller's orchestration rules for `em_config` (`src/orch/em_orch_ctrl.cpp`).

use em_cmd::{Command, CommandType, NodeKey, NodeKind, OrchOp};
use em_core::{CtrlState, Node, NodeTable};
use em_orch::{OrchRole, RoleEffect, TransientOut};
use em_types::MacAddr;
use em_utils::SortedVecMap;

/// `EM_MAX_CMD_GEN_TTL + EM_MAX_CMD_EXT_TTL` in seconds (`inc/em_base.h:112-113`).
const TOPO_SYNC_LIMIT_SECS: u64 = 40;

/// What the controller knows about the agent a radio node belongs to (0013 D7).
#[derive(Debug, Clone, Copy)]
pub(crate) struct RadioLink {
    /// The agent's AL MAC, taken from the M1's sender.
    pub(crate) agent_al: MacAddr,
    /// Set when a Topology Response reported an SSID the controller does not know.
    pub(crate) ssid_mismatch: bool,
}

/// The controller's nodes and the link of each radio node to its agent.
#[derive(Debug, Default)]
pub(crate) struct CtrlNodes {
    pub(crate) table: NodeTable<CtrlState>,
    pub(crate) links: SortedVecMap<NodeKey, RadioLink>,
}

impl CtrlNodes {
    /// The radio nodes of one agent, in insertion order (`get_all_em_for_al_mac`,
    /// `em_mgr.cpp:294-310`).
    pub(crate) fn radios_of(&self, agent_al: MacAddr) -> Vec<NodeKey> {
        self.table
            .iter()
            .filter(|node| {
                node.key.kind == NodeKind::Radio
                    && self
                        .links
                        .get(&node.key)
                        .is_some_and(|link| link.agent_al == agent_al)
            })
            .map(|node| node.key)
            .collect()
    }

    pub(crate) fn state(&self, key: NodeKey) -> Option<CtrlState> {
        self.table.get(key).map(|node| node.state)
    }
}

/// Orchestration rules of the controller role.
#[derive(Debug, Default)]
pub(crate) struct CtrlRules;

impl OrchRole for CtrlRules {
    type Nodes = CtrlNodes;
    type State = CtrlState;
    type Out = Vec<RoleEffect<CtrlState>>;

    fn apply(
        &mut self,
        nodes: &mut Self::Nodes,
        effect: RoleEffect<CtrlState>,
        out: &mut Self::Out,
    ) {
        match effect {
            RoleEffect::CreateNode {
                key,
                band,
                profile,
                state,
            } => {
                nodes.table.insert(Node::new(key, state, profile, band));
            }
            RoleEffect::SetState(key, state) => {
                if let Some(node) = nodes.table.get_mut(key) {
                    node.state = state;
                }
            }
            RoleEffect::Renew(_) | RoleEffect::RestartProto(_) => out.push(effect),
        }
    }

    /// `em_orch_ctrl.cpp:593-599`: the BSS delete is pre-processed and never submitted
    /// (the database is not touched in S2); the topology sync is.
    fn pre_process(
        &mut self,
        cmd: &Command,
        op: OrchOp,
        _out: &mut Vec<RoleEffect<CtrlState>>,
    ) -> bool {
        cmd.plan()
            .iter()
            .any(|stage| stage.op == op && stage.submit)
    }

    /// `em_orch_ctrl.cpp:530-545`: the radio the command names.
    fn build_candidates(&self, cmd: &Command, _op: OrchOp, nodes: &Self::Nodes) -> Vec<NodeKey> {
        match cmd {
            Command::EmConfig(config) => nodes
                .table
                .get(config.radio)
                .filter(|node| node.key.kind == NodeKind::Radio)
                .map(|node| node.key)
                .into_iter()
                .collect(),
            _ => Vec::new(),
        }
    }

    /// `em_orch_ctrl.cpp:336-355` for `em_config`. Upstream's `orch_execute` then moves a
    /// radio from `wsc_m2_sent` to `topo_sync_pending` (`em.cpp:173-175`); `on_start` sees
    /// no node state, so that move is made here as a readiness effect, which the
    /// orchestrator applies before it starts the node.
    fn ready_for_exec(
        &self,
        cmd: &Command,
        node: NodeKey,
        nodes: &Self::Nodes,
        out: &mut Vec<RoleEffect<CtrlState>>,
    ) -> bool {
        if !matches!(cmd, Command::EmConfig(_)) {
            return false;
        }
        let Some(state) = nodes.state(node) else {
            return false;
        };
        if state == CtrlState::WscM2Sent {
            out.push(RoleEffect::SetState(node, CtrlState::TopoSyncPending));
        }
        matches!(
            state,
            CtrlState::Unconfigured
                | CtrlState::WscM2Sent
                | CtrlState::TopoSynchronized
                | CtrlState::Misconfigured
        )
    }

    /// `em_orch_ctrl.cpp:182-204`. A radio that a second M1/M2 exchange put back in
    /// `wsc_m2_sent` goes to `topo_sync_pending`, so the Query is not blocked.
    fn ready_for_fini(
        &self,
        cmd: &Command,
        node: NodeKey,
        nodes: &Self::Nodes,
        out: &mut Vec<RoleEffect<CtrlState>>,
    ) -> bool {
        if !matches!(cmd, Command::EmConfig(_)) {
            return false;
        }
        match nodes.state(node) {
            Some(CtrlState::TopoSynchronized) => true,
            Some(CtrlState::WscM2Sent) => {
                out.push(RoleEffect::SetState(node, CtrlState::TopoSyncPending));
                false
            }
            _ => false,
        }
    }

    /// The state change of `em.cpp:173-175` is made in [`Self::ready_for_exec`].
    fn on_start(&self, _cmd: &Command, _node: NodeKey, _out: &mut Vec<RoleEffect<CtrlState>>) {}

    /// `em_orch_ctrl.cpp:60-92,142-150`.
    ///
    /// A radio in `topo_sync_pending` past 40 s cancels `em_config` for every radio of its
    /// agent, marks each misconfigured and sends one renew per radio. A radio in another
    /// state past 40 s cancels the type. The SSID-mismatch branch (a 120 s path with a
    /// Query every 10 s) belongs to S3: while a mismatch is flagged nothing happens here
    /// (tag `ssid-mismatch-path-deferred`).
    fn transient(
        &self,
        cmd: &Command,
        node: NodeKey,
        nodes: &Self::Nodes,
        _elapsed: u64,
        out: &mut TransientOut<CtrlState>,
    ) {
        let Command::EmConfig(config) = cmd else {
            return;
        };
        if nodes.state(node) == Some(CtrlState::TopoSyncPending) {
            let radios = nodes.radios_of(config.agent_al);
            if radios.is_empty() {
                return;
            }
            let mismatch = radios
                .iter()
                .any(|key| nodes.links.get(key).is_some_and(|link| link.ssid_mismatch));
            if mismatch || out.elapsed() <= TOPO_SYNC_LIMIT_SECS {
                return;
            }
            tracing::warn!(
                agent = %config.agent_al,
                radios = radios.len(),
                "em_config cancelled: topology sync did not complete within 40 s"
            );
            out.cancel_scoped(CommandType::EmConfig, Vec::from(radios.as_slice()));
            for radio in radios {
                out.effect(RoleEffect::SetState(radio, CtrlState::Misconfigured));
                out.effect(RoleEffect::Renew(radio));
            }
        } else if out.elapsed() > TOPO_SYNC_LIMIT_SECS {
            tracing::warn!(radio = %node.mac, "em_config cancelled: time limit exceeded");
            out.cancel(CommandType::EmConfig);
        }
    }

    /// `em_orch_ctrl.cpp:404-416`: a cancelled `em_config` leaves the radio misconfigured.
    /// Upstream also posts a `cfg_renew` bus event; the 40 s path sends the renew itself.
    fn pre_process_cancel(
        &self,
        cmd: &Command,
        node: NodeKey,
        out: &mut Vec<RoleEffect<CtrlState>>,
    ) {
        if matches!(cmd, Command::EmConfig(_)) {
            out.push(RoleEffect::SetState(node, CtrlState::Misconfigured));
        }
    }
}
