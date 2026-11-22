// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::time::{Duration, Instant};

use em_cmd::{
    ActiveOp, ChainId, Command, CommandType, CommandView, DevInit, NodeKey, NodeKind, OrchOp,
    OrchState, RadioSpec,
};
use em_types::{FreqBand, MacAddr};

use crate::{OrchRole, Orchestrator, RoleEffect, TransientOut};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Created,
    Searching,
    Finished,
    Cancelled,
    Reset,
}

#[derive(Debug, Clone, Copy)]
struct Node {
    key: NodeKey,
    state: State,
}

#[derive(Clone, Copy)]
enum CandidateSelection {
    All,
    Command,
}

struct TestRole {
    preprocessed: Vec<OrchOp>,
    ready: bool,
    reset_searching: bool,
    cancel_first: bool,
    selection: CandidateSelection,
    calls: RefCell<Vec<(NodeKey, u64)>>,
    reset_values: RefCell<Vec<u64>>,
}

impl Default for TestRole {
    fn default() -> Self {
        Self {
            preprocessed: Vec::new(),
            ready: true,
            reset_searching: false,
            cancel_first: false,
            selection: CandidateSelection::All,
            calls: RefCell::new(Vec::new()),
            reset_values: RefCell::new(Vec::new()),
        }
    }
}

impl OrchRole for TestRole {
    type Nodes = Vec<Node>;
    type State = State;

    fn pre_process(&mut self, cmd: &Command, op: OrchOp, out: &mut Vec<RoleEffect<State>>) -> bool {
        self.preprocessed.push(op);
        let Command::DevInit(init) = cmd;
        match op {
            OrchOp::AlInsert => {
                out.push(RoleEffect::CreateNode {
                    key: NodeKey {
                        mac: init.al_mac,
                        kind: NodeKind::Al,
                    },
                    band: None,
                    profile: 3,
                    state: State::Created,
                });
                false
            }
            OrchOp::EmInsert => {
                for radio in &init.radios {
                    out.push(RoleEffect::CreateNode {
                        key: NodeKey {
                            mac: radio.ruid,
                            kind: NodeKind::Radio,
                        },
                        band: Some(radio.band),
                        profile: 3,
                        state: State::Created,
                    });
                }
                true
            }
        }
    }

    fn build_candidates(&self, cmd: &Command, _op: OrchOp, nodes: &Vec<Node>) -> Vec<NodeKey> {
        let Command::DevInit(init) = cmd;
        nodes
            .iter()
            .filter(|node| {
                node.key.kind == NodeKind::Radio
                    && (matches!(self.selection, CandidateSelection::All)
                        || init.radios.iter().any(|radio| radio.ruid == node.key.mac))
            })
            .map(|node| node.key)
            .collect()
    }

    fn ready_for_exec(&self, _cmd: &Command, _node: NodeKey, _nodes: &Vec<Node>) -> bool {
        self.ready
    }

    fn ready_for_fini(&self, _cmd: &Command, node: NodeKey, nodes: &Vec<Node>) -> bool {
        nodes
            .iter()
            .any(|entry| entry.key == node && entry.state == State::Finished)
    }

    fn on_start(&self, _cmd: &Command, node: NodeKey, out: &mut Vec<RoleEffect<State>>) {
        out.push(RoleEffect::SetState(node, State::Searching));
    }

    fn transient(
        &self,
        _cmd: &Command,
        node: NodeKey,
        nodes: &Vec<Node>,
        elapsed: u64,
        out: &mut TransientOut<State>,
    ) {
        self.calls.borrow_mut().push((node, elapsed));
        if self.reset_searching
            && nodes
                .iter()
                .any(|entry| entry.key == node && entry.state == State::Searching)
        {
            out.reset_start_time();
            self.reset_values.borrow_mut().push(out.elapsed());
        }
        if self.cancel_first || out.elapsed() > 10 {
            out.cancel(CommandType::DevInit);
            out.effect(RoleEffect::SetState(node, State::Reset));
        }
    }

