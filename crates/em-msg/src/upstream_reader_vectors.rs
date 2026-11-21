// SPDX-License-Identifier: Apache-2.0

//! Test-only check of the field readers against the upstream reader vectors.
//!
//! `tests/data/upstream_reader_vectors.tsv` is written by `tools/upstream-tlv-vectors/regen`,
//! which runs the pinned `em_msg_t` readers on a corpus of TLV areas. This module runs the ports
//! on the same areas (note 0009 A10, vector mapping):
//!
//! - An upstream `true` must be `Ok(Some)`, and its output is compared.
//! - An upstream `false` must be `Ok(None)`, except for the pinned cases in
//!   [`EXPECTED_ERRORS`], which must be exactly that `Err`. Outputs are not compared on `false`.
//! - `find_tlv` is compared by the position of its value slice in the area.
//! - `parse_profile_tlv` is run on the value cut to its `value_len`.
//! - `get_first_tlv` and `get_next_tlv` are compared by offset: the TLV at the returned offset must
//!   have the type and value that upstream's pointer addresses in the area.

use em_types::{MacAddr, TlvType};

use crate::error::ReaderError;
use crate::readers::{
    FirstTlvError, WalkedTlv, al_mac_address, bss_id, find_tlv, first_tlv, freq_band, next_tlv,
    parse_profile_value, profile, radio_id, sta_mac, supported_services,
};
use crate::upstream_tlv_vectors::decode_hex;

const VECTORS: &str = include_str!("../tests/data/upstream_reader_vectors.tsv");

const COLUMN_LINE: &str = "case\tfunction\tinputs\tarea\treturn\toutput";

/// Number of data rows the fixture must hold.
const EXPECTED_ROWS: usize = 1421;

const GET_AL_MAC_ADDRESS: &str = "em_msg_t::get_al_mac_address";
const GET_SUPPORTED_SERVICE: &str = "em_msg_t::get_supported_service";
const GET_PROFILE: &str = "em_msg_t::get_profile";
const GET_PROFILE_TYPE: &str = "em_msg_t::get_profile_type";
const GET_STA_MAC: &str = "em_msg_t::get_sta_mac";
const GET_BSS_ID: &str = "em_msg_t::get_bss_id";
const GET_RADIO_ID: &str = "em_msg_t::get_radio_id";
const GET_FREQ_BAND: &str = "em_msg_t::get_freq_band";
const GET_FIRST_TLV: &str = "em_msg_t::get_first_tlv";
const GET_NEXT_TLV: &str = "em_msg_t::get_next_tlv";
const GET_TLV_BY_TYPE: &str = "em_msg_t::get_tlv(type)";
const GET_TLV_IN_BUFFER: &str = "em_msg_t::get_tlv(buff,len,type)";
const PARSE_PROFILE_TLV: &str = "em_msg_t::parse_profile_tlv";

/// The rows where upstream returns `false` and the port returns an error (A10): the malformed
/// Profile values and lengths of `get_profile` and of `get_profile_type` (four rows each), and a
/// Supported Service count of 9.
const EXPECTED_ERRORS: [(&str, &str, ReaderError); 9] = [
    (
        GET_PROFILE,
        "profile-value-out-of-range",
        ReaderError::ValueOutOfRange {
            tlv_type: TlvType::MULTI_AP_PROFILE,
            value: 4,
        },
    ),
    (
        GET_PROFILE,
        "profile-value-ff",
        ReaderError::ValueOutOfRange {
            tlv_type: TlvType::MULTI_AP_PROFILE,
            value: 0xFF,
        },
    ),
    (
        GET_PROFILE,
        "profile-length-2",
        ReaderError::InvalidLength {
            tlv_type: TlvType::MULTI_AP_PROFILE,
            len: 2,
        },
    ),
    (
        GET_PROFILE,
        "profile-length-0",
        ReaderError::InvalidLength {
            tlv_type: TlvType::MULTI_AP_PROFILE,
            len: 0,
        },
    ),
    (
        GET_PROFILE_TYPE,
        "profile-value-out-of-range",
        ReaderError::ValueOutOfRange {
            tlv_type: TlvType::MULTI_AP_PROFILE,
            value: 4,
        },
    ),
    (
        GET_PROFILE_TYPE,
        "profile-value-ff",
        ReaderError::ValueOutOfRange {
            tlv_type: TlvType::MULTI_AP_PROFILE,
            value: 0xFF,
        },
    ),
    (
        GET_PROFILE_TYPE,
        "profile-length-2",
        ReaderError::InvalidLength {
            tlv_type: TlvType::MULTI_AP_PROFILE,
            len: 2,
        },
    ),
    (
        GET_PROFILE_TYPE,
        "profile-length-0",
        ReaderError::InvalidLength {
            tlv_type: TlvType::MULTI_AP_PROFILE,
            len: 0,
        },
    ),
    (
        GET_SUPPORTED_SERVICE,
        "supported-service-9",
        ReaderError::ValueOutOfRange {
            tlv_type: TlvType::SUPPORTED_SERVICE,
            value: 9,
        },
    ),
];

