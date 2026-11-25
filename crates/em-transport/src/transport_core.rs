// SPDX-License-Identifier: Apache-2.0

//! `TransportCore`: every decision of the transport as a synchronous state machine (0016 D4).
//!
//! No tokio, no clock, no I/O (0016 D1). The caller passes the current time in, reports what
//! happened on the sockets, and performs the [`TransportAction`]s the core returns. The
//! `handler` module (`TransportHandler`) and the tokio `Io` (`tokio_io`) drive this core on the
//! event loop (0016 D8 step 4b-3). The old actor was deleted in step 5b; the citations of
//! `actor.rs` below name the file at `b6995f2`.
//!
//! # State machine
//!
//! ```text
//!                  tick: reconnect due              registration_frame: success
//!   Disconnected ------------------------> Registering ----------------------------> InSession
//!        ^   (Connect)                       |      |                                    |
//!        |                                   |      +--- fatal verdict ---> Finished     |
//!        +----- retry: CloseSession, wait <--+                                           |
//!        +----- write error, end of stream, write stall: CloseSession, wait <------------+
//!
//!   InSession --stop--> StopWriting --buffer empty, limit, write error--> StopLingering
//!   StopLingering --linger over: CloseSession, Finish(Stopped)--> Finished
//!   Disconnected / Registering --stop--> Finished (at once, no linger)
//! ```
//!
//! `new` starts in `Disconnected` with the reconnect due at the construction time, so the first
//! `tick` returns `Connect`.
//!
//! **One counting rule for every session end** (0016 D3b): the outbound buffer counts a partly
//! written SDU as `dropped_send_failed` and every SDU not yet started as
//! `dropped_send_disconnected`; an unfinished fragment set counts in `dropped_receive_fragments`;
//! the frame splitter starts again. A session that ends by itself also clears `link.al_mac`,
//! emits `CloseSession` and waits `reconnect_delay`. A stop ends in the same way, but `al_mac` is
//! cleared and the sockets are closed after the linger (as in `actor.rs` `run_session` at `b6995f2`).
//!
//! **Deadlines that do not fit.** A deadline is `now + duration`. When that sum cannot be
//! represented as an `Instant` (for example a policy duration of `Duration::MAX`), the deadline
//! is "never": [`TransportCore::next_deadline`] returns `None` for it and nothing panics.
//!
//! **Sockets and actions.** `Connect` opens an attempt. Every attempt or session ends with one
//! `CloseSession` before the next `Connect` and before `Finish`, so the caller never has open
//! sockets at `Finish`.

use std::io;
use std::time::{Duration, Instant};

use em_types::MacAddr;

use crate::frame_splitter::{Frame, FrameSplitter};
use crate::outbound_buffer::OutboundBuffer;
use crate::policy;
use crate::received_queue::ReceivedQueue;
use crate::wire::{
    self, RESULT_SUCCESS, Reassembler, ReassemblyStep, decode_registration_response,
    decode_sdu_frame,
};
use crate::{LinkState, OutboundSdu, Sdu, TransportCounters, TransportError};

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "tests build inputs with the real clock; 0016 U27 allows test modules"
)]
mod tests;

/// Every number the core uses (0016 U15). The core holds none of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransportPolicy {
    /// Wait before each new attempt to register, and after a session ends.
    pub reconnect_delay: Duration,
    /// Most SDUs in the received queue (0016 U1).
    pub rx_queue_sdus: usize,
    /// Most octets of CMDU in the received queue (0016 U1).
    pub rx_queue_bytes: usize,
    /// Largest joined payload of a fragmented set (0016 U2, U6).
    pub rx_sdu_cap: usize,
    /// Most octets in the outbound buffer (0016 U12).
    pub tx_buffer_bytes: usize,
    /// Queued outbound bytes that make no progress for this long end the session (0016 U4).
    pub write_stall: Duration,
    /// How long a stop writes the queued outbound bytes (0016 U8).
    pub stop_write_limit: Duration,
    /// How long a stop keeps the sockets open after the writes (0016 U3).
    pub linger: Duration,
}

