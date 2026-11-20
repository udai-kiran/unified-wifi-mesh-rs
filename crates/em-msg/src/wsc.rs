// SPDX-License-Identifier: Apache-2.0

//! WSC Message Type selector (upstream `em_configuration_t::get_wsc_msg_type`,
//! `em_configuration.cpp:6053-6092`).
//!
//! An AP-Autoconfiguration WSC message carries a WSC TLV whose attributes
//! (2-byte id, 2-byte length, value; big-endian) include the Message Type
//! attribute `0x1022`. Its first value byte says M1 or M2.
//!
//! The module also has the attribute walker ([`WscAttrIter`]) and writer
//! ([`write_wsc_attr`]) that the WSC bodies are built on (note 0009, D5).

use em_types::TlvType;

use crate::body::{Reader, Writer};
use crate::error::{DecodeError, EncodeError};
use crate::tlv::TlvIter;

/// WSC message type M1 (`em_wsc_msg_type_m1`).
pub const WSC_M1: u8 = 4;
/// WSC message type M2 (`em_wsc_msg_type_m2`).
pub const WSC_M2: u8 = 5;

/// Attribute id of the WSC Message Type attribute (`attr_id_msg_type`).
const ATTR_ID_MSG_TYPE: u16 = 0x1022;
/// Length of an attribute header (id and length), in bytes.
const ATTR_HEADER_LEN: usize = 4;

/// Returns the first WSC Message Type value found in `tlv_area`.
///
/// The walk follows upstream: TLVs are walked until End of Message, and a TLV
/// whose length runs past the buffer ends the walk with `None`. In each WSC
/// TLV the attributes are walked; an attribute that runs past its TLV ends
/// the walk of that TLV only. The first Message Type attribute with a value of
/// at least one byte wins; one with length 0 is skipped.
#[must_use]
pub fn wsc_msg_type(tlv_area: &[u8]) -> Option<u8> {
    for item in TlvIter::new(tlv_area) {
        let Ok(tlv) = item else {
            return None;
        };
        if tlv.tlv_type == TlvType::WSC {
            if let Some(msg_type) = attribute_msg_type(tlv.value) {
                return Some(msg_type);
            }
        }
    }
    None
}

/// One WSC attribute: id and value, without the 4-byte attribute header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WscAttr<'a> {
    /// Attribute id (big-endian on the wire).
    pub id: u16,
    /// Value bytes.
    pub value: &'a [u8],
}

/// Walks the attributes of a WSC TLV value (or of decrypted settings): 2-byte id,
/// 2-byte length, value, all big-endian.
///
/// An empty input yields nothing. An attribute whose header or value is cut
/// short yields [`DecodeError::Truncated`]; after an error the iterator yields
/// nothing more. Unlike [`wsc_msg_type`], which follows upstream and ends a
/// walk quietly at a short attribute, this walker reports the error.
#[derive(Debug)]
pub struct WscAttrIter<'a> {
    reader: Reader<'a>,
    finished: bool,
}

impl<'a> WscAttrIter<'a> {
    /// Starts a walk over `value`.
    #[must_use]
    pub fn new(value: &'a [u8]) -> Self {
        Self {
            reader: Reader::new(value),
            finished: false,
        }
    }

    fn read_attr(&mut self) -> Result<WscAttr<'a>, DecodeError> {
        let id = self.reader.be_u16()?;
        let len = usize::from(self.reader.be_u16()?);
        let value = self.reader.bytes(len)?;
        Ok(WscAttr { id, value })
    }
}

impl<'a> Iterator for WscAttrIter<'a> {
    type Item = Result<WscAttr<'a>, DecodeError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished || self.reader.remaining() == 0 {
            return None;
        }
        let result = self.read_attr();
        if result.is_err() {
            self.finished = true;
        }
        Some(result)
    }
}