/// Upstream's `em_supported_service_t` holds a count and 8 service octets.
const SERVICE_ARRAY_LEN: usize = 8;

struct Row<'a> {
    case: &'a str,
    function: &'a str,
    inputs: &'a str,
    area: Vec<u8>,
    returned: &'a str,
    output: Vec<u8>,
}

fn parse_rows(text: &str) -> Result<Vec<Row<'_>>, String> {
    let mut lines = Vec::new();
    for line in text.lines() {
        if !line.starts_with('#') && !line.is_empty() {
            lines.push(line);
        }
    }
    if lines.first().copied() != Some(COLUMN_LINE) {
        return Err("column line missing or changed".to_string());
    }
    let mut rows = Vec::new();
    for line in &lines[1..] {
        let columns: Vec<&str> = line.split('\t').collect();
        if columns.len() != 6 {
            return Err(format!(
                "expected 6 columns, found {}: {line}",
                columns.len()
            ));
        }
        let area = decode_hex(columns[3]).map_err(|e| format!("{}: area: {e}", columns[0]))?;
        let output = decode_hex(columns[5]).map_err(|e| format!("{}: output: {e}", columns[0]))?;
        rows.push(Row {
            case: columns[0],
            function: columns[1],
            inputs: columns[2],
            area,
            returned: columns[4],
            output,
        });
    }
    Ok(rows)
}

/// The value of `key` in an inputs column such as `len=15;from=3`.
fn input<'a>(row: &Row<'a>, key: &str) -> Result<&'a str, String> {
    for pair in row.inputs.split(';') {
        if let Some((name, value)) = pair.split_once('=') {
            if name == key {
                return Ok(value);
            }
        }
    }
    Err(format!("no input {key} in {:?}", row.inputs))
}

fn input_number(row: &Row<'_>, key: &str) -> Result<usize, String> {
    let text = input(row, key)?;
    text.parse()
        .map_err(|e| format!("bad input {key}={text:?}: {e}"))
}

fn pinned_error(row: &Row<'_>) -> Option<ReaderError> {
    for (function, case, error) in EXPECTED_ERRORS {
        if function == row.function && case == row.case {
            return Some(error);
        }
    }
    None
}

/// Applies the mapping of an upstream result onto a reader result. `Ok(Some)` is a `true` row
/// whose output is to be compared; `Ok(None)` means there is nothing left to compare.
fn map_outcome<T>(
    row: &Row<'_>,
    result: Result<Option<T>, ReaderError>,
) -> Result<Option<T>, String> {
    if let Some(expected) = pinned_error(row) {
        if row.returned != "false" {
            return Err(format!(
                "pinned error on a row that returned {}",
                row.returned
            ));
        }
        return match result {
            Err(error) if error == expected => Ok(None),
            other => Err(format!(
                "expected {expected:?}, got {:?}",
                other.map(|found| found.is_some())
            )),
        };
    }
    match (row.returned, result) {
        ("true", Ok(Some(value))) => Ok(Some(value)),
        ("false", Ok(None)) => Ok(None),
        (returned, Ok(found)) => Err(format!(
            "upstream returned {returned}, the reader found something: {}",
            found.is_some()
        )),
        (returned, Err(error)) => Err(format!(
            "upstream returned {returned}, the reader failed: {error:?}"
        )),
    }
}

fn check_mac(row: &Row<'_>, result: Result<Option<MacAddr>, ReaderError>) -> Result<(), String> {
    if let Some(mac) = map_outcome(row, result)? {
        if mac.octets().as_slice() != row.output {
            return Err(format!("MAC {mac:?}, upstream wrote {:02x?}", row.output));
        }
    }
    Ok(())
}

fn check_services(row: &Row<'_>, result: Result<Option<&[u8]>, ReaderError>) -> Result<(), String> {
    let Some(services) = map_outcome(row, result)? else {
        return Ok(());
    };
    // Upstream's output: the count, then the services padded with zeros to 8.
    let mut expected = vec![u8::try_from(services.len()).map_err(|e| e.to_string())?];
    expected.extend_from_slice(services);
    expected.resize(1 + SERVICE_ARRAY_LEN, 0);
    if expected != row.output {
        return Err(format!(
            "services {expected:02x?}, upstream wrote {:02x?}",
            row.output
        ));
    }
    Ok(())
}

