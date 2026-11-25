// SPDX-License-Identifier: Apache-2.0

//! The agent on the event loop (0016 D2, D3b; D8 step 4c): one `LoopCore` per process whose
//! handlers are the transport ([`TransportHandler`]) and the role ([`AgentCore`]). The app below
//! moves the transport's received SDUs and the platform's confirmations into the core, the core's
//! frames into the transport, and the configurations of accepted M2s to the [`Platform`]. All
//! protocol logic is in the core.
//!
//! # Stop
//!
//! `TransportIo` has one application channel. A forwarder future, polled in the same task as the
//! loop, moves the platform's confirmations into it and drops its sender when `stop` completes;
//! the closed channel is the stop signal. A closed platform channel is not a stop.
//!
//! # Link changes
//!
//! After every turn the app compares the transport's link with what the role was last told and
//! tells it the difference in order ([`link_changes`]). A reconnect always gives `unregistered()`
//! before `registered()`, even if the loop never saw the state in between (0016 D5).

use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::time::Instant;

use em_event_loop::{App, HandlerId, LoopCore, SourceId, Turn, run as run_event_loop};
use em_transport::{
    IoAction, IoEvent, LinkState, OutboundSdu, Role, Sdu, TransportConfig, TransportError,
    TransportHandler, TransportIo, TransportPolicy, TransportSources,
};
use em_types::MacAddr;
use tokio::sync::mpsc;

use crate::platform::{Confirmation, Platform};
use crate::{AgentCore, AlMacChanged, Frame};

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

/// How many platform confirmations can wait for the loop.
const APPLICATION_CHANNEL_CAPACITY: usize = 8;

/// What the agent needs besides the platform.
#[derive(Debug)]
pub struct AgentConfig {
    /// AL-SAP control socket path.
    pub control_path: PathBuf,
    /// AL-SAP data socket path.
    pub data_path: PathBuf,
    /// The Multi-AP profile to advertise, 2 or 3 (0012 D4).
    pub profile: u8,
}

/// Why the agent ended other than by a stop request. The process exits 1.
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

/// What the application channel delivered, until the application source serves it.
enum ApplicationInput {
    Confirmation(Confirmation),
    /// The channel is closed: the stop signal.
    Closed,
}

/// The loop's application: owns the role, the platform and the transport handler.
struct AgentApp<P: Platform> {
    role: AgentCore,
    platform: P,
    handler: TransportHandler,
    /// The link state the role has been told about.
    told: LinkState,
    /// Recorded at the first registration (0012 D9).
    al_mac: Option<MacAddr>,
    arrived: Option<ApplicationInput>,
    /// The stop signal was served: the role gets no further input.
    stopping: bool,
    /// A registration returned a different AL MAC: the loop ends at once.
    fatal: Option<AlMacChanged>,
}

