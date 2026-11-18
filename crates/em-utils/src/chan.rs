// SPDX-License-Identifier: Apache-2.0

//! Wi-Fi channel table and channel/frequency lookups.
//!
//! Replaces upstream `frequency_ranges`, `em_chan_to_freq` and
//! `em_freq_to_chan` (`src/utils/util.cpp:309-454`). The table is kept exactly
//! as upstream has it, quirks included (see note 0003 D2).

/// One row of the channel table: the channels of one operating class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreqRange {
    /// Operating class number.
    pub op_class: u8,
    /// First channel number of the row.
    pub min_chan: u8,
    /// Last channel number of the row.
    pub max_chan: u8,
    /// Frequency of channel 0, in MHz.
    pub base_freq: u16,
    /// Distance between channel numbers, in MHz.
    pub spacing: u16,
    /// Region of the row (for example `"US"`), or `""` for a row that applies everywhere.
    pub region: &'static str,
}

/// Builds one table row, so each row fits on one line.
const fn row(
    op_class: u8,
    min_chan: u8,
    max_chan: u8,
    base_freq: u16,
    spacing: u16,
    region: &'static str,
) -> FreqRange {
    FreqRange {
        op_class,
        min_chan,
        max_chan,
        base_freq,
        spacing,
        region,
    }
}

/// The channel table: all 68 rows of upstream `frequency_ranges`, in the same order.
///
/// A row with region `""` is global.
pub const FREQ_RANGES: &[FreqRange] = &[
    // 2.4 GHz, global
    row(81, 1, 13, 2407, 5, ""),
    row(82, 14, 14, 2414, 5, ""),
    row(83, 1, 13, 2407, 5, ""),
    row(84, 5, 13, 2407, 5, ""),
    // 5 GHz, global
    row(115, 36, 48, 5000, 5, ""),
    row(116, 36, 44, 5000, 5, ""),
    row(117, 40, 48, 5000, 5, ""),
    row(118, 52, 64, 5000, 5, ""),
    row(119, 52, 60, 5000, 5, ""),
    row(120, 56, 64, 5000, 5, ""),
    row(121, 100, 140, 5000, 5, ""),
    row(122, 100, 142, 5000, 5, ""),
    row(123, 104, 136, 5000, 5, ""),
    row(124, 149, 161, 5000, 5, ""),
    row(125, 149, 177, 5000, 5, ""),
    row(126, 149, 173, 5000, 5, ""),
    row(127, 153, 177, 5000, 5, ""),
    row(128, 36, 177, 5000, 5, ""),
    row(129, 36, 177, 5000, 5, ""),
    row(130, 36, 177, 5000, 5, ""),
    // 6 GHz, global
    row(131, 1, 233, 5950, 5, ""),
    row(132, 3, 233, 5950, 5, ""),
    row(133, 7, 233, 5950, 5, ""),
    row(134, 15, 233, 5950, 5, ""),
    row(135, 7, 233, 5950, 5, ""),
    row(136, 2, 2, 5935, 1, ""),
    // 60 GHz, global
    row(180, 1, 8, 56160, 2160, ""),
    row(181, 9, 15, 56160, 2160, ""),
    row(182, 17, 22, 56160, 2160, ""),
    row(183, 25, 29, 56160, 2160, ""),
    // 2.4 GHz, US
    row(12, 1, 11, 2407, 5, "US"),
    row(32, 1, 7, 2407, 5, "US"),
    row(33, 5, 11, 2407, 5, "US"),
    // 5 GHz, US
    row(1, 36, 48, 5000, 5, "US"),
    row(2, 52, 64, 5000, 5, "US"),
    row(4, 100, 144, 5000, 5, "US"),
    row(5, 149, 165, 5000, 5, "US"),
    // 60 GHz, US
    row(34, 1, 8, 56160, 2160, "US"),
    row(37, 9, 15, 56160, 2160, "US"),
    row(38, 17, 22, 56160, 2160, "US"),
    row(39, 25, 29, 56160, 2160, "US"),
    // 2.4 GHz, EU
    row(4, 1, 13, 2407, 5, "EU"),
    row(11, 1, 9, 2407, 5, "EU"),
    row(12, 5, 13, 2407, 5, "EU"),
    // 5 GHz, EU
    row(1, 36, 48, 5000, 5, "EU"),
    row(2, 52, 64, 5000, 5, "EU"),
    row(3, 100, 140, 5000, 5, "EU"),
    row(17, 149, 169, 5000, 5, "EU"),
    // 60 GHz, EU
    row(18, 1, 6, 56160, 2160, "EU"),
    row(21, 9, 11, 56160, 2160, "EU"),
    row(22, 17, 18, 56160, 2160, "EU"),
    row(23, 25, 25, 56160, 2160, "EU"),
    // 2.4 GHz, JP
    row(30, 1, 13, 2407, 5, "JP"),
    row(31, 14, 14, 2414, 5, "JP"),
    // 5 GHz, JP
    row(1, 34, 64, 5000, 5, "JP"),
    row(32, 52, 64, 5000, 5, "JP"),
    row(34, 100, 140, 5000, 5, "JP"),
    // 60 GHz, JP
    row(59, 1, 6, 56160, 2160, "JP"),
    row(62, 9, 11, 56160, 2160, "JP"),
    row(63, 17, 18, 56160, 2160, "JP"),
    row(64, 25, 25, 56160, 2160, "JP"),
    // 2.4 GHz, CN
    row(7, 1, 13, 2407, 5, "CN"),
    row(8, 1, 9, 2407, 5, "CN"),
    row(9, 5, 13, 2407, 5, "CN"),
    // 5 GHz, CN
    row(1, 36, 48, 5000, 5, "CN"),
    row(2, 52, 64, 5000, 5, "CN"),
    row(3, 149, 165, 5000, 5, "CN"),
    row(6, 149, 157, 5000, 5, "CN"),
];

