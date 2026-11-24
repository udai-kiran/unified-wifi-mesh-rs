// SPDX-License-Identifier: Apache-2.0

//! Coverage of the method map (A3, scopes node and list) and the upstream test map (A10, node
//! and list rows).
//!
//! The data files are in `crates/em-dm/tests/data`. `tools/check-test-map` checks, against the
//! list printed by `cargo test`, that every Rust test the test map names exists.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

const UPSTREAM_METHODS: &str = include_str!("../../../tests/data/dm_upstream_methods.tsv");
const METHOD_MAP: &str = include_str!("../../../tests/data/dm_method_map.tsv");
const TEST_MAP: &str = include_str!("../../../tests/data/upstream_test_map.tsv");
const LIST_METHODS: &str = include_str!("../../../tests/data/dm_list_methods.tsv");
const LIST_TRIAGE: &str = include_str!("../../../tests/data/list_test_triage.tsv");
const NODE_TRIAGE: &str = include_str!("../../../tests/data/node_test_triage.tsv");

/// The helpers outside `dm_easy_mesh_t` that the method map also covers (A3).
const HELPER_ROWS: [&str; 4] = [
    "dm_sta_t::decode_sta_capability@cpp:464",
    "dm_sta_t::copy_sta_info@cpp:704",
    "dm_sta_t::set_frame_body@cpp:671",
    "dm_bss_t::match_criteria@cpp:375",
];

/// Data rows of a TSV: comment lines and the column header are dropped. Every row is checked
/// to have `columns` fields.
fn rows(text: &str, columns: usize) -> Vec<Vec<&str>> {
    let mut lines = text.lines().filter(|line| !line.starts_with('#'));
    lines.next().expect("a column header");
    lines
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            assert_eq!(
                fields.len(),
                columns,
                "row with the wrong column count: {line}"
            );
            fields
        })
        .collect()
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn model_test_sources() -> String {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/model/tests");
    let mut all = String::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|extension| extension == "rs") {
            all.push_str(&fs::read_to_string(path).unwrap());
        }
    }
    all
}

// --- upstream method list ---

#[test]
fn the_upstream_method_list_states_its_row_count_and_has_unique_keys() {
    let stated: usize = UPSTREAM_METHODS
        .lines()
        .find_map(|line| line.strip_prefix("# Row count: "))
        .and_then(|rest| rest.split_whitespace().next())
        .expect("a '# Row count: N' comment")
        .parse()
        .unwrap();
    let methods = rows(UPSTREAM_METHODS, 7);
    assert_eq!(methods.len(), stated);
    let keys: BTreeSet<&str> = methods.iter().map(|row| row[0]).collect();
    assert_eq!(keys.len(), methods.len(), "duplicate upstream key");
}

#[test]
fn every_upstream_key_is_its_name_plus_the_definition_line() {
    for row in rows(UPSTREAM_METHODS, 7) {
        let (key, name, h_line, cpp_line) = (row[0], row[1], row[2], row[3]);
        let expected = if cpp_line.chars().all(|c| c.is_ascii_digit()) {
            format!("{name}@cpp:{cpp_line}")
        } else {
            format!("{name}@h:{h_line}")
        };
        assert_eq!(key, expected);
    }
}

#[test]
fn overloaded_names_have_one_row_per_definition() {
    let keys: BTreeSet<&str> = rows(UPSTREAM_METHODS, 7).iter().map(|row| row[0]).collect();
    let commit_config = keys
        .iter()
        .filter(|key| key.starts_with("commit_config@"))
        .count();
    assert_eq!(commit_config, 4, "A3: commit_config has 4 rows");
    let get_radio = keys
        .iter()
        .filter(|key| key.starts_with("get_radio@"))
        .count();
    assert_eq!(get_radio, 2, "A3: get_radio has 2 rows");
}

// --- method map ---

fn method_map() -> Vec<Vec<&'static str>> {
    rows(METHOD_MAP, 4)
}

#[test]
fn the_method_map_covers_every_upstream_row_with_no_duplicates_and_no_unknown_rows() {
    let upstream: BTreeSet<&str> = rows(UPSTREAM_METHODS, 7).iter().map(|row| row[0]).collect();
    let mut mapped: BTreeSet<&str> = BTreeSet::new();
    for row in method_map() {
        if row[1] != "node" {
            continue;
        }
        assert!(mapped.insert(row[0]), "duplicate method map row {}", row[0]);
    }
    let known: BTreeSet<&str> = upstream.iter().copied().chain(HELPER_ROWS).collect();
    let missing: Vec<_> = known.difference(&mapped).collect();
    let unknown: Vec<_> = mapped.difference(&known).collect();
    assert!(
        missing.is_empty(),
        "upstream rows without a map row: {missing:?}"
    );
    assert!(
        unknown.is_empty(),
        "map rows that are not upstream rows: {unknown:?}"
    );
}

