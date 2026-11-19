// SPDX-License-Identifier: Apache-2.0

//! Shared test helpers for TLV bodies (design note 0008, D3) and two sample
//! bodies that exercise them.
//!
//! A real body writes a small [`Codec`] that forwards to its `decode_value`
//! and `encode_value`, then calls the `check_*` helper for each D3 item with
//! its own fixtures. The helpers return `Err(message)` instead of panicking,
//! so the tests below can show that each one rejects a deliberately wrong
//! body or fixture. [`assert_passes`] turns an `Err` into a test failure.
//!
//! The sample bodies are not upstream bodies and are not public API:
//!
//! - [`Sample`] has no tail, so any extra byte is an error; an empty value is accepted.
//! - [`TailSample`] is [`Sample`] followed by a tail that runs to the end of the TLV.
//!
//! Wire layout of [`Sample`]: flags (1 byte: bit 7 enabled, bit 6 urgent,
//! bits 5..0 reserved), id (big-endian u16), entry count (1 byte), then per
//! entry a channel (1 byte) and a weight (big-endian u16).

use core::fmt::Debug;

use crate::body::{Reader, Writer};
use crate::error::{DecodeError, EncodeError};

/// What a body offers to the helpers: decode a value, encode a body.
pub trait Codec {
    /// The body type; it borrows from the decoded value.
    type Body<'a>: Debug + PartialEq;

    /// Decodes a whole TLV value.
    fn decode(value: &[u8]) -> Result<Self::Body<'_>, DecodeError>;

    /// Encodes a body into a whole TLV value.
    fn encode(body: &Self::Body<'_>) -> Result<Vec<u8>, EncodeError>;
}

/// Fails the test with the message of a helper that returned `Err`.
pub fn assert_passes(result: Result<(), String>) {
    if let Err(message) = result {
        panic!("{message}");
    }
}

/// D3 item 1: the fixture decodes to `expected` and `expected` encodes back to the fixture.
pub fn check_golden<'a, C: Codec>(fixture: &'a [u8], expected: &C::Body<'a>) -> Result<(), String> {
    let decoded =
        C::decode(fixture).map_err(|e| format!("golden fixture did not decode: {e:?}"))?;
    if decoded != *expected {
        return Err(format!(
            "golden decode gave {decoded:?}, expected {expected:?}"
        ));
    }
    let encoded = C::encode(expected).map_err(|e| format!("golden body did not encode: {e:?}"))?;
    if encoded != fixture {
        return Err(format!(
            "golden encode gave {encoded:02X?}, expected {fixture:02X?}"
        ));
    }
    Ok(())
}

/// D3 item 2: every strict prefix of the fixture is an error, except the
/// lengths for which `is_accepted` is true (alternate lengths, empty value, tail bytes), which
/// must decode. `expected_error` gives the error each other length must
/// produce, so the expectation is stated per fixture and per length.
pub fn check_strict_prefixes<C: Codec>(
    fixture: &[u8],
    is_accepted: impl Fn(usize) -> bool,
    expected_error: impl Fn(usize) -> DecodeError,
) -> Result<(), String> {
    C::decode(fixture).map_err(|e| format!("the whole fixture did not decode: {e:?}"))?;
    for len in 0..fixture.len() {
        let result = C::decode(&fixture[..len]);
        if is_accepted(len) {
            if let Err(error) = result {
                return Err(format!(
                    "prefix of {len} bytes is accepted but gave {error:?}"
                ));
            }
        } else {
            let expected = expected_error(len);
            match result {
                Ok(body) => {
                    return Err(format!(
                        "prefix of {len} bytes decoded to {body:?}, expected {expected:?}"
                    ));
                }
                Err(error) if error != expected => {
                    return Err(format!(
                        "prefix of {len} bytes gave {error:?}, expected {expected:?}"
                    ));
                }
                Err(_) => {}
            }
        }
    }
    Ok(())
}

