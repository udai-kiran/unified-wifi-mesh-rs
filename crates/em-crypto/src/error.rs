// SPDX-License-Identifier: Apache-2.0

//! The error type shared by all modules. Messages never contain secret bytes.

use core::fmt;

use crate::rand::RandomError;

/// Why a primitive refused to run or failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The random source failed.
    Random(RandomError),
    /// A key, KEK or MAC key has an unsupported length.
    InvalidKeyLength,
    /// An input has a length the operation does not accept.
    InvalidInputLength,
    /// The requested output length is not supported (too long, or empty where upstream refuses).
    OutputLength,
    /// A KDF label or boot-key prefix contains a NUL byte (upstream treats these as C strings
    /// and would truncate them).
    InvalidLabel,
    /// A tag, key-wrap integrity check or other authenticator did not verify.
    AuthenticationFailed,
    /// CBC padding is malformed (or the ciphertext length is not a positive block multiple).
    InvalidPadding,
    /// Text is not valid base64 (or base64url) under the accepted rules.
    InvalidEncoding,
    /// A Diffie-Hellman value is outside `2 ..= p - 2`.
    ValueOutOfRange,
    /// The curve is not P-256, P-384 or P-521.
    UnsupportedCurve,
    /// Two EC values belong to different curves.
    CurveMismatch,
    /// A point has the wrong length, is not on the curve, or is the point at infinity.
    InvalidPoint,
    /// A scalar has the wrong length, is zero where that is refused, or is not below the group
    /// order.
    InvalidScalar,
    /// A `SubjectPublicKeyInfo` is malformed or does not hold an EC public key.
    InvalidSpki,
    /// An ECDSA signature is malformed or does not verify.
    InvalidSignature,
    /// Rejection sampling did not find an acceptable value (a broken random source).
    SamplingExhausted,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Error::Random(_) => "random source failure",
            Error::InvalidKeyLength => "unsupported key length",
            Error::InvalidInputLength => "unsupported input length",
            Error::OutputLength => "unsupported output length",
            Error::InvalidLabel => "label contains a NUL byte",
            Error::AuthenticationFailed => "authentication failed",
            Error::InvalidPadding => "invalid padding",
            Error::InvalidEncoding => "invalid encoding",
            Error::ValueOutOfRange => "value out of range",
            Error::UnsupportedCurve => "unsupported curve",
            Error::CurveMismatch => "curve mismatch",
            Error::InvalidPoint => "invalid point",
            Error::InvalidScalar => "invalid scalar",
            Error::InvalidSpki => "invalid SubjectPublicKeyInfo",
            Error::InvalidSignature => "invalid signature",
            Error::SamplingExhausted => "rejection sampling exhausted",
        };
        f.write_str(text)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Random(source) => Some(source),
            _ => None,
        }
    }
}

impl From<RandomError> for Error {
    fn from(error: RandomError) -> Self {
        Error::Random(error)
    }
}
