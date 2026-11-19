// SPDX-License-Identifier: Apache-2.0

//! AES-128-CBC with PKCS#7 padding (WSC Encrypted Settings).
//!
//! Unlike upstream's `platform_cipher_decrypt`, which loses the last `len % 16` plaintext
//! bytes (note 0006, defect 1), [`decrypt`] returns the complete plaintext. CBC is not
//! authenticated: callers must verify a MAC (the WSC Key Wrap Authenticator) over the result.
//!
//! Padding-oracle rule for WSC: verify the M2 Authenticator over the received message before
//! decrypting Encrypted Settings, so an unauthenticated sender never reaches [`decrypt`].
//! Treat [`Error::InvalidPadding`] and a Key Wrap Authenticator mismatch as one opaque
//! rejection (the same error to the peer, no timing or message difference) and end the exchange.

use aes::Aes128;
use cbc::cipher::block_padding::Pkcs7;
use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};

use crate::error::Error;
use crate::secret::SecretBytes;

/// AES-128 key length in bytes.
pub const KEY_LEN: usize = 16;
/// AES block and IV length in bytes.
pub const BLOCK_LEN: usize = 16;

/// Encrypts `plaintext` with PKCS#7 padding. The output length is the next multiple of 16 above
/// the input length, so an aligned input gets a full block of padding.
#[must_use]
pub fn encrypt(key: &[u8; KEY_LEN], iv: &[u8; BLOCK_LEN], plaintext: &[u8]) -> Vec<u8> {
    cbc::Encryptor::<Aes128>::new(key.into(), iv.into()).encrypt_padded_vec_mut::<Pkcs7>(plaintext)
}

/// Decrypts and unpads `ciphertext`. An empty plaintext is a success with an empty result.
///
/// # Errors
/// [`Error::InvalidPadding`] when the ciphertext is empty, not a multiple of 16 bytes, or its
/// final block does not unpad.
pub fn decrypt(
    key: &[u8; KEY_LEN],
    iv: &[u8; BLOCK_LEN],
    ciphertext: &[u8],
) -> Result<SecretBytes, Error> {
    // Decrypt in a wiped-on-drop copy; the error paths drop it too.
    let mut buffer = SecretBytes::from_slice(ciphertext);
    let plaintext_len = cbc::Decryptor::<Aes128>::new(key.into(), iv.into())
        .decrypt_padded_mut::<Pkcs7>(buffer.expose_mut())
        .map_err(|_| Error::InvalidPadding)?
        .len();
    buffer.truncate(plaintext_len);
    Ok(buffer)
}
