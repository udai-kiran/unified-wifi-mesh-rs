// SPDX-License-Identifier: Apache-2.0

//! Spec minimum lengths that replace upstream's where upstream is stricter (note 0008 D8, 0005 D3).
//!
//! Upstream's tables in [`crate::tables`] are kept verbatim. For the rows listed
//! here, validation uses the lower spec minimum instead, so a TLV the spec
//! allows is not reported as too short.
//!
//! Each zero case is either "explicit" (the spec text states that the empty
//! case is allowed) or "inferred" (a count with no stated minimum is taken to
//! allow 0).

use em_types::TlvType;

use crate::tables::{Row, Table};

/// A spec minimum length for one mandatory row of an upstream table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpecMinLen {
    /// `Table::upstream_name` of the table.
    pub table: &'static str,
    /// 0-based index of the row in `Table::rows`.
    pub row: usize,
    /// TLV type of the row.
    pub tlv_type: TlvType,
    /// Minimum length upstream sets for the row. Must equal the row's `min_len`.
    pub upstream_min_len: u16,
    /// Spec minimum length, counting the 3-byte TLV header.
    pub min_len: u16,
    /// Specification section the minimum comes from.
    pub spec_ref: &'static str,
}

/// The 25 rows where upstream's minimum is above the spec minimum.
pub const SPEC_MIN_LENS: &[SpecMinLen] = &[
    // 3 + 1 (Num_Agent = 0). Inferred.
    SpecMinLen {
        table: "agent_list",
        row: 0,
        tlv_type: TlvType::AGENT_LIST,
        upstream_min_len: 12,
        min_len: 4,
        spec_ref: "EasyMesh §17.2.77",
    },
    // 3 + 1 (Num_OpClass = 0). Inferred.
    SpecMinLen {
        table: "anticipated_channel_pref",
        row: 0,
        tlv_type: TlvType::ANTICIPATED_CHANNEL_PREFERENCE,
        upstream_min_len: 8,
        min_len: 4,
        spec_ref: "EasyMesh §17.2.87",
    },
    // 3 + 1 (OpClass) + 1 (Channel) + 6 (Reference BSSID) + 1 (Num_Usage = 0). Inferred.
    SpecMinLen {
        table: "anticipated_channel_usage_rprt",
        row: 0,
        tlv_type: TlvType::ANTICIPATED_CHANNEL_USAGE,
        upstream_min_len: 41,
        min_len: 12,
        spec_ref: "EasyMesh §17.2.88",
    },
    // 3 + 6 (RUID) + 1 (Max_BSS) + 1 (Num_OpClass = 0). Inferred.
    SpecMinLen {
        table: "ap_cap_rprt",
        row: 1,
        tlv_type: TlvType::AP_RADIO_BASIC_CAPABILITIES,
        upstream_min_len: 15,
        min_len: 11,
        spec_ref: "EasyMesh §17.2.7",
    },
    // 3 + 1 (Num_Radio = 0). Inferred.
    SpecMinLen {
        table: "ap_cap_rprt",
        row: 6,
        tlv_type: TlvType::CHANNEL_SCAN_CAPABILITIES,
        upstream_min_len: 17,
        min_len: 4,
        spec_ref: "EasyMesh §17.2.38",
    },
    // 3 + 1 (Num_BSSID = 0). Inferred.
    SpecMinLen {
        table: "ap_metrics_query",
        row: 0,
        tlv_type: TlvType::AP_METRICS_QUERY,
        upstream_min_len: 10,
        min_len: 4,
        spec_ref: "EasyMesh §17.2.21",
    },
    // 3 + 1 (Num_BSSID = 0). Inferred.
    SpecMinLen {
        table: "assoc_status_notif",
        row: 0,
        tlv_type: TlvType::ASSOCIATION_STATUS_NOTIFICATION,
        upstream_min_len: 11,
        min_len: 4,
        spec_ref: "EasyMesh §17.2.53",
    },
    // 3 + 6 + 1 + 1 + 6 + 1 (fields before SSID Length) + 1 (SSID Length = 0)
    // + 1 (Num_Report = 0) + 1 (Num_Ids = 0). Explicit.
    SpecMinLen {
        table: "beacon_metrics_query",
        row: 0,
        tlv_type: TlvType::BEACON_METRICS_QUERY,
        upstream_min_len: 23,
        min_len: 21,
        spec_ref: "EasyMesh §17.2.27",
    },
    // 3 + 1 (Num_Service = 0). Inferred.
    SpecMinLen {
        table: "bss_config_req",
        row: 1,
        tlv_type: TlvType::SUPPORTED_SERVICE,
        upstream_min_len: 5,
        min_len: 4,
        spec_ref: "EasyMesh §17.2.1",
    },
    // 3 + 6 (RUID) + 1 (Max_BSS) + 1 (Num_OpClass = 0). Inferred.
    SpecMinLen {
        table: "bss_config_req",
        row: 2,
        tlv_type: TlvType::AP_RADIO_BASIC_CAPABILITIES,
        upstream_min_len: 15,
        min_len: 11,
        spec_ref: "EasyMesh §17.2.7",
    },
    // 3 + 1 (Num_Radio = 0). Inferred.
    SpecMinLen {
        table: "bss_config_res",
        row: 0,
        tlv_type: TlvType::BSS_CONFIGURATION_REPORT,
        upstream_min_len: 17,
        min_len: 4,
        spec_ref: "EasyMesh §17.2.75",
    },
    // 3 + 1 (Num_Radio = 0). Inferred.
    SpecMinLen {
        table: "cac_req",
        row: 0,
        tlv_type: TlvType::CAC_REQUEST,
        upstream_min_len: 12,
        min_len: 4,
        spec_ref: "EasyMesh §17.2.42",
    },
    // 3 + 1 + 1 + 1 (three counts = 0). Inferred.
    SpecMinLen {
        table: "channel_pref_rprt",
        row: 3,
        tlv_type: TlvType::CAC_STATUS_REPORT,
        upstream_min_len: 19,
        min_len: 6,
        spec_ref: "EasyMesh §17.2.45",
    },
    // 3 + 1 (flags) + 1 (Num_Radio = 0). Inferred.
    SpecMinLen {
        table: "channel_scan_req",
        row: 0,
        tlv_type: TlvType::CHANNEL_SCAN_REQUEST,
        upstream_min_len: 13,
        min_len: 5,
        spec_ref: "EasyMesh §17.2.39",
    },
    // 3 + 6 (RUID) + 1 + 1 + 1 (Scan Status != 0x00: the later fields are absent).
    // Explicit. If AggregateScanDuration (4) and Scan Type (1) were always present,
    // the minimum would be 17, still below 32.
    SpecMinLen {
        table: "channel_scan_rprt",
        row: 1,
        tlv_type: TlvType::CHANNEL_SCAN_RESULT,
        upstream_min_len: 32,
        min_len: 12,
        spec_ref: "EasyMesh §17.2.40",
    },
    // 3 + 6 + 1 + 2 + 1 + 3 (EstServiceParametersBE, IncludeBE shall be 1). Explicit.
    SpecMinLen {
        table: "combined_infra_metrics",
        row: 0,
        tlv_type: TlvType::AP_METRICS,
        upstream_min_len: 24,
        min_len: 16,
        spec_ref: "EasyMesh §17.2.22",
    },
    // 3 + 12 + 29 × 1 (at least one neighbour entry, n >= 1). Explicit.
    SpecMinLen {
        table: "combined_infra_metrics",
        row: 1,
        tlv_type: TlvType::TRANSMITTER_LINK_METRIC,
        upstream_min_len: 50,
        min_len: 44,
        spec_ref: "IEEE 1905.1 §6.4.11",
    },
    // 3 + 12 + 29 × 1 (at least one neighbour entry, n >= 1). Explicit.
    SpecMinLen {
        table: "combined_infra_metrics",
        row: 2,
        tlv_type: TlvType::TRANSMITTER_LINK_METRIC,
        upstream_min_len: 50,
        min_len: 44,
        spec_ref: "IEEE 1905.1 §6.4.11",
    },
    // 3 + 6 (RUID) + 1 (Max_BSS) + 1 (Num_OpClass = 0). Inferred.
    SpecMinLen {
        table: "early_ap_cap_rprt",
        row: 1,
        tlv_type: TlvType::AP_RADIO_BASIC_CAPABILITIES,
        upstream_min_len: 15,
        min_len: 11,
        spec_ref: "EasyMesh §17.2.7",
    },
    // 3 + 6 (RUID) + 1 (Num_OpClass = 0) + 1 (TxPower). Inferred.
    SpecMinLen {
        table: "op_channel_rprt",
        row: 0,
        tlv_type: TlvType::OPERATING_CHANNEL_REPORT,
        upstream_min_len: 13,
        min_len: 11,
        spec_ref: "EasyMesh §17.2.17",
    },
    // 3 + 6 (STA MAC) + 1 (Num_BSSID = 0). Explicit.
    SpecMinLen {
        table: "sta_link_metrics_rsp",
        row: 0,
        tlv_type: TlvType::ASSOCIATED_STA_LINK_METRICS,
        upstream_min_len: 29,
        min_len: 10,
        spec_ref: "EasyMesh §17.2.24",
    },
    // 3 + 6 + 1 (Num_BSSID = 0). Explicit.
    SpecMinLen {
        table: "sta_link_metrics_rsp",
        row: 2,
        tlv_type: TlvType::ASSOCIATED_STA_EXTENDED_LINK_METRICS,
        upstream_min_len: 32,
        min_len: 10,
        spec_ref: "EasyMesh §17.2.62",
    },
    // 3 + 1 (Num_Radio = 0). Inferred.
    SpecMinLen {
        table: "topo_resp",
        row: 1,
        tlv_type: TlvType::AP_OPERATIONAL_BSS,
        upstream_min_len: 18,
        min_len: 4,
        spec_ref: "EasyMesh §17.2.4",
    },
    // 3 + 1 (Num_Radio = 0). Inferred.
    SpecMinLen {
        table: "topo_resp",
        row: 5,
        tlv_type: TlvType::BSS_CONFIGURATION_REPORT,
        upstream_min_len: 17,
        min_len: 4,
        spec_ref: "EasyMesh §17.2.75",
    },
    // 3 + 1 (OpClass) + 1 (Num_Chan = 0). Inferred.
    SpecMinLen {
        table: "unassoc_sta_link_metrics_query",
        row: 0,
        tlv_type: TlvType::UNASSOCIATED_STA_LINK_METRICS_QUERY,
        upstream_min_len: 13,
        min_len: 5,
        spec_ref: "EasyMesh §17.2.25",
    },
];

