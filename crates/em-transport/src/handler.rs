// SPDX-License-Identifier: Apache-2.0

//! `TransportHandler`: composes [`TransportCore`] into an `em-event-loop` application (0016 D2,
//! D3b; step 4b-3).
//!
//! Synchronous, no tokio (0016 D1). A role crate owns one handler, gives it the ids of the loop
//! sources and the timer it serves ([`TransportSources`]), passes every [`TransportEvent`] its `Io`
//! reports to [`TransportHandler::on_event`], and passes every loop turn to
//! [`TransportHandler::serve`]. What the `Io` must do comes out as [`TransportIoAction`]s.
//!
//! # Sources
//!
//! | Source | Ready when | One turn |
//! |---|---|---|
//! | `read` | a frame is in the splitter, a read ended, or a read may be armed | handles one frame, or the end of the stream; arms the next read |
//! | `registration` | a registration result is stored | gives it to the core |
//! | `write` | a write failed, or no write is in flight and the core has a chunk | emits one `Write`, or handles the failure |
//!
//! The timer handler runs [`TransportCore::tick`].
//!
//! # Reads are armed
//!
//! The `Io` reads the data socket only after a [`TransportIoAction::Read`], once per action. The
//! handler emits `Read` when the core is in a session, the splitter holds no frame and no read is
//! armed, so the splitter never holds more than one read plus one partial frame (the rule of
//! [`TransportCore::bytes_read`]).
//!
//! # Writes
//!
//! One [`TransportIoAction::Write`] is in flight at a time. It carries a copy of the core's chunk
//! (at most one frame). The `Io` writes it as soon as the pass ends, so the result is reported to
//! the core with the time of the pass that emitted the `Write` (the core's `wrote(now, ..)` rule:
//! a result is judged at the time the write was performed). Until the result arrives the core
//! keeps the SDU of the chunk (see [`TransportCore::write_issued`]), so a `send` in the same pass
//! cannot evict it.
//!
//! # Closing a session
//!
//! On `CloseSession` the handler forgets the armed read, the write in flight and every stored
//! socket event; the `Io` drops the events it has not delivered yet.

use std::io;
use std::time::Instant;

use em_event_loop::{HandlerId, LoopCore, SourceId, Turn};

use crate::frame_splitter::Frame;
use crate::transport_core::{
    RegistrationStep, TransportAction, TransportCore, TransportOutcome, TransportPolicy,
};
use crate::{LinkState, OutboundSdu, Sdu, TransportCounters};

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "tests build inputs with the real clock; 0016 U27 allows test modules"
)]
mod tests;

/// The loop sources and the timer a [`TransportHandler`] serves. The role picks the ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransportSources {
    /// The data-socket read (budget: 64 frames per pass, 0016 D3b).
    pub read: SourceId,
    /// The registration verdict (budget 1).
    pub registration: SourceId,
    /// The data-socket write (budget 1: one `Write` per pass).
    pub write: SourceId,
    /// The transport timers.
    pub timer: HandlerId,
}

/// What the `Io` reports about the sockets.
#[derive(Debug, PartialEq, Eq)]
pub enum TransportEvent {
    /// The registration response arrived on the control socket.
    Registered(Frame),
    /// A step of the registration attempt failed.
    RegistrationFailed {
        /// The step that failed.
        step: RegistrationStep,
        /// The kind of the I/O error; the `Io` keeps the error itself.
        kind: io::ErrorKind,
    },
    /// Octets arrived on the data socket, the answer to one `Read`.
    Bytes(Vec<u8>),
    /// The data socket reached end of file or a read failed.
    ReadEnded,
    /// The `Write` wrote this many octets.
    Wrote(usize),
    /// The `Write` wrote nothing: the socket would block.
    WriteBlocked,
    /// The socket can take bytes again after `WriteBlocked`.
    Writable,
    /// The `Write` failed.
    WriteFailed,
}

/// What the handler asks the `Io` to do.
#[derive(Debug, PartialEq, Eq)]
pub enum TransportIoAction {
    /// Start one registration attempt (see [`TransportAction::Connect`]).
    Connect,
    /// Close the sockets of the session or attempt, and drop undelivered socket events.
    CloseSession,
    /// Read the data socket once and report [`TransportEvent::Bytes`] or
    /// [`TransportEvent::ReadEnded`].
    Read,
    /// Write these octets to the data socket at once and report the result.
    Write(Vec<u8>),
}