impl TransportPolicy {
    /// The values the agent and the controller use: 2 s, 1024 SDUs, 4 MiB, 1 MiB, 4 MiB, 5 s,
    /// 900 ms and 100 ms (0016 D3b, from `policy.rs`).
    #[must_use]
    pub fn easymesh() -> Self {
        Self {
            reconnect_delay: policy::RECONNECT_DELAY,
            rx_queue_sdus: policy::RX_QUEUE_SDUS,
            rx_queue_bytes: policy::RX_QUEUE_BYTES,
            rx_sdu_cap: policy::RX_SDU_CAP,
            tx_buffer_bytes: policy::TX_BUFFER_BYTES,
            write_stall: policy::WRITE_STALL,
            stop_write_limit: policy::STOP_WRITE_LIMIT,
            linger: policy::LINGER,
        }
    }
}

/// What the caller must do after a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportAction {
    /// Start one registration attempt: connect the control socket, connect the data socket, send
    /// the registration request and read one framed response. Report the end with
    /// [`TransportCore::registration_failed`] or [`TransportCore::registration_frame`].
    Connect,
    /// Close the sockets of the current session or attempt.
    CloseSession,
    /// The core has ended. Every later input is ignored.
    Finish(TransportOutcome),
}

/// Why the core finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportOutcome {
    /// A stop was handled.
    Stopped,
    /// The service answered the registration with a `result` other than success. The caller
    /// builds [`TransportError::RegistrationRejected`].
    Rejected {
        /// The `result` octet of the response.
        result: u8,
    },
    /// A socket error that a reconnect cannot fix. The caller keeps the original `io::Error` and
    /// builds [`TransportError::Io`] from it, so the message and `source()` stay as they were.
    IoFailed {
        /// What the transport was doing.
        operation: &'static str,
    },
}

/// The step of a registration attempt that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationStep {
    /// Connecting the control socket.
    ConnectControl,
    /// Connecting the data socket.
    ConnectData,
    /// Sending the registration request.
    SendRequest,
    /// Reading the registration response.
    ReadResponse,
}

impl RegistrationStep {
    /// The operation name `TransportError::Io` carries (as `actor.rs` `register` at `b6995f2` names them).
    fn operation(self) -> &'static str {
        match self {
            Self::ConnectControl => "connect control socket",
            Self::ConnectData => "connect data socket",
            Self::SendRequest => "send registration request",
            Self::ReadResponse => "read registration response",
        }
    }

    /// True for the error kinds that mean "try again" (`actor.rs` `classify_connect` at `b6995f2` for the
    /// two connects, `classify_registration_io` for the request and the response). Any other
    /// kind is fatal.
    fn retries_on(self, kind: io::ErrorKind) -> bool {
        match self {
            Self::ConnectControl | Self::ConnectData => matches!(
                kind,
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ),
            Self::SendRequest | Self::ReadResponse => matches!(
                kind,
                io::ErrorKind::UnexpectedEof
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::ConnectionAborted
                    | io::ErrorKind::BrokenPipe
            ),
        }
    }
}

/// True when a frame body of `length` octets is longer than the wire allows.
fn is_over_frame_limit(length: usize) -> bool {
    u32::try_from(length).map_or(true, |length| length > wire::MAX_FRAME_LEN)
}

#[derive(Debug, PartialEq, Eq)]
enum State {
    /// No socket. The next `Connect` is due at `reconnect_at`; `None` is never.
    Disconnected { reconnect_at: Option<Instant> },
    /// `Connect` was returned and the attempt has not ended.
    Registering,
    /// Registered: reading and writing.
    InSession,
    /// Stopping in a session: writing the queued bytes until `limit_at` (`None` is never). Not
    /// reading.
    StopWriting { limit_at: Option<Instant> },
    /// Stopping in a session, writes over: the sockets stay open until `until` (`None` is never).
    StopLingering { until: Option<Instant> },
    /// `Finish` was returned.
    Finished,
}

/// The transport's decisions, with the data they act on (0016 D4). See the module docs.
#[derive(Debug)]
pub struct TransportCore {
    policy: TransportPolicy,
    state: State,
    link: LinkState,
    counters: TransportCounters,
    splitter: FrameSplitter,
    reassembler: Reassembler,
    received: ReceivedQueue,
    outbound: OutboundBuffer,
    /// False from `write_blocked` until `writable`: the socket is known not to take bytes.
    socket_writable: bool,
    /// When the queued outbound bytes last made progress, or became queued. `None` while the
    /// buffer is empty or not in a session.
    stall_since: Option<Instant>,
}

