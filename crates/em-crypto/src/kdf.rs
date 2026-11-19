// SPDX-License-Identifier: Apache-2.0

//! Key derivation: the 802.11 KDF, the WPS KDF, HKDF and PBKDF2-HMAC-SHA1.

use hkdf::Hkdf;
use sha1::Sha1;
use sha2::{Sha256, Sha384, Sha512};

use crate::error::Error;
use crate::hash::{self, HashAlgorithm};
use crate::secret::SecretBytes;

/// Longest 802.11 KDF output in bytes. Upstream writes `output_len * 8` into a 16-bit field and
/// silently truncates it from 8192 bytes up; this port refuses those lengths.
pub const KDF_80211_MAX_OUTPUT_LEN: usize = 8191;

/// Longest WPS KDF output in bytes. The bit count is a 32-bit field that upstream wraps from
/// 536,870,912 bytes up; this port refuses those lengths before allocating.
pub const WPS_KDF_MAX_OUTPUT_LEN: usize = 536_870_911;

/// Key length of the WPS KDF: the 32-byte SHA-256 digest size.
pub const WPS_KDF_KEY_LEN: usize = 32;

fn reject_nul(label: &str) -> Result<(), Error> {
    if label.contains('\0') {
        return Err(Error::InvalidLabel);
    }
    Ok(())
}

/// The 802.11 KDF: block `i` is `HMAC(key, LE16(i) || label || context || LE16(output_bits))`.
///
/// Like upstream it refuses an empty key and an empty output.
///
/// # Errors
/// [`Error::InvalidKeyLength`] for an empty key, [`Error::OutputLength`] when `output_len` is
/// 0 or above [`KDF_80211_MAX_OUTPUT_LEN`], [`Error::InvalidLabel`] for a label with a NUL.
pub fn kdf_80211(
    algorithm: HashAlgorithm,
    key: &[u8],
    label: &str,
    context: &[u8],
    output_len: usize,
) -> Result<SecretBytes, Error> {
    reject_nul(label)?;
    if key.is_empty() {
        return Err(Error::InvalidKeyLength);
    }
    if output_len == 0 || output_len > KDF_80211_MAX_OUTPUT_LEN {
        return Err(Error::OutputLength);
    }
    let output_bits = u16::try_from(output_len * 8).map_err(|_| Error::OutputLength)?;
    let block_len = algorithm.output_len();

    let mut output = SecretBytes::zeroed(output_len);
    for (index, chunk) in output.expose_mut().chunks_mut(block_len).enumerate() {
        // At most 8191 / 32 = 256 blocks, so the counter fits in 16 bits.
        let counter = u16::try_from(index + 1).map_err(|_| Error::OutputLength)?;
        let block = hash::hmac(
            algorithm,
            key,
            &[
                &counter.to_le_bytes(),
                label.as_bytes(),
                context,
                &output_bits.to_le_bytes(),
            ],
        )?;
        chunk.copy_from_slice(&block.expose()[..chunk.len()]);
    }
    Ok(output)
}

/// The WPS KDF: block `i` is `HMAC-SHA256(key, BE32(i) || prefix || label || BE32(output_bits))`.
///
/// # Errors
/// [`Error::InvalidKeyLength`] unless `key` has [`WPS_KDF_KEY_LEN`] bytes,
/// [`Error::OutputLength`] when `output_len` is above [`WPS_KDF_MAX_OUTPUT_LEN`] (checked
/// before allocating), [`Error::InvalidLabel`] for a label with a NUL.
pub fn wps_kdf(
    key: &[u8],
    prefix: &[u8],
    label: &str,
    output_len: usize,
) -> Result<SecretBytes, Error> {
    reject_nul(label)?;
    if key.len() != WPS_KDF_KEY_LEN {
        return Err(Error::InvalidKeyLength);
    }
    if output_len > WPS_KDF_MAX_OUTPUT_LEN {
        return Err(Error::OutputLength);
    }
    // `output_len < 2^29`, so the bit count fits in 32 bits.
    let output_bits = u32::try_from(output_len).map_err(|_| Error::OutputLength)? * 8;

    let block_len = HashAlgorithm::Sha256.output_len();
    let mut output = SecretBytes::zeroed(output_len);
    for (index, chunk) in output.expose_mut().chunks_mut(block_len).enumerate() {
        let counter = u32::try_from(index + 1).map_err(|_| Error::OutputLength)?;
        let block = hash::hmac(
            HashAlgorithm::Sha256,
            key,
            &[
                &counter.to_be_bytes(),
                prefix,
                label.as_bytes(),
                &output_bits.to_be_bytes(),
            ],
        )?;
        chunk.copy_from_slice(&block.expose()[..chunk.len()]);
    }
    Ok(output)
}

/// HKDF (RFC 5869) extract-and-expand. An empty `salt` behaves as a salt of zeros.
///
/// # Errors
/// [`Error::OutputLength`] when `output_len` exceeds 255 hash blocks (checked before
/// allocating).
pub fn hkdf(
    algorithm: HashAlgorithm,
    ikm: &[u8],
    salt: &[u8],
    info: &[u8],
    output_len: usize,
) -> Result<SecretBytes, Error> {
    if output_len > 255 * algorithm.output_len() {
        return Err(Error::OutputLength);
    }
    let mut output = SecretBytes::zeroed(output_len);
    // Each arm builds the extractor for one hash; the expand step is identical.
    macro_rules! expand_with {
        ($hash:ty) => {
            Hkdf::<$hash>::new(Some(salt), ikm)
                .expand(info, output.expose_mut())
                .map_err(|_| Error::OutputLength)?
        };
    }
    match algorithm {
        HashAlgorithm::Sha256 => expand_with!(Sha256),
        HashAlgorithm::Sha384 => expand_with!(Sha384),
        HashAlgorithm::Sha512 => expand_with!(Sha512),
    }
    Ok(output)
}

/// PBKDF2-HMAC-SHA1 (RFC 8018), as `gen_psk` uses it (4096 iterations, 32 bytes).
///
/// # Errors
/// [`Error::InvalidInputLength`] when `iterations` is 0.
pub fn pbkdf2_hmac_sha1(
    password: &[u8],
    salt: &[u8],
    iterations: u32,
    output_len: usize,
) -> Result<SecretBytes, Error> {
    if iterations == 0 {
        return Err(Error::InvalidInputLength);
    }
    let mut output = SecretBytes::zeroed(output_len);
    pbkdf2::pbkdf2_hmac::<Sha1>(password, salt, iterations, output.expose_mut());
    Ok(output)
}