    fn pre_process_cancel(&self, _cmd: &Command, node: NodeKey, out: &mut Vec<RoleEffect<State>>) {
        out.push(RoleEffect::SetState(node, State::Cancelled));
    }
}

fn mac(id: u8) -> MacAddr {
    MacAddr::new([2, 0, 0, 0, 0, id])
}

fn radio(id: u8) -> NodeKey {
    NodeKey {
        mac: mac(id),
        kind: NodeKind::Radio,
    }
}

fn command(radios: &[u8]) -> Command {
    Command::DevInit(DevInit {
        al_mac: mac(0),
        radios: radios
            .iter()
            .map(|id| RadioSpec {
                ruid: mac(*id),
                band: FreqBand::BAND_5_GHZ,
            })
            .collect(),
    })
}

fn apply(nodes: &mut Vec<Node>, effects: Vec<RoleEffect<State>>) {
    for effect in effects {
        match effect {
            RoleEffect::CreateNode { key, state, .. } => nodes.push(Node { key, state }),
            RoleEffect::SetState(key, state) => {
                nodes.iter_mut().find(|node| node.key == key).unwrap().state = state;
            }
        }
    }
}

fn setup(
    role: &mut TestRole,
    radios: &[u8],
    now: Instant,
) -> (Orchestrator<TestRole>, Vec<Node>, ChainId) {
    let mut orch = Orchestrator::new();
    let mut nodes = Vec::new();
    let (id, effects) = orch.submit(role, command(radios), now);
    apply(&mut nodes, effects);
    assert!(orch.admit(role, &nodes, id));
    (orch, nodes, id)
}

#[test]
fn submit_creates_nodes_before_admission_and_visits_candidates_in_reverse_insertion_order() {
    let now = Instant::now();
    let mut role = TestRole::default();
    let mut orch = Orchestrator::new();
    let (id, effects) = orch.submit(&mut role, command(&[1, 2]), now);
    assert_eq!(role.preprocessed, [OrchOp::AlInsert, OrchOp::EmInsert]);
    assert_eq!(effects.len(), 3);
    assert!(orch.command(id).is_some());
    assert!(!orch.is_type_in_progress(CommandType::DevInit));
    let mut nodes = Vec::new();
    apply(&mut nodes, effects);
    assert_eq!(
        nodes.iter().map(|node| node.key).collect::<Vec<_>>(),
        [
            NodeKey {
                mac: mac(0),
                kind: NodeKind::Al
            },
            radio(1),
            radio(2)
        ]
    );
    assert!(orch.admit(&role, &nodes, id));
    assert!(orch.is_type_in_progress(CommandType::DevInit));
    let effects = orch.tick(&mut role, &nodes, now);
    assert_eq!(
        effects,
        [
            RoleEffect::SetState(radio(2), State::Searching),
            RoleEffect::SetState(radio(1), State::Searching)
        ]
    );
    apply(&mut nodes, effects);
    for node in [radio(1), radio(2)] {
        assert_eq!(orch.orch_state(node), OrchState::Progress);
        assert_eq!(
            orch.active(node),
            Some(ActiveOp {
                chain: id,
                stage: 1,
                kind: CommandType::DevInit
            })
        );
    }
}