impl TransportCore {
    /// A core that is disconnected and wants to connect at once: [`TransportCore::next_deadline`]
    /// is `now`, so the first [`TransportCore::tick`] returns `Connect`.
    #[must_use]
    pub fn new(policy: TransportPolicy, now: Instant) -> Self {
        Self {
            policy,
            state: State::Disconnected {
                reconnect_at: Some(now),
            },
            link: LinkState::default(),
            counters: TransportCounters::default(),
            splitter: FrameSplitter::new(),
            reassembler: Reassembler::new(policy.rx_sdu_cap),
            received: ReceivedQueue::new(policy.rx_queue_sdus, policy.rx_queue_bytes),
            outbound: OutboundBuffer::new(policy.tx_buffer_bytes),
            socket_writable: true,
            stall_since: None,
        }
    }

    /// The latest link state.
    #[must_use]
    pub fn link(&self) -> LinkState {
        self.link
    }

    /// The counters.
    #[must_use]
    pub fn counters(&self) -> TransportCounters {
        self.counters
    }

    // ----- registration -----

    /// The registration response arrived as one frame (0011 D3 step 3; `actor.rs` `register` at `b6995f2`).
    ///
    /// Success starts a session. A frame that is oversize, too short, or carries an all-zero AL
    /// MAC ends the attempt and is retried after `reconnect_delay`. Any `result` other than
    /// success finishes the core with [`TransportOutcome::Rejected`]. Ignored unless an attempt is
    /// in progress.
    pub fn registration_frame(
        &mut self,
        now: Instant,
        frame: Frame,
        actions: &mut Vec<TransportAction>,
    ) {
        if self.state != State::Registering {
            return;
        }
        // The old actor's `FrameReader` (`b6995f2`) never yields a body above the limit, so neither does the core
        // accept one.
        let body = match frame {
            Frame::Complete(body) if !is_over_frame_limit(body.len()) => body,
            Frame::Complete(_) | Frame::Oversize { .. } => {
                self.retry_registration(now, "registration response too long", actions);
                return;
            }
        };
        let Some(response) = decode_registration_response(&body) else {
            self.retry_registration(now, "registration response too short", actions);
            return;
        };
        if response.result != RESULT_SUCCESS {
            let error = TransportError::RegistrationRejected {
                result: response.result,
            };
            tracing::error!("registration failed: {error}");
            self.finish_fatal(
                TransportOutcome::Rejected {
                    result: response.result,
                },
                actions,
            );
            return;
        }
        if response.al_mac.is_zero() {
            // The service sends all zeros when it cannot find its interface (`al_sap.rs:277-282`).
            tracing::warn!("service answered with an all-zero AL MAC, will retry");
            self.retry_registration(now, "all-zero AL MAC", actions);
            return;
        }
        self.start_session(response.al_mac);
    }

    /// A step of the registration attempt failed with an I/O error of `kind` (`actor.rs`
    /// `classify_connect` and `classify_registration_io` at `b6995f2`). A missing or refusing socket, and a
    /// service that closed or reset before the response, are retried after `reconnect_delay`;
    /// any other kind finishes the core with [`TransportOutcome::IoFailed`]. Ignored unless an
    /// attempt is in progress.
    pub fn registration_failed(
        &mut self,
        now: Instant,
        step: RegistrationStep,
        kind: io::ErrorKind,
        actions: &mut Vec<TransportAction>,
    ) {
        if self.state != State::Registering {
            return;
        }
        if step.retries_on(kind) {
            self.retry_registration(now, step.operation(), actions);
            return;
        }
        // The core has the kind only; the caller holds the whole error and its message.
        tracing::error!("registration failed: {}: {kind}", step.operation());
        self.finish_fatal(
            TransportOutcome::IoFailed {
                operation: step.operation(),
            },
            actions,
        );
    }

