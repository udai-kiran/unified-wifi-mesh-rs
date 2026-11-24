// SPDX-License-Identifier: Apache-2.0

//! The numbers of the transport's policy (0016 U15, U31). The mechanisms in `wire`, `actor` and
//! `transport_core` take them as arguments and hold none; `TransportPolicy::easymesh` is the one
//! place that gathers them.

use std::time::Duration;

/// Wait before each new attempt to register (U1 does not change it): the service waits 2 s before
/// it listens again (`al_sap.rs:96-107`).
pub(crate) const RECONNECT_DELAY: Duration = Duration::from_secs(2);

/// Most SDUs in the received queue (0016 U1, difference `rx-queue-1024-4mib`).
pub(crate) const RX_QUEUE_SDUS: usize = 1024;

/// Most octets of CMDU in the received queue: 4 MiB (0016 U1).
pub(crate) const RX_QUEUE_BYTES: usize = 4 * 1024 * 1024;

/// Largest joined payload of a fragmented set, in octets: 1 MiB (0016 D4 "size cap", U2, U6).
/// The daemon's largest CMDU is about 373 KiB. Upstream has no cap below the framing limit
/// (`al_service_access_point.cpp:224-300`).
pub(crate) const RX_SDU_CAP: usize = 1_048_576;

/// Most octets in the outbound buffer: 4 MiB (0016 U12, difference `tx-buffer`).
pub(crate) const TX_BUFFER_BYTES: usize = 4 * 1024 * 1024;

/// How long queued outbound bytes may make no progress in a session (0016 U4, difference
/// `tx-write-stall-5s`).
pub(crate) const WRITE_STALL: Duration = Duration::from_secs(5);

/// How long a stop writes the queued outbound bytes (0016 U8).
pub(crate) const STOP_WRITE_LIMIT: Duration = Duration::from_millis(900);

/// How long a stop keeps the sockets open after the writes, so that the last writes leave
/// (0011, "SDUs just before a close can be lost"; 0016 U3).
pub(crate) const LINGER: Duration = Duration::from_millis(100);
