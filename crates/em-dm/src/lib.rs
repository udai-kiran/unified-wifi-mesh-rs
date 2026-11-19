// SPDX-License-Identifier: Apache-2.0

//! `EasyMesh` data model records and enums (note 0007, Step 2).
//!
//! This crate holds the native Rust form of the `em-dm`-owned types in the approved
//! classification table that Step 2 covers. The types deferred to later steps are listed in the
//! crate README. It has no models, operations, JSON, database or wire codecs (those are Step 3
//! and the consumers).
//!
//! # Representation (0007 D3)
//!
//! - Small values are `Copy` and at most [`COPY_SIZE_LIMIT`] bytes: enums, MACs, ids, short
//!   strings and records made only of those. The `record!` macro asserts the size at compile
//!   time for every `Copy` record, and the golden test checks it again.
//! - Records above that size, or holding a heap collection or an `em_types::Interface`, are
//!   neither `Clone` nor `Copy`. Each one's doc comment says to store it as `Box<Self>` where
//!   its inline size matters, and it carries compile-fail doctests for both traits.
//! - Strings are byte strings ([`CStrBytes`], [`LenBytes`]) unless upstream treats the member as
//!   text ([`Text`]). In-record arrays with a count field are a [`BoundedVec`]: the count is the
//!   length, and going over the capacity is an error.
//! - Multi-bit C bit-fields are [`UBits`]; 1-bit fields are `bool`.
//! - Credentials (passphrases, `client_id`, `client_secret`) are wrapped in [`Secret`], whose
//!   `Debug` output is redacted. C `float` members are [`Float32`], compared by bit pattern.
//! - C enums are open newtypes: unknown codes round-trip; sentinels are not named.
//!
//! # Lints (0007 D3, criterion 4)
//!
//! `large_types_passed_by_value`, `large_stack_arrays` and `large_enum_variant` are denied below,
//! at clippy's default thresholds. There is deliberately no crate `clippy.toml`: it would replace
//! the workspace one and drop the clone ban. Defaults, checked on clippy 0.1.85 (Rust 1.85.0)
//! and clippy 0.1.100 (stable):
//!
//! - `large_types_passed_by_value`: 256 bytes (`pass-by-value-size-limit`); like clippy's default
//!   `avoid-breaking-exported-api`, it skips `pub` functions. [`COPY_SIZE_LIMIT`] matches it.
//! - `large_stack_arrays`: a local array above 16 384 bytes (`array-size-threshold`).
//! - `large_enum_variant`: variants whose size differs by more than 200 bytes
//!   (`enum-variant-size-threshold`).
//!
//! Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.

#![deny(
    clippy::large_types_passed_by_value,
    clippy::large_stack_arrays,
    clippy::large_enum_variant
)]

#[macro_use]
mod macros;

pub mod bits;
pub mod bounded;
pub mod enums;
pub mod float;
#[cfg(test)]
mod limit_tests;
pub mod limits;
#[cfg(test)]
mod mapping;
#[cfg(test)]
mod record_tests;
pub mod records;
pub mod secret;
pub mod text;

#[cfg(test)]
mod golden;

pub use bits::{BitsError, UBits};
pub use bounded::{BoundedVec, CapacityError};
pub use enums::{
    ChannelBandwidth, DeviceRole, HaulType, Ieee80211Variant, OpClassType, PolicyIdType,
    ProductClass, ScannerType, SteeringPolicyType, VapMode,
};
pub use float::Float32;
pub use records::*;
pub use secret::Secret;
pub use text::{CStrBytes, LenBytes, StringError, Text};

/// Largest size in bytes of a `Copy` type in this crate (0007 D3, criterion 4).
pub const COPY_SIZE_LIMIT: usize = 256;
