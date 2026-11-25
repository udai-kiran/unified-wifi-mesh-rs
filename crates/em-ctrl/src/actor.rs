// SPDX-License-Identifier: Apache-2.0

//! The controller on the event loop (0016 D2, D3b; D8 step 5): one `LoopCore` per process whose
//! handlers are the transport ([`TransportHandler`]) and the role ([`CtrlCore`]). The app below
//! moves the transport's received SDUs into the core and the core's frames into the transport.
//! All protocol logic is in the core.
//!
//! # Stop
//!
//! `TransportIo` has one application channel. The controller has no platform, so the channel
//! carries nothing ([`Infallible`]): a future, polled in the same task as the loop, drops the
//! channel's sender when `stop` completes, and the closed channel is the stop signal.
//!
//! # Link changes
//!
//! After every turn the app compares the transport's link with what the role was last told and
//! tells it the difference in order ([`link_changes`]). A reconnect always gives `unregistered()`
//! before `registered()`, even if the loop never saw the state in between (0016 D5).

use std::convert::Infallible;
use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::time::Instant;

use em_dm::NetworkSsidInfo;
use em_event_loop::{App, HandlerId, LoopCore, SourceId, Turn, run as run_event_loop};
use em_transport::{
    IoAction, IoEvent, LinkState, OutboundSdu, Role, Sdu, TransportConfig, TransportError,
    TransportHandler, TransportIo, TransportPolicy, TransportSources,
};
use em_types::MacAddr;
use tokio::sync::mpsc;

use crate::{AlMacChanged, CtrlCore, Frame};

/// The sources of the loop, in the order of 0016 D3b.
const READ: SourceId = SourceId(0);
const REGISTRATION: SourceId = SourceId(1);
const APPLICATION: SourceId = SourceId(2);
const RECEIVED: SourceId = SourceId(3);
const WRITE: SourceId = SourceId(4);

/// The timers of the loop.
const TRANSPORT_TIMER: HandlerId = HandlerId(0);
const ROLE_TIMER: HandlerId = HandlerId(1);
const HANDLER_COUNT: usize = 2;

/// Source budgets, in the order of the ids above (0016 D3b): 64 frames, then one turn each.
const BUDGETS: [usize; 5] = [64, 1, 1, 1, 1];

const TRANSPORT_SOURCES: TransportSources = TransportSources {
    read: READ,
    registration: REGISTRATION,
    write: WRITE,
    timer: TRANSPORT_TIMER,
};

/// What the controller needs.
#[derive(Debug)]
pub struct CtrlConfig {
    /// AL-SAP control socket path.
    pub control_path: PathBuf,
    /// AL-SAP data socket path.
    pub data_path: PathBuf,
    /// The reset file's `NetworkSSIDList` (0013 D2).
    pub networks: Vec<Box<NetworkSsidInfo>>,
    /// The test-only switch that drops the first M2's hand-off (0013 D10).
    pub drop_first_m2: bool,
}

/// Why the controller ended other than by a stop request. The process exits 1.
#[derive(Debug)]
pub enum ActorError {
    /// A registration returned a different AL MAC (0012 D9).
    AlMacChanged(AlMacChanged),
    /// The transport ended with an error, for example a rejected registration.
    Transport(TransportError),
}

impl fmt::Display for ActorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlMacChanged(error) => write!(f, "{error}"),
            Self::Transport(error) => write!(f, "transport ended: {error}"),
        }
    }
}

impl std::error::Error for ActorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::AlMacChanged(error) => Some(error),
            Self::Transport(error) => Some(error),
        }
    }
}

/// One thing the role has to be told about the link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkChange {
    /// The registration of `session` is gone.
    Unregistered { session: u64 },
    /// The transport registered `session` with `al_mac`.
    Registered { session: u64, al_mac: MacAddr },
}

/// What the role has to be told when the link goes from `told` to `current`, in order.
///
/// A new session of a registered link is `Unregistered` then `Registered`, also when the loop
/// never saw the link lost in between.
fn link_changes(told: LinkState, current: LinkState) -> Vec<LinkChange> {
    if told == current {
        return Vec::new();
    }
    let mut changes = Vec::new();
    if told.al_mac.is_some() {
        changes.push(LinkChange::Unregistered {
            session: told.session,
        });
    }
    if let Some(al_mac) = current.al_mac {
        changes.push(LinkChange::Registered {
            session: current.session,
            al_mac,
        });
    }
    changes
}