fn check_profile(
    row: &Row<'_>,
    result: Result<Option<em_types::ProfileType>, ReaderError>,
) -> Result<(), String> {
    if let Some(found) = map_outcome(row, result)? {
        // A 4-octet little-endian enum.
        let expected = [u8::from(found), 0, 0, 0];
        if expected != row.output.as_slice() {
            return Err(format!(
                "profile {expected:02x?}, upstream wrote {:02x?}",
                row.output
            ));
        }
    }
    Ok(())
}

fn check_freq_band(
    row: &Row<'_>,
    result: Result<Option<em_types::FreqBand>, ReaderError>,
) -> Result<(), String> {
    if let Some(found) = map_outcome(row, result)? {
        // Upstream writes the first octet of the 4-octet enum and leaves the rest alone.
        let expected = [u8::from(found), 0xFF, 0xFF, 0xFF];
        if expected != row.output.as_slice() {
            return Err(format!(
                "band {expected:02x?}, upstream wrote {:02x?}",
                row.output
            ));
        }
    }
    Ok(())
}

/// Checks a walker result against an expected offset (`-` is none).
fn check_walked(row: &Row<'_>, walked: Option<WalkedTlv<'_>>) -> Result<(), String> {
    match (row.returned, walked) {
        ("-", None) => Ok(()),
        ("-", Some(found)) => Err(format!(
            "upstream returned NULL, got offset {}",
            found.offset
        )),
        (_, None) => Err(format!(
            "upstream returned offset {}, got none",
            row.returned
        )),
        (offset_text, Some(found)) => {
            let offset: usize = offset_text
                .parse()
                .map_err(|e| format!("bad offset {offset_text:?}: {e}"))?;
            if found.offset != offset {
                return Err(format!("offset {}, upstream {offset}", found.offset));
            }
            let header = &row.area[offset..offset + 3];
            let value_len = usize::from(u16::from_be_bytes([header[1], header[2]]));
            let value = &row.area[offset + 3..offset + 3 + value_len];
            if found.tlv.tlv_type.0 != header[0] || found.tlv.value != value {
                return Err(format!("TLV at {offset} differs from the area"));
            }
            Ok(())
        }
    }
}

fn check_find_tlv(row: &Row<'_>) -> Result<(), String> {
    let type_text = input(row, "type")?;
    let type_byte = u8::from_str_radix(type_text, 16).map_err(|e| format!("bad type: {e}"))?;
    let found = find_tlv(&row.area, TlvType(type_byte))
        .map_err(|e| format!("framing error on a well-formed area: {e}"))?;
    match (row.returned, found) {
        ("-", None) => Ok(()),
        ("-", Some(_)) => Err("upstream returned NULL, found a TLV".to_string()),
        (returned, None) => Err(format!("upstream returned offset {returned}, found none")),
        (offset_text, Some(value)) => {
            let header_offset: usize = offset_text
                .parse()
                .map_err(|e| format!("bad offset {offset_text:?}: {e}"))?;
            // The value starts 3 octets after upstream's pointer to the TLV.
            let position = value.as_ptr().addr() - row.area.as_ptr().addr();
            if position != header_offset + 3 {
                return Err(format!(
                    "value at {position}, upstream TLV at {header_offset}"
                ));
            }
            let value_len = usize::from(u16::from_be_bytes([
                row.area[header_offset + 1],
                row.area[header_offset + 2],
            ]));
            if value.len() != value_len {
                return Err(format!(
                    "value length {}, upstream {value_len}",
                    value.len()
                ));
            }
            Ok(())
        }
    }
}

fn check_parse_profile(row: &Row<'_>) -> Result<(), String> {
    let value_len = input_number(row, "value_len")?;
    let value = row.area.get(..value_len).ok_or_else(|| {
        format!(
            "value_len {value_len} is past the {} octets",
            row.area.len()
        )
    })?;
    match (row.returned, parse_profile_value(value)) {
        ("true", Ok(found)) => {
            let expected = [u8::from(found), 0, 0, 0];
            if expected == row.output.as_slice() {
                Ok(())
            } else {
                Err(format!(
                    "profile {expected:02x?}, upstream wrote {:02x?}",
                    row.output
                ))
            }
        }
        ("false", Err(_)) => Ok(()),
        (returned, result) => Err(format!("upstream returned {returned}, got {result:?}")),
    }
}