    fn retry_registration(
        &mut self,
        now: Instant,
        reason: &str,
        actions: &mut Vec<TransportAction>,
    ) {
        tracing::debug!("registration did not complete, will retry: {reason}");
        actions.push(TransportAction::CloseSession);
        self.state = State::Disconnected {
            reconnect_at: now.checked_add(self.policy.reconnect_delay),
        };
    }

    /// The same order and meaning as `actor.rs` `run_session` at `b6995f2`: counted, then the link shows the
    /// new session and AL MAC before any frame of it is read.
    fn start_session(&mut self, al_mac: MacAddr) {
        tracing::info!("registered, local AL MAC {al_mac}");
        self.counters.registrations += 1;
        self.link = LinkState {
            session: self.link.session + 1,
            al_mac: Some(al_mac),
        };
        self.socket_writable = true;
        self.stall_since = None;
        self.state = State::InSession;
    }

    /// A fatal registration verdict: like every return of the old actor (`actor.rs` `run` at `b6995f2`), it counts the received
    /// SDUs nobody will take (0011 A6).
    fn finish_fatal(&mut self, outcome: TransportOutcome, actions: &mut Vec<TransportAction>) {
        self.drop_received_for_stop();
        actions.push(TransportAction::CloseSession);
        actions.push(TransportAction::Finish(outcome));
        self.state = State::Finished;
    }

    // ----- reading -----

    /// Octets arrived on the data socket. Ignored unless in a session that is not stopping.
    ///
    /// The caller must not read more octets from the socket while [`TransportCore::read_ready`]
    /// is true: the driver reads between passes, and frames left in the splitter keep the read
    /// source ready (0016 D3b). That bounds the splitter to one read plus one partial frame.
    pub fn bytes_read(&mut self, bytes: &[u8]) {
        if self.state == State::InSession {
            self.splitter.push_bytes(bytes);
        }
    }

    /// True while in a session that is not stopping and the splitter holds a frame: frames left
    /// in the splitter keep the read source ready (0016 D3b). The caller must not read more
    /// octets from the socket, or call [`TransportCore::bytes_read`], while this is true.
    #[must_use]
    pub fn read_ready(&self) -> bool {
        self.state == State::InSession && self.splitter.has_frame()
    }

    /// True while in a session that is not stopping and the splitter holds no complete frame: the
    /// caller may read more octets from the socket (a partial frame may be waiting for the rest).
    /// The opposite of the read condition in [`TransportCore::read_ready`], within a session.
    #[must_use]
    pub fn wants_bytes(&self) -> bool {
        self.state == State::InSession && !self.splitter.has_frame()
    }

    /// Handles one frame, with the logic and counters of `actor.rs` `handle_frame` at `b6995f2`. Receive
    /// errors drop the frame and keep the session. `_now` is there for the same shape as the
    /// other handlers; reading needs no time.
    pub fn serve_read(&mut self, _now: Instant) {
        if !self.read_ready() {
            return;
        }
        self.handle_next_frame();
    }

    /// Takes the oldest frame from the splitter and handles it.
    fn handle_next_frame(&mut self) {
        let Some(frame) = self.splitter.next_frame() else {
            return;
        };
        let body = match frame {
            Frame::Complete(body) => body,
            Frame::Oversize { length } => {
                tracing::warn!(
                    "frame of {length} octets skipped: above {}",
                    wire::MAX_FRAME_LEN
                );
                self.counters.dropped_receive_oversize += 1;
                return;
            }
        };
        let Some(sdu_frame) = decode_sdu_frame(&body) else {
            tracing::warn!(
                "frame of {} octets dropped: shorter than an SDU header",
                body.len()
            );
            self.counters.dropped_receive_malformed += 1;
            return;
        };

        let ReassemblyStep {
            sdu,
            dropped,
            too_large,
        } = self.reassembler.push(sdu_frame);
        if dropped > 0 {
            tracing::warn!(
                "{dropped} fragment(s) dropped: out of order, address mismatch or incomplete set"
            );
            self.counters.dropped_receive_fragments += dropped;
        }
        if too_large {
            tracing::warn!(
                "fragment set refused: joined payload would pass {} octets",
                self.policy.rx_sdu_cap
            );
            self.counters.dropped_receive_too_large += 1;
        }
        let Some(sdu) = sdu else {
            return;
        };
        self.counters.sdus_received += 1;
        let overflowed = self.received.push(sdu);
        if overflowed > 0 {
            tracing::warn!("received queue full: oldest received SDU dropped");
            self.counters.dropped_receive_overflow += overflowed;
        }
    }