/// The loop's application: owns the role and the transport handler.
struct CtrlApp {
    role: CtrlCore,
    handler: TransportHandler,
    /// The link state the role has been told about.
    told: LinkState,
    /// Recorded at the first registration (0012 D9).
    al_mac: Option<MacAddr>,
    /// The application channel is closed: the stop signal, until the application source serves it.
    application_closed: bool,
    /// The stop signal was served: the role gets no further input.
    stopping: bool,
    /// A registration returned a different AL MAC: the loop ends at once.
    fatal: Option<AlMacChanged>,
}

impl CtrlApp {
    fn new(role: CtrlCore, now: Instant, loop_core: &mut LoopCore<Instant>) -> Self {
        let handler = TransportHandler::new(
            TransportPolicy::easymesh(),
            now,
            TRANSPORT_SOURCES,
            loop_core,
        );
        // The first turn of the application source arms the first receive.
        loop_core.set_ready(APPLICATION, true);
        Self {
            role,
            handler,
            told: LinkState::default(),
            al_mac: None,
            application_closed: false,
            stopping: false,
            fatal: None,
        }
    }

    /// False once the role must get no further input.
    fn role_is_active(&self) -> bool {
        !self.stopping && self.fatal.is_none()
    }

    /// Sets what depends on the role after an event or a turn: the received source and the role
    /// timer. Both are off while the role is not active.
    fn refresh(&self, loop_core: &mut LoopCore<Instant>) {
        let active = self.role_is_active();
        loop_core.set_ready(RECEIVED, active && self.handler.received_ready());
        let deadline = if active {
            self.role.next_deadline()
        } else {
            None
        };
        loop_core.set_deadline(ROLE_TIMER, deadline);
    }

    /// Tells the role about the link changes since the last time (0011 A5).
    fn sync_link(&mut self) {
        if !self.role_is_active() {
            return;
        }
        let current = self.handler.link();
        for change in link_changes(self.told, current) {
            self.tell_role(change);
            if self.fatal.is_some() {
                return;
            }
        }
        self.told = current;
    }

    fn tell_role(&mut self, change: LinkChange) {
        match change {
            LinkChange::Unregistered { session } => {
                self.role.unregistered();
                tracing::info!(session, "link lost, waiting to register");
            }
            LinkChange::Registered { session, al_mac } => {
                if let Err(changed) = self.role.registered(al_mac) {
                    self.fatal = Some(changed);
                    return;
                }
                if self.al_mac.is_none() {
                    tracing::info!(session, %al_mac, "registered");
                } else {
                    tracing::info!(session, %al_mac, "new session, same AL MAC");
                }
                self.al_mac = Some(al_mac);
            }
        }
    }

    /// Puts each frame into the outbound buffer of the current session, stamped with the
    /// recorded AL MAC (0012 D9). A frame of a dead session is dropped and counted by the
    /// transport.
    fn send_frames(&mut self, now: Instant, frames: Vec<Frame>, loop_core: &mut LoopCore<Instant>) {
        if frames.is_empty() {
            return;
        }
        let Some(local) = self.al_mac else {
            tracing::warn!("frames built before the first registration were dropped");
            return;
        };
        for frame in frames {
            let outbound = OutboundSdu {
                session: self.handler.link().session,
                sdu: Sdu {
                    local,
                    remote: frame.remote,
                    cmdu: frame.cmdu,
                },
            };
            self.handler.send(now, outbound, loop_core);
        }
    }

    fn serve_application(
        &mut self,
        now: Instant,
        loop_core: &mut LoopCore<Instant>,
        actions: &mut Vec<IoAction>,
    ) {
        if self.application_closed {
            self.stopping = true;
            self.handler.stop(now, loop_core, actions);
            // No receive is armed again.
            return;
        }
        actions.push(IoAction::RecvApp);
    }

