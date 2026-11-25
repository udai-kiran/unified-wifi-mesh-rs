// SPDX-License-Identifier: Apache-2.0

//! `TransportIo`: the tokio `Io` of the event loop for the transport (0016 D2, U19; step 4b-3).
//!
//! It does what the old actor did at the sockets and nothing more: connect, register, read
//! when told to, write when told to, receive from the application channel when told to. Every
//! decision belongs to [`TransportHandler`](crate::TransportHandler) and the core.
//!
//! * **Reads and receives are armed** by actions ([`TransportIoAction::Read`],
//!   [`IoAction::RecvApp`]) and happen once per action, so the splitter and the application input
//!   stay bounded by the handler's own rules.
//! * **Writes are synchronous.** `perform` calls `try_write` at once, so the result is reported
//!   for the pass that issued the write (the handler passes that time to the core).
//! * **Waiting** is one `select!` over the registration attempt, the writable and readable
//!   waits, the application receive and the deadline. Writable comes before readable: it is
//!   enabled only while a write is blocked and is cleared once stored, so it cannot starve
//!   reads, but an inbound flood keeps the read wait ready and, tried first, would keep a
//!   blocked write waiting until the 5 s write stall. What it finds is stored for the next
//!   `poll_events`, in the order it happened. **Yielding** (the driver's call while a source
//!   stays ready) is the same `select!` after one tokio yield, with the deadline replaced by
//!   "now", so the registration attempt and a blocked write advance even when the application
//!   channel never goes empty.
//! * **The registration attempt** is a future owned by the `Io`, not a task. It connects, writes
//!   the request and reads one frame; the control socket then stays open for the session.
//! * **Errors.** The `Io` keeps the `io::Error` of a failed registration step, so
//!   [`TransportIo::into_result`] builds [`TransportError::Io`] with the original message and
//!   `source()`.

use std::future::{Future, pending};
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::Instant;

use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::time::sleep_until;

use crate::frame_reader::FrameReader;
use crate::frame_splitter::Frame;
use crate::handler::{TransportEvent, TransportIoAction};
use crate::transport_core::{RegistrationStep, TransportOutcome};
use crate::wire::encode_registration_request;
use crate::{Role, TransportConfig, TransportError};

/// Octets asked from the data socket per armed read.
const READ_BUFFER_LEN: usize = 64 * 1024;

/// What [`TransportIo`] reports to the loop.
#[derive(Debug, PartialEq, Eq)]
pub enum IoEvent<E> {
    /// Something happened at the sockets.
    Transport(TransportEvent),
    /// The answer to one [`IoAction::RecvApp`].
    App(E),
    /// The application channel is closed and empty. Reported once.
    AppClosed,
}

/// What the loop asks [`TransportIo`] to do.
#[derive(Debug, PartialEq, Eq)]
pub enum IoAction {
    /// An action for the sockets.
    Transport(TransportIoAction),
    /// Receive one value from the application channel and report it as [`IoEvent::App`], or
    /// [`IoEvent::AppClosed`] if the channel is closed.
    RecvApp,
}

impl From<TransportIoAction> for IoAction {
    fn from(action: TransportIoAction) -> Self {
        Self::Transport(action)
    }
}

/// A finished registration attempt: the response and the two sockets.
struct Registered {
    frame: Frame,
    /// Never read again, never written again; closing it ends the registration on this side.
    control: FrameReader<UnixStream>,
    data: UnixStream,
}

struct RegistrationFailure {
    step: RegistrationStep,
    error: io::Error,
}

type Attempt = Pin<Box<dyn Future<Output = Result<Registered, RegistrationFailure>> + Send>>;

/// The sockets of a registered session, and what the `Io` waits for on them.
struct Session {
    data: UnixStream,
    _control: FrameReader<UnixStream>,
    read_armed: bool,
    /// A write returned `WouldBlock`: wait for the socket to become writable.
    write_blocked: bool,
}

