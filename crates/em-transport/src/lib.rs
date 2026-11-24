// SPDX-License-Identifier: Apache-2.0

//! AL-SAP client: carries CMDUs to and from an external IEEE 1905 daemon over two UNIX sockets.
//!
//! Design: note 0011 (slice S0), D1 to D3. The client does what upstream's
//! `AlServiceAccessPoint` does (`src/al-sap/al_service_access_point.cpp`,
//! `inc/al_service_*.h` at `ba4180b`) against the service in `ieee1905-rs`
//! (`ieee1905-core/src/al_sap.rs` at `a6443225`), as one tokio task:
//!
//! 1. connect the control socket, then the data socket;
//! 2. send a registration request and read the response;
//! 3. exchange SDUs on the data socket, fragmenting and joining as needed;
//! 4. when the data socket closes or fails, publish a [`LinkState`] without an AL MAC,
//!    wait, and register again.
//!
//! The role actor sends [`OutboundSdu`]s: the SDU with the `session` of the [`LinkState`] it
//! was built under (note 0011 A6). The actor writes it only into that session.
//!
//! The role actor gets four things from [`spawn`] (note 0011 A5): a sender for SDUs, a
//! receiver that carries only received SDUs (a plain drop-oldest queue, counted), a
//! `watch` of the [`LinkState`], and a `watch` of the [`TransportCounters`]. A link change
//! is a latest value, not an event, and is not ordered against the received SDUs as the
//! role actor's `select!` sees them. At each registration the actor publishes the new
//! state before it reads any frame of the session, so when the role actor handles a
//! received SDU, `link.borrow()` shows that SDU's session or a later one.
//!
//! The payload of an [`Sdu`] is opaque here. **Contract for callers:** every SDU is
//! exactly one CMDU ending in End of Message. The service drops anything else
//! without telling the client (0011, "The AL-SAP protocol").
//!
//! `local` and `remote` are the only address names above this crate. In both
//! directions `local` is the local AL MAC and `remote` the other side's: on the wire,
//! MAC A and MAC B (0011, "Address fields").

mod actor;
mod frame_reader;
mod policy;
mod wire;

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use em_types::MacAddr;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

pub use wire::{MAX_FRAGMENT_PAYLOAD, MAX_SDU_PAYLOAD};

/// Which service type to register as (upstream `al_service_registration_enums.h:7-14`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// `EasyMesh` agent, service type 0x01.
    Agent,
    /// `EasyMesh` controller, service type 0x02.
    Controller,
}

/// What [`spawn`] needs to know.
#[derive(Debug)]
pub struct TransportConfig {
    /// Path of the control socket. Upstream's controller uses
    /// `/tmp/al_em_ctrl_control_socket` (`em_ctrl.cpp:1797-1798`), its agent
    /// `/tmp/al_control_socket` (`em_agent.h:34-35`).
    pub control_path: PathBuf,
    /// Path of the data socket (`/tmp/al_em_ctrl_data_socket` or `/tmp/al_data_socket`).
    pub data_path: PathBuf,
    /// The role to register as.
    pub role: Role,
    /// Wait before each new attempt to register. The service waits 2 s before it
    /// listens again (`al_sap.rs:96-107`), so this defaults to 2 s.
    pub reconnect_delay: Duration,
}

impl TransportConfig {
    /// A configuration with the default reconnect delay of 2 s.
    #[must_use]
    pub fn new(control_path: PathBuf, data_path: PathBuf, role: Role) -> Self {
        Self {
            control_path,
            data_path,
            role,
            reconnect_delay: Duration::from_secs(2),
        }
    }
}

/// One CMDU with its two AL MAC addresses.
#[derive(Debug, PartialEq, Eq)]
pub struct Sdu {
    /// The local AL MAC (wire MAC A).
    pub local: MacAddr,
    /// The remote AL MAC (wire MAC B). For a multicast send, the 1905 group address
    /// 01:80:c2:00:00:13.
    pub remote: MacAddr,
    /// One CMDU, from the 8-octet header, without an Ethernet header.
    pub cmdu: Vec<u8>,
}

