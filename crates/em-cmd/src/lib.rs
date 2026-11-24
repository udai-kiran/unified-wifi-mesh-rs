// SPDX-License-Identifier: Apache-2.0

//! Shared command types and the read-only orchestration view for S1 and S2.
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

/// The two node kinds.
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

/// The command kinds supported through S2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandType {
    /// Initialize an agent's device and radio nodes.
    DevInit,
    /// Confirm platform BSS configuration.
    OneWifiCb,
    /// Restart autoconfiguration on a radio.
    CfgRenew,
    /// Configure a controller radio after WSC.
    EmConfig,
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

/// Orchestration operations in the S1 and S2 command plans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrchOp {
    /// Create the agent's AL node.
    AlInsert,
    /// Create the agent's radio nodes.
    EmInsert,
    /// Confirm `OneWifi` configuration.
    OwconfigCnf,
    /// Transmit configuration renewal.
    TxCfgRenew,
    /// Pre-process deletion of a radio's BSS records.
    BssDelete,
    /// Synchronize the topology.
    TopoSync,
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

/// Platform-confirmed radios for one device (0013 D4).
///
/// Upstream chooses the radio named by the first BSS in its callback
/// (`src/orch/em_orch_agent.cpp:397-405`). S2 submits one callback for the
/// device, carrying all its candidate radios instead.
#[derive(Debug, PartialEq, Eq)]
pub struct OneWifiCb {
    /// Radio nodes to consider, in platform order.
    pub radios: Vec<NodeKey>,
}

/// The radio selected for an agent configuration renewal.
///
/// Upstream matches the command model's radio to a candidate
/// (`src/orch/em_orch_agent.cpp:347-353`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CfgRenew {
    /// Radio to renew.
    pub radio: NodeKey,
}

/// Controller inputs for the BSS-delete and topology-sync stages.
///
/// Upstream uses the radio and agent AL addresses for BSS deletion and the
/// radio for candidate selection (`src/orch/em_orch_ctrl.cpp:530-545,593-599`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmConfig {
    /// Radio being configured.
    pub radio: NodeKey,
    /// Agent AL address identifying the device's data model.
    pub agent_al: MacAddr,
}

/// An immutable command owned by an orchestration chain.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    /// Initialize the agent's device and radios.
    DevInit(DevInit),
    /// Confirm a device's platform configuration.
    OneWifiCb(OneWifiCb),
    /// Renew configuration for one agent radio.
    CfgRenew(CfgRenew),
    /// Configure one controller radio.
    EmConfig(EmConfig),
}

impl Command {
    /// Returns the command's type.
    #[must_use]
    pub const fn kind(&self) -> CommandType {
        match self {
            Self::DevInit(_) => CommandType::DevInit,
            Self::OneWifiCb(_) => CommandType::OneWifiCb,
            Self::CfgRenew(_) => CommandType::CfgRenew,
            Self::EmConfig(_) => CommandType::EmConfig,
        }
    }

    /// Returns the ordered plan, including stages that only pre-process.
    ///
    /// `src/cmd/em_cmd_dev_init.cpp:46-51` specifies AL insert without
    /// submission followed by radio insert with submission. The S2 plans use
    /// `src/cmd/em_cmd_onewifi_cb.cpp:47-50`,
    /// `src/cmd/em_cmd_cfg_renew.cpp:38-41`, and the first two stages of
    /// `src/cmd/em_cmd_em_config.cpp:47-52` (0013 D3).
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
            Self::OneWifiCb(_) => &[PlanStage {
                op: OrchOp::OwconfigCnf,
                submit: true,
            }],
            Self::CfgRenew(_) => &[PlanStage {
                op: OrchOp::TxCfgRenew,
                submit: true,
            }],
            Self::EmConfig(_) => &[
                PlanStage {
                    op: OrchOp::BssDelete,
                    submit: false,
                },
                PlanStage {
                    op: OrchOp::TopoSync,
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
        CfgRenew, ChainId, Command, CommandType, CommandView, DevInit, EmConfig, NoCommands,
        NodeKey, NodeKind, OneWifiCb, OrchOp, OrchState, PlanStage,
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
    fn s2_plans_match_upstream_stages_in_scope() {
        let radio = NodeKey {
            mac: MacAddr::ZERO,
            kind: NodeKind::Radio,
        };
        let cases = [
            (
                Command::OneWifiCb(OneWifiCb {
                    radios: vec![radio],
                }),
                CommandType::OneWifiCb,
                vec![PlanStage {
                    op: OrchOp::OwconfigCnf,
                    submit: true,
                }],
            ),
            (
                Command::CfgRenew(CfgRenew { radio }),
                CommandType::CfgRenew,
                vec![PlanStage {
                    op: OrchOp::TxCfgRenew,
                    submit: true,
                }],
            ),
            (
                Command::EmConfig(EmConfig {
                    radio,
                    agent_al: MacAddr::ZERO,
                }),
                CommandType::EmConfig,
                vec![
                    PlanStage {
                        op: OrchOp::BssDelete,
                        submit: false,
                    },
                    PlanStage {
                        op: OrchOp::TopoSync,
                        submit: true,
                    },
                ],
            ),
        ];
        for (command, kind, stages) in cases {
            assert_eq!(command.kind(), kind);
            assert_eq!(command.plan(), stages);
        }
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
