// SPDX-License-Identifier: Apache-2.0

//! Channel and op-class table of the derived reads `get_*_by_opclass` and
//! `get_*_by_bandwidth` (`src/dm/dm_easy_mesh.cpp:2576-2623`, `em_e4_table_t`, `inc/em_base.h:3361`).
//!
//! These are static functions upstream (`inc/dm_easy_mesh.h:130-214`); they read the table and no
//! model state, so they are free functions here.

use em_types::FreqBand;

/// One row of the table: C `em_e4_table_t` (`inc/em_base.h:3361-3369`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct E4Row {
    /// C `op_class`.
    pub op_class: u32,
    /// C `band`.
    pub band: FreqBand,
    /// C `channel_spacing`, in MHz.
    pub channel_spacing: u32,
    /// C `has_beaconchannel`: the listed channels are beacon (primary) channels, not centre
    /// channels.
    pub has_beacon_channel: bool,
    /// C `channels` (the first `num_channels` entries). Every value fits a byte.
    pub channels: &'static [u8],
}

/// The 27 rows at `src/dm/dm_easy_mesh.cpp:2577-2622`, in upstream order.
pub static E4_TABLE: &[E4Row] = &[
    E4Row {
        op_class: 81,
        band: FreqBand::BAND_2_4_GHZ,
        channel_spacing: 25,
        has_beacon_channel: true,
        channels: &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13],
    },
    E4Row {
        op_class: 82,
        band: FreqBand::BAND_2_4_GHZ,
        channel_spacing: 25,
        has_beacon_channel: true,
        channels: &[14],
    },
    E4Row {
        op_class: 83,
        band: FreqBand::BAND_2_4_GHZ,
        channel_spacing: 40,
        has_beacon_channel: true,
        channels: &[1, 2, 3, 4, 5, 6, 7, 8, 9],
    },
    E4Row {
        op_class: 84,
        band: FreqBand::BAND_2_4_GHZ,
        channel_spacing: 40,
        has_beacon_channel: true,
        channels: &[5, 6, 7, 8, 9, 10, 11, 12, 13],
    },
    E4Row {
        op_class: 115,
        band: FreqBand::BAND_5_GHZ,
        channel_spacing: 20,
        has_beacon_channel: true,
        channels: &[36, 40, 44, 48],
    },
    E4Row {
        op_class: 116,
        band: FreqBand::BAND_5_GHZ,
        channel_spacing: 40,
        has_beacon_channel: true,
        channels: &[36, 44],
    },
    E4Row {
        op_class: 117,
        band: FreqBand::BAND_5_GHZ,
        channel_spacing: 40,
        has_beacon_channel: true,
        channels: &[40, 48],
    },
    E4Row {
        op_class: 118,
        band: FreqBand::BAND_5_GHZ,
        channel_spacing: 20,
        has_beacon_channel: true,
        channels: &[52, 56, 60, 64],
    },
    E4Row {
        op_class: 119,
        band: FreqBand::BAND_5_GHZ,
        channel_spacing: 40,
        has_beacon_channel: true,
        channels: &[52, 60],
    },
    E4Row {
        op_class: 120,
        band: FreqBand::BAND_5_GHZ,
        channel_spacing: 40,
        has_beacon_channel: true,
        channels: &[56, 64],
    },
    E4Row {
        op_class: 121,
        band: FreqBand::BAND_5_GHZ,
        channel_spacing: 20,
        has_beacon_channel: true,
        channels: &[100, 104, 108, 112, 116, 120, 124, 128, 132, 136, 140, 144],
    },
    E4Row {
        op_class: 122,
        band: FreqBand::BAND_5_GHZ,
        channel_spacing: 40,
        has_beacon_channel: true,
        channels: &[100, 108, 116, 124, 132, 140],
    },
    E4Row {
        op_class: 123,
        band: FreqBand::BAND_5_GHZ,
        channel_spacing: 40,
        has_beacon_channel: true,
        channels: &[104, 112, 120, 128, 136, 144],
    },
    E4Row {
        op_class: 124,
        band: FreqBand::BAND_5_GHZ,
        channel_spacing: 20,
        has_beacon_channel: true,
        channels: &[149, 153, 157, 161],
    },
    E4Row {
        op_class: 125,
        band: FreqBand::BAND_5_GHZ,
        channel_spacing: 20,
        has_beacon_channel: true,
        channels: &[149, 153, 157, 161, 165, 169, 173, 177],
    },
    E4Row {
        op_class: 126,
        band: FreqBand::BAND_5_GHZ,
        channel_spacing: 40,
        has_beacon_channel: true,
        channels: &[149, 157, 165, 173],
    },
    E4Row {
        op_class: 127,
        band: FreqBand::BAND_5_GHZ,
        channel_spacing: 40,
        has_beacon_channel: true,
        channels: &[153, 161, 169, 177],
    },
    E4Row {
        op_class: 128,
        band: FreqBand::BAND_5_GHZ,
        channel_spacing: 80,
        has_beacon_channel: false,
        channels: &[42, 58, 106, 122, 138, 155, 171],
    },
    E4Row {
        op_class: 129,
        band: FreqBand::BAND_5_GHZ,
        channel_spacing: 160,
        has_beacon_channel: false,
        channels: &[50, 114, 163],
    },
    E4Row {
        op_class: 130,
        band: FreqBand::BAND_5_GHZ,
        channel_spacing: 80,
        has_beacon_channel: false,
        channels: &[42, 58, 106, 122, 138, 155, 171],
    },
    E4Row {
        op_class: 131,
        band: FreqBand::BAND_6_GHZ,
        channel_spacing: 20,
        has_beacon_channel: true,
        channels: &[
            1, 5, 9, 13, 17, 21, 25, 29, 33, 37, 41, 45, 49, 53, 57, 61, 65, 69, 73, 77, 81, 85,
            89, 93, 97, 101, 105, 109, 113, 117, 121, 125, 129, 133, 137, 141, 145, 149, 153, 157,
            161, 165, 169, 173, 177, 181, 185, 189, 193, 197, 201, 205, 209, 213, 217, 221, 225,
            229, 233,
        ],
    },
    E4Row {
        op_class: 132,
        band: FreqBand::BAND_6_GHZ,
        channel_spacing: 40,
        has_beacon_channel: false,
        channels: &[
            3, 11, 19, 27, 35, 43, 51, 59, 67, 75, 83, 91, 99, 107, 115, 123, 131, 139, 147, 155,
            163, 171, 179, 187, 195, 203, 211, 219, 227,
        ],
    },
    E4Row {
        op_class: 133,
        band: FreqBand::BAND_6_GHZ,
        channel_spacing: 80,
        has_beacon_channel: false,
        channels: &[
            7, 23, 39, 55, 71, 87, 103, 119, 135, 151, 167, 183, 199, 215,
        ],
    },
    E4Row {
        op_class: 134,
        band: FreqBand::BAND_6_GHZ,
        channel_spacing: 160,
        has_beacon_channel: false,
        channels: &[15, 47, 79, 111, 143, 175, 207],
    },
    E4Row {
        op_class: 135,
        band: FreqBand::BAND_6_GHZ,
        channel_spacing: 80,
        has_beacon_channel: false,
        channels: &[
            7, 23, 39, 55, 71, 87, 103, 119, 135, 151, 167, 183, 199, 215,
        ],
    },
    E4Row {
        op_class: 136,
        band: FreqBand::BAND_6_GHZ,
        channel_spacing: 20,
        has_beacon_channel: false,
        channels: &[2],
    },
    E4Row {
        op_class: 137,
        band: FreqBand::BAND_6_GHZ,
        channel_spacing: 320,
        has_beacon_channel: false,
        channels: &[31, 63, 95, 127, 159, 191],
    },
];

