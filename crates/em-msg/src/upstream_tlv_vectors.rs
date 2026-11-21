// SPDX-License-Identifier: Apache-2.0

//! Test-only check of the upstream TLV sender vectors.
//!
//! `tests/data/upstream_tlv_vectors.tsv` is written by `tools/upstream-tlv-vectors/regen`, which
//! extracts the seven senders in its `SENDERS` table from the pinned sources and compiles them
//! with the structs and macros they use: the Profile-2 capability, the Error Code (configuration
//! and metrics), the Client Info, the Backhaul BSS Configuration policy and the two Unassociated
//! STA Link Metrics functions. This module loads the fixture, checks the exact case list, and
//! decodes and re-encodes each row through the body that matches its sender. The two rows of the
//! unreachable empty-value guards are not decoded; a test checks that their bodies reject an
//! empty value instead.

use crate::TlvBody;
use crate::bodies::{
    BackhaulBssConfiguration, ClientInfo, ErrorCode, Profile2ApCapability,
    UnassociatedStaLinkMetricsQuery, UnassociatedStaLinkMetricsResponse,
};

const VECTORS: &str = include_str!("../tests/data/upstream_tlv_vectors.tsv");

const COLUMN_LINE: &str = "case\tfunction\tinputs\treturn\thex";

const PROF_2: &str = "em_t::create_prof_2_tlv";
const CONFIG_ERROR_CODE: &str = "em_configuration_t::create_error_code_tlv";
const METRICS_ERROR_CODE: &str = "em_metrics_t::create_error_code_tlv";
const CLIENT_INFO: &str = "em_capability_t::create_client_info_tlv";
const BACKHAUL_BSS: &str = "em_policy_cfg_t::create_backhaul_bss_conf_policy_tlv";
const UNASSOC_QUERY: &str = "em_metrics_t::create_unassoc_sta_link_metrics_query_tlv";
const UNASSOC_RESPONSE: &str = "em_metrics_t::create_unassoc_sta_link_metrics_resp_tlv";

/// The exact (function, case) rows the fixture holds, in order.
const EXPECTED_CASES: [(&str, &str); 31] = [
    (PROF_2, "default"),
    (CONFIG_ERROR_CODE, "val-0-null"),
    (CONFIG_ERROR_CODE, "val-0-mac"),
    (CONFIG_ERROR_CODE, "val-1-null"),
    (CONFIG_ERROR_CODE, "val-1-mac"),
    (CONFIG_ERROR_CODE, "val-255-null"),
    (CONFIG_ERROR_CODE, "val-255-mac"),
    (CONFIG_ERROR_CODE, "val-256-null"),
    (CONFIG_ERROR_CODE, "val-256-mac"),
    (METRICS_ERROR_CODE, "found-0-assoc-0"),
    (METRICS_ERROR_CODE, "found-0-assoc-1"),
    (METRICS_ERROR_CODE, "found-1-assoc-0"),
    (METRICS_ERROR_CODE, "found-1-assoc-1"),
    (CLIENT_INFO, "default"),
    (CLIENT_INFO, "swapped"),
    (BACKHAUL_BSS, "null-bssid"),
    (BACKHAUL_BSS, "p-one-0-p-two-0"),
    (BACKHAUL_BSS, "p-one-0-p-two-1"),
    (BACKHAUL_BSS, "p-one-1-p-two-0"),
    (BACKHAUL_BSS, "p-one-1-p-two-1"),
    (UNASSOC_QUERY, "null-op"),
    (UNASSOC_QUERY, "no-channels"),
    (UNASSOC_QUERY, "one-channel-one-sta"),
    (UNASSOC_QUERY, "one-channel-two-sta"),
    (UNASSOC_QUERY, "two-channels"),
    (UNASSOC_QUERY, "channel-without-sta"),
    (UNASSOC_RESPONSE, "empty"),
    (UNASSOC_RESPONSE, "one-entry"),
    (UNASSOC_RESPONSE, "op-115-of-mixed"),
    (UNASSOC_RESPONSE, "op-121-of-mixed"),
    (UNASSOC_RESPONSE, "op-125-of-mixed"),
];

