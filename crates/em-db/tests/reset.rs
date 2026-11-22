// SPDX-License-Identifier: Apache-2.0

//! Reset-loader integration tests and the pinned upstream fixture.

use std::path::Path;

use em_db::{FieldProblem, ResetError, load_reset_file, load_reset_str};
use em_dm::{CStrBytes, HaulType, MacAddr, StringError};

// Exact copy of install/bin/Reset.json at rdkcentral/unified-wifi-mesh@ba4180b.
const UPSTREAM_RESET: &str = include_str!("data/Reset.json");

fn document(rows: &str) -> String {
    format!(r#"{{"wfa-dataelements:Reset":{{"NetworkSSIDList":[{rows}]}}}}"#)
}

#[test]
fn pinned_upstream_reset_loads_every_expected_field_in_file_order() {
    let rows = load_reset_str(UPSTREAM_RESET).unwrap();
    assert_eq!(rows.len(), 5);
    let expected = [
        (
            "private_ssid",
            "test-fronthaul",
            "Fronthaul",
            HaulType::FRONTHAUL,
            12,
        ),
        ("iot_ssid", "test-backhaul", "IoT", HaulType::IOT, 14),
        (
            "lnf_radius",
            "test-backhaul",
            "Configurator",
            HaulType::CONFIGURATOR,
            15,
        ),
        (
            "mesh_backhaul",
            "test-backhaul",
            "Backhaul",
            HaulType::BACKHAUL,
            13,
        ),
        ("hotspot", "test-hotspot", "Hotspot", HaulType::HOTSPOT, 16),
    ];
    for (index, (row, (ssid, password, name, haul, vlan))) in rows.iter().zip(expected).enumerate()
    {
        assert_eq!(row.id.as_bytes(), format!("{name}@OneWifiMesh").as_bytes());
        assert_eq!(row.ssid.as_bytes(), ssid.as_bytes());
        assert_eq!(row.pass_phrase.expose().as_bytes(), password.as_bytes());
        assert_eq!(
            row.band
                .as_slice()
                .iter()
                .map(CStrBytes::as_bytes)
                .collect::<Vec<_>>(),
            [b"2.4".as_slice(), b"5".as_slice(), b"6".as_slice()]
        );
        assert!(row.enable);
        let expected_akms: &[&[u8]] = if index == 0 {
            &[b"dpp"]
        } else {
            &[b"dpp", b"sae", b"SuiteSelector"]
        };
        assert_eq!(
            row.akm
                .as_slice()
                .iter()
                .map(CStrBytes::as_bytes)
                .collect::<Vec<_>>(),
            expected_akms
        );
        assert_eq!(
            row.suite_select.as_bytes(),
            if index == 0 {
                &b""[..]
            } else {
                &b"00010203"[..]
            }
        );
        assert!(row.advertisement);
        assert_eq!(
            row.mfp.as_bytes(),
            if index == 0 {
                &b"Optional"[..]
            } else {
                &b"Required"[..]
            }
        );
        assert_eq!(row.mobility_domain, MacAddr::new([0, 1, 2, 3, 4, 5]));
        assert_eq!(row.haul_type.as_slice(), &[haul]);
        assert_eq!(row.auth_type.as_bytes(), b"WPA3 Personal");
        assert_eq!(row.vlan_id, vlan);
    }
}

#[test]
fn path_loader_reads_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/Reset.json");
    let rows = load_reset_file(path).unwrap();
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[3].ssid.as_bytes(), b"mesh_backhaul");
}

#[test]
fn initial_utf8_bom_is_accepted() {
    let input = format!("\u{feff}{}", document(r#"{"SSID":"with-bom"}"#));
    let rows = load_reset_str(&input).unwrap();
    assert_eq!(rows[0].ssid.as_bytes(), b"with-bom");
}

#[test]
fn passphrase_spelling_and_object_keys_are_case_insensitive_and_first_wins() {
    let json = r#"{"WFA-DATAELEMENTS:RESET":{"networkssidlist":[
        {"PassPhrase":"first", "Passphrase":"second", "SSID":"one", "ssid":"two"},
        {"Passphrase":"lower-p"}, {"pASsPHRASE":"mixed"},
        {"PassPhrase":"original", "PassPhrase":"duplicate"}
    ]}}"#;
    let rows = load_reset_str(json).unwrap();
    let values = rows
        .iter()
        .map(|row| row.pass_phrase.expose().as_bytes())
        .collect::<Vec<_>>();
    assert_eq!(
        values,
        [b"first".as_slice(), b"lower-p", b"mixed", b"original"]
    );
    assert_eq!(rows[0].ssid.as_bytes(), b"one");
}

