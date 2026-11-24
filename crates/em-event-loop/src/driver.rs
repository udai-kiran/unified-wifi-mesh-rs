//! The async driver (note 0016 D2, U14): runs [`LoopCore`] passes against an [`Io`].
//!
//! It needs no async runtime. The `Io` implementation does the real work (a clock, sockets,
//! tokio); the application ([`App`]) owns the events and actions and serves each turn.

use std::future::Future;

use crate::{LoopCore, Turn};

/// What the driver needs from the outside world.
pub trait Io<T: Ord + Copy> {
    /// What arrives from outside (a received frame, a reply, ...).
    type Event;
    /// What the application asks the outside world to do (a write, an offloaded job, ...).
    type Action;

    /// The current time.
    fn now(&mut self) -> T;

    /// Suspends until an event is available or the deadline has passed; `None` means no
    /// deadline. A deadline that is already due must return at once: a deadline set again
    /// during a pass can be due when the pass ends, and it must not be lost (spec S9).
    /// An occasional spurious return is tolerated (the driver runs one empty pass and waits
    /// again), but a `wait` that keeps returning with no event and no due deadline makes the
    /// driver spin.
    fn wait(&mut self, deadline: Option<T>) -> impl Future<Output = ()> + Send;

    /// Appends the events available now. Never waits.
    fn poll_events(&mut self, events: &mut Vec<Self::Event>);

    /// Starts one external action. Never waits; the result comes back as an event.
    fn perform(&mut self, action: Self::Action);
}

/// The application that [`run`] drives.
pub trait App<T: Ord + Copy> {
    /// Same type as [`Io::Event`].
    type Event;
    /// Same type as [`Io::Action`].
    type Action;

    /// Takes one event: stores it and marks the sources it feeds ready in `core`.
    fn on_event(&mut self, event: Self::Event, core: &mut LoopCore<T>);

    /// Serves one turn at time `now`. Actions between the application's own handlers are
    /// routed at once, inside this call; external actions are pushed to `actions`.
    fn serve(
        &mut self,
        turn: Turn,
        now: T,
        core: &mut LoopCore<T>,
        actions: &mut Vec<Self::Action>,
    );

    /// Whether the loop should stop. Checked only between passes.
    fn finished(&self) -> bool;
}

/// Runs the loop until `app.finished()`.
///
/// Per round: one pass at `io.now()`, then every external action of the pass goes to
/// [`Io::perform`] in the order the application produced it (0016 E3, AC3), then the events
/// that arrived are delivered. This is the driver rule (0016 D2): between two passes every
/// action is performed and readiness refreshed, without waiting. Only then does the driver
/// wait, and only if no source is ready; it waits until the next deadline of `core`.
///
/// `finished()` is checked after the first delivery and after each pass's actions are
/// performed and its events delivered, never inside a pass. So a pass that makes the
/// application finish still has all its actions performed, and no wait follows it.
pub async fn run<T, I, A>(core: &mut LoopCore<T>, io: &mut I, app: &mut A)
where
    T: Ord + Copy,
    I: Io<T>,
    A: App<T, Event = I::Event, Action = I::Action>,
{
    let mut events = Vec::new();
    let mut actions = Vec::new();
    deliver_events(core, io, app, &mut events);
    while !app.finished() {
        let now = io.now();
        core.begin_pass(now);
        loop {
            let turn = core.next_turn();
            if turn == Turn::PassDone {
                break;
            }
            app.serve(turn, now, core, &mut actions);
        }
        for action in actions.drain(..) {
            io.perform(action);
        }
        deliver_events(core, io, app, &mut events);
        if !app.finished() && !core.has_ready_source() {
            io.wait(core.next_deadline()).await;
            deliver_events(core, io, app, &mut events);
        }
    }
}

/// Polls the events available now and hands each to the application, in arrival order.
fn deliver_events<T, I, A>(
    core: &mut LoopCore<T>,
    io: &mut I,
    app: &mut A,
    events: &mut Vec<I::Event>,
) where
    T: Ord + Copy,
    I: Io<T>,
    A: App<T, Event = I::Event, Action = I::Action>,
{
    io.poll_events(events);
    for event in events.drain(..) {
        app.on_event(event, core);
    }
}
