// SPDX-License-Identifier: Apache-2.0

//! base64 and base64url.
//!
//! Encoding is strict: standard alphabet with padding, or URL-safe alphabet without padding.
//! [`base64url_decode`] is lenient in the way upstream's decoder is (note 0006, D3): it accepts
//! both alphabets and adds missing padding. Decoding is otherwise strict: canonical
//! trailing bits, no whitespace; inputs outside the pinned fixtures can therefore be rejected
//! where upstream's OpenSSL decoder accepts them.

use base64ct::{Base64, Base64UrlUnpadded, Encoding};

use crate::error::Error;

/// Upstream passes the text to OpenSSL as a C string, so decoding stops at the first NUL.
fn up_to_first_nul(input: &str) -> &str {
    input.split('\0').next().unwrap_or_default()
}

/// Standard base64 with padding. Empty input gives an empty string.
#[must_use]
pub fn base64_encode(input: &[u8]) -> String {
    Base64::encode_string(input)
}

/// Strict standard base64 (padding required). Text after a NUL is ignored, as upstream does.
/// Empty input gives empty output.
///
/// # Errors
/// [`Error::InvalidEncoding`].
pub fn base64_decode(input: &str) -> Result<Vec<u8>, Error> {
    let text = up_to_first_nul(input);
    if text.is_empty() {
        return Ok(Vec::new());
    }
    Base64::decode_vec(text).map_err(|_| Error::InvalidEncoding)
}

/// URL-safe base64 without padding. Empty input gives an empty string.
#[must_use]
pub fn base64url_encode(input: &[u8]) -> String {
    Base64UrlUnpadded::encode_string(input)
}

/// Lenient base64url decoding: `-` and `_` map to `+` and `/`, `+` and `/` stay valid, missing
/// padding is added and existing padding is accepted. Text after a NUL is ignored. Empty input
/// gives empty output.
///
/// # Errors
/// [`Error::InvalidEncoding`], for example for a length of 1 modulo 4 or too much padding.
pub fn base64url_decode(input: &str) -> Result<Vec<u8>, Error> {
    let text = up_to_first_nul(input);
    let mut standard: String = text
        .chars()
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            other => other,
        })
        .collect();
    let missing = (4 - standard.len() % 4) % 4;
    standard.extend(core::iter::repeat_n('=', missing));
    base64_decode(&standard)
}