    /// The data socket reached end of file or failed on a read. Handles every complete frame
    /// already in the splitter, as [`TransportCore::serve_read`] would (the old `FrameReader`
    /// delivered buffered frames before the end), then ends the session; the caller logs the
    /// error. Ignored when not in a session or when stopping (the socket is no longer read).
    pub fn read_ended(&mut self, now: Instant, actions: &mut Vec<TransportAction>) {
        if self.state != State::InSession {
            return;
        }
        while self.splitter.has_frame() {
            self.handle_next_frame();
        }
        tracing::info!("data socket ended");
        self.end_session(now, actions);
    }

    /// True while the received queue holds an SDU for the role.
    #[must_use]
    pub fn received_ready(&self) -> bool {
        !self.received.is_empty()
    }

    /// Hands the oldest received SDU to the role. After a stop the queue is empty.
    pub fn take_received(&mut self) -> Option<Sdu> {
        self.received.pop()
    }

    // ----- writing -----

    /// The role has an SDU to send. It is written only into the session it was built for
    /// (0011 A6); in any other case, or after a stop, it is dropped and counted.
    // The role hands the SDU over; the buffer keeps only its encoded frames.
    #[allow(clippy::needless_pass_by_value)]
    pub fn send(&mut self, now: Instant, outbound: OutboundSdu) {
        match self.state {
            State::InSession if outbound.session == self.link.session => {}
            State::InSession => {
                tracing::debug!(
                    "SDU dropped: built for session {}, this is session {}",
                    outbound.session,
                    self.link.session
                );
                self.counters.dropped_send_disconnected += 1;
                return;
            }
            State::StopWriting { .. } | State::StopLingering { .. } | State::Finished => {
                tracing::debug!("SDU dropped: the transport is stopping");
                self.counters.dropped_send_disconnected += 1;
                return;
            }
            State::Disconnected { .. } | State::Registering => {
                tracing::debug!("SDU dropped: not written");
                self.counters.dropped_send_disconnected += 1;
                return;
            }
        }

        let outcome = self.outbound.push(&outbound.sdu);
        if outcome.send_failed > 0 {
            tracing::error!(
                "SDU of {} octets needs more than 256 fragments, dropped",
                outbound.sdu.cmdu.len()
            );
            self.counters.dropped_send_failed += outcome.send_failed;
        }
        if outcome.overflow > 0 {
            tracing::warn!("outbound buffer full: {} SDU(s) dropped", outcome.overflow);
            self.counters.dropped_send_overflow += outcome.overflow;
        }
        // The clock starts when the buffer becomes non-empty; a later push does not restart it.
        if self.stall_since.is_none() && !self.outbound.is_empty() {
            self.stall_since = Some(now);
        }
    }

    fn may_write(&self) -> bool {
        matches!(self.state, State::InSession | State::StopWriting { .. })
    }

    /// True when a deadline of the writing states is due at `now`: the stall deadline in a
    /// session, the stop write limit while stopping. The event loop serves sources before it ticks
    /// the deadlines, so a write that starts at or after a due deadline would be written first and
    /// judged by the deadline afterwards.
    fn write_deadline_due(&self, now: Instant) -> bool {
        self.may_write() && self.next_deadline().is_some_and(|deadline| deadline <= now)
    }

    /// The octets that are queued for writing, whatever the time.
    fn queued_chunk(&self) -> Option<&[u8]> {
        if !self.may_write() {
            return None;
        }
        self.outbound.next_chunk()
    }

    /// True while writing is allowed (in a session, or stopping within the write window), the
    /// socket is writable, and the buffer has a chunk. It is false while the socket is known to
    /// be blocked, so the write source does not spin (0016 E10). It is also false when the stall
    /// deadline or the stop write limit is due at `now`: the deadline wins over a write at the
    /// same `now`.
    #[must_use]
    pub fn write_ready(&self, now: Instant) -> bool {
        self.socket_writable && self.write_chunk(now).is_some()
    }

