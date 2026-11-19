// SPDX-License-Identifier: Apache-2.0

//! Diffie-Hellman over the RFC 3526 1536-bit group (group 5, generator 2), as WSC uses it.
//!
//! Exponentiation is constant time with a fixed 1536-bit exponent bound. Exponents are drawn
//! from a [`RandomSource`] by rejection sampling in `2 ..= p - 2`. A peer value is accepted
//! only in `2 ..= p - 2`; there is no subgroup test. The shared secret is the big-endian value
//! with leading zero bytes removed (OpenSSL 3 default, no padding), and public values are 192
//! bytes, left-padded with zeros.

use core::fmt;

use crypto_bigint::modular::runtime_mod::{DynResidue, DynResidueParams};
use crypto_bigint::{Encoding, U1536};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::error::Error;
use crate::rand::RandomSource;
use crate::secret::{SecretArray, SecretBytes};

/// Length of every DH value (public key, exported exponent) in bytes (`em_dh5_key_t`).
pub const DH_VALUE_LEN: usize = 192;

/// RFC 3526 group 5 prime (upstream `g_dh1536_p`).
const PRIME: U1536 = U1536::from_be_hex(concat!(
    "ffffffffffffffffc90fdaa22168c234c4c6628b80dc1cd129024e088a67cc74",
    "020bbea63b139b22514a08798e3404ddef9519b3cd3a431b302b0a6df25f1437",
    "4fe1356d6d51c245e485b576625e7ec6f44c42e9a637ed6b0bff5cb6f406b7ed",
    "ee386bfb5a899fa5ae9f24117c4b1fe649286651ece45b3dc2007cb8a163bf05",
    "98da48361c55d39a69163fa8fd24cf5f83655d23dca3ad961c62f356208552bb",
    "9ed529077096966d670c354e4abc9804f1746c08ca237327ffffffffffffffff",
));
const GENERATOR: U1536 = U1536::from_u8(2);
const TWO: U1536 = U1536::from_u8(2);
/// `p - 2`, the largest acceptable value.
const MAX_VALUE: U1536 = PRIME.wrapping_sub(&TWO);

/// Draws before giving up on a source that never yields an acceptable value. Each draw fails
/// with probability about 2^-64.
const MAX_SAMPLING_ATTEMPTS: usize = 64;

/// A DH public value: exactly [`DH_VALUE_LEN`] bytes, big-endian, zero-padded on the left
/// (the Rust replacement of `em_dh5_key_t`). Public data, so it is `Copy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DhPublic([u8; DH_VALUE_LEN]);

impl DhPublic {
    /// Imports a peer's public value from big-endian bytes: any number of leading zero bytes,
    /// at most 192 significant bytes, and a value in `2 ..= p - 2`. The value is stored at the
    /// fixed width, so a shorter input is zero-padded on the left.
    ///
    /// # Errors
    /// [`Error::ValueOutOfRange`] for an empty input, more than 192 significant bytes, or a
    /// value outside `2 ..= p - 2`.
    pub fn from_be_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let value = parse_be(bytes)?;
        if !in_range(&value) {
            return Err(Error::ValueOutOfRange);
        }
        Ok(Self(value.to_be_bytes()))
    }

    /// The 192 big-endian bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; DH_VALUE_LEN] {
        &self.0
    }
}

/// A DH private exponent in `2 ..= p - 2`. Wiped on drop; no `Clone`; `Debug` is redacted.
pub struct DhPrivate(Zeroizing<U1536>);

impl fmt::Debug for DhPrivate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DhPrivate(<redacted>)")
    }
}

impl ZeroizeOnDrop for DhPrivate {}

