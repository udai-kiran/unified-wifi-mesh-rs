// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::time::{Duration, Instant};

use em_cmd::{
    ActiveOp, CfgRenew, ChainId, Command, CommandType, CommandView, DevInit, EmConfig, NodeKey,
    NodeKind, OneWifiCb, OrchOp, OrchState, RadioSpec,
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

#[expect(clippy::struct_excessive_bools, reason = "independent test scenarios")]
struct TestRole {
    preprocessed: Vec<OrchOp>,
    ready: bool,
    reset_searching: bool,
    cancel_first: bool,
    depend_on_previous_start: bool,
    readiness_effects: bool,
    queue_requests: bool,
    scoped_cancel: bool,
    selection: CandidateSelection,
    calls: RefCell<Vec<(NodeKey, u64)>>,
    transient_states: RefCell<Vec<(NodeKey, State)>>,
    reset_values: RefCell<Vec<u64>>,
}

impl Default for TestRole {
    fn default() -> Self {
        Self {
            preprocessed: Vec::new(),
            ready: true,
            reset_searching: false,
            cancel_first: false,
            depend_on_previous_start: false,
            readiness_effects: false,
            queue_requests: false,
            scoped_cancel: false,
            selection: CandidateSelection::All,
            calls: RefCell::new(Vec::new()),
            transient_states: RefCell::new(Vec::new()),
            reset_values: RefCell::new(Vec::new()),
        }
    }
}

impl OrchRole for TestRole {
    type Nodes = Vec<Node>;
    type State = State;
    type Out = Vec<RoleEffect<State>>;

    fn apply(&mut self, nodes: &mut Vec<Node>, effect: RoleEffect<State>, out: &mut Self::Out) {
        out.push(effect);
        apply(nodes, vec![effect]);
    }

    fn pre_process(&mut self, cmd: &Command, op: OrchOp, out: &mut Vec<RoleEffect<State>>) -> bool {
        self.preprocessed.push(op);
        let Command::DevInit(init) = cmd else {
            return cmd
                .plan()
                .iter()
                .any(|stage| stage.op == op && stage.submit);
        };
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
            _ => false,
        }
    }

    fn build_candidates(&self, cmd: &Command, _op: OrchOp, nodes: &Vec<Node>) -> Vec<NodeKey> {
        nodes
            .iter()
            .filter(|node| {
                node.key.kind == NodeKind::Radio
                    && (matches!(self.selection, CandidateSelection::All)
                        || match cmd {
                            Command::DevInit(init) => {
                                init.radios.iter().any(|radio| radio.ruid == node.key.mac)
                            }
                            Command::OneWifiCb(callback) => callback.radios.contains(&node.key),
                            Command::CfgRenew(renew) => renew.radio == node.key,
                            Command::EmConfig(config) => config.radio == node.key,
                        })
            })
            .map(|node| node.key)
            .collect()
    }

    fn ready_for_exec(
        &self,
        _cmd: &Command,
        node: NodeKey,
        nodes: &Vec<Node>,
        out: &mut Vec<RoleEffect<State>>,
    ) -> bool {
        if self.readiness_effects {
            out.push(RoleEffect::SetState(node, State::Reset));
        }
        if self.depend_on_previous_start && node == radio(1) {
            return nodes
                .iter()
                .any(|entry| entry.key == radio(2) && entry.state == State::Searching);
        }
        self.ready
    }

    fn ready_for_fini(
        &self,
        _cmd: &Command,
        node: NodeKey,
        nodes: &Vec<Node>,
        out: &mut Vec<RoleEffect<State>>,
    ) -> bool {
        if self.readiness_effects {
            out.push(RoleEffect::SetState(node, State::Finished));
            return false;
        }
        nodes
            .iter()
            .any(|entry| entry.key == node && entry.state == State::Finished)
    }

    fn on_start(&self, _cmd: &Command, node: NodeKey, out: &mut Vec<RoleEffect<State>>) {
        out.push(RoleEffect::SetState(node, State::Searching));
        if self.queue_requests {
            out.push(RoleEffect::Renew(node));
            out.push(RoleEffect::RestartProto(node));
        }
    }

    fn transient(
        &self,
        cmd: &Command,
        node: NodeKey,
        nodes: &Vec<Node>,
        elapsed: u64,
        out: &mut TransientOut<State>,
    ) {
        self.calls.borrow_mut().push((node, elapsed));
        if let Some(entry) = nodes.iter().find(|entry| entry.key == node) {
            self.transient_states.borrow_mut().push((node, entry.state));
        }
        if self.scoped_cancel && node == radio(2) {
            out.cancel_scoped(cmd.kind(), vec![node]);
            self.reset_values.borrow_mut().push(out.elapsed());
            out.effect(RoleEffect::Renew(node));
        }
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
            RoleEffect::Renew(_) | RoleEffect::RestartProto(_) => {}
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
    let (id, effects) = orch.submit(role, &mut nodes, command(radios), now);
    apply(&mut nodes, effects);
    assert!(orch.admit(role, &mut nodes, id));
    (orch, nodes, id)
}

fn tick(
    orch: &mut Orchestrator<TestRole>,
    role: &mut TestRole,
    nodes: &mut Vec<Node>,
    now: Instant,
) -> Vec<RoleEffect<State>> {
    let mut out = Vec::new();
    orch.tick(role, nodes, now, &mut out);
    out
}

fn on_event(
    orch: &mut Orchestrator<TestRole>,
    role: &mut TestRole,
    nodes: &mut Vec<Node>,
    now: Instant,
) -> Vec<RoleEffect<State>> {
    let mut out = Vec::new();
    orch.on_event(role, nodes, now, &mut out);
    out
}

#[test]
fn submit_creates_nodes_before_admission_and_visits_candidates_in_reverse_insertion_order() {
    let now = Instant::now();
    let mut role = TestRole::default();
    let mut orch = Orchestrator::new();
    let mut nodes = Vec::new();
    let (id, effects) = orch.submit(&mut role, &mut nodes, command(&[1, 2]), now);
    assert_eq!(role.preprocessed, [OrchOp::AlInsert, OrchOp::EmInsert]);
    assert_eq!(effects.len(), 3);
    assert!(orch.command(id).is_some());
    assert!(!orch.is_type_in_progress(CommandType::DevInit));
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
    assert!(orch.admit(&mut role, &mut nodes, id));
    assert!(orch.is_type_in_progress(CommandType::DevInit));
    let effects = tick(&mut orch, &mut role, &mut nodes, now);
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
    let (older, effects) = orch.submit(&mut role, &mut nodes, command(&[1, 2]), now);
    apply(&mut nodes, effects);
    let (newer, effects) = orch.submit(&mut role, &mut nodes, command(&[3, 4]), now);
    apply(&mut nodes, effects);
    assert!(orch.admit(&mut role, &mut nodes, older));
    assert!(orch.admit(&mut role, &mut nodes, newer));

    let effects = tick(&mut orch, &mut role, &mut nodes, now);
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

    let effects = tick(&mut orch, &mut role, &mut nodes, now);
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
    tick(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(1),
    );
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
    let mut nodes = Vec::new();
    let (id, effects) = orch.submit(&mut role, &mut nodes, command(&[]), now);
    apply(&mut nodes, effects);
    assert!(!orch.admit(&mut role, &mut nodes, id));
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
    assert_eq!(on_event(&mut orch, &mut role, &mut nodes, now), []);
    assert_eq!(orch.orch_state(radio(1)), OrchState::Pending);
    assert_eq!(orch.active(radio(1)), None);
    role.ready = true;
    let effects = on_event(&mut orch, &mut role, &mut nodes, now);
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
    let (mut orch, mut nodes, id) = setup(&mut role, &[1, 2], now);
    assert_eq!(tick(&mut orch, &mut role, &mut nodes, now), []);
    assert_eq!(&*role.calls.borrow(), &[(radio(2), 0), (radio(1), 0)]);
    assert_eq!(orch.orch_state(radio(1)), OrchState::Pending);
    assert_eq!(orch.active(radio(1)), None);
    role.calls.borrow_mut().clear();
    role.cancel_first = true;
    let effects = tick(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(1),
    );
    assert_eq!(&*role.calls.borrow(), &[(radio(2), 1)]);
    assert_eq!(effects.len(), 3);
    assert_eq!(orch.orch_state(radio(1)), OrchState::Fini);
    assert_eq!(orch.orch_state(radio(2)), OrchState::Fini);
    tick(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(2),
    );
    assert!(orch.command(id).is_none());
    assert_eq!(orch.orch_state(radio(1)), OrchState::Idle);
}

#[test]
fn fini_is_observed_then_destroyed_and_candidates_return_to_idle() {
    let now = Instant::now();
    let mut role = TestRole::default();
    let (mut orch, mut nodes, id) = setup(&mut role, &[1, 2], now);
    let effects = tick(&mut orch, &mut role, &mut nodes, now);
    apply(&mut nodes, effects);
    for node in &mut nodes {
        if node.key.kind == NodeKind::Radio {
            node.state = State::Finished;
        }
    }
    assert_eq!(tick(&mut orch, &mut role, &mut nodes, now), []);
    assert_eq!(orch.orch_state(radio(1)), OrchState::Fini);
    assert!(orch.command(id).is_some());
    assert!(orch.is_type_in_progress(CommandType::DevInit));
    assert_eq!(tick(&mut orch, &mut role, &mut nodes, now), []);
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
    let effects = tick(&mut orch, &mut role, &mut nodes, now);
    apply(&mut nodes, effects);
    for secs in [12, 24, 36] {
        let effects = tick(
            &mut orch,
            &mut role,
            &mut nodes,
            now + Duration::from_secs(secs),
        );
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
    let effects = tick(&mut orch, &mut role, &mut nodes, now);
    apply(&mut nodes, effects);
    let effects = tick(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(1),
    );
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
    tick(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(2),
    );
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
    let (older, effects) = orch.submit(&mut role, &mut nodes, command(&[1]), now);
    apply(&mut nodes, effects);
    assert!(orch.admit(&mut role, &mut nodes, older));
    let effects = tick(&mut orch, &mut role, &mut nodes, now);
    apply(&mut nodes, effects);
    let (newer, effects) = orch.submit(&mut role, &mut nodes, command(&[2]), now);
    apply(&mut nodes, effects);
    assert!(orch.admit(&mut role, &mut nodes, newer));
    let effects = tick(&mut orch, &mut role, &mut nodes, now);
    apply(&mut nodes, effects);
    role.calls.borrow_mut().clear();
    role.cancel_first = true;

    let effects = tick(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(1),
    );
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
    tick(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(2),
    );
    assert!(orch.command(newer).is_none());
    assert!(!orch.is_type_in_progress(CommandType::DevInit));
}

#[test]
fn elapsed_whole_seconds_threshold_cancels_after_ten_seconds() {
    // intentional-difference:elapsed-whole-seconds
    let now = Instant::now();
    let mut role = TestRole::default();
    let (mut orch, mut nodes, id) = setup(&mut role, &[1], now);
    let effects = tick(&mut orch, &mut role, &mut nodes, now);
    apply(&mut nodes, effects);
    assert_eq!(
        tick(
            &mut orch,
            &mut role,
            &mut nodes,
            now + Duration::from_millis(10_900)
        ),
        []
    );
    assert_eq!(orch.orch_state(radio(1)), OrchState::Progress);
    assert_eq!(role.calls.borrow()[0].1, 10);
    let effects = tick(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(12),
    );
    assert_eq!(role.calls.borrow()[1].1, 12);
    assert_eq!(orch.orch_state(radio(1)), OrchState::Fini);
    assert!(orch.command(id).is_some());
    apply(&mut nodes, effects);
    tick(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(13),
    );
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
    let effects = tick(&mut orch, &mut role, &mut nodes, now);
    apply(&mut nodes, effects);
    tick(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(9),
    );
    tick(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(15),
    );
    nodes
        .iter_mut()
        .find(|node| node.key == radio(1))
        .unwrap()
        .state = State::Created;
    assert_eq!(
        tick(
            &mut orch,
            &mut role,
            &mut nodes,
            now + Duration::from_millis(25_900)
        ),
        []
    );
    assert_eq!(orch.orch_state(radio(1)), OrchState::Progress);
    assert_eq!(role.calls.borrow().last().unwrap().1, 10);
    tick(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(27),
    );
    assert_eq!(role.calls.borrow().last().unwrap().1, 12);
    assert_eq!(orch.orch_state(radio(1)), OrchState::Fini);
}

#[test]
fn events_never_update_elapsed_or_run_transient() {
    let now = Instant::now();
    let mut role = TestRole::default();
    let (mut orch, mut nodes, _) = setup(&mut role, &[1], now);
    let effects = on_event(&mut orch, &mut role, &mut nodes, now);
    apply(&mut nodes, effects);
    assert_eq!(
        on_event(
            &mut orch,
            &mut role,
            &mut nodes,
            now + Duration::from_secs(12)
        ),
        []
    );
    assert!(role.calls.borrow().is_empty());
    assert_eq!(orch.orch_state(radio(1)), OrchState::Progress);
    tick(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(12),
    );
    assert_eq!(&*role.calls.borrow(), &[(radio(1), 12)]);
    assert_eq!(orch.orch_state(radio(1)), OrchState::Fini);
}

#[test]
fn progress_includes_blocked_pending_stages_and_cancel_removes_them() {
    let now = Instant::now();
    let mut role = TestRole::default();
    let (mut orch, mut nodes, first) = setup(&mut role, &[1], now);
    let effects = tick(&mut orch, &mut role, &mut nodes, now);
    apply(&mut nodes, effects);
    let (second, _) = orch.submit(&mut role, &mut nodes, command(&[1]), now);
    assert!(orch.admit(&mut role, &mut nodes, second));
    assert!(orch.admit(&mut role, &mut nodes, second));
    assert!(orch.command(second).is_some());
    assert!(orch.is_type_in_progress(CommandType::DevInit));
    role.cancel_first = true;
    tick(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(1),
    );
    assert!(orch.command(second).is_none());
    assert!(orch.command(first).is_some());
    assert!(orch.is_type_in_progress(CommandType::DevInit));
    tick(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(2),
    );
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
    let (first, _) = orch.submit(&mut role, &mut nodes, command(&[1]), now);
    assert!(orch.admit(&mut role, &mut nodes, first));
    let effects = tick(&mut orch, &mut role, &mut nodes, now);
    apply(&mut nodes, effects);
    let (blocked, _) = orch.submit(&mut role, &mut nodes, command(&[1]), now);
    let (independent, _) = orch.submit(&mut role, &mut nodes, command(&[2]), now);
    assert!(orch.admit(&mut role, &mut nodes, independent));
    assert!(orch.admit(&mut role, &mut nodes, blocked));
    let effects = on_event(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(1),
    );
    apply(&mut nodes, effects);
    assert_eq!(orch.active(radio(1)).unwrap().chain, first);
    assert_eq!(orch.active(radio(2)).unwrap().chain, independent);
    let effects = tick(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(100),
    );
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
    let (first, _) = orch.submit(&mut role, &mut nodes, command(&[1]), now);
    let (second, _) = orch.submit(&mut role, &mut nodes, command(&[2]), now);
    assert!(orch.admit(&mut role, &mut nodes, first));
    assert!(orch.admit(&mut role, &mut nodes, second));
    let effects = tick(&mut orch, &mut role, &mut nodes, now);
    apply(&mut nodes, effects);
    let effects = on_event(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(5),
    );
    apply(&mut nodes, effects);
    tick(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(6),
    );
    assert_eq!(&*role.calls.borrow(), &[(radio(1), 1), (radio(2), 6)]);
    tick(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(7),
    );
    assert_eq!(
        &*role.calls.borrow(),
        &[(radio(1), 1), (radio(2), 6), (radio(1), 6), (radio(2), 7)]
    );
}

fn test_nodes(ids: &[u8]) -> Vec<Node> {
    ids.iter()
        .map(|id| Node {
            key: radio(*id),
            state: State::Created,
        })
        .collect()
}

fn config(id: u8) -> Command {
    Command::EmConfig(EmConfig {
        radio: radio(id),
        agent_al: mac(0),
    })
}

fn submit_existing(
    orch: &mut Orchestrator<TestRole>,
    role: &mut TestRole,
    nodes: &mut Vec<Node>,
    command: Command,
    now: Instant,
) -> ChainId {
    let (id, effects) = orch.submit(role, nodes, command, now);
    assert_eq!(effects, []);
    assert!(orch.admit(role, nodes, id));
    id
}

#[test]
fn later_candidate_sees_the_previous_candidates_start_effect() {
    let now = Instant::now();
    let mut role = TestRole {
        depend_on_previous_start: true,
        ..TestRole::default()
    };
    let (mut orch, mut nodes, _) = setup(&mut role, &[1, 2], now);
    on_event(&mut orch, &mut role, &mut nodes, now);
    assert_eq!(orch.orch_state(radio(2)), OrchState::Progress);
    assert_eq!(orch.orch_state(radio(1)), OrchState::Progress);
}

#[test]
fn readiness_effects_apply_before_start_and_even_when_not_ready_to_finish() {
    let now = Instant::now();
    let mut role = TestRole {
        readiness_effects: true,
        ..TestRole::default()
    };
    let (mut orch, mut nodes, id) = setup(&mut role, &[1], now);
    assert_eq!(
        on_event(&mut orch, &mut role, &mut nodes, now),
        [
            RoleEffect::SetState(radio(1), State::Reset),
            RoleEffect::SetState(radio(1), State::Searching),
        ]
    );
    assert_eq!(
        on_event(&mut orch, &mut role, &mut nodes, now),
        [RoleEffect::SetState(radio(1), State::Finished),]
    );
    assert_eq!(
        nodes
            .iter()
            .find(|node| node.key == radio(1))
            .unwrap()
            .state,
        State::Finished
    );
    assert_eq!(orch.orch_state(radio(1)), OrchState::Progress);
    role.readiness_effects = false;
    on_event(&mut orch, &mut role, &mut nodes, now);
    assert_eq!(orch.orch_state(radio(1)), OrchState::Fini);
    on_event(&mut orch, &mut role, &mut nodes, now);
    assert!(orch.command(id).is_none());
}

#[test]
fn transient_observes_the_preceding_fini_readiness_effect() {
    let now = Instant::now();
    let mut role = TestRole {
        readiness_effects: true,
        ..TestRole::default()
    };
    let (mut orch, mut nodes, _) = setup(&mut role, &[1], now);
    on_event(&mut orch, &mut role, &mut nodes, now);
    tick(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(1),
    );
    assert_eq!(
        &*role.transient_states.borrow(),
        &[(radio(1), State::Finished)]
    );
}

#[test]
fn request_buffer_preserves_hook_and_candidate_order() {
    let now = Instant::now();
    let mut role = TestRole {
        queue_requests: true,
        ..TestRole::default()
    };
    let (mut orch, mut nodes, _) = setup(&mut role, &[1, 2], now);
    let effects = on_event(&mut orch, &mut role, &mut nodes, now);
    let requests: Vec<_> = effects
        .into_iter()
        .filter(|effect| matches!(effect, RoleEffect::Renew(_) | RoleEffect::RestartProto(_)))
        .collect();
    assert_eq!(
        requests,
        [
            RoleEffect::Renew(radio(2)),
            RoleEffect::RestartProto(radio(2)),
            RoleEffect::Renew(radio(1)),
            RoleEffect::RestartProto(radio(1)),
        ]
    );
}

#[test]
fn scoped_cancel_keeps_other_pending_candidates_and_finishes_only_selected_active_nodes() {
    let now = Instant::now();
    let mut role = TestRole {
        selection: CandidateSelection::Command,
        ..TestRole::default()
    };
    let mut orch = Orchestrator::new();
    let mut nodes = test_nodes(&[1, 2, 3]);
    let active = submit_existing(
        &mut orch,
        &mut role,
        &mut nodes,
        Command::CfgRenew(CfgRenew { radio: radio(1) }),
        now,
    );
    on_event(&mut orch, &mut role, &mut nodes, now);
    role.selection = CandidateSelection::All;
    let pending = submit_existing(&mut orch, &mut role, &mut nodes, config(1), now);
    role.selection = CandidateSelection::Command;
    let empty_after_cancel = submit_existing(&mut orch, &mut role, &mut nodes, config(1), now);
    let mut out = Vec::new();
    orch.cancel_command(
        &mut role,
        &mut nodes,
        CommandType::EmConfig,
        &[radio(1), radio(2)],
        &mut out,
    );
    assert_eq!(out, []);
    assert!(orch.command(empty_after_cancel).is_none());
    assert!(orch.command(pending).is_some());
    assert_eq!(orch.orch_state(radio(1)), OrchState::Progress);
    assert!(orch.command(active).is_some());
    on_event(&mut orch, &mut role, &mut nodes, now);
    assert_eq!(orch.active(radio(3)).unwrap().chain, pending);
    assert_eq!(orch.orch_state(radio(2)), OrchState::Idle);
    orch.cancel_command(
        &mut role,
        &mut nodes,
        CommandType::EmConfig,
        &[radio(3)],
        &mut out,
    );
    assert_eq!(out, [RoleEffect::SetState(radio(3), State::Cancelled)]);
    assert_eq!(orch.orch_state(radio(3)), OrchState::Fini);
    assert_eq!(orch.orch_state(radio(1)), OrchState::Progress);
    on_event(&mut orch, &mut role, &mut nodes, now);
    assert!(orch.command(pending).is_none());
    assert!(!orch.is_type_in_progress(CommandType::EmConfig));
    assert!(orch.is_type_in_progress(CommandType::CfgRenew));
}

#[test]
fn scoped_cancel_from_transient_runs_in_order_before_the_next_candidate() {
    let now = Instant::now();
    let mut role = TestRole {
        scoped_cancel: true,
        ..TestRole::default()
    };
    let mut orch = Orchestrator::new();
    let mut nodes = test_nodes(&[1, 2]);
    submit_existing(
        &mut orch,
        &mut role,
        &mut nodes,
        Command::CfgRenew(CfgRenew { radio: radio(1) }),
        now,
    );
    tick(&mut orch, &mut role, &mut nodes, now);
    role.calls.borrow_mut().clear();
    assert_eq!(
        tick(
            &mut orch,
            &mut role,
            &mut nodes,
            now + Duration::from_secs(1)
        ),
        [
            RoleEffect::SetState(radio(2), State::Cancelled),
            RoleEffect::Renew(radio(2)),
        ]
    );
    assert_eq!(&*role.calls.borrow(), &[(radio(2), 1), (radio(1), 1)]);
    assert_eq!(&*role.reset_values.borrow(), &[0]);
    assert_eq!(orch.orch_state(radio(2)), OrchState::Fini);
    assert_eq!(orch.orch_state(radio(1)), OrchState::Progress);
}

#[test]
fn scoped_cancellation_only_resets_the_elapsed_value_for_its_own_type() {
    let mut out = TransientOut::<State>::new(CommandType::DevInit, 9);
    out.cancel_scoped(CommandType::EmConfig, vec![radio(1)]);
    assert_eq!(out.elapsed(), 9);
    out.cancel_scoped(CommandType::DevInit, vec![radio(1)]);
    assert_eq!(out.elapsed(), 0);
}

#[test]
fn scoped_cancel_resets_the_shared_elapsed_time() {
    let now = Instant::now();
    let mut role = TestRole {
        selection: CandidateSelection::Command,
        ..TestRole::default()
    };
    let mut orch = Orchestrator::new();
    let mut nodes = test_nodes(&[1, 2]);
    submit_existing(&mut orch, &mut role, &mut nodes, config(1), now);
    tick(&mut orch, &mut role, &mut nodes, now);
    submit_existing(
        &mut orch,
        &mut role,
        &mut nodes,
        config(2),
        now + Duration::from_secs(8),
    );
    tick(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(8),
    );
    tick(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(9),
    );
    let mut out = Vec::new();
    orch.cancel_command(
        &mut role,
        &mut nodes,
        CommandType::EmConfig,
        &[radio(1)],
        &mut out,
    );
    role.calls.borrow_mut().clear();
    tick(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(10),
    );
    assert_eq!(&*role.calls.borrow(), &[(radio(2), 2)]);
}

#[test]
fn remove_chain_for_node_preserves_nonempty_pending_stages_and_other_command_types() {
    let now = Instant::now();
    let mut role = TestRole {
        selection: CandidateSelection::Command,
        ..TestRole::default()
    };
    let mut orch = Orchestrator::new();
    let mut nodes = test_nodes(&[1, 2]);
    let other = submit_existing(
        &mut orch,
        &mut role,
        &mut nodes,
        Command::CfgRenew(CfgRenew { radio: radio(1) }),
        now,
    );
    on_event(&mut orch, &mut role, &mut nodes, now);
    role.selection = CandidateSelection::All;
    let pending = submit_existing(&mut orch, &mut role, &mut nodes, config(1), now);
    role.selection = CandidateSelection::Command;
    let empty = submit_existing(&mut orch, &mut role, &mut nodes, config(1), now);
    assert_eq!(orch.remove_chain_for_node(radio(1), State::Reset), None);
    assert!(orch.command(empty).is_none());
    assert!(orch.command(pending).is_some());
    assert!(orch.command(other).is_some());
    assert_eq!(orch.orch_state(radio(1)), OrchState::Progress);
    on_event(&mut orch, &mut role, &mut nodes, now);
    assert_eq!(orch.active(radio(2)).unwrap().chain, pending);
}

#[test]
fn remove_chain_for_node_marks_active_fini_returns_state_and_resets_elapsed() {
    let now = Instant::now();
    let mut role = TestRole {
        selection: CandidateSelection::Command,
        ..TestRole::default()
    };
    let mut orch = Orchestrator::new();
    let mut nodes = test_nodes(&[1, 2]);
    let first = submit_existing(&mut orch, &mut role, &mut nodes, config(1), now);
    tick(&mut orch, &mut role, &mut nodes, now);
    let second = submit_existing(
        &mut orch,
        &mut role,
        &mut nodes,
        config(2),
        now + Duration::from_secs(8),
    );
    tick(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(8),
    );
    tick(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(9),
    );
    let effect = orch.remove_chain_for_node(radio(1), State::Reset);
    assert_eq!(effect, Some(RoleEffect::SetState(radio(1), State::Reset)));
    assert_eq!(orch.orch_state(radio(1)), OrchState::Fini);
    assert_eq!(orch.orch_state(radio(2)), OrchState::Progress);
    assert!(orch.command(first).is_some());
    apply(&mut nodes, vec![effect.unwrap()]);
    assert_eq!(nodes[0].state, State::Reset);
    role.calls.borrow_mut().clear();
    tick(
        &mut orch,
        &mut role,
        &mut nodes,
        now + Duration::from_secs(10),
    );
    assert_eq!(&*role.calls.borrow(), &[(radio(2), 2)]);
    assert!(orch.command(first).is_none());
    assert!(orch.command(second).is_some());
    assert_eq!(orch.orch_state(radio(1)), OrchState::Idle);
}

#[test]
fn remove_chain_for_node_retains_an_active_chain_until_all_candidates_finish() {
    let now = Instant::now();
    let mut role = TestRole::default();
    let mut orch = Orchestrator::new();
    let mut nodes = test_nodes(&[1, 2]);
    let chain = submit_existing(&mut orch, &mut role, &mut nodes, config(1), now);
    on_event(&mut orch, &mut role, &mut nodes, now);
    assert_eq!(
        orch.remove_chain_for_node(radio(2), State::Reset),
        Some(RoleEffect::SetState(radio(2), State::Reset))
    );
    on_event(&mut orch, &mut role, &mut nodes, now);
    assert!(orch.command(chain).is_some());
    assert_eq!(orch.orch_state(radio(1)), OrchState::Progress);
    assert_eq!(orch.orch_state(radio(2)), OrchState::Fini);
    nodes[0].state = State::Finished;
    on_event(&mut orch, &mut role, &mut nodes, now);
    on_event(&mut orch, &mut role, &mut nodes, now);
    assert!(orch.command(chain).is_none());
    assert_eq!(orch.orch_state(radio(2)), OrchState::Idle);
}

#[test]
fn new_command_types_are_in_progress_until_their_chains_finish() {
    let now = Instant::now();
    let mut role = TestRole {
        selection: CandidateSelection::Command,
        ..TestRole::default()
    };
    let mut orch = Orchestrator::new();
    let mut nodes = test_nodes(&[1]);
    let commands = [
        Command::OneWifiCb(OneWifiCb {
            radios: vec![radio(1)],
        }),
        Command::CfgRenew(CfgRenew { radio: radio(1) }),
        config(1),
    ];
    for cmd in commands {
        let kind = cmd.kind();
        let (id, effects) = orch.submit(&mut role, &mut nodes, cmd, now);
        assert_eq!(effects, []);
        assert!(!orch.is_type_in_progress(kind));
        assert!(orch.admit(&mut role, &mut nodes, id));
        assert!(orch.is_type_in_progress(kind));
        on_event(&mut orch, &mut role, &mut nodes, now);
        assert_eq!(
            orch.active(radio(1)).unwrap().stage,
            usize::from(kind == CommandType::EmConfig)
        );
        assert!(orch.is_type_in_progress(kind));
        nodes[0].state = State::Finished;
        on_event(&mut orch, &mut role, &mut nodes, now);
        assert!(orch.is_type_in_progress(kind));
        on_event(&mut orch, &mut role, &mut nodes, now);
        assert!(!orch.is_type_in_progress(kind));
    }
    assert_eq!(
        role.preprocessed,
        [
            OrchOp::OwconfigCnf,
            OrchOp::TxCfgRenew,
            OrchOp::BssDelete,
            OrchOp::TopoSync,
        ]
    );
}
