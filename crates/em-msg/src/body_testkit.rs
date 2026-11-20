// SPDX-License-Identifier: Apache-2.0

//! Shared test helpers for TLV bodies (design note 0008, D3) and three sample
//! bodies that exercise them.
//!
//! A body's tests call the `body_checks!` macro with its fixtures. The macro
//! generates a [`Codec`] that forwards to the body's `decode_value` and
//! `encode_value`, and calls the `check_*` helper for each D3 item and each
//! lenient item. The helpers can also be called directly with a `Codec`. They
//! return `Err(message)` instead of panicking, so the tests below can show that
//! each one rejects a deliberately wrong body or fixture. [`assert_passes`] turns an `Err` into a
//! test failure.
//!
//! `body_checks!` runs the checks on one body from a list of fixtures and
//! expectations. It also runs a round trip through `MsgBuilder::body` and
//! `TlvIter`, which checks that the End of Message TLV follows the body, and a
//! wrong-type check. Without the `lenient;` marker, it also checks that the body
//! has no lenient form.
//!
//! The sample bodies are not upstream bodies and are not public API. Each has a
//! test TLV type that `em-types` does not name:
//!
//! - [`Sample`] has no tail, so any extra byte is an error; an empty value is accepted.
//! - [`TailSample`] is [`Sample`] followed by a tail that runs to the end of the TLV.
//! - [`LenientSample`] is [`Sample`] that rejects an empty value, with a lenient
//!   form that returns `Ok(None)` for it and ignores trailing bytes.
//!
//! Wire layout of [`Sample`]: flags (1 byte: bit 7 enabled, bit 6 urgent,
//! bits 5..0 reserved), id (big-endian u16), entry count (1 byte), then per
//! entry a channel (1 byte) and a weight (big-endian u16).

use core::fmt::Debug;

use em_types::TlvType;

use crate::bodies::{TlvBody, TlvBodyLenient};
use crate::body::{Reader, Writer};
use crate::body_fixtures::{self, Key, Kind, Row};
use crate::builder::{MAX_TLV_VALUE, MsgBuilder};
use crate::error::{DecodeError, EncodeError};
use crate::header::CmduHeader;
use crate::tlv::{TLV_HEADER_LEN, Tlv, TlvIter};

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

/// Lenient item: `fixture` is upstream's empty form, so the lenient decode gives
/// `Ok(None)` (also through `Tlv::decode_lenient`) and the strict decode rejects it.
pub fn check_lenient_absent<'a, B>(_witness: &B, fixture: &'a [u8]) -> Result<(), String>
where
    B: TlvBodyLenient<'a> + Debug + PartialEq,
{
    if !fixture.is_empty() {
        return Err("only an empty value may decode to Ok(None)".to_string());
    }
    match B::decode_value_lenient(fixture) {
        Ok(None) => {}
        other => return Err(format!("lenient decode gave {other:?}, expected Ok(None)")),
    }
    let tlv = Tlv {
        tlv_type: B::TLV_TYPE,
        value: fixture,
    };
    match tlv.decode_lenient::<B>() {
        Ok(None) => {}
        other => {
            return Err(format!(
                "Tlv::decode_lenient gave {other:?}, expected Ok(None)"
            ));
        }
    }
    if let Ok(body) = B::decode_value(fixture) {
        return Err(format!(
            "the strict decode accepted the empty value: {body:?}"
        ));
    }
    Ok(())
}

/// Lenient item: the fixture decodes leniently to `Ok(Some(expected))` (also
/// through `Tlv::decode_lenient`), the strict decode rejects the same bytes
/// (also through `Tlv::decode`), and `expected` encodes to `reencoded`.
pub fn check_lenient_value<'a, B>(
    fixture: &'a [u8],
    expected: &B,
    reencoded: &[u8],
) -> Result<(), String>
where
    B: TlvBodyLenient<'a> + Debug + PartialEq,
{
    match B::decode_value_lenient(fixture) {
        Ok(Some(decoded)) if decoded == *expected => {}
        other => {
            return Err(format!(
                "lenient decode gave {other:?}, expected Ok(Some({expected:?}))"
            ));
        }
    }
    let tlv = Tlv {
        tlv_type: B::TLV_TYPE,
        value: fixture,
    };
    match tlv.decode_lenient::<B>() {
        Ok(Some(decoded)) if decoded == *expected => {}
        other => {
            return Err(format!(
                "Tlv::decode_lenient gave {other:?}, expected Ok(Some({expected:?}))"
            ));
        }
    }
    if let Ok(body) = B::decode_value(fixture) {
        return Err(format!(
            "the strict decode accepted the lenient fixture: {body:?}"
        ));
    }
    if let Ok(body) = tlv.decode::<B>() {
        return Err(format!(
            "Tlv::decode accepted the lenient fixture: {body:?}"
        ));
    }
    let encoded = expected
        .encode_value()
        .map_err(|e| format!("the lenient body did not encode: {e:?}"))?;
    if encoded != reencoded {
        return Err(format!(
            "re-encode gave {encoded:02X?}, expected {reencoded:02X?}"
        ));
    }
    Ok(())
}

/// Lenient item (note 0009 D4): the lenient decoders accept every spec form, so
/// `golden` (the spec-form fixture) decodes leniently to exactly
/// `Ok(Some(expected))`, through `decode_value_lenient` and `Tlv::decode_lenient`.
/// Lenient decoding only normalises non-spec forms.
pub fn check_lenient_golden<'a, B>(golden: &'a [u8], expected: &B) -> Result<(), String>
where
    B: TlvBodyLenient<'a> + Debug + PartialEq,
{
    match B::decode_value_lenient(golden) {
        Ok(Some(decoded)) if decoded == *expected => {}
        other => {
            return Err(format!(
                "lenient decode of the golden gave {other:?}, expected Ok(Some({expected:?}))"
            ));
        }
    }
    let tlv = Tlv {
        tlv_type: B::TLV_TYPE,
        value: golden,
    };
    match tlv.decode_lenient::<B>() {
        Ok(Some(decoded)) if decoded == *expected => Ok(()),
        other => Err(format!(
            "Tlv::decode_lenient of the golden gave {other:?}, expected Ok(Some({expected:?}))"
        )),
    }
}

/// Lenient item (note 0009 D4): `fixture` is a spec form, one the strict decoder
/// accepts (an `alternates:` or `reserved_bits:` fixture). The lenient decoder
/// accepts every spec form, so it must give exactly `Ok(Some(body))` with the
/// body the strict decoder gives, through `decode_value_lenient` and
/// `Tlv::decode_lenient`. `Ok(None)`, an error or a different body fails.
pub fn check_lenient_matches_strict<'a, B>(_witness: &B, fixture: &'a [u8]) -> Result<(), String>
where
    B: TlvBodyLenient<'a> + Debug + PartialEq,
{
    let strict = B::decode_value(fixture).map_err(|e| {
        format!("the strict decode of {fixture:02X?} gave {e:?}, so it is not a spec form")
    })?;
    match B::decode_value_lenient(fixture) {
        Ok(Some(decoded)) if decoded == strict => {}
        other => {
            return Err(format!(
                "lenient decode of the spec form {fixture:02X?} gave {other:?}, expected Ok(Some({strict:?}))"
            ));
        }
    }
    let tlv = Tlv {
        tlv_type: B::TLV_TYPE,
        value: fixture,
    };
    match tlv.decode_lenient::<B>() {
        Ok(Some(decoded)) if decoded == strict => Ok(()),
        other => Err(format!(
            "Tlv::decode_lenient of the spec form {fixture:02X?} gave {other:?}, expected Ok(Some({strict:?}))"
        )),
    }
}