/// Finds the frequency of a channel, in MHz.
///
/// Rows are tried in table order. A row matches when its operating class is
/// `op_class`, the channel is inside the row's channel range, and the row is
/// global or its region equals `region`. The first match wins. A `chan` of 0
/// means "the first channel of whichever row is being looked at".
///
/// Use `""` for `region` when there is no region. Region matching is exact and
/// case-sensitive. Returns `None` when no row matches.
#[must_use]
pub fn chan_to_freq(op_class: u8, chan: u8, region: &str) -> Option<u32> {
    for row in FREQ_RANGES {
        let c = if chan == 0 { row.min_chan } else { chan };
        let region_matches = row.region.is_empty() || row.region == region;
        if row.op_class == op_class && row.min_chan <= c && c <= row.max_chan && region_matches {
            return Some(u32::from(row.base_freq) + u32::from(c) * u32::from(row.spacing));
        }
    }
    None
}

/// Finds the operating class and channel for a frequency in MHz.
///
/// Returns `(op_class, channel)`. Rows are tried in table order, and a row
/// matches when the frequency is exactly a channel of that row. A row whose
/// region equals `region` (exact, case-sensitive) is returned at once. A global
/// row is only remembered.
///
/// So with `region` `""`, the first global match wins. With a region and no
/// match in that region, the last global match is returned. Returns `None`
/// when nothing matches.
#[must_use]
pub fn freq_to_chan(freq_mhz: u32, region: &str) -> Option<(u8, u8)> {
    let mut global = None;
    for row in FREQ_RANGES {
        let base = u32::from(row.base_freq);
        let spacing = u32::from(row.spacing);
        let min = u32::from(row.min_chan);
        let max = u32::from(row.max_chan);
        // Check the range first, so the subtraction below cannot go below zero.
        if freq_mhz < base + min * spacing || freq_mhz > base + max * spacing {
            continue;
        }
        if (freq_mhz - base) % spacing != 0 {
            continue;
        }
        let c = (freq_mhz - base) / spacing;
        if c < min || c > max {
            continue;
        }
        let Ok(c8) = u8::try_from(c) else {
            continue;
        };
        if row.region == region {
            return Some((row.op_class, c8));
        }
        if row.region.is_empty() {
            global = Some((row.op_class, c8));
        }
    }
    global
}