    fn serve_received(&mut self, now: Instant, loop_core: &mut LoopCore<Instant>) {
        if !self.role_is_active() {
            return;
        }
        let Some(sdu) = self.handler.take_received() else {
            return;
        };
        let frames = self.role.received(sdu.remote, &sdu.cmdu, now);
        self.send_frames(now, frames, loop_core);
    }

    fn serve_role_timer(&mut self, now: Instant, loop_core: &mut LoopCore<Instant>) {
        if !self.role_is_active() {
            return;
        }
        let frames = self.role.tick(now);
        self.send_frames(now, frames, loop_core);
    }
}

impl App<Instant> for CtrlApp {
    type Event = IoEvent<Infallible>;
    type Action = IoAction;

    fn on_event(&mut self, event: Self::Event, loop_core: &mut LoopCore<Instant>) {
        match event {
            IoEvent::Transport(event) => self.handler.on_event(event, loop_core),
            // The channel carries no values.
            IoEvent::App(never) => match never {},
            IoEvent::AppClosed => {
                self.application_closed = true;
                loop_core.set_ready(APPLICATION, true);
            }
        }
    }

    fn serve(
        &mut self,
        turn: Turn,
        now: Instant,
        loop_core: &mut LoopCore<Instant>,
        actions: &mut Vec<IoAction>,
    ) {
        // The driver checks `finished()` only after the pass. A turn served after the end could
        // stop the transport again, or hide its outcome behind a stop that was not asked for.
        if self.finished() {
            return;
        }
        if !self.handler.serve(turn, now, loop_core, actions) {
            match turn {
                Turn::Serve(APPLICATION) => self.serve_application(now, loop_core, actions),
                Turn::Serve(RECEIVED) => self.serve_received(now, loop_core),
                Turn::Deadline(ROLE_TIMER) => self.serve_role_timer(now, loop_core),
                Turn::Serve(_) | Turn::Deadline(_) | Turn::PassDone => {}
            }
        }
        self.sync_link();
        self.refresh(loop_core);
    }

    fn finished(&self) -> bool {
        self.fatal.is_some() || self.handler.outcome().is_some()
    }
}

/// Drops the application channel's sender when `stop` completes: that closes the channel, which
/// is the loop's stop signal.
async fn close_application_on_stop(
    sender: mpsc::Sender<Infallible>,
    stop: impl Future<Output = ()>,
) {
    stop.await;
    drop(sender);
}

/// Runs the controller until `stop` completes (returns `Ok`) or the transport ends
/// or the AL MAC changes (returns the error; the process exits 1).
///
/// A stop closes the application channel; the loop then runs the transport's stop: queued
/// frames are written for at most 900 ms, then the sockets stay open for 100 ms (0016 D3b).
///
/// # Errors
///
/// [`ActorError`] when the controller ends without a stop request.
///
/// # Panics
///
/// Never in practice: the loop ends only when the transport has finished or the AL MAC changed.
pub async fn run(config: CtrlConfig, stop: impl Future<Output = ()>) -> Result<(), ActorError> {
    let (sender, application_events) = mpsc::channel(1);
    let transport_config =
        TransportConfig::new(config.control_path, config.data_path, Role::Controller);
    let mut io = TransportIo::new(&transport_config, application_events);
    let mut loop_core = LoopCore::new(&BUDGETS, HANDLER_COUNT);
    let now = tokio::time::Instant::now().into_std();
    let role = CtrlCore::new(config.networks, config.drop_first_m2);
    let mut app = CtrlApp::new(role, now, &mut loop_core);

    // Both futures run in this task. The stop future ends at the stop; the loop goes on until
    // the transport has finished, so only the loop ends this block.
    {
        let event_loop = run_event_loop(&mut loop_core, &mut io, &mut app);
        let closer = close_application_on_stop(sender, stop);
        tokio::pin!(event_loop, closer);
        let mut waiting_for_stop = true;
        loop {
            tokio::select! {
                () = &mut event_loop => break,
                () = &mut closer, if waiting_for_stop => waiting_for_stop = false,
            }
        }
    }

    if let Some(changed) = app.fatal {
        return Err(ActorError::AlMacChanged(changed));
    }
    let outcome = app
        .handler
        .outcome()
        .expect("the loop ends only when the transport has finished or the AL MAC changed");
    let result = io.into_result(outcome);
    if !app.stopping {
        return result.map_err(ActorError::Transport);
    }
    match result {
        Ok(()) => tracing::info!("transport stopped"),
        Err(error) => tracing::warn!(%error, "transport ended with an error while stopping"),
    }
    Ok(())
}

