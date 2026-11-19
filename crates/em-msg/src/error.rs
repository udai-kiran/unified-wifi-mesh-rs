// SPDX-License-Identifier: Apache-2.0

//! Error types for framing, building and TLV body decoding and encoding.

use core::fmt;

/// A CMDU or its TLV area is malformed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramingError {
    /// Fewer than 8 bytes, so there is no complete CMDU header.
    TruncatedCmduHeader,
    /// A TLV declares a length that runs past the end of the buffer.
    TlvLengthPastEnd {
        /// Type byte of the offending TLV.
        tlv_type: u8,
    },
    /// The buffer ends before a complete End of Message TLV header (3 bytes).
    MissingEndOfMessage,
}

impl fmt::Display for FramingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedCmduHeader => write!(f, "the CMDU header is shorter than 8 bytes"),
            Self::TlvLengthPastEnd { tlv_type } => {
                write!(
                    f,
                    "the length of TLV type 0x{tlv_type:02X} runs past the end of the message"
                )
            }
            Self::MissingEndOfMessage => write!(f, "the End of Message TLV is missing"),
        }
    }
}

impl std::error::Error for FramingError {}

/// A message could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildError {
    /// The message does not fit in the size budget.
    OverBudget,
    /// A TLV value is longer than 65535 bytes.
    TlvTooLong,
    /// End of Message cannot be added with `tlv`; `finish` adds it.
    EndOfMessageNotAllowed,
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OverBudget => write!(f, "the message does not fit in the size budget"),
            Self::TlvTooLong => write!(f, "a TLV value is longer than 65535 bytes"),
            Self::EndOfMessageNotAllowed => {
                write!(
                    f,
                    "End of Message is added by finish and cannot be added as a TLV"
                )
            }
        }
    }
}

impl std::error::Error for BuildError {}

/// A TLV value could not be decoded.
///
/// Kinds are added per body when a body needs one (for example an invalid
/// enumerated value); callers match on the kind, never on message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// A read needed more bytes than were left.
    Truncated {
        /// Number of bytes the read asked for.
        needed: usize,
        /// Number of bytes that were left.
        remaining: usize,
    },
    /// The body was decoded completely but bytes were left over.
    TrailingBytes {
        /// Number of bytes left over.
        remaining: usize,
    },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated { needed, remaining } => {
                write!(
                    f,
                    "the TLV value is truncated: {needed} bytes needed, {remaining} left"
                )
            }
            Self::TrailingBytes { remaining } => {
                write!(f, "the TLV value has {remaining} unexpected trailing bytes")
            }
        }
    }
}

impl std::error::Error for DecodeError {}

/// A TLV value could not be encoded.
///
/// Kinds are added per body when a body needs one; callers match on the
/// kind, never on message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeError {
    /// A collection is longer than its count field can hold.
    CountTooLarge {
        /// Length of the collection.
        len: usize,
        /// Largest count the field can hold.
        max: usize,
    },
    /// A value does not fit in its field.
    ValueTooLarge {
        /// The value that was given.
        value: u32,
        /// Largest value the field can hold.
        max: u32,
    },
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CountTooLarge { len, max } => {
                write!(
                    f,
                    "a collection of {len} items does not fit in its count field (largest count {max})"
                )
            }
            Self::ValueTooLarge { value, max } => {
                write!(
                    f,
                    "the value {value} does not fit in its field (largest value {max})"
                )
            }
        }
    }
}

impl std::error::Error for EncodeError {}