#[test]
fn newest_eligible_stage_is_activated_and_newest_active_stage_is_visited_first() {
    // em_orch.cpp:521-540 and :549-559 walk stages and candidates in reverse.
    let now = Instant::now();
    let mut role = TestRole {
        selection: CandidateSelection::Command,
        ..TestRole::default()
    };
    let mut orch = Orchestrator::new();
    let mut nodes = Vec::new();
    let (older, effects) = orch.submit(&mut role, command(&[1, 2]), now);
    apply(&mut nodes, effects);
    let (newer, effects) = orch.submit(&mut role, command(&[3, 4]), now);
    apply(&mut nodes, effects);
    assert!(orch.admit(&role, &nodes, older));
    assert!(orch.admit(&role, &nodes, newer));

    let effects = orch.tick(&mut role, &nodes, now);
    assert_eq!(
        effects,
        [
            RoleEffect::SetState(radio(4), State::Searching),
            RoleEffect::SetState(radio(3), State::Searching),
        ]
    );
    apply(&mut nodes, effects);
    assert_eq!(orch.active(radio(4)).unwrap().chain, newer);
    assert_eq!(orch.orch_state(radio(1)), OrchState::Idle);

    let effects = orch.tick(&mut role, &nodes, now);
    assert_eq!(
        effects,
        [
            RoleEffect::SetState(radio(2), State::Searching),
            RoleEffect::SetState(radio(1), State::Searching),
        ]
    );
    apply(&mut nodes, effects);
    assert_eq!(orch.active(radio(1)).unwrap().chain, older);
    role.calls.borrow_mut().clear();
    orch.tick(&mut role, &nodes, now + Duration::from_secs(1));
    assert_eq!(
        role.calls
            .borrow()
            .iter()
            .map(|(node, _)| *node)
            .collect::<Vec<_>>(),
        [radio(2), radio(1), radio(4), radio(3)]
    );
}

#[test]
fn admit_without_candidates_destroys_the_chain() {
    let now = Instant::now();
    let mut role = TestRole::default();
    let mut orch = Orchestrator::new();
    let (id, effects) = orch.submit(&mut role, command(&[]), now);
    let mut nodes = Vec::new();
    apply(&mut nodes, effects);
    assert!(!orch.admit(&role, &nodes, id));
    assert!(orch.command(id).is_none());
    assert!(!orch.is_type_in_progress(CommandType::DevInit));
    assert_eq!(orch.orch_state(radio(1)), OrchState::Idle);
}

#[test]
fn activation_waits_for_readiness_before_on_start() {
    let now = Instant::now();
    let mut role = TestRole {
        ready: false,
        ..TestRole::default()
    };
    let (mut orch, mut nodes, id) = setup(&mut role, &[1], now);
    assert_eq!(orch.on_event(&mut role, &nodes, now), []);
    assert_eq!(orch.orch_state(radio(1)), OrchState::Pending);
    assert_eq!(orch.active(radio(1)), None);
    role.ready = true;
    let effects = orch.on_event(&mut role, &nodes, now);
    assert_eq!(effects, [RoleEffect::SetState(radio(1), State::Searching)]);
    apply(&mut nodes, effects);
    assert_eq!(orch.orch_state(radio(1)), OrchState::Progress);
    assert_eq!(orch.active(radio(1)).unwrap().chain, id);
    assert!(role.calls.borrow().is_empty());
}

#[test]
fn pending_candidates_run_transient_on_ticks_and_can_be_cancelled() {
    let now = Instant::now();
    let mut role = TestRole {
        ready: false,
        ..TestRole::default()
    };
    let (mut orch, nodes, id) = setup(&mut role, &[1, 2], now);
    assert_eq!(orch.tick(&mut role, &nodes, now), []);
    assert_eq!(&*role.calls.borrow(), &[(radio(2), 0), (radio(1), 0)]);
    assert_eq!(orch.orch_state(radio(1)), OrchState::Pending);
    assert_eq!(orch.active(radio(1)), None);
    role.calls.borrow_mut().clear();
    role.cancel_first = true;
    let effects = orch.tick(&mut role, &nodes, now + Duration::from_secs(1));
    assert_eq!(&*role.calls.borrow(), &[(radio(2), 1)]);
    assert_eq!(effects.len(), 3);
    assert_eq!(orch.orch_state(radio(1)), OrchState::Fini);
    assert_eq!(orch.orch_state(radio(2)), OrchState::Fini);
    orch.tick(&mut role, &nodes, now + Duration::from_secs(2));
    assert!(orch.command(id).is_none());
    assert_eq!(orch.orch_state(radio(1)), OrchState::Idle);
}

