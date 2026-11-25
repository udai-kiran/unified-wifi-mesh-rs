// SPDX-License-Identifier: Apache-2.0

//! The fallible random source every key, nonce, IV, DH exponent and ECDSA `k` is drawn from.
//!
//! The curve crates' own `random` constructors and OS RNG integration are not used (they panic
//! on failure); callers sample from a [`RandomSource`] instead, so a failing source surfaces as
//! an error.

use core::fmt;

use crate::secret::SecretArray;

/// The random source could not produce bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RandomError;

impl fmt::Display for RandomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("random source failure")
    }
}

impl std::error::Error for RandomError {}

/// A source of random bytes that can fail.
pub trait RandomSource {
    /// Fills `dest` completely, or fails without any guarantee about `dest`'s contents.
    ///
    /// # Errors
    /// [`RandomError`] when the source cannot deliver the bytes.
    fn fill(&mut self, dest: &mut [u8]) -> Result<(), RandomError>;
}

impl<R: RandomSource + ?Sized> RandomSource for &mut R {
    fn fill(&mut self, dest: &mut [u8]) -> Result<(), RandomError> {
        (**self).fill(dest)
    }
}

/// The operating system's random generator (via `getrandom`).
#[allow(
    clippy::disallowed_types,
    reason = "0016 U27: the OsRandom definition (derives name the type)"
)]
///
/// The private field and the missing `Default` make [`OsRandom::new`] the only way to build
/// one, so the workspace `disallowed-methods` entry for `new` catches every use in a core.
/// `Clone` and `Copy` stay: they only duplicate a value that already exists.
#[derive(Debug, Clone, Copy)]
pub struct OsRandom {
    _private: (),
}

#[allow(
    clippy::disallowed_types,
    reason = "0016 U27: the OsRandom constructor names the type"
)]
impl OsRandom {
    /// The OS random generator; the one way to get it.
    #[must_use]
    #[allow(
        clippy::new_without_default,
        reason = "0016 U27: no Default on purpose, so cores cannot build one without `new`"
    )]
    pub const fn new() -> Self {
        Self { _private: () }
    }
}

#[allow(
    clippy::disallowed_types,
    reason = "0016 U27: the OsRandom impl is where OS randomness enters"
)]
impl RandomSource for OsRandom {
    #[allow(
        clippy::disallowed_methods,
        reason = "0016 U27: the one production getrandom call"
    )]
    fn fill(&mut self, dest: &mut [u8]) -> Result<(), RandomError> {
        getrandom::getrandom(dest).map_err(|_| RandomError)
    }
}

/// A source that always fails, for testing error propagation.
#[derive(Debug, Clone, Copy, Default)]
pub struct FailingRandom;

impl RandomSource for FailingRandom {
    fn fill(&mut self, _dest: &mut [u8]) -> Result<(), RandomError> {
        Err(RandomError)
    }
}

/// `N` random bytes in a zeroizing array (keys, IVs, secret nonces, scalar candidates).
///
/// # Errors
/// [`RandomError`] when the source fails; the partly filled array is wiped.
pub fn random_secret<const N: usize>(
    source: &mut impl RandomSource,
) -> Result<SecretArray<N>, RandomError> {
    let mut secret = SecretArray::<N>::zeroed();
    source.fill(secret.expose_mut())?;
    Ok(secret)
}

/// Length of the 16-byte nonces used by WSC and DPP (`em_nonce_t`).
pub const NONCE_LEN: usize = 16;

/// A 16-byte nonce (`em_nonce_t`). Nonces are sent in clear, so this type is `Copy` and
/// prints its bytes; use [`random_secret`] with `N = 16` for a nonce that must stay secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nonce([u8; NONCE_LEN]);

impl Nonce {
    /// Wraps existing bytes.
    #[must_use]
    pub fn from_bytes(bytes: [u8; NONCE_LEN]) -> Self {
        Self(bytes)
    }

    /// Draws a fresh nonce.
    ///
    /// # Errors
    /// [`RandomError`] when the source fails.
    pub fn random(source: &mut impl RandomSource) -> Result<Self, RandomError> {
        let mut bytes = [0; NONCE_LEN];
        source.fill(&mut bytes)?;
        Ok(Self(bytes))
    }

    /// The nonce bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; NONCE_LEN] {
        &self.0
    }
}

/// A deterministic byte stream (`SplitMix64`) for replay tests.
///
/// This is NOT random. The same seed always gives the same bytes, which is the point: a role
/// core driven by it produces identical output on every run. Never use it for real keys,
/// nonces or IVs. It is deliberately not `Clone` or `Copy`, because a copy would repeat the
/// stream of the original.
#[cfg(feature = "test-random")]
#[allow(
    clippy::disallowed_types,
    reason = "0016 U27: the SeededRandom definition"
)]
#[derive(Debug)]
pub struct SeededRandom {
    state: u64,
}

#[cfg(feature = "test-random")]
#[allow(
    clippy::disallowed_types,
    reason = "0016 U27: the SeededRandom constructor names the type"
)]
impl SeededRandom {
    /// A generator whose stream is fixed by `seed`.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

#[cfg(feature = "test-random")]
#[allow(clippy::disallowed_types, reason = "0016 U27: the SeededRandom impl")]
impl RandomSource for SeededRandom {
    fn fill(&mut self, dest: &mut [u8]) -> Result<(), RandomError> {
        for chunk in dest.chunks_mut(8) {
            let word = self.next_u64().to_le_bytes();
            chunk.copy_from_slice(&word[..chunk.len()]);
        }
        Ok(())
    }
}

#[cfg(all(test, feature = "test-random"))]
#[allow(
    clippy::disallowed_types,
    reason = "0016 U27: the seeded generator's own tests"
)]
mod seeded_random_tests {
    use super::{RandomSource, SeededRandom};
    use crate::dh::DhPrivate;

    fn bytes_from(source: &mut SeededRandom) -> [u8; 32] {
        let mut bytes = [0; 32];
        source.fill(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn same_seed_gives_same_bytes() {
        let first = bytes_from(&mut SeededRandom::new(7));
        let second = bytes_from(&mut SeededRandom::new(7));
        assert_eq!(first, second);
    }

    #[test]
    fn different_seeds_differ() {
        let first = bytes_from(&mut SeededRandom::new(1));
        let second = bytes_from(&mut SeededRandom::new(2));
        assert_ne!(first, second);
    }

    #[test]
    fn successive_fills_differ() {
        let mut source = SeededRandom::new(7);
        let first = bytes_from(&mut source);
        let second = bytes_from(&mut source);
        assert_ne!(first, second);
    }

    #[test]
    fn odd_length_fill_is_a_prefix_of_the_stream() {
        let full = bytes_from(&mut SeededRandom::new(9));
        let mut short = [0; 11];
        SeededRandom::new(9).fill(&mut short).unwrap();
        assert_eq!(short, full[..11]);
    }

    #[test]
    fn drives_dh_key_generation() {
        let mut source = SeededRandom::new(42);
        assert!(DhPrivate::generate(&mut source).is_ok());
    }
}
