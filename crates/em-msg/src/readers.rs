// SPDX-License-Identifier: Apache-2.0

//! Field readers over a TLV area (note 0009 D6, amendment A10).
//!
//! These are the message-level helpers of upstream `em_msg_t` (`em_msg.cpp:71-480`). They read
//! one field from the first matching TLV in frame order, with no priority by type. The input is
//! the TLV area that [`CmduHeader::parse`](crate::CmduHeader::parse) returns, with Ethernet
//! padding allowed after End of Message.
//!
//! Readers do not use the typed bodies, so their offsets stay upstream's even where a body
//! follows the spec better.
//!
//! The walk is [`TlvIter`] and stops at the first framing error. Where upstream reads past the
//! end of a value or returns `false` after its own check on a malformed value, the reader
//! returns a [`ReaderError`]. `Ok(None)` is upstream's "not found".
//!
//! [`first_tlv`] and [`next_tlv`] are the offset-addressed walker of `get_first_tlv` and
//! `get_next_tlv`. Unlike [`TlvIter`], they stop at any TLV with a zero length.

use core::fmt;

use em_types::{FreqBand, MacAddr, ProfileType, TlvType};

use crate::error::{FramingError, ReaderError};
use crate::tlv::{TLV_HEADER_LEN, Tlv, TlvIter};

/// Length of a MAC address, in octets.
const MAC_LEN: usize = 6;

/// `EM_MAX_SERVICE` (`em_base.h:87`): the most services a Supported Service TLV can list.
const MAX_SERVICES: u8 = 8;

/// `em_profile_type_max` (`em_base.h:493-499`): raw Profile values from here up are rejected.
const PROFILE_TYPE_MAX: u8 = 4;

/// `sizeof(em_channel_scan_req_t)` (`em_base.h:1121-1128`): the smallest Channel Scan Request
/// value `get_radio_id` accepts, apart from the exact 2-octet value with no radios.
const CHANNEL_SCAN_REQUEST_MIN_LEN: usize = 9;

/// Finds the first TLV of `tlv_type` and returns its value.
///
/// Upstream `get_tlv(em_tlv_type_t)` (`em_msg.cpp:387-402`) and the static
/// `get_tlv(em_tlv_t*, unsigned int, em_tlv_type_t)` (`em_msg.cpp:404-422`) return the TLV
/// pointer; this returns the value, which starts 3 octets after upstream's pointer. End of
/// Message is never found, as upstream.
///
/// # Errors
///
/// The [`FramingError`] at which the walk stopped, if the TLV was not found before it.
/// Upstream reads past the end of the buffer instead, or returns "not found" when the frame has
/// no End of Message.
pub fn find_tlv(area: &[u8], tlv_type: TlvType) -> Result<Option<&[u8]>, FramingError> {
    for item in TlvIter::new(area) {
        let tlv = item?;
        if tlv.tlv_type == tlv_type {
            return Ok(Some(tlv.value));
        }
    }
    Ok(None)
}

/// Reads the AL MAC address of the first AL MAC Address TLV.
///
/// Upstream `get_al_mac_address` (`em_msg.cpp:71-87`) copies `htons(tlv->len)` octets into a
/// 6-octet buffer, which overflows for a longer value and leaves the rest unset for a shorter
/// one. The value must be exactly 6 octets (note 0009 D6).
///
/// # Errors
///
/// A [`ReaderError`] for a framing error, or [`ReaderError::InvalidLength`] if the value is not
/// exactly 6 octets.
pub fn al_mac_address(area: &[u8]) -> Result<Option<MacAddr>, ReaderError> {
    let Some(value) = find_tlv(area, TlvType::AL_MAC_ADDRESS)? else {
        return Ok(None);
    };
    match <[u8; MAC_LEN]>::try_from(value) {
        Ok(octets) => Ok(Some(MacAddr::new(octets))),
        Err(_) => Err(ReaderError::InvalidLength {
            tlv_type: TlvType::AL_MAC_ADDRESS,
            len: value.len(),
        }),
    }
}

/// Reads the service octets of the first Supported Service TLV.
///
/// Upstream `get_supported_service` (`em_msg.cpp:89-129`) fills an `em_supported_service_t`
/// (a count and 8 service octets, zero padded). This returns the service octets, without the
/// count or the padding. Upstream's checks are kept: the value length is `1 + num`, and `num`
/// is at most 8.
///
/// # Errors
///
/// A [`ReaderError`] for a framing error, [`ReaderError::ValueTooShort`] for an empty value,
/// [`ReaderError::InvalidLength`] if the length is not `1 + num`, and
/// [`ReaderError::ValueOutOfRange`] if `num` is over 8.
pub fn supported_services(area: &[u8]) -> Result<Option<&[u8]>, ReaderError> {
    let Some(value) = find_tlv(area, TlvType::SUPPORTED_SERVICE)? else {
        return Ok(None);
    };
    let Some((&count, services)) = value.split_first() else {
        return Err(ReaderError::ValueTooShort {
            tlv_type: TlvType::SUPPORTED_SERVICE,
            needed: 1,
            found: 0,
        });
    };
    if services.len() != usize::from(count) {
        return Err(ReaderError::InvalidLength {
            tlv_type: TlvType::SUPPORTED_SERVICE,
            len: value.len(),
        });
    }
    if count > MAX_SERVICES {
        return Err(ReaderError::ValueOutOfRange {
            tlv_type: TlvType::SUPPORTED_SERVICE,
            value: count,
        });
    }
    Ok(Some(services))
}

