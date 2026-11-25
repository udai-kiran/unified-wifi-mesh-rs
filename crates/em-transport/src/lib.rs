// SPDX-License-Identifier: Apache-2.0

//! AL-SAP client: carries CMDUs to and from an external IEEE 1905 daemon over two UNIX sockets.
//!
//! Design: note 0011 (slice S0), D1 to D3; note 0016 for the loop transport. The client does
//! what upstream's `AlServiceAccessPoint` does (`src/al-sap/al_service_access_point.cpp`,
//! `inc/al_service_*.h` at `ba4180b`) against the service in `ieee1905-rs`
//! (`ieee1905-core/src/al_sap.rs` at `a6443225`):
//!
//! 1. connect the control socket, then the data socket;
//! 2. send a registration request and read the response;
//! 3. exchange SDUs on the data socket, fragmenting and joining as needed;
//! 4. when the data socket closes or fails, publish a [`LinkState`] without an AL MAC,
//!    wait, and register again.
//!
//! The client runs on the event loop (`em-event-loop`), in three parts:
//!
//! * [`TransportCore`]: every decision as a synchronous state machine, with no clock and no
//!   I/O. It is driven with the current time and returns [`TransportAction`]s.
//! * [`TransportHandler`]: composes the core into a loop application. The role crate owns one,
//!   gives it its [`TransportSources`] and passes it the [`TransportEvent`]s and the loop turns.
//!   What the I/O must do comes out as [`TransportIoAction`]s.
//! * [`TransportIo`]: the tokio side. It performs the actions on the two sockets and reports
//!   [`IoEvent`]s. It adds no decisions.
//!
//! The role sends [`OutboundSdu`]s: the SDU with the `session` of the [`LinkState`] it was
//! built under (note 0011 A6). The core writes it only into that session. The role reads the
//! received SDUs, the [`LinkState`] and the [`TransportCounters`] from the core through the
//! handler. A link change is a latest value, not an event. At each registration the core
//! publishes the new state before it handles any frame of the session, so when the role
//! handles a received SDU, its link state shows that SDU's session or a later one.
//!
//! The old tokio actor (`spawn`, `Transport`, `TransportTask`) was deleted in 0016 D8 step 5b.
//! It can be read at `b6995f2` (`crates/em-transport/src/actor.rs`).
//!
//! The payload of an [`Sdu`] is opaque here. **Contract for callers:** every SDU is
//! exactly one CMDU ending in End of Message. The service drops anything else
//! without telling the client (0011, "The AL-SAP protocol").
//!
//! `local` and `remote` are the only address names above this crate. In both
//! directions `local` is the local AL MAC and `remote` the other side's: on the wire,
//! MAC A and MAC B (0011, "Address fields").

mod frame_reader;
mod frame_splitter;
mod handler;
mod outbound_buffer;
mod policy;
mod received_queue;
#[cfg(test)]
mod test_prng;
mod tokio_io;
mod transport_core;
mod wire;

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use em_types::MacAddr;

pub use frame_splitter::Frame;
pub use handler::{TransportEvent, TransportHandler, TransportIoAction, TransportSources};
pub use tokio_io::{IoAction, IoEvent, TransportIo};
pub use transport_core::{
    RegistrationStep, TransportAction, TransportCore, TransportOutcome, TransportPolicy,
};
pub use wire::{MAX_FRAGMENT_PAYLOAD, MAX_SDU_PAYLOAD};

/// Which service type to register as (upstream `al_service_registration_enums.h:7-14`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// `EasyMesh` agent, service type 0x01.
    Agent,
    /// `EasyMesh` controller, service type 0x02.
    Controller,
}

/// What the transport needs to know to reach the service.
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
            reconnect_delay: policy::RECONNECT_DELAY,
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

/// An [`Sdu`] for the transport to send, with the session it was built for (0011 A6).
///
/// The role reads the [`LinkState`] when it builds the SDU, takes `local` from it and carries
/// its `session` here. [`TransportCore`] writes the SDU only if the session is still the
/// current one; otherwise it drops and counts it. So an SDU is written only into the session
/// it was built for.
#[derive(Debug, PartialEq, Eq)]
pub struct OutboundSdu {
    /// The [`LinkState::session`] the SDU was built for.
    pub session: u64,
    /// The SDU.
    pub sdu: Sdu,
}

/// Whether the transport is registered, as a latest value (0011 A5).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LinkState {
    /// The number of registrations the service has accepted so far; it equals
    /// [`TransportCounters::registrations`]. A change means a reconnect happened, even if
    /// the role missed the `None` in between.
    pub session: u64,
    /// The local AL MAC of the service while registered, `None` otherwise.
    pub al_mac: Option<MacAddr>,
}

/// Why the transport ended with an error ([`TransportIo::into_result`]).
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
        /// What the transport was doing.
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

/// Counts of what [`TransportCore`] dropped or moved. Errors are logged through `tracing` and
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
    /// failed write), or were sent after the stop began (0011 A6, A7).
    /// Matching SDUs queued at a stop are written until one write fails; the failed write counts
    /// in [`TransportCounters::dropped_send_failed`], the rest here.
    pub dropped_send_disconnected: u64,
    /// SDUs that need more than 256 fragments, or whose write failed.
    pub dropped_send_failed: u64,
    /// SDUs dropped because the outbound buffer was full: the oldest SDU not yet started (and not
    /// being written), or the new SDU when nothing could be dropped for it. The old actor never counted it (0016 U12,
    /// difference `tx-buffer`).
    pub dropped_send_overflow: u64,
    /// Received SDUs dropped because the received queue was full (the oldest goes first).
    pub dropped_receive_overflow: u64,
    /// Received SDUs still in the received queue when the transport finished, for any reason
    /// (0011 A6). `sdus_received` is the SDUs the role took, plus `dropped_receive_overflow`,
    /// plus this. Aborting the task that runs the loop skips the count, so the send and
    /// receive counts do not balance after an abort (0011 A7).
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