#[test]
fn every_method_map_row_has_a_scope_and_a_disposition_of_the_two_kinds() {
    for row in method_map() {
        assert!(
            row[1] == "node" || row[1] == "list",
            "{}: scope {}",
            row[0],
            row[1]
        );
        let disposition = row[2];
        let reason = disposition.strip_prefix("not-ported:");
        if let Some(reason) = reason {
            assert!(!reason.trim().is_empty(), "{}: empty reason", row[0]);
        } else {
            assert!(
                disposition.starts_with("rust:"),
                "{}: {disposition}",
                row[0]
            );
        }
    }
}

/// A `rust:` row names a file and a function; the function is defined in that file. A cite in
/// a doc comment alone is not enough, so the check reads the source.
#[test]
fn every_rust_disposition_names_a_function_defined_in_its_file() {
    for row in method_map() {
        let Some(path_and_fn) = row[2].strip_prefix("rust:") else {
            continue;
        };
        let (path, owner_and_fn) = path_and_fn.split_once("::").expect("file::Type::fn");
        let function = owner_and_fn.rsplit("::").next().unwrap();
        let source = fs::read_to_string(workspace_root().join(path))
            .unwrap_or_else(|error| panic!("{}: {path}: {error}", row[0]));
        let needle = format!("fn {function}(");
        let generic = format!("fn {function}<");
        assert!(
            source.contains(&needle) || source.contains(&generic),
            "{}: no `{needle}` in {path}",
            row[0]
        );
    }
}

/// MODEL-OP and DERIVED-READ rows are `rust:` unless the reason cites the C6 count rule, a stub
/// or UNDEFINED body, or a decision (A3).
#[test]
fn model_op_and_derived_read_rows_are_ported_unless_the_reason_cites_a_rule() {
    const ALLOWED_REASONS: [&str; 9] = [
        "C6 count rule",
        "stub",
        "UNDEFINED",
        "0007 D",
        "0004 R",
        "plan A1",
        "plan C1",
        "(C4)",
        "decision",
    ];
    let classes: BTreeMap<&str, &str> = rows(UPSTREAM_METHODS, 7)
        .iter()
        .map(|row| (row[0], row[4]))
        .chain(
            rows(LIST_METHODS, 7)
                .iter()
                .map(|row| (row[0], row[5]))
                .collect::<Vec<_>>(),
        )
        .collect();
    for row in method_map() {
        let class = classes.get(row[0]).copied().unwrap_or("MODEL-OP");
        if class != "MODEL-OP" && class != "DERIVED-READ" {
            continue;
        }
        if let Some(reason) = row[2].strip_prefix("not-ported:") {
            assert!(
                ALLOWED_REASONS
                    .iter()
                    .any(|allowed| reason.contains(allowed) || row[3].contains(allowed)),
                "{} ({class}) is not ported without a cited rule: {reason}",
                row[0]
            );
        }
    }
}

#[test]
fn the_count_rows_cite_the_c6_count_rule() {
    const COUNT_NAMES: [&str; 8] = [
        "ap_mld",
        "assoc_sta_mld",
        "bss",
        "op_class",
        "radios",
        "network_ssid",
        "policy",
        "scan_results",
    ];
    let mut found = 0;
    for row in method_map() {
        let name = row[0].split('@').next().unwrap();
        let is_count = ["get_num_", "set_num_"].iter().any(|prefix| {
            name.strip_prefix(prefix)
                .is_some_and(|rest| COUNT_NAMES.contains(&rest))
        });
        if is_count {
            found += 1;
            assert!(
                row[2].contains("C6") || row[3].contains("C6"),
                "{} does not cite the C6 count rule",
                row[0]
            );
        }
    }
    assert!(
        found >= 20,
        "expected the get_num_* and set_num_* rows, found {found}"
    );
}