/// Checks the value of a Multi-AP Profile TLV and returns the profile.
///
/// Upstream `parse_profile_tlv` (`em_msg.cpp:131-152`, used by `em_configuration.cpp:2415`)
/// takes the value and its length. The value is 1 octet (legacy) or 4 octets (the profile and 3
/// reserved octets), and the first octet must be below 4. Raw 0 is accepted, as upstream does;
/// the fallback policy for it stays in `em-core` (note 0005 D5.3).
///
/// # Errors
///
/// [`ReaderError::InvalidLength`] for any other length, and [`ReaderError::ValueOutOfRange`]
/// for a raw profile of 4 or more.
pub fn parse_profile_value(value: &[u8]) -> Result<ProfileType, ReaderError> {
    let (&[raw] | &[raw, _, _, _]) = value else {
        return Err(ReaderError::InvalidLength {
            tlv_type: TlvType::MULTI_AP_PROFILE,
            len: value.len(),
        });
    };
    if raw >= PROFILE_TYPE_MAX {
        return Err(ReaderError::ValueOutOfRange {
            tlv_type: TlvType::MULTI_AP_PROFILE,
            value: raw,
        });
    }
    Ok(ProfileType::from(raw))
}

/// Reads the profile of the first Multi-AP Profile TLV.
///
/// Upstream `get_profile` (`em_msg.cpp:154-175`) and `get_profile_type` (`em_msg.cpp:365-385`)
/// are the same walk; both are this reader. Upstream's default `em_profile_type_reserved` for
/// "not found" is the caller's fallback (note 0005 D5.3), so this returns `Ok(None)`.
///
/// # Errors
///
/// A [`ReaderError`] for a framing error or for a value [`parse_profile_value`] rejects.
pub fn profile(area: &[u8]) -> Result<Option<ProfileType>, ReaderError> {
    let Some(value) = find_tlv(area, TlvType::MULTI_AP_PROFILE)? else {
        return Ok(None);
    };
    parse_profile_value(value).map(Some)
}

/// Reads the station MAC address of the first TLV that holds one.
///
/// Upstream `get_sta_mac` (`em_msg.cpp:177-221`): the station MAC is at offset 6 of a Client
/// Info value (minimum 12 octets), and at offset 0 of a Client Association Event, Beacon Metrics
/// Query, Beacon Metrics Response or Associated STA Link Metrics value (minimum 6 octets).
/// Upstream's too-short value is a `false` that ends the walk; this is an error.
///
/// # Errors
///
/// A [`ReaderError`] for a framing error, or [`ReaderError::ValueTooShort`].
pub fn sta_mac(area: &[u8]) -> Result<Option<MacAddr>, ReaderError> {
    mac_at_offset_of_first_match(area, |tlv_type| match tlv_type {
        TlvType::CLIENT_INFO => Some(MAC_LEN),
        TlvType::CLIENT_ASSOCIATION_EVENT
        | TlvType::BEACON_METRICS_QUERY
        | TlvType::BEACON_METRICS_RESPONSE
        | TlvType::ASSOCIATED_STA_LINK_METRICS => Some(0),
        _ => None,
    })
}

/// Reads the BSSID of the first TLV that holds one.
///
/// Upstream `get_bss_id` (`em_msg.cpp:223-249`): the BSSID is at offset 0 of a Client Info, AP
/// Metrics or BSSID value, and at offset 6 of a Client Association Event value (after the
/// station MAC). Upstream copies 6 octets without a length check; a value too short for the read
/// is an error here.
///
/// # Errors
///
/// A [`ReaderError`] for a framing error, or [`ReaderError::ValueTooShort`].
pub fn bss_id(area: &[u8]) -> Result<Option<MacAddr>, ReaderError> {
    mac_at_offset_of_first_match(area, |tlv_type| match tlv_type {
        TlvType::CLIENT_INFO | TlvType::AP_METRICS | TlvType::BSSID => Some(0),
        TlvType::CLIENT_ASSOCIATION_EVENT => Some(MAC_LEN),
        _ => None,
    })
}

/// Reads the radio ID (RUID) of the first TLV that holds one.
///
/// Upstream `get_radio_id` (`em_msg.cpp:251-344`). The RUID is the first 6 octets of the value
/// of an AP Radio Identifier, AP Radio Basic, Advanced, HT, VHT and HE Capabilities, Channel
/// Preference, Channel Selection Response, Operating Channel Report, Channel Scan Result, Radio
/// Metrics and Backhaul STA Radio Capabilities TLV. Upstream's quirks are kept:
///
/// - A Channel Scan Request value shorter than 9 octets ends the whole search, whatever follows
///   it (`em_msg.cpp:273-276`, before the End of Message test at `:277`). The exact 2-octet value with
///   `Num_Radio` = 0 is spec-valid (§17.2.39) and is `Ok(None)`; any other short value is an
///   error. A full-size one with 0 radios is `Ok(None)` too, and ends the search.
/// - An AP Operational BSS TLV gives the RUID of its first radio, at offset 1. With 0 radios the
///   walk continues with the next TLV (`em_msg.cpp:304-312`).
///
/// Upstream copies 6 octets without a length check; a value too short for the read is an error
/// here.
///
/// # Errors
///
/// A [`ReaderError`] for a framing error, or [`ReaderError::ValueTooShort`].
pub fn radio_id(area: &[u8]) -> Result<Option<MacAddr>, ReaderError> {
    for item in TlvIter::new(area) {
        let tlv = item?;
        match tlv.tlv_type {
            TlvType::CHANNEL_SCAN_REQUEST => return channel_scan_request_radio_id(&tlv),
            TlvType::AP_OPERATIONAL_BSS => {
                if let Some(radio_id) = operational_bss_radio_id(&tlv)? {
                    return Ok(Some(radio_id));
                }
            }
            TlvType::AP_RADIO_IDENTIFIER
            | TlvType::AP_RADIO_BASIC_CAPABILITIES
            | TlvType::AP_RADIO_ADVANCED_CAPABILITIES
            | TlvType::AP_HT_CAPABILITIES
            | TlvType::AP_VHT_CAPABILITIES
            | TlvType::AP_HE_CAPABILITIES
            | TlvType::CHANNEL_PREFERENCE
            | TlvType::CHANNEL_SELECTION_RESPONSE
            | TlvType::OPERATING_CHANNEL_REPORT
            | TlvType::CHANNEL_SCAN_RESULT
            | TlvType::RADIO_METRICS
            | TlvType::BACKHAUL_STA_RADIO_CAPABILITIES => {
                return mac_at(&tlv, 0).map(Some);
            }
            _ => {}
        }
    }
    Ok(None)
}

