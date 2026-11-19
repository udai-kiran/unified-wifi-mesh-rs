// SPDX-License-Identifier: Apache-2.0

//! Error types for framing and building.

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