#[test]
fn fini_is_observed_then_destroyed_and_candidates_return_to_idle() {
    let now = Instant::now();
    let mut role = TestRole::default();
    let (mut orch, mut nodes, id) = setup(&mut role, &[1, 2], now);
    let effects = orch.tick(&mut role, &nodes, now);
    apply(&mut nodes, effects);
    for node in &mut nodes {
        if node.key.kind == NodeKind::Radio {
            node.state = State::Finished;
        }
    }
    assert_eq!(orch.tick(&mut role, &nodes, now), []);
    assert_eq!(orch.orch_state(radio(1)), OrchState::Fini);
    assert!(orch.command(id).is_some());
    assert!(orch.is_type_in_progress(CommandType::DevInit));
    assert_eq!(orch.tick(&mut role, &nodes, now), []);
    assert_eq!(orch.orch_state(radio(1)), OrchState::Idle);
    assert_eq!(orch.orch_state(radio(2)), OrchState::Idle);
    assert_eq!(orch.active(radio(1)), None);
    assert!(orch.command(id).is_none());
    assert!(!orch.is_type_in_progress(CommandType::DevInit));
}

#[test]
fn transient_reset_extends_the_start_time_and_clears_type_elapsed_immediately() {
    let now = Instant::now();
    let mut role = TestRole {
        reset_searching: true,
        ..TestRole::default()
    };
    let (mut orch, mut nodes, id) = setup(&mut role, &[1, 2], now);
    let effects = orch.tick(&mut role, &nodes, now);
    apply(&mut nodes, effects);
    for secs in [12, 24, 36] {
        let effects = orch.tick(&mut role, &nodes, now + Duration::from_secs(secs));
        assert_eq!(effects, []);
    }
    assert_eq!(&*role.reset_values.borrow(), &[0, 0, 0, 0, 0, 0]);
    assert_eq!(
        &*role.calls.borrow(),
        &[
            (radio(2), 12),
            (radio(1), 0),
            (radio(2), 12),
            (radio(1), 0),
            (radio(2), 12),
            (radio(1), 0),
        ]
    );
    assert_eq!(orch.orch_state(radio(1)), OrchState::Progress);
    assert!(orch.command(id).is_some());
}

#[test]
fn cancel_from_first_transient_marks_every_candidate_before_the_next_is_visited() {
    let now = Instant::now();
    let mut role = TestRole {
        cancel_first: true,
        ..TestRole::default()
    };
    let (mut orch, mut nodes, id) = setup(&mut role, &[1, 2, 3], now);
    let effects = orch.tick(&mut role, &nodes, now);
    apply(&mut nodes, effects);
    let effects = orch.tick(&mut role, &nodes, now + Duration::from_secs(1));
    assert_eq!(&*role.calls.borrow(), &[(radio(3), 1)]);
    assert_eq!(
        effects,
        [
            RoleEffect::SetState(radio(3), State::Cancelled),
            RoleEffect::SetState(radio(2), State::Cancelled),
            RoleEffect::SetState(radio(1), State::Cancelled),
            RoleEffect::SetState(radio(3), State::Reset),
        ]
    );
    for node in [radio(1), radio(2), radio(3)] {
        assert_eq!(orch.orch_state(node), OrchState::Fini);
    }
    apply(&mut nodes, effects);
    assert_eq!(
        nodes
            .iter()
            .find(|node| node.key == radio(3))
            .unwrap()
            .state,
        State::Reset
    );
    assert!(orch.command(id).is_some());
    orch.tick(&mut role, &nodes, now + Duration::from_secs(2));
    assert!(orch.command(id).is_none());
}