/// Reads the frequency band of the first Supported or Autoconfig Frequency Band TLV.
///
/// Upstream `get_freq_band` (`em_msg.cpp:346-363`) copies the first octet of the value; for an
/// empty value it reads the next TLV header. An empty value is an error here.
///
/// # Errors
///
/// A [`ReaderError`] for a framing error, or [`ReaderError::ValueTooShort`] for an empty value.
pub fn freq_band(area: &[u8]) -> Result<Option<FreqBand>, ReaderError> {
    for item in TlvIter::new(area) {
        let tlv = item?;
        if tlv.tlv_type == TlvType::SUPPORTED_FREQ_BAND
            || tlv.tlv_type == TlvType::AUTOCONFIG_FREQ_BAND
        {
            let Some(&band) = tlv.value.first() else {
                return Err(ReaderError::ValueTooShort {
                    tlv_type: tlv.tlv_type,
                    needed: 1,
                    found: 0,
                });
            };
            return Ok(Some(FreqBand::from(band)));
        }
    }
    Ok(None)
}

/// Walks to the first TLV whose type has a MAC offset, and reads the MAC there.
fn mac_at_offset_of_first_match(
    area: &[u8],
    offset_of: fn(TlvType) -> Option<usize>,
) -> Result<Option<MacAddr>, ReaderError> {
    for item in TlvIter::new(area) {
        let tlv = item?;
        if let Some(offset) = offset_of(tlv.tlv_type) {
            return mac_at(&tlv, offset).map(Some);
        }
    }
    Ok(None)
}

/// Reads the 6 octets at `offset` of the value.
fn mac_at(tlv: &Tlv<'_>, offset: usize) -> Result<MacAddr, ReaderError> {
    let octets = tlv
        .value
        .get(offset..)
        .and_then(|rest| rest.first_chunk::<MAC_LEN>());
    match octets {
        Some(octets) => Ok(MacAddr::new(*octets)),
        None => Err(ReaderError::ValueTooShort {
            tlv_type: tlv.tlv_type,
            needed: offset + MAC_LEN,
            found: tlv.value.len(),
        }),
    }
}

/// The Channel Scan Request branch of `get_radio_id`: this TLV always ends the search.
fn channel_scan_request_radio_id(tlv: &Tlv<'_>) -> Result<Option<MacAddr>, ReaderError> {
    match tlv.value {
        // Spec-valid: no radios (§17.2.39). Upstream's size check returns `false` here.
        [_, 0] => Ok(None),
        value if value.len() < CHANNEL_SCAN_REQUEST_MIN_LEN => Err(ReaderError::ValueTooShort {
            tlv_type: tlv.tlv_type,
            needed: CHANNEL_SCAN_REQUEST_MIN_LEN,
            found: value.len(),
        }),
        value if value[1] == 0 => Ok(None),
        _ => mac_at(tlv, 2).map(Some),
    }
}

/// The AP Operational BSS branch of `get_radio_id`: `Ok(None)` means "walk on".
fn operational_bss_radio_id(tlv: &Tlv<'_>) -> Result<Option<MacAddr>, ReaderError> {
    let Some(&radios_num) = tlv.value.first() else {
        return Err(ReaderError::ValueTooShort {
            tlv_type: tlv.tlv_type,
            needed: 1,
            found: 0,
        });
    };
    if radios_num == 0 {
        return Ok(None);
    }
    mac_at(tlv, 1).map(Some)
}

/// A TLV found by the walker, with its offset in the area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkedTlv<'a> {
    /// Offset of the TLV header in the area.
    pub offset: usize,
    /// The TLV.
    pub tlv: Tlv<'a>,
}

/// Why [`first_tlv`] returned no TLV.
///
/// Upstream's `get_first_tlv` returns `NULL` for all of these (`em_msg.cpp:424-444`). Its
/// callers treat that as a failure, where a `NULL` from `get_next_tlv` is the end of the walk,
/// so the two are different types here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstTlvError {
    /// The area is empty.
    EmptyArea,
    /// The area is shorter than a TLV header.
    TruncatedHeader,
    /// The TLV has a zero length, which upstream rejects (End of Message included).
    ZeroLength,
    /// The TLV length runs past the end of the area.
    LengthPastEnd,
}

impl fmt::Display for FirstTlvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyArea => write!(f, "the TLV area is empty"),
            Self::TruncatedHeader => write!(f, "the TLV area is shorter than a TLV header"),
            Self::ZeroLength => write!(f, "the TLV has a zero length"),
            Self::LengthPastEnd => write!(f, "the TLV length runs past the end of the area"),
        }
    }
}