impl<P: Platform> AgentApp<P> {
    fn new(profile: u8, platform: P, now: Instant, loop_core: &mut LoopCore<Instant>) -> Self {
        let handler = TransportHandler::new(
            TransportPolicy::easymesh(),
            now,
            TRANSPORT_SOURCES,
            loop_core,
        );
        // The first turn of the application source arms the first receive.
        loop_core.set_ready(APPLICATION, true);
        Self {
            role: AgentCore::new(profile, platform.radios()),
            platform,
            handler,
            told: LinkState::default(),
            al_mac: None,
            arrived: None,
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
    fn sync_link(&mut self, now: Instant) {
        if !self.role_is_active() {
            return;
        }
        let current = self.handler.link();
        for change in link_changes(self.told, current) {
            self.tell_role(change, now);
            if self.fatal.is_some() {
                return;
            }
        }
        self.told = current;
    }

    fn tell_role(&mut self, change: LinkChange, now: Instant) {
        match change {
            LinkChange::Unregistered { session } => {
                self.role.unregistered();
                tracing::info!(session, "link lost, waiting to register");
            }
            LinkChange::Registered { session, al_mac } => {
                if let Err(changed) = self.role.registered(al_mac, now) {
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

    /// Hands the configurations of accepted M2s to the platform.
    fn apply_platform_requests(&mut self) {
        for request in self.role.take_platform_requests() {
            self.platform
                .apply_radio_config(request.radio, &request.bss);
        }
    }

    fn serve_application(
        &mut self,
        now: Instant,
        loop_core: &mut LoopCore<Instant>,
        actions: &mut Vec<IoAction>,
    ) {
        match self.arrived.take() {
            Some(ApplicationInput::Confirmation(confirmation)) => {
                // The platform confirmed every radio's configuration (0013 D4).
                if self.role_is_active() {
                    self.role.platform_confirmed(confirmation.bss, now);
                }
            }
            Some(ApplicationInput::Closed) => {
                self.stopping = true;
                self.handler.stop(now, loop_core, actions);
                // No receive is armed again.
                return;
            }
            None => {}
        }
        // The next receive is armed only now, so the channel feeds the loop one value per turn.
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
        self.apply_platform_requests();
    }

    fn serve_role_timer(&mut self, now: Instant, loop_core: &mut LoopCore<Instant>) {
        if !self.role_is_active() {
            return;
        }
        let frames = self.role.tick(now);
        self.send_frames(now, frames, loop_core);
    }
}

impl<P: Platform> App<Instant> for AgentApp<P> {
    type Event = IoEvent<Confirmation>;
    type Action = IoAction;

    fn on_event(&mut self, event: Self::Event, loop_core: &mut LoopCore<Instant>) {
        match event {
            IoEvent::Transport(event) => self.handler.on_event(event, loop_core),
            IoEvent::App(confirmation) => {
                self.arrived = Some(ApplicationInput::Confirmation(confirmation));
                loop_core.set_ready(APPLICATION, true);
            }
            IoEvent::AppClosed => {
                self.arrived = Some(ApplicationInput::Closed);
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
        self.sync_link(now);
        self.refresh(loop_core);
    }

    fn finished(&self) -> bool {
        self.fatal.is_some() || self.handler.outcome().is_some()
    }
}

/// Moves the platform's confirmations into the application channel until `stop` completes, then
/// drops the sender: that closes the channel, which is the loop's stop signal.
async fn forward_confirmations(
    mut confirmations: mpsc::Receiver<Confirmation>,
    sender: mpsc::Sender<Confirmation>,
    stop: impl Future<Output = ()>,
) {
    tokio::pin!(stop);
    let mut platform_is_open = true;
    loop {
        tokio::select! {
            () = &mut stop => return,
            received = confirmations.recv(), if platform_is_open => match received {
                // A full channel waits for the loop, but a stop must not wait with it.
                Some(confirmation) => tokio::select! {
                    () = &mut stop => return,
                    _ = sender.send(confirmation) => {}
                },
                None => platform_is_open = false,
            },
        }
    }
}

/// Runs the agent until `stop` completes (returns `Ok`) or the transport ends
/// or the AL MAC changes (returns the error; the process exits 1).
///
/// A stop closes the application channel; the loop then runs the transport's stop: queued
/// frames are written for at most 900 ms, then the sockets stay open for 100 ms (0016 D3b).
///
/// # Errors
///
/// [`ActorError`] when the agent ends without a stop request.
///
/// # Panics
///
/// Never in practice: the loop ends only when the transport has finished or the AL MAC changed.
pub async fn run(
    config: AgentConfig,
    mut platform: impl Platform,
    stop: impl Future<Output = ()>,
) -> Result<(), ActorError> {
    let confirmations = platform.confirmations();
    let (sender, application_events) = mpsc::channel(APPLICATION_CHANNEL_CAPACITY);
    let transport_config = TransportConfig::new(config.control_path, config.data_path, Role::Agent);
    let mut io = TransportIo::new(&transport_config, application_events);
    let mut loop_core = LoopCore::new(&BUDGETS, HANDLER_COUNT);
    let now = tokio::time::Instant::now().into_std();
    let mut app = AgentApp::new(config.profile, platform, now, &mut loop_core);

    // Both futures run in this task. The forwarder ends at the stop; the loop goes on until the
    // transport has finished, so only the loop ends this block.
    {
        let event_loop = run_event_loop(&mut loop_core, &mut io, &mut app);
        let forwarder = forward_confirmations(confirmations, sender, stop);
        tokio::pin!(event_loop, forwarder);
        let mut forwarding = true;
        loop {
            tokio::select! {
                () = &mut event_loop => break,
                () = &mut forwarder, if forwarding => forwarding = false,
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

// Tests moved or retired by step 4c (0016 D8 step 4c), with their replacements:
//
// * `sync_link_returns_the_state_it_copied` (unit): retired. The actor no longer copies a
//   watch value; `AgentApp::sync_link` reads `handler.link()` in the same turn. Replaced by
//   `link_changes_*` (the pure function) and `a_reconnect_tells_the_role_unregistered_then_registered`.
// * `frames_are_stamped_with_the_synced_session` (unit): retired. A frame is stamped when it is
//   handed to the transport, from the link the role was told in the same turn. Replaced by
//   `a_role_frame_lands_in_the_transport` (the frame is accepted into the session, not dropped
//   and counted) and `tests/actor.rs` `searches_continue_on_a_new_session_with_the_same_al_mac`.
// * `pending.rs` `a_full_queue_drops_the_oldest_and_counts`: retired, tag
//   `intentional-difference:tx-buffer`. `PendingFrames` (64 frames, drop the oldest) is replaced
//   by the transport's outbound buffer (4 MiB, drop the oldest not yet started,
//   `dropped_send_overflow`, 0016 U12). The bound itself is tested in em-transport by
//   `the_outbound_buffer_overflow_counts_and_drops_the_oldest_not_started` (`TransportCore`) and
//   `outbound_buffer_of_4_mib_drops_the_oldest_unstarted_sdus_and_counts_them` (the loop, in
//   `em-transport/tests/loop_session.rs`).
#[cfg(test)]
mod tests {
    use std::time::Duration;

    use em_cmd::RadioSpec;
    use em_event_loop::{App, LoopCore, Turn};
    use em_transport::{
        Frame, IoAction, IoEvent, LinkState, TransportEvent, TransportIoAction, TransportOutcome,
    };
    use em_types::{FreqBand, MacAddr};

    use super::{AgentApp, BUDGETS, HANDLER_COUNT, LinkChange, link_changes};
    use crate::PROTO_IDLE;
    use crate::platform::{Confirmation, FakePlatform};

    use std::time::Instant;

    const AL_MAC: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 2]);
    const OTHER_AL_MAC: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 9]);

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
        app: AgentApp<FakePlatform>,
        now: Instant,
    }

    impl Rig {
        fn new() -> Self {
            let radios = vec![RadioSpec {
                ruid: MacAddr::new([2, 0, 0, 0, 1, 1]),
                band: FreqBand::BAND_5_GHZ,
            }];
            let now = Instant::now();
            let mut loop_core = LoopCore::new(&BUDGETS, HANDLER_COUNT);
            let app = AgentApp::new(2, FakePlatform::new(radios), now, &mut loop_core);
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

        fn event(&mut self, event: IoEvent<Confirmation>) {
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

    /// The registration made the radio search; its Search is due after `PROTO_IDLE`, and the
    /// tick hands it to the transport, which writes it on the next pass.
    #[test]
    fn a_role_frame_lands_in_the_transport() {
        let mut rig = Rig::new();
        rig.register(AL_MAC);
        rig.advance(PROTO_IDLE);
        let tick_pass = rig.pass();
        assert!(!has_write(&tick_pass), "the write comes on the next pass");
        let write_pass = rig.pass();
        assert!(has_write(&write_pass), "no Write after the role's frame");
        assert_eq!(rig.app.handler.counters().dropped_send_disconnected, 0);
    }

    /// While the link is lost the role gets `unregistered()`, so its tick builds no Search; after
    /// the new session it is registered again and searches. A role that missed `unregistered()`
    /// would build frames for a dead session, and the transport would count them dropped.
    #[test]
    fn a_reconnect_tells_the_role_unregistered_then_registered() {
        let mut rig = Rig::new();
        rig.register(AL_MAC);

        rig.lose_link();
        assert_eq!(rig.app.told, linked(1, None));
        rig.advance(PROTO_IDLE);
        rig.pass();
        rig.pass();
        assert_eq!(rig.app.handler.counters().dropped_send_disconnected, 0);

        rig.reconnect(AL_MAC);
        assert_eq!(rig.app.told, linked(2, Some(AL_MAC)));
        assert!(rig.app.fatal.is_none());
        let after_reconnect = rig.pass();
        assert!(has_write(&after_reconnect), "no Search on the new session");
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

    /// Planted-bug check: without the `role_is_active` guards the tick would run after the stop.
    #[test]
    fn the_role_gets_no_input_after_the_application_channel_closed() {
        let mut rig = Rig::new();
        rig.register(AL_MAC);
        let deadline_before_stop = rig.app.role.next_deadline();
        assert!(deadline_before_stop.is_some());

        rig.event(IoEvent::AppClosed);
        let stop_pass = rig.pass();
        assert!(rig.app.stopping);
        assert!(
            !stop_pass.contains(&IoAction::RecvApp),
            "a receive was armed after the stop"
        );

        // The tick of the role is due, but it does not run: its deadline is still there.
        rig.advance(PROTO_IDLE);
        let later = rig.pass();
        assert!(!has_write(&later));
        assert_eq!(rig.app.role.next_deadline(), deadline_before_stop);
    }

    /// A frame is in flight and its write is blocked when the stop comes: the transport writes
    /// for 900 ms, lingers for 100 ms and finishes, so the stop takes 1 s (0016 D3b).
    #[test]
    fn a_stop_with_a_frame_in_flight_finishes_within_a_second() {
        let mut rig = Rig::new();
        rig.register(AL_MAC);
        rig.advance(PROTO_IDLE);
        rig.pass();
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
