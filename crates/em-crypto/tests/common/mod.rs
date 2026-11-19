// SPDX-License-Identifier: Apache-2.0

//! Shared helpers: hex, and a reader for the upstream oracle fixture that tracks which fields
//! a test actually read.

#![allow(dead_code)] // each test binary uses a different subset

use std::cell::{Cell, OnceCell};

/// Path of the oracle fixture, relative to the crate directory.
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/data/upstream_crypto_vectors.tsv"
);

/// Decodes lowercase or uppercase hex; `-` is the fixture's spelling of "empty".
pub fn hex(text: &str) -> Vec<u8> {
    if text == "-" {
        return Vec::new();
    }
    assert!(text.len() % 2 == 0, "odd-length hex: {text}");
    text.as_bytes()
        .chunks(2)
        .map(|pair| {
            let digits = std::str::from_utf8(pair).expect("ascii hex");
            u8::from_str_radix(digits, 16).expect("hex digit")
        })
        .collect()
}

/// One `key=value` list (`a=1;b=2`) whose entries must each be read by the test.
pub struct Fields {
    entries: Vec<(String, String, Cell<bool>)>,
}

impl Fields {
    fn parse(text: &str) -> Self {
        let entries = text
            .split(';')
            .map(|entry| {
                let (key, value) = entry
                    .split_once('=')
                    .unwrap_or_else(|| panic!("not key=value: {entry:.40}"));
                (key.to_string(), value.to_string(), Cell::new(false))
            })
            .collect();
        Self { entries }
    }

    /// The value of `key`; marks it as read.
    pub fn get(&self, key: &str) -> &str {
        let entry = self
            .entries
            .iter()
            .find(|(name, _, _)| name == key)
            .unwrap_or_else(|| panic!("fixture field {key} missing"));
        entry.2.set(true);
        &entry.1
    }

    /// Hex-decoded value of `key`.
    pub fn bytes(&self, key: &str) -> Vec<u8> {
        hex(self.get(key))
    }

    /// Numeric value of `key`.
    pub fn number(&self, key: &str) -> usize {
        self.get(key)
            .parse()
            .unwrap_or_else(|_| panic!("field {key} is not a number"))
    }

    /// Hex-decoded comma-separated pieces (`a,b,c`) of `key`. `none` is zero pieces, and `-` is
    /// one empty piece.
    pub fn pieces(&self, key: &str) -> Vec<Vec<u8>> {
        let value = self.get(key);
        if value == "none" {
            return Vec::new();
        }
        value.split(',').map(hex).collect()
    }

    fn assert_all_read(&self, row: &str) {
        for (name, _, read) in &self.entries {
            assert!(read.get(), "{row}: fixture field {name} was never read");
        }
    }
}

/// One fixture row: category, case, inputs, upstream return value, upstream output.
pub struct Row {
    pub kind: String,
    pub case: String,
    /// The input column.
    pub inputs: Fields,
    upstream_return: String,
    output_text: String,
    output_fields: OnceCell<Fields>,
    output_read: Cell<bool>,
    return_read: Cell<bool>,
}

impl Row {
    fn parse(line: &str) -> Self {
        let columns: Vec<&str> = line.split('\t').collect();
        assert_eq!(
            columns.len(),
            5,
            "fixture row must have 5 columns: {line:.60}"
        );
        Self {
            kind: columns[0].to_string(),
            case: columns[1].to_string(),
            inputs: Fields::parse(columns[2]),
            upstream_return: columns[3].to_string(),
            output_text: columns[4].to_string(),
            output_fields: OnceCell::new(),
            output_read: Cell::new(false),
            return_read: Cell::new(false),
        }
    }

    /// `kind/case`, for messages.
    pub fn name(&self) -> String {
        format!("{}/{}", self.kind, self.case)
    }

    /// Upstream's return value column.
    pub fn upstream_return(&self) -> &str {
        self.return_read.set(true);
        &self.upstream_return
    }

    /// The output column as one hex string (`-` is empty).
    pub fn output_hex(&self) -> Vec<u8> {
        self.output_read.set(true);
        hex(&self.output_text)
    }

    /// The output column as `key=value;...` fields.
    pub fn output_fields(&self) -> &Fields {
        self.output_read.set(true);
        self.output_fields
            .get_or_init(|| Fields::parse(&self.output_text))
    }

    /// Fails unless every column and every field was read.
    pub fn assert_consumed(&self) {
        let name = self.name();
        assert!(self.return_read.get(), "{name}: return column never read");
        assert!(self.output_read.get(), "{name}: output column never read");
        self.inputs.assert_all_read(&name);
        if let Some(fields) = self.output_fields.get() {
            fields.assert_all_read(&name);
        }
    }
}

/// All data rows of the fixture, in file order.
pub fn rows() -> Vec<Row> {
    let text = std::fs::read_to_string(FIXTURE).expect("fixture file");
    text.lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(Row::parse)
        .collect()
}

/// The row with this kind and case. Marks nothing as consumed.
pub fn row(kind: &str, case: &str) -> Row {
    rows()
        .into_iter()
        .find(|row| row.kind == kind && row.case == case)
        .unwrap_or_else(|| panic!("fixture row {kind}/{case} missing"))
}