fn row_for(op_class: u32) -> Option<&'static E4Row> {
    E4_TABLE.iter().find(|row| row.op_class == op_class)
}

/// Beacon channel from a centre channel and bandwidth in MHz
/// (`dm_easy_mesh.cpp:2626-2642`). An unsupported bandwidth returns the channel unchanged.
#[must_use]
pub fn beacon_channel_by_bandwidth(center_channel: i32, bandwidth: u32) -> i32 {
    match bandwidth {
        40 => center_channel - 2,
        80 => center_channel - 6,
        160 => center_channel - 14,
        320 => center_channel - 30,
        _ => center_channel,
    }
}

/// Centre channel from a beacon channel and bandwidth in MHz (`dm_easy_mesh.cpp:2644-2660`).
/// An unsupported bandwidth returns the channel unchanged.
#[must_use]
pub fn center_channel_by_bandwidth(beacon_channel: i32, bandwidth: u32) -> i32 {
    match bandwidth {
        40 => beacon_channel + 2,
        80 => beacon_channel + 6,
        160 => beacon_channel + 14,
        320 => beacon_channel + 30,
        _ => beacon_channel,
    }
}

/// Centre channel for an op class and channel (`dm_easy_mesh.cpp:2662-2680`): a row that lists
/// beacon channels, or an unknown op class, returns `channel` unchanged.
#[must_use]
pub fn center_channel_by_op_class(op_class: u32, channel: i32) -> i32 {
    match row_for(op_class) {
        Some(row) if !row.has_beacon_channel => {
            center_channel_by_bandwidth(channel, row.channel_spacing)
        }
        _ => channel,
    }
}