impl std::error::Error for FirstTlvError {}

/// Returns the TLV at the start of `area`, as upstream `get_first_tlv` (`em_msg.cpp:424-444`).
///
/// Upstream does not look at the type, and rejects a zero-length TLV. The walk of
/// [`next_tlv`] therefore ends at the first zero-length TLV, an End of Message TLV included
/// (note 0009 A10); [`TlvIter`] does not stop there. Offsets and checks are upstream's.
///
/// # Errors
///
/// [`FirstTlvError`] if upstream returns `NULL`.
pub fn first_tlv(area: &[u8]) -> Result<WalkedTlv<'_>, FirstTlvError> {
    tlv_at(area, 0)
}

/// Returns the TLV after the one at `current_offset`, as upstream `get_next_tlv`
/// (`em_msg.cpp:446-480`). `None` is the end of the walk, whatever the reason: the current TLV
/// is the last one, or the next one fails the checks of [`first_tlv`].
///
/// Upstream holds the size of the current TLV in a `uint16_t` (`em_msg.cpp:466`), which wraps
/// for lengths 0xFFFD to 0xFFFF and then returns the same TLV, or one inside it. The size is
/// computed here without wrapping, so a walk always advances or ends (note 0009 A10).
#[must_use]
pub fn next_tlv(area: &[u8], current_offset: usize) -> Option<WalkedTlv<'_>> {
    let &[_, len_high, len_low] = area
        .get(current_offset..)?
        .first_chunk::<TLV_HEADER_LEN>()?;
    let current_size = TLV_HEADER_LEN + usize::from(u16::from_be_bytes([len_high, len_low]));
    let next_offset = current_offset + current_size;
    if next_offset >= area.len() {
        return None;
    }
    tlv_at(area, next_offset).ok()
}

/// `get_first_tlv` applied to the part of `area` from `offset`.
fn tlv_at(area: &[u8], offset: usize) -> Result<WalkedTlv<'_>, FirstTlvError> {
    let rest = area.get(offset..).unwrap_or_default();
    if rest.is_empty() {
        return Err(FirstTlvError::EmptyArea);
    }
    let Some(&[type_byte, len_high, len_low]) = rest.first_chunk::<TLV_HEADER_LEN>() else {
        return Err(FirstTlvError::TruncatedHeader);
    };
    let value_len = usize::from(u16::from_be_bytes([len_high, len_low]));
    if value_len == 0 {
        return Err(FirstTlvError::ZeroLength);
    }
    let Some(value) = rest.get(TLV_HEADER_LEN..TLV_HEADER_LEN + value_len) else {
        return Err(FirstTlvError::LengthPastEnd);
    };
    Ok(WalkedTlv {
        offset,
        tlv: Tlv {
            tlv_type: TlvType(type_byte),
            value,
        },
    })
}

#[cfg(test)]
mod tests {
    use core::mem::size_of;

    use super::*;

    const MAC_A: [u8; 6] = [0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5];
    const MAC_B: [u8; 6] = [0xB0, 0xB1, 0xB2, 0xB3, 0xB4, 0xB5];

    fn push_tlv(out: &mut Vec<u8>, tlv_type: TlvType, value: &[u8]) {
        out.push(tlv_type.0);
        out.extend_from_slice(&u16::try_from(value.len()).unwrap().to_be_bytes());
        out.extend_from_slice(value);
    }

    /// The TLVs followed by an End of Message TLV.
    fn area_of(tlvs: &[(TlvType, &[u8])]) -> Vec<u8> {
        let mut area = area_without_end_of_message(tlvs);
        push_tlv(&mut area, TlvType::END_OF_MESSAGE, &[]);
        area
    }

    fn area_without_end_of_message(tlvs: &[(TlvType, &[u8])]) -> Vec<u8> {
        let mut area = Vec::new();
        for (tlv_type, value) in tlvs {
            push_tlv(&mut area, *tlv_type, value);
        }
        area
    }

    #[test]
    fn get_al_mac_address_overlong_value_is_an_error() {
        // intentional-difference:al-mac-length
        // Upstream em_msg.cpp:71-86 copies htons(tlv->len) octets into a 6-octet buffer, so an
        // 8-octet value overflows it. The value must be exactly 6 octets (note 0009 D6).
        let overlong = area_of(&[(TlvType::AL_MAC_ADDRESS, &[1, 2, 3, 4, 5, 6, 7, 8])]);
        assert_eq!(
            al_mac_address(&overlong),
            Err(ReaderError::InvalidLength {
                tlv_type: TlvType::AL_MAC_ADDRESS,
                len: 8
            })
        );
        let short = area_of(&[(TlvType::AL_MAC_ADDRESS, &[1, 2, 3, 4, 5])]);
        assert_eq!(
            al_mac_address(&short),
            Err(ReaderError::InvalidLength {
                tlv_type: TlvType::AL_MAC_ADDRESS,
                len: 5
            })
        );
        let exact = area_of(&[(TlvType::AL_MAC_ADDRESS, &MAC_A)]);
        assert_eq!(al_mac_address(&exact), Ok(Some(MacAddr::new(MAC_A))));
    }