/// A stored registration result, handled on the registration turn.
#[derive(Debug)]
enum RegistrationResult {
    Frame(Frame),
    Failed {
        step: RegistrationStep,
        kind: io::ErrorKind,
    },
}

/// The transport as one application of the event loop. See the module docs.
#[derive(Debug)]
pub struct TransportHandler {
    core: TransportCore,
    sources: TransportSources,
    registration: Option<RegistrationResult>,
    read_ended: bool,
    write_failed: bool,
    /// A `Read` was emitted and its result has not arrived.
    read_armed: bool,
    /// The time of the pass that emitted the `Write` that is in flight.
    write_issued_at: Option<Instant>,
    /// The time of the last pass this handler served, for the readiness of the write source.
    last_now: Instant,
    outcome: Option<TransportOutcome>,
}

impl TransportHandler {
    /// A handler whose core wants to connect at `now`. Sets the timer deadline in `loop_core`, so
    /// the first pass at or after `now` connects.
    #[must_use]
    pub fn new(
        policy: TransportPolicy,
        now: Instant,
        sources: TransportSources,
        loop_core: &mut LoopCore<Instant>,
    ) -> Self {
        let handler = Self {
            core: TransportCore::new(policy, now),
            sources,
            registration: None,
            read_ended: false,
            write_failed: false,
            read_armed: false,
            write_issued_at: None,
            last_now: now,
            outcome: None,
        };
        handler.refresh(loop_core);
        handler
    }

    // ----- the role's view -----

    /// The latest link state.
    #[must_use]
    pub fn link(&self) -> LinkState {
        self.core.link()
    }

    /// The counters.
    #[must_use]
    pub fn counters(&self) -> TransportCounters {
        self.core.counters()
    }

    /// How the transport ended, once it has.
    #[must_use]
    pub fn outcome(&self) -> Option<TransportOutcome> {
        self.outcome
    }

    /// True while the received queue holds an SDU for the role.
    #[must_use]
    pub fn received_ready(&self) -> bool {
        self.core.received_ready()
    }

    /// Hands the oldest received SDU to the role.
    pub fn take_received(&mut self) -> Option<Sdu> {
        self.core.take_received()
    }

    /// The role has an SDU to send (see [`TransportCore::send`]).
    pub fn send(&mut self, now: Instant, outbound: OutboundSdu, loop_core: &mut LoopCore<Instant>) {
        self.core.send(now, outbound);
        self.refresh(loop_core);
    }

    /// The role is gone (see [`TransportCore::stop`]).
    pub fn stop<A: From<TransportIoAction>>(
        &mut self,
        now: Instant,
        loop_core: &mut LoopCore<Instant>,
        actions: &mut Vec<A>,
    ) {
        let mut core_actions = Vec::new();
        self.core.stop(now, &mut core_actions);
        self.route(&core_actions, actions);
        self.refresh(loop_core);
    }

    // ----- the loop's view -----

    /// Takes what the `Io` reported. Needs no time: results that the core judges by time are
    /// stored and handled on their turn.
    pub fn on_event(&mut self, event: TransportEvent, loop_core: &mut LoopCore<Instant>) {
        match event {
            TransportEvent::Registered(frame) => {
                self.registration = Some(RegistrationResult::Frame(frame));
            }
            TransportEvent::RegistrationFailed { step, kind } => {
                self.registration = Some(RegistrationResult::Failed { step, kind });
            }
            TransportEvent::Bytes(bytes) => {
                self.read_armed = false;
                self.core.bytes_read(&bytes);
            }
            TransportEvent::ReadEnded => {
                self.read_armed = false;
                self.read_ended = true;
            }
            TransportEvent::Wrote(written) => {
                if let Some(issued_at) = self.write_issued_at.take() {
                    self.core.wrote(issued_at, written);
                }
            }
            TransportEvent::WriteBlocked => {
                self.write_issued_at = None;
                self.core.write_blocked();
            }
            TransportEvent::Writable => self.core.writable(),
            TransportEvent::WriteFailed => {
                self.write_issued_at = None;
                self.write_failed = true;
            }
        }
        self.refresh(loop_core);
    }

