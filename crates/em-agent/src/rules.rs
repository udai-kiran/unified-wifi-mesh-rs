// SPDX-License-Identifier: Apache-2.0

//! The agent's orchestration rules for `dev_init` (`src/orch/em_orch_agent.cpp`).

use em_cmd::{Command, CommandType, NodeKey, NodeKind, OrchOp};
use em_core::{AgentState, NodeTable};
use em_orch::{OrchRole, RoleEffect, TransientOut};

/// Upstream's `EM_MAX_CMD_GEN_TTL` in seconds (`inc/em_base.h:112`).
const MAX_COMMAND_AGE_SECS: u64 = 10;

/// Orchestration rules of the agent role.
///
/// Kept apart from `AgentCore` so the core can lend the rules, the node table and
/// the orchestrator as disjoint borrows (0012 D10).
#[derive(Debug)]
pub struct AgentRules {
    profile: u8,
}

impl AgentRules {
    /// Creates the rules; `profile` is given to every node `dev_init` creates.
    #[must_use]
    pub const fn new(profile: u8) -> Self {
        Self { profile }
    }

    fn new_node(&self, key: NodeKey, band: Option<em_types::FreqBand>) -> RoleEffect<AgentState> {
        // Upstream's em_t constructor starts every agent node in
        // `agent_unconfigured` (`em_sm.cpp:43-46`, `em.cpp:3227`).
        RoleEffect::CreateNode {
            key,
            band,
            profile: self.profile,
            state: AgentState::Unconfigured,
        }
    }
}

impl OrchRole for AgentRules {
    type Nodes = NodeTable<AgentState>;
    type State = AgentState;

    /// `em_orch_agent.cpp:235-348`: the AL node is created without submitting
    /// the command, the radio nodes with submission.
    fn pre_process(
        &mut self,
        cmd: &Command,
        op: OrchOp,
        out: &mut Vec<RoleEffect<AgentState>>,
    ) -> bool {
        let Command::DevInit(init) = cmd;
        match op {
            OrchOp::AlInsert => {
                let key = NodeKey {
                    mac: init.al_mac,
                    kind: NodeKind::Al,
                };
                out.push(self.new_node(key, None));
                false
            }
            OrchOp::EmInsert => {
                for radio in &init.radios {
                    let key = NodeKey {
                        mac: radio.ruid,
                        kind: NodeKind::Radio,
                    };
                    out.push(self.new_node(key, Some(radio.band)));
                }
                true
            }
        }
    }

    /// Every node except the AL node, in insertion order (`em_orch_agent.cpp:320-345`).
    fn build_candidates(&self, _cmd: &Command, _op: OrchOp, nodes: &Self::Nodes) -> Vec<NodeKey> {
        nodes
            .iter()
            .filter(|node| node.key.kind != NodeKind::Al)
            .map(|node| node.key)
            .collect()
    }

    /// `em_orch_agent.cpp:174-175`: a `dev_init` is always ready.
    fn ready_for_exec(&self, _cmd: &Command, _node: NodeKey, _nodes: &Self::Nodes) -> bool {
        true
    }

    /// `em_orch_agent.cpp:76-87`.
    fn ready_for_fini(&self, _cmd: &Command, node: NodeKey, nodes: &Self::Nodes) -> bool {
        nodes.get(node).is_some_and(|node| {
            matches!(
                node.state,
                AgentState::OwconfigPending | AgentState::WscM2Processed
            ) || node.state >= AgentState::TopoSynchronized
        })
    }

    /// `em.cpp:112-113`: `dev_init` starts the node in `agent_unconfigured`.
    fn on_start(&self, _cmd: &Command, node: NodeKey, out: &mut Vec<RoleEffect<AgentState>>) {
        out.push(RoleEffect::SetState(node, AgentState::Unconfigured));
    }

    /// `em_orch_agent.cpp:42-74`, with the "until found" difference of 0012 D2.
    ///
    /// Upstream resets the start time only while DPP onboarding runs. The port
    /// resets it while any radio node is still searching, so the 10 s cancel
    /// counts from the last Response. The cancel resets only the node being
    /// iterated; the orchestrator marks the other candidates finished at once.
    fn transient(
        &self,
        _cmd: &Command,
        node: NodeKey,
        nodes: &Self::Nodes,
        _elapsed: u64,
        out: &mut TransientOut<AgentState>,
    ) {
        let any_radio_searching = nodes.iter().any(|candidate| {
            candidate.key.kind == NodeKind::Radio
                && matches!(
                    candidate.state,
                    AgentState::Unconfigured | AgentState::AutoconfigRspPending
                )
        });
        if any_radio_searching {
            out.reset_start_time();
        }
        if out.elapsed() <= MAX_COMMAND_AGE_SECS {
            return;
        }
        let Some(current) = nodes.get(node) else {
            return;
        };
        tracing::info!(
            radio = %node.mac,
            elapsed_secs = out.elapsed(),
            "dev_init cancelled: time limit exceeded"
        );
        out.cancel(CommandType::DevInit);
        let state = if current.state < AgentState::TopoSynchronized {
            AgentState::Unconfigured
        } else {
            AgentState::TopoSynchronized
        };
        out.effect(RoleEffect::SetState(node, state));
    }

    /// Upstream's hook is empty (`em_orch_agent.cpp:230-233`).
    fn pre_process_cancel(
        &self,
        _cmd: &Command,
        _node: NodeKey,
        _out: &mut Vec<RoleEffect<AgentState>>,
    ) {
    }
}