    #[test]
    fn get_bss_id_value_too_short_for_the_read_is_an_error() {
        // intentional-difference:bss-id-short-read
        // Upstream em_msg.cpp:223-249 copies 6 octets from the per-type offset with no length
        // check, so a short value over-reads the next TLV. The reader reports the shortfall.
        let cases = [
            (TlvType::CLIENT_INFO, 0, 6),
            (TlvType::AP_METRICS, 0, 6),
            (TlvType::BSSID, 0, 6),
            (TlvType::CLIENT_ASSOCIATION_EVENT, 6, 12),
        ];
        for (tlv_type, offset, needed) in cases {
            let short_value = vec![0x11; needed - 1];
            let area = area_of(&[(tlv_type, &short_value)]);
            assert_eq!(
                bss_id(&area),
                Err(ReaderError::ValueTooShort {
                    tlv_type,
                    needed,
                    found: needed - 1
                }),
                "type 0x{:02X}, offset {offset}",
                tlv_type.0
            );
        }
    }

    #[test]
    fn get_radio_id_value_too_short_for_the_read_is_an_error() {
        // intentional-difference:radio-id-short-read
        // Upstream em_msg.cpp:251-344 copies 6 octets from the value (or from the first radio
        // of an Operational BSS) with no length check.
        let plain_types = [
            TlvType::AP_RADIO_IDENTIFIER,
            TlvType::AP_RADIO_BASIC_CAPABILITIES,
            TlvType::AP_RADIO_ADVANCED_CAPABILITIES,
            TlvType::AP_HT_CAPABILITIES,
            TlvType::AP_VHT_CAPABILITIES,
            TlvType::AP_HE_CAPABILITIES,
            TlvType::CHANNEL_PREFERENCE,
            TlvType::CHANNEL_SELECTION_RESPONSE,
            TlvType::OPERATING_CHANNEL_REPORT,
            TlvType::CHANNEL_SCAN_RESULT,
            TlvType::RADIO_METRICS,
            TlvType::BACKHAUL_STA_RADIO_CAPABILITIES,
        ];
        for tlv_type in plain_types {
            let area = area_of(&[(tlv_type, &MAC_A[..5])]);
            assert_eq!(
                radio_id(&area),
                Err(ReaderError::ValueTooShort {
                    tlv_type,
                    needed: 6,
                    found: 5
                }),
                "type 0x{:02X}",
                tlv_type.0
            );
        }

        let mut one_radio_cut = vec![1];
        one_radio_cut.extend_from_slice(&MAC_A[..5]);
        let area = area_of(&[(TlvType::AP_OPERATIONAL_BSS, &one_radio_cut)]);
        assert_eq!(
            radio_id(&area),
            Err(ReaderError::ValueTooShort {
                tlv_type: TlvType::AP_OPERATIONAL_BSS,
                needed: 7,
                found: 6
            })
        );

        // Upstream reads Num_Radio from the next TLV header here.
        let area = area_of(&[(TlvType::AP_OPERATIONAL_BSS, &[])]);
        assert_eq!(
            radio_id(&area),
            Err(ReaderError::ValueTooShort {
                tlv_type: TlvType::AP_OPERATIONAL_BSS,
                needed: 1,
                found: 0
            })
        );
    }

    #[test]
    fn get_radio_id_short_channel_scan_request_fails_the_whole_walk_before_the_end_of_message_check()
     {
        // intentional-difference:channel-scan-request-short
        // Upstream em_msg.cpp:273-276 returns `false` for a Channel Scan Request value shorter
        // than sizeof(em_channel_scan_req_t) = 9, before it tests for End of Message and
        // whatever the other TLVs hold. The port keeps the failed walk and reports it as an
        // error (note 0009 D6).
        let short = [0x80, 0x01, 0x00];
        let expected = Err(ReaderError::ValueTooShort {
            tlv_type: TlvType::CHANNEL_SCAN_REQUEST,
            needed: 9,
            found: 3,
        });
        let alone = area_of(&[(TlvType::CHANNEL_SCAN_REQUEST, &short)]);
        assert_eq!(radio_id(&alone), expected);
        let before_radio_id = area_of(&[
            (TlvType::CHANNEL_SCAN_REQUEST, &short),
            (TlvType::AP_RADIO_IDENTIFIER, &MAC_B),
        ]);
        assert_eq!(radio_id(&before_radio_id), expected);

        // Two octets with 1 radio are short too; only Num_Radio = 0 is spec-valid (§17.2.39).
        let one_radio_cut = area_of(&[(TlvType::CHANNEL_SCAN_REQUEST, &[0x80, 0x01])]);
        assert_eq!(
            radio_id(&one_radio_cut),
            Err(ReaderError::ValueTooShort {
                tlv_type: TlvType::CHANNEL_SCAN_REQUEST,
                needed: 9,
                found: 2
            })
        );
    }

    #[test]
    fn get_radio_id_channel_scan_request_with_0_radios_ends_with_not_found() {
        // Upstream em_msg.cpp:320-328: Num_Radio = 0 returns `false` and does not go on to the
        // Radio Identifier that follows. The exact 2-octet value is the spec form (A10).
        let spec_form = area_of(&[
            (TlvType::CHANNEL_SCAN_REQUEST, &[0x80, 0x00]),
            (TlvType::AP_RADIO_IDENTIFIER, &MAC_B),
        ]);
        assert_eq!(radio_id(&spec_form), Ok(None));
        let full_size = area_of(&[
            (
                TlvType::CHANNEL_SCAN_REQUEST,
                &[0x80, 0, 0, 0, 0, 0, 0, 0, 0],
            ),
            (TlvType::AP_RADIO_IDENTIFIER, &MAC_B),
        ]);
        assert_eq!(radio_id(&full_size), Ok(None));
    }

