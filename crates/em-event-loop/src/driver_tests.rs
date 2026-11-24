//! Driver tests (note 0016 AC3, driver rule): a fake `Io` over `u64` time that records every
//! call, and a toy application with one source (an inbox) and one handler.

use std::cell::Cell;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use super::{App, HandlerId, Io, LoopCore, SourceId, Turn, run};

/// Runs a future that never returns `Pending` (the fake `Io` never blocks).
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    match future.as_mut().poll(&mut context) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("the fake Io never blocks"),
    }
}

fn assert_send<F: Send>(_: &F) {}

thread_local! {
    /// How many actions the fake `Io` has performed on this test's thread, so the app can
    /// see whether any were performed before one of its turns.
    static PERFORMED: Cell<usize> = const { Cell::new(0) };
}

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Data(u32),
    Stop,
}

#[derive(Debug, PartialEq, Eq)]
enum Call {
    Poll,
    Wait(Option<u64>),
    Perform(u32),
}

/// `poll_script` gives the events of each `poll_events` call (empty once used up).
/// `wake_script` gives, per `wait` that reaches it, the time of the wake and its events.
#[derive(Default)]
struct FakeIo {
    clock: u64,
    poll_script: VecDeque<Vec<Event>>,
    wake_script: VecDeque<(u64, Vec<Event>)>,
    woken_events: Vec<Event>,
    log: Vec<Call>,
}

impl Io<u64> for FakeIo {
    type Event = Event;
    type Action = u32;

    fn now(&mut self) -> u64 {
        self.clock
    }

    fn wait(&mut self, deadline: Option<u64>) -> impl Future<Output = ()> + Send {
        self.log.push(Call::Wait(deadline));
        let next_wake = self.wake_script.front().map(|(time, _)| *time);
        match (deadline, next_wake) {
            (Some(due), Some(wake)) if due <= wake => self.clock = self.clock.max(due),
            (Some(due), None) => self.clock = self.clock.max(due),
            (_, Some(_)) => {
                let (time, events) = self.wake_script.pop_front().unwrap();
                self.clock = self.clock.max(time);
                self.woken_events.extend(events);
            }
            (None, None) => panic!("wait(None) with nothing scripted would block forever"),
        }
        std::future::ready(())
    }

    fn poll_events(&mut self, events: &mut Vec<Event>) {
        self.log.push(Call::Poll);
        events.append(&mut self.woken_events);
        if let Some(scripted) = self.poll_script.pop_front() {
            events.extend(scripted);
        }
    }

    fn perform(&mut self, action: u32) {
        PERFORMED.with(|count| count.set(count.get() + 1));
        self.log.push(Call::Perform(action));
    }
}

/// `Data(n)` queues `n` in the inbox. Serving the source takes one item and produces the
/// actions `n * 10`, `n * 10 + 1`, `n * 10 + 2`; a deadline produces `900`. Serving `9`
/// finishes the app, as does `Stop`.
#[derive(Default)]
struct ToyApp {
    inbox: VecDeque<u32>,
    stop: bool,
    /// Marks the source ready again after every turn, whether or not it has work.
    always_ready: bool,
    turns: Vec<(Turn, u64)>,
    /// Actions the `Io` had performed when each turn was served.
    performed_before_turn: Vec<usize>,
}

impl App<u64> for ToyApp {
    type Event = Event;
    type Action = u32;

    fn on_event(&mut self, event: Event, core: &mut LoopCore<u64>) {
        match event {
            Event::Data(item) => {
                self.inbox.push_back(item);
                core.set_ready(SourceId(0), true);
            }
            Event::Stop => self.stop = true,
        }
    }

    fn serve(&mut self, turn: Turn, now: u64, core: &mut LoopCore<u64>, actions: &mut Vec<u32>) {
        self.turns.push((turn, now));
        self.performed_before_turn.push(PERFORMED.with(Cell::get));
        match turn {
            Turn::Serve(_) => {
                if let Some(item) = self.inbox.pop_front() {
                    actions.extend([item * 10, item * 10 + 1, item * 10 + 2]);
                    if item == 9 {
                        self.stop = true;
                    }
                }
                if self.always_ready || !self.inbox.is_empty() {
                    core.set_ready(SourceId(0), true);
                }
            }
            Turn::Deadline(_) => actions.push(900),
            Turn::PassDone => unreachable!("the driver does not serve PassDone"),
        }
    }

    fn finished(&self) -> bool {
        self.stop
    }
}

fn drive(core: &mut LoopCore<u64>, io: &mut FakeIo, app: &mut ToyApp) {
    block_on(run(core, io, app));
}

fn waits(io: &FakeIo) -> Vec<Option<u64>> {
    io.log
        .iter()
        .filter_map(|call| match call {
            Call::Wait(deadline) => Some(*deadline),
            _ => None,
        })
        .collect()
}

#[test]
fn actions_are_performed_in_order_before_the_next_poll() {
    let mut core = LoopCore::new(&[1], 1);
    let mut io = FakeIo {
        poll_script: VecDeque::from([vec![Event::Data(1)], vec![Event::Stop]]),
        ..FakeIo::default()
    };
    let mut app = ToyApp::default();
    drive(&mut core, &mut io, &mut app);
    assert_eq!(
        io.log,
        [
            Call::Poll,
            Call::Perform(10),
            Call::Perform(11),
            Call::Perform(12),
            Call::Poll,
        ]
    );
}