    /// The octets of the oldest queued SDU that are not written yet. Write some of them, then
    /// call [`TransportCore::wrote`]. `None` when a deadline is due at `now` (see
    /// [`TransportCore::write_ready`]), so a write never starts at or after a due deadline.
    #[must_use]
    pub fn write_chunk(&self, now: Instant) -> Option<&[u8]> {
        if self.write_deadline_due(now) {
            return None;
        }
        self.queued_chunk()
    }

    /// The caller took [`TransportCore::write_chunk`] and hands it to the `Io`, which writes it
    /// later. Until [`TransportCore::wrote`], [`TransportCore::write_blocked`] or a session end,
    /// the SDU of the chunk is kept by `send` as if it were started: the result applies to the
    /// oldest SDU, so a `send` in between must not evict it. A session end still counts that SDU
    /// as not started unless octets of it were reported written.
    pub fn write_issued(&mut self) {
        self.outbound.mark_write_in_flight();
    }

    /// `written` octets of [`TransportCore::write_chunk`] left. Counts `sdus_sent` when an SDU
    /// completes and resets the stall clock. Ignored when written is 0 or there is no chunk.
    ///
    /// Report the result with the time the write was performed. The write was issued at a `now`
    /// before every deadline (`write_chunk` refuses otherwise), so the chunk check here does not
    /// look at deadlines. A result reported at or after the stop write limit is too late and is
    /// not accepted.
    ///
    /// # Panics
    ///
    /// If `written` is more than the length of the chunk.
    pub fn wrote(&mut self, now: Instant, written: usize) {
        self.outbound.write_finished();
        if written == 0 || self.queued_chunk().is_none() {
            return;
        }
        if self.stop_limit_reached(now) {
            return;
        }
        let outcome = self.outbound.advance(written);
        self.counters.sdus_sent += outcome.sdus_sent;
        self.stall_since = if self.outbound.is_empty() {
            None
        } else {
            Some(now)
        };
        let finished_stop_writes =
            matches!(self.state, State::StopWriting { .. }) && self.outbound.is_empty();
        if finished_stop_writes {
            // The limit is not reached (checked above), so `now` is before it.
            self.start_linger(now);
        }
    }

    /// The socket would block, so the write in flight wrote nothing: `write_ready` stays false
    /// until [`TransportCore::writable`].
    pub fn write_blocked(&mut self) {
        self.outbound.write_finished();
        self.socket_writable = false;
    }

    /// The socket can take bytes again.
    pub fn writable(&mut self) {
        self.socket_writable = true;
    }

    /// A write on the data socket failed. In a session the session ends and the transport
    /// reconnects; while stopping the writes end and the linger starts (0011 A7: nothing more is
    /// written, the stream may be out of step). The caller logs the error.
    pub fn write_failed(&mut self, now: Instant, actions: &mut Vec<TransportAction>) {
        match self.state {
            State::InSession => {
                tracing::warn!("write to the data socket failed, ending the session");
                self.end_session(now, actions);
            }
            State::StopWriting { .. } => {
                tracing::warn!("write to the data socket failed while stopping");
                if !self.stop_limit_reached(now) {
                    self.start_linger(now);
                }
            }
            State::Disconnected { .. }
            | State::Registering
            | State::StopLingering { .. }
            | State::Finished => {}
        }
    }

    // ----- session end -----

    /// The counting rule every session end shares (0016 D3b).
    fn count_session_end(&mut self) {
        let ended = self.outbound.end_session();
        self.counters.dropped_send_failed += ended.send_failed;
        self.counters.dropped_send_disconnected += ended.send_disconnected;
        self.counters.dropped_receive_fragments += self.reassembler.abandon_set();
        self.splitter = FrameSplitter::new();
        self.stall_since = None;
    }

    /// A session that ends while the role is still there: count, close, wait, reconnect.
    fn end_session(&mut self, now: Instant, actions: &mut Vec<TransportAction>) {
        self.count_session_end();
        self.link.al_mac = None;
        actions.push(TransportAction::CloseSession);
        self.state = State::Disconnected {
            reconnect_at: now.checked_add(self.policy.reconnect_delay),
        };
    }

