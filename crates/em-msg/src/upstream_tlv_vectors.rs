// SPDX-License-Identifier: Apache-2.0

//! Test-only check of the upstream TLV sender vectors.
//!
//! `tests/data/upstream_tlv_vectors.tsv` is written by `tools/upstream-tlv-vectors/regen`, which
//! compiles the pinned upstream senders (`create_prof_2_tlv`, `create_error_code_tlv`) against
//! their `em_base.h` structs. This module loads the fixture and checks the case list and the
//! bytes that later bodies are compared with.

const VECTORS: &str = include_str!("../tests/data/upstream_tlv_vectors.tsv");

const COLUMN_LINE: &str = "case\tfunction\tinputs\treturn\thex";

const PROF_2: &str = "em_t::create_prof_2_tlv";
const ERROR_CODE: &str = "em_configuration_t::create_error_code_tlv";

/// The exact (function, case) rows the fixture holds, in order.
const EXPECTED_CASES: [(&str, &str); 9] = [
    (PROF_2, "default"),
    (ERROR_CODE, "val-0-null"),
    (ERROR_CODE, "val-0-mac"),
    (ERROR_CODE, "val-1-null"),
    (ERROR_CODE, "val-1-mac"),
    (ERROR_CODE, "val-255-null"),
    (ERROR_CODE, "val-255-mac"),
    (ERROR_CODE, "val-256-null"),
    (ERROR_CODE, "val-256-mac"),
];

struct Row<'a> {
    case: &'a str,
    function: &'a str,
    inputs: &'a str,
    returned: usize,
    bytes: Vec<u8>,
}

fn hex_digit(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(format!("bad hex digit {:?}", char::from(byte))),
    }
}

fn decode_hex(text: &str) -> Result<Vec<u8>, String> {
    if text == "-" {
        return Ok(Vec::new());
    }
    let digits = text.as_bytes();
    if digits.len() % 2 != 0 {
        return Err(format!("odd number of hex digits in {text:?}"));
    }
    let mut out = Vec::with_capacity(digits.len() / 2);
    let mut index = 0;
    while index < digits.len() {
        out.push((hex_digit(digits[index])? << 4) | hex_digit(digits[index + 1])?);
        index += 2;
    }
    Ok(out)
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
        if columns.len() != 5 {
            return Err(format!(
                "expected 5 columns, found {}: {line}",
                columns.len()
            ));
        }
        let returned: usize = columns[3]
            .parse()
            .map_err(|e| format!("{}: bad return {:?}: {e}", columns[0], columns[3]))?;
        let bytes = decode_hex(columns[4]).map_err(|e| format!("{}: {e}", columns[0]))?;
        rows.push(Row {
            case: columns[0],
            function: columns[1],
            inputs: columns[2],
            returned,
            bytes,
        });
    }
    Ok(rows)
}

fn find<'a, 'b>(rows: &'b [Row<'a>], function: &str, case: &str) -> &'b Row<'a> {
    for row in rows {
        if row.function == function && row.case == case {
            return row;
        }
    }
    panic!("no row {function}/{case}");
}

#[test]
fn fixture_has_exactly_the_expected_cases() {
    let rows = parse_rows(VECTORS).expect("fixture parses");
    let found: Vec<(&str, &str)> = rows.iter().map(|r| (r.function, r.case)).collect();
    assert_eq!(found, EXPECTED_CASES);
    for row in &rows {
        assert_eq!(
            row.returned,
            row.bytes.len(),
            "{}: return is the byte count",
            row.case
        );
    }
}

#[test]
fn profile_2_ap_capability_sets_only_dpp_onboarding() {
    let rows = parse_rows(VECTORS).expect("fixture parses");
    let row = find(&rows, PROF_2, "default");
    assert_eq!(row.inputs, "fill=ff");
    // em_profile_2_ap_cap_t is 4 octets under GCC (see its `# Probe:` line in the fixture):
    // max_prior_rule, reserved1, one bit-field octet, max_vid_count. GCC allocates bit-fields from
    // the least significant bit, so reserved2 is bits 0-2, traffic_separation bit 3, dpp_onboarding bit 4
    // (0x10), prioritization bit 5 and byte_counter_units bits 6-7.
    assert_eq!(row.bytes, [0x00, 0x00, 0x10, 0x00]);
}

#[test]
fn error_code_is_the_low_byte_of_val_then_the_station_mac() {
    const STATION: [u8; 6] = [0x02, 0x11, 0x22, 0x33, 0x44, 0x55];
    let rows = parse_rows(VECTORS).expect("fixture parses");
    for (value, low_byte) in [(0u16, 0u8), (1, 1), (255, 255), (256, 0)] {
        let with_mac = find(&rows, ERROR_CODE, &format!("val-{value}-mac"));
        let mut expected = vec![low_byte];
        expected.extend_from_slice(&STATION);
        assert_eq!(with_mac.bytes, expected, "val {value} with a MAC");

        // A NULL sta_mac is written as six zero octets, not left alone.
        let without_mac = find(&rows, ERROR_CODE, &format!("val-{value}-null"));
        let mut expected = vec![low_byte];
        expected.extend_from_slice(&[0; 6]);
        assert_eq!(without_mac.bytes, expected, "val {value} with NULL");
    }
}