#[test]
fn cancellation_visits_active_stages_newest_first_and_cleans_up_older_stage() {
    // em_orch.cpp:279-301 and :549-572 walk the queues newest first.
    let now = Instant::now();
    let mut role = TestRole {
        selection: CandidateSelection::Command,
        ..TestRole::default()
    };
    let mut orch = Orchestrator::new();
    let mut nodes = Vec::new();
    let (older, effects) = orch.submit(&mut role, command(&[1]), now);
    apply(&mut nodes, effects);
    assert!(orch.admit(&role, &nodes, older));
    let effects = orch.tick(&mut role, &nodes, now);
    apply(&mut nodes, effects);
    let (newer, effects) = orch.submit(&mut role, command(&[2]), now);
    apply(&mut nodes, effects);
    assert!(orch.admit(&role, &nodes, newer));
    let effects = orch.tick(&mut role, &nodes, now);
    apply(&mut nodes, effects);
    role.calls.borrow_mut().clear();
    role.cancel_first = true;

    let effects = orch.tick(&mut role, &nodes, now + Duration::from_secs(1));
    assert_eq!(&*role.calls.borrow(), &[(radio(2), 1)]);
    assert_eq!(
        effects,
        [
            RoleEffect::SetState(radio(2), State::Cancelled),
            RoleEffect::SetState(radio(1), State::Cancelled),
            RoleEffect::SetState(radio(2), State::Reset),
        ]
    );
    assert!(orch.command(older).is_none());
    assert!(orch.command(newer).is_some());
    assert_eq!(orch.orch_state(radio(1)), OrchState::Idle);
    assert_eq!(orch.orch_state(radio(2)), OrchState::Fini);
    orch.tick(&mut role, &nodes, now + Duration::from_secs(2));
    assert!(orch.command(newer).is_none());
    assert!(!orch.is_type_in_progress(CommandType::DevInit));
}

#[test]
fn elapsed_whole_seconds_threshold_cancels_after_ten_seconds() {
    // intentional-difference:elapsed-whole-seconds
    let now = Instant::now();
    let mut role = TestRole::default();
    let (mut orch, mut nodes, id) = setup(&mut role, &[1], now);
    let effects = orch.tick(&mut role, &nodes, now);
    apply(&mut nodes, effects);
    assert_eq!(
        orch.tick(&mut role, &nodes, now + Duration::from_millis(10_900)),
        []
    );
    assert_eq!(orch.orch_state(radio(1)), OrchState::Progress);
    assert_eq!(role.calls.borrow()[0].1, 10);
    let effects = orch.tick(&mut role, &nodes, now + Duration::from_secs(12));
    assert_eq!(role.calls.borrow()[1].1, 12);
    assert_eq!(orch.orch_state(radio(1)), OrchState::Fini);
    assert!(orch.command(id).is_some());
    apply(&mut nodes, effects);
    orch.tick(&mut role, &nodes, now + Duration::from_secs(13));
    assert!(orch.command(id).is_none());
}

#[test]
fn timeout_is_measured_from_the_last_searching_tick() {
    // intentional-difference:elapsed-whole-seconds
    let now = Instant::now();
    let mut role = TestRole {
        reset_searching: true,
        ..TestRole::default()
    };
    let (mut orch, mut nodes, _) = setup(&mut role, &[1], now);
    let effects = orch.tick(&mut role, &nodes, now);
    apply(&mut nodes, effects);
    orch.tick(&mut role, &nodes, now + Duration::from_secs(9));
    orch.tick(&mut role, &nodes, now + Duration::from_secs(15));
    nodes
        .iter_mut()
        .find(|node| node.key == radio(1))
        .unwrap()
        .state = State::Created;
    assert_eq!(
        orch.tick(&mut role, &nodes, now + Duration::from_millis(25_900)),
        []
    );
    assert_eq!(orch.orch_state(radio(1)), OrchState::Progress);
    assert_eq!(role.calls.borrow().last().unwrap().1, 10);
    orch.tick(&mut role, &nodes, now + Duration::from_secs(27));
    assert_eq!(role.calls.borrow().last().unwrap().1, 12);
    assert_eq!(orch.orch_state(radio(1)), OrchState::Fini);
}

#[test]
fn events_never_update_elapsed_or_run_transient() {
    let now = Instant::now();
    let mut role = TestRole::default();
    let (mut orch, mut nodes, _) = setup(&mut role, &[1], now);
    let effects = orch.on_event(&mut role, &nodes, now);
    apply(&mut nodes, effects);
    assert_eq!(
        orch.on_event(&mut role, &nodes, now + Duration::from_secs(12)),
        []
    );
    assert!(role.calls.borrow().is_empty());
    assert_eq!(orch.orch_state(radio(1)), OrchState::Progress);
    orch.tick(&mut role, &nodes, now + Duration::from_secs(12));
    assert_eq!(&*role.calls.borrow(), &[(radio(1), 12)]);
    assert_eq!(orch.orch_state(radio(1)), OrchState::Fini);
}