/// Converts 0..=192 big-endian bytes to an integer; more than 192 bytes are accepted only when
/// the excess is leading zeros (upstream's `BN_bin2bn` ignores them).
fn parse_be(bytes: &[u8]) -> Result<Zeroizing<U1536>, Error> {
    let first_nonzero = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
    let significant = &bytes[first_nonzero..];
    if significant.len() > DH_VALUE_LEN {
        return Err(Error::ValueOutOfRange);
    }
    let mut padded = SecretArray::<DH_VALUE_LEN>::zeroed();
    padded.expose_mut()[DH_VALUE_LEN - significant.len()..].copy_from_slice(significant);
    Ok(Zeroizing::new(U1536::from_be_slice(padded.expose())))
}

fn in_range(value: &U1536) -> bool {
    *value >= TWO && *value <= MAX_VALUE
}

/// `base^exponent mod p` with the exponent bound fixed at 1536 bits.
fn pow_mod_p(base: &U1536, exponent: &U1536) -> Zeroizing<U1536> {
    let params = DynResidueParams::new(&PRIME);
    let mut power = DynResidue::new(base, params).pow(exponent);
    let result = Zeroizing::new(power.retrieve());
    power.zeroize();
    result
}

impl DhPrivate {
    /// Draws a private exponent from `source`.
    ///
    /// # Errors
    /// [`Error::Random`] when the source fails, [`Error::SamplingExhausted`] when no draw falls
    /// in range (a broken source).
    pub fn generate(source: &mut impl RandomSource) -> Result<Self, Error> {
        for _ in 0..MAX_SAMPLING_ATTEMPTS {
            let mut candidate = SecretArray::<DH_VALUE_LEN>::zeroed();
            source.fill(candidate.expose_mut())?;
            let value = Zeroizing::new(U1536::from_be_slice(candidate.expose()));
            if in_range(&value) {
                return Ok(Self(value));
            }
        }
        Err(Error::SamplingExhausted)
    }

    /// Imports a private exponent from 1 to 192 big-endian bytes.
    ///
    /// # Errors
    /// [`Error::InvalidInputLength`] for an empty or longer input (checked before conversion),
    /// [`Error::ValueOutOfRange`] when the value is not in `2 ..= p - 2`.
    pub fn from_be_bytes(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.is_empty() || bytes.len() > DH_VALUE_LEN {
            return Err(Error::InvalidInputLength);
        }
        // Left-pad by length alone: the secret's leading zeros must not decide the control flow.
        let mut padded = SecretArray::<DH_VALUE_LEN>::zeroed();
        padded.expose_mut()[DH_VALUE_LEN - bytes.len()..].copy_from_slice(bytes);
        let value = Zeroizing::new(U1536::from_be_slice(padded.expose()));
        if !in_range(&value) {
            return Err(Error::ValueOutOfRange);
        }
        Ok(Self(value))
    }

    /// The exponent as 192 big-endian bytes.
    #[must_use]
    pub fn to_be_bytes(&self) -> SecretArray<DH_VALUE_LEN> {
        let bytes = Zeroizing::new(self.0.to_be_bytes());
        let mut secret = SecretArray::zeroed();
        secret.expose_mut().copy_from_slice(&*bytes);
        secret
    }

    /// The public value `2^exponent mod p`, 192 bytes wide.
    #[must_use]
    pub fn public_key(&self) -> DhPublic {
        DhPublic(pow_mod_p(&GENERATOR, &self.0).to_be_bytes())
    }

    /// The shared secret `peer^exponent mod p` as minimal big-endian bytes (no leading zeros).
    ///
    /// `peer` is big-endian of any length up to 192 significant bytes.
    ///
    /// # Errors
    /// [`Error::ValueOutOfRange`] when the peer value is not in `2 ..= p - 2`.
    pub fn shared_secret(&self, peer: &[u8]) -> Result<SecretBytes, Error> {
        let peer = parse_be(peer)?;
        if !in_range(&peer) {
            return Err(Error::ValueOutOfRange);
        }
        let full_width = Zeroizing::new(pow_mod_p(&peer, &self.0).to_be_bytes());
        let first_nonzero = full_width
            .iter()
            .position(|&b| b != 0)
            .unwrap_or(DH_VALUE_LEN);
        Ok(SecretBytes::from_slice(&full_width[first_nonzero..]))
    }
}
