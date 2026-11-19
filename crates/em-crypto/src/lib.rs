// SPDX-License-Identifier: Apache-2.0

//! Cryptographic primitives for the unified-wifi-mesh port (note 0006).
//!
//! The crate is stateless and synchronous. Secrets are returned in zeroizing types that
//! implement neither `Clone` nor `Copy` and redact their `Debug` output. All randomness comes
//! from a fallible [`rand::RandomSource`]; a failing source is an error, never a panic.

pub mod cbc;
pub mod ct;
pub mod dh;
pub mod ec;
pub mod encoding;
pub mod error;
pub mod hash;
pub mod kdf;
pub mod keywrap;
pub mod rand;
pub mod secret;
pub mod siv;

pub use error::Error;
pub use hash::HashAlgorithm;
pub use rand::{Nonce, OsRandom, RandomError, RandomSource};
pub use secret::{SecretArray, SecretBytes};