    #[test]
    fn get_radio_id_operational_bss_with_0_radios_continues_the_walk() {
        // Upstream em_msg.cpp:304-312: the `if (ap->radios_num >= 1)` has no `else`, so a TLV
        // with 0 radios falls through to the next TLV. This is an upstream quirk the port keeps
        // (note 0009 D6), not a difference.
        let area = area_of(&[
            (TlvType::AP_OPERATIONAL_BSS, &[0]),
            (TlvType::AP_RADIO_IDENTIFIER, &MAC_B),
        ]);
        assert_eq!(radio_id(&area), Ok(Some(MacAddr::new(MAC_B))));
        let alone = area_of(&[(TlvType::AP_OPERATIONAL_BSS, &[0])]);
        assert_eq!(radio_id(&alone), Ok(None));
    }

    #[test]
    fn get_freq_band_empty_value_is_an_error() {
        // intentional-difference:freq-band-empty
        // Upstream em_msg.cpp:346-363 copies one octet whatever the length, so an empty value
        // returns the first octet of the next TLV header.
        for tlv_type in [TlvType::SUPPORTED_FREQ_BAND, TlvType::AUTOCONFIG_FREQ_BAND] {
            let area = area_of(&[(tlv_type, &[])]);
            assert_eq!(
                freq_band(&area),
                Err(ReaderError::ValueTooShort {
                    tlv_type,
                    needed: 1,
                    found: 0
                })
            );
        }
    }

    #[test]
    fn readers_without_end_of_message_return_a_framing_error() {
        // intentional-difference:missing-end-of-message
        // Upstream's loops stop when `len` reaches 0 and return "not found" for a frame
        // without End of Message. Such a frame is not spec-valid; the walk reports it (D6).
        let area = area_without_end_of_message(&[(TlvType::DEVICE_INFORMATION, &[1, 2, 3])]);
        let missing = ReaderError::Framing(FramingError::MissingEndOfMessage);
        assert_eq!(al_mac_address(&area), Err(missing));
        assert_eq!(supported_services(&area), Err(missing));
        assert_eq!(profile(&area), Err(missing));
        assert_eq!(sta_mac(&area), Err(missing));
        assert_eq!(bss_id(&area), Err(missing));
        assert_eq!(radio_id(&area), Err(missing));
        assert_eq!(freq_band(&area), Err(missing));
        assert_eq!(
            find_tlv(&area, TlvType::AL_MAC_ADDRESS),
            Err(FramingError::MissingEndOfMessage)
        );

        // A match before the missing End of Message is still found.
        let found = area_without_end_of_message(&[(TlvType::AL_MAC_ADDRESS, &MAC_A)]);
        assert_eq!(al_mac_address(&found), Ok(Some(MacAddr::new(MAC_A))));
    }

    #[test]
    fn readers_stop_at_the_first_framing_error() {
        // intentional-difference:framing-error
        // Upstream em_msg.cpp:266-272 returns `false` where a TLV runs past the end; the port
        // reports the framing error. A match before the bad TLV wins.
        let past_end = [0x42, 0x00, 0x40, 0x01];
        let mut bad_first = past_end.to_vec();
        push_tlv(&mut bad_first, TlvType::AP_RADIO_IDENTIFIER, &MAC_A);
        assert_eq!(
            radio_id(&bad_first),
            Err(ReaderError::Framing(FramingError::TlvLengthPastEnd {
                tlv_type: 0x42
            }))
        );
        let mut found_first =
            area_without_end_of_message(&[(TlvType::AP_RADIO_IDENTIFIER, &MAC_A)]);
        found_first.extend_from_slice(&past_end);
        assert_eq!(radio_id(&found_first), Ok(Some(MacAddr::new(MAC_A))));
    }

    #[test]
    fn first_matching_tlv_in_frame_order_wins_for_every_reader() {
        let sta_first = area_of(&[
            (TlvType::BEACON_METRICS_RESPONSE, &[1, 2, 3, 4, 5, 6, 0, 0]),
            (TlvType::CLIENT_INFO, &[0xAA; 12]),
        ]);
        assert_eq!(
            sta_mac(&sta_first),
            Ok(Some(MacAddr::new([1, 2, 3, 4, 5, 6])))
        );
        let band_first = area_of(&[
            (TlvType::AUTOCONFIG_FREQ_BAND, &[3]),
            (TlvType::SUPPORTED_FREQ_BAND, &[1]),
        ]);
        assert_eq!(freq_band(&band_first), Ok(Some(FreqBand::BAND_6_GHZ)));
    }

    #[test]
    fn sta_mac_value_too_short_for_the_read_is_an_error() {
        // intentional-difference:sta-mac-short-value
        // Upstream em_msg.cpp:197-211 returns `false` ("TLV data too small"); the minimums are
        // 12 octets for Client Info and 6 for the other types.
        let client_info = area_of(&[(TlvType::CLIENT_INFO, &[0x11; 11])]);
        assert_eq!(
            sta_mac(&client_info),
            Err(ReaderError::ValueTooShort {
                tlv_type: TlvType::CLIENT_INFO,
                needed: 12,
                found: 11
            })
        );
        let assoc_event = area_of(&[(TlvType::CLIENT_ASSOCIATION_EVENT, &[0x11; 5])]);
        assert_eq!(
            sta_mac(&assoc_event),
            Err(ReaderError::ValueTooShort {
                tlv_type: TlvType::CLIENT_ASSOCIATION_EVENT,
                needed: 6,
                found: 5
            })
        );
    }

