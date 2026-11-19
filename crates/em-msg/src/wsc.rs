// SPDX-License-Identifier: Apache-2.0

//! WSC Message Type selector (upstream `em_configuration_t::get_wsc_msg_type`,
//! `em_configuration.cpp:6053-6092`).
//!
//! An AP-Autoconfiguration WSC message carries a WSC TLV whose attributes
//! (2-byte id, 2-byte length, value; big-endian) include the Message Type
//! attribute `0x1022`. Its first value byte says M1 or M2.

use em_types::TlvType;

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
}