/// Lenient item (note 0009 D2 and D4): every non-empty prefix of `golden`, the
/// whole `golden` included, is checked through `decode_value_lenient` and through
/// `Tlv::decode_lenient`.
///
/// - No non-empty prefix may give `Ok(None)` through either lenient entry point. The
///   nine zero octets of the 0xB0 absent form (note 0009, A7) are not a prefix checked
///   here; that form has its own test.
/// - Where the strict decoder accepts a prefix, the lenient result must be exactly
///   `Ok(Some(the strict body))`.
///
/// A prefix the strict decoder rejects may be `Ok(Some(..))` or an error, as the
/// lenient form decides.
pub fn check_lenient_prefixes<'a, B>(_witness: &B, golden: &'a [u8]) -> Result<(), String>
where
    B: TlvBodyLenient<'a> + Debug + PartialEq,
{
    for len in 1..=golden.len() {
        let prefix = &golden[..len];
        let strict = B::decode_value(prefix);
        let tlv = Tlv {
            tlv_type: B::TLV_TYPE,
            value: prefix,
        };
        let results = [
            ("lenient decode", B::decode_value_lenient(prefix)),
            ("Tlv::decode_lenient", tlv.decode_lenient::<B>()),
        ];
        for (name, lenient) in results {
            if let Ok(None) = lenient {
                return Err(format!(
                    "{name} of the non-empty prefix of {len} bytes gave Ok(None)"
                ));
            }
            if let Ok(strict_body) = &strict {
                match &lenient {
                    Ok(Some(body)) if body == strict_body => {}
                    other => {
                        return Err(format!(
                            "{name} of the prefix of {len} bytes, which the strict decoder accepts, gave {other:?}, expected Ok(Some({strict_body:?}))"
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

/// Lenient item: the non-empty, malformed `fixture` gives exactly
/// `Err(expected)` through `decode_value_lenient` and `Tlv::decode_lenient`.
pub fn check_lenient_rejects<'a, B>(
    _witness: &B,
    fixture: &'a [u8],
    expected: DecodeError,
) -> Result<(), String>
where
    B: TlvBodyLenient<'a> + Debug + PartialEq,
{
    if fixture.is_empty() {
        return Err("an empty value is not a malformed lenient fixture".to_string());
    }
    match B::decode_value_lenient(fixture) {
        Err(error) if error == expected => {}
        other => {
            return Err(format!(
                "lenient decode gave {other:?}, expected Err({expected:?})"
            ));
        }
    }
    let tlv = Tlv {
        tlv_type: B::TLV_TYPE,
        value: fixture,
    };
    match tlv.decode_lenient::<B>() {
        Err(error) if error == expected => Ok(()),
        other => Err(format!(
            "Tlv::decode_lenient gave {other:?}, expected Err({expected:?})"
        )),
    }
}

/// Fixture sweep (note 0009 D3 and D4). Every row of a fixture group is passed to
/// `check_row` with its bytes and, for a row named in `canonical_rows`, the canonical bytes.
/// `check_row` dispatches on the row's TLV code, so a row cannot go unchecked. The row's
/// `kind` selects the checks:
///
/// - `spec` (the default): the strict decode succeeds through `decode_value` and `Tlv::decode`,
///   and the body encodes to bytes that decode to the same body. Those bytes equal the row,
///   unless the row has a nonzero reserved field or `canonical` is given. `canonical` is the
///   bytes a row that is not canonical must encode to (a sibling row of the group). A lenient
///   body must decode the row leniently to `Ok(Some(the same body))`.
/// - `invalid`: the strict decode gives the error variant named by `error=`, through both entry
///   points. A lenient body must give an error through both lenient entry points too, unless
///   the row is named in the body's lenient inputs (the last argument of `sweep_row_lenient`).
/// - `lenient`: the strict decode and `Tlv::decode` fail, the lenient decode and
///   `Tlv::decode_lenient` both give `Ok(Some(body))`, and the body encodes to bytes that the
///   strict decoder accepts and that give the same body. A row with `decoded=absent` must have
///   an empty value and give `Ok(None)` through both lenient entry points instead, and has
///   nothing to re-encode.
///
/// A dispatcher returns `Err` for a TLV code it has no body for. The sweep then fails, with the
/// message of every row that failed.
pub fn sweep_group(
    group: &str,
    canonical_rows: &[(Key, Key)],
    check_row: impl Fn(&Row<'_>, &[u8], Option<&[u8]>) -> Result<(), String>,
) {
    let rows = body_fixtures::all_rows(group);
    assert!(!rows.is_empty(), "the {group} fixtures have no rows");
    let mut failures = Vec::new();
    let mut canonical_used = 0;
    for row in &rows {
        let bytes = row.bytes();
        let canonical_key = canonical_rows
            .iter()
            .find(|&&(key, _)| key == row.key())
            .map(|&(_, canonical_key)| canonical_key);
        if canonical_key.is_some() {
            canonical_used += 1;
        }
        let canonical = canonical_key.map(|key| body_fixtures::value(group, key));
        if let Err(message) = check_row(row, &bytes, canonical.as_deref()) {
            failures.push(format!("{group}/{}/{}: {message}", row.tlv, row.case));
        }
    }
    assert_eq!(
        canonical_used,
        canonical_rows.len(),
        "a canonical row names no {group} fixture"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Leaks the re-encoded bytes so they have a `'static` lifetime, which a body that borrows its
/// value can decode from. The test process frees them when it ends; a fixture is a few bytes.
fn leak(bytes: Vec<u8>) -> &'static [u8] {
    Box::leak(bytes.into_boxed_slice())
}

/// The name of an error's variant, to compare with a row's `error=` field.
fn error_variant(error: &DecodeError) -> String {
    let text = format!("{error:?}");
    let end = text.find([' ', '(', '{']).unwrap_or(text.len());
    text[..end].to_string()
}

/// Fails when the `tlv` column of a fixture row (`0x01`) is not the TLV type of body `B`, so a
/// dispatcher cannot check a row against a body of another TLV type that has the same layout.
pub fn check_tlv_column<'a, B: TlvBody<'a>>(column: &str) -> Result<(), String> {
    let parsed = column
        .strip_prefix("0x")
        .and_then(|digits| u8::from_str_radix(digits, 16).ok())
        .ok_or_else(|| format!("the row TLV column {column:?} is not of the form 0xNN"))?;
    if TlvType(parsed) != B::TLV_TYPE {
        return Err(format!(
            "row TLV {column} dispatched to a body of TLV {:?}",
            B::TLV_TYPE
        ));
    }
    Ok(())
}

fn check_strict_row<'a, B>(
    row: &Row<'_>,
    bytes: &'a [u8],
    canonical: Option<&[u8]>,
) -> Result<(), String>
where
    B: TlvBody<'a> + Debug + PartialEq,
{
    check_tlv_column::<B>(row.tlv)?;
    let tlv = Tlv {
        tlv_type: B::TLV_TYPE,
        value: bytes,
    };
    match row.kind() {
        Kind::Spec => {
            let body = B::decode_value(bytes)
                .map_err(|e| format!("the strict decode of a spec row gave {e:?}"))?;
            match tlv.decode::<B>() {
                Ok(through_tlv) if through_tlv == body => {}
                other => return Err(format!("Tlv::decode gave {other:?}, expected {body:?}")),
            }
            let encoded = leak(
                body.encode_value()
                    .map_err(|e| format!("the decoded body did not encode: {e:?}"))?,
            );
            match B::decode_value(encoded) {
                Ok(again) if again == body => {}
                other => {
                    return Err(format!(
                        "the re-encoded bytes {encoded:02X?} decode to {other:?}, expected {body:?}"
                    ));
                }
            }
            if let Some(canonical) = canonical {
                if canonical == bytes {
                    return Err(
                        "the canonical bytes equal the row, so it tests nothing".to_string()
                    );
                }
                if encoded != canonical {
                    return Err(format!(
                        "re-encode gave {encoded:02X?}, expected the canonical {canonical:02X?}"
                    ));
                }
            } else if encoded != bytes && !row.names_reserved_bits() {
                return Err(format!(
                    "re-encode gave {encoded:02X?}, expected {bytes:02X?}, and the row names no reserved bits"
                ));
            }
            Ok(())
        }
        Kind::Invalid => {
            let name = row
                .field("error")
                .ok_or_else(|| "an invalid row has no error= field".to_string())?;
            match B::decode_value(bytes) {
                Err(error) if error_variant(&error) == name => {}
                other => return Err(format!("the strict decode gave {other:?}, expected {name}")),
            }
            match tlv.decode::<B>() {
                Err(error) if error_variant(&error) == name => Ok(()),
                other => Err(format!("Tlv::decode gave {other:?}, expected {name}")),
            }
        }
        Kind::Lenient => Err("a lenient row needs a body with a lenient form".to_string()),
    }
}

/// Sweep check of one row for a body with no lenient form.
pub fn sweep_row<'a, B>(
    row: &Row<'_>,
    bytes: &'a [u8],
    canonical: Option<&[u8]>,
) -> Result<(), String>
where
    B: TlvBody<'a> + Debug + PartialEq,
{
    check_strict_row::<B>(row, bytes, canonical)
}

/// Sweep check of one row for a body with a lenient form.
///
/// An `invalid` row must also be an error through both lenient decoders, unless `lenient_inputs`
/// names the row: that is for a row the lenient form accepts on purpose (upstream's empty Vendor
/// Specific value gives `Ok(None)`, and a zero-radio Device Inventory decodes leniently to a
/// body that cannot be encoded). The listed rows are not checked against the lenient
/// decoders, and an `invalid` row is never re-encoded, so a body that refuses to encode what
/// only the lenient decoder accepts passes.
pub fn sweep_row_lenient<'a, B>(
    row: &Row<'_>,
    bytes: &'a [u8],
    canonical: Option<&[u8]>,
    lenient_inputs: &[Key],
) -> Result<(), String>
where
    B: TlvBodyLenient<'a> + Debug + PartialEq,
{
    check_tlv_column::<B>(row.tlv)?;
    let tlv = Tlv {
        tlv_type: B::TLV_TYPE,
        value: bytes,
    };
    let lenient = [
        ("lenient decode", B::decode_value_lenient(bytes)),
        ("Tlv::decode_lenient", tlv.decode_lenient::<B>()),
    ];
    match row.kind() {
        Kind::Lenient => {
            if let Ok(body) = B::decode_value(bytes) {
                return Err(format!(
                    "the strict decode accepted a lenient row: {body:?}"
                ));
            }
            if let Ok(body) = tlv.decode::<B>() {
                return Err(format!("Tlv::decode accepted a lenient row: {body:?}"));
            }
            if row.field("decoded") == Some("absent") {
                // A decoded=absent row is the empty value only; a lenient decoder that
                // swallows other bytes would hide a real body behind Ok(None).
                if !bytes.is_empty() {
                    return Err(format!(
                        "a decoded=absent row must have an empty value, got {bytes:02X?}"
                    ));
                }
                for (name, result) in lenient {
                    if !matches!(result, Ok(None)) {
                        return Err(format!(
                            "{name} of a decoded=absent row gave {result:?}, expected Ok(None)"
                        ));
                    }
                }
                return Ok(());
            }
            let [(_, first), (_, second)] = lenient;
            let Ok(Some(body)) = first else {
                return Err(format!(
                    "the lenient decode gave {first:?}, expected Ok(Some(..))"
                ));
            };
            if !matches!(&second, Ok(Some(other)) if *other == body) {
                return Err(format!(
                    "Tlv::decode_lenient gave {second:?}, expected Ok(Some({body:?}))"
                ));
            }
            let encoded = leak(
                body.encode_value()
                    .map_err(|e| format!("the lenient body did not encode: {e:?}"))?,
            );
            match B::decode_value(encoded) {
                Ok(again) if again == body => Ok(()),
                other => Err(format!(
                    "the re-encoded bytes {encoded:02X?} decode to {other:?}, expected {body:?}"
                )),
            }
        }
        Kind::Spec => {
            check_strict_row::<B>(row, bytes, canonical)?;
            let strict = B::decode_value(bytes).map_err(|e| format!("{e:?}"))?;
            for (name, result) in lenient {
                if !matches!(&result, Ok(Some(other)) if *other == strict) {
                    return Err(format!(
                        "{name} of a spec row gave {result:?}, expected Ok(Some({strict:?}))"
                    ));
                }
            }
            Ok(())
        }
        Kind::Invalid => {
            check_strict_row::<B>(row, bytes, canonical)?;
            if lenient_inputs.iter().any(|&key| key == row.key()) {
                return Ok(());
            }
            for (name, result) in lenient {
                if let Ok(accepted) = result {
                    return Err(format!(
                        "{name} of an invalid row gave Ok({accepted:?}), expected an error; \
                         list the row as a lenient input if the lenient form accepts it"
                    ));
                }
            }
            Ok(())
        }
    }
}

/// Budget that fits a header (8 bytes), one TLV with a value of the largest
/// size (3 bytes of TLV header and 65535 of value) and End of Message (3 bytes).
pub const MAX_TLV_MESSAGE_BUDGET: usize = 8 + 3 + MAX_TLV_VALUE + 3;

/// Builds a whole CMDU holding only `body`, through `MsgBuilder::body`, with a
/// budget that fits a body at the 65 535-byte TLV value limit.
pub fn build_message<'a, B: TlvBody<'a>>(body: &B) -> Result<Vec<u8>, String> {
    build_message_with_budget(body, MAX_TLV_MESSAGE_BUDGET)
}

/// Like [`build_message`], with the byte budget of the whole CMDU given.
pub fn build_message_with_budget<'a, B: TlvBody<'a>>(
    body: &B,
    budget: usize,
) -> Result<Vec<u8>, String> {
    let header = CmduHeader {
        msg_type: em_types::MsgType::TOPOLOGY_QUERY,
        msg_id: 1,
        frag_id: 0,
        last_fragment: true,
        relay: false,
    };
    let mut builder = MsgBuilder::new(&header, budget);
    builder.body(body);
    builder
        .finish()
        .map_err(|e| format!("the message did not build: {e:?}"))
}

/// Returns the one TLV of a message made by [`build_message`], after checking
/// that exactly the End of Message TLV (`00 00 00`) follows it.
pub fn single_tlv(message: &[u8]) -> Result<Tlv<'_>, String> {
    let (_, tlv_area) =
        CmduHeader::parse(message).map_err(|e| format!("the header did not parse: {e:?}"))?;
    let mut tlvs = TlvIter::new(tlv_area);
    let first = tlvs
        .next()
        .ok_or_else(|| "the message holds no TLV".to_string())?
        .map_err(|e| format!("the TLV did not frame: {e:?}"))?;
    if tlvs.next().is_some() {
        return Err("the message holds more than one TLV".to_string());
    }
    let after_tlv = &tlv_area[TLV_HEADER_LEN + first.value.len()..];
    if after_tlv != [0x00, 0x00, 0x00] {
        return Err(format!(
            "the bytes after the TLV are {after_tlv:02X?}, expected the End of Message [00, 00, 00]"
        ));
    }
    Ok(first)
}

/// `tlv` has the body's TLV type and `Tlv::decode` gives `expected`.
pub fn check_tlv_decodes<'a, B>(tlv: Tlv<'a>, expected: &B) -> Result<(), String>
where
    B: TlvBody<'a> + Debug + PartialEq,
{
    if tlv.tlv_type != B::TLV_TYPE {
        return Err(format!(
            "the TLV has type {:?}, expected {:?}",
            tlv.tlv_type,
            B::TLV_TYPE
        ));
    }
    match tlv.decode::<B>() {
        Ok(decoded) if decoded == *expected => Ok(()),
        other => Err(format!("Tlv::decode gave {other:?}, expected {expected:?}")),
    }
}

/// A TLV of another type, with the same value, is refused by `Tlv::decode` with `WrongType`.
pub fn check_wrong_type<'a, B>(_witness: &B, value: &'a [u8]) -> Result<(), String>
where
    B: TlvBody<'a> + Debug,
{
    let (tlv, expected) = wrong_type_case::<B>(value);
    match tlv.decode::<B>() {
        Err(error) if error == expected => Ok(()),
        other => Err(format!("Tlv::decode gave {other:?}, expected {expected:?}")),
    }
}

/// Like [`check_wrong_type`], for `Tlv::decode_lenient`.
pub fn check_wrong_type_lenient<'a, B>(_witness: &B, value: &'a [u8]) -> Result<(), String>
where
    B: TlvBodyLenient<'a> + Debug,
{
    let (tlv, expected) = wrong_type_case::<B>(value);
    match tlv.decode_lenient::<B>() {
        Err(error) if error == expected => Ok(()),
        other => Err(format!(
            "Tlv::decode_lenient gave {other:?}, expected {expected:?}"
        )),
    }
}

/// A TLV whose type is not the body's, and the error decoding it must give.
fn wrong_type_case<'a, B: TlvBody<'a>>(value: &'a [u8]) -> (Tlv<'a>, DecodeError) {
    let other_type = TlvType(B::TLV_TYPE.0 ^ 0xFF);
    let tlv = Tlv {
        tlv_type: other_type,
        value,
    };
    let expected = DecodeError::WrongType {
        expected: B::TLV_TYPE,
        found: other_type,
    };
    (tlv, expected)
}

/// TLV type of [`Sample`]; unnamed in `em-types`.
pub const SAMPLE_TLV_TYPE: TlvType = TlvType(0xF8);
/// TLV type of [`TailSample`]; unnamed in `em-types`.
pub const TAIL_SAMPLE_TLV_TYPE: TlvType = TlvType(0xF9);
/// TLV type of [`LenientSample`]; unnamed in `em-types`.
pub const LENIENT_SAMPLE_TLV_TYPE: TlvType = TlvType(0xFA);

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
        // Capacity from the wire count, capped by how many 3-byte entries the
        // input can hold, so a large count in a short value does not reserve
        // room for the count.
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
}

impl<'a> TlvBody<'a> for Sample {
    const TLV_TYPE: TlvType = SAMPLE_TLV_TYPE;

    /// An empty value gives the default body.
    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        if value.is_empty() {
            return Ok(Self::default());
        }
        let mut reader = Reader::new(value);
        let body = Self::decode(&mut reader)?;
        reader.finish()?;
        Ok(body)
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
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

impl<'a> TlvBody<'a> for TailSample<'a> {
    const TLV_TYPE: TlvType = TAIL_SAMPLE_TLV_TYPE;

    /// Every byte after the entries is the tail.
    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let sample = Sample::decode(&mut reader)?;
        let tail = reader.rest();
        reader.finish()?;
        Ok(Self { sample, tail })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        self.sample.encode(&mut writer)?;
        writer.bytes(self.tail);
        Ok(writer.into_bytes())
    }
}