/// An [`Sdu`] for the actor to send, with the session it was built for (0011 A6).
///
/// The role actor copies `link.borrow()` when it builds the SDU, takes `local` from that
/// copy and carries its `session` here. The actor writes the SDU only if the session is
/// still the current one; otherwise it drops and counts it. So an SDU is written only
/// into the session it was built for.
#[derive(Debug, PartialEq, Eq)]
pub struct OutboundSdu {
    /// The [`LinkState::session`] the SDU was built for.
    pub session: u64,
    /// The SDU.
    pub sdu: Sdu,
}

/// Whether the actor is registered, as a latest value (0011 A5).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LinkState {
    /// The number of registrations the service has accepted so far; it equals
    /// [`TransportCounters::registrations`]. A change means a reconnect happened, even if
    /// the role actor missed the `None` in between.
    pub session: u64,
    /// The local AL MAC of the service while registered, `None` otherwise.
    pub al_mac: Option<MacAddr>,
}

/// Why the actor ended with an error.
#[derive(Debug)]
pub enum TransportError {
    /// The service answered the registration with a `result` other than success
    /// (0x00 unknown, 0x02 not supported). Upstream's controller exits too
    /// (`em_ctrl.cpp:1800-1803`).
    RegistrationRejected {
        /// The `result` octet of the response.
        result: u8,
    },
    /// A socket error that is neither "no such socket", "connection refused",
    /// nor a service restart.
    Io {
        /// What the actor was doing.
        operation: &'static str,
        /// The error.
        source: std::io::Error,
    },
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RegistrationRejected { result } => {
                write!(
                    f,
                    "registration rejected by the service, result {result:#04x}"
                )
            }
            Self::Io { operation, source } => write!(f, "{operation}: {source}"),
        }
    }
}

impl std::error::Error for TransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::RegistrationRejected { .. } => None,
            Self::Io { source, .. } => Some(source),
        }
    }
}

/// Counts of what the actor dropped or moved. Errors are logged through `tracing` and
/// counted here; there is no error event (0011 D2).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TransportCounters {
    /// Registrations the service accepted.
    pub registrations: u64,
    /// SDUs written to the data socket.
    pub sdus_sent: u64,
    /// SDUs that arrived complete.
    pub sdus_received: u64,
    /// SDUs not written because they were sent while not registered, were built for another
    /// session, were queued at a stop but not written (built for another session, or behind a
    /// failed write), or arrived through a permit held across that stop's close, within the
    /// linger (0011 A6, A7).
    /// Matching SDUs queued at a stop are written until one write fails; the failed write counts
    /// in [`TransportCounters::dropped_send_failed`], the rest here.
    pub dropped_send_disconnected: u64,
    /// SDUs that need more than 256 fragments, or whose write failed.
    pub dropped_send_failed: u64,
    /// Received SDUs dropped because the received queue was full (the oldest goes first).
    pub dropped_receive_overflow: u64,
    /// Received SDUs still in the actor's queue when it returned, for any reason (0011 A6).
    /// `sdus_received` is the SDUs moved into the received channel, plus
    /// `dropped_receive_overflow`, plus this. Aborting the task skips the count, so the send
    /// and receive counts do not balance after an abort (0011 A7).
    pub dropped_receive_stopped: u64,
    /// Frames longer than 65 536 octets that were skipped.
    pub dropped_receive_oversize: u64,
    /// Frames shorter than an SDU header.
    pub dropped_receive_malformed: u64,
    /// Fragments out of order, and fragment sets that never finished.
    pub dropped_receive_fragments: u64,
    /// Fragment sets refused because their joined payload would pass 1 MiB, counted once per
    /// set when the fragment that would pass the cap arrives. None of the set's frames is
    /// delivered, and the rest of the set is skipped without counting. Not in
    /// `dropped_receive_fragments` and not in `sdus_received` (0016 D4, U2, U6).
    pub dropped_receive_too_large: u64,
}

