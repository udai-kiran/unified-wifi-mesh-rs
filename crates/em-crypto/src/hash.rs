// SPDX-License-Identifier: Apache-2.0

//! SHA-2 hashing and HMAC over a list of input pieces (upstream `platform_hash` and
//! `platform_hmac_hash`). The pieces are processed as their concatenation.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256, Sha384, Sha512};

use crate::error::Error;
use crate::secret::SecretBytes;

/// The hash functions supported by the KDFs, hashes and HMACs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashAlgorithm {
    /// SHA-256 (32-byte output).
    Sha256,
    /// SHA-384 (48-byte output).
    Sha384,
    /// SHA-512 (64-byte output).
    Sha512,
}

impl HashAlgorithm {
    /// Length of the digest and of the HMAC output in bytes.
    #[must_use]
    pub const fn output_len(self) -> usize {
        match self {
            HashAlgorithm::Sha256 => 32,
            HashAlgorithm::Sha384 => 48,
            HashAlgorithm::Sha512 => 64,
        }
    }
}

fn digest_with<D: Digest>(pieces: &[&[u8]]) -> SecretBytes {
    let mut hasher = D::new();
    for piece in pieces {
        hasher.update(piece);
    }
    SecretBytes::from_slice(&hasher.finalize())
}

fn hmac_with<M: Mac + hmac::digest::KeyInit>(
    key: &[u8],
    pieces: &[&[u8]],
) -> Result<SecretBytes, Error> {
    let mut mac = <M as Mac>::new_from_slice(key).map_err(|_| Error::InvalidKeyLength)?;
    for piece in pieces {
        mac.update(piece);
    }
    Ok(SecretBytes::from_slice(&mac.finalize().into_bytes()))
}

/// Hash of the concatenated `pieces`. The result is returned as a secret because it is often
/// derived from key material (for example the WSC DH key); read it with `expose`.
#[must_use]
pub fn digest(algorithm: HashAlgorithm, pieces: &[&[u8]]) -> SecretBytes {
    match algorithm {
        HashAlgorithm::Sha256 => digest_with::<Sha256>(pieces),
        HashAlgorithm::Sha384 => digest_with::<Sha384>(pieces),
        HashAlgorithm::Sha512 => digest_with::<Sha512>(pieces),
    }
}

/// HMAC of the concatenated `pieces`. `key` may have any length.
///
/// # Errors
/// [`Error::InvalidKeyLength`] if the HMAC implementation refuses the key (it accepts every
/// length, so this is not expected).
pub fn hmac(algorithm: HashAlgorithm, key: &[u8], pieces: &[&[u8]]) -> Result<SecretBytes, Error> {
    match algorithm {
        HashAlgorithm::Sha256 => hmac_with::<Hmac<Sha256>>(key, pieces),
        HashAlgorithm::Sha384 => hmac_with::<Hmac<Sha384>>(key, pieces),
        HashAlgorithm::Sha512 => hmac_with::<Hmac<Sha512>>(key, pieces),
    }
}