#[test]
fn lists_retain_order_and_upstream_haul_prefix_rules() {
    let input = document(
        r#"{"Band":["6","2.4","5"],"AKMsAllowed":["sae","dpp","sae"],
        "HaulType":["F","Back","Io","Config","Hot","", "unknown", "fronthaul"]}"#,
    );
    let rows = load_reset_str(&input).unwrap();
    let row = &rows[0];
    assert_eq!(
        row.band
            .as_slice()
            .iter()
            .map(CStrBytes::as_bytes)
            .collect::<Vec<_>>(),
        [b"6".as_slice(), b"2.4", b"5"]
    );
    assert_eq!(
        row.akm
            .as_slice()
            .iter()
            .map(CStrBytes::as_bytes)
            .collect::<Vec<_>>(),
        [b"sae".as_slice(), b"dpp", b"sae"]
    );
    assert_eq!(
        row.haul_type.as_slice(),
        &[
            HaulType::FRONTHAUL,
            HaulType::BACKHAUL,
            HaulType::IOT,
            HaulType::CONFIGURATOR,
            HaulType::HOTSPOT,
            HaulType::FRONTHAUL,
            HaulType(5),
            HaulType(5)
        ]
    );
    assert_eq!(row.id.as_bytes(), b"Fronthaul@OneWifiMesh");
}

#[test]
fn zero_defaults_and_unknown_haul_match_the_zeroed_upstream_record() {
    let input = document(r#"{}, {"HaulType":[]}, {"HaulType":["unknown"]}"#);
    let rows = load_reset_str(&input).unwrap();
    for row in &rows[..2] {
        assert_eq!(row.id.as_bytes(), b"Fronthaul@OneWifiMesh");
        assert!(row.ssid.is_empty());
        assert!(row.pass_phrase.expose().is_empty());
        assert!(row.band.is_empty());
        assert!(row.akm.is_empty());
        assert!(row.haul_type.is_empty());
        assert!(!row.enable);
        assert!(!row.advertisement);
        assert_eq!(row.mobility_domain, MacAddr::ZERO);
        assert_eq!(row.vlan_id, 0);
    }
    assert_eq!(rows[2].haul_type.as_slice(), &[HaulType(5)]);
    assert_eq!(rows[2].id.as_bytes(), b"Unknown@OneWifiMesh");
}

// intentional-difference:reset-global-id: dm_easy_mesh.cpp:951-973 uses the file's ID while
// decoding row IDs, although dm_easy_mesh_ctrl.cpp:10864-10866 does not apply its network record.
#[test]
fn ignored_network_fields_cannot_change_rows_or_their_global_id() {
    let json = r#"{"ignored": [1,2], "wfa-dataelements:Reset": {
        "ID": {"ignored":"not a network id"}, "ControllerID": null,
        "ColocatedAgentID": 17, "Interfaces": false, "NumberOfDevices": "ignored",
        "TimeStamp": [false], "extra": {"anything":true},
        "NetworkSSIDList":[{"SSID":"ok", "HaulType":["Backhaul"], "Other": 100}]}}"#;
    let rows = load_reset_str(json).unwrap();
    assert_eq!(rows[0].id.as_bytes(), b"Backhaul@OneWifiMesh");
    assert_eq!(rows[0].ssid.as_bytes(), b"ok");
    let no_network_fields = document(r#"{"SSID":"ok","HaulType":["Backhaul"]}"#);
    assert_eq!(rows, load_reset_str(&no_network_fields).unwrap());
}