/// The task [`spawn`] starts: ends with `Ok` on a normal stop.
pub type TransportTask = JoinHandle<Result<(), TransportError>>;

/// What [`spawn`] returns: the role actor's ends of the actor's channels (0011 A5).
///
/// The fields are owned, so the role actor may move them apart. Dropping `link` or
/// `counters` stops nothing.
#[derive(Debug)]
pub struct Transport {
    /// SDUs to send to the service, each with the session it was built for (0011 A6). The
    /// role actor holds the only sender (0004 R6). Once the actor starts to stop, the
    /// channel is closed and sends fail.
    pub sdus: mpsc::Sender<OutboundSdu>,
    /// SDUs received from the service. When the role actor does not read, the actor keeps
    /// the newest 256 beyond the channel's 64 and drops the oldest, counted in
    /// [`TransportCounters::dropped_receive_overflow`].
    pub received: mpsc::Receiver<Sdu>,
    /// The latest [`LinkState`].
    pub link: watch::Receiver<LinkState>,
    /// The latest [`TransportCounters`].
    pub counters: watch::Receiver<TransportCounters>,
    /// The actor's task.
    pub task: TransportTask,
}

/// Starts the actor on the current tokio runtime.
///
/// The actor ends with `Ok` when the role actor drops the SDU sender or the `received`
/// receiver, and with an error if the service rejects the registration or a socket fails
/// in a way the reconnect cannot fix (0011 A5, A6). On a stop or a fatal registration error
/// the actor first closes the send channel, so later sends fail.
/// - **In a session** it writes the SDUs that were in the send channel and belong to the
///   session, then stays 100 ms with the sockets open so that the last writes leave, and
///   then ends. The 100 ms is counted from the end of those writes. SDUs that arrive through
///   a permit held across the close are counted as `dropped_send_disconnected`. An SDU whose
///   permit is still held when the 100 ms ends is lost uncounted (0011 A6).
/// - **While not registered** it counts the queued SDUs as dropped and ends at once, unless
///   a send permit is outstanding: then it waits for it, at most 100 ms.
///
/// If a read fails and the role actor has gone, the actor stops as above and still writes
/// the queued SDUs. If a write fails, the session ends without another write, because the
/// stream may be out of step; on a stop the queued SDUs are counted as
/// `dropped_send_disconnected`. In either case the actor stops reading, so every received
/// frame not yet read is lost and not counted (0011 A7).
///
/// A stop writes the matching SDUs counted by the channel's length, read just after the
/// close: a send completed before the read is in the batch, one in progress may fall on
/// either side (0011 A7).
///
/// **No time limit on writes (0011 A7).** A write can block without limit if the service
/// stays alive and stops reading, in a session or during a stop. To bound a stop, time out
/// on `&mut task` (or keep an `AbortHandle`) and then call `abort()`. A timeout that takes
/// the [`TransportTask`] by value drops it on expiry and leaves the actor running. An
/// aborted actor returns a cancelled `JoinError`. It skips the linger, the counts for the
/// blocked SDU and the SDUs still queued, the unfinished fragment set and
/// `dropped_receive_stopped`, and does not publish `al_mac: None`.
///
/// Dropping only `link` or `counters` stops nothing.
#[must_use]
pub fn spawn(config: TransportConfig) -> Transport {
    let (sdus, sdu_receiver) = mpsc::channel(actor::SDU_CHANNEL_CAPACITY);
    let (received_sender, received) = mpsc::channel(actor::RECEIVED_CHANNEL_CAPACITY);
    let (link_sender, link) = watch::channel(LinkState::default());
    let (counters_sender, counters) = watch::channel(TransportCounters::default());
    let task = tokio::spawn(actor::run(
        config,
        sdu_receiver,
        received_sender,
        link_sender,
        counters_sender,
    ));
    Transport {
        sdus,
        received,
        link,
        counters,
        task,
    }
}
