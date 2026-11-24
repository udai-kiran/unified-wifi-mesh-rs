// SPDX-License-Identifier: Apache-2.0

//! The numbers of the transport's policy (0016 U15, U31). The mechanisms in `wire` and
//! `actor` take them as arguments and hold none.

/// Largest joined payload of a fragmented set, in octets: 1 MiB (0016 D4 "size cap", U2, U6).
/// The daemon's largest CMDU is about 373 KiB. Upstream has no cap below the framing limit
/// (`al_service_access_point.cpp:224-300`).
pub(crate) const RX_SDU_CAP: usize = 1_048_576;