/// Beacon channel for an op class and channel (`dm_easy_mesh.cpp:2682-2702`): a row that lists
/// beacon channels, or an unknown op class, returns `channel` unchanged.
#[must_use]
pub fn beacon_channel_by_op_class(op_class: u32, channel: i32) -> i32 {
    match row_for(op_class) {
        Some(row) if !row.has_beacon_channel => {
            beacon_channel_by_bandwidth(channel, row.channel_spacing)
        }
        _ => channel,
    }
}

/// Band of an op class (`dm_easy_mesh.cpp:2704-2714`). Upstream returns `em_freq_band_unknown`
/// for an unknown op class; the port returns `None`.
#[must_use]
pub fn freq_band_by_op_class(op_class: u32) -> Option<FreqBand> {
    row_for(op_class).map(|row| row.band)
}

/// Channels of an op class (`dm_easy_mesh.cpp:2716-2729`); empty for an unknown op class.
#[must_use]
pub fn channel_list_by_op_class(op_class: u32) -> &'static [u8] {
    row_for(op_class).map_or(&[], |row| row.channels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_the_upstream_row_count_and_channel_counts() {
        assert_eq!(E4_TABLE.len(), 27);
        let rows_6ghz = E4_TABLE.iter().find(|r| r.op_class == 131).unwrap();
        assert_eq!(rows_6ghz.channels.len(), 59);
    }

    #[test]
    fn bandwidth_offsets_round_trip() {
        for bandwidth in [20, 40, 80, 160, 320] {
            let beacon = beacon_channel_by_bandwidth(100, bandwidth);
            assert_eq!(center_channel_by_bandwidth(beacon, bandwidth), 100);
        }
        assert_eq!(
            beacon_channel_by_bandwidth(100, 7),
            100,
            "unsupported bandwidth"
        );
        assert_eq!(
            center_channel_by_bandwidth(100, 7),
            100,
            "unsupported bandwidth"
        );
    }

    #[test]
    fn op_class_channel_conversion_uses_the_row_kind() {
        // 128 lists centre channels (80 MHz): beacon = centre - 6.
        assert_eq!(beacon_channel_by_op_class(128, 42), 36);
        assert_eq!(center_channel_by_op_class(128, 36), 42);
        // 115 lists beacon channels: unchanged.
        assert_eq!(beacon_channel_by_op_class(115, 36), 36);
        assert_eq!(center_channel_by_op_class(115, 36), 36);
        // Unknown op class: unchanged.
        assert_eq!(beacon_channel_by_op_class(1, 36), 36);
        assert_eq!(center_channel_by_op_class(1, 36), 36);
    }

    #[test]
    fn band_and_channels_by_op_class() {
        assert_eq!(freq_band_by_op_class(81), Some(FreqBand::BAND_2_4_GHZ));
        assert_eq!(freq_band_by_op_class(115), Some(FreqBand::BAND_5_GHZ));
        assert_eq!(freq_band_by_op_class(137), Some(FreqBand::BAND_6_GHZ));
        assert_eq!(freq_band_by_op_class(0), None);
        assert_eq!(channel_list_by_op_class(82), &[14]);
        assert_eq!(channel_list_by_op_class(0), &[] as &[u8]);
    }
}