#[test]
fn events_between_passes_are_delivered_without_a_wait() {
    let mut core = LoopCore::new(&[1], 1);
    let mut io = FakeIo {
        poll_script: VecDeque::from([
            vec![Event::Data(1)],
            vec![Event::Data(2)],
            vec![Event::Stop],
        ]),
        ..FakeIo::default()
    };
    let mut app = ToyApp::default();
    drive(&mut core, &mut io, &mut app);
    let performed: Vec<_> = io.log.iter().filter(|call| **call != Call::Poll).collect();
    assert_eq!(
        performed,
        [
            &Call::Perform(10),
            &Call::Perform(11),
            &Call::Perform(12),
            &Call::Perform(20),
            &Call::Perform(21),
            &Call::Perform(22),
        ]
    );
    assert_eq!(app.turns.len(), 2);
}

#[test]
fn a_source_still_ready_after_a_pass_gets_another_pass_at_once() {
    let mut core = LoopCore::new(&[1], 1);
    let mut io = FakeIo {
        poll_script: VecDeque::from([
            vec![Event::Data(1), Event::Data(2)],
            vec![],
            vec![Event::Stop],
        ]),
        ..FakeIo::default()
    };
    let mut app = ToyApp::default();
    drive(&mut core, &mut io, &mut app);
    assert_eq!(waits(&io), Vec::<Option<u64>>::new());
    assert_eq!(
        app.turns,
        [(Turn::Serve(SourceId(0)), 0), (Turn::Serve(SourceId(0)), 0)]
    );
}

#[test]
fn wait_gets_the_next_deadline_and_the_due_deadline_runs() {
    let mut core = LoopCore::new(&[1], 1);
    core.set_deadline(HandlerId(0), Some(10));
    let mut io = FakeIo {
        wake_script: VecDeque::from([(50, vec![Event::Stop])]),
        ..FakeIo::default()
    };
    let mut app = ToyApp::default();
    drive(&mut core, &mut io, &mut app);
    assert_eq!(
        io.log,
        [
            Call::Poll, // start
            Call::Poll, // after the empty pass at time 0 (the deadline is not due yet)
            Call::Wait(Some(10)),
            Call::Poll,         // after the wake at time 10
            Call::Perform(900), // the deadline's action, before the next poll and wait
            Call::Poll,
            Call::Wait(None),
            Call::Poll,
        ]
    );
    assert_eq!(app.turns, [(Turn::Deadline(HandlerId(0)), 10)]);
}

#[test]
fn a_source_ready_after_every_turn_does_not_hold_back_a_due_deadline() {
    let mut core = LoopCore::new(&[3], 1);
    core.set_deadline(HandlerId(0), Some(0));
    let mut io = FakeIo {
        poll_script: VecDeque::from([vec![Event::Data(1)], vec![], vec![Event::Stop]]),
        ..FakeIo::default()
    };
    let mut app = ToyApp {
        always_ready: true,
        ..ToyApp::default()
    };
    drive(&mut core, &mut io, &mut app);
    let serve = (Turn::Serve(SourceId(0)), 0);
    let deadline = (Turn::Deadline(HandlerId(0)), 0);
    assert_eq!(app.turns[..4], [serve, serve, serve, deadline]);
    assert_eq!(waits(&io), Vec::<Option<u64>>::new());
}

#[test]
fn finishing_stops_after_the_last_pass_actions_are_performed() {
    let mut core = LoopCore::new(&[1], 1);
    let mut io = FakeIo {
        poll_script: VecDeque::from([vec![Event::Data(9)]]),
        ..FakeIo::default()
    };
    let mut app = ToyApp::default();
    drive(&mut core, &mut io, &mut app);
    assert_eq!(
        io.log,
        [
            Call::Poll,
            Call::Perform(90),
            Call::Perform(91),
            Call::Perform(92),
            Call::Poll,
        ]
    );
    assert_eq!(app.turns.len(), 1);
}

#[test]
fn nothing_ready_and_no_deadline_waits_with_none() {
    let mut core = LoopCore::new(&[1], 1);
    let mut io = FakeIo {
        wake_script: VecDeque::from([(5, vec![Event::Stop])]),
        ..FakeIo::default()
    };
    let mut app = ToyApp::default();
    drive(&mut core, &mut io, &mut app);
    assert_eq!(waits(&io), [None]);
}

#[test]
fn the_run_future_is_send() {
    let mut core = LoopCore::new(&[1], 1);
    let mut io = FakeIo::default();
    let mut app = ToyApp::default();
    assert_send(&run(&mut core, &mut io, &mut app));
}

#[test]
fn no_action_is_performed_before_the_pass_ends() {
    let mut core = LoopCore::new(&[2], 1);
    let mut io = FakeIo {
        poll_script: VecDeque::from([vec![Event::Data(1), Event::Data(2)], vec![Event::Stop]]),
        ..FakeIo::default()
    };
    let mut app = ToyApp::default();
    drive(&mut core, &mut io, &mut app);
    assert_eq!(app.turns.len(), 2);
    assert_eq!(app.performed_before_turn, [0, 0]);
    assert_eq!(PERFORMED.with(Cell::get), 6);
}