// Tests moved or retired by step 5 (0016 D8 step 5), with their replacements:
//
// * `sync_link_returns_the_state_it_copied` (unit): retired. The actor no longer copies a
//   watch value; `CtrlApp::sync_link` reads `handler.link()` in the same turn. Replaced by
//   `link_changes_*` (the pure function) and `a_reconnect_tells_the_role_unregistered_then_registered`.
// * `frames_are_stamped_with_the_synced_session` (unit): retired. A frame is stamped when it is
//   handed to the transport, from the link the role was told in the same turn. Replaced by
//   `a_role_frame_lands_in_the_transport` (the frame is accepted into the session, not dropped
//   and counted) and `tests/actor.rs`
//   `a_search_is_answered_with_a_response_for_the_current_session`.
// * `pending.rs` `a_full_queue_drops_the_oldest_and_counts`: retired, tag
//   `intentional-difference:tx-buffer`. `PendingFrames` (64 frames, drop the oldest) is replaced
//   by the transport's outbound buffer (4 MiB, drop the oldest not yet started,
//   `dropped_send_overflow`, 0016 U12). The bound itself is tested in em-transport by
//   `the_outbound_buffer_overflow_counts_and_drops_the_oldest_not_started` (`TransportCore`) and
//   `outbound_buffer_of_4_mib_drops_the_oldest_unstarted_sdus_and_counts_them` (the loop, in
//   `em-transport/tests/loop_session.rs`).
#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::time::Duration;

    use em_core::configuration::build_search;
    use em_event_loop::{App, LoopCore, Turn};
    use em_transport::{
        Frame, IoAction, IoEvent, LinkState, TransportEvent, TransportIoAction, TransportOutcome,
    };
    use em_types::{FreqBand, MacAddr};

    use super::{BUDGETS, CtrlApp, HANDLER_COUNT, LinkChange, link_changes};
    use crate::CtrlCore;

    use std::time::Instant;

    const AL_MAC: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 2]);
    const OTHER_AL_MAC: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 9]);
    const AGENT: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 1]);

    fn linked(session: u64, al_mac: Option<MacAddr>) -> LinkState {
        LinkState { session, al_mac }
    }

    #[test]
    fn link_changes_is_empty_without_a_change() {
        let registered = linked(1, Some(AL_MAC));
        assert_eq!(link_changes(registered, registered), []);
        assert_eq!(link_changes(LinkState::default(), LinkState::default()), []);
    }

    #[test]
    fn link_changes_registered_after_the_start() {
        assert_eq!(
            link_changes(LinkState::default(), linked(1, Some(AL_MAC))),
            [LinkChange::Registered {
                session: 1,
                al_mac: AL_MAC
            }]
        );
    }

    #[test]
    fn link_changes_unregistered_when_the_link_is_lost() {
        assert_eq!(
            link_changes(linked(1, Some(AL_MAC)), linked(1, None)),
            [LinkChange::Unregistered { session: 1 }]
        );
    }

    #[test]
    fn link_changes_registered_again_after_a_lost_link() {
        assert_eq!(
            link_changes(linked(1, None), linked(2, Some(AL_MAC))),
            [LinkChange::Registered {
                session: 2,
                al_mac: AL_MAC
            }]
        );
    }

    /// The loop never saw the link lost: session 1 straight to session 2. The role must still
    /// get `unregistered()` first (0016 D5).
    #[test]
    fn link_changes_unregistered_then_registered_when_the_loss_was_skipped() {
        assert_eq!(
            link_changes(linked(1, Some(AL_MAC)), linked(2, Some(AL_MAC))),
            [
                LinkChange::Unregistered { session: 1 },
                LinkChange::Registered {
                    session: 2,
                    al_mac: AL_MAC
                }
            ]
        );
    }

    /// The app, a real `LoopCore` and the clock the test moves. The test plays the `Io`: it
    /// reports what the sockets would report.
    struct Rig {
        loop_core: LoopCore<Instant>,
        app: CtrlApp,
        now: Instant,
    }

    impl Rig {
        fn new() -> Self {
            let now = Instant::now();
            let mut loop_core = LoopCore::new(&BUDGETS, HANDLER_COUNT);
            let role = CtrlCore::new(Vec::new(), false);
            let app = CtrlApp::new(role, now, &mut loop_core);
            Self {
                loop_core,
                app,
                now,
            }
        }

        /// One pass at the current time; returns the actions it produced.
        fn pass(&mut self) -> Vec<IoAction> {
            self.loop_core.begin_pass(self.now);
            let mut actions = Vec::new();
            loop {
                let turn = self.loop_core.next_turn();
                if turn == Turn::PassDone {
                    return actions;
                }
                self.app
                    .serve(turn, self.now, &mut self.loop_core, &mut actions);
            }
        }

        fn event(&mut self, event: IoEvent<Infallible>) {
            self.app.on_event(event, &mut self.loop_core);
        }

        fn transport_event(&mut self, event: TransportEvent) {
            self.event(IoEvent::Transport(event));
        }

        fn advance(&mut self, by: Duration) {
            self.now += by;
        }

        /// Connects, registers with `al_mac` and arms the first read.
        fn register(&mut self, al_mac: MacAddr) {
            let first = self.pass();
            assert!(first.contains(&IoAction::Transport(TransportIoAction::Connect)));
            self.transport_event(TransportEvent::Registered(response(al_mac)));
            self.pass();
            assert_eq!(self.app.handler.link().al_mac, Some(al_mac));
        }

        /// Ends the data socket and waits for the reconnect.
        fn lose_link(&mut self) {
            self.transport_event(TransportEvent::ReadEnded);
            self.pass();
            assert_eq!(self.app.handler.link().al_mac, None);
        }

        fn reconnect(&mut self, al_mac: MacAddr) {
            self.advance(Duration::from_secs(2));
            self.register(al_mac);
        }

        /// The data socket delivers a Search from the agent, as one unfragmented SDU frame.
        fn receive_search(&mut self) {
            let cmdu = build_search(AGENT, FreqBand::BAND_5_GHZ, 3, 1).unwrap();
            let mut body = AL_MAC.octets().to_vec();
            body.extend_from_slice(&AGENT.octets());
            body.extend_from_slice(&[0, 1, 0]);
            body.extend_from_slice(&cmdu);
            let mut frame = u32::try_from(body.len()).unwrap().to_be_bytes().to_vec();
            frame.extend_from_slice(&body);
            self.transport_event(TransportEvent::Bytes(frame));
        }
    }

    fn response(al_mac: MacAddr) -> Frame {
        let mut body = al_mac.octets().to_vec();
        body.push(1);
        Frame::Complete(body)
    }

    fn has_write(actions: &[IoAction]) -> bool {
        actions
            .iter()
            .any(|action| matches!(action, IoAction::Transport(TransportIoAction::Write(_))))
    }

    /// A Search from the agent makes the core build a Response; the app hands it to the
    /// transport, which writes it.
    #[test]
    fn a_role_frame_lands_in_the_transport() {
        let mut rig = Rig::new();
        rig.register(AL_MAC);
        rig.receive_search();
        let actions = rig.pass();
        assert!(has_write(&actions), "no Write after the role's frame");
        assert_eq!(rig.app.handler.counters().dropped_send_disconnected, 0);
    }

    /// While the link is lost the role gets `unregistered()`; after the new session it is
    /// registered again. A role that missed `unregistered()` would answer on a dead session, and
    /// the transport would count the frame dropped.
    #[test]
    fn a_reconnect_tells_the_role_unregistered_then_registered() {
        let mut rig = Rig::new();
        rig.register(AL_MAC);

        rig.lose_link();
        assert_eq!(rig.app.told, linked(1, None));

        rig.reconnect(AL_MAC);
        assert_eq!(rig.app.told, linked(2, Some(AL_MAC)));
        assert!(rig.app.fatal.is_none());
        rig.receive_search();
        let actions = rig.pass();
        assert!(has_write(&actions), "no Response on the new session");
        assert_eq!(rig.app.handler.counters().dropped_send_disconnected, 0);
    }

    #[test]
    fn a_different_al_mac_ends_the_loop_at_once() {
        let mut rig = Rig::new();
        rig.register(AL_MAC);
        rig.lose_link();
        assert!(!rig.app.finished());

        rig.reconnect(OTHER_AL_MAC);
        let changed = rig.app.fatal.as_ref().expect("the AL MAC change is fatal");
        assert_eq!(changed.recorded, AL_MAC);
        assert_eq!(changed.new, OTHER_AL_MAC);
        assert!(rig.app.finished());
    }

    /// The rejection is handled on the registration turn; the closed channel on the application
    /// turn of the same pass. Without the check in `serve` the second turn would stop the
    /// transport and `run` would return `Ok` instead of the rejection.
    #[test]
    fn no_turn_is_served_after_the_transport_finished() {
        let mut rig = Rig::new();
        let first = rig.pass();
        assert!(first.contains(&IoAction::Transport(TransportIoAction::Connect)));
        let mut body = AL_MAC.octets().to_vec();
        body.push(0);
        rig.transport_event(TransportEvent::Registered(Frame::Complete(body)));
        rig.event(IoEvent::AppClosed);

        rig.pass();
        assert_eq!(
            rig.app.handler.outcome(),
            Some(TransportOutcome::Rejected { result: 0 })
        );
        assert!(!rig.app.stopping);
    }

    /// The AL MAC change is found on the registration turn; the closed channel on the application
    /// turn of the same pass must not stop the transport.
    #[test]
    fn no_turn_is_served_after_an_al_mac_change() {
        let mut rig = Rig::new();
        rig.register(AL_MAC);
        rig.lose_link();
        rig.advance(Duration::from_secs(2));
        let connect = rig.pass();
        assert!(connect.contains(&IoAction::Transport(TransportIoAction::Connect)));
        rig.transport_event(TransportEvent::Registered(response(OTHER_AL_MAC)));
        rig.event(IoEvent::AppClosed);

        let pass = rig.pass();
        assert!(rig.app.fatal.is_some());
        assert!(!rig.app.stopping);
        assert!(!pass.contains(&IoAction::Transport(TransportIoAction::CloseSession)));
    }

    /// Planted-bug check: without the `role_is_active` guards the role would get input after
    /// the stop.
    #[test]
    fn the_role_gets_no_input_after_the_application_channel_closed() {
        let mut rig = Rig::new();
        rig.register(AL_MAC);
        rig.event(IoEvent::AppClosed);
        let stop_pass = rig.pass();
        assert!(rig.app.stopping);
        assert!(
            !stop_pass.contains(&IoAction::RecvApp),
            "a receive was armed after the stop"
        );

        // A Search that arrives after the stop is not answered.
        rig.receive_search();
        let later = rig.pass();
        assert!(!has_write(&later));
    }

    /// A frame is in flight and its write is blocked when the stop comes: the transport writes
    /// for 900 ms, lingers for 100 ms and finishes, so the stop takes 1 s (0016 D3b).
    #[test]
    fn a_stop_with_a_frame_in_flight_finishes_within_a_second() {
        let mut rig = Rig::new();
        rig.register(AL_MAC);
        rig.receive_search();
        let write_pass = rig.pass();
        assert!(has_write(&write_pass));
        rig.transport_event(TransportEvent::WriteBlocked);
        rig.pass();

        let stopped_at = rig.now;
        rig.event(IoEvent::AppClosed);
        rig.pass();
        assert!(!rig.app.finished());

        rig.advance(Duration::from_millis(900));
        rig.pass();
        assert!(!rig.app.finished(), "the linger is still running");
        rig.advance(Duration::from_millis(100));
        rig.pass();

        assert!(rig.app.finished());
        assert_eq!(rig.app.handler.outcome(), Some(TransportOutcome::Stopped));
        assert!(rig.now.duration_since(stopped_at) <= Duration::from_secs(1));
        assert!(rig.app.fatal.is_none());
    }
}