fn check_row(row: &Row<'_>) -> Result<(), String> {
    if row.function != PARSE_PROFILE_TLV {
        let len = input_number(row, "len")?;
        if len != row.area.len() {
            return Err(format!("len {len}, area of {} octets", row.area.len()));
        }
    }
    let area = row.area.as_slice();
    match row.function {
        GET_AL_MAC_ADDRESS => check_mac(row, al_mac_address(area)),
        GET_SUPPORTED_SERVICE => check_services(row, supported_services(area)),
        GET_PROFILE | GET_PROFILE_TYPE => check_profile(row, profile(area)),
        GET_STA_MAC => check_mac(row, sta_mac(area)),
        GET_BSS_ID => check_mac(row, bss_id(area)),
        GET_RADIO_ID => check_mac(row, radio_id(area)),
        GET_FREQ_BAND => check_freq_band(row, freq_band(area)),
        GET_FIRST_TLV => check_walked(row, first_tlv(area).ok()),
        GET_NEXT_TLV => check_walked(row, next_tlv(area, input_number(row, "from")?)),
        GET_TLV_BY_TYPE | GET_TLV_IN_BUFFER => check_find_tlv(row),
        PARSE_PROFILE_TLV => check_parse_profile(row),
        other => Err(format!("unknown function {other}")),
    }
}

#[test]
fn fixture_has_exactly_the_expected_rows() {
    let rows = parse_rows(VECTORS).expect("fixture parses");
    assert_eq!(rows.len(), EXPECTED_ROWS);
    let corpus_areas = 73;
    let per_function = [
        (GET_AL_MAC_ADDRESS, corpus_areas),
        (GET_SUPPORTED_SERVICE, corpus_areas),
        (GET_PROFILE, corpus_areas),
        (GET_PROFILE_TYPE, corpus_areas),
        (GET_STA_MAC, corpus_areas),
        (GET_BSS_ID, corpus_areas),
        (GET_RADIO_ID, corpus_areas),
        (GET_FREQ_BAND, corpus_areas),
        (GET_FIRST_TLV, corpus_areas),
        (GET_NEXT_TLV, 204),
        (GET_TLV_BY_TYPE, 273),
        (GET_TLV_IN_BUFFER, 273),
        (PARSE_PROFILE_TLV, 14),
    ];
    let mut total = 0;
    for (function, expected) in per_function {
        let count = rows.iter().filter(|row| row.function == function).count();
        assert_eq!(count, expected, "{function}");
        total += count;
    }
    assert_eq!(total, EXPECTED_ROWS);
}

#[test]
fn every_reader_equals_upstream_on_the_corpus() {
    let rows = parse_rows(VECTORS).expect("fixture parses");
    for row in &rows {
        check_row(row).unwrap_or_else(|e| panic!("{} / {}: {e}", row.function, row.case));
    }
}

#[test]
fn pinned_error_rows_exist_and_are_upstream_false() {
    let rows = parse_rows(VECTORS).expect("fixture parses");
    for (function, case, _) in EXPECTED_ERRORS {
        let found: Vec<&Row<'_>> = rows
            .iter()
            .filter(|row| row.function == function && row.case == case)
            .collect();
        assert_eq!(found.len(), 1, "{function} / {case}");
        assert_eq!(found[0].returned, "false", "{function} / {case}");
    }
}

#[test]
fn channel_scan_req_0_radios_is_ok_none_for_get_radio_id() {
    // A10: the exact 2-octet value with Num_Radio = 0 is spec-valid, and upstream's `false` is
    // `Ok(None)`.
    let rows = parse_rows(VECTORS).expect("fixture parses");
    let mut found = 0;
    for row in &rows {
        if row.function == GET_RADIO_ID && row.case == "channel-scan-req-0-radios" {
            assert_eq!(row.returned, "false");
            assert_eq!(radio_id(&row.area), Ok(None));
            found += 1;
        }
    }
    assert_eq!(found, 1);
}

#[test]
fn first_tlv_failure_and_next_tlv_end_are_different_results() {
    // `zero-length-then-al-mac`: get_first_tlv fails at the empty first TLV, and the walk from
    // that TLV by offset still reaches the next one.
    let rows = parse_rows(VECTORS).expect("fixture parses");
    let mut found = 0;
    for row in &rows {
        if row.case == "zero-length-then-al-mac" && row.function == GET_FIRST_TLV {
            assert_eq!(
                first_tlv(&row.area).map(|w| w.offset),
                Err(FirstTlvError::ZeroLength)
            );
            assert_eq!(next_tlv(&row.area, 0).map(|w| w.offset), Some(3));
            found += 1;
        }
    }
    assert_eq!(found, 1);
}