/// D3 item 3: one extra byte after a complete value is `TrailingBytes`.
/// Not for a body with a remainder-bounded tail, which takes the byte as tail.
pub fn check_trailing_byte<C: Codec>(fixture: &[u8]) -> Result<(), String> {
    C::decode(fixture).map_err(|e| format!("the fixture itself did not decode: {e:?}"))?;
    let mut longer = fixture.to_vec();
    longer.push(0xA5);
    match C::decode(&longer) {
        Err(DecodeError::TrailingBytes { remaining: 1 }) => Ok(()),
        other => Err(format!(
            "one extra byte gave {other:?}, expected TrailingBytes with 1 byte"
        )),
    }
}

/// D3 item 4: encoding `too_long` fails with exactly `expected` and returns no bytes.
pub fn check_count_too_large<C: Codec>(
    too_long: &C::Body<'_>,
    expected: EncodeError,
) -> Result<(), String> {
    match C::encode(too_long) {
        Err(error) if error == expected => Ok(()),
        other => Err(format!(
            "encoding the over-long body gave {other:02X?}, expected {expected:?}"
        )),
    }
}

/// D3 item 5: a fixture with nonzero reserved bits decodes to `expected` and
/// re-encodes to `canonical`, which must differ from the fixture.
pub fn check_reserved_bits<'a, C: Codec>(
    fixture: &'a [u8],
    expected: &C::Body<'a>,
    canonical: &[u8],
) -> Result<(), String> {
    if fixture == canonical {
        return Err(
            "the reserved-bit fixture equals the canonical bytes, so it tests nothing".to_string(),
        );
    }
    check_decodes_and_reencodes::<C>(fixture, expected, canonical)
}

/// D3 item 6: an alternate-length or empty fixture decodes to `expected` and
/// re-encodes to `reencoded` (which may equal the fixture).
pub fn check_alternate_value<'a, C: Codec>(
    fixture: &'a [u8],
    expected: &C::Body<'a>,
    reencoded: &[u8],
) -> Result<(), String> {
    check_decodes_and_reencodes::<C>(fixture, expected, reencoded)
}

fn check_decodes_and_reencodes<'a, C: Codec>(
    fixture: &'a [u8],
    expected: &C::Body<'a>,
    reencoded: &[u8],
) -> Result<(), String> {
    let decoded = C::decode(fixture).map_err(|e| format!("the fixture did not decode: {e:?}"))?;
    if decoded != *expected {
        return Err(format!("decode gave {decoded:?}, expected {expected:?}"));
    }
    let encoded =
        C::encode(&decoded).map_err(|e| format!("the decoded body did not encode: {e:?}"))?;
    if encoded != reencoded {
        return Err(format!(
            "re-encode gave {encoded:02X?}, expected {reencoded:02X?}"
        ));
    }
    Ok(())
}

/// One fixed-width field of a fixture, for building the expected `Truncated` errors.
#[derive(Debug, Clone, Copy)]
pub struct Field {
    /// Offset of the field in the value.
    pub offset: usize,
    /// Width of the field in bytes.
    pub width: usize,
}

/// Expected error per prefix length for a value that is a plain sequence of
/// `fields`: a prefix is `Truncated` at the first field it does not fully
/// contain, including a prefix that ends exactly before that field.
pub fn truncated_in(fields: &[Field]) -> impl Fn(usize) -> DecodeError + '_ {
    move |prefix_len| {
        let cut = fields
            .iter()
            .find(|field| field.offset + field.width > prefix_len)
            .expect("the prefix is shorter than the whole value");
        DecodeError::Truncated {
            needed: cut.width,
            remaining: prefix_len - cut.offset,
        }
    }
}

const ENABLED: u8 = 0x80;
const URGENT: u8 = 0x40;

/// Flag bits of [`Sample`]; bits 5..0 are reserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Flags {
    pub enabled: bool,
    pub urgent: bool,
}

