// SPDX-License-Identifier: Apache-2.0

//! Test-only check of the builder against bytes made by upstream's own code.
//!
//! The fixture `tests/data/upstream_cmdu_vectors.tsv` is written by
//! `tools/upstream-cmdu-vectors/vectors.cpp`, which is built from upstream's
//! `em_msg.cpp` and `em_base.h` (`add_1905_header`, `add_tlv`).

use em_types::{MsgType, TlvType};

use crate::{AL_SAP_BUDGET, CmduHeader, MsgBuilder, TlvIter, default_relay};

const VECTORS: &str = include_str!("../tests/data/upstream_cmdu_vectors.tsv");
const CLASSIFICATION: &str = include_str!("../../em-types/tests/data/classification.tsv");

/// Number of data lines the fixture must hold.
const EXPECTED_CASES: usize = 76;

fn data_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    for line in text.lines() {
        if !line.starts_with('#') && !line.is_empty() {
            lines.push(line);
        }
    }
    lines
}

fn rust_name<'a>(upstream: &str) -> Result<&'a str, String> {
    for line in data_lines(CLASSIFICATION) {
        let columns: Vec<&str> = line.split('\t').collect();
        if columns.len() > 3 && columns[1] == upstream {
            return Ok(columns[3]);
        }
    }
    Err(format!("{upstream} is not in the classification"))
}

fn msg_type_of(upstream: &str) -> Result<MsgType, String> {
    let name = rust_name(upstream)?;
    for candidate in MsgType::ALL {
        if candidate.name() == Some(name) {
            return Ok(*candidate);
        }
    }
    Err(format!("no MsgType named {name} for {upstream}"))
}

fn tlv_type_of(upstream: &str) -> Result<TlvType, String> {
    let name = rust_name(upstream)?;
    for candidate in TlvType::ALL {
        if candidate.name() == Some(name) {
            return Ok(*candidate);
        }
    }
    Err(format!("no TlvType named {name} for {upstream}"))
}

fn hex_digit(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(format!("bad hex digit {:?}", char::from(byte))),
    }
}

fn decode_hex(text: &str) -> Result<Vec<u8>, String> {
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

/// Splits the `tlvs` column into (type, value) pairs.
fn parse_tlvs(column: &str) -> Result<Vec<(TlvType, Vec<u8>)>, String> {
    let mut tlvs = Vec::new();
    if column == "-" {
        return Ok(tlvs);
    }
    for item in column.split(';') {
        let Some((name, value)) = item.split_once('=') else {
            return Err(format!("TLV item {item:?} has no '='"));
        };
        tlvs.push((tlv_type_of(name)?, decode_hex(value)?));
    }
    Ok(tlvs)
}

fn check_line(line: &str) -> Result<(), String> {
    let columns: Vec<&str> = line.split('\t').collect();
    if columns.len() != 6 {
        return Err(format!(
            "expected 6 columns, found {}: {line}",
            columns.len()
        ));
    }
    let case = columns[0];
    let msg_type = msg_type_of(columns[1]).map_err(|e| format!("{case}: {e}"))?;
    let msg_id: u16 = columns[2]
        .parse()
        .map_err(|e| format!("{case}: bad msg_id {:?}: {e}", columns[2]))?;
    let relay_text = columns[3];
    let relay = match relay_text {
        "0" => false,
        "1" => true,
        other => return Err(format!("{case}: bad relay {other:?}")),
    };
    let tlvs = parse_tlvs(columns[4]).map_err(|e| format!("{case}: {e}"))?;
    let expected = decode_hex(columns[5]).map_err(|e| format!("{case}: {e}"))?;

    if default_relay(msg_type) != relay {
        return Err(format!(
            "{case}: default_relay is {}, upstream says {relay}",
            default_relay(msg_type)
        ));
    }

    let header = CmduHeader {
        msg_type,
        msg_id,
        frag_id: 0,
        last_fragment: true,
        relay: default_relay(msg_type),
    };
    let mut builder = MsgBuilder::new(&header, AL_SAP_BUDGET);
    for (tlv_type, value) in &tlvs {
        builder.tlv(*tlv_type, value);
    }
    let built = builder
        .finish()
        .map_err(|e| format!("{case}: build failed: {e}"))?;
    if built != expected {
        return Err(format!(
            "{case}: builder bytes differ from upstream\n  built:    {built:02x?}\n  upstream: {expected:02x?}"
        ));
    }

    let (parsed, area) = CmduHeader::parse(&built).map_err(|e| format!("{case}: {e}"))?;
    if parsed != header {
        return Err(format!("{case}: parsed header {parsed:?} != {header:?}"));
    }
    let mut found = Vec::new();
    for item in TlvIter::new(area).take(64) {
        let tlv = item.map_err(|e| format!("{case}: TLV walk failed: {e}"))?;
        found.push(tlv);
    }
    if found.len() != tlvs.len() {
        return Err(format!(
            "{case}: parsed {} TLVs, expected {}",
            found.len(),
            tlvs.len()
        ));
    }
    for (index, (tlv, (tlv_type, value))) in found.iter().zip(&tlvs).enumerate() {
        if tlv.tlv_type != *tlv_type || tlv.value != value.as_slice() {
            return Err(format!("{case}: TLV {index} differs after parsing"));
        }
    }
    Ok(())
}

#[test]
fn builder_matches_upstream_bytes() {
    let lines = data_lines(VECTORS);
    // The first data line is the column line.
    assert_eq!(
        lines.first().copied(),
        Some("case\tmsg_type\tmsg_id\trelay\ttlvs\tcmdu_hex"),
        "column line"
    );
    let cases = &lines[1..];
    assert_eq!(cases.len(), EXPECTED_CASES, "number of cases");
    for line in cases {
        if let Err(message) = check_line(line) {
            panic!("{message}");
        }
    }
}