#[test]
fn the_rows_the_brief_names_are_not_ported_with_the_stated_kind_of_reason() {
    let map: BTreeMap<&str, (&str, &str)> = method_map()
        .iter()
        .map(|row| (row[0], (row[2], row[3])))
        .collect();
    for key in ["get_dpp@h:1751", "get_em@h:2649"] {
        assert!(map[key].0.starts_with("not-ported:"), "{key}");
    }
    for name in [
        "hex@cpp:1994",
        "unhex@cpp:2024",
        "securitymode_to_str@cpp:2104",
        "str_to_securitymode@cpp:2128",
    ] {
        assert!(map[name].0.starts_with("not-ported:"), "{name}");
    }
    for name in [
        "decode_config@cpp:853",
        "encode_config@cpp:447",
        "get_subdoc_vap_type_for_freq@h:2677",
        "get_interfaces_list@cpp:2219",
    ] {
        assert!(map[name].0.starts_with("not-ported:"), "{name}");
    }
    assert!(
        map["macbytes_to_string@cpp:2056"]
            .0
            .contains("MacAddr Display")
    );
    assert!(
        map["maclist_to_string@cpp:2084"]
            .0
            .contains("MacAddr Display")
    );
    assert!(
        map["string_to_macbytes@cpp:2070"]
            .0
            .contains("MacAddr FromStr")
    );
    assert!(
        map["update_bsta_mld_info@cpp:3822"]
            .0
            .starts_with("not-ported:")
    );
}

// --- upstream test map ---

fn test_map() -> Vec<Vec<&'static str>> {
    rows(TEST_MAP, 8)
}

#[test]
fn the_test_map_covers_the_node_triage_counts() {
    let map: Vec<_> = test_map()
        .into_iter()
        .filter(|row| row[2] == "node")
        .collect();
    let count = |verdict: &str| map.iter().filter(|row| row[3] == verdict).count();
    assert_eq!(count("PORT"), 173);
    assert_eq!(count("PORT-CTOR"), 5);
    assert_eq!(
        count("SKIP-TRIVIAL(ambiguous-check)"),
        1,
        "the one ambiguous row is decided"
    );
    assert_eq!(map.len(), 179);
    let keys: BTreeSet<(&str, &str)> = map.iter().map(|row| (row[0], row[1])).collect();
    assert_eq!(keys.len(), map.len(), "duplicate test row");
}

fn node_triage() -> Vec<Vec<&'static str>> {
    rows(NODE_TRIAGE, 4)
}

/// A10: the node rows of the test map are exactly the committed node triage: the same
/// `(test, line)` keys, and for each the same method and verdict. An invented or dropped test key
/// fails on the key sets; a changed verdict or method fails on the row.
#[test]
fn the_test_map_node_rows_match_the_committed_node_triage() {
    let triage = node_triage();
    let triage_rows: BTreeMap<(&str, &str), (&str, &str)> = triage
        .iter()
        .map(|row| ((row[0], row[1]), (row[2], row[3])))
        .collect();
    assert_eq!(triage_rows.len(), triage.len(), "duplicate triage row");
    assert_eq!(triage.len(), 179);

    let map: Vec<_> = test_map()
        .into_iter()
        .filter(|row| row[2] == "node")
        .collect();
    let map_rows: BTreeMap<(&str, &str), (&str, &str)> = map
        .iter()
        .map(|row| ((row[0], row[1]), (row[4], row[3])))
        .collect();
    assert_eq!(map_rows.len(), map.len(), "duplicate test map row");

    let invented: Vec<_> = map_rows
        .keys()
        .filter(|key| !triage_rows.contains_key(*key))
        .collect();
    assert!(
        invented.is_empty(),
        "map rows not in the triage: {invented:?}"
    );
    let dropped: Vec<_> = triage_rows
        .keys()
        .filter(|key| !map_rows.contains_key(*key))
        .collect();
    assert!(
        dropped.is_empty(),
        "triage rows not in the map: {dropped:?}"
    );
    for (key, expected) in &triage_rows {
        assert_eq!(map_rows[key], *expected, "{}: method and verdict", key.0);
    }
}

/// 28 triage rows have the method "(unmatched)" and were triaged by hand: 4 are SKIP and the
/// rest PORT (plan.md).
#[test]
fn the_hand_verdict_rows_are_28_with_4_skips() {
    let hand: Vec<_> = test_map()
        .into_iter()
        .filter(|row| row[2] == "node" && row[4] == "(unmatched)")
        .collect();
    assert_eq!(hand.len(), 28);
    let skipped: BTreeSet<&str> = hand
        .iter()
        .filter(|row| row[5].starts_with("skip:"))
        .map(|row| row[0])
        .collect();
    let expected: BTreeSet<&str> = [
        "ctor_name_resolution_success_if_mac_exists",
        "ctor_nullptr_network_reference_bug",
        "UpdateScanResults_negative_NullScanResult",
        "get_num_scan_results_null_map_crash",
    ]
    .into_iter()
    .collect();
    assert_eq!(skipped, expected);
}