    /// The stop's writes are over (buffer empty, limit reached or a write failed): count what is
    /// left, keep the sockets open for the linger that starts at `started_at`.
    fn start_linger(&mut self, started_at: Instant) {
        self.count_session_end();
        self.state = State::StopLingering {
            until: started_at.checked_add(self.policy.linger),
        };
    }

    /// While stopping, a write result or failure at or after the limit comes too late: the limit
    /// counts as reached first, so nothing is written past it and the linger runs from the limit.
    /// Returns true when it did that.
    fn stop_limit_reached(&mut self, now: Instant) -> bool {
        let State::StopWriting {
            limit_at: Some(limit_at),
        } = self.state
        else {
            return false;
        };
        if now < limit_at {
            return false;
        }
        self.start_linger(limit_at);
        true
    }

    // ----- stop -----

    /// The role is gone (0016 U3, U8). The role gets no more input: the received queue is
    /// counted in `dropped_receive_stopped` at once and later sends are counted in
    /// `dropped_send_disconnected`.
    ///
    /// In a session the queued outbound bytes are written for at most `stop_write_limit`, the rest
    /// is counted by the rule above, the sockets stay open for `linger`, then `CloseSession` and
    /// `Finish(Stopped)`. Without a session it finishes at once, closing an attempt in progress.
    pub fn stop(&mut self, now: Instant, actions: &mut Vec<TransportAction>) {
        match self.state {
            State::InSession => {
                self.drop_received_for_stop();
                if self.outbound.is_empty() {
                    self.start_linger(now);
                } else {
                    self.stall_since = None;
                    self.state = State::StopWriting {
                        limit_at: now.checked_add(self.policy.stop_write_limit),
                    };
                }
            }
            State::Registering => {
                self.drop_received_for_stop();
                actions.push(TransportAction::CloseSession);
                actions.push(TransportAction::Finish(TransportOutcome::Stopped));
                self.state = State::Finished;
            }
            State::Disconnected { .. } => {
                self.drop_received_for_stop();
                actions.push(TransportAction::Finish(TransportOutcome::Stopped));
                self.state = State::Finished;
            }
            State::StopWriting { .. } | State::StopLingering { .. } | State::Finished => {}
        }
    }

    fn drop_received_for_stop(&mut self) {
        self.counters.dropped_receive_stopped += self.received.clear();
    }

    // ----- time -----

    /// The earliest time something is due: the reconnect, the write stall, the stop write limit
    /// or the end of the linger. `None` when nothing waits.
    #[must_use]
    pub fn next_deadline(&self) -> Option<Instant> {
        match self.state {
            State::Disconnected { reconnect_at } => reconnect_at,
            State::InSession => self
                .stall_since
                .and_then(|since| since.checked_add(self.policy.write_stall)),
            State::StopWriting { limit_at } => limit_at,
            State::StopLingering { until } => until,
            State::Registering | State::Finished => None,
        }
    }

    /// Acts on every deadline that is due at `now`. Afterwards [`TransportCore::next_deadline`]
    /// is after `now` or `None`.
    pub fn tick(&mut self, now: Instant, actions: &mut Vec<TransportAction>) {
        while self.next_deadline().is_some_and(|deadline| deadline <= now) {
            match self.state {
                State::Disconnected { .. } => {
                    actions.push(TransportAction::Connect);
                    self.state = State::Registering;
                }
                State::InSession => {
                    tracing::warn!(
                        "no write progress for {:?}, ending the session",
                        self.policy.write_stall
                    );
                    self.end_session(now, actions);
                }
                // The linger runs from the limit, not from a late tick, so a stop ends by
                // stop_write_limit + linger after it began (0016 U8).
                State::StopWriting {
                    limit_at: Some(limit_at),
                } => self.start_linger(limit_at),
                State::StopLingering { .. } => {
                    self.link.al_mac = None;
                    actions.push(TransportAction::CloseSession);
                    actions.push(TransportAction::Finish(TransportOutcome::Stopped));
                    self.state = State::Finished;
                }
                // No deadline is due in these states (a "never" deadline is not returned).
                State::Registering | State::Finished | State::StopWriting { limit_at: None } => {
                    break;
                }
            }
        }
    }
}
