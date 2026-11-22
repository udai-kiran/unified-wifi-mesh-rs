// SPDX-License-Identifier: Apache-2.0

//! The agent's orchestration rules for `dev_init`, `onewifi_cb` and `cfg_renew`
//! (`src/orch/em_orch_agent.cpp`).

use em_cmd::{Command, CommandType, NodeKey, NodeKind, OrchOp};
use em_core::{AgentState, Node, NodeTable};
use em_orch::{OrchRole, RoleEffect, TransientOut};

use crate::wsc::WscSessions;

/// Upstream's `EM_MAX_CMD_GEN_TTL` in seconds (`inc/em_base.h:112`).
const MAX_COMMAND_AGE_SECS: u64 = 10;

/// Orchestration rules of the agent role.
///
/// Kept apart from `AgentCore` so the core can lend the rules, the node table and
/// the orchestrator as disjoint borrows (0012 D10). The rules also own the WSC
/// sessions, because applying a state effect can end a radio's exchange.
#[derive(Debug)]
pub struct AgentRules {
    profile: u8,
    pub(crate) wsc: WscSessions,
}

impl AgentRules {
    /// Creates the rules; `profile` is given to every node `dev_init` creates.
    #[must_use]
    pub fn new(profile: u8) -> Self {
        Self {
            profile,
            wsc: WscSessions::default(),
        }
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
    type Out = Vec<RoleEffect<AgentState>>;

    fn apply(
        &mut self,
        nodes: &mut Self::Nodes,
        effect: RoleEffect<AgentState>,
        out: &mut Self::Out,
    ) {
        match effect {
            RoleEffect::CreateNode {
                key,
                band,
                profile,
                state,
            } => {
                nodes.insert(Node::new(key, state, profile, band));
                out.push(RoleEffect::RestartProto(key));
            }
            RoleEffect::SetState(key, state) => {
                if let Some(node) = nodes.get_mut(key) {
                    node.state = state;
                }
                // A radio that starts over, or starts a renew, has no exchange left
                // (0013 D6: the session is cleared on every terminal path).
                if matches!(
                    state,
                    AgentState::Unconfigured | AgentState::AutoconfigRenewPending
                ) {
                    self.wsc.clear(key);
                }
            }
            RoleEffect::Renew(_) | RoleEffect::RestartProto(_) => out.push(effect),
        }
    }

    /// `em_orch_agent.cpp:235-348`: the AL node is created without submitting
    /// the command, the radio nodes with submission.
    fn pre_process(
        &mut self,
        cmd: &Command,
        op: OrchOp,
        out: &mut Vec<RoleEffect<AgentState>>,
    ) -> bool {
        let Command::DevInit(init) = cmd else {
            return cmd
                .plan()
                .iter()
                .any(|stage| stage.op == op && stage.submit);
        };
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
            _ => false,
        }
    }

    /// `dev_init`: every node except the AL node, in insertion order
    /// (`em_orch_agent.cpp:320-345`). `cfg_renew`: the named radio
    /// (`em_orch_agent.cpp:347-353`). `onewifi_cb`: the named radios that wait in
    /// `owconfig_pending` (`em_orch_agent.cpp:397-405`).
    fn build_candidates(&self, cmd: &Command, _op: OrchOp, nodes: &Self::Nodes) -> Vec<NodeKey> {
        match cmd {
            Command::DevInit(_) => nodes
                .iter()
                .filter(|node| node.key.kind != NodeKind::Al)
                .map(|node| node.key)
                .collect(),
            Command::CfgRenew(renew) => nodes
                .get(renew.radio)
                .filter(|node| node.key.kind == NodeKind::Radio)
                .map(|node| node.key)
                .into_iter()
                .collect(),
            Command::OneWifiCb(callback) => callback
                .radios
                .iter()
                .copied()
                .filter(|key| {
                    nodes
                        .get(*key)
                        .is_some_and(|node| node.state == AgentState::OwconfigPending)
                })
                .collect(),
            Command::EmConfig(_) => Vec::new(),
        }
    }

    /// `em_orch_agent.cpp:174-180`: these three commands are always ready.
    fn ready_for_exec(
        &self,
        _cmd: &Command,
        _node: NodeKey,
        _nodes: &Self::Nodes,
        _out: &mut Vec<RoleEffect<AgentState>>,
    ) -> bool {
        true
    }

    /// `em_orch_agent.cpp:76-99`.
    fn ready_for_fini(
        &self,
        cmd: &Command,
        node: NodeKey,
        nodes: &Self::Nodes,
        _out: &mut Vec<RoleEffect<AgentState>>,
    ) -> bool {
        let Some(node) = nodes.get(node) else {
            return false;
        };
        match cmd {
            // `dev_init` and `cfg_renew` finish at the same states.
            Command::DevInit(_) | Command::CfgRenew(_) => {
                matches!(
                    node.state,
                    AgentState::OwconfigPending | AgentState::WscM2Processed
                ) || node.state >= AgentState::TopoSynchronized
            }
            Command::OneWifiCb(_) => node.state == AgentState::TopoSynchronized,
            Command::EmConfig(_) => false,
        }
    }