#[test]
fn every_test_map_row_is_a_rust_test_or_an_allowed_skip_and_has_a_tag() {
    for row in test_map() {
        let (name, disposition, tag) = (row[0], row[5], row[6]);
        if let Some(reason) = disposition.strip_prefix("skip:") {
            assert!(
                ["host-dependent", "UB/null-deref", "stale API"]
                    .iter()
                    .any(|allowed| reason.starts_with(allowed)),
                "{name}: skip reason {reason}"
            );
        } else {
            let path = disposition
                .strip_prefix("rust:")
                .unwrap_or_else(|| panic!("{name}: {disposition}"));
            assert!(path.starts_with("model::tests::"), "{name}: {path}");
        }
        assert!(
            tag == "parity" || tag.starts_with("intentional-difference:"),
            "{name}: tag {tag}"
        );
        if tag.starts_with("intentional-difference:") {
            assert!(
                !row[7].trim().is_empty() || !disposition.starts_with("skip:"),
                "{name}"
            );
        }
    }
}

/// Each named Rust test is defined under `src/model/tests`; `tools/check-test-map` checks the
/// full list printed by `cargo test`.
#[test]
fn every_rust_test_in_the_test_map_is_defined_in_the_model_tests() {
    let sources = model_test_sources();
    for row in test_map() {
        let Some(path) = row[5].strip_prefix("rust:") else {
            continue;
        };
        let function = path.rsplit("::").next().unwrap();
        assert!(
            sources.contains(&format!("fn {function}()")),
            "{}: no test `{function}`",
            row[0]
        );
    }
}

/// An intentional difference is tagged in the map and in the doc comment of a Rust test.
#[test]
fn every_intentional_difference_tag_appears_in_a_rust_test_doc() {
    let sources = model_test_sources();
    let tags: BTreeSet<&str> = test_map()
        .iter()
        .filter_map(|row| row[6].strip_prefix("intentional-difference:"))
        .collect();
    assert!(!tags.is_empty());
    for tag in tags {
        assert!(
            sources.contains(&format!("intentional-difference:{tag}")),
            "no test doc mentions intentional-difference:{tag}"
        );
    }
}

// --- list rows (brief 3b) ---

fn list_methods() -> Vec<Vec<&'static str>> {
    rows(LIST_METHODS, 7)
}

#[test]
fn the_list_method_list_states_its_row_count_and_has_unique_keys() {
    let stated: usize = LIST_METHODS
        .lines()
        .find_map(|line| line.strip_prefix("# Row count: "))
        .and_then(|rest| rest.split_whitespace().next())
        .expect("a '# Row count: N' comment")
        .parse()
        .unwrap();
    let methods = list_methods();
    assert_eq!(methods.len(), stated);
    let keys: BTreeSet<&str> = methods.iter().map(|row| row[0]).collect();
    assert_eq!(keys.len(), methods.len(), "duplicate list key");
    let count_owned = |owner_name: &str| methods.iter().filter(|row| row[1] == owner_name).count();
    assert_eq!(
        count_owned("dm_easy_mesh_list_t"),
        62,
        "A3: the 62 dm_easy_mesh_list_t members"
    );
    for row in &methods {
        let (owner, name, h_line, cpp_line) = (row[1], row[2], row[3], row[4]);
        let expected = if cpp_line == "none" {
            format!("{owner}::{name}@h:{h_line}")
        } else {
            format!("{owner}::{name}@cpp:{cpp_line}")
        };
        assert_eq!(row[0], expected);
    }
}

/// A3: every row of the committed list is in the method map with scope `list`, no duplicates and
/// no unknown rows; together with the node test above, the map covers all upstream rows.
#[test]
fn the_method_map_covers_every_list_row_with_no_duplicates_and_no_unknown_rows() {
    let upstream: BTreeSet<&str> = list_methods().iter().map(|row| row[0]).collect();
    let mut mapped: BTreeSet<&str> = BTreeSet::new();
    for row in method_map() {
        if row[1] == "list" {
            assert!(mapped.insert(row[0]), "duplicate method map row {}", row[0]);
        }
    }
    let missing: Vec<_> = upstream.difference(&mapped).collect();
    let unknown: Vec<_> = mapped.difference(&upstream).collect();
    assert!(
        missing.is_empty(),
        "list rows without a map row: {missing:?}"
    );
    assert!(
        unknown.is_empty(),
        "map rows that are not list rows: {unknown:?}"
    );
}