/// [`Sample`] that rejects an empty value.
///
/// The strict form is the spec form. The lenient form is upstream's: an empty
/// value is `Ok(None)` and bytes after the entries are ignored.
#[derive(Debug, PartialEq, Eq)]
pub struct LenientSample(pub Sample);

impl LenientSample {
    fn decode_strict(reader: &mut Reader<'_>) -> Result<Self, DecodeError> {
        Ok(Self(Sample::decode(reader)?))
    }
}

impl<'a> TlvBody<'a> for LenientSample {
    const TLV_TYPE: TlvType = LENIENT_SAMPLE_TLV_TYPE;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let body = Self::decode_strict(&mut reader)?;
        reader.finish()?;
        Ok(body)
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        self.0.encode_value()
    }
}

impl<'a> TlvBodyLenient<'a> for LenientSample {
    fn decode_value_lenient(value: &'a [u8]) -> Result<Option<Self>, DecodeError> {
        if value.is_empty() {
            return Ok(None);
        }
        let mut reader = Reader::new(value);
        let body = Self::decode_strict(&mut reader)?;
        Ok(Some(body))
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

/// Probe for [`assert_not_lenient!`], using autoref specialization: with `probe` set to
/// `Probe(&body)`, the method call `(&&probe).implements_lenient()` picks [`ProbeLenient`] if
/// the body type implements `TlvBodyLenient`, and [`ProbeStrict`] otherwise.
#[allow(dead_code)] // the field only gives the probe the body type
pub struct Probe<'p, T>(pub &'p T);

/// Chosen first, so only when `T` implements `TlvBodyLenient`.
pub trait ProbeLenient {
    fn implements_lenient(&self) -> bool;
}

impl<'a, T: TlvBodyLenient<'a>> ProbeLenient for &Probe<'_, T> {
    fn implements_lenient(&self) -> bool {
        true
    }
}

/// The fallback for every other body.
pub trait ProbeStrict {
    fn implements_lenient(&self) -> bool;
}

impl<T> ProbeStrict for Probe<'_, T> {
    fn implements_lenient(&self) -> bool {
        false
    }
}

