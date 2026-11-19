// SPDX-License-Identifier: Apache-2.0

//! The 8-byte CMDU header (upstream `em_cmdu_t`).
//!
//! Wire layout: byte 0 version (written 0, ignored on read), byte 1 reserved
//! (written 0, ignored), bytes 2-3 message type, bytes 4-5 message id (both
//! big-endian), byte 6 fragment id, byte 7 flags. In the flags byte the
//! last-fragment bit is `0x80` and the relay bit is `0x40`; the other bits are
//! written as 0 and ignored on read.

use em_types::MsgType;

use crate::error::FramingError;

/// Last-fragment bit in the flags byte.
const FLAG_LAST_FRAGMENT: u8 = 0x80;
/// Relay bit in the flags byte.
const FLAG_RELAY: u8 = 0x40;

/// A CMDU header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CmduHeader {
    /// Message type.
    pub msg_type: MsgType,
    /// Message id.
    pub msg_id: u16,
    /// Fragment id.
    pub frag_id: u8,
    /// Last-fragment indicator.
    pub last_fragment: bool,
    /// Relay indicator.
    pub relay: bool,
}

impl CmduHeader {
    /// Length of the header on the wire, in bytes.
    pub const LEN: usize = 8;

    /// Reads a header from the start of `bytes`.
    ///
    /// Returns the header and the bytes after it.
    ///
    /// # Errors
    ///
    /// [`FramingError::TruncatedCmduHeader`] if fewer than 8 bytes are given.
    pub fn parse(bytes: &[u8]) -> Result<(Self, &[u8]), FramingError> {
        let Some((head, rest)) = bytes.split_at_checked(Self::LEN) else {
            return Err(FramingError::TruncatedCmduHeader);
        };
        let flags = head[7];
        let header = Self {
            msg_type: MsgType(u16::from_be_bytes([head[2], head[3]])),
            msg_id: u16::from_be_bytes([head[4], head[5]]),
            frag_id: head[6],
            last_fragment: flags & FLAG_LAST_FRAGMENT != 0,
            relay: flags & FLAG_RELAY != 0,
        };
        Ok((header, rest))
    }

    /// Appends the 8 header bytes to `out`.
    pub fn write(&self, out: &mut Vec<u8>) {
        let mut flags = 0;
        if self.last_fragment {
            flags |= FLAG_LAST_FRAGMENT;
        }
        if self.relay {
            flags |= FLAG_RELAY;
        }
        out.push(0);
        out.push(0);
        out.extend_from_slice(&self.msg_type.0.to_be_bytes());
        out.extend_from_slice(&self.msg_id.to_be_bytes());
        out.push(self.frag_id);
        out.push(flags);
    }
}

/// The relay flag upstream's `add_1905_header` sets for `msg_type`.
///
/// True for exactly AP-Autoconfiguration Search, AP-Autoconfiguration Renew
/// and Topology Notification (`em_msg.cpp:513-522`). The caller decides the
/// final flag: upstream's unicast Renew sender computes it from the
/// destination MAC instead.
#[must_use]
pub fn default_relay(msg_type: MsgType) -> bool {
    msg_type == MsgType::AP_AUTOCONFIGURATION_SEARCH
        || msg_type == MsgType::AP_AUTOCONFIGURATION_RENEW
        || msg_type == MsgType::TOPOLOGY_NOTIFICATION
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(last_fragment: bool, relay: bool) -> CmduHeader {
        CmduHeader {
            msg_type: MsgType::TOPOLOGY_QUERY,
            msg_id: 0x1234,
            frag_id: 0,
            last_fragment,
            relay,
        }
    }

    fn written(header: CmduHeader) -> Vec<u8> {
        let mut out = Vec::new();
        header.write(&mut out);
        out
    }

    #[test]
    fn round_trip_for_all_flag_combinations() {
        for last_fragment in [false, true] {
            for relay in [false, true] {
                let original = header(last_fragment, relay);
                let bytes = written(original);
                assert_eq!(bytes.len(), CmduHeader::LEN);
                let (parsed, rest) = CmduHeader::parse(&bytes).unwrap();
                assert_eq!(parsed, original);
                assert_eq!(rest, []);
            }
        }
    }

    #[test]
    fn flag_bytes_are_pinned() {
        assert_eq!(written(header(true, false))[7], 0x80);
        assert_eq!(written(header(false, true))[7], 0x40);
        assert_eq!(written(header(true, true))[7], 0xC0);
        assert_eq!(written(header(false, false))[7], 0x00);
    }

    #[test]
    fn concrete_header_bytes() {
        let concrete = CmduHeader {
            msg_type: MsgType::AP_AUTOCONFIGURATION_SEARCH,
            msg_id: 0xA1B2,
            frag_id: 3,
            last_fragment: true,
            relay: true,
        };
        assert_eq!(MsgType::AP_AUTOCONFIGURATION_SEARCH.0, 0x0007);
        assert_eq!(
            written(concrete),
            [0x00, 0x00, 0x00, 0x07, 0xA1, 0xB2, 0x03, 0xC0]
        );
    }

    #[test]
    fn frag_id_round_trip() {
        for frag_id in [0, 1, 0x7F, 0x80, 0xFF] {
            let mut original = header(true, false);
            original.frag_id = frag_id;
            let (parsed, _) = CmduHeader::parse(&written(original)).unwrap();
            assert_eq!(parsed.frag_id, frag_id);
        }
    }

    #[test]
    fn parse_returns_the_bytes_after_the_header() {
        let mut bytes = written(header(true, false));
        bytes.extend_from_slice(&[9, 8, 7]);
        let (_, rest) = CmduHeader::parse(&bytes).unwrap();
        assert_eq!(rest, [9, 8, 7]);
    }

    #[test]
    fn default_relay_is_true_for_exactly_three_types() {
        let relayed = [
            MsgType::AP_AUTOCONFIGURATION_SEARCH,
            MsgType::AP_AUTOCONFIGURATION_RENEW,
            MsgType::TOPOLOGY_NOTIFICATION,
        ];
        for msg_type in relayed {
            assert!(default_relay(msg_type), "{msg_type:?} should be relayed");
        }
        for msg_type in MsgType::ALL {
            if !relayed.contains(msg_type) {
                assert!(
                    !default_relay(*msg_type),
                    "{msg_type:?} should not be relayed"
                );
            }
        }
    }

    #[test]
    fn truncated_header_is_rejected() {
        let bytes = written(header(true, false));
        for len in 0..CmduHeader::LEN {
            assert_eq!(
                CmduHeader::parse(&bytes[..len]),
                Err(FramingError::TruncatedCmduHeader),
                "length {len}"
            );
        }
    }

    #[test]
    fn version_reserved_and_other_flag_bits_are_ignored() {
        let bytes = [0x05, 0xFF, 0x00, 0x02, 0x00, 0x09, 0x04, 0x3F];
        let (parsed, rest) = CmduHeader::parse(&bytes).unwrap();
        assert_eq!(parsed.msg_type, MsgType::TOPOLOGY_QUERY);
        assert_eq!(parsed.msg_id, 9);
        assert_eq!(parsed.frag_id, 4);
        assert!(!parsed.last_fragment);
        assert!(!parsed.relay);
        assert_eq!(rest, []);
    }
}
