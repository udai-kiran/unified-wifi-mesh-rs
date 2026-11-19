// SPDX-License-Identifier: Apache-2.0

//! TLV framing: one type byte, a 2-byte big-endian length, then the value.
//!
//! [`TlvIter`] follows upstream `em_msg_t::validate` (`em_msg.cpp:649-667`).
//! It borrows the input and never copies TLV values.

use em_types::TlvType;

use crate::error::FramingError;

/// Length of a TLV header (type byte and 2-byte length), in bytes.
pub(crate) const TLV_HEADER_LEN: usize = 3;

/// One TLV, borrowing its value from the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tlv<'a> {
    /// TLV type.
    pub tlv_type: TlvType,
    /// Value bytes, without the 3-byte TLV header.
    pub value: &'a [u8],
}

/// Walks the TLVs of a TLV area (the bytes after the CMDU header).
///
/// The walk ends cleanly at the first TLV of type End of Message, which is
/// not yielded. The End of Message length field and any bytes after it are
/// not checked. After one error the iterator yields nothing more.
#[derive(Debug)]
pub struct TlvIter<'a> {
    rest: &'a [u8],
    finished: bool,
}

impl<'a> TlvIter<'a> {
    /// Starts a walk over `tlv_area`.
    #[must_use]
    pub fn new(tlv_area: &'a [u8]) -> Self {
        Self {
            rest: tlv_area,
            finished: false,
        }
    }
}