/// What ended a [`TransportIo::poll_branches`].
enum Woken<E> {
    Attempt(Result<Registered, RegistrationFailure>),
    Readable(io::Result<()>),
    Writable(io::Result<()>),
    App(Option<E>),
    /// The `stop` future of `poll_branches` completed first.
    Stopped,
}

/// The tokio `Io` for the transport. `E` is what the application channel carries.
pub struct TransportIo<E> {
    control_path: PathBuf,
    data_path: PathBuf,
    role: Role,
    app_events: mpsc::Receiver<E>,
    app_armed: bool,
    app_closed: bool,
    attempt: Option<Attempt>,
    session: Option<Session>,
    /// Events found but not yet delivered, oldest first.
    pending: Vec<IoEvent<E>>,
    /// The error of the last failed registration step, for [`TransportIo::into_result`].
    registration_error: Option<io::Error>,
    read_buffer: Box<[u8]>,
}

impl<E> TransportIo<E> {
    /// An `Io` for the sockets of `config`, with `app_events` as the application channel. The
    /// reconnect delay of `config` is not used here: the core owns it (`TransportPolicy`).
    #[must_use]
    pub fn new(config: &TransportConfig, app_events: mpsc::Receiver<E>) -> Self {
        Self {
            control_path: config.control_path.as_path().to_path_buf(),
            data_path: config.data_path.as_path().to_path_buf(),
            role: config.role,
            app_events,
            app_armed: false,
            app_closed: false,
            attempt: None,
            session: None,
            pending: Vec::new(),
            registration_error: None,
            read_buffer: vec![0; READ_BUFFER_LEN].into_boxed_slice(),
        }
    }

    /// The result the old actor's task returned, from the handler's outcome: `Ok` for a stop,
    /// [`TransportError::RegistrationRejected`], or [`TransportError::Io`] with the `io::Error`
    /// the failed step produced.
    ///
    /// # Errors
    ///
    /// The error cases above: a rejected registration, or an I/O error that a reconnect cannot fix.
    ///
    /// # Panics
    ///
    /// If `outcome` is [`TransportOutcome::IoFailed`] and no registration step failed. The core
    /// reports that outcome only after a `RegistrationFailed` event, which this `Io` sends only
    /// together with its error.
    pub fn into_result(self, outcome: TransportOutcome) -> Result<(), TransportError> {
        match outcome {
            TransportOutcome::Stopped => Ok(()),
            TransportOutcome::Rejected { result } => {
                Err(TransportError::RegistrationRejected { result })
            }
            TransportOutcome::IoFailed { operation } => {
                let source = self
                    .registration_error
                    .expect("IoFailed follows a failed registration step");
                Err(TransportError::Io { operation, source })
            }
        }
    }

    fn start_attempt(&mut self) {
        let control_path = self.control_path.as_path().to_path_buf();
        let data_path = self.data_path.as_path().to_path_buf();
        self.attempt = Some(Box::pin(register(control_path, data_path, self.role)));
    }

    /// Drops the sockets, the attempt, every wait on them and every undelivered socket event.
    fn close_session(&mut self) {
        self.attempt = None;
        self.session = None;
        self.pending
            .retain(|event| !matches!(event, IoEvent::Transport(_)));
    }

