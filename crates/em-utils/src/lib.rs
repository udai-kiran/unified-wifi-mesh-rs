// SPDX-License-Identifier: Apache-2.0

//! Small utilities for the unified-wifi-mesh Rust port.
//!
//! This crate contains exactly the pieces listed in design note
//! [0003-em-utils-scope](https://github.com/udai-kiran/unified-wifi-mesh-rs/blob/main/docs/0003-em-utils-scope.md), section D1:
//!
//! | Module | Holds | Replaces (upstream) |
//! |---|---|---|
//! | [`chan`] | Wi-Fi channel table and lookups in both directions | `frequency_ranges`, `em_chan_to_freq`, `em_freq_to_chan` |
//! | [`akm`] | AKM name and suite selector map | `akm_to_oui`, `oui_to_akm`, `akm_to_bytes` |
//! | [`time`] | UTC timestamp text | `get_date_time_rfc3399` |
//! | [`hex`] | Hex dump text | `print_hex_dump` |
//!
//! No dependencies, no `unsafe`.
//!
//! Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.

#![forbid(unsafe_code)]

pub mod akm;
pub mod chan;
pub mod hex;
pub mod time;

pub use akm::{akm_to_suite, suite_to_akm};
pub use chan::{FREQ_RANGES, FreqRange, chan_to_freq, freq_to_chan};
pub use hex::HexDump;
pub use time::Rfc3339Utc;