    /// `em.cpp:112-118,181-182`: where each command puts the node when it starts.
    fn on_start(&self, cmd: &Command, node: NodeKey, out: &mut Vec<RoleEffect<AgentState>>) {
        let state = match cmd {
            Command::DevInit(_) => AgentState::Unconfigured,
            Command::OneWifiCb(_) => AgentState::OneWifiBssconfigInd,
            Command::CfgRenew(_) => AgentState::AutoconfigRenewPending,
            Command::EmConfig(_) => return,
        };
        out.push(RoleEffect::SetState(node, state));
    }

    /// `em_orch_agent.cpp:42-74`, with the "until found" difference of 0012 D2 extended
    /// to `wsc_m2_pending` (0013 D5) and the `onewifi_cb` restart of 0013 D4.
    ///
    /// Upstream resets the start time only while DPP onboarding runs. The port
    /// resets a `dev_init`'s start time while any radio node is still searching or
    /// waits for its M2, so its 10 s cancel cannot happen in an S2 flow. The cancel
    /// resets only the node being iterated; the orchestrator marks the other
    /// candidates finished at once.
    fn transient(
        &self,
        cmd: &Command,
        node: NodeKey,
        nodes: &Self::Nodes,
        _elapsed: u64,
        out: &mut TransientOut<AgentState>,
    ) {
        // intentional-difference:search-until-found
        // Upstream resets only while DPP onboarding runs (`em_orch_agent.cpp:42-74`).
        if matches!(cmd, Command::DevInit(_)) && any_radio_before_m2_processed(nodes) {
            out.reset_start_time();
        }
        if out.elapsed() <= MAX_COMMAND_AGE_SECS {
            return;
        }
        if matches!(cmd, Command::OneWifiCb(_)) {
            restart_every_radio(nodes, out);
            return;
        }
        let Some(current) = nodes.get(node) else {
            return;
        };
        tracing::info!(
            radio = %node.mac,
            elapsed_secs = out.elapsed(),
            "{} cancelled: time limit exceeded",
            command_name(cmd.kind())
        );
        out.cancel(cmd.kind());
        out.effect(RoleEffect::SetState(
            node,
            state_after_cancel(current.state),
        ));
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

/// Upstream's cancel puts a radio below `topo_synchronized` back to `unconfigured`
/// (`em_orch_agent.cpp:64-71`).
fn state_after_cancel(state: AgentState) -> AgentState {
    if state < AgentState::TopoSynchronized {
        AgentState::Unconfigured
    } else {
        AgentState::TopoSynchronized
    }
}

/// Whether a radio still searches for a controller or waits for its M2.
fn any_radio_before_m2_processed(nodes: &NodeTable<AgentState>) -> bool {
    nodes.iter().any(|candidate| {
        candidate.key.kind == NodeKind::Radio
            && matches!(
                candidate.state,
                AgentState::Unconfigured
                    | AgentState::AutoconfigRspPending
                    | AgentState::WscM2Pending
            )
    })
}

// intentional-difference:onewifi-cb-timeout-restart
//
/// Upstream cancels the command and resets the one radio being iterated
/// (`em_orch_agent.cpp:64-71`) and has no recovery. The port resets every radio of the
/// device below `topo_synchronized` and drops every `cfg_renew` of those radios, queued
/// or running; the core then submits a new `dev_init` so the search gate reopens
/// (0013 D4).
fn restart_every_radio(nodes: &NodeTable<AgentState>, out: &mut TransientOut<AgentState>) {
    tracing::info!(
        elapsed_secs = out.elapsed(),
        "onewifi_cb cancelled: time limit exceeded"
    );
    out.cancel(CommandType::OneWifiCb);
    // A queued or running cfg_renew would start (or go on) after the recovery has put
    // the radios through discovery again, send an M1 the controller never answers with a
    // Query, and time out into a third dev_init.
    let radios: Vec<NodeKey> = nodes
        .iter()
        .filter(|node| node.key.kind == NodeKind::Radio)
        .map(|node| node.key)
        .collect();
    out.cancel_scoped(CommandType::CfgRenew, radios);
    for radio in nodes.iter().filter(|node| node.key.kind == NodeKind::Radio) {
        out.effect(RoleEffect::SetState(
            radio.key,
            state_after_cancel(radio.state),
        ));
    }
}

fn command_name(kind: CommandType) -> &'static str {
    match kind {
        CommandType::DevInit => "dev_init",
        CommandType::OneWifiCb => "onewifi_cb",
        CommandType::CfgRenew => "cfg_renew",
        CommandType::EmConfig => "em_config",
    }
}