impl<'a> Iterator for TlvIter<'a> {
    type Item = Result<Tlv<'a>, FramingError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        if self.rest.len() < TLV_HEADER_LEN {
            self.finished = true;
            return Some(Err(FramingError::MissingEndOfMessage));
        }
        let type_byte = self.rest[0];
        if type_byte == TlvType::END_OF_MESSAGE.0 {
            self.finished = true;
            return None;
        }
        let value_len = usize::from(u16::from_be_bytes([self.rest[1], self.rest[2]]));
        let Some(tlv_bytes) = self.rest.get(..TLV_HEADER_LEN + value_len) else {
            self.finished = true;
            return Some(Err(FramingError::TlvLengthPastEnd {
                tlv_type: type_byte,
            }));
        };
        self.rest = &self.rest[TLV_HEADER_LEN + value_len..];
        Some(Ok(Tlv {
            tlv_type: TlvType(type_byte),
            value: &tlv_bytes[TLV_HEADER_LEN..],
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bound for collecting an iterator in tests, so a runaway iterator fails the test instead of exhausting memory.
    const MAX_ITEMS: usize = 64;

    fn tlv(tlv_type: u8, value: &[u8]) -> Tlv<'_> {
        Tlv {
            tlv_type: TlvType(tlv_type),
            value,
        }
    }

    #[test]
    fn normal_walk() {
        let area = [
            0x01, 0x00, 0x02, 0xAA, 0xBB, 0x02, 0x00, 0x01, 0xCC, 0x00, 0x00, 0x00,
        ];
        let mut iter = TlvIter::new(&area);
        assert_eq!(iter.next(), Some(Ok(tlv(0x01, &[0xAA, 0xBB]))));
        assert_eq!(iter.next(), Some(Ok(tlv(0x02, &[0xCC]))));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn end_of_message_with_nonzero_length_is_accepted() {
        let area = [0x01, 0x00, 0x00, 0x00, 0x00, 0x05];
        let items: Vec<_> = TlvIter::new(&area).take(MAX_ITEMS).collect();
        assert_eq!(items, [Ok(tlv(0x01, &[]))]);
    }

    #[test]
    fn bytes_after_end_of_message_are_ignored() {
        let area = [0x01, 0x00, 0x01, 0x07, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0x09];
        let items: Vec<_> = TlvIter::new(&area).take(MAX_ITEMS).collect();
        assert_eq!(items, [Ok(tlv(0x01, &[0x07]))]);
    }

    #[test]
    fn truncated_end_of_message_is_rejected() {
        for tail in [&[0x00][..], &[0x00, 0x00][..]] {
            let mut area = vec![0x01, 0x00, 0x01, 0x07];
            area.extend_from_slice(tail);
            let items: Vec<_> = TlvIter::new(&area).take(MAX_ITEMS).collect();
            assert_eq!(
                items,
                [
                    Ok(tlv(0x01, &[0x07])),
                    Err(FramingError::MissingEndOfMessage)
                ],
                "tail of {} bytes",
                tail.len()
            );
        }
    }

    #[test]
    fn short_tail_with_nonzero_type_is_missing_end_of_message() {
        for tail in [&[0x07][..], &[0x07, 0x00][..]] {
            let mut area = vec![0x01, 0x00, 0x01, 0x07];
            area.extend_from_slice(tail);
            let mut iter = TlvIter::new(&area);
            assert_eq!(iter.next(), Some(Ok(tlv(0x01, &[0x07]))));
            assert_eq!(
                iter.next(),
                Some(Err(FramingError::MissingEndOfMessage)),
                "tail of {} bytes",
                tail.len()
            );
            assert_eq!(iter.next(), None, "tail of {} bytes", tail.len());
        }
    }

    #[test]
    fn empty_area_is_missing_end_of_message() {
        let items: Vec<_> = TlvIter::new(&[]).take(MAX_ITEMS).collect();
        assert_eq!(items, [Err(FramingError::MissingEndOfMessage)]);
    }

    #[test]
    fn no_end_of_message_at_all() {
        let area = [0x01, 0x00, 0x01, 0x07];
        let items: Vec<_> = TlvIter::new(&area).take(MAX_ITEMS).collect();
        assert_eq!(
            items,
            [
                Ok(tlv(0x01, &[0x07])),
                Err(FramingError::MissingEndOfMessage)
            ]
        );
    }

    #[test]
    fn length_past_the_buffer_reports_the_type() {
        let area = [0x01, 0x00, 0x01, 0x07, 0x42, 0x00, 0x05, 0x01, 0x02];
        let items: Vec<_> = TlvIter::new(&area).take(MAX_ITEMS).collect();
        assert_eq!(
            items,
            [
                Ok(tlv(0x01, &[0x07])),
                Err(FramingError::TlvLengthPastEnd { tlv_type: 0x42 })
            ]
        );
    }

    #[test]
    fn length_past_the_buffer_when_only_the_header_fits() {
        let area = [0x42, 0xFF, 0xFF];
        let items: Vec<_> = TlvIter::new(&area).take(MAX_ITEMS).collect();
        assert_eq!(
            items,
            [Err(FramingError::TlvLengthPastEnd { tlv_type: 0x42 })]
        );
    }

    #[test]
    fn nothing_is_yielded_after_an_error() {
        let area = [0x42, 0x00, 0x05, 0x01];
        let mut iter = TlvIter::new(&area);
        assert_eq!(
            iter.next(),
            Some(Err(FramingError::TlvLengthPastEnd { tlv_type: 0x42 }))
        );
        for _ in 0..3 {
            assert_eq!(iter.next(), None);
        }
    }

    #[test]
    fn zero_length_tlv_is_yielded() {
        let area = [0x09, 0x00, 0x00, 0x00, 0x00, 0x00];
        let items: Vec<_> = TlvIter::new(&area).take(MAX_ITEMS).collect();
        assert_eq!(items, [Ok(tlv(0x09, &[]))]);
    }

    #[test]
    fn error_is_yielded_once_then_none() {
        let area = [0x42, 0x00, 0x05, 0x01];
        assert_eq!(TlvIter::new(&area).take(MAX_ITEMS).count(), 1);
        assert_eq!(
            TlvIter::new(&area).next(),
            Some(Err(FramingError::TlvLengthPastEnd { tlv_type: 0x42 }))
        );
    }
}
