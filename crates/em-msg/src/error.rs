// SPDX-License-Identifier: Apache-2.0

//! Error types for framing, building, TLV body decoding and encoding, and the field readers.

use core::fmt;

use em_types::TlvType;

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
    /// A TLV body could not be encoded (see [`MsgBuilder::body`](crate::MsgBuilder::body)).
    Encode(EncodeError),
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
            Self::Encode(error) => write!(f, "a TLV body could not be encoded: {error}"),
        }
    }
}

impl std::error::Error for BuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Encode(error) => Some(error),
            Self::OverBudget | Self::TlvTooLong | Self::EndOfMessageNotAllowed => None,
        }
    }
}

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
    /// The TLV is not of the type the body is decoded as.
    WrongType {
        /// Type of the body that was asked for.
        expected: TlvType,
        /// Type of the TLV that was given.
        found: TlvType,
    },
    /// A length or set-count field, or the length of the whole value, cannot describe the bytes.
    /// For example, a transmitter link metric value of 12 octets holds two MACs and no pairs.
    InvalidLength {
        /// The value of the field, or the length of the whole value.
        len: usize,
    },
    /// A field value that changes the layout is outside the range the body accepts.
    ValueOutOfRange {
        /// The value that was read.
        value: u32,
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
            Self::WrongType { expected, found } => {
                write!(f, "expected a TLV of type {expected:?}, found {found:?}")
            }
            Self::InvalidLength { len } => {
                write!(f, "the length field value {len} does not fit the TLV value")
            }
            Self::ValueOutOfRange { value } => {
                write!(f, "the field value {value} is out of range")
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
    /// A collection is shorter than the body requires, so the bytes would not decode strictly.
    CountTooSmall {
        /// Length of the collection.
        len: usize,
        /// Smallest count the body accepts.
        min: usize,
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
            Self::CountTooSmall { len, min } => {
                write!(
                    f,
                    "a collection of {len} items is below the smallest count {min} the body accepts"
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

/// A field reader (note 0009 D6, [`readers`](crate::readers)) could not read its field.
///
/// `Ok(None)` from a reader is upstream's "not found". This error is for a frame or a value
/// that upstream either rejects through its own check or reads past the end of. Callers match
/// on the kind, never on message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderError {
    /// The TLV area is malformed; the walk stopped at the first framing error.
    Framing(FramingError),
    /// The value is shorter than the read needs, where upstream would read past it.
    ValueTooShort {
        /// Type of the TLV whose value is too short.
        tlv_type: TlvType,
        /// Number of octets the read needs, counted from the start of the value.
        needed: usize,
        /// Length of the value.
        found: usize,
    },
    /// The length of the value is rejected by upstream's own check, or is not the exact length
    /// the reader requires.
    InvalidLength {
        /// Type of the TLV whose value has the wrong length.
        tlv_type: TlvType,
        /// Length of the value.
        len: usize,
    },
    /// A field of the value is rejected by upstream's own check.
    ValueOutOfRange {
        /// Type of the TLV whose value is rejected.
        tlv_type: TlvType,
        /// The field value.
        value: u8,
    },
}

impl From<FramingError> for ReaderError {
    fn from(error: FramingError) -> Self {
        Self::Framing(error)
    }
}

impl fmt::Display for ReaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Framing(error) => write!(f, "the TLV area is malformed: {error}"),
            Self::ValueTooShort {
                tlv_type,
                needed,
                found,
            } => write!(
                f,
                "the value of TLV type 0x{:02X} is too short: {needed} octets needed, {found} found",
                tlv_type.0
            ),
            Self::InvalidLength { tlv_type, len } => write!(
                f,
                "the value of TLV type 0x{:02X} has the invalid length {len}",
                tlv_type.0
            ),
            Self::ValueOutOfRange { tlv_type, value } => write!(
                f,
                "the field value {value} in TLV type 0x{:02X} is out of range",
                tlv_type.0
            ),
        }
    }
}

impl std::error::Error for ReaderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Framing(error) => Some(error),
            Self::ValueTooShort { .. }
            | Self::InvalidLength { .. }
            | Self::ValueOutOfRange { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_kinds_have_readable_messages() {
        let wrong_type = DecodeError::WrongType {
            expected: TlvType::WSC,
            found: TlvType(0xF8),
        };
        assert_eq!(
            wrong_type.to_string(),
            "expected a TLV of type WSC(0x11), found TlvType(0xF8)"
        );
        assert_eq!(
            DecodeError::InvalidLength { len: 5 }.to_string(),
            "the length field value 5 does not fit the TLV value"
        );
        assert_eq!(
            DecodeError::ValueOutOfRange { value: 9 }.to_string(),
            "the field value 9 is out of range"
        );
        let encode = BuildError::Encode(EncodeError::CountTooLarge { len: 256, max: 255 });
        assert_eq!(
            encode.to_string(),
            "a TLV body could not be encoded: a collection of 256 items does not fit in its count field (largest count 255)"
        );
    }

    #[test]
    fn count_too_small_has_a_readable_message() {
        assert_eq!(
            EncodeError::CountTooSmall { len: 0, min: 1 }.to_string(),
            "a collection of 0 items is below the smallest count 1 the body accepts"
        );
    }

    #[test]
    fn build_error_encode_has_the_encode_error_as_source() {
        use std::error::Error;
        let inner = EncodeError::ValueTooLarge { value: 2, max: 1 };
        assert!(BuildError::Encode(inner).source().is_some());
        assert!(BuildError::OverBudget.source().is_none());
    }
}