    #[test]
    fn supported_services_applies_upstreams_checks() {
        // intentional-difference:supported-service-rejected
        // Upstream returns `false` for these values (em_msg.cpp:104-117); the port reports why.
        let empty = area_of(&[(TlvType::SUPPORTED_SERVICE, &[])]);
        assert_eq!(
            supported_services(&empty),
            Err(ReaderError::ValueTooShort {
                tlv_type: TlvType::SUPPORTED_SERVICE,
                needed: 1,
                found: 0
            })
        );
        let length_mismatch = area_of(&[(TlvType::SUPPORTED_SERVICE, &[2, 0xA0])]);
        assert_eq!(
            supported_services(&length_mismatch),
            Err(ReaderError::InvalidLength {
                tlv_type: TlvType::SUPPORTED_SERVICE,
                len: 2
            })
        );
        let two = area_of(&[(TlvType::SUPPORTED_SERVICE, &[2, 0xA0, 0xA1])]);
        assert_eq!(supported_services(&two), Ok(Some(&[0xA0, 0xA1][..])));
    }

    #[test]
    fn parse_profile_value_accepts_one_or_four_octets_below_four() {
        // intentional-difference:profile-rejected
        // Upstream returns `false` for a rejected value (em_msg.cpp:139-148); the port reports why.
        assert_eq!(parse_profile_value(&[0]), Ok(ProfileType::from(0)));
        assert_eq!(
            parse_profile_value(&[3, 9, 9, 9]),
            Ok(ProfileType::MULTI_AP_PROFILE_3)
        );
        assert_eq!(
            parse_profile_value(&[4]),
            Err(ReaderError::ValueOutOfRange {
                tlv_type: TlvType::MULTI_AP_PROFILE,
                value: 4
            })
        );
        assert_eq!(
            parse_profile_value(&[1, 0]),
            Err(ReaderError::InvalidLength {
                tlv_type: TlvType::MULTI_AP_PROFILE,
                len: 2
            })
        );
    }

    #[test]
    fn find_tlv_returns_the_value_and_never_end_of_message() {
        let area = area_of(&[
            (TlvType::DEVICE_INFORMATION, &[7]),
            (TlvType::AL_MAC_ADDRESS, &MAC_A),
        ]);
        assert_eq!(
            find_tlv(&area, TlvType::AL_MAC_ADDRESS),
            Ok(Some(&MAC_A[..]))
        );
        assert_eq!(find_tlv(&area, TlvType::END_OF_MESSAGE), Ok(None));
    }

    #[test]
    fn first_tlv_reports_why_it_failed() {
        assert_eq!(first_tlv(&[]), Err(FirstTlvError::EmptyArea));
        assert_eq!(first_tlv(&[1, 0]), Err(FirstTlvError::TruncatedHeader));
        assert_eq!(first_tlv(&[1, 0, 0]), Err(FirstTlvError::ZeroLength));
        assert_eq!(first_tlv(&[1, 0, 2, 9]), Err(FirstTlvError::LengthPastEnd));
        let walked = first_tlv(&[1, 0, 1, 9, 0, 0, 0]).unwrap();
        assert_eq!(walked.offset, 0);
        assert_eq!(walked.tlv.tlv_type, TlvType(1));
        assert_eq!(walked.tlv.value, [9]);
    }

    #[test]
    fn next_tlv_walks_by_offset() {
        let area = area_of(&[(TlvType(0x01), &[1, 2]), (TlvType(0x02), &[3])]);
        let second = next_tlv(&area, 0).unwrap();
        assert_eq!(second.offset, 5);
        assert_eq!(second.tlv.value, [3]);
        // The End of Message TLV has a zero length, so the walk ends before it.
        assert_eq!(next_tlv(&area, 5), None);
        assert_eq!(next_tlv(&area, area.len()), None);
        assert_eq!(next_tlv(&area, area.len() + 10), None);
    }

    #[test]
    fn get_next_tlv_stops_at_a_zero_length_tlv_that_hides_the_eht_operations_tlv() {
        // intentional-difference:walker-zero-length-stop
        // User decision, note 0009 A10: the walker ends at any zero-length TLV, as upstream. An
        // empty Backhaul STA MLD Configuration TLV (em_configuration.cpp:3771-3777 can send
        // one) hides the EHT Operations TLV after it from the walks at em_configuration.cpp:
        // 5141-5242, 5570-5612 and 5748-5776. TlvIter, and so find_tlv, still reaches it.
        let area = area_of(&[
            (TlvType::TRAFFIC_SEPARATION_POLICY, &[0]),
            (TlvType::BACKHAUL_STA_MLD_CONFIGURATION, &[]),
            (TlvType::EHT_OPERATIONS, &[1, 2]),
        ]);
        assert!(first_tlv(&area).is_ok());
        assert_eq!(next_tlv(&area, 0), None);
        assert_eq!(
            find_tlv(&area, TlvType::EHT_OPERATIONS),
            Ok(Some(&[1, 2][..]))
        );
    }

    #[test]
    fn get_first_tlv_fails_on_an_empty_first_tlv_of_a_backhaul_sta_capability_report() {
        // intentional-difference:walker-zero-length-stop
        // User decision, note 0009 A10. Both single-TLV lookups of handle_bsta_cap_report start
        // with get_first_tlv (em_capability.cpp:1214-1230, 1242-1255). An empty first TLV makes
        // both fail, so Client Info is never processed.
        let area = area_of(&[
            (TlvType(0xFE), &[]),
            (TlvType::BACKHAUL_STA_RADIO_CAPABILITIES, &MAC_A),
            (TlvType::CLIENT_INFO, &[0xAA; 12]),
        ]);
        assert_eq!(first_tlv(&area), Err(FirstTlvError::ZeroLength));
        // A walk from the empty TLV does not skip it either: the TLV after it is reachable.
        assert_eq!(next_tlv(&area, 0).map(|walked| walked.offset), Some(3));
    }

