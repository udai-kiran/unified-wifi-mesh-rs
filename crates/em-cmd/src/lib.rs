// SPDX-License-Identifier: Apache-2.0

//! Shared command types and the read-only orchestration view for S1.
//!
//! The command is owned once per chain; consumers borrow it through
//! [`CommandView`]. See design note 0012 D10.

use em_types::{FreqBand, MacAddr};

/// Identifies an AL or radio node, including when they share a MAC address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeKey {
    /// Interface MAC address.
    pub mac: MacAddr,
    /// Kind of interface.
    pub kind: NodeKind,
}

/// The two node kinds used in S1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind {
    /// 1905 AL interface.
    Al,
    /// Radio interface.
    Radio,
}

/// Identifies a command chain within an orchestrator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChainId(pub u64);

/// The command kinds supported by S1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandType {
    /// Initialize an agent's device and radio nodes.
    DevInit,
}

/// Per-node orchestration state.
///
/// Upstream defines these in `inc/em_base.h:3667-3674`; its cancel is stored
/// as fini by `src/em/em.cpp:283-293` and is therefore not a separate state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrchState {
    /// No orchestration state.
    None,
    /// Available for a command.
    Idle,
    /// A stage is active but this node has not started executing it.
    Pending,
    /// The node has started executing its stage.
    Progress,
    /// The node has finished or has been cancelled.
    Fini,
}

/// Orchestration operations in the S1 device initialization plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrchOp {
    /// Create the agent's AL node.
    AlInsert,
    /// Create the agent's radio nodes.
    EmInsert,
}

/// Command result codes, corresponding to `inc/em_base.h:3694-3704`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CmdStatus {
    /// The command was accepted.
    Success,
    /// The command could not be admitted.
    NotReady,
    /// An input was invalid.
    InvalidInput,
    /// The operation timed out.
    Timeout,
    /// A MAC address was invalid.
    InvalidMac,
    /// The interface was down.
    InterfaceDown,
    /// Another error occurred.
    Other,
    /// A previous command is still in progress.
    PrevCmdInProgress,
    /// The requested operation made no change.
    NoChange,
}

/// One stage in a command plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanStage {
    /// The operation passed to the role's pre-processing hook.
    pub op: OrchOp,
    /// Upstream's default admission decision for this stage.
    pub submit: bool,
}

/// Device initialization inputs.
#[derive(Debug, PartialEq, Eq)]
pub struct DevInit {
    /// The agent's AL MAC address.
    pub al_mac: MacAddr,
    /// Radios supplied by the platform, in insertion order.
    pub radios: Vec<RadioSpec>,
}

/// A platform radio and its frequency band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadioSpec {
    /// Radio interface MAC address.
    pub ruid: MacAddr,
    /// Radio frequency band.
    pub band: FreqBand,
}

/// An immutable command owned by an orchestration chain.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    /// Initialize the agent's device and radios.
    DevInit(DevInit),
}

impl Command {
    /// Returns the command's type.
    #[must_use]
    pub const fn kind(&self) -> CommandType {
        match self {
            Self::DevInit(_) => CommandType::DevInit,
        }
    }

    /// Returns the ordered plan, including stages that only pre-process.
    ///
    /// `src/cmd/em_cmd_dev_init.cpp:46-51` specifies AL insert without
    /// submission followed by radio insert with submission.
    #[must_use]
    pub const fn plan(&self) -> &'static [PlanStage] {
        match self {
            Self::DevInit(_) => &[
                PlanStage {
                    op: OrchOp::AlInsert,
                    submit: false,
                },
                PlanStage {
                    op: OrchOp::EmInsert,
                    submit: true,
                },
            ],
        }
    }
}

/// The command stage currently executing on a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveOp {
    /// Chain owning the command.
    pub chain: ChainId,
    /// Zero-based index in the command plan.
    pub stage: usize,
    /// Command type.
    pub kind: CommandType,
}

/// Read-only access to commands and node orchestration state.
pub trait CommandView {
    /// Borrows the command owned by a chain, if it exists.
    fn command(&self, chain: ChainId) -> Option<&Command>;
    /// Returns the node's orchestration state.
    fn orch_state(&self, node: NodeKey) -> OrchState;
    /// Returns the stage executing on the node, if execution has started.
    fn active(&self, node: NodeKey) -> Option<ActiveOp>;
}

/// An empty view for roles that do not submit commands.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoCommands;

impl CommandView for NoCommands {
    fn command(&self, _chain: ChainId) -> Option<&Command> {
        None
    }

    fn orch_state(&self, _node: NodeKey) -> OrchState {
        OrchState::Idle
    }

    fn active(&self, _node: NodeKey) -> Option<ActiveOp> {
        None
    }
}

#[cfg(test)]
mod tests {
    use em_types::MacAddr;

    use super::{
        ChainId, Command, CommandType, CommandView, DevInit, NoCommands, NodeKey, NodeKind, OrchOp,
        OrchState, PlanStage,
    };

    #[test]
    fn dev_init_plan_matches_upstream() {
        let command = Command::DevInit(DevInit {
            al_mac: MacAddr::ZERO,
            radios: Vec::new(),
        });
        assert_eq!(command.kind(), CommandType::DevInit);
        assert_eq!(
            command.plan(),
            [
                PlanStage {
                    op: OrchOp::AlInsert,
                    submit: false
                },
                PlanStage {
                    op: OrchOp::EmInsert,
                    submit: true
                },
            ]
        );
    }

    #[test]
    fn no_commands_has_no_active_or_owned_command_and_nodes_are_idle() {
        let view = NoCommands;
        let node = NodeKey {
            mac: MacAddr::ZERO,
            kind: NodeKind::Al,
        };
        assert!(view.command(ChainId(0)).is_none());
        assert_eq!(view.orch_state(node), OrchState::Idle);
        assert_eq!(view.active(node), None);
    }
}
