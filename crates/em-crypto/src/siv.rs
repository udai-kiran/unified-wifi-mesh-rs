// SPDX-License-Identifier: Apache-2.0

//! AES-SIV (RFC 5297) with an ordered list of associated-data pieces.
//!
//! Keys are 32, 48 or 64 bytes (two AES-128, AES-192 or AES-256 keys). Both directions use the
//! layout `tag (16 bytes) || ciphertext`, as the DPP wrapped-data wrapper does.

use aes::{Aes128, Aes192, Aes256};
use aes_siv::KeyInit;
use aes_siv::siv::{CmacSiv, MAX_HEADERS};

use crate::error::Error;
use crate::secret::SecretBytes;

/// Length of the synthetic IV (the tag) in bytes.
pub const TAG_LEN: usize = 16;

/// Builds the SIV cipher for the key length and runs `$body` with it as `$siv`.
macro_rules! with_siv {
    ($key:expr, $siv:ident => $body:expr) => {
        match $key.len() {
            32 => {
                let mut $siv =
                    CmacSiv::<Aes128>::new_from_slice($key).map_err(|_| Error::InvalidKeyLength)?;
                $body
            }
            48 => {
                let mut $siv =
                    CmacSiv::<Aes192>::new_from_slice($key).map_err(|_| Error::InvalidKeyLength)?;
                $body
            }
            64 => {
                let mut $siv =
                    CmacSiv::<Aes256>::new_from_slice($key).map_err(|_| Error::InvalidKeyLength)?;
                $body
            }
            _ => Err(Error::InvalidKeyLength),
        }
    };
}

/// Encrypts `plaintext`, authenticating `associated_data` (in order). Returns
/// `tag || ciphertext`.
///
/// # Errors
/// [`Error::InvalidKeyLength`] for a key that is not 32, 48 or 64 bytes, or
/// [`Error::InvalidInputLength`] for more associated-data pieces than the cipher supports.
pub fn encrypt(key: &[u8], associated_data: &[&[u8]], plaintext: &[u8]) -> Result<Vec<u8>, Error> {
    if associated_data.len() > MAX_HEADERS {
        return Err(Error::InvalidInputLength);
    }
    with_siv!(key, siv => siv
        .encrypt(associated_data.iter().copied(), plaintext)
        .map_err(|_| Error::InvalidInputLength))
}

/// Verifies and decrypts `tag_and_ciphertext` (`tag || ciphertext`).
///
/// # Errors
/// [`Error::InvalidKeyLength`], [`Error::InvalidInputLength`] when the input is shorter than the
/// tag or has too many associated-data pieces, or [`Error::AuthenticationFailed`] when the tag
/// does not verify (no plaintext is returned).
pub fn decrypt(
    key: &[u8],
    associated_data: &[&[u8]],
    tag_and_ciphertext: &[u8],
) -> Result<SecretBytes, Error> {
    if tag_and_ciphertext.len() < TAG_LEN || associated_data.len() > MAX_HEADERS {
        return Err(Error::InvalidInputLength);
    }
    // Decrypt in a wiped-on-drop buffer; a failed check drops it before returning.
    let mut buffer = SecretBytes::from_slice(tag_and_ciphertext);
    with_siv!(key, siv => siv
        .decrypt_in_place(associated_data.iter().copied(), buffer.as_vec_mut())
        .map_err(|_| Error::AuthenticationFailed))?;
    Ok(buffer)
}