/// The sender rows whose value is empty. Each sender returns 0 before it writes anything.
const UNREACHABLE_SENDER_GUARDS: [(&str, &str); 2] =
    [(BACKHAUL_BSS, "null-bssid"), (UNASSOC_QUERY, "null-op")];

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

pub(crate) fn decode_hex(text: &str) -> Result<Vec<u8>, String> {
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
        let with_mac = find(&rows, CONFIG_ERROR_CODE, &format!("val-{value}-mac"));
        let mut expected = vec![low_byte];
        expected.extend_from_slice(&STATION);
        assert_eq!(with_mac.bytes, expected, "val {value} with a MAC");

        // A NULL sta_mac is written as six zero octets, not left alone.
        let without_mac = find(&rows, CONFIG_ERROR_CODE, &format!("val-{value}-null"));
        let mut expected = vec![low_byte];
        expected.extend_from_slice(&[0; 6]);
        assert_eq!(without_mac.bytes, expected, "val {value} with NULL");
    }
}

/// Decodes `bytes` as body `B` (strict) and encodes it again.
fn round_trip<'a, B: TlvBody<'a>>(bytes: &'a [u8]) -> Result<Vec<u8>, String> {
    let body = B::decode_value(bytes).map_err(|e| format!("decode: {e}"))?;
    body.encode_value().map_err(|e| format!("encode: {e}"))
}

fn round_trip_for(function: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
    match function {
        PROF_2 => round_trip::<Profile2ApCapability>(bytes),
        CONFIG_ERROR_CODE | METRICS_ERROR_CODE => round_trip::<ErrorCode>(bytes),
        CLIENT_INFO => round_trip::<ClientInfo>(bytes),
        BACKHAUL_BSS => round_trip::<BackhaulBssConfiguration>(bytes),
        UNASSOC_QUERY => round_trip::<UnassociatedStaLinkMetricsQuery>(bytes),
        UNASSOC_RESPONSE => round_trip::<UnassociatedStaLinkMetricsResponse>(bytes),
        other => Err(format!("no body for {other}")),
    }
}

#[test]
fn every_sender_row_decodes_and_encodes_byte_identically_through_its_body() {
    let rows = parse_rows(VECTORS).expect("fixture parses");
    let mut checked = 0;
    for row in &rows {
        if UNREACHABLE_SENDER_GUARDS.contains(&(row.function, row.case)) {
            continue;
        }
        let encoded = round_trip_for(row.function, &row.bytes)
            .unwrap_or_else(|e| panic!("{}/{}: {e}", row.function, row.case));
        assert_eq!(encoded, row.bytes, "{}/{}", row.function, row.case);
        checked += 1;
    }
    assert_eq!(
        checked,
        EXPECTED_CASES.len() - UNREACHABLE_SENDER_GUARDS.len()
    );
}

#[test]
fn null_bssid_and_null_op_are_unreachable_sender_guards() {
    // The two senders return 0 with nothing written when their input is NULL or the null MAC:
    // `create_backhaul_bss_conf_policy_tlv` for a zero BSSID and
    // `create_unassoc_sta_link_metrics_query_tlv` for a NULL op class. Neither guard is
    // reachable. The caller skips a null BSSID before it calls the sender
    // (em_policy_cfg.cpp:630-633), and the query caller passes the address of an array element
    // (em_metrics.cpp:2765). An empty value is not a form of either body, so no lenient form is
    // added.
    let rows = parse_rows(VECTORS).expect("fixture parses");
    for (function, case) in UNREACHABLE_SENDER_GUARDS {
        let row = find(&rows, function, case);
        assert_eq!(row.returned, 0, "{function}/{case}");
        assert!(row.bytes.is_empty(), "{function}/{case}");
    }
    assert!(BackhaulBssConfiguration::decode_value(&[]).is_err());
    assert!(UnassociatedStaLinkMetricsQuery::decode_value(&[]).is_err());
}