/// The minimum length to check for row `index` of `table`.
///
/// The spec minimum from [`SPEC_MIN_LENS`] if the row is listed there, else
/// upstream's `row.min_len`. `row` must be `table.rows[index]`. The result is
/// never above `row.min_len` (lower only).
#[must_use]
pub(crate) fn min_len(table: &Table, index: usize, row: &Row) -> u16 {
    for entry in SPEC_MIN_LENS {
        if entry.table == table.upstream_name && entry.row == index {
            return entry.min_len.min(row.min_len);
        }
    }
    row.min_len
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables::{Requirement, TABLES};
    use em_types::ProfileType;

    fn upstream_table(name: &str) -> &'static Table {
        TABLES
            .iter()
            .find(|table| table.upstream_name == name)
            .unwrap_or_else(|| panic!("no upstream table named {name}"))
    }

    /// Removing an entry silently restores upstream's stricter minimum, so the
    /// count is pinned (note 0005 D3: 25 rows).
    #[test]
    fn the_list_has_25_entries() {
        assert_eq!(SPEC_MIN_LENS.len(), 25);
    }

    /// The spec minimums, written out independently of the entries above so
    /// that a changed value fails here (arithmetic in note 0005 D3 data and
    /// docs/tlv-spec-audit.md).
    const EXPECTED: [(&str, usize, u16); 25] = [
        ("agent_list", 0, 4),
        ("anticipated_channel_pref", 0, 4),
        ("anticipated_channel_usage_rprt", 0, 12),
        ("ap_cap_rprt", 1, 11),
        ("ap_cap_rprt", 6, 4),
        ("ap_metrics_query", 0, 4),
        ("assoc_status_notif", 0, 4),
        ("beacon_metrics_query", 0, 21),
        ("bss_config_req", 1, 4),
        ("bss_config_req", 2, 11),
        ("bss_config_res", 0, 4),
        ("cac_req", 0, 4),
        ("channel_pref_rprt", 3, 6),
        ("channel_scan_req", 0, 5),
        ("channel_scan_rprt", 1, 12),
        ("combined_infra_metrics", 0, 16),
        ("combined_infra_metrics", 1, 44),
        ("combined_infra_metrics", 2, 44),
        ("early_ap_cap_rprt", 1, 11),
        ("op_channel_rprt", 0, 11),
        ("sta_link_metrics_rsp", 0, 10),
        ("sta_link_metrics_rsp", 2, 10),
        ("topo_resp", 1, 4),
        ("topo_resp", 5, 4),
        ("unassoc_sta_link_metrics_query", 0, 5),
    ];

    #[test]
    fn every_entry_has_its_expected_spec_minimum() {
        for (table, row, expected) in EXPECTED {
            let entry = SPEC_MIN_LENS
                .iter()
                .find(|entry| entry.table == table && entry.row == row)
                .unwrap_or_else(|| panic!("{table} row {row} is not listed"));
            assert_eq!(entry.min_len, expected, "{table} row {row}");
            let upstream = upstream_table(table);
            assert_eq!(
                min_len(upstream, row, &upstream.rows[row]),
                expected,
                "{table} row {row}"
            );
        }
    }

    /// Every row not listed keeps upstream's minimum, so an override cannot leak
    /// to another row with the same TLV type.
    #[test]
    fn every_unlisted_row_keeps_upstream_minimum() {
        let mut lowered = 0;
        for table in TABLES {
            for (index, row) in table.rows.iter().enumerate() {
                let listed = SPEC_MIN_LENS
                    .iter()
                    .any(|entry| entry.table == table.upstream_name && entry.row == index);
                if listed {
                    lowered += 1;
                } else {
                    assert_eq!(
                        min_len(table, index, row),
                        row.min_len,
                        "{} row {index}",
                        table.upstream_name
                    );
                }
            }
        }
        assert_eq!(lowered, SPEC_MIN_LENS.len());
    }

    #[test]
    fn min_len_never_raises_the_row_minimum() {
        let topo = upstream_table("topo_resp");
        let mut row = topo.rows[1];
        row.min_len = 3;
        assert_eq!(min_len(topo, 1, &row), 3);
    }

    #[test]
    fn every_entry_names_an_upstream_row_with_the_same_type_and_minimum() {
        for entry in SPEC_MIN_LENS {
            let table = upstream_table(entry.table);
            let row = table
                .rows
                .get(entry.row)
                .unwrap_or_else(|| panic!("{} has no row {}", entry.table, entry.row));
            assert_eq!(
                row.tlv_type, entry.tlv_type,
                "{} row {}",
                entry.table, entry.row
            );
            assert_eq!(
                row.min_len, entry.upstream_min_len,
                "{} row {}",
                entry.table, entry.row
            );
        }
    }

    #[test]
    fn every_entry_lowers_the_minimum_and_keeps_the_header() {
        for entry in SPEC_MIN_LENS {
            assert!(
                entry.min_len < entry.upstream_min_len,
                "{} row {}",
                entry.table,
                entry.row
            );
            assert!(entry.min_len >= 3, "{} row {}", entry.table, entry.row);
        }
    }

    #[test]
    fn no_row_is_listed_twice() {
        for (i, first) in SPEC_MIN_LENS.iter().enumerate() {
            for second in &SPEC_MIN_LENS[i + 1..] {
                assert!(
                    first.table != second.table || first.row != second.row,
                    "{} row {} listed twice",
                    first.table,
                    first.row
                );
            }
        }
    }

    #[test]
    fn every_entry_row_is_mandatory_for_some_profile() {
        for entry in SPEC_MIN_LENS {
            let row = upstream_table(entry.table).rows[entry.row];
            let mandatory_somewhere = (0..=4)
                .any(|profile| row.rule.resolve(ProfileType(profile)) == Requirement::Mandatory);
            assert!(
                mandatory_somewhere,
                "{} row {} is never mandatory",
                entry.table, entry.row
            );
        }
    }

    #[test]
    fn min_len_uses_the_override_only_for_listed_rows() {
        let topo = upstream_table("topo_resp");
        // Row 1 is listed (AP_OPERATIONAL_BSS, 18 -> 4).
        assert_eq!(min_len(topo, 1, &topo.rows[1]), 4);
        // Row 0 (DEVICE_INFORMATION) is not listed and keeps upstream's 19.
        assert_eq!(min_len(topo, 0, &topo.rows[0]), 19);
    }
}
