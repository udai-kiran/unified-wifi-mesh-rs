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
#[derive(Debug, Clone, Copy, Default)]
pub struct OsRandom;

impl RandomSource for OsRandom {
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