/// One element of the counted list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub channel: u8,
    pub weight: u16,
}

/// Sample body without a tail. An empty value is accepted and means "all zero".
#[derive(Debug, PartialEq, Eq, Default)]
pub struct Sample {
    pub flags: Flags,
    pub id: u16,
    pub entries: Vec<Entry>,
}

impl Sample {
    fn decode(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let flag_byte = r.u8()?;
        let flags = Flags {
            enabled: flag_byte & ENABLED != 0,
            urgent: flag_byte & URGENT != 0,
        };
        let id = r.be_u16()?;
        let count = r.u8()?;
        // Capacity from the wire count, capped by what the input can hold
        // (3 bytes per entry), so a template for wider counts cannot be made
        // to allocate more than the value's size.
        let mut entries = Vec::with_capacity(usize::from(count).min(r.remaining() / 3));
        for _ in 0..count {
            let channel = r.u8()?;
            let weight = r.be_u16()?;
            entries.push(Entry { channel, weight });
        }
        Ok(Self { flags, id, entries })
    }

    fn encode(&self, w: &mut Writer) -> Result<(), EncodeError> {
        let mut flag_byte = 0;
        if self.flags.enabled {
            flag_byte |= ENABLED;
        }
        if self.flags.urgent {
            flag_byte |= URGENT;
        }
        w.u8(flag_byte);
        w.be_u16(self.id);
        w.count_u8(self.entries.len())?;
        for entry in &self.entries {
            w.u8(entry.channel);
            w.be_u16(entry.weight);
        }
        Ok(())
    }

    /// Decodes a TLV value; an empty value gives the default body.
    pub fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        if value.is_empty() {
            return Ok(Self::default());
        }
        let mut reader = Reader::new(value);
        let body = Self::decode(&mut reader)?;
        reader.finish()?;
        Ok(body)
    }

    /// Encodes the TLV value; the bytes exist only if encoding succeeded.
    pub fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        self.encode(&mut writer)?;
        Ok(writer.into_bytes())
    }
}

/// [`Sample`] followed by a tail that runs to the end of the TLV.
#[derive(Debug, PartialEq, Eq)]
pub struct TailSample<'a> {
    pub sample: Sample,
    pub tail: &'a [u8],
}

impl<'a> TailSample<'a> {
    /// Decodes a TLV value; every byte after the entries is the tail.
    pub fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let sample = Sample::decode(&mut reader)?;
        let tail = reader.rest();
        reader.finish()?;
        Ok(Self { sample, tail })
    }

    /// Encodes the TLV value; the bytes exist only if encoding succeeded.
    pub fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        self.sample.encode(&mut writer)?;
        writer.bytes(self.tail);
        Ok(writer.into_bytes())
    }
}

/// [`Codec`] of [`Sample`].
pub struct SampleCodec;

impl Codec for SampleCodec {
    type Body<'a> = Sample;

    fn decode(value: &[u8]) -> Result<Sample, DecodeError> {
        Sample::decode_value(value)
    }

    fn encode(body: &Sample) -> Result<Vec<u8>, EncodeError> {
        body.encode_value()
    }
}

/// [`Codec`] of [`TailSample`].
pub struct TailSampleCodec;

impl Codec for TailSampleCodec {
    type Body<'a> = TailSample<'a>;