// intentional-difference:reset-file-required: upstream logs a missing file and continues
// (dm_easy_mesh_ctrl.cpp:10856-10858); 0013 D2 requires the error to reach the caller.
#[test]
fn missing_and_unreadable_file_errors_are_distinct() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    assert!(matches!(
        load_reset_file(base.join("missing-reset-file.json")),
        Err(ResetError::MissingFile { .. })
    ));
    assert!(matches!(
        load_reset_file(base),
        Err(ResetError::ReadFile { .. })
    ));
}

// intentional-difference:reset-invalid-document: the upstream reset decoder reports these
// failures (dm_easy_mesh.cpp:903-912,953-963), but bootstrap ignores its return value
// (dm_easy_mesh_ctrl.cpp:10861-10867); this loader returns them to the caller.
#[test]
fn malformed_json_and_missing_or_wrong_envelopes_are_errors() {
    assert!(matches!(load_reset_str("{"), Err(ResetError::Json(_))));
    assert!(matches!(load_reset_str("["), Err(ResetError::Json(_))));
    assert!(matches!(
        load_reset_str(&format!("{} trailing", document("{}"))),
        Err(ResetError::Json(_))
    ));
    assert!(matches!(
        load_reset_str("[]"),
        Err(ResetError::Field {
            field: "$",
            problem: FieldProblem::WrongType { .. },
            ..
        })
    ));
    assert!(matches!(
        load_reset_str(r#"{"wfa-dataelements:Reset":{"unknown":[}}"#),
        Err(ResetError::Json(_))
    ));
    assert!(matches!(
        load_reset_str("{}"),
        Err(ResetError::Field {
            field: "wfa-dataelements:Reset",
            problem: FieldProblem::Missing,
            ..
        })
    ));
    assert!(matches!(
        load_reset_str(r#"{"wfa-dataelements:Reset":{}}"#),
        Err(ResetError::Field {
            field: "NetworkSSIDList",
            problem: FieldProblem::Missing,
            ..
        })
    ));
    assert!(matches!(
        load_reset_str(&document("")),
        Err(ResetError::Field {
            field: "NetworkSSIDList",
            problem: FieldProblem::Empty,
            ..
        })
    ));
    assert!(matches!(
        load_reset_str(r#"{"wfa-dataelements:Reset":{"NetworkSSIDList":{}}}"#),
        Err(ResetError::Field {
            field: "NetworkSSIDList",
            problem: FieldProblem::WrongType { .. },
            ..
        })
    ));
}

// intentional-difference:reset-bounds: upstream truncates snprintf strings and writes the lists
// without bounds checks (dm_network_ssid.cpp:45-59,67-71,75-84,92-106;
// dm_easy_mesh.cpp:959-973). em-dm instead reports fields that exceed its bounds.
#[test]
fn oversized_rows_lists_and_each_string_field_are_rejected() {
    let cases = [
        ("SSID", 32),
        ("PassPhrase", 127),
        ("SuiteSelector", 31),
        ("MFPConfig", 31),
        ("AuthType", 31),
    ];
    for (field, limit) in cases {
        let input = document(&format!(r#"{{"{field}":"{}"}}"#, "a".repeat(limit + 1)));
        assert!(matches!(load_reset_str(&input), Err(ResetError::Field {
            row: Some(0), field: actual, problem: FieldProblem::String(StringError::TooLong { len, capacity })
        }) if actual == field && len == limit + 1 && capacity == limit));
    }
    for field in ["Band", "AKMsAllowed"] {
        let input = document(&format!(r#"{{"{field}":["{}"]}}"#, "x".repeat(64)));
        assert!(matches!(load_reset_str(&input), Err(ResetError::Field {
            field: actual, problem: FieldProblem::String(StringError::TooLong { len: 64, capacity: 63 }), ..
        }) if actual == field));
    }
    for (field, count) in [("Band", 4), ("AKMsAllowed", 11), ("HaulType", 9)] {
        let elements = vec!["\"x\""; count].join(",");
        let input = document(&format!(r#"{{"{field}":[{elements}]}}"#));
        assert!(matches!(load_reset_str(&input), Err(ResetError::Field {
            field: actual, problem: FieldProblem::TooMany { actual: n, capacity }, ..
        }) if actual == field && n == count && capacity == count - 1));
    }
    let input = document(&["{}"; 6].join(","));
    assert!(matches!(
        load_reset_str(&input),
        Err(ResetError::Field {
            field: "NetworkSSIDList",
            problem: FieldProblem::TooMany {
                actual: 6,
                capacity: 5
            },
            ..
        })
    ));
}

// intentional-difference:reset-ssid-32: the native model accepts a 32-byte SSID (0007 A1),
// while the upstream reset decoder uses snprintf into ssid_t (dm_network_ssid.cpp:46-47).
#[test]
fn values_at_model_limits_are_accepted() {
    let input = document(&format!(
        r#"{{"SSID":"{}","PassPhrase":"{}","SuiteSelector":"{}",
        "MFPConfig":"{}","AuthType":"{}","Band":["{}","b","c"],
        "AKMsAllowed":[{}],"HaulType":[{}],"VLANID":65535}}"#,
        "s".repeat(32),
        "p".repeat(127),
        "q".repeat(31),
        "m".repeat(31),
        "a".repeat(31),
        "b".repeat(63),
        ["\"x\""; 10].join(","),
        ["\"IoT\""; 8].join(",")
    ));
    let rows = load_reset_str(&input).unwrap();
    assert_eq!(rows[0].ssid.len(), 32);
    assert_eq!(rows[0].pass_phrase.expose().len(), 127);
    assert_eq!(rows[0].band.len(), 3);
    assert_eq!(rows[0].akm.len(), 10);
    assert_eq!(rows[0].haul_type.len(), 8);
    assert_eq!(rows[0].vlan_id, 65535);
}

// intentional-difference:reset-invalid-values: C strings truncate at NUL, cJSON coercions and
// unchecked sscanf/numeric casts can yield partial or unsafe results (dm_network_ssid.cpp:45-106,
// dm_easy_mesh.cpp:2070-2080); this loader rejects values it cannot represent unambiguously.
#[test]
fn invalid_field_types_nul_mac_and_vlan_are_errors() {
    let cases = [
        (r#"{"SSID":"a\u0000b"}"#, "SSID"),
        (r#"{"HaulType":["F\u0000ignored"]}"#, "HaulType"),
        (r#"{"Band":[1]}"#, "Band"),
        (r#"{"AKMsAllowed":"sae"}"#, "AKMsAllowed"),
        (r#"{"HaulType":[null]}"#, "HaulType"),
        (r#"{"Enable":null}"#, "Enable"),
        (r#"{"AdvertisementEnabled":1}"#, "AdvertisementEnabled"),
        (r#"{"PassPhrase":true}"#, "PassPhrase"),
        (r#"{"MobilityDomain":"not-a-mac"}"#, "MobilityDomain"),
        (r#"{"VLANID":65536}"#, "VLANID"),
        (r#"{"VLANID":-1}"#, "VLANID"),
        (r#"{"VLANID":1.5}"#, "VLANID"),
    ];
    for (body, expected) in cases {
        assert!(
            matches!(load_reset_str(&document(body)), Err(ResetError::Field {
            row: Some(0), field, ..
        }) if field == expected),
            "{body}"
        );
    }
    let input = document(r#"{}, {"SSID":"a\u0000b"}"#);
    assert!(matches!(
        load_reset_str(&input),
        Err(ResetError::Field {
            row: Some(1),
            field: "SSID",
            ..
        })
    ));
}

#[test]
fn mobility_domain_accepts_both_upstream_address_forms() {
    let rows = load_reset_str(&document(
        r#"{"MobilityDomain":"aabbccddeeff"},
        {"MobilityDomain":"AA:BB:CC:DD:EE:FF"}"#,
    ))
    .unwrap();
    assert_eq!(
        rows[0].mobility_domain,
        MacAddr::new([0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff])
    );
    assert_eq!(rows[1].mobility_domain, rows[0].mobility_domain);
}

#[test]
fn field_errors_never_include_rejected_passphrase() {
    let input = document(&format!(
        r#"{{"PassPhrase":"{}"}}"#,
        "private-secret".repeat(12)
    ));
    let error = load_reset_str(&input).unwrap_err();
    assert!(!format!("{error}").contains("private-secret"));
    assert!(!format!("{error:?}").contains("private-secret"));
}