/// Panics if the type of `$body` implements `TlvBodyLenient`. The strict arm of
/// `body_checks!` runs it, so a lenient body cannot get the strict checks only by
/// leaving out the `lenient;` marker.
macro_rules! assert_not_lenient {
    ($body:expr) => {{
        #[allow(unused_imports)]
        use $crate::body_testkit::{ProbeLenient, ProbeStrict};
        // Bound first: a temporary inside the probe would not live long enough on Rust 1.85.
        let body = $body;
        let probe = $crate::body_testkit::Probe(&body);
        assert!(
            !(&&probe).implements_lenient(),
            "the body implements TlvBodyLenient, so its body_checks! needs the `lenient;` marker"
        );
    }};
}

/// Fails to compile when a lenient item is used without the `lenient;` marker.
macro_rules! lenient_marker_required {
    (lenient) => {};
    (lenient_should_fail) => {};
    (strict) => {
        compile_error!("the lenient items need the `lenient;` marker right after `golden`");
    };
}

/// Generates the test for the strict arm of `body_checks!`, and the `lenient_forms`
/// test of a lenient body. A strict body gets `body_is_not_lenient`, which fails if
/// the body implements `TlvBodyLenient`. A lenient body gets `lenient_forms`, which
/// checks `golden` and its prefixes, the wrong-type case, and that the lenient
/// decoder agrees with the strict one on every strict fixture. The fixtures come
/// from `alternates:` and `reserved_bits:`, so a lenient body cannot skip them. For
/// `lenient_should_fail` the test must panic with a failed agreement check; the
/// testkit's own self-tests use that.
macro_rules! lenient_forms_test {
    (strict; $golden_bytes:expr, $golden_body:expr; $($unused:tt)*) => {
        #[test]
        fn body_is_not_lenient() {
            assert_not_lenient!($golden_body);
        }
    };
    (lenient; $($args:tt)*) => {
        lenient_forms_test!(@emit [#[test]] $($args)*);
    };
    (lenient_should_fail; $($args:tt)*) => {
        lenient_forms_test!(
            @emit [#[test] #[should_panic(expected = "lenient decode of the spec form")]]
            $($args)*
        );
    };
    (
        @emit [ $($attr:tt)* ]
        $golden_bytes:expr, $golden_body:expr;
        [ $( $strict_fixture:expr ),* $(,)? ]
    ) => {
        $($attr)*
        fn lenient_forms() {
            use $crate::body_testkit as kit;
            kit::assert_passes(kit::check_wrong_type_lenient(&$golden_body, $golden_bytes));
            kit::assert_passes(kit::check_lenient_golden($golden_bytes, &$golden_body));
            kit::assert_passes(kit::check_lenient_prefixes(&$golden_body, $golden_bytes));
            $(
                kit::assert_passes(kit::check_lenient_matches_strict(
                    &$golden_body,
                    $strict_fixture,
                ));
            )*
        }
    };
}

/// Generates one test module for one body. The `golden` test (D3 item 1), the
/// `message_round_trip` test (the golden body through `MsgBuilder::body`, with
/// `single_tlv` checking the TLV and the End of Message after it) and the
/// `wrong_type_is_refused` test always run. `mod`, `body` and
/// `golden` are required. Every other item is optional, and each item given adds
/// its test. The items must come in this order:
///
/// ```text
/// body_checks! {
///     mod sample_checks;                  // name of the generated test module
///     body: Sample;                       // `TailSample<'a>` for a body that borrows
///     golden: &GOLDEN_BYTES, golden_body_expr();
///     lenient;                                             // only for a body with a lenient form
///     prefixes: accepted_len_fn, expected_error_fn;        // D3 item 2
///     trailing_byte: &GOLDEN_BYTES;                        // D3 item 3
///     count_too_large: too_long_body_expr, expected_encode_error;  // D3 item 4
///     reserved_bits: [(fixture, expected_body, canonical_bytes), ...];  // D3 item 5
///     alternates: [(fixture, expected_body, reencoded_bytes), ...];     // D3 item 6
///     lenient_absent: [empty_fixture, ...];               // lenient `Ok(None)`
///     lenient: [(fixture, expected_body, reencoded_bytes), ...];        // lenient `Ok(Some)`
///     lenient_rejects: [(fixture, expected_decode_error), ...];         // lenient `Err`
/// }
/// ```
///
/// The `lenient;` marker is for a body that implements `TlvBodyLenient`. The
/// three `lenient_*` items are a compile error without it. Without the marker, the
/// `body_is_not_lenient` test fails for a body that implements `TlvBodyLenient`.
/// With the marker, the `lenient_forms` test always runs, even if no `lenient_*`
/// item is given. It checks the wrong-type case through `Tlv::decode_lenient`, and
/// that no non-empty prefix of `golden`, `golden` itself included, decodes leniently
/// to `Ok(None)`. Where the strict decoder accepts a prefix, the lenient decoder
/// must give the same body. It also checks that the lenient decoder accepts every spec form
/// (note 0009 D4): on `golden` and on every `alternates:` and `reserved_bits:`
/// fixture, which the strict decoder accepts, `decode_value_lenient` and
/// `Tlv::decode_lenient` must give exactly `Ok(Some(body))` with the body the
/// strict decoder gives. `lenient` fixtures must be rejected by the strict
/// decoder.
///
/// `lenient_should_fail;` replaces `lenient;` in the testkit's own self-tests: the
/// `lenient_forms` test must then panic with a failed agreement check.
///
/// Expressions are evaluated inside the generated module, which sees the items of the module
/// that calls the macro.
macro_rules! body_checks {
    // Entry points: read the marker, then expand with the mode.
    (
        mod $name:ident;
        body: $Body:ty;
        golden: $golden_bytes:expr, $golden_body:expr;
        lenient;
        $($items:tt)*
    ) => {
        body_checks! {
            @expand lenient;
            mod $name;
            body: $Body;
            golden: $golden_bytes, $golden_body;
            $($items)*
        }
    };
    (
        mod $name:ident;
        body: $Body:ty;
        golden: $golden_bytes:expr, $golden_body:expr;
        lenient_should_fail;
        $($items:tt)*
    ) => {
        body_checks! {
            @expand lenient_should_fail;
            mod $name;
            body: $Body;
            golden: $golden_bytes, $golden_body;
            $($items)*
        }
    };
    (
        mod $name:ident;
        body: $Body:ty;
        golden: $golden_bytes:expr, $golden_body:expr;
        $($items:tt)*
    ) => {
        body_checks! {
            @expand strict;
            mod $name;
            body: $Body;
            golden: $golden_bytes, $golden_body;
            $($items)*
        }
    };
    (
        @expand $mode:ident;
        mod $name:ident;
        body: $Body:ty;
        golden: $golden_bytes:expr, $golden_body:expr;
        $( prefixes: $accepted:expr, $prefix_error:expr; )?
        $( trailing_byte: $trailing_fixture:expr; )?
        $( count_too_large: $too_long:expr, $count_error:expr; )?
        $( reserved_bits: [ $( ($res_fixture:expr, $res_body:expr, $res_canonical:expr) ),* $(,)? ]; )?
        $( alternates: [ $( ($alt_fixture:expr, $alt_body:expr, $alt_reencoded:expr) ),* $(,)? ]; )?
        $( lenient_absent: [ $( $absent:expr ),* $(,)? ]; )?
        $( lenient: [ $( ($len_fixture:expr, $len_body:expr, $len_reencoded:expr) ),* $(,)? ]; )?
        $( lenient_rejects: [ $( ($rej_fixture:expr, $rej_error:expr) ),* $(,)? ]; )?
    ) => {
        mod $name {
            #[allow(unused_imports)]
            use super::*;
            use $crate::body_testkit as kit;

            struct BodyCodec;

            impl kit::Codec for BodyCodec {
                type Body<'a> = $Body;

                fn decode<'a>(
                    value: &'a [u8],
                ) -> Result<Self::Body<'a>, $crate::error::DecodeError> {
                    <$Body as $crate::bodies::TlvBody<'a>>::decode_value(value)
                }

                fn encode<'a>(
                    body: &Self::Body<'a>,
                ) -> Result<Vec<u8>, $crate::error::EncodeError> {
                    <$Body as $crate::bodies::TlvBody<'a>>::encode_value(body)
                }
            }

            #[test]
            fn golden() {
                kit::assert_passes(kit::check_golden::<BodyCodec>($golden_bytes, &$golden_body));
            }

            #[test]
            fn message_round_trip() {
                let message = kit::build_message(&$golden_body).unwrap();
                let tlv = kit::single_tlv(&message).unwrap();
                kit::assert_passes(kit::check_tlv_decodes(tlv, &$golden_body));
            }

            #[test]
            fn wrong_type_is_refused() {
                kit::assert_passes(kit::check_wrong_type(&$golden_body, $golden_bytes));
            }

            $(
                #[test]
                fn strict_prefixes() {
                    kit::assert_passes(kit::check_strict_prefixes::<BodyCodec>(
                        $golden_bytes,
                        $accepted,
                        $prefix_error,
                    ));
                }
            )?

            $(
                #[test]
                fn trailing_byte() {
                    kit::assert_passes(kit::check_trailing_byte::<BodyCodec>($trailing_fixture));
                }
            )?

            $(
                #[test]
                fn count_too_large() {
                    kit::assert_passes(kit::check_count_too_large::<BodyCodec>(
                        &$too_long,
                        $count_error,
                    ));
                }
            )?

            $(
                #[test]
                fn reserved_bits() {
                    $(
                        kit::assert_passes(kit::check_reserved_bits::<BodyCodec>(
                            $res_fixture,
                            &$res_body,
                            $res_canonical,
                        ));
                    )*
                }
            )?

            $(
                #[test]
                fn alternate_values() {
                    $(
                        kit::assert_passes(kit::check_alternate_value::<BodyCodec>(
                            $alt_fixture,
                            &$alt_body,
                            $alt_reencoded,
                        ));
                    )*
                }
            )?

            lenient_forms_test!(
                $mode;
                $golden_bytes, $golden_body;
                [ $( $( $alt_fixture, )* )? $( $( $res_fixture, )* )? ]
            );

            $(
                lenient_marker_required!($mode);

                #[test]
                fn lenient_absent() {
                    $(
                        kit::assert_passes(kit::check_lenient_absent(&$golden_body, $absent));
                    )*
                }
            )?

            $(
                lenient_marker_required!($mode);

                #[test]
                fn lenient_values() {
                    $(
                        kit::assert_passes(kit::check_lenient_value(
                            $len_fixture,
                            &$len_body,
                            $len_reencoded,
                        ));
                    )*
                }
            )?

            $(
                lenient_marker_required!($mode);

                #[test]
                fn lenient_rejects() {
                    $(
                        kit::assert_passes(kit::check_lenient_rejects(
                            &$golden_body,
                            $rej_fixture,
                            $rej_error,
                        ));
                    )*
                }
            )?
        }
    };
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

    /// [`SAMPLE_GOLDEN`] followed by one byte that the lenient form ignores.
    const LENIENT_TRAILING: [u8; 11] = [
        0xC0, 0x01, 0x02, 0x02, // flags, id, count
        0x24, 0x0A, 0x0B, // entry 1
        0x06, 0x0C, 0x0D, // entry 2
        0xA5, // ignored by the lenient form
    ];

    fn never(_len: usize) -> bool {
        false
    }

    /// [`SAMPLE_GOLDEN`] with all six reserved bits set; a spec form.
    const RESERVED_BITS_FIXTURE: [u8; 10] = [
        0xFF, 0x01, 0x02, 0x02, // flags with reserved bits, id, count
        0x24, 0x0A, 0x0B, // entry 1
        0x06, 0x0C, 0x0D, // entry 2
    ];

    /// A value with no entries; a spec form that is not the golden.
    const ZERO_ENTRIES: [u8; 4] = [0xC0, 0x01, 0x02, 0x00];

    fn zero_entries() -> Sample {
        Sample {
            entries: Vec::new(),
            ..sample_golden()
        }
    }

    // The per-body macro on the three sample bodies.

    body_checks! {
        mod sample_body;
        body: Sample;
        golden: &SAMPLE_GOLDEN, sample_golden();
        prefixes: only_empty, truncated_in(&SAMPLE_FIELDS);
        trailing_byte: &SAMPLE_GOLDEN;
        count_too_large: too_many_entries(256), EncodeError::CountTooLarge { len: 256, max: 255 };
        reserved_bits: [
            (&[0xFF, 0x01, 0x02, 0x02, 0x24, 0x0A, 0x0B, 0x06, 0x0C, 0x0D], sample_golden(), &SAMPLE_GOLDEN),
        ];
        alternates: [(&[], Sample::default(), &[0, 0, 0, 0])];
    }

    body_checks! {
        mod tail_sample_body;
        body: TailSample<'a>;
        golden: &TAIL_GOLDEN, tail_golden();
        prefixes: tail_accepted, truncated_in(&TAIL_FIELDS);
        count_too_large: TailSample { sample: too_many_entries(256), tail: &[] },
            EncodeError::CountTooLarge { len: 256, max: 255 };
        alternates: [
            (&TAIL_GOLDEN[..7], TailSample { tail: &[], ..tail_golden() }, &TAIL_GOLDEN[..7]),
        ];
    }

    body_checks! {
        mod lenient_sample_body;
        body: LenientSample;
        golden: &SAMPLE_GOLDEN, LenientSample(sample_golden());
        lenient;
        prefixes: never, truncated_in(&SAMPLE_FIELDS);
        trailing_byte: &SAMPLE_GOLDEN;
        count_too_large: LenientSample(too_many_entries(256)),
            EncodeError::CountTooLarge { len: 256, max: 255 };
        reserved_bits: [
            (&RESERVED_BITS_FIXTURE, LenientSample(sample_golden()), &SAMPLE_GOLDEN),
        ];
        alternates: [(&ZERO_ENTRIES, LenientSample(zero_entries()), &ZERO_ENTRIES)];
        lenient_absent: [&[]];
        lenient: [(&LENIENT_TRAILING, LenientSample(sample_golden()), &SAMPLE_GOLDEN)];
        lenient_rejects: [(
            &SAMPLE_GOLDEN[..1],
            DecodeError::Truncated { needed: 2, remaining: 0 }
        )];
    }

    #[test]
    fn sample_tlv_types_are_unnamed_and_distinct() {
        let types = [
            SAMPLE_TLV_TYPE,
            TAIL_SAMPLE_TLV_TYPE,
            LENIENT_SAMPLE_TLV_TYPE,
        ];
        for (index, tlv_type) in types.iter().enumerate() {
            assert_eq!(tlv_type.name(), None);
            assert!(!types[..index].contains(tlv_type));
        }
    }

    // The lenient and Tlv helpers must fail on a wrong body or fixture.

    #[test]
    fn lenient_absent_helper_rejects_a_value_that_is_not_empty() {
        assert!(check_lenient_absent(&LenientSample(sample_golden()), &SAMPLE_GOLDEN).is_err());
    }

    #[test]
    fn lenient_absent_helper_rejects_a_body_that_decodes_the_empty_value() {
        // `Sample` has no lenient form, so use a lenient body whose strict form accepts the empty value.
        struct AcceptsEmpty;
        impl TlvBody<'_> for AcceptsEmpty {
            const TLV_TYPE: TlvType = TlvType(0xFB);
            fn decode_value(_value: &[u8]) -> Result<Self, DecodeError> {
                Ok(Self)
            }
            fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
                Ok(Vec::new())
            }
        }
        impl TlvBodyLenient<'_> for AcceptsEmpty {
            fn decode_value_lenient(_value: &[u8]) -> Result<Option<Self>, DecodeError> {
                Ok(None)
            }
        }
        impl Debug for AcceptsEmpty {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, "AcceptsEmpty")
            }
        }
        impl PartialEq for AcceptsEmpty {
            fn eq(&self, _other: &Self) -> bool {
                true
            }
        }
        assert!(check_lenient_absent(&AcceptsEmpty, &[]).is_err());
    }

    #[test]
    fn lenient_value_helper_rejects_a_wrong_body_and_a_wrong_reencoding() {
        let good = LenientSample(sample_golden());
        let wrong_body = LenientSample(Sample::default());
        assert!(check_lenient_value(&SAMPLE_GOLDEN, &wrong_body, &SAMPLE_GOLDEN).is_err());
        // LENIENT_TRAILING passes every earlier check, so only the re-encoding can fail.
        let error = check_lenient_value(&LENIENT_TRAILING, &good, &[0]).unwrap_err();
        assert!(error.contains("re-encode gave"), "{error}");
        assert_passes(check_lenient_value(
            &LENIENT_TRAILING,
            &good,
            &SAMPLE_GOLDEN,
        ));
        assert!(check_lenient_value(&[], &good, &SAMPLE_GOLDEN).is_err());
    }

    /// A body whose strict decoder is too loose: it ignores trailing bytes, like the lenient one.
    #[derive(Debug, PartialEq)]
    struct LooseStrict(Sample);

    impl TlvBody<'_> for LooseStrict {
        const TLV_TYPE: TlvType = TlvType(0xFB);
        fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
            Ok(Self(Sample::decode(&mut Reader::new(value))?))
        }
        fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
            self.0.encode_value()
        }
    }

    impl TlvBodyLenient<'_> for LooseStrict {
        fn decode_value_lenient(value: &[u8]) -> Result<Option<Self>, DecodeError> {
            if value.is_empty() {
                return Ok(None);
            }
            Self::decode_value(value).map(Some)
        }
    }

    /// A lenient body that turns every decode error into `Ok(None)`.
    #[derive(Debug, PartialEq)]
    struct Swallowing(Sample);

    impl TlvBody<'_> for Swallowing {
        const TLV_TYPE: TlvType = TlvType(0xFC);
        fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
            let mut reader = Reader::new(value);
            let body = Sample::decode(&mut reader)?;
            reader.finish()?;
            Ok(Self(body))
        }
        fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
            self.0.encode_value()
        }
    }

    impl TlvBodyLenient<'_> for Swallowing {
        fn decode_value_lenient(value: &[u8]) -> Result<Option<Self>, DecodeError> {
            if value.is_empty() {
                return Ok(None);
            }
            Ok(Self::decode_value(value).ok())
        }
    }

    #[test]
    fn lenient_value_helper_rejects_a_strict_decoder_that_accepts_the_fixture() {
        let expected = LooseStrict(sample_golden());
        let result = check_lenient_value(&LENIENT_TRAILING, &expected, &SAMPLE_GOLDEN);
        assert!(result.unwrap_err().contains("strict decode accepted"));
        // The same fixture passes for the body with a correct strict decoder.
        let good = LenientSample(sample_golden());
        assert_passes(check_lenient_value(
            &LENIENT_TRAILING,
            &good,
            &SAMPLE_GOLDEN,
        ));
    }

    #[test]
    fn lenient_prefix_helper_rejects_a_lenient_decoder_that_swallows_errors() {
        let witness = Swallowing(sample_golden());
        let result = check_lenient_prefixes(&witness, &SAMPLE_GOLDEN);
        assert!(result.unwrap_err().contains("gave Ok(None)"));
        let good = LenientSample(sample_golden());
        assert_passes(check_lenient_prefixes(&good, &SAMPLE_GOLDEN));
    }

    /// A lenient body that gives `Ok(None)` for the exact, valid golden.
    #[derive(Debug, PartialEq)]
    struct NoneOnExact(Sample);

    impl TlvBody<'_> for NoneOnExact {
        const TLV_TYPE: TlvType = TlvType(0xFD);
        fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
            let mut reader = Reader::new(value);
            let body = Sample::decode(&mut reader)?;
            reader.finish()?;
            Ok(Self(body))
        }
        fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
            self.0.encode_value()
        }
    }

    impl TlvBodyLenient<'_> for NoneOnExact {
        fn decode_value_lenient(value: &[u8]) -> Result<Option<Self>, DecodeError> {
            if value.is_empty() {
                return Ok(None);
            }
            let mut reader = Reader::new(value);
            let body = Self(Sample::decode(&mut reader)?);
            if reader.finish().is_ok() {
                return Ok(None);
            }
            Ok(Some(body))
        }
    }

    #[test]
    fn lenient_golden_helper_rejects_a_body_that_gives_none_on_the_exact_golden() {
        let witness = NoneOnExact(sample_golden());
        let error = check_lenient_golden(&SAMPLE_GOLDEN, &witness).unwrap_err();
        assert!(error.contains("of the golden gave Ok(None)"), "{error}");
        // The prefix sweep alone also catches it, because it now covers the whole golden.
        let error = check_lenient_prefixes(&witness, &SAMPLE_GOLDEN).unwrap_err();
        assert!(error.contains("10 bytes gave Ok(None)"), "{error}");
        // A wrong body is refused, a correct one passes.
        let good = LenientSample(sample_golden());
        let wrong = LenientSample(Sample::default());
        assert!(check_lenient_golden(&SAMPLE_GOLDEN, &wrong).is_err());
        assert_passes(check_lenient_golden(&SAMPLE_GOLDEN, &good));
    }

    /// Which spec form a [`Quirky`] body mishandles in its lenient decoder.
    const QUIRK_NONE: u8 = 0;
    const QUIRK_NONE_ON_ALTERNATE: u8 = 1;
    const QUIRK_ERR_ON_ALTERNATE: u8 = 2;
    const QUIRK_OTHER_BODY_ON_ALTERNATE: u8 = 3;
    const QUIRK_ERR_ON_RESERVED_BITS: u8 = 4;

    /// A lenient body that is right on the golden and its prefixes and wrong on
    /// one spec fixture, chosen by `QUIRK`: [`ZERO_ENTRIES`] (an alternate) or
    /// [`RESERVED_BITS_FIXTURE`] (a reserved-bit form).
    #[derive(Debug, PartialEq)]
    struct Quirky<const QUIRK: u8>(Sample);

    impl<const QUIRK: u8> TlvBody<'_> for Quirky<QUIRK> {
        const TLV_TYPE: TlvType = TlvType(0xFE);
        fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
            let mut reader = Reader::new(value);
            let body = Sample::decode(&mut reader)?;
            reader.finish()?;
            Ok(Self(body))
        }
        fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
            self.0.encode_value()
        }
    }

    impl<const QUIRK: u8> TlvBodyLenient<'_> for Quirky<QUIRK> {
        fn decode_value_lenient(value: &[u8]) -> Result<Option<Self>, DecodeError> {
            if value.is_empty() {
                return Ok(None);
            }
            let on_alternate = value == ZERO_ENTRIES;
            let has_reserved_bits = value[0] & 0x3F != 0;
            match QUIRK {
                QUIRK_NONE_ON_ALTERNATE if on_alternate => Ok(None),
                QUIRK_ERR_ON_ALTERNATE if on_alternate => {
                    Err(DecodeError::TrailingBytes { remaining: 1 })
                }
                QUIRK_OTHER_BODY_ON_ALTERNATE if on_alternate => Ok(Some(Self(Sample::default()))),
                QUIRK_ERR_ON_RESERVED_BITS if has_reserved_bits => {
                    Err(DecodeError::TrailingBytes { remaining: 1 })
                }
                _ => Self::decode_value(value).map(Some),
            }
        }
    }

    #[test]
    fn lenient_matches_strict_helper_rejects_a_wrong_lenient_decode_of_a_spec_form() {
        let golden = Quirky::<QUIRK_NONE>(sample_golden());
        assert_passes(check_lenient_matches_strict(&golden, &ZERO_ENTRIES));
        assert_passes(check_lenient_matches_strict(
            &golden,
            &RESERVED_BITS_FIXTURE,
        ));

        let none = Quirky::<QUIRK_NONE_ON_ALTERNATE>(sample_golden());
        let error = check_lenient_matches_strict(&none, &ZERO_ENTRIES).unwrap_err();
        assert!(error.contains("gave Ok(None)"), "{error}");
        assert_passes(check_lenient_matches_strict(&none, &RESERVED_BITS_FIXTURE));

        let err = Quirky::<QUIRK_ERR_ON_ALTERNATE>(sample_golden());
        let error = check_lenient_matches_strict(&err, &ZERO_ENTRIES).unwrap_err();
        assert!(error.contains("gave Err("), "{error}");

        let other = Quirky::<QUIRK_OTHER_BODY_ON_ALTERNATE>(sample_golden());
        let error = check_lenient_matches_strict(&other, &ZERO_ENTRIES).unwrap_err();
        assert!(error.contains("gave Ok(Some("), "{error}");

        let reserved = Quirky::<QUIRK_ERR_ON_RESERVED_BITS>(sample_golden());
        assert!(check_lenient_matches_strict(&reserved, &RESERVED_BITS_FIXTURE).is_err());
        assert_passes(check_lenient_matches_strict(&reserved, &ZERO_ENTRIES));
    }

    #[test]
    fn lenient_matches_strict_helper_rejects_a_fixture_the_strict_decoder_rejects() {
        let good = LenientSample(sample_golden());
        let error = check_lenient_matches_strict(&good, &LENIENT_TRAILING).unwrap_err();
        assert!(error.contains("not a spec form"), "{error}");
        assert_passes(check_lenient_matches_strict(&good, &ZERO_ENTRIES));
    }

    // The macro runs that check on every `alternates:` and `reserved_bits:` fixture.
    // Each body below is wrong on one fixture, so its `lenient_forms` test must panic.

    body_checks! {
        mod quirky_none_on_alternate;
        body: Quirky<QUIRK_NONE_ON_ALTERNATE>;
        golden: &SAMPLE_GOLDEN, Quirky::<QUIRK_NONE_ON_ALTERNATE>(sample_golden());
        lenient_should_fail;
        alternates: [(&ZERO_ENTRIES, Quirky::<QUIRK_NONE_ON_ALTERNATE>(zero_entries()), &ZERO_ENTRIES)];
    }

    body_checks! {
        mod quirky_err_on_alternate;
        body: Quirky<QUIRK_ERR_ON_ALTERNATE>;
        golden: &SAMPLE_GOLDEN, Quirky::<QUIRK_ERR_ON_ALTERNATE>(sample_golden());
        lenient_should_fail;
        alternates: [(&ZERO_ENTRIES, Quirky::<QUIRK_ERR_ON_ALTERNATE>(zero_entries()), &ZERO_ENTRIES)];
    }

    body_checks! {
        mod quirky_other_body_on_alternate;
        body: Quirky<QUIRK_OTHER_BODY_ON_ALTERNATE>;
        golden: &SAMPLE_GOLDEN, Quirky::<QUIRK_OTHER_BODY_ON_ALTERNATE>(sample_golden());
        lenient_should_fail;
        alternates: [(&ZERO_ENTRIES, Quirky::<QUIRK_OTHER_BODY_ON_ALTERNATE>(zero_entries()), &ZERO_ENTRIES)];
    }

    body_checks! {
        mod quirky_err_on_reserved_bits;
        body: Quirky<QUIRK_ERR_ON_RESERVED_BITS>;
        golden: &SAMPLE_GOLDEN, Quirky::<QUIRK_ERR_ON_RESERVED_BITS>(sample_golden());
        lenient_should_fail;
        reserved_bits: [
            (&RESERVED_BITS_FIXTURE, Quirky::<QUIRK_ERR_ON_RESERVED_BITS>(sample_golden()), &SAMPLE_GOLDEN),
        ];
    }

    // The same body without the quirk passes, so the failures above come from the quirk.
    body_checks! {
        mod quirky_without_a_quirk;
        body: Quirky<QUIRK_NONE>;
        golden: &SAMPLE_GOLDEN, Quirky::<QUIRK_NONE>(sample_golden());
        lenient;
        reserved_bits: [
            (&RESERVED_BITS_FIXTURE, Quirky::<QUIRK_NONE>(sample_golden()), &SAMPLE_GOLDEN),
        ];
        alternates: [(&ZERO_ENTRIES, Quirky::<QUIRK_NONE>(zero_entries()), &ZERO_ENTRIES)];
    }

    // The lenient probe that the strict arm of `body_checks!` runs.

    #[test]
    fn probe_accepts_a_strict_body() {
        assert_not_lenient!(Sample::default());
    }

    #[test]
    #[should_panic(expected = "needs the `lenient;` marker")]
    fn probe_refuses_a_lenient_body_without_the_marker() {
        assert_not_lenient!(LenientSample(sample_golden()));
    }

    // The prefix sweep must also compare the lenient and strict results.

    const SHORT_GOLDEN: [u8; 2] = [7, 8];

    /// Strict form: one or two octets. The lenient form is right in mode `OK`, an error for
    /// one octet in `ERR_ON_ONE`, and another body for one octet in `OTHER_ON_ONE`.
    #[derive(Debug, PartialEq)]
    struct Short<const MODE: u8>(usize);

    const SHORT_OK: u8 = 0;
    const SHORT_ERR_ON_ONE: u8 = 1;
    const SHORT_OTHER_ON_ONE: u8 = 2;

    impl<const MODE: u8> TlvBody<'_> for Short<MODE> {
        const TLV_TYPE: TlvType = TlvType(0xFD);
        fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
            match value.len() {
                1 | 2 => Ok(Self(value.len())),
                0 => Err(DecodeError::Truncated {
                    needed: 1,
                    remaining: 0,
                }),
                _ => Err(DecodeError::TrailingBytes {
                    remaining: value.len() - 2,
                }),
            }
        }
        fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
            Ok(vec![0; self.0])
        }
    }

    impl<const MODE: u8> TlvBodyLenient<'_> for Short<MODE> {
        fn decode_value_lenient(value: &[u8]) -> Result<Option<Self>, DecodeError> {
            match (MODE, value.len()) {
                (_, 0) => Ok(None),
                (SHORT_ERR_ON_ONE, 1) => Err(DecodeError::Truncated {
                    needed: 2,
                    remaining: 1,
                }),
                (SHORT_OTHER_ON_ONE, 1) => Ok(Some(Self(99))),
                _ => Ok(Some(Self(value.len().min(2)))),
            }
        }
    }

    #[test]
    fn lenient_prefix_helper_requires_the_strict_body_where_strict_accepts_the_prefix() {
        assert_passes(check_lenient_prefixes(&Short::<SHORT_OK>(2), &SHORT_GOLDEN));
        let error =
            check_lenient_prefixes(&Short::<SHORT_ERR_ON_ONE>(2), &SHORT_GOLDEN).unwrap_err();
        assert!(
            error.contains("prefix of 1 bytes, which the strict decoder accepts"),
            "{error}"
        );
        let error =
            check_lenient_prefixes(&Short::<SHORT_OTHER_ON_ONE>(2), &SHORT_GOLDEN).unwrap_err();
        assert!(error.contains("expected Ok(Some(Short(1)))"), "{error}");
    }

    // The fixture sweep checks each row by its kind.

    /// A row whose `tlv` column is the TLV type of body `B`.
    fn row<B: TlvBody<'static>>(fields: &'static str, hex: &'static str) -> Row<'static> {
        // Leaked like the re-encoded bytes in the sweep: a self-test row is a few bytes.
        let tlv = Box::leak(format!("0x{:02x}", B::TLV_TYPE.0).into_boxed_str());
        Row {
            tlv,
            case: "self-test",
            fields,
            hex,
        }
    }

    const GOLDEN_HEX: &str = "c0010202240a0b060c0d";
    const RESERVED_HEX: &str = "ff010202240a0b060c0d";
    const TRAILING_HEX: &str = "c0010202240a0b060c0da5";

    /// The key of a row made by [`row`] for [`LenientSample`].
    const LENIENT_SAMPLE_ROW: Key = ("0xfa", "self-test");

    #[test]
    fn sweep_accepts_each_kind_of_row() {
        let golden = row::<Sample>("kind=spec", GOLDEN_HEX);
        assert_passes(sweep_row::<Sample>(&golden, &golden.bytes(), None));
        let default = row::<Sample>("", "-");
        assert_passes(sweep_row::<Sample>(
            &default,
            &default.bytes(),
            Some(&[0, 0, 0, 0]),
        ));
        let reserved = row::<Sample>("a_reserved=0x3f", RESERVED_HEX);
        assert_passes(sweep_row::<Sample>(&reserved, &reserved.bytes(), None));
        let invalid = row::<LenientSample>("kind=invalid;error=TrailingBytes", TRAILING_HEX);
        assert_passes(sweep_row::<LenientSample>(&invalid, &invalid.bytes(), None));
        // The lenient form of LenientSample ignores the trailing byte, so the row is a lenient input.
        assert_passes(sweep_row_lenient::<LenientSample>(
            &invalid,
            &invalid.bytes(),
            None,
            &[LENIENT_SAMPLE_ROW],
        ));
        let lenient = row::<LenientSample>("kind=lenient", TRAILING_HEX);
        assert_passes(sweep_row_lenient::<LenientSample>(
            &lenient,
            &lenient.bytes(),
            None,
            &[],
        ));
    }

    #[test]
    fn sweep_refuses_a_row_that_does_not_match_its_kind() {
        // A spec row that the strict decoder rejects.
        let trailing_as_spec = row::<LenientSample>("kind=spec", TRAILING_HEX);
        assert!(
            sweep_row::<LenientSample>(&trailing_as_spec, &trailing_as_spec.bytes(), None).is_err()
        );
        // An invalid row with the wrong error, with no error, and that decodes.
        let wrong_error = row::<LenientSample>("kind=invalid;error=Truncated", TRAILING_HEX);
        assert!(sweep_row::<LenientSample>(&wrong_error, &wrong_error.bytes(), None).is_err());
        let no_error = row::<LenientSample>("kind=invalid", TRAILING_HEX);
        assert!(sweep_row::<LenientSample>(&no_error, &no_error.bytes(), None).is_err());
        let decodes = row::<LenientSample>("kind=invalid;error=Truncated", GOLDEN_HEX);
        assert!(sweep_row::<LenientSample>(&decodes, &decodes.bytes(), None).is_err());
        // A lenient row for a body with no lenient form, and one that the strict decoder accepts.
        let lenient = row::<LenientSample>("kind=lenient", TRAILING_HEX);
        assert!(sweep_row::<LenientSample>(&lenient, &lenient.bytes(), None).is_err());
        let strict_as_lenient = row::<LenientSample>("kind=lenient", GOLDEN_HEX);
        assert!(
            sweep_row_lenient::<LenientSample>(
                &strict_as_lenient,
                &strict_as_lenient.bytes(),
                None,
                &[]
            )
            .is_err()
        );
    }

    #[test]
    fn sweep_refuses_a_re_encoding_that_is_not_the_row_or_the_canonical_bytes() {
        // Reserved bits change the bytes only if the row says it has some.
        let unmarked = row::<Sample>("kind=spec", RESERVED_HEX);
        assert!(sweep_row::<Sample>(&unmarked, &unmarked.bytes(), None).is_err());
        let zero_reserved = row::<Sample>("a_reserved=0", RESERVED_HEX);
        assert!(sweep_row::<Sample>(&zero_reserved, &zero_reserved.bytes(), None).is_err());
        // A canonical form must be what the body encodes to, and differ from the row.
        let reserved = row::<Sample>("kind=spec", RESERVED_HEX);
        assert_passes(sweep_row::<Sample>(
            &reserved,
            &reserved.bytes(),
            Some(&SAMPLE_GOLDEN),
        ));
        assert!(sweep_row::<Sample>(&reserved, &reserved.bytes(), Some(&ZERO_ENTRIES)).is_err());
        let golden = row::<Sample>("kind=spec", GOLDEN_HEX);
        assert!(sweep_row::<Sample>(&golden, &golden.bytes(), Some(&SAMPLE_GOLDEN)).is_err());
    }

    #[test]
    fn sweep_refuses_a_lenient_decoder_that_disagrees_with_the_strict_one() {
        let golden = row::<LenientSample>("kind=spec", GOLDEN_HEX);
        assert_passes(sweep_row_lenient::<LenientSample>(
            &golden,
            &golden.bytes(),
            None,
            &[],
        ));
        let golden = row::<NoneOnExact>("kind=spec", GOLDEN_HEX);
        assert!(sweep_row_lenient::<NoneOnExact>(&golden, &golden.bytes(), None, &[]).is_err());
        // A lenient decoder that gives Ok(None) for a value that is not empty.
        let invalid = row::<LenientSample>("kind=invalid;error=Truncated", "c0");
        assert_passes(sweep_row_lenient::<LenientSample>(
            &invalid,
            &invalid.bytes(),
            None,
            &[],
        ));
        let invalid = row::<Swallowing>("kind=invalid;error=Truncated", "c0");
        assert!(sweep_row_lenient::<Swallowing>(&invalid, &invalid.bytes(), None, &[]).is_err());
    }

    #[test]
    fn sweep_refuses_a_lenient_body_that_accepts_an_invalid_row_unless_it_is_listed() {
        // The lenient form of LenientSample ignores a trailing byte, so it gives Ok(Some(..)).
        let invalid = row::<LenientSample>("kind=invalid;error=TrailingBytes", TRAILING_HEX);
        let error =
            sweep_row_lenient::<LenientSample>(&invalid, &invalid.bytes(), None, &[]).unwrap_err();
        assert!(error.contains("of an invalid row gave Ok(Some("), "{error}");
        // Listing another row does not excuse it.
        let other = [("0xfa", "another-row")];
        assert!(
            sweep_row_lenient::<LenientSample>(&invalid, &invalid.bytes(), None, &other).is_err()
        );
        assert_passes(sweep_row_lenient::<LenientSample>(
            &invalid,
            &invalid.bytes(),
            None,
            &[LENIENT_SAMPLE_ROW],
        ));
        // An empty invalid row is Ok(None), which is also not an error.
        let empty = row::<LenientSample>("kind=invalid;error=Truncated", "-");
        let error =
            sweep_row_lenient::<LenientSample>(&empty, &empty.bytes(), None, &[]).unwrap_err();
        assert!(error.contains("gave Ok(None)"), "{error}");
    }

    /// A lenient body that gives `Ok(Some)` for the empty value, which the strict decoder rejects.
    #[derive(Debug, PartialEq)]
    struct SomeOnEmpty(Sample);

    impl TlvBody<'_> for SomeOnEmpty {
        const TLV_TYPE: TlvType = TlvType(0xF7);
        fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
            let mut reader = Reader::new(value);
            let body = Sample::decode(&mut reader)?;
            reader.finish()?;
            Ok(Self(body))
        }
        fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
            self.0.encode_value()
        }
    }

    impl TlvBodyLenient<'_> for SomeOnEmpty {
        fn decode_value_lenient(value: &[u8]) -> Result<Option<Self>, DecodeError> {
            if value.is_empty() {
                return Ok(Some(Self(Sample::decode(&mut Reader::new(
                    &SAMPLE_GOLDEN,
                ))?)));
            }
            Self::decode_value(value).map(Some)
        }
    }

    /// A lenient body that gives `Ok(None)` for every value, which the strict decoder rejects.
    #[derive(Debug, PartialEq)]
    struct SwallowsEverything;

    impl TlvBody<'_> for SwallowsEverything {
        const TLV_TYPE: TlvType = TlvType(0xF6);
        fn decode_value(_value: &[u8]) -> Result<Self, DecodeError> {
            Err(DecodeError::Truncated {
                needed: 1,
                remaining: 0,
            })
        }
        fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
            Ok(vec![])
        }
    }

    impl TlvBodyLenient<'_> for SwallowsEverything {
        fn decode_value_lenient(_value: &[u8]) -> Result<Option<Self>, DecodeError> {
            Ok(None)
        }
    }

    #[test]
    fn sweep_refuses_a_decoded_absent_lenient_row_with_a_value() {
        let empty = row::<SwallowsEverything>("kind=lenient;decoded=absent", "-");
        assert_passes(sweep_row_lenient::<SwallowsEverything>(
            &empty,
            &empty.bytes(),
            None,
            &[],
        ));
        let non_empty = row::<SwallowsEverything>("kind=lenient;decoded=absent", "a5");
        let error =
            sweep_row_lenient::<SwallowsEverything>(&non_empty, &non_empty.bytes(), None, &[])
                .unwrap_err();
        assert!(error.contains("must have an empty value"), "{error}");
    }

    #[test]
    fn sweep_checks_a_decoded_absent_lenient_row_gives_ok_none() {
        let absent = row::<LenientSample>("kind=lenient;decoded=absent", "-");
        assert_passes(sweep_row_lenient::<LenientSample>(
            &absent,
            &absent.bytes(),
            None,
            &[],
        ));
        let absent = row::<SomeOnEmpty>("kind=lenient;decoded=absent", "-");
        let error =
            sweep_row_lenient::<SomeOnEmpty>(&absent, &absent.bytes(), None, &[]).unwrap_err();
        assert!(
            error.contains("decoded=absent row gave Ok(Some("),
            "{error}"
        );
        // Without decoded=absent the same body is a valid lenient row.
        let plain = row::<SomeOnEmpty>("kind=lenient", "-");
        assert_passes(sweep_row_lenient::<SomeOnEmpty>(
            &plain,
            &plain.bytes(),
            None,
            &[],
        ));
        // A lenient row that decodes to Ok(None) without decoded=absent is still refused.
        let unmarked = row::<LenientSample>("kind=lenient", "-");
        assert!(
            sweep_row_lenient::<LenientSample>(&unmarked, &unmarked.bytes(), None, &[]).is_err()
        );
    }

    #[test]
    fn sweep_refuses_a_row_dispatched_to_a_body_of_another_tlv_type() {
        // LenientSample has the layout of Sample, so only the TLV type tells them apart.
        let golden = row::<Sample>("kind=spec", GOLDEN_HEX);
        let error = sweep_row::<LenientSample>(&golden, &golden.bytes(), None).unwrap_err();
        assert!(
            error.contains("row TLV 0xf8 dispatched to a body of TLV"),
            "{error}"
        );
        assert_passes(sweep_row::<Sample>(&golden, &golden.bytes(), None));
        let invalid = row::<Sample>("kind=invalid;error=Truncated", "c0");
        assert!(sweep_row::<LenientSample>(&invalid, &invalid.bytes(), None).is_err());
        // The lenient entry point checks the column for each kind of row.
        for fields in ["kind=spec", "kind=lenient", "kind=invalid;error=Truncated"] {
            let wrong = row::<Sample>(fields, GOLDEN_HEX);
            let error =
                sweep_row_lenient::<LenientSample>(&wrong, &wrong.bytes(), None, &[]).unwrap_err();
            assert!(
                error.contains("dispatched to a body of TLV"),
                "{fields}: {error}"
            );
        }
        // A column that is not a hex byte is refused.
        let malformed = Row {
            tlv: "f8",
            case: "self-test",
            fields: "kind=spec",
            hex: GOLDEN_HEX,
        };
        assert!(check_tlv_column::<Sample>(malformed.tlv).is_err());
    }

    #[test]
    #[should_panic(expected = "no body for TLV")]
    fn sweep_group_fails_on_a_tlv_code_with_no_body() {
        sweep_group("linkmetric", &[], |row, _, _| {
            Err(format!("no body for TLV {}", row.tlv))
        });
    }

    #[test]
    #[should_panic(expected = "a canonical row names no linkmetric fixture")]
    fn sweep_group_fails_on_a_canonical_row_that_matches_nothing() {
        sweep_group(
            "linkmetric",
            &[(("0x08", "not-a-case"), ("0x08", "golden"))],
            |_, _, _| Ok(()),
        );
    }

    #[test]
    fn lenient_rejects_helper_rejects_a_swallowing_body_and_a_wrong_error() {
        let truncated = DecodeError::Truncated {
            needed: 2,
            remaining: 0,
        };
        let swallowing = Swallowing(sample_golden());
        assert!(check_lenient_rejects(&swallowing, &SAMPLE_GOLDEN[..1], truncated).is_err());
        let good = LenientSample(sample_golden());
        assert_passes(check_lenient_rejects(&good, &SAMPLE_GOLDEN[..1], truncated));
        let wrong = DecodeError::TrailingBytes { remaining: 1 };
        assert!(check_lenient_rejects(&good, &SAMPLE_GOLDEN[..1], wrong).is_err());
        // An empty value is not a malformed fixture.
        assert!(check_lenient_rejects(&good, &[], truncated).is_err());
    }

    #[test]
    fn tlv_decode_helper_rejects_a_wrong_type_and_a_wrong_body() {
        let tlv = Tlv {
            tlv_type: TAIL_SAMPLE_TLV_TYPE,
            value: &SAMPLE_GOLDEN,
        };
        assert!(check_tlv_decodes(tlv, &sample_golden()).is_err());
        let tlv = Tlv {
            tlv_type: SAMPLE_TLV_TYPE,
            value: &SAMPLE_GOLDEN,
        };
        assert!(check_tlv_decodes(tlv, &Sample::default()).is_err());
        assert!(check_tlv_decodes(tlv, &sample_golden()).is_ok());
    }

    #[test]
    fn message_round_trip_carries_a_value_of_the_largest_tlv_size() {
        let body = TailSample {
            sample: Sample::default(),
            tail: &vec![0x5A; MAX_TLV_VALUE - 4],
        };
        assert_eq!(body.encode_value().unwrap().len(), MAX_TLV_VALUE);
        let message = build_message(&body).unwrap();
        assert_eq!(message.len(), MAX_TLV_MESSAGE_BUDGET);
        assert_passes(check_tlv_decodes(single_tlv(&message).unwrap(), &body));
        // A message with the 1500-byte RAW_BUDGET cannot hold it.
        assert!(build_message_with_budget(&body, RAW_BUDGET).is_err());
    }

    #[test]
    fn single_tlv_rejects_two_tlvs_and_none() {
        let header = CmduHeader {
            msg_type: MsgType::TOPOLOGY_QUERY,
            msg_id: 1,
            frag_id: 0,
            last_fragment: true,
            relay: false,
        };
        let mut two = MsgBuilder::new(&header, RAW_BUDGET);
        two.body(&sample_golden()).body(&sample_golden());
        assert!(single_tlv(&two.finish().unwrap()).is_err());
        let none = MsgBuilder::new(&header, RAW_BUDGET).finish().unwrap();
        assert!(single_tlv(&none).is_err());
    }

    #[test]
    fn single_tlv_requires_exactly_the_end_of_message_after_the_tlv() {
        let message = build_message(&sample_golden()).unwrap();
        assert_passes(check_tlv_decodes(
            single_tlv(&message).unwrap(),
            &sample_golden(),
        ));
        let mut wrong_length = build_message(&sample_golden()).unwrap();
        let last = wrong_length.len() - 1;
        wrong_length[last] = 0x01;
        let error = single_tlv(&wrong_length).unwrap_err();
        assert!(error.contains("End of Message"), "{error}");
        let mut extra = message;
        extra.push(0x00);
        let error = single_tlv(&extra).unwrap_err();
        assert!(error.contains("End of Message"), "{error}");
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
            other => other,
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