    fn decode(value: &[u8]) -> Result<TailSample<'_>, DecodeError> {
        TailSample::decode_value(value)
    }

    fn encode(body: &TailSample<'_>) -> Result<Vec<u8>, EncodeError> {
        body.encode_value()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::{MsgBuilder, RAW_BUDGET};
    use crate::header::CmduHeader;
    use crate::tlv::TlvIter;
    use em_types::{MsgType, TlvType};

    const SAMPLE_GOLDEN: [u8; 10] = [
        0xC0, 0x01, 0x02, 0x02, // flags, id, count
        0x24, 0x0A, 0x0B, // entry 1
        0x06, 0x0C, 0x0D, // entry 2
    ];
    const SAMPLE_FIELDS: [Field; 7] = [
        Field {
            offset: 0,
            width: 1,
        },
        Field {
            offset: 1,
            width: 2,
        },
        Field {
            offset: 3,
            width: 1,
        },
        Field {
            offset: 4,
            width: 1,
        },
        Field {
            offset: 5,
            width: 2,
        },
        Field {
            offset: 7,
            width: 1,
        },
        Field {
            offset: 8,
            width: 2,
        },
    ];
    const TAIL_GOLDEN: [u8; 10] = [
        0xC0, 0x01, 0x02, 0x01, // flags, id, count
        0x24, 0x0A, 0x0B, // entry
        0xDE, 0xAD, 0xBE, // tail
    ];
    const TAIL_FIELDS: [Field; 5] = [
        Field {
            offset: 0,
            width: 1,
        },
        Field {
            offset: 1,
            width: 2,
        },
        Field {
            offset: 3,
            width: 1,
        },
        Field {
            offset: 4,
            width: 1,
        },
        Field {
            offset: 5,
            width: 2,
        },
    ];
    /// Lengths of [`TAIL_GOLDEN`] that are complete values: header and entry, then any tail length.
    fn tail_accepted(len: usize) -> bool {
        (7..=10).contains(&len)
    }

    fn only_empty(len: usize) -> bool {
        len == 0
    }

    fn sample_golden() -> Sample {
        Sample {
            flags: Flags {
                enabled: true,
                urgent: true,
            },
            id: 0x0102,
            entries: vec![
                Entry {
                    channel: 0x24,
                    weight: 0x0A0B,
                },
                Entry {
                    channel: 0x06,
                    weight: 0x0C0D,
                },
            ],
        }
    }

    fn tail_golden() -> TailSample<'static> {
        TailSample {
            sample: Sample {
                flags: Flags {
                    enabled: true,
                    urgent: true,
                },
                id: 0x0102,
                entries: vec![Entry {
                    channel: 0x24,
                    weight: 0x0A0B,
                }],
            },
            tail: &[0xDE, 0xAD, 0xBE],
        }
    }

    fn too_many_entries(count: usize) -> Sample {
        Sample {
            flags: Flags::default(),
            id: 0,
            entries: vec![
                Entry {
                    channel: 1,
                    weight: 1
                };
                count
            ],
        }
    }

    // The helpers on the two good sample bodies.

    #[test]
    fn golden_round_trip() {
        assert_passes(check_golden::<SampleCodec>(
            &SAMPLE_GOLDEN,
            &sample_golden(),
        ));
        assert_passes(check_golden::<TailSampleCodec>(
            &TAIL_GOLDEN,
            &tail_golden(),
        ));
    }

    #[test]
    fn every_strict_prefix_is_an_error() {
        assert_passes(check_strict_prefixes::<SampleCodec>(
            &SAMPLE_GOLDEN,
            only_empty,
            truncated_in(&SAMPLE_FIELDS),
        ));
        assert_passes(check_strict_prefixes::<TailSampleCodec>(
            &TAIL_GOLDEN,
            tail_accepted,
            truncated_in(&TAIL_FIELDS),
        ));
    }

    #[test]
    fn extra_trailing_byte_is_trailing_bytes() {
        assert_passes(check_trailing_byte::<SampleCodec>(&SAMPLE_GOLDEN));
    }

    #[test]
    fn tail_body_takes_an_extra_byte_as_tail() {
        let mut longer = TAIL_GOLDEN.to_vec();
        longer.push(0xA5);
        let body = TailSample::decode_value(&longer).unwrap();
        assert_eq!(body.tail, &[0xDE, 0xAD, 0xBE, 0xA5]);
    }

    #[test]
    fn over_long_collection_is_count_too_large() {
        let expected = EncodeError::CountTooLarge { len: 256, max: 255 };
        assert_passes(check_count_too_large::<SampleCodec>(
            &too_many_entries(256),
            expected,
        ));
        let tail_body = TailSample {
            sample: too_many_entries(256),
            tail: &[],
        };
        assert_passes(check_count_too_large::<TailSampleCodec>(
            &tail_body, expected,
        ));
    }

    #[test]
    fn largest_collection_still_encodes() {
        let bytes = too_many_entries(255).encode_value().unwrap();
        assert_eq!(bytes.len(), 4 + 255 * 3);
        assert_eq!(bytes[3], 255);
    }

    #[test]
    fn reserved_bits_are_ignored_and_written_as_zero() {
        // 0xFF sets both flags and all six reserved bits.
        let mut all_reserved = SAMPLE_GOLDEN;
        all_reserved[0] = 0xFF;
        assert_passes(check_reserved_bits::<SampleCodec>(
            &all_reserved,
            &sample_golden(),
            &SAMPLE_GOLDEN,
        ));
        // 0x3F sets only the reserved bits, so both flags are false.
        let mut only_reserved = TAIL_GOLDEN;
        only_reserved[0] = 0x3F;
        let mut expected = tail_golden();
        expected.sample.flags = Flags::default();
        let mut canonical = TAIL_GOLDEN;
        canonical[0] = 0x00;
        assert_passes(check_reserved_bits::<TailSampleCodec>(
            &only_reserved,
            &expected,
            &canonical,
        ));
    }

    #[test]
    fn empty_value_is_accepted() {
        assert_passes(check_alternate_value::<SampleCodec>(
            &[],
            &Sample::default(),
            &[0, 0, 0, 0],
        ));
    }

    #[test]
    fn empty_tail_is_accepted() {
        let fixture = &TAIL_GOLDEN[..7];
        let expected = TailSample {
            tail: &[],
            ..tail_golden()
        };
        assert_passes(check_alternate_value::<TailSampleCodec>(
            fixture, &expected, fixture,
        ));
    }

    // Entry-point pattern.

    #[test]
    fn encoded_value_goes_through_msg_builder_and_decodes_back() {
        let header = CmduHeader {
            msg_type: MsgType::TOPOLOGY_QUERY,
            msg_id: 1,
            frag_id: 0,
            last_fragment: true,
            relay: false,
        };
        let value = sample_golden().encode_value().unwrap();
        let mut builder = MsgBuilder::new(&header, RAW_BUDGET);
        builder.tlv(TlvType(0x42), &value);
        let message = builder.finish().unwrap();

        let mut tlvs = TlvIter::new(&message[8..]);
        let tlv = tlvs.next().unwrap().unwrap();
        assert_eq!(tlv.tlv_type, TlvType(0x42));
        assert_eq!(Sample::decode_value(tlv.value), Ok(sample_golden()));
        assert!(tlvs.next().is_none());
    }

    #[test]
    fn failed_encode_publishes_nothing() {
        let header = CmduHeader {
            msg_type: MsgType::TOPOLOGY_QUERY,
            msg_id: 1,
            frag_id: 0,
            last_fragment: true,
            relay: false,
        };
        let mut builder = MsgBuilder::new(&header, RAW_BUDGET);
        // The caller adds the TLV only when encoding succeeded.
        match too_many_entries(256).encode_value() {
            Ok(value) => {
                builder.tlv(TlvType(0x42), &value);
            }
            Err(error) => assert_eq!(error, EncodeError::CountTooLarge { len: 256, max: 255 }),
        }
        let message = builder.finish().unwrap();
        // Header (8) and End of Message (3) only.
        assert_eq!(message.len(), 11);
    }

    // Each helper must fail on a deliberately wrong body or fixture.

    /// Does not call `finish`, so a trailing byte goes unnoticed.
    struct NoFinishCodec;

    impl Codec for NoFinishCodec {
        type Body<'a> = Sample;

        fn decode(value: &[u8]) -> Result<Sample, DecodeError> {
            Sample::decode(&mut Reader::new(value))
        }

        fn encode(body: &Sample) -> Result<Vec<u8>, EncodeError> {
            body.encode_value()
        }
    }

    /// Turns any decode error into the default body.
    struct SwallowsErrorsCodec;

    impl Codec for SwallowsErrorsCodec {
        type Body<'a> = Sample;

        fn decode(value: &[u8]) -> Result<Sample, DecodeError> {
            Ok(Sample::decode_value(value).unwrap_or_default())
        }

        fn encode(body: &Sample) -> Result<Vec<u8>, EncodeError> {
            body.encode_value()
        }
    }

    /// Turns an encode error into an empty value.
    struct SwallowsEncodeErrorCodec;

    impl Codec for SwallowsEncodeErrorCodec {
        type Body<'a> = Sample;

        fn decode(value: &[u8]) -> Result<Sample, DecodeError> {
            Sample::decode_value(value)
        }

        fn encode(body: &Sample) -> Result<Vec<u8>, EncodeError> {
            Ok(body.encode_value().unwrap_or_default())
        }
    }

    /// Writes a reserved bit.
    struct WritesReservedBitCodec;

    impl Codec for WritesReservedBitCodec {
        type Body<'a> = Sample;

        fn decode(value: &[u8]) -> Result<Sample, DecodeError> {
            Sample::decode_value(value)
        }

        fn encode(body: &Sample) -> Result<Vec<u8>, EncodeError> {
            let mut bytes = body.encode_value()?;
            bytes[0] |= 0x01;
            Ok(bytes)
        }
    }

    /// Loses the last byte on encode.
    struct DropsLastByteCodec;

    impl Codec for DropsLastByteCodec {
        type Body<'a> = Sample;

        fn decode(value: &[u8]) -> Result<Sample, DecodeError> {
            Sample::decode_value(value)
        }

        fn encode(body: &Sample) -> Result<Vec<u8>, EncodeError> {
            let mut bytes = body.encode_value()?;
            bytes.pop();
            Ok(bytes)
        }
    }

    #[test]
    fn golden_helper_rejects_a_wrong_body() {
        let mut wrong = sample_golden();
        wrong.id = 0x0201;
        assert!(check_golden::<SampleCodec>(&SAMPLE_GOLDEN, &wrong).is_err());
    }

    #[test]
    fn golden_helper_rejects_a_wrong_fixture() {
        let mut wrong = SAMPLE_GOLDEN;
        wrong[3] = 3;
        assert!(check_golden::<SampleCodec>(&wrong, &sample_golden()).is_err());
    }

    #[test]
    fn golden_helper_rejects_a_codec_that_encodes_differently() {
        assert!(check_golden::<DropsLastByteCodec>(&SAMPLE_GOLDEN, &sample_golden()).is_err());
    }

    #[test]
    fn prefix_helper_rejects_a_codec_that_accepts_truncated_values() {
        let result = check_strict_prefixes::<SwallowsErrorsCodec>(
            &SAMPLE_GOLDEN,
            only_empty,
            truncated_in(&SAMPLE_FIELDS),
        );
        assert!(result.is_err());
    }

    #[test]
    fn prefix_helper_rejects_a_wrong_expected_error() {
        let wrong = |_len: usize| DecodeError::TrailingBytes { remaining: 1 };
        assert!(check_strict_prefixes::<SampleCodec>(&SAMPLE_GOLDEN, only_empty, wrong).is_err());
    }

    #[test]
    fn prefix_helper_rejects_an_expected_error_off_by_one() {
        let off_by_one = |len: usize| match truncated_in(&SAMPLE_FIELDS)(len) {
            DecodeError::Truncated { needed, remaining } => DecodeError::Truncated {
                needed,
                remaining: remaining + 1,
            },
            DecodeError::TrailingBytes { remaining } => DecodeError::TrailingBytes { remaining },
        };
        assert!(
            check_strict_prefixes::<SampleCodec>(&SAMPLE_GOLDEN, only_empty, off_by_one).is_err()
        );
    }

    #[test]
    fn prefix_helper_rejects_a_missing_allow_list_entry() {
        // The empty value is accepted by the body, so leaving it off the list must fail.
        let result = check_strict_prefixes::<SampleCodec>(
            &SAMPLE_GOLDEN,
            |_| false,
            truncated_in(&SAMPLE_FIELDS),
        );
        assert!(result.is_err());
    }

    #[test]
    fn prefix_helper_rejects_an_allow_list_entry_that_is_an_error() {
        let result = check_strict_prefixes::<SampleCodec>(
            &SAMPLE_GOLDEN,
            |len| len <= 1,
            truncated_in(&SAMPLE_FIELDS),
        );
        assert!(result.is_err());
    }

    #[test]
    fn trailing_helper_rejects_a_codec_that_ignores_trailing_bytes() {
        assert!(check_trailing_byte::<NoFinishCodec>(&SAMPLE_GOLDEN).is_err());
    }

    #[test]
    fn trailing_helper_rejects_a_body_with_a_tail() {
        assert!(check_trailing_byte::<TailSampleCodec>(&TAIL_GOLDEN).is_err());
    }

    #[test]
    fn count_helper_rejects_a_codec_that_swallows_the_error() {
        let expected = EncodeError::CountTooLarge { len: 256, max: 255 };
        let result =
            check_count_too_large::<SwallowsEncodeErrorCodec>(&too_many_entries(256), expected);
        assert!(result.is_err());
    }

    #[test]
    fn count_helper_rejects_a_collection_that_fits() {
        let expected = EncodeError::CountTooLarge { len: 255, max: 255 };
        assert!(check_count_too_large::<SampleCodec>(&too_many_entries(255), expected).is_err());
    }

    #[test]
    fn count_helper_rejects_a_wrong_expected_error() {
        let wrong = EncodeError::CountTooLarge { len: 256, max: 256 };
        assert!(check_count_too_large::<SampleCodec>(&too_many_entries(256), wrong).is_err());
    }

    #[test]
    fn reserved_helper_rejects_a_codec_that_writes_reserved_bits() {
        let mut fixture = SAMPLE_GOLDEN;
        fixture[0] = 0xFF;
        let result = check_reserved_bits::<WritesReservedBitCodec>(
            &fixture,
            &sample_golden(),
            &SAMPLE_GOLDEN,
        );
        assert!(result.is_err());
    }

    #[test]
    fn reserved_helper_rejects_a_fixture_without_reserved_bits() {
        let result =
            check_reserved_bits::<SampleCodec>(&SAMPLE_GOLDEN, &sample_golden(), &SAMPLE_GOLDEN);
        assert!(result.is_err());
    }

    #[test]
    fn reserved_helper_rejects_values_that_change_with_reserved_bits() {
        let mut fixture = SAMPLE_GOLDEN;
        fixture[0] = 0x3F;
        // Reserved bits do not carry the flags, so the expected body must have none set.
        let result = check_reserved_bits::<SampleCodec>(&fixture, &sample_golden(), &SAMPLE_GOLDEN);
        assert!(result.is_err());
    }

    #[test]
    fn alternate_helper_rejects_a_wrong_reencoding() {
        let result = check_alternate_value::<SampleCodec>(&[], &Sample::default(), &[]);
        assert!(result.is_err());
    }

    #[test]
    fn alternate_helper_rejects_a_wrong_decoded_value() {
        let result = check_alternate_value::<SampleCodec>(&[], &sample_golden(), &[0, 0, 0, 0]);
        assert!(result.is_err());
    }

    #[test]
    fn assert_passes_panics_on_err() {
        let result = std::panic::catch_unwind(|| assert_passes(Err("boom".to_string())));
        assert!(result.is_err());
    }
}