    fn write(&mut self, bytes: &[u8]) {
        let Some(session) = &mut self.session else {
            return;
        };
        match session.data.try_write(bytes) {
            Ok(written) => self
                .pending
                .push(IoEvent::Transport(TransportEvent::Wrote(written))),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                session.write_blocked = true;
                self.pending
                    .push(IoEvent::Transport(TransportEvent::WriteBlocked));
            }
            Err(error) => {
                tracing::warn!("write to the data socket failed: {error}");
                self.pending
                    .push(IoEvent::Transport(TransportEvent::WriteFailed));
            }
        }
    }

    /// Reads once if a read is armed. `WouldBlock` leaves it armed.
    fn read(&mut self, events: &mut Vec<IoEvent<E>>) {
        let Some(session) = &mut self.session else {
            return;
        };
        if !session.read_armed {
            return;
        }
        match session.data.try_read(&mut self.read_buffer) {
            Ok(0) => {
                session.read_armed = false;
                events.push(IoEvent::Transport(TransportEvent::ReadEnded));
            }
            Ok(count) => {
                session.read_armed = false;
                let bytes = self.read_buffer[..count].to_vec();
                events.push(IoEvent::Transport(TransportEvent::Bytes(bytes)));
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) => {
                tracing::info!("data socket ended: {error}");
                session.read_armed = false;
                events.push(IoEvent::Transport(TransportEvent::ReadEnded));
            }
        }
    }

    /// Receives once if a receive is armed. `Empty` leaves it armed.
    fn receive_app(&mut self, events: &mut Vec<IoEvent<E>>) {
        if !self.app_armed {
            return;
        }
        match self.app_events.try_recv() {
            Ok(event) => {
                self.app_armed = false;
                events.push(IoEvent::App(event));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.app_armed = false;
                self.app_closed = true;
                events.push(IoEvent::AppClosed);
            }
        }
    }

    /// Waits for the first of the registration attempt, the writable and readable waits, the
    /// application receive, and `stop`, and stores what it found. The branches are tried in this
    /// order, so `stop` wins only when nothing else is ready (`ready(())` makes this a poll).
    ///
    /// Writable is before readable on purpose. Under a continuous inbound flood the armed read is
    /// ready at every poll, so a read wait tried first would always win and the writability a
    /// blocked write waits for would never be stored; the 5 s write stall would then end a healthy
    /// session. Writable cannot starve reads in turn: its wait is enabled only while a write is
    /// blocked, and storing it clears that. The application channel is also drained by `try_recv`
    /// in `poll_events`, so its place near the end does not starve it either.
    async fn poll_branches(&mut self, stop: impl Future<Output = ()>) {
        let woken = tokio::select! {
            biased;
            result = attempt_ended(&mut self.attempt) => Woken::Attempt(result),
            result = data_writable(self.session.as_ref()) => Woken::Writable(result),
            result = data_readable(self.session.as_ref()) => Woken::Readable(result),
            received = app_received(&mut self.app_events, self.app_armed) => Woken::App(received),
            () = stop => Woken::Stopped,
        };
        self.store(woken);
    }

    /// Stores what ended a wait, as the events `poll_events` will deliver.
    fn store(&mut self, woken: Woken<E>) {
        match woken {
            Woken::Attempt(Ok(registered)) => {
                self.attempt = None;
                let Registered {
                    frame,
                    control,
                    data,
                } = registered;
                self.session = Some(Session {
                    data,
                    _control: control,
                    read_armed: false,
                    write_blocked: false,
                });
                self.pending
                    .push(IoEvent::Transport(TransportEvent::Registered(frame)));
            }
            Woken::Attempt(Err(failure)) => {
                self.attempt = None;
                let RegistrationFailure { step, error } = failure;
                let kind = error.kind();
                self.registration_error = Some(error);
                self.pending
                    .push(IoEvent::Transport(TransportEvent::RegistrationFailed {
                        step,
                        kind,
                    }));
            }
            // The octets themselves are read by `poll_events`.
            Woken::Readable(Ok(())) | Woken::Stopped => {}
            Woken::Readable(Err(error)) => {
                tracing::info!("data socket ended: {error}");
                if let Some(session) = &mut self.session {
                    session.read_armed = false;
                }
                self.pending
                    .push(IoEvent::Transport(TransportEvent::ReadEnded));
            }
            Woken::Writable(result) => {
                if let Some(session) = &mut self.session {
                    session.write_blocked = false;
                }
                match result {
                    Ok(()) => self
                        .pending
                        .push(IoEvent::Transport(TransportEvent::Writable)),
                    Err(error) => {
                        tracing::warn!("write to the data socket failed: {error}");
                        self.pending
                            .push(IoEvent::Transport(TransportEvent::WriteFailed));
                    }
                }
            }
            Woken::App(Some(event)) => {
                self.app_armed = false;
                self.pending.push(IoEvent::App(event));
            }
            Woken::App(None) => {
                self.app_armed = false;
                self.app_closed = true;
                self.pending.push(IoEvent::AppClosed);
            }
        }
    }
}