    /// Serves `turn` if it is one of the transport's. Returns false for any other turn.
    pub fn serve<A: From<TransportIoAction>>(
        &mut self,
        turn: Turn,
        now: Instant,
        loop_core: &mut LoopCore<Instant>,
        actions: &mut Vec<A>,
    ) -> bool {
        match turn {
            Turn::Serve(source) if source == self.sources.read => {
                self.last_now = now;
                self.serve_read(now, actions);
            }
            Turn::Serve(source) if source == self.sources.registration => {
                self.last_now = now;
                self.serve_registration(now, actions);
            }
            Turn::Serve(source) if source == self.sources.write => {
                self.last_now = now;
                self.serve_write(now, actions);
            }
            Turn::Deadline(handler) if handler == self.sources.timer => {
                self.last_now = now;
                let mut core_actions = Vec::new();
                self.core.tick(now, &mut core_actions);
                self.route(&core_actions, actions);
            }
            Turn::Serve(_) | Turn::Deadline(_) | Turn::PassDone => return false,
        }
        self.refresh(loop_core);
        true
    }

    fn serve_read<A: From<TransportIoAction>>(&mut self, now: Instant, actions: &mut Vec<A>) {
        if self.read_ended {
            self.read_ended = false;
            let mut core_actions = Vec::new();
            self.core.read_ended(now, &mut core_actions);
            self.route(&core_actions, actions);
            return;
        }
        self.core.serve_read(now);
        if self.read_wanted() {
            self.read_armed = true;
            actions.push(TransportIoAction::Read.into());
        }
    }

    fn serve_registration<A: From<TransportIoAction>>(
        &mut self,
        now: Instant,
        actions: &mut Vec<A>,
    ) {
        let Some(result) = self.registration.take() else {
            return;
        };
        let mut core_actions = Vec::new();
        match result {
            RegistrationResult::Frame(frame) => {
                self.core.registration_frame(now, frame, &mut core_actions);
            }
            RegistrationResult::Failed { step, kind } => {
                self.core
                    .registration_failed(now, step, kind, &mut core_actions);
            }
        }
        self.route(&core_actions, actions);
    }

    fn serve_write<A: From<TransportIoAction>>(&mut self, now: Instant, actions: &mut Vec<A>) {
        if self.write_failed {
            self.write_failed = false;
            let mut core_actions = Vec::new();
            self.core.write_failed(now, &mut core_actions);
            self.route(&core_actions, actions);
            return;
        }
        if self.write_issued_at.is_some() || !self.core.write_ready(now) {
            return;
        }
        // The chunk is copied: the `Io` writes it after the pass, when the core may have moved on.
        let Some(chunk) = self.core.write_chunk(now).map(<[u8]>::to_vec) else {
            return;
        };
        self.core.write_issued();
        self.write_issued_at = Some(now);
        actions.push(TransportIoAction::Write(chunk).into());
    }

    /// True when a read may be armed now.
    fn read_wanted(&self) -> bool {
        self.core.wants_bytes() && !self.read_armed
    }

    /// Turns the core's actions into `Io` actions and keeps the outcome.
    fn route<A: From<TransportIoAction>>(
        &mut self,
        core_actions: &[TransportAction],
        actions: &mut Vec<A>,
    ) {
        for action in core_actions {
            match *action {
                TransportAction::Connect => actions.push(TransportIoAction::Connect.into()),
                TransportAction::CloseSession => {
                    self.forget_session_state();
                    actions.push(TransportIoAction::CloseSession.into());
                }
                TransportAction::Finish(outcome) => self.outcome = Some(outcome),
            }
        }
    }

    /// The sockets are closed: what was armed or stored for them is void.
    fn forget_session_state(&mut self) {
        self.registration = None;
        self.read_ended = false;
        self.write_failed = false;
        self.read_armed = false;
        self.write_issued_at = None;
    }

    /// Sets the timer deadline and the ready flags from the core's state.
    fn refresh(&self, loop_core: &mut LoopCore<Instant>) {
        loop_core.set_deadline(self.sources.timer, self.core.next_deadline());
        loop_core.set_ready(
            self.sources.read,
            self.core.read_ready() || self.read_ended || self.read_wanted(),
        );
        loop_core.set_ready(self.sources.registration, self.registration.is_some());
        loop_core.set_ready(
            self.sources.write,
            self.write_failed
                || (self.write_issued_at.is_none() && self.core.write_ready(self.last_now)),
        );
    }
}
