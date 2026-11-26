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

    /// `em_orch_ctrl.cpp:60-92,142-150`. Two rules:
    ///
    /// (a) A radio in `topo_sync_pending` past 40 s: every radio of its agent is set
    /// misconfigured and sent its first Renew, and `em_config` is cancelled for them
    /// ([`cancel_em_config_of_agent`]); upstream's 40 s path does the same itself
    /// (`em_orch_ctrl.cpp:75-92`). While a mismatch is flagged nothing happens here: the
    /// SSID-mismatch path (a 120 s limit with a Query every 10 s) belongs to S3 (tag
    /// `ssid-mismatch-path-deferred`).
    ///
    /// (b) A radio in any other state past 40 s: `em_config` is cancelled for the radios of
    /// its agent and nothing else is done here. Upstream cancels the command type
    /// (`:142-150`), and its cancel hook ([`Self::pre_process_cancel`]) marks and renews the
    /// candidates of the active stages only; a stage still pending is dropped silently
    /// (`em_orch.cpp:278-302`). The mismatch flag is not checked.
    ///
    /// intentional-difference:em-config-age-per-agent. Upstream compares one elapsed time
    /// per command type (`em_orch.cpp:72-88,129-138`); here each stage is compared with its
    /// own age (`stage_elapsed`), which a fresh M1 or M2 of the agent restarts.
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
            if mismatch || out.stage_elapsed() <= TOPO_SYNC_LIMIT_SECS {
                return;
            }
            tracing::warn!(
                agent = %config.agent_al,
                radios = radios.len(),
                "em_config cancelled: topology sync did not complete within 40 s"
            );
            cancel_em_config_of_agent(out, radios);
        } else if out.stage_elapsed() > TOPO_SYNC_LIMIT_SECS {
            tracing::warn!(radio = %node.mac, "em_config cancelled: time limit exceeded");
            // intentional-difference:em-config-cancel-per-agent
            // Upstream cancels the whole command type here (`em_orch_ctrl.cpp:142-150`), so
            // one stuck radio cancels every agent's `em_config`. Only the stuck radio's
            // agent is cancelled. The radios are marked and renewed by the cancel hook, for
            // active stages only, as upstream (`em_orch.cpp:270-302`).
            out.cancel_scoped(CommandType::EmConfig, nodes.radios_of(config.agent_al));
        }
    }

    /// `em_orch_ctrl.cpp:404-416`: a cancelled `em_config` leaves the radio misconfigured.
    ///
    /// Every cancel of a radio's `em_config` that reaches an active stage runs this hook and
    /// starts one Renew retry episode. The 40 s cancel of [`Self::transient`] has started it
    /// for its radios already; the second start in the same instant does nothing. A stage
    /// still pending is dropped without this hook, as upstream. Upstream posts a
    /// `cfg_renew` bus event and also sends the Renew itself on the 40 s path
    /// (`em_orch_ctrl.cpp:75-92`); the controller has no `cfg_renew` command (tag
    /// `renew-retry-without-cfg-renew-command`).
    fn pre_process_cancel(
        &self,
        cmd: &Command,
        node: NodeKey,
        out: &mut Vec<RoleEffect<CtrlState>>,
    ) {
        if matches!(cmd, Command::EmConfig(_)) {
            out.push(RoleEffect::SetState(node, CtrlState::Misconfigured));
            out.push(RoleEffect::Renew(node));
        }
    }
}

/// The 40 s `topo_sync_pending` path: marks the given radios misconfigured, sends each its
/// first Renew, and cancels their `em_config` (`em_orch_ctrl.cpp:75-92`). The effects come
/// first because a radio whose stage is still pending is removed by the cancel without a
/// cancel hook. The Renews go out in the order of `radios`, as upstream
/// enumerates the radios (`em_orch_ctrl.cpp:81-90`); the hook's own Renew requests of the
/// scoped cancel come later in the same instant and are dropped.
fn cancel_em_config_of_agent(out: &mut TransientOut<CtrlState>, radios: Vec<NodeKey>) {
    for radio in &radios {
        out.effect(RoleEffect::SetState(*radio, CtrlState::Misconfigured));
        out.effect(RoleEffect::Renew(*radio));
    }
    out.cancel_scoped(CommandType::EmConfig, radios);
}