impl<E: Send> em_event_loop::Io<Instant> for TransportIo<E> {
    type Event = IoEvent<E>;
    type Action = IoAction;

    fn now(&mut self) -> Instant {
        tokio::time::Instant::now().into_std()
    }

    async fn yield_now(&mut self) {
        // The driver calls this instead of `wait` while a source stays ready, so the registration
        // attempt and the blocked write must advance here too, or a busy application channel
        // would starve them. The branches are the ones of `wait`; none of them is waited for.
        tokio::task::yield_now().await;
        self.poll_branches(std::future::ready(())).await;
    }

    async fn wait(&mut self, deadline: Option<Instant>) {
        // The driver awaits only here, and only when no source is ready. A wait whose branch is
        // already ready returns without letting the runtime run, so a busy app channel could keep
        // the thread: other tasks and the I/O driver would not run, and `try_read` would see no
        // new readiness. One yield first lets the runtime poll its driver and other tasks.
        tokio::task::yield_now().await;
        self.poll_branches(deadline_reached(deadline)).await;
    }

    fn poll_events(&mut self, events: &mut Vec<IoEvent<E>>) {
        events.append(&mut self.pending);
        self.read(events);
        self.receive_app(events);
    }

    fn perform(&mut self, action: IoAction) {
        match action {
            IoAction::Transport(TransportIoAction::Connect) => self.start_attempt(),
            IoAction::Transport(TransportIoAction::CloseSession) => self.close_session(),
            IoAction::Transport(TransportIoAction::Read) => {
                if let Some(session) = &mut self.session {
                    session.read_armed = true;
                }
            }
            IoAction::Transport(TransportIoAction::Write(bytes)) => self.write(&bytes),
            IoAction::RecvApp => self.app_armed = !self.app_closed,
        }
    }
}

/// Connects control, then data, sends the request and reads one framed response
/// (0011 D3 steps 1 to 3; `actor.rs` `register`). The response is judged by the core.
async fn register(
    control_path: PathBuf,
    data_path: PathBuf,
    role: Role,
) -> Result<Registered, RegistrationFailure> {
    let mut control = connect(&control_path, RegistrationStep::ConnectControl).await?;
    let data = connect(&data_path, RegistrationStep::ConnectData).await?;

    control
        .write_all(&encode_registration_request(role))
        .await
        .map_err(|error| RegistrationFailure {
            step: RegistrationStep::SendRequest,
            error,
        })?;

    let mut control = FrameReader::new(control);
    let frame = control
        .next_frame()
        .await
        .map_err(|error| RegistrationFailure {
            step: RegistrationStep::ReadResponse,
            error,
        })?;
    Ok(Registered {
        frame,
        control,
        data,
    })
}

async fn connect(path: &Path, step: RegistrationStep) -> Result<UnixStream, RegistrationFailure> {
    UnixStream::connect(path)
        .await
        .map_err(|error| RegistrationFailure { step, error })
}

// The wait helpers below are `pending` when there is nothing to wait for, so that `select!` can
// list every branch without a precondition (it builds the future of a disabled branch too).

async fn attempt_ended(attempt: &mut Option<Attempt>) -> Result<Registered, RegistrationFailure> {
    match attempt {
        Some(future) => future.as_mut().await,
        None => pending().await,
    }
}

async fn data_readable(session: Option<&Session>) -> io::Result<()> {
    match session {
        Some(session) if session.read_armed => session.data.readable().await,
        _ => pending().await,
    }
}

async fn data_writable(session: Option<&Session>) -> io::Result<()> {
    match session {
        Some(session) if session.write_blocked => session.data.writable().await,
        _ => pending().await,
    }
}

async fn app_received<E>(events: &mut mpsc::Receiver<E>, armed: bool) -> Option<E> {
    if armed {
        events.recv().await
    } else {
        pending().await
    }
}

async fn deadline_reached(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => sleep_until(tokio::time::Instant::from_std(deadline)).await,
        None => pending().await,
    }
}