#[test]
fn the_method_map_has_no_row_outside_the_node_and_list_lists() {
    let node: BTreeSet<&str> = rows(UPSTREAM_METHODS, 7)
        .iter()
        .map(|row| row[0])
        .chain(HELPER_ROWS)
        .collect();
    let list: BTreeSet<&str> = list_methods().iter().map(|row| row[0]).collect();
    let all = method_map();
    assert_eq!(all.len(), node.len() + list.len());
    for row in all {
        let known = if row[1] == "node" { &node } else { &list };
        assert!(known.contains(row[0]), "{} is not a {} row", row[0], row[1]);
    }
}

#[test]
fn the_list_not_ported_rows_are_the_ones_with_no_model_effect() {
    let not_ported: BTreeSet<&str> = method_map()
        .iter()
        .filter(|row| row[1] == "list" && row[2].starts_with("not-ported:"))
        .map(|row| row[0])
        .collect();
    let expected: BTreeSet<&str> = [
        "dm_easy_mesh_list_t::debug_probe@cpp:894",
        "dm_easy_mesh_list_t::dm_easy_mesh_list_t@cpp:1827",
        "dm_easy_mesh_list_t::~dm_easy_mesh_list_t@cpp:1832",
    ]
    .into_iter()
    .collect();
    assert_eq!(not_ported, expected);
}

fn triage() -> Vec<Vec<&'static str>> {
    rows(LIST_TRIAGE, 5)
}

#[test]
fn the_list_triage_has_155_tests_with_one_class_each() {
    let triage = triage();
    assert_eq!(triage.len(), 155);
    let keys: BTreeSet<(&str, &str)> = triage.iter().map(|row| (row[0], row[1])).collect();
    assert_eq!(keys.len(), 155, "duplicate triage row");
    let count = |class: &str| triage.iter().filter(|row| row[3] == class).count();
    assert_eq!(count("PORT"), 44);
    assert_eq!(count("PORT-REWRITE"), 52);
    assert_eq!(count("SKIP-UB"), 58);
    assert_eq!(count("SKIP-STALE-API"), 1);
    // Plan A10 rules: a SKIP-UB row is a null case; an empty-key case is PORT-REWRITE.
    for row in &triage {
        let name = row[0].to_lowercase();
        if row[3] == "SKIP-UB" {
            assert!(name.contains("null"), "{} is not a null case", row[0]);
        }
        let empty_key = ["emptykey", "empty_key", "empty_netid", "empty_string_key"]
            .iter()
            .any(|marker| name.contains(marker));
        if empty_key {
            assert_eq!(row[3], "PORT-REWRITE", "{}", row[0]);
        }
    }
}

/// A10: the test map covers the committed list triage: one `list` row per triage row, with the
/// triage class and method, a Rust test for every PORT and PORT-REWRITE row and a skip reason for
/// every SKIP row.
#[test]
fn the_test_map_covers_every_list_triage_row() {
    let map: Vec<_> = test_map()
        .into_iter()
        .filter(|row| row[2] == "list")
        .collect();
    let triage = triage();
    assert_eq!(map.len(), triage.len());
    for (mapped, tri) in map.iter().zip(triage.iter()) {
        assert_eq!((mapped[0], mapped[1]), (tri[0], tri[1]));
        assert_eq!(mapped[3], tri[3], "{}: class", tri[0]);
        assert_eq!(mapped[4], tri[2], "{}: method", tri[0]);
        let skipped = mapped[5].starts_with("skip:");
        assert_eq!(skipped, tri[3].starts_with("SKIP"), "{}", tri[0]);
        if skipped {
            assert_eq!(mapped[6], "parity", "{}", tri[0]);
        }
    }
}

/// The two SSID tests port as rewrites to the stored-once model and carry the tag (A10).
#[test]
fn the_two_ssid_tests_are_tagged_as_stored_once_rewrites() {
    for name in [
        "network_ssid_update_without_database_reaches_all_models",
        "network_ssid_propagation_requires_exact_ids",
    ] {
        let row = test_map()
            .into_iter()
            .find(|row| row[2] == "list" && row[0] == name)
            .unwrap();
        assert_eq!(row[6], "intentional-difference:u1-network-stored-once");
    }
}