/// Writes one WSC attribute: id, length, value.
///
/// # Errors
///
/// [`EncodeError::CountTooLarge`] if `value` is longer than 65535 bytes; nothing is written.
pub fn write_wsc_attr(writer: &mut Writer, id: u16, value: &[u8]) -> Result<(), EncodeError> {
    let Ok(len) = u16::try_from(value.len()) else {
        return Err(EncodeError::CountTooLarge {
            len: value.len(),
            max: usize::from(u16::MAX),
        });
    };
    writer.be_u16(id);
    writer.be_u16(len);
    writer.bytes(value);
    Ok(())
}

/// Looks for the Message Type attribute in the value of one WSC TLV.
fn attribute_msg_type(value: &[u8]) -> Option<u8> {
    let mut rest = value;
    while rest.len() >= ATTR_HEADER_LEN {
        let id = u16::from_be_bytes([rest[0], rest[1]]);
        let len = usize::from(u16::from_be_bytes([rest[2], rest[3]]));
        let attribute = rest.get(ATTR_HEADER_LEN..ATTR_HEADER_LEN + len)?;
        if id == ATTR_ID_MSG_TYPE {
            if let Some(first) = attribute.first() {
                return Some(*first);
            }
        }
        rest = &rest[ATTR_HEADER_LEN + len..];
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One attribute: id, length, value.
    fn attr(id: u16, value: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&id.to_be_bytes());
        out.extend_from_slice(&u16::try_from(value.len()).unwrap().to_be_bytes());
        out.extend_from_slice(value);
        out
    }

    /// One TLV: type, length, value.
    fn tlv(tlv_type: TlvType, value: &[u8]) -> Vec<u8> {
        let mut out = vec![tlv_type.0];
        out.extend_from_slice(&u16::try_from(value.len()).unwrap().to_be_bytes());
        out.extend_from_slice(value);
        out
    }

    fn end() -> Vec<u8> {
        vec![0, 0, 0]
    }

    fn area(parts: &[Vec<u8>]) -> Vec<u8> {
        parts.concat()
    }

    #[test]
    fn constants() {
        assert_eq!(WSC_M1, 4);
        assert_eq!(WSC_M2, 5);
    }

    #[test]
    fn m1_and_m2_are_found() {
        for wanted in [WSC_M1, WSC_M2] {
            let attrs = [attr(0x104A, &[0x10]), attr(0x1022, &[wanted])].concat();
            let bytes = area(&[tlv(TlvType::WSC, &attrs), end()]);
            assert_eq!(wsc_msg_type(&bytes), Some(wanted));
        }
    }

    #[test]
    fn missing_attribute_gives_none() {
        let bytes = area(&[tlv(TlvType::WSC, &attr(0x104A, &[0x10])), end()]);
        assert_eq!(wsc_msg_type(&bytes), None);
    }

    #[test]
    fn no_wsc_tlv_gives_none() {
        let bytes = area(&[tlv(TlvType(0x01), &attr(0x1022, &[4])), end()]);
        assert_eq!(wsc_msg_type(&bytes), None);
    }

    #[test]
    fn zero_length_attribute_is_skipped() {
        let attrs = [attr(0x1022, &[]), attr(0x1022, &[WSC_M2])].concat();
        let bytes = area(&[tlv(TlvType::WSC, &attrs), end()]);
        assert_eq!(wsc_msg_type(&bytes), Some(WSC_M2));
    }

    #[test]
    fn only_a_zero_length_attribute_gives_none() {
        let bytes = area(&[tlv(TlvType::WSC, &attr(0x1022, &[])), end()]);
        assert_eq!(wsc_msg_type(&bytes), None);
    }

    #[test]
    fn overrun_inside_one_wsc_tlv_does_not_stop_the_next_wsc_tlv() {
        // The attribute claims 9 value bytes but the TLV holds only 1.
        let broken = [0x10, 0x22, 0x00, 0x09, 0x04];
        let good = attr(0x1022, &[WSC_M1]);
        let bytes = area(&[tlv(TlvType::WSC, &broken), tlv(TlvType::WSC, &good), end()]);
        assert_eq!(wsc_msg_type(&bytes), Some(WSC_M1));
    }

    #[test]
    fn overrun_attribute_value_is_not_returned_but_the_next_tlv_is() {
        // The malformed attribute holds M2 but claims 9 bytes; the valid one holds M1.
        let broken = [0x10, 0x22, 0x00, 0x09, WSC_M2];
        let good = attr(0x1022, &[WSC_M1]);
        let bytes = area(&[tlv(TlvType::WSC, &broken), tlv(TlvType::WSC, &good), end()]);
        assert_eq!(wsc_msg_type(&bytes), Some(WSC_M1));
    }

    #[test]
    fn first_message_type_attribute_in_one_wsc_tlv_wins() {
        let attrs = [attr(0x1022, &[0x09]), attr(0x1022, &[WSC_M1])].concat();
        let bytes = area(&[tlv(TlvType::WSC, &attrs), end()]);
        assert_eq!(wsc_msg_type(&bytes), Some(0x09));
    }

    #[test]
    fn first_wsc_tlv_with_a_message_type_wins() {
        // An unrecognised value in the first TLV still beats M1 in a later one.
        let first = tlv(TlvType::WSC, &attr(0x1022, &[0x09]));
        let second = tlv(TlvType::WSC, &attr(0x1022, &[WSC_M1]));
        let bytes = area(&[first, second, end()]);
        assert_eq!(wsc_msg_type(&bytes), Some(0x09));
    }

    #[test]
    fn tlv_overrun_gives_none() {
        // The second TLV claims 9 bytes but only 1 remains; its type is WSC and would match.
        let mut bytes = tlv(TlvType(0x01), &[]);
        bytes.extend_from_slice(&[TlvType::WSC.0, 0x00, 0x09, 0x10]);
        assert_eq!(wsc_msg_type(&bytes), None);
    }

    #[test]
    fn match_before_a_later_tlv_overrun_is_still_returned() {
        let mut bytes = tlv(TlvType::WSC, &attr(0x1022, &[WSC_M1]));
        bytes.extend_from_slice(&[0x01, 0x00, 0x09]);
        assert_eq!(wsc_msg_type(&bytes), Some(WSC_M1));
    }

    #[test]
    fn non_wsc_tlvs_are_ignored() {
        let other = tlv(TlvType(0x01), &attr(0x1022, &[WSC_M1]));
        let wsc = tlv(TlvType::WSC, &attr(0x1022, &[WSC_M2]));
        let bytes = area(&[other, wsc, end()]);
        assert_eq!(wsc_msg_type(&bytes), Some(WSC_M2));
    }

    #[test]
    fn walk_stops_at_end_of_message() {
        let wsc = tlv(TlvType::WSC, &attr(0x1022, &[WSC_M2]));
        let bytes = area(&[end(), wsc]);
        assert_eq!(wsc_msg_type(&bytes), None);
    }

    #[test]
    fn other_values_are_returned_as_is() {
        for value in [0u8, 1, 6, 0xFF] {
            let bytes = area(&[tlv(TlvType::WSC, &attr(0x1022, &[value])), end()]);
            assert_eq!(wsc_msg_type(&bytes), Some(value));
        }
    }

    #[test]
    fn only_the_first_value_byte_is_used() {
        let bytes = area(&[tlv(TlvType::WSC, &attr(0x1022, &[WSC_M2, WSC_M1])), end()]);
        assert_eq!(wsc_msg_type(&bytes), Some(WSC_M2));
    }

    // The attribute walker and writer.

    fn walk(value: &[u8]) -> Vec<Result<WscAttr<'_>, DecodeError>> {
        WscAttrIter::new(value).take(MAX_ITEMS).collect()
    }

    /// Bound for collecting an iterator in tests, so a runaway iterator fails the test instead of exhausting memory.
    const MAX_ITEMS: usize = 64;

    #[test]
    fn empty_input_has_no_attributes() {
        assert_eq!(walk(&[]).len(), 0);
    }

    #[test]
    fn attributes_round_trip_through_the_writer() {
        let mut writer = Writer::new();
        write_wsc_attr(&mut writer, 0x1022, &[0x04]).unwrap();
        write_wsc_attr(&mut writer, 0x104A, &[0x10]).unwrap();
        write_wsc_attr(&mut writer, 0x1021, b"vendor").unwrap();
        let bytes = writer.into_bytes();
        assert_eq!(bytes, attr_bytes_for_round_trip());
        assert_eq!(
            walk(&bytes),
            [
                Ok(WscAttr {
                    id: 0x1022,
                    value: &[0x04]
                }),
                Ok(WscAttr {
                    id: 0x104A,
                    value: &[0x10]
                }),
                Ok(WscAttr {
                    id: 0x1021,
                    value: b"vendor"
                }),
            ]
        );
    }

    fn attr_bytes_for_round_trip() -> Vec<u8> {
        let mut bytes = attr(0x1022, &[0x04]);
        bytes.extend(attr(0x104A, &[0x10]));
        bytes.extend(attr(0x1021, b"vendor"));
        bytes
    }

    #[test]
    fn zero_length_attribute_is_yielded() {
        let mut bytes = attr(0x1022, &[]);
        bytes.extend(attr(0x104A, &[0x10]));
        assert_eq!(
            walk(&bytes),
            [
                Ok(WscAttr {
                    id: 0x1022,
                    value: &[]
                }),
                Ok(WscAttr {
                    id: 0x104A,
                    value: &[0x10]
                }),
            ]
        );
    }

    #[test]
    fn partial_header_is_truncated() {
        let whole = attr(0x1022, &[0x04]);
        let expected = [
            DecodeError::Truncated {
                needed: 2,
                remaining: 1,
            },
            DecodeError::Truncated {
                needed: 2,
                remaining: 0,
            },
            DecodeError::Truncated {
                needed: 2,
                remaining: 1,
            },
        ];
        for (index, error) in expected.iter().enumerate() {
            let header_len = index + 1;
            assert_eq!(
                walk(&whole[..header_len]),
                [Err(*error)],
                "header of {header_len} bytes"
            );
        }
    }

    #[test]
    fn partial_header_after_a_good_attribute_is_truncated() {
        let mut bytes = attr(0x1022, &[0x04]);
        bytes.push(0x10);
        assert_eq!(
            walk(&bytes),
            [
                Ok(WscAttr {
                    id: 0x1022,
                    value: &[0x04]
                }),
                Err(DecodeError::Truncated {
                    needed: 2,
                    remaining: 1
                }),
            ]
        );
    }

    #[test]
    fn length_past_the_end_is_truncated() {
        let mut bytes = attr(0x1022, &[0x04, 0x05, 0x06]);
        bytes.pop();
        assert_eq!(
            walk(&bytes),
            [Err(DecodeError::Truncated {
                needed: 3,
                remaining: 2
            })]
        );
        // A header that promises 65535 bytes of nothing.
        assert_eq!(
            walk(&[0x10, 0x22, 0xFF, 0xFF]),
            [Err(DecodeError::Truncated {
                needed: 65535,
                remaining: 0
            })]
        );
    }

    #[test]
    fn nothing_is_yielded_after_an_error() {
        let bytes = [0x10, 0x22, 0x00, 0x05, 0x01];
        let mut iter = WscAttrIter::new(&bytes);
        assert!(matches!(iter.next(), Some(Err(_))));
        for _ in 0..3 {
            assert_eq!(iter.next(), None);
        }
    }

    #[test]
    fn writer_accepts_65535_bytes_and_refuses_65536() {
        let value = vec![0x5A; 65535];
        let mut writer = Writer::new();
        write_wsc_attr(&mut writer, 0x1001, &value).unwrap();
        let bytes = writer.into_bytes();
        assert_eq!(bytes.len(), 4 + 65535);
        assert_eq!(bytes[..4], [0x10, 0x01, 0xFF, 0xFF]);
        assert_eq!(
            walk(&bytes),
            [Ok(WscAttr {
                id: 0x1001,
                value: &value
            })]
        );

        let too_long = vec![0u8; 65536];
        let mut writer = Writer::new();
        writer.u8(0xEE);
        assert_eq!(
            write_wsc_attr(&mut writer, 0x1001, &too_long),
            Err(EncodeError::CountTooLarge {
                len: 65536,
                max: 65535
            })
        );
        assert_eq!(writer.into_bytes(), [0xEE]);
    }
}