#[test]
fn progress_includes_blocked_pending_stages_and_cancel_removes_them() {
    let now = Instant::now();
    let mut role = TestRole::default();
    let (mut orch, mut nodes, first) = setup(&mut role, &[1], now);
    let effects = orch.tick(&mut role, &nodes, now);
    apply(&mut nodes, effects);
    let (second, _) = orch.submit(&mut role, command(&[1]), now);
    assert!(orch.admit(&role, &nodes, second));
    assert!(orch.admit(&role, &nodes, second));
    assert!(orch.command(second).is_some());
    assert!(orch.is_type_in_progress(CommandType::DevInit));
    role.cancel_first = true;
    orch.tick(&mut role, &nodes, now + Duration::from_secs(1));
    assert!(orch.command(second).is_none());
    assert!(orch.command(first).is_some());
    assert!(orch.is_type_in_progress(CommandType::DevInit));
    orch.tick(&mut role, &nodes, now + Duration::from_secs(2));
    assert!(!orch.is_type_in_progress(CommandType::DevInit));
}

#[test]
fn activation_skips_blocked_stages_and_pending_stages_do_not_expire() {
    let now = Instant::now();
    let mut role = TestRole {
        selection: CandidateSelection::Command,
        reset_searching: true,
        ..TestRole::default()
    };
    let mut orch = Orchestrator::new();
    let mut nodes = vec![
        Node {
            key: radio(1),
            state: State::Created,
        },
        Node {
            key: radio(2),
            state: State::Created,
        },
    ];
    let (first, _) = orch.submit(&mut role, command(&[1]), now);
    assert!(orch.admit(&role, &nodes, first));
    let effects = orch.tick(&mut role, &nodes, now);
    apply(&mut nodes, effects);
    let (blocked, _) = orch.submit(&mut role, command(&[1]), now);
    let (independent, _) = orch.submit(&mut role, command(&[2]), now);
    assert!(orch.admit(&role, &nodes, independent));
    assert!(orch.admit(&role, &nodes, blocked));
    let effects = orch.on_event(&mut role, &nodes, now + Duration::from_secs(1));
    apply(&mut nodes, effects);
    assert_eq!(orch.active(radio(1)).unwrap().chain, first);
    assert_eq!(orch.active(radio(2)).unwrap().chain, independent);
    let effects = orch.tick(&mut role, &nodes, now + Duration::from_secs(100));
    apply(&mut nodes, effects);
    assert!(orch.command(blocked).is_some());
    assert_eq!(orch.active(radio(1)).unwrap().chain, first);
}

#[test]
fn elapsed_is_the_maximum_across_active_stages_of_the_same_type() {
    let now = Instant::now();
    let mut role = TestRole {
        selection: CandidateSelection::Command,
        ..TestRole::default()
    };
    let mut orch = Orchestrator::new();
    let mut nodes = vec![
        Node {
            key: radio(1),
            state: State::Created,
        },
        Node {
            key: radio(2),
            state: State::Created,
        },
    ];
    let (first, _) = orch.submit(&mut role, command(&[1]), now);
    let (second, _) = orch.submit(&mut role, command(&[2]), now);
    assert!(orch.admit(&role, &nodes, first));
    assert!(orch.admit(&role, &nodes, second));
    let effects = orch.tick(&mut role, &nodes, now);
    apply(&mut nodes, effects);
    let effects = orch.on_event(&mut role, &nodes, now + Duration::from_secs(5));
    apply(&mut nodes, effects);
    orch.tick(&mut role, &nodes, now + Duration::from_secs(6));
    assert_eq!(&*role.calls.borrow(), &[(radio(1), 1), (radio(2), 6)]);
    orch.tick(&mut role, &nodes, now + Duration::from_secs(7));
    assert_eq!(
        &*role.calls.borrow(),
        &[(radio(1), 1), (radio(2), 6), (radio(1), 6), (radio(2), 7)]
    );
}