#[cfg(test)]
mod tests {
    use super::{FREQ_RANGES, FreqRange, chan_to_freq, freq_to_chan};

    /// Reads the fixture generated from upstream.
    fn fixture_rows() -> Vec<FreqRange> {
        let text = include_str!("../tests/data/upstream_freq_ranges.tsv");
        let mut rows = Vec::new();
        for line in text.lines() {
            if line.starts_with('#') || line.starts_with("op_class") {
                continue;
            }
            let cols: Vec<&str> = line.split('\t').collect();
            assert_eq!(cols.len(), 6, "bad column count: {line}");
            let region = if cols[5] == "-" { "" } else { cols[5] };
            rows.push(FreqRange {
                op_class: cols[0].parse().expect("op_class"),
                min_chan: cols[1].parse().expect("min_chan"),
                max_chan: cols[2].parse().expect("max_chan"),
                base_freq: cols[3].parse().expect("base_freq"),
                spacing: cols[4].parse().expect("spacing"),
                region,
            });
        }
        rows
    }

    #[test]
    fn table_equals_fixture() {
        let fixture = fixture_rows();
        assert_eq!(fixture.len(), 68);
        assert_eq!(FREQ_RANGES.len(), fixture.len());
        for (i, expected) in fixture.iter().enumerate() {
            assert_eq!(&FREQ_RANGES[i], expected, "row {i} differs");
        }
    }

    #[test]
    fn forward_every_row_min_and_max() {
        // Within one region (and among global rows) no op class appears twice, and global and
        // regional op classes do not overlap, so each row is the first match for its own class and region.
        for (i, r) in fixture_rows().iter().enumerate() {
            let base = u32::from(r.base_freq);
            let spacing = u32::from(r.spacing);
            let expected_min = base + u32::from(r.min_chan) * spacing;
            let expected_max = base + u32::from(r.max_chan) * spacing;
            assert_eq!(
                chan_to_freq(r.op_class, r.min_chan, r.region),
                Some(expected_min),
                "row {i} min"
            );
            assert_eq!(
                chan_to_freq(r.op_class, r.max_chan, r.region),
                Some(expected_max),
                "row {i} max"
            );
            assert_eq!(
                chan_to_freq(r.op_class, 0, r.region),
                Some(expected_min),
                "row {i} channel 0"
            );
        }
    }

    #[test]
    fn forward_literal_cases() {
        assert_eq!(chan_to_freq(83, 13, ""), Some(2472));
        assert_eq!(chan_to_freq(1, 0, "US"), Some(5180));
        assert_eq!(chan_to_freq(1, 0, "JP"), Some(5170));
        assert_eq!(chan_to_freq(136, 2, ""), Some(5937));
        assert_eq!(chan_to_freq(200, 1, ""), None);
        // Class 1 has regional rows only.
        assert_eq!(chan_to_freq(1, 36, ""), None);
        assert_eq!(chan_to_freq(81, 14, ""), None);
        assert_eq!(chan_to_freq(1, 36, "us"), None); // region match is case-sensitive
        assert_eq!(chan_to_freq(81, 1, "US"), Some(2412)); // global rows apply in every region
        assert_eq!(chan_to_freq(84, 4, ""), None); // below the row's first channel
    }

    #[test]
    fn reverse_literal_cases() {
        assert_eq!(freq_to_chan(2412, ""), Some((81, 1)));
        assert_eq!(freq_to_chan(2412, "US"), Some((12, 1)));
        assert_eq!(freq_to_chan(2412, "us"), Some((83, 1)));
        assert_eq!(freq_to_chan(2412, "XX"), Some((83, 1)));
        assert_eq!(freq_to_chan(5170, ""), None);
        assert_eq!(freq_to_chan(5170, "JP"), Some((1, 34)));
        assert_eq!(freq_to_chan(2413, ""), None);
        assert_eq!(freq_to_chan(u32::MAX, ""), None);
        assert_eq!(freq_to_chan(0, ""), None);
        assert_eq!(freq_to_chan(5937, ""), Some((136, 2)));
    }
}
