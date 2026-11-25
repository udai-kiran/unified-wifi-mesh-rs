// SPDX-License-Identifier: Apache-2.0

//! Test-only access to the generated TLV body fixtures.

#![allow(
    clippy::disallowed_types,
    reason = "test-only module; HashSet checks fixture keys are unique (0016 U27 allows tests)"
)]

use std::collections::HashSet;

pub type Key = (&'static str, &'static str);

/// One row of a fixture group.
pub struct Row<'a> {
    pub tlv: &'a str,
    pub case: &'a str,
    pub fields: &'a str,
    pub hex: &'a str,
}

/// The `kind` field of a row. A row without one is a spec form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The strict decoder accepts the value.
    Spec,
    /// The strict decoder rejects the value with the error named in `error=`.
    Invalid,
    /// Only the lenient decoder accepts the value.
    Lenient,
}

impl Row<'_> {
    pub fn key(&self) -> (&str, &str) {
        (self.tlv, self.case)
    }

    /// Returns the value of the named `fields` entry.
    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .split(';')
            .filter_map(|field| field.split_once('='))
            .find(|&(field_name, _)| field_name == name)
            .map(|(_, value)| value)
    }

    pub fn kind(&self) -> Kind {
        match self.field("kind") {
            None | Some("spec") => Kind::Spec,
            Some("invalid") => Kind::Invalid,
            Some("lenient") => Kind::Lenient,
            Some(other) => panic!("unknown kind {other:?} in {:?}", self.key()),
        }
    }

    /// True if a reserved field of the row holds a nonzero value. Decoding such a row ignores
    /// those bits, so encoding it may give other bytes.
    pub fn names_reserved_bits(&self) -> bool {
        self.fields
            .split(';')
            .filter_map(|field| field.split_once('='))
            .any(|(name, value)| name.ends_with("reserved") && value != "0")
    }

    /// The row's value, decoded from the hex column (`-` is the empty value).
    pub fn bytes(&self) -> Vec<u8> {
        decode_hex(self.hex, self.key())
    }
}

fn group_text(group: &str) -> &'static str {
    match group {
        "autoconfig" => include_str!("../tests/data/bodies/autoconfig.tsv"),
        "cac" => include_str!("../tests/data/bodies/cac.tsv"),
        "capability" => include_str!("../tests/data/bodies/capability.tsv"),
        "channel" => include_str!("../tests/data/bodies/channel.tsv"),
        "ieee1905" => include_str!("../tests/data/bodies/ieee1905.tsv"),
        "linkmetric" => include_str!("../tests/data/bodies/linkmetric.tsv"),
        "metrics" => include_str!("../tests/data/bodies/metrics.tsv"),
        "mld" => include_str!("../tests/data/bodies/mld.tsv"),
        "policy" => include_str!("../tests/data/bodies/policy.tsv"),
        "security" => include_str!("../tests/data/bodies/security.tsv"),
        "steering" => include_str!("../tests/data/bodies/steering.tsv"),
        _ => panic!("unknown body fixture group {group:?}"),
    }
}

fn rows(group: &str) -> Vec<Row<'static>> {
    let mut rows = Vec::new();
    let mut saw_columns = false;
    for line in group_text(group).lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if !saw_columns {
            assert_eq!(
                line, "tlv\tcase\tfields\thex",
                "invalid {group} fixture columns"
            );
            saw_columns = true;
            continue;
        }
        let mut columns = line.split('\t');
        let (Some(tlv), Some(case), Some(fields), Some(hex), None) = (
            columns.next(),
            columns.next(),
            columns.next(),
            columns.next(),
            columns.next(),
        ) else {
            panic!("invalid {group} fixture row: {line:?}");
        };
        rows.push(Row {
            tlv,
            case,
            fields,
            hex,
        });
    }
    assert!(saw_columns, "missing {group} fixture columns");
    rows
}

fn find(group: &str, key: Key) -> Row<'static> {
    let mut matches = rows(group)
        .into_iter()
        .filter(|row| (row.tlv, row.case) == key);
    let row = matches
        .next()
        .unwrap_or_else(|| panic!("missing body fixture {group}/{}/{}", key.0, key.1));
    assert!(
        matches.next().is_none(),
        "duplicate body fixture {group}/{}/{}",
        key.0,
        key.1
    );
    row
}

fn decode_hex(hex: &str, key: (&str, &str)) -> Vec<u8> {
    if hex == "-" {
        return Vec::new();
    }
    let digits = hex.as_bytes();
    assert!(
        digits.len() % 2 == 0,
        "odd hex length in {}/{}",
        key.0,
        key.1
    );
    digits
        .chunks_exact(2)
        .map(|pair| {
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => panic!("invalid hex in {}/{}", key.0, key.1),
            };
            (digit(pair[0]) << 4) | digit(pair[1])
        })
        .collect()
}

/// Returns the decoded bytes for the named fixture. A hex column of `-` is the empty value.
pub fn value(group: &str, key: Key) -> Vec<u8> {
    find(group, key).bytes()
}

/// Returns every row of a group, in file order. A test that checks coverage walks these, so a
/// row cannot be left out by a hand-kept list.
pub fn all_rows(group: &str) -> Vec<Row<'static>> {
    rows(group)
}

/// Returns a fixture's metadata column.
pub fn fields(group: &str, key: Key) -> &'static str {
    find(group, key).fields
}

/// Fails if a fixture row is not in `used`, if `used` names a row that does not exist, or if
/// either list repeats a row.
pub fn assert_all_used(group: &str, used: &[Key]) {
    let all_rows = rows(group);
    let all: HashSet<_> = all_rows.iter().map(|row| (row.tlv, row.case)).collect();
    assert_eq!(
        all.len(),
        all_rows.len(),
        "duplicate rows in {group} fixtures"
    );
    let used_keys: HashSet<_> = used.iter().copied().collect();
    assert_eq!(
        used_keys.len(),
        used.len(),
        "duplicate used keys in {group} fixtures"
    );
    let mut missing: Vec<_> = all.difference(&used_keys).copied().collect();
    let mut unknown: Vec<_> = used_keys.difference(&all).copied().collect();
    missing.sort_unstable();
    unknown.sort_unstable();
    assert!(
        missing.is_empty() && unknown.is_empty(),
        "{group} fixture coverage: unused {missing:?}; unknown {unknown:?}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "missing body fixture ieee1905/0x0b/not-a-case")]
    fn missing_case_has_a_clear_message() {
        let _ = value("ieee1905", ("0x0b", "not-a-case"));
    }
}