    #[test]
    fn get_next_tlv_size_wrap_at_0xfffd_to_0xffff_is_not_reproduced() {
        // intentional-difference:walker-size-wrap
        // Upstream em_msg.cpp:466 holds `sizeof(em_tlv_t) + ntohs(tlv->len)` in a uint16_t. For
        // lengths 0xFFFD, 0xFFFE and 0xFFFF it wraps to 0, 1 and 2, so upstream returns the same
        // TLV again, or one starting inside it. The port computes the size without wrapping.
        for value_len in [0xFFFC_u16, 0xFFFD, 0xFFFE, 0xFFFF] {
            let mut area = Vec::new();
            push_tlv(&mut area, TlvType(0x01), &vec![0; usize::from(value_len)]);
            let second_offset = area.len();
            push_tlv(&mut area, TlvType(0x02), &[0xAA]);
            let next = next_tlv(&area, 0).unwrap();
            assert_eq!(next.offset, second_offset, "length {value_len:#06X}");
            assert_eq!(next.tlv.tlv_type, TlvType(0x02));
            assert_eq!(next.tlv.value, [0xAA]);
        }
    }

    #[test]
    fn get_first_tlv_and_get_next_tlv_return_end_of_message_with_a_non_zero_length() {
        // Rust-only pin of note 0009 A10: the walker applies no type check, so an End of
        // Message TLV is a TLV like any other. Only a zero length stops it (get_first_tlv,
        // em_msg.cpp:424-444), and an End of Message TLV with a non-zero length passes.
        let end_of_message_first = [TlvType::END_OF_MESSAGE.0, 0x00, 0x02, 0xAA, 0xBB];
        let first = first_tlv(&end_of_message_first).unwrap();
        assert_eq!(first.offset, 0);
        assert_eq!(first.tlv.tlv_type, TlvType::END_OF_MESSAGE);
        assert_eq!(first.tlv.value, [0xAA, 0xBB]);

        let mut after_regular = Vec::new();
        push_tlv(&mut after_regular, TlvType(0x01), &[0x11]);
        push_tlv(&mut after_regular, TlvType::END_OF_MESSAGE, &[0xAA, 0xBB]);
        let next = next_tlv(&after_regular, 0).unwrap();
        assert_eq!(next.offset, 4);
        assert_eq!(next.tlv.tlv_type, TlvType::END_OF_MESSAGE);
        assert_eq!(next.tlv.value, [0xAA, 0xBB]);
    }

    #[test]
    fn reader_types_have_bounded_stack_cost() {
        let word = size_of::<usize>();
        assert!(size_of::<ReaderError>() <= 4 * word);
        assert!(size_of::<FirstTlvError>() <= 1);
        assert!(size_of::<WalkedTlv<'_>>() <= 4 * word);
        assert!(size_of::<Result<Option<MacAddr>, ReaderError>>() <= 4 * word);
        assert!(size_of::<Result<WalkedTlv<'_>, FirstTlvError>>() <= 4 * word);
    }

    #[test]
    fn get_supported_service_rejects_a_value_longer_than_one_plus_num() {
        // Upstream's check is `tlv_len != 1 + num` (`em_msg.cpp:112`), in both directions.
        let area = [0x80, 0x00, 0x03, 0x01, 0xAA, 0xBB, 0x00, 0x00, 0x00];
        assert_eq!(
            supported_services(&area),
            Err(ReaderError::InvalidLength {
                tlv_type: TlvType::SUPPORTED_SERVICE,
                len: 3,
            })
        );
    }

    #[test]
    fn get_freq_band_reads_the_first_octet_of_a_longer_value() {
        // Upstream copies one octet from the start of the value (`em_msg.cpp:354`).
        let area = [
            TlvType::SUPPORTED_FREQ_BAND.0,
            0x00,
            0x02,
            0x01,
            0x02,
            0x00,
            0x00,
            0x00,
        ];
        assert_eq!(freq_band(&area), Ok(Some(FreqBand::from(0x01))));
    }

    #[test]
    fn get_first_tlv_fails_on_a_one_octet_area_as_a_truncated_header() {
        // `buff_len < sizeof(em_tlv_t)` (`em_msg.cpp:431`), separate from the empty buffer.
        assert_eq!(
            first_tlv(&[0x01]).map(|walked| walked.offset),
            Err(FirstTlvError::TruncatedHeader)
        );
    }

    #[test]
    fn get_first_tlv_reads_both_length_octets() {
        // A length of 0x0100 is not zero-length (`em_msg.cpp:437-439`).
        let mut area = vec![0x01, 0x01, 0x00];
        area.resize(3 + 256, 0xAB);
        let walked = first_tlv(&area).expect("a 256-octet value");
        assert_eq!(walked.tlv.value.len(), 256);
    }

    #[test]
    fn get_next_tlv_ends_at_a_failing_tlv_without_resynchronising() {
        // The TLV at offset 4 has a zero length, so the walk ends (`em_msg.cpp:479`), although
        // the octets from offset 5 would parse as a TLV with a 5-octet value.
        let area = [
            0x02, 0x00, 0x01, 0xAA, 0x01, 0x00, 0x00, 0x05, 0x01, 0x02, 0x03, 0x04, 0x05,
        ];
        assert!(next_tlv(&area, 0).is_none());
    }
}
