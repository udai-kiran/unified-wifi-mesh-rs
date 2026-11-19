// SPDX-License-Identifier: Apache-2.0

//! Golden test: every row of `tests/data/dm_inventory.tsv` has a Rust-side mapping, and every
//! mapping has a row.
//!
//! Rust-side sources of truth:
//!
//! - **Generated tables** (`record!`, `open_enum!`): one table per struct or enum, emitted from
//!   the same declaration as the type. See [`record_tables`] and [`enum_tables`].
//! - **External mapping** (hand-written): rows owned by `em-types`, C scalars, string,
//!   byte-array and type aliases, and the typedef/ABI rows. See [`EXTERNAL_STRUCTS`],
//!   [`STRING_ALIASES`], [`BYTE_ALIASES`], [`TYPE_ALIASES`], [`PRIMITIVES`] and [`TYPEDEFS`].
//!
//! The test fails on unmapped rows, mappings without a row, duplicate claims, wrong field types,
//! wrong capacities, wrong enum codes and widths, unknown flags, count-like fields that are
//! neither folded nor justified, wire lengths without a sweep result, Rust field names that do
//! not follow the C names, and oversized `Copy` types. Counts come from the TSV, never from
//! constants.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use em_types::{FreqBand, Interface, InterfaceName, MacAddr, MediaType, ProfileType};

use crate::COPY_SIZE_LIMIT;
use crate::bits::{BitsError, UBits};
use crate::bounded::CapacityError;
use crate::enums::{
    ChannelBandwidth, DeviceRole, HaulType, Ieee80211Variant, OpClassType, PolicyIdType,
    ProductClass, ScannerType, SteeringPolicyType, VapMode,
};
use crate::float::Float32;
use crate::limits::{
    LONG_STRING_CAP, SHORT_STRING_CAP, SMALL_STRING_CAP, SSID_CAP, STRING_CAP, TINY_STRING_CAP,
};
use crate::mapping::{
    EnumTable, FieldMap, Map, NO_WIRE, NativeShape, RecordTable, ShapeKind, StrKind,
};
use crate::records::{
    AffiliatedApInfo, AffiliatedBstaInfo, AffiliatedStaInfo, AlarmSamples, ApHeCap, ApHeMcsMaps,
    ApHtCap, ApMldInfo, ApRadioAdvancedCap, ApVhtCap, AssocStaMldInfo, BackhaulBssConfigPolicy,
    BeaconReport, BssId, BssInfo, BstaMldInfo, CacCapMethod, CacCapRadio, CacCompInfo,
    CacCompRprtPair, CacOpClass, ChannelScanCapRadio, ClientFiltersCfg, DeviceId, DeviceInfo,
    EhtOperations, EhtOperationsBss, EhtOperationsRadio, ExtDeviceInfo, Ieee1905Security,
    Ieee1905SecurityCap, Ieee1905SecurityInfo, Ieee8021qSettingsPolicy, InterfacePreference,
    LinkReport, LinkStatsAlarmCfg, MediaSpecData, MetricCltnInterval, MloPolicy, Neighbor,
    NetworkInfo, NetworkSsidInfo, OpClassId, OpClassInfo, Policy, PolicyId, Profile2ApCap,
    QosMgtPolicy, RadioCapInfo, RadioId, RadioInfo, RadioWifi6CapData, ScanCapOpClassInfo,
    ScanResult, ScanResultId, Ssid2VidMapInfo, StaInfo, TidToLinkInfo, TidToLinkMapInfo,
    TrafficSeparSsidInfo, TrafficSeparationPolicy, UnassocQueryChannel, UnassocQueryList,
    UnassocQueryOpclass, UnassocStaMetricEntry, UnassocStaMetricsRsp, WideBwChannel,
    Wifi6CapRoleHead, Wifi6CapRoleTail, Wifi6RoleWire, Wifi7AgentCap, Wifi7FreqRecord,
    Wifi7FreqRecords, Wifi7MloCapRecords, Wifi7MloCapSupport,
};
use crate::secret::Secret;
use crate::text::{CStrBytes, LenBytes, StringError, Text};

// ---------------------------------------------------------------------------
// Rust-side tables
// ---------------------------------------------------------------------------

/// Types classified `em-dm` that Step 2 deliberately does not implement. Each needs a reason.
const DEFERRED: &[(&str, &str)] = &[
    (
        "em_neighbor_info_t",
        "not reached from the stored roots; to its consumer",
    ),
    ("em_target_sta_map_t", "model-map selector; Step 3"),
    (
        "SecurityTypeMap",
        "static table; to the consumer that ports the security-mode strings (0007 D4 labels)",
    ),
];

/// C scalar types, the Rust type that represents them, and its size in bytes.
const PRIMITIVES: &[(&str, &str, usize)] = &[
    ("unsigned char", "u8", size_of::<u8>()),
    ("unsigned short", "u16", size_of::<u16>()),
    ("unsigned int", "u32", size_of::<u32>()),
    ("unsigned long long", "u64", size_of::<u64>()),
    ("signed char", "i8", size_of::<i8>()),
    ("signed int", "i32", size_of::<i32>()),
    ("int", "i32", size_of::<i32>()),
    ("float", "Float32", size_of::<Float32>()),
];

/// `<mapping>` rows: typedef or ABI leaf, its inventory text, Rust type, and Rust size in bytes.
/// A text with `size=` is an ABI leaf of the generator; a bare text names a [`PRIMITIVES`] row.
const TYPEDEFS: &[(&str, &str, &str, usize)] = &[
    ("bool", "bool; size=1", "bool", size_of::<bool>()),
    ("uint8_t", "unsigned char; size=1", "u8", size_of::<u8>()),
    ("uint32_t", "unsigned int; size=4", "u32", size_of::<u32>()),
    ("size_t", "unsigned long; size=8", "u64", size_of::<u64>()),
    ("time_t", "signed long; size=8", "i64", size_of::<i64>()),
    // HAL typedefs. `BOOL` is an `unsigned char` that holds 0 or 1: a flag, so `bool`.
    ("UCHAR", "unsigned char", "u8", size_of::<u8>()),
    ("USHORT", "unsigned short", "u16", size_of::<u16>()),
    ("UINT", "unsigned int", "u32", size_of::<u32>()),
    ("ULLONG", "unsigned long long", "u64", size_of::<u64>()),
    ("BOOL", "unsigned char", "bool", size_of::<bool>()),
];

/// `<alias>` rows that name a `char[N]`: alias, native payload capacity, and the decision tag of
/// an intentional difference from the C payload `N - 1`.
const STRING_ALIASES: &[(&str, usize, Option<&str>)] = &[
    ("em_string_t", STRING_CAP, None),
    ("em_short_string_t", SHORT_STRING_CAP, None),
    ("em_long_string_t", LONG_STRING_CAP, None),
    ("em_small_string_t", SMALL_STRING_CAP, None),
    ("em_tiny_string_t", TINY_STRING_CAP, None),
    // 0007 amendment A1: native 32 bytes, C payload 31.
    ("ssid_t", SSID_CAP, Some("0007-A1")),
    // 0001 (docs/0001-em-types-scope.md): em-types' InterfaceName holds 15 bytes (Linux IFNAMSIZ - 1),
    // C payload 31.
    ("em_interface_name_t", InterfaceName::MAX_LEN, Some("0001")),
];

/// `<alias>` rows that name a byte array, with the Rust type's byte length.
const BYTE_ALIASES: &[(&str, usize)] = &[
    ("mac_address_t", size_of::<MacAddr>()),
    ("mac_addr_t", size_of::<MacAddr>()),
];

/// `<alias>` rows that rename another alias: the alias and its target.
const TYPE_ALIASES: &[(&str, &str)] = &[("bssid_t", "mac_address_t")];

/// C type, Rust type, and (C field, Rust field) pairs of an `em-types` struct. The pairs are
/// proved by an exhaustive destructuring in `external_struct_fields_exist`.
type ExternalStruct = (
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
);

const EXTERNAL_STRUCTS: &[ExternalStruct] = &[(
    "em_interface_t",
    "Interface",
    &[("name", "name"), ("mac", "mac"), ("media", "media")],
)];

/// One enumerator of an `em-types` enum: C identifier, inventory value, the value of the Rust
/// const that represents it.
type ExternalEntry = (&'static str, u32, u32);

/// An enum owned by `em-types`: C type, Rust type, retained entries, sentinels with reasons.
struct ExternalEnum {
    c_type: &'static str,
    rust_name: &'static str,
    retained: Vec<ExternalEntry>,
    sentinels: Vec<(&'static str, u32, &'static str)>,
}

fn external_enums() -> Vec<ExternalEnum> {
    let media = |c: &'static str, v: u32, r: MediaType| (c, v, u32::from(r.0));
    let band = |c: &'static str, v: u32, r: FreqBand| (c, v, u32::from(r.0));
    let profile = |c: &'static str, v: u32, r: ProfileType| (c, v, u32::from(r.0));
    vec![
        ExternalEnum {
            c_type: "em_media_type_t",
            rust_name: "MediaType",
            retained: vec![
                media("em_media_type_ieee8023ab", 1, MediaType::IEEE_802_3AB),
                media(
                    "em_media_type_ieee80211b_24",
                    256,
                    MediaType::IEEE_802_11B_2_4_GHZ,
                ),
                media(
                    "em_media_type_ieee80211g_24",
                    257,
                    MediaType::IEEE_802_11G_2_4_GHZ,
                ),
                media(
                    "em_media_type_ieee80211a_5",
                    258,
                    MediaType::IEEE_802_11A_5_GHZ,
                ),
                media(
                    "em_media_type_ieee80211n_24",
                    259,
                    MediaType::IEEE_802_11N_2_4_GHZ,
                ),
                media(
                    "em_media_type_ieee80211n_5",
                    260,
                    MediaType::IEEE_802_11N_5_GHZ,
                ),
                media(
                    "em_media_type_ieee80211ac_5",
                    261,
                    MediaType::IEEE_802_11AC_5_GHZ,
                ),
                media(
                    "em_media_type_ieee80211ad_60",
                    262,
                    MediaType::IEEE_802_11AD_60_GHZ,
                ),
                media("em_media_type_ieee80211af", 263, MediaType::IEEE_802_11AF),
            ],
            sentinels: vec![("em_media_type_max", 264, "sentinel: upper-bound marker")],
        },
        ExternalEnum {
            c_type: "em_freq_band_t",
            rust_name: "FreqBand",
            retained: vec![
                band("em_freq_band_24", 0, FreqBand::BAND_2_4_GHZ),
                band("em_freq_band_5", 1, FreqBand::BAND_5_GHZ),
                band("em_freq_band_60", 2, FreqBand::BAND_60_GHZ),
                band("em_freq_band_6", 3, FreqBand::BAND_6_GHZ),
            ],
            sentinels: vec![("em_freq_band_unknown", 4, "sentinel: unknown marker")],
        },
        ExternalEnum {
            c_type: "em_profile_type_t",
            rust_name: "ProfileType",
            retained: vec![
                profile("em_profile_type_1", 1, ProfileType::MULTI_AP_PROFILE_1),
                profile("em_profile_type_2", 2, ProfileType::MULTI_AP_PROFILE_2),
                profile("em_profile_type_3", 3, ProfileType::MULTI_AP_PROFILE_3),
            ],
            sentinels: vec![
                ("em_profile_type_reserved", 0, "sentinel: reserved"),
                ("em_profile_type_max", 4, "sentinel: upper-bound marker"),
            ],
        },
    ]
}

/// Field tables of every record, in dependency order.
fn record_tables() -> Vec<&'static RecordTable> {
    vec![
        &InterfacePreference::TABLE,
        &NetworkInfo::TABLE,
        &DeviceId::TABLE,
        &Ieee1905Security::TABLE,
        &ExtDeviceInfo::TABLE,
        &DeviceInfo::TABLE,
        &Ieee1905SecurityCap::TABLE,
        &Ieee1905SecurityInfo::TABLE,
        &NetworkSsidInfo::TABLE,
        &Ssid2VidMapInfo::TABLE,
        &RadioId::TABLE,
        &MediaSpecData::TABLE,
        &RadioInfo::TABLE,
        &ApHtCap::TABLE,
        &ApVhtCap::TABLE,
        &ApHeMcsMaps::TABLE,
        &ApHeCap::TABLE,
        &Wifi6CapRoleHead::TABLE,
        &Wifi6CapRoleTail::TABLE,
        &Wifi6RoleWire::TABLE,
        &RadioWifi6CapData::TABLE,
        &Wifi7MloCapSupport::TABLE,
        &Wifi7FreqRecord::TABLE,
        &Wifi7FreqRecords::TABLE,
        &Wifi7MloCapRecords::TABLE,
        &Wifi7AgentCap::TABLE,
        &EhtOperationsBss::TABLE,
        &EhtOperationsRadio::TABLE,
        &EhtOperations::TABLE,
        &ScanCapOpClassInfo::TABLE,
        &ChannelScanCapRadio::TABLE,
        &ApRadioAdvancedCap::TABLE,
        &Profile2ApCap::TABLE,
        &CacOpClass::TABLE,
        &CacCapMethod::TABLE,
        &CacCapRadio::TABLE,
        &MetricCltnInterval::TABLE,
        &RadioCapInfo::TABLE,
        &OpClassId::TABLE,
        &OpClassInfo::TABLE,
        &CacCompRprtPair::TABLE,
        &CacCompInfo::TABLE,
        &BssId::TABLE,
        &BssInfo::TABLE,
        &PolicyId::TABLE,
        &Ieee8021qSettingsPolicy::TABLE,
        &TrafficSeparSsidInfo::TABLE,
        &TrafficSeparationPolicy::TABLE,
        &QosMgtPolicy::TABLE,
        &BackhaulBssConfigPolicy::TABLE,
        &LinkStatsAlarmCfg::TABLE,
        &ClientFiltersCfg::TABLE,
        &Policy::TABLE,
        &AlarmSamples::TABLE,
        &LinkReport::TABLE,
        &WideBwChannel::TABLE,
        &BeaconReport::TABLE,
        &StaInfo::TABLE,
        &ScanResultId::TABLE,
        &Neighbor::TABLE,
        &ScanResult::TABLE,
        &AffiliatedApInfo::TABLE,
        &ApMldInfo::TABLE,
        &AffiliatedBstaInfo::TABLE,
        &BstaMldInfo::TABLE,
        &AffiliatedStaInfo::TABLE,
        &AssocStaMldInfo::TABLE,
        &TidToLinkMapInfo::TABLE,
        &TidToLinkInfo::TABLE,
        &MloPolicy::TABLE,
        &UnassocQueryChannel::TABLE,
        &UnassocQueryOpclass::TABLE,
        &UnassocQueryList::TABLE,
        &UnassocStaMetricEntry::TABLE,
        &UnassocStaMetricsRsp::TABLE,
    ]
}

/// Tables of the ten enums owned by `em-dm`.
fn enum_tables() -> Vec<&'static EnumTable> {
    vec![
        &Ieee80211Variant::TABLE,
        &ProductClass::TABLE,
        &DeviceRole::TABLE,
        &HaulType::TABLE,
        &VapMode::TABLE,
        &OpClassType::TABLE,
        &PolicyIdType::TABLE,
        &SteeringPolicyType::TABLE,
        &ScannerType::TABLE,
        &ChannelBandwidth::TABLE,
    ]
}

// ---------------------------------------------------------------------------
// TSV loading
// ---------------------------------------------------------------------------

/// One data row of `dm_inventory.tsv`.
#[derive(Debug)]
struct Row {
    ty: String,
    field: String,
    c_type: String,
    bound_value: Option<usize>,
    bit_width: Option<u32>,
    flags: String,
    value: Option<u32>,
}

impl Row {
    fn is_alias(&self) -> bool {
        self.field == "<alias>"
    }

    fn is_typedef(&self) -> bool {
        self.field == "<mapping>"
    }

    fn is_enumerator(&self) -> bool {
        self.c_type == "enumerator"
    }

    fn key(&self) -> (&str, &str) {
        (&self.ty, &self.field)
    }
}

fn data_lines(name: &str) -> Vec<Vec<String>> {
    let path = format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
    let mut lines = text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|l| l.split('\t').map(str::to_owned).collect::<Vec<String>>());
    let header = lines
        .next()
        .unwrap_or_else(|| panic!("{name} has no header"));
    let rows: Vec<_> = lines.collect();
    for row in &rows {
        assert_eq!(
            row.len(),
            header.len(),
            "{name}: wrong column count in {row:?}"
        );
    }
    rows
}

fn optional<T: std::str::FromStr>(text: &str) -> Option<T> {
    if text == "-" {
        None
    } else {
        Some(
            text.parse()
                .unwrap_or_else(|_| panic!("bad number {text:?}")),
        )
    }
}

fn load_inventory() -> Vec<Row> {
    data_lines("dm_inventory.tsv")
        .into_iter()
        .map(|mut c| Row {
            ty: std::mem::take(&mut c[0]),
            field: std::mem::take(&mut c[1]),
            c_type: std::mem::take(&mut c[2]),
            bound_value: optional(&c[4]),
            bit_width: optional(&c[5]),
            flags: std::mem::take(&mut c[6]),
            value: optional(&c[7]),
        })
        .collect()
}

/// One row of `dm_type_classification.tsv`.
struct Classification {
    ty: String,
    owner: String,
}

fn load_classification() -> Vec<Classification> {
    data_lines("dm_type_classification.tsv")
        .into_iter()
        .map(|mut c| Classification {
            ty: std::mem::take(&mut c[0]),
            owner: std::mem::take(&mut c[3]),
        })
        .collect()
}

fn fail_if_any(what: &str, problems: &[String]) {
    assert!(problems.is_empty(), "{what}:\n  {}", problems.join("\n  "));
}

fn squash(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

// ---------------------------------------------------------------------------
// Derived sets
// ---------------------------------------------------------------------------

fn inventory_types(rows: &[Row]) -> BTreeSet<&str> {
    rows.iter().map(|r| r.ty.as_str()).collect()
}

// ---------------------------------------------------------------------------
// Claims: which Rust-side entry accounts for which row
// ---------------------------------------------------------------------------

/// A (type, field) inventory key.
type Key<'a> = (&'a str, &'a str);

/// Every (type, field) key that some Rust-side table claims, with the claimant's name.
/// Duplicate claims are reported through `problems`.
fn collect_claims(problems: &mut Vec<String>) -> BTreeMap<Key<'static>, String> {
    let mut claims: BTreeMap<Key<'static>, String> = BTreeMap::new();
    let mut claim = |key: Key<'static>, by: String, problems: &mut Vec<String>| {
        if let Some(first) = claims.get(&key) {
            problems.push(format!("{key:?} claimed twice: by {first} and by {by}"));
        } else {
            claims.insert(key, by);
        }
    };

    for table in record_tables() {
        for field in table.fields {
            for c_field in field.map.c_fields() {
                let by = format!("{}.{}", table.rust_name, field.rust_field);
                claim((table.c_type, c_field), by, problems);
            }
            if let Some((wrapper_type, wrapper_field)) = field.map.cross_fold {
                let by = format!("{}.{}", table.rust_name, field.rust_field);
                claim((wrapper_type, wrapper_field), by, problems);
            }
        }
    }
    for table in enum_tables() {
        for entry in table.entries {
            let by = format!("{}::{}", table.rust_name, entry.c_ident);
            claim((table.c_type, entry.c_ident), by, problems);
        }
    }
    for external in external_enums() {
        let idents = external
            .retained
            .iter()
            .map(|e| e.0)
            .chain(external.sentinels.iter().map(|e| e.0));
        for ident in idents {
            let by = format!("external {}", external.rust_name);
            claim((external.c_type, ident), by, problems);
        }
    }
    for (c_type, rust, pairs) in EXTERNAL_STRUCTS {
        for (c_field, _) in *pairs {
            claim((c_type, c_field), format!("external {rust}"), problems);
        }
    }
    for (alias, _, _) in STRING_ALIASES {
        claim((alias, "<alias>"), "string alias".to_owned(), problems);
    }
    for (alias, _) in BYTE_ALIASES {
        claim((alias, "<alias>"), "byte alias".to_owned(), problems);
    }
    for (alias, _) in TYPE_ALIASES {
        claim((alias, "<alias>"), "type alias".to_owned(), problems);
    }
    for (name, _, _, _) in TYPEDEFS {
        claim((name, "<mapping>"), "typedef".to_owned(), problems);
    }
    claims
}

#[test]
fn every_inventory_row_has_exactly_one_mapping() {
    let rows = load_inventory();
    assert!(!rows.is_empty(), "inventory has no rows");
    let mut problems = Vec::new();
    let claims = collect_claims(&mut problems);

    let wanted: BTreeSet<Key<'_>> = rows.iter().map(Row::key).collect();
    let mut seen_rows = BTreeSet::new();
    for row in &rows {
        if !seen_rows.insert(row.key()) {
            problems.push(format!("inventory repeats {:?}", row.key()));
        }
    }
    for key in &wanted {
        if !claims.contains_key(key) {
            problems.push(format!("unmapped inventory row {key:?}"));
        }
    }
    for (key, by) in &claims {
        if !wanted.contains(key) {
            problems.push(format!("extra mapping {key:?} by {by}: no such row"));
        }
    }
    fail_if_any("inventory rows and Rust mappings differ", &problems);
}

// ---------------------------------------------------------------------------
// Field types
// ---------------------------------------------------------------------------

/// Rust name of the type that represents C type `c_type` as one value (not an array).
fn rust_element(c_type: &str, row: &Row) -> Option<String> {
    let c_type = TYPE_ALIASES
        .iter()
        .find(|(alias, _)| *alias == c_type)
        .map_or(c_type, |(_, target)| *target);
    if row.flags.contains("enum") {
        let from_em_dm = enum_tables()
            .into_iter()
            .find(|t| t.c_type == c_type)
            .map(|t| t.rust_name);
        let ext = external_enums();
        let from_external = ext.iter().find(|e| e.c_type == c_type).map(|e| e.rust_name);
        return from_em_dm.or(from_external).map(str::to_owned);
    }
    if let Some((_, rust, _)) = EXTERNAL_STRUCTS.iter().find(|(c, _, _)| *c == c_type) {
        return Some((*rust).to_owned());
    }
    if BYTE_ALIASES.iter().any(|(alias, _)| *alias == c_type) {
        return Some("MacAddr".to_owned());
    }
    if let Some(table) = record_tables().into_iter().find(|t| t.c_type == c_type) {
        return Some(table.rust_name.to_owned());
    }
    if let Some((_, _, rust, _)) = TYPEDEFS.iter().find(|(name, _, _, _)| *name == c_type) {
        return Some((*rust).to_owned());
    }
    PRIMITIVES
        .iter()
        .find(|(c, _, _)| *c == c_type)
        .map(|(_, rust, _)| (*rust).to_owned())
}

/// The row of an `<alias>` whose name is `c_type`.
fn alias_row<'a>(rows: &'a [Row], c_type: &str) -> Option<&'a Row> {
    rows.iter().find(|r| r.ty == c_type && r.is_alias())
}

/// Element length of the `char[N]` behind `row`, if the row is a string: the array bound of the
/// alias, or the row's own bound for a direct `char` member.
fn char_array_len(rows: &[Row], row: &Row) -> Option<usize> {
    if row.c_type == "char" {
        return row.bound_value;
    }
    let alias = alias_row(rows, &row.c_type)?;
    if alias.c_type == "char" {
        alias.bound_value
    } else {
        None
    }
}

fn is_string_row(rows: &[Row], row: &Row) -> bool {
    char_array_len(rows, row).is_some()
}

/// Pointer rows: inventory key, the cap of the bounded buffer that replaces the pointer, and the
/// cite of the constant that bounds it.
const POINTER_BOUNDS: &[(&str, &str, usize, &str)] = &[(
    "em_sta_info_t",
    "frame_body",
    11_426, // (11454 - 24 - 4), inc/em_base.h:74; written out so the constant cannot drift
    "inc/em_base.h:74",
)];

/// A pointer row becomes a `BoundedVec<u8, CAP>` whose length is the folded `*_len` field.
fn check_pointer_field(row: &Row, field: &FieldMap, at: &str, problems: &mut Vec<String>) {
    let Some((_, _, cap, cite)) = POINTER_BOUNDS
        .iter()
        .find(|(ty, name, _, _)| *ty == row.ty && *name == row.field)
    else {
        problems.push(format!("{at}: pointer row without a recorded bound"));
        return;
    };
    if row.c_type != "unsigned char *" {
        problems.push(format!("{at}: only byte pointers are mapped"));
    }
    let declared = squash(field.rust_type);
    if !declared.starts_with("BoundedVec<u8,") {
        problems.push(format!("{at}: a pointer row must be a BoundedVec<u8, _>"));
    }
    if field.shape.kind != ShapeKind::List || field.shape.cap != *cap {
        problems.push(format!(
            "{at}: native shape {:?} is not a list of {cap}",
            field.shape
        ));
    }
    if field.map.count_field.is_none() {
        problems.push(format!("{at}: pointer row must fold its length field"));
    }
    if field.map.bound_cite != Some(*cite) {
        problems.push(format!(
            "{at}: bound cite {:?} is not {cite:?}",
            field.map.bound_cite
        ));
    }
}

/// A member whose type is a one-field wrapper struct around an array (`em_scan_cap_op_class_info_t`
/// `channels`) becomes one `BoundedVec<element, N>`: the wrapper's array bound is the capacity
/// and the folded count is the length.
fn check_cross_fold_field(
    rows: &[Row],
    row: &Row,
    field: &FieldMap,
    (wrapper_type, wrapper_field): (&str, &str),
    at: &str,
    problems: &mut Vec<String>,
) {
    if row.c_type != wrapper_type {
        problems.push(format!(
            "{at}: member has type {} but folds a field of {wrapper_type}",
            row.c_type
        ));
    }
    let wrapper_rows: Vec<&Row> = rows.iter().filter(|r| r.ty == wrapper_type).collect();
    if wrapper_rows.len() != 1 || wrapper_rows[0].field != wrapper_field {
        problems.push(format!(
            "{at}: {wrapper_type} must have exactly the field {wrapper_field}"
        ));
        return;
    }
    let inner = wrapper_rows[0];
    let (Some(bound), Some(element)) = (inner.bound_value, rust_element(&inner.c_type, inner))
    else {
        problems.push(format!(
            "{at}: {wrapper_type}.{wrapper_field} is not an array"
        ));
        return;
    };
    let expected = format!("BoundedVec<{element},{bound}>");
    if squash(field.rust_type) != squash(&expected) {
        problems.push(format!("{at}: expected type {expected}"));
    }
    if field.map.count_field.is_none() {
        problems.push(format!("{at}: a folded array must fold its count field"));
    }
    if field.shape.kind != ShapeKind::List || field.shape.cap != bound {
        problems.push(format!(
            "{at}: native shape {:?} disagrees with bound {bound}",
            field.shape
        ));
    }
}

fn check_field_type(rows: &[Row], row: &Row, field: &FieldMap, problems: &mut Vec<String>) {
    let at = format!(
        "{}.{} -> {}.{}",
        row.ty, row.field, field.rust_field, field.rust_type
    );
    let declared = squash(field.rust_type);

    if row.flags.contains("bit-field") {
        let Some(width) = row.bit_width else {
            problems.push(format!("{at}: bit-field row without a width"));
            return;
        };
        let expected = if width == 1 {
            "bool".to_owned()
        } else {
            format!("UBits<{width}>")
        };
        if declared != squash(&expected) {
            problems.push(format!(
                "{at}: bit-field of {width} bits must be {expected}"
            ));
        }
        return;
    }
    if row.flags == "pointer" {
        check_pointer_field(row, field, &at, problems);
        return;
    }
    if let Some((wrapper_type, wrapper_field)) = field.map.cross_fold {
        check_cross_fold_field(
            rows,
            row,
            field,
            (wrapper_type, wrapper_field),
            &at,
            problems,
        );
        return;
    }
    if is_string_row(rows, row) {
        check_string_field(rows, row, field, &at, problems);
        return;
    }

    let Some(element) = rust_element(&row.c_type, row) else {
        problems.push(format!(
            "{at}: no Rust type is known for C type {}",
            row.c_type
        ));
        return;
    };
    let counted = field.map.count_field.is_some();
    let expected = match (row.bound_value, counted) {
        (None, _) => element,
        (Some(bound), true) => format!("BoundedVec<{element},{bound}>"),
        (Some(bound), false) => format!("[{element};{bound}]"),
    };
    if declared != squash(&expected) {
        problems.push(format!("{at}: expected type {expected}"));
    }
    if row.bound_value.is_some() {
        let wanted = if counted {
            ShapeKind::List
        } else {
            ShapeKind::Array
        };
        if field.shape.kind != wanted || Some(field.shape.cap) != row.bound_value {
            problems.push(format!(
                "{at}: native shape {:?} disagrees with bound",
                field.shape
            ));
        }
    }
    if field.map.string.is_some()
        || field.map.wire.is_some()
        || field.map.tag.is_some()
        || field.map.reader_nul.is_some()
        || field.map.bound_cite.is_some()
    {
        problems.push(format!("{at}: string facts on a non-string field"));
    }
}

fn cite_is_well_formed(cite: &str) -> bool {
    let (path, line) = cite.rsplit_once(':').unwrap_or(("", ""));
    path.contains('/') && !line.is_empty() && line.chars().all(|c| c.is_ascii_digit())
}

/// True if `text` names a source line, such as `src/dm/dm_device.cpp:368`.
fn has_line_cite(text: &str) -> bool {
    [".cpp:", ".h:"].iter().any(|extension| {
        text.match_indices(extension).any(|(at, _)| {
            text[at + extension.len()..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
        })
    })
}

fn wire_is_well_formed(wire: &str) -> bool {
    if wire == "none at pin" {
        return true;
    }
    if let Some(rest) = wire.strip_prefix("consumer: ") {
        return rest
            .split_once(", ")
            .is_some_and(|(krate, cite)| krate.starts_with("em-") && cite_is_well_formed(cite));
    }
    wire.split_once(" @ ").is_some_and(|(number, cite)| {
        !number.is_empty()
            && number.chars().all(|c| c.is_ascii_digit())
            && cite_is_well_formed(cite)
    })
}

/// The evidence each string kind needs.
fn check_string_kind(map: &Map, kind: StrKind, at: &str, problems: &mut Vec<String>) {
    match kind {
        StrKind::Text => {
            let reason = map.text_reason.unwrap_or("");
            if !has_line_cite(reason) {
                problems.push(format!("{at}: text field needs a cited reason"));
            }
        }
        StrKind::LenBytes => {
            if map.len_field.is_none() {
                problems.push(format!("{at}: len_bytes field must fold its length field"));
            }
        }
        StrKind::CStr => {}
    }
    if kind != StrKind::LenBytes && map.len_field.is_some() {
        problems.push(format!("{at}: only len_bytes folds a length field"));
    }
    if let Some(cite) = map.reader_nul {
        if kind != StrKind::LenBytes {
            problems.push(format!("{at}: only len_bytes can declare a reader NUL"));
        }
        if !cite_is_well_formed(cite) {
            problems.push(format!("{at}: malformed reader NUL cite {cite:?}"));
        }
    }
}

fn check_string_field(
    rows: &[Row],
    row: &Row,
    field: &FieldMap,
    at: &str,
    problems: &mut Vec<String>,
) {
    let map = &field.map;
    let Some(kind) = map.string else {
        problems.push(format!(
            "{at}: string field without a kind (cstr, text or len_bytes)"
        ));
        return;
    };
    let Some(elem_len) = char_array_len(rows, row) else {
        return;
    };

    // (a) C payload: N - 1 for NUL-terminated text, N for length-delimited bytes, N - 1 for
    // length-delimited bytes whose reader appends a NUL.
    let c_payload = match kind {
        StrKind::CStr | StrKind::Text => elem_len - 1,
        StrKind::LenBytes if map.reader_nul.is_some() => elem_len - 1,
        StrKind::LenBytes => elem_len,
    };
    let (want_kind, kind_name) = match kind {
        StrKind::CStr => (ShapeKind::CStrBytes, "cstr"),
        StrKind::Text => (ShapeKind::Text, "text"),
        StrKind::LenBytes => (ShapeKind::LenBytes, "len_bytes"),
    };

    // (b) arrays of strings keep both the element capacity and the outer count.
    let (native, kind_seen) = match (row.bound_value, row.c_type == "char") {
        (Some(outer), false) => {
            if map.count_field.is_none()
                || field.shape.kind != ShapeKind::List
                || field.shape.cap != outer
            {
                problems.push(format!(
                    "{at}: array of {outer} strings must be a BoundedVec of that size"
                ));
            }
            (field.shape.elem_cap, field.shape.elem_kind)
        }
        _ => (field.shape.cap, field.shape.kind),
    };
    if kind_seen != want_kind {
        problems.push(format!(
            "{at}: declared {kind_name} but the Rust type is {kind_seen:?}"
        ));
    }

    // (c) wire length: a number with a cite, `none at pin`, or the consumer's cite.
    match map.wire {
        Some(wire) if wire_is_well_formed(wire) => {}
        Some(wire) => problems.push(format!("{at}: malformed wire length {wire:?}")),
        None => problems.push(format!("{at}: wire length not recorded")),
    }

    // (d) native capacity equals the C payload unless an intentional-difference tag says why.
    let alias_tag = STRING_ALIASES
        .iter()
        .find(|(alias, _, _)| *alias == row.c_type)
        .and_then(|(_, _, tag)| *tag);
    match (native == c_payload, map.tag) {
        (true, None) => {}
        (true, Some(tag)) => {
            problems.push(format!("{at}: stale tag {tag:?}, capacities already match"));
        }
        (false, None) => {
            problems.push(format!(
                "{at}: native capacity {native} differs from C payload {c_payload} without a tag"
            ));
        }
        (false, Some(tag)) => {
            if alias_tag != Some(tag) {
                problems.push(format!(
                    "{at}: tag {tag:?} is not the decision recorded for {}",
                    row.c_type
                ));
            }
        }
    }
    // (e) only ssid_t uses amendment A1: native 32, C payload 31.
    if map.tag == Some("0007-A1") && (row.c_type != "ssid_t" || native != 32 || c_payload != 31) {
        problems.push(format!(
            "{at}: 0007-A1 applies only to ssid_t (native 32, C payload 31)"
        ));
    }
    if let Some(alias_native) = STRING_ALIASES
        .iter()
        .find(|(a, _, _)| *a == row.c_type)
        .map(|a| a.1)
    {
        if alias_native != native {
            problems.push(format!(
                "{at}: native {native} differs from the alias capacity {alias_native}"
            ));
        }
    }

    check_string_kind(map, kind, at, problems);
}

/// C types a folded count or length field may have.
const COUNT_TYPES: &[&str] = &[
    "unsigned char",
    "unsigned short",
    "unsigned int",
    "uint8_t",
    "size_t",
    "int",
];

#[test]
fn rust_field_types_match_inventory_rows() {
    let rows = load_inventory();
    let by_key: BTreeMap<Key<'_>, &Row> = rows.iter().map(|r| (r.key(), r)).collect();
    let mut problems = Vec::new();

    for table in record_tables() {
        for field in table.fields {
            let Some(row) = by_key.get(&(table.c_type, field.map.field)) else {
                continue; // reported by the coverage test
            };
            if row.is_enumerator() || row.is_alias() || row.is_typedef() {
                problems.push(format!(
                    "{}.{} maps a non-field row",
                    table.c_type, field.map.field
                ));
                continue;
            }
            check_field_type(&rows, row, field, &mut problems);

            // A folded count or length field must be an integer member of the same C struct.
            let folded = field.map.count_field.into_iter().chain(field.map.len_field);
            for name in folded {
                match by_key.get(&(table.c_type, name)) {
                    Some(r) if COUNT_TYPES.contains(&r.c_type.as_str()) => {}
                    Some(r) => problems.push(format!(
                        "{}.{name}: folded count has type {}",
                        table.c_type, r.c_type
                    )),
                    None => {
                        problems.push(format!("{}.{name}: folded count has no row", table.c_type));
                    }
                }
            }
        }
    }

    // Every row uses a flag the harness understands.
    for row in &rows {
        if !["-", "enum", "bit-field", "abi", "pointer"].contains(&row.flags.as_str()) {
            problems.push(format!("{:?}: unknown flag {:?}", row.key(), row.flags));
        }
    }
    fail_if_any("native field types disagree with the inventory", &problems);
}

/// String fields with no wire reader at the pin, with the result of the upstream sweep: where the
/// field is written instead (database, JSON, local value, command argument), or `no writer`. The
/// sweep covered `src/em`, `src/agent`, `src/ctrl` and the TLV decoders in `src/dm` for writes
/// into the field from a wire buffer. A field leaves this table when a wire cite is recorded.
const NO_WIRE_SWEEP: &[(&str, &str, &str)] = &[
    (
        "em_ssid_info_traffic_separ_pol_t",
        "ssid",
        "no writer: em_policy_t.traffic_separ is only declared, inc/em_base.h:3841; the TLV case is a no-op, src/em/config/em_configuration.cpp:5595; the policy reader fills a different struct, src/em/policy_cfg/em_policy_cfg.cpp:804",
    ),
    (
        "em_bss_id_t",
        "net_id",
        "key text from the database key string, src/dm/dm_bss.cpp:405",
    ),
    (
        "em_bss_info_t",
        "timestamp",
        "local clock string, src/em/config/em_configuration.cpp:2285; JSON, src/dm/dm_bss.cpp:76",
    ),
    (
        "em_bss_info_t",
        "est_svc_params_be",
        "database column, src/dm/dm_bss_list.cpp:315; the wire only receives it, src/em/metrics/em_metrics.cpp:1964",
    ),
    (
        "em_bss_info_t",
        "est_svc_params_bk",
        "database column, src/dm/dm_bss_list.cpp:316; the wire only receives it, src/em/metrics/em_metrics.cpp:1971",
    ),
    (
        "em_bss_info_t",
        "est_svc_params_vi",
        "database column, src/dm/dm_bss_list.cpp:317; the wire only receives it, src/em/metrics/em_metrics.cpp:1985",
    ),
    (
        "em_bss_info_t",
        "est_svc_params_vo",
        "database column, src/dm/dm_bss_list.cpp:318; the wire only receives it, src/em/metrics/em_metrics.cpp:1978",
    ),
    (
        "em_bss_info_t",
        "mesh_sta_passphrase",
        "argument of the connect command, not a TLV (strcpy, upstream defect 2), src/em/em.cpp:954",
    ),
    (
        "em_device_id_t",
        "net_id",
        "key text from the database key string, src/dm/dm_device.cpp:368",
    ),
    (
        "em_device_info_t",
        "multi_ap_cap",
        "JSON, src/dm/dm_device.cpp:54",
    ),
    (
        "em_device_info_t",
        "exec_env",
        "JSON, src/dm/dm_device.cpp:81",
    ),
    (
        "em_device_info_t",
        "dscp_map",
        "JSON, src/dm/dm_device.cpp:84",
    ),
    (
        "em_device_info_t",
        "ctrl_operation_mode",
        "JSON, src/dm/dm_device.cpp:121",
    ),
    (
        "em_device_info_t",
        "backhaul_down_mac",
        "JSON, src/dm/dm_device.cpp:133",
    ),
    (
        "em_device_info_t",
        "environment",
        "no writer: only read into a TLV, src/em/em.cpp:1838",
    ),
    (
        "em_device_info_t",
        "primary_device_type",
        "no writer: only copied between records, src/dm/dm_device.cpp:306",
    ),
    (
        "em_device_info_t",
        "secondary_device_type",
        "no writer: only copied between records, src/dm/dm_device.cpp:307",
    ),
    (
        "em_interface_preference_t",
        "platform",
        "constant text from the platform JSON, src/dm/dm_easy_mesh.cpp:924",
    ),
    (
        "em_network_info_t",
        "id",
        "JSON, src/dm/dm_network.cpp:45 and src/dm/dm_easy_mesh.cpp:1019",
    ),
    (
        "em_network_info_t",
        "timestamp",
        "JSON, src/dm/dm_network.cpp:53; local clock string, src/dm/dm_network.cpp:203",
    ),
    (
        "em_network_info_t",
        "mscs_disallowed_sta",
        "JSON, src/dm/dm_network.cpp:69",
    ),
    (
        "em_network_info_t",
        "scs_disallowed_sta",
        "JSON, src/dm/dm_network.cpp:77",
    ),
    (
        "em_network_ssid_info_t",
        "id",
        "built from the haul name and network id, src/dm/dm_network_ssid.cpp:108",
    ),
    (
        "em_network_ssid_info_t",
        "band",
        "JSON, src/dm/dm_network_ssid.cpp:59",
    ),
    (
        "em_network_ssid_info_t",
        "akm",
        "JSON, src/dm/dm_network_ssid.cpp:71",
    ),
    (
        "em_network_ssid_info_t",
        "suite_select",
        "JSON, src/dm/dm_network_ssid.cpp:76",
    ),
    (
        "em_network_ssid_info_t",
        "mfp",
        "JSON, src/dm/dm_network_ssid.cpp:84",
    ),
    (
        "em_network_ssid_info_t",
        "auth_type",
        "JSON, src/dm/dm_network_ssid.cpp:102",
    ),
    (
        "em_ssid_2_vid_map_info_t",
        "id",
        "built from the SSID and device id, src/dm/dm_ssid_2_vid_map.cpp:78",
    ),
    (
        "em_ssid_2_vid_map_info_t",
        "ssid",
        "JSON, src/dm/dm_ssid_2_vid_map.cpp:75",
    ),
    (
        "em_policy_id_t",
        "net_id",
        "key text from the database key string, src/dm/dm_policy.cpp:298",
    ),
    (
        "em_radio_id_t",
        "net_id",
        "key text from the database key string, src/dm/dm_radio.cpp:260",
    ),
    (
        "em_radio_info_t",
        "chip_vendor",
        "JSON, src/dm/dm_radio.cpp:119; the wire only receives it, src/em/em.cpp:1870",
    ),
    (
        "em_scan_result_id_t",
        "net_id",
        "copy of the local network id, src/em/channel/em_channel.cpp:2429",
    ),
    ("em_sta_info_t", "timestamp", "JSON, src/dm/dm_sta.cpp:65"),
    (
        "em_sta_info_t",
        "wifi6_cap",
        "no writer: the STA member is not referenced anywhere at the pin",
    ),
    (
        "em_sta_info_t",
        "wifi7_cap",
        "no writer: the STA member is not referenced anywhere at the pin",
    ),
    (
        "em_sta_info_t",
        "cellular_data_pref",
        "JSON, src/dm/dm_sta.cpp:149",
    ),
    (
        "em_sta_info_t",
        "listen_interval",
        "no writer: only JSON output and a record copy, src/dm/dm_sta.cpp:369",
    ),
];

#[test]
fn every_no_wire_field_has_a_sweep_result() {
    let mut problems = Vec::new();
    let mut no_wire = BTreeSet::new();
    for table in record_tables() {
        for field in table.fields {
            if field.map.wire == Some(NO_WIRE) {
                no_wire.insert((table.c_type, field.map.field));
            }
        }
    }
    for key in &no_wire {
        if !NO_WIRE_SWEEP
            .iter()
            .any(|(ty, name, _)| (*ty, *name) == *key)
        {
            problems.push(format!(
                "{key:?} has no wire reader recorded and no sweep result"
            ));
        }
    }
    for (ty, name, evidence) in NO_WIRE_SWEEP {
        if !no_wire.contains(&(*ty, *name)) {
            problems.push(format!(
                "{ty}.{name}: sweep result for a field that has a wire cite"
            ));
        }
        if !(has_line_cite(evidence) || evidence.starts_with("no writer")) {
            problems.push(format!("{ty}.{name}: sweep result without a cite"));
        }
    }
    // The cite that justifies a `text` field must be the writer the sweep found.
    for table in record_tables() {
        for field in table.fields {
            let (Some(reason), Some((ty, name, evidence))) = (
                field.map.text_reason,
                NO_WIRE_SWEEP
                    .iter()
                    .find(|(ty, name, _)| (*ty, *name) == (table.c_type, field.map.field)),
            ) else {
                continue;
            };
            let cite = reason
                .rsplit_once('(')
                .map_or("", |(_, rest)| rest.trim_end_matches(')'));
            if !evidence.contains(cite) {
                problems.push(format!(
                    "{ty}.{name}: text reason cites {cite:?}, the sweep found {evidence:?}"
                ));
            }
        }
    }
    fail_if_any("NO_WIRE fields and the sweep table differ", &problems);
}

/// C name of an integer member that looks like a count or a length.
fn is_count_like(name: &str) -> bool {
    name == "num"
        || name.starts_with("num_")
        || name.starts_with("numberof")
        || name.starts_with("number_of_")
        || name.ends_with("_num")
        || name.ends_with("_count")
        || name.ends_with("_len")
}

/// Count-like integer members that are deliberately kept as independent scalars: (C type, C field,
/// reason, upstream cite). Every other count-like member must be folded into a `BoundedVec` or
/// `LenBytes` length by some mapping.
const INDEPENDENT_SCALARS: &[(&str, &str, &str, &str)] = &[
    (
        "em_ap_he_cap_t",
        "sprt_mcs_len",
        "byte length of the MCS list, not an element count: the writer derives len / EM_MIN_HE_MCS_LEN and clamps it to MAX_MCS; sprt_tx_rx_mcs stays a fixed array",
        "src/em/em.cpp:1421",
    ),
    (
        "em_wifi6_cap_role_head_tlv_t",
        "mcs_nss_num",
        "4-bit byte length of the role's MCS list, used as len / EM_MIN_HE_MCS_LEN and clamped to MAX_MCS; the maps stay a fixed array",
        "src/em/em.cpp:1509",
    ),
    (
        "em_network_info_t",
        "num_of_devices",
        "network statistic bumped per device and reported as NumberOfDevices; the struct has no device array",
        "src/dm/dm_easy_mesh_list.cpp:263",
    ),
    (
        "em_radio_info_t",
        "number_of_bss",
        "BSS count taken from the capability TLV (num_bss), reported as NumberOfBSS; BSS records live outside the struct",
        "src/em/capability/em_capability.cpp:1302",
    ),
    (
        "em_radio_info_t",
        "number_of_unassoc_sta",
        "unassociated-STA statistic reported as NumberOfUnassocSta; the struct has no STA array",
        "src/dm/dm_radio.cpp:146",
    ),
    (
        "em_radio_cap_info_t",
        "num_op_classes",
        "JSON and database value (NumberOfOpClass); op-class rows are separate em_op_class_info_t records, not an array of this struct",
        "src/dm/dm_radio_cap.cpp:77",
    ),
    (
        "em_profile_2_ap_cap_t",
        "max_vid_count",
        "capability value (maximum number of VLAN ids the agent supports), not a list length",
        "src/em/em.cpp:1794",
    ),
    (
        "em_bss_info_t",
        "numberofsta",
        "associated-STA statistic reported as NumberOfSTA; STAs are separate em_sta_info_t records",
        "src/dm/dm_bss.cpp:238",
    ),
    (
        "em_sta_info_t",
        "retrans_count",
        "retransmission statistic read from the wire as a 32-bit counter",
        "src/em/metrics/em_metrics.cpp:917",
    ),
    (
        "em_neighbor_t",
        "sta_count",
        "station count of the neighbor's BSS Load element, read from the wire as a 16-bit measurement",
        "src/em/channel/em_channel.cpp:2381",
    ),
];

#[test]
fn count_like_fields_are_folded_or_justified() {
    let rows = load_inventory();
    let mut folded: BTreeSet<Key<'static>> = BTreeSet::new();
    for table in record_tables() {
        for field in table.fields {
            for name in field.map.count_field.into_iter().chain(field.map.len_field) {
                folded.insert((table.c_type, name));
            }
        }
    }
    let mut problems = Vec::new();
    let mut count_like = BTreeSet::new();
    for row in &rows {
        let is_member = !row.is_alias() && !row.is_typedef() && !row.is_enumerator();
        let is_integer = row.bound_value.is_none() && COUNT_TYPES.contains(&row.c_type.as_str());
        if is_member && is_integer && is_count_like(&row.field) {
            count_like.insert(row.key());
        }
    }
    for key in &count_like {
        let justified = INDEPENDENT_SCALARS
            .iter()
            .any(|(ty, field, _, _)| (*ty, *field) == *key);
        match (folded.contains(key), justified) {
            (false, false) => problems.push(format!(
                "{key:?} looks like a count but is neither folded nor listed as an independent scalar"
            )),
            (true, true) => problems.push(format!("{key:?} is folded and also listed")),
            _ => {}
        }
    }
    for (ty, field, reason, cite) in INDEPENDENT_SCALARS {
        if !count_like.contains(&(*ty, *field)) {
            problems.push(format!("{ty}.{field}: listed but not a count-like row"));
        }
        if reason.is_empty() || !cite_is_well_formed(cite) {
            problems.push(format!("{ty}.{field}: needs a reason and a cite"));
        }
    }
    // A folded name must be an integer member of its struct (checked for type elsewhere).
    for key in &folded {
        if !rows.iter().any(|r| r.key() == *key) {
            problems.push(format!("{key:?}: folded count has no row"));
        }
    }
    fail_if_any("count-like fields", &problems);
}

#[test]
fn alias_rows_match_their_native_capacity() {
    let rows = load_inventory();
    let mut problems = Vec::new();
    for (alias, native, tag) in STRING_ALIASES {
        let Some(row) = alias_row(&rows, alias) else {
            problems.push(format!("{alias}: no alias row"));
            continue;
        };
        let Some(bound) = row.bound_value.filter(|_| row.c_type == "char") else {
            problems.push(format!("{alias}: not a char array"));
            continue;
        };
        match tag {
            None if bound - 1 != *native => {
                problems.push(format!("{alias}: char[{bound}] vs native {native}"));
            }
            Some(_) if bound - 1 == *native => problems.push(format!("{alias}: stale tag")),
            _ => {}
        }
    }
    for (alias, size) in BYTE_ALIASES {
        let row = alias_row(&rows, alias);
        let bound = row.and_then(|r| r.bound_value);
        if bound != Some(*size) || row.map(|r| r.c_type.as_str()) != Some("unsigned char") {
            problems.push(format!(
                "{alias}: byte array {bound:?} vs native size {size}"
            ));
        }
    }
    for (alias, target) in TYPE_ALIASES {
        let row = alias_row(&rows, alias);
        if row.map(|r| r.c_type.as_str()) != Some(*target) {
            problems.push(format!("{alias}: alias row differs from target {target}"));
        }
        if !BYTE_ALIASES.iter().any(|(name, _)| name == target) {
            problems.push(format!("{alias}: target {target} is not a byte alias"));
        }
    }
    for (name, text, _, size) in TYPEDEFS {
        let row = rows.iter().find(|r| r.ty == *name && r.is_typedef());
        match row {
            Some(r) if r.c_type == *text => {
                // An ABI leaf states its size; a HAL typedef names a primitive whose size counts.
                let expected = match text.rsplit_once("size=") {
                    Some((_, n)) => n.parse::<usize>().ok(),
                    None => PRIMITIVES
                        .iter()
                        .find(|(c, _, _)| c == text)
                        .map(|(_, _, primitive_size)| *primitive_size),
                };
                if expected != Some(*size) {
                    problems.push(format!("{name}: ABI size {expected:?} vs Rust size {size}"));
                }
            }
            _ => problems.push(format!("{name}: typedef row differs from {text:?}")),
        }
    }
    fail_if_any(
        "alias or typedef rows disagree with the native types",
        &problems,
    );

    // The alias constants match the C names used in the field tables.
    assert_eq!(<[u8; 6]>::SHAPE.cap, 6);
    assert_eq!(InterfaceName::MAX_LEN, 15);
}

#[test]
fn external_struct_fields_exist() {
    // Exhaustive destructuring (no `..`): a renamed or added em-types field fails to compile.
    let name = InterfaceName::try_from("wlan0").unwrap();
    let interface = Interface {
        name: Some(name),
        mac: MacAddr::ZERO,
        media: MediaType::IEEE_802_3AB,
    };
    let Interface { name, mac, media } = interface;
    assert_eq!(name.map(|n| n.as_str().len()), Some(5));
    assert_eq!(mac, MacAddr::ZERO);
    assert_eq!(media, MediaType::IEEE_802_3AB);
}

// ---------------------------------------------------------------------------
// Enum codes
// ---------------------------------------------------------------------------

fn enumerators<'a>(rows: &'a [Row], c_type: &str) -> BTreeMap<&'a str, u32> {
    rows.iter()
        .filter(|r| r.ty == c_type && r.is_enumerator())
        .map(|r| {
            (
                r.field.as_str(),
                r.value
                    .unwrap_or_else(|| panic!("{} has no value", r.field)),
            )
        })
        .collect()
}

fn is_sentinel_name(ident: &str) -> bool {
    ident.ends_with("_max")
        || ident.ends_with("_unknown")
        || ident.ends_with("_none")
        || ident.ends_with("_reserved")
}

#[test]
fn enum_codes_match_inventory() {
    let rows = load_inventory();
    let mut problems = Vec::new();

    for table in enum_tables() {
        let inventory = enumerators(&rows, table.c_type);
        let mut seen = BTreeSet::new();
        for entry in table.entries {
            seen.insert(entry.c_ident);
            let at = format!("{}::{}", table.rust_name, entry.c_ident);
            match inventory.get(entry.c_ident) {
                Some(value) if *value == entry.value => {}
                Some(value) => problems.push(format!(
                    "{at}: code {} but the inventory says {value}",
                    entry.value
                )),
                None => problems.push(format!("{at}: not in the inventory")),
            }
            if entry.value >> table.bits != 0 {
                problems.push(format!(
                    "{at}: code {} does not fit {} bits",
                    entry.value, table.bits
                ));
            }
            match (entry.rust_const, entry.not_retained) {
                (Some(_), None) if is_sentinel_name(entry.c_ident) => {
                    problems.push(format!("{at}: sentinel retained as a named const"));
                }
                (None, Some(reason)) => {
                    if !is_sentinel_name(entry.c_ident) {
                        problems.push(format!("{at}: not retained but not a sentinel"));
                    }
                    if !reason.starts_with("sentinel: ") {
                        problems.push(format!("{at}: sentinel without a reason"));
                    }
                }
                (Some(_), None) => {}
                _ => problems.push(format!(
                    "{at}: must be exactly one of retained or not retained"
                )),
            }
        }
        for ident in inventory.keys().filter(|i| !seen.contains(*i)) {
            problems.push(format!(
                "{}: inventory value {ident} has no entry",
                table.rust_name
            ));
        }
    }

    for external in external_enums() {
        let inventory = enumerators(&rows, external.c_type);
        let mut seen = BTreeSet::new();
        for (ident, value, rust_value) in &external.retained {
            seen.insert(*ident);
            if inventory.get(ident) != Some(value) || rust_value != value {
                problems.push(format!(
                    "{}: {ident} inventory {:?}, expected {value}, em-types {rust_value}",
                    external.rust_name,
                    inventory.get(ident)
                ));
            }
        }
        for (ident, value, reason) in &external.sentinels {
            seen.insert(*ident);
            if inventory.get(ident) != Some(value)
                || !is_sentinel_name(ident)
                || !reason.starts_with("sentinel: ")
            {
                problems.push(format!("{}: sentinel {ident} is wrong", external.rust_name));
            }
        }
        for ident in inventory.keys().filter(|i| !seen.contains(*i)) {
            problems.push(format!(
                "{}: inventory value {ident} has no entry",
                external.rust_name
            ));
        }
    }
    fail_if_any("enum codes disagree with the inventory", &problems);
}

#[test]
fn retained_enum_consts_carry_their_codes() {
    // The table value comes from the associated const itself; name() must agree both ways.
    fn check<T: Copy>(
        table: &EnumTable,
        all: &[T],
        code: impl Fn(T) -> u32,
        name: impl Fn(T) -> Option<&'static str>,
    ) {
        let retained: Vec<_> = table
            .entries
            .iter()
            .filter(|e| e.rust_const.is_some())
            .collect();
        assert_eq!(
            all.len(),
            retained.len(),
            "{}: ALL and table differ",
            table.rust_name
        );
        for (value, entry) in all.iter().zip(retained) {
            assert_eq!(code(*value), entry.value, "{}", table.rust_name);
            assert_eq!(name(*value), entry.rust_const, "{}", table.rust_name);
        }
        for entry in table.entries.iter().filter(|e| e.rust_const.is_none()) {
            assert!(
                all.iter().all(|v| code(*v) != entry.value),
                "{}: sentinel {} must not be a named const",
                table.rust_name,
                entry.c_ident
            );
        }
    }
    check(
        &Ieee80211Variant::TABLE,
        Ieee80211Variant::ALL,
        |v| u32::from(v.0),
        Ieee80211Variant::name,
    );
    check(
        &ProductClass::TABLE,
        ProductClass::ALL,
        |v| u32::from(v.0),
        ProductClass::name,
    );
    check(
        &DeviceRole::TABLE,
        DeviceRole::ALL,
        |v| u32::from(v.0),
        DeviceRole::name,
    );
    check(
        &HaulType::TABLE,
        HaulType::ALL,
        |v| u32::from(v.0),
        HaulType::name,
    );
    check(
        &VapMode::TABLE,
        VapMode::ALL,
        |v| u32::from(v.0),
        VapMode::name,
    );
    check(
        &OpClassType::TABLE,
        OpClassType::ALL,
        |v| u32::from(v.0),
        OpClassType::name,
    );
    check(
        &PolicyIdType::TABLE,
        PolicyIdType::ALL,
        |v| u32::from(v.0),
        PolicyIdType::name,
    );
    check(
        &SteeringPolicyType::TABLE,
        SteeringPolicyType::ALL,
        |v| u32::from(v.0),
        SteeringPolicyType::name,
    );
    check(
        &ScannerType::TABLE,
        ScannerType::ALL,
        |v| u32::from(v.0),
        ScannerType::name,
    );
    check(
        &ChannelBandwidth::TABLE,
        ChannelBandwidth::ALL,
        |v| u32::from(v.0),
        ChannelBandwidth::name,
    );
}

/// Where the width of an enum comes from.
enum WidthSource {
    /// The widest wire field that feeds the enum, in bits, with the upstream cites of every
    /// reader and writer of that field.
    Wire {
        bits: u32,
        cites: &'static [&'static str],
    },
    /// No wire field feeds the enum: the stored width only has to hold every inventory value.
    /// The reason says where the values come from.
    InventoryRange { reason: &'static str },
}

/// Per-enum width evidence, keyed by C type. `upstream` below is `rdkcentral/unified-wifi-mesh`
/// @ `ba4180b`. The Rust width must be at least the widest wire field of the table.
const ENUM_WIDTHS: &[(&str, WidthSource)] = &[
    (
        "wifi_ieee80211Variant_t",
        WidthSource::InventoryRange {
            reason: "bit mask up to 0x200, built from a 16-bit std mask at src/em/capability/em_capability.cpp:1467",
        },
    ),
    (
        "emex_product_class_t",
        WidthSource::Wire {
            bits: 8,
            cites: &["src/em/config/em_configuration.cpp:1324"],
        },
    ),
    (
        "emex_device_role_t",
        WidthSource::Wire {
            bits: 8,
            cites: &["src/em/config/em_configuration.cpp:1327"],
        },
    ),
    (
        // `unsigned short haultype` of the packed em_ap_vendor_operational_bss_t; the byte
        // readers at :4917 and :5924 are narrower.
        "em_haul_type_t",
        WidthSource::Wire {
            bits: 16,
            cites: &[
                "inc/em_base.h:1617",
                "src/em/config/em_configuration.cpp:1638",
                "src/em/config/em_configuration.cpp:4917",
                "src/em/config/em_configuration.cpp:5924",
            ],
        },
    ),
    (
        // `unsigned short vap_mode` of the same packed struct.
        "em_vap_mode_t",
        WidthSource::Wire {
            bits: 16,
            cites: &[
                "inc/em_base.h:1617",
                "src/em/config/em_configuration.cpp:1639",
            ],
        },
    ),
    (
        "em_op_class_type_t",
        WidthSource::InventoryRange {
            reason: "stored only; decoded from a decimal key string at src/dm/dm_op_class.cpp:206",
        },
    ),
    (
        "em_policy_id_type_t",
        WidthSource::InventoryRange {
            reason: "stored only; read from JSON or a decimal key at src/dm/dm_policy.cpp:310",
        },
    ),
    (
        "em_steering_policy_type_t",
        WidthSource::InventoryRange {
            reason: "stored only; read from JSON or the database at src/dm/dm_policy.cpp:75",
        },
    ),
    (
        "em_scanner_type_t",
        WidthSource::InventoryRange {
            reason: "stored only; decoded from a decimal key string at src/dm/dm_scan_result.cpp:207",
        },
    ),
    (
        "wifi_channelBandwidth_t",
        WidthSource::InventoryRange {
            reason: "stored only; chosen from the decoded bandwidth text (strncmp chain) at src/em/channel/em_channel.cpp:2357",
        },
    ),
];

/// Number of bits needed to hold `value`.
fn bits_needed(value: u32) -> u32 {
    u32::BITS - value.leading_zeros()
}

#[test]
fn enum_widths_cover_the_wire_widths() {
    let rows = load_inventory();
    let mut problems = Vec::new();
    for table in enum_tables() {
        let Some((_, source)) = ENUM_WIDTHS.iter().find(|(c, _)| *c == table.c_type) else {
            problems.push(format!("{}: no width evidence", table.rust_name));
            continue;
        };
        let largest = enumerators(&rows, table.c_type)
            .values()
            .copied()
            .max()
            .unwrap_or(0);
        let needed = match source {
            WidthSource::Wire { bits, cites } => {
                if cites.is_empty() || !cites.iter().all(|c| cite_is_well_formed(c)) {
                    problems.push(format!("{}: malformed width cites", table.rust_name));
                }
                *bits
            }
            WidthSource::InventoryRange { reason } => {
                if !(reason.contains(".cpp:") || reason.contains(".h:")) {
                    problems.push(format!("{}: width reason has no cite", table.rust_name));
                }
                // Nothing wider can reach the type, so the inventory values decide.
                bits_needed(largest)
            }
        };
        if table.bits < needed {
            problems.push(format!(
                "{}: {} bits are narrower than the {needed} bits that reach it",
                table.rust_name, table.bits
            ));
        }
        if table.bits < bits_needed(largest) {
            problems.push(format!(
                "{}: {} bits cannot hold the inventory value {largest}",
                table.rust_name, table.bits
            ));
        }
    }
    for (c_type, _) in ENUM_WIDTHS {
        if !enum_tables().iter().any(|t| t.c_type == *c_type) {
            problems.push(format!("{c_type}: width evidence without an enum"));
        }
    }
    fail_if_any("enum widths", &problems);
}

// ---------------------------------------------------------------------------
// Representation
// ---------------------------------------------------------------------------

#[test]
fn copy_records_fit_the_size_limit_and_owned_records_have_a_reason() {
    let tables = record_tables();
    let copy_by_name: BTreeMap<&str, bool> =
        tables.iter().map(|t| (t.rust_name, t.is_copy)).collect();
    let mut problems = Vec::new();
    for table in &tables {
        if table.is_copy {
            if table.size > COPY_SIZE_LIMIT {
                problems.push(format!(
                    "{}: Copy record of {} bytes",
                    table.rust_name, table.size
                ));
            }
            continue;
        }
        // A non-Copy record must be large, hold a heap collection or an Interface, or hold
        // another non-Copy record.
        let holds_non_copy = table.fields.iter().any(|f| {
            f.shape.kind == ShapeKind::List
                || squash(f.rust_type) == "Interface"
                || copy_by_name.get(squash(f.rust_type).as_str()) == Some(&false)
        });
        if table.size <= COPY_SIZE_LIMIT && !holds_non_copy {
            problems.push(format!(
                "{}: owned record of {} bytes with only Copy parts",
                table.rust_name, table.size
            ));
        }
    }
    fail_if_any("record representation rules broken", &problems);
}

/// `size_of::<T>()` of a `Copy` non-record type; the bound proves the type is `Copy`.
fn copy_size<T: Copy>() -> usize {
    size_of::<T>()
}

#[test]
fn copy_non_record_types_fit_the_size_limit() {
    let mut problems = Vec::new();

    // Each enum is exactly as wide as its repr integer.
    let mut enum_sizes: Vec<(&str, usize, u32)> = Vec::new();
    let mut add_enum = |table: &EnumTable, size: usize| {
        enum_sizes.push((table.rust_name, size, table.bits));
    };
    add_enum(&Ieee80211Variant::TABLE, copy_size::<Ieee80211Variant>());
    add_enum(&ProductClass::TABLE, copy_size::<ProductClass>());
    add_enum(&DeviceRole::TABLE, copy_size::<DeviceRole>());
    add_enum(&HaulType::TABLE, copy_size::<HaulType>());
    add_enum(&VapMode::TABLE, copy_size::<VapMode>());
    add_enum(&OpClassType::TABLE, copy_size::<OpClassType>());
    add_enum(&PolicyIdType::TABLE, copy_size::<PolicyIdType>());
    add_enum(
        &SteeringPolicyType::TABLE,
        copy_size::<SteeringPolicyType>(),
    );
    add_enum(&ScannerType::TABLE, copy_size::<ScannerType>());
    add_enum(&ChannelBandwidth::TABLE, copy_size::<ChannelBandwidth>());
    if enum_sizes.len() != enum_tables().len() {
        problems.push(format!(
            "{} enums are size-checked but the golden test knows {}",
            enum_sizes.len(),
            enum_tables().len()
        ));
    }
    for (name, size, bits) in &enum_sizes {
        if *size * 8 != *bits as usize {
            problems.push(format!("{name}: {size} bytes but a {bits}-bit repr"));
        }
    }

    // Every other `Copy` type: bit-fields, floats, strings at the capacities the records use,
    // credentials wrapping them, and the error types.
    let mut sizes: Vec<(&str, usize)> = enum_sizes
        .iter()
        .map(|(name, size, _)| (*name, *size))
        .collect();
    sizes.extend([
        ("UBits<2>", copy_size::<UBits<2>>()),
        ("UBits<24>", copy_size::<UBits<24>>()),
        ("Float32", copy_size::<Float32>()),
        ("CStrBytes<127>", copy_size::<CStrBytes<LONG_STRING_CAP>>()),
        ("Text<127>", copy_size::<Text<LONG_STRING_CAP>>()),
        ("LenBytes<255>", copy_size::<LenBytes<255>>()),
        (
            "Secret<CStrBytes<63>>",
            copy_size::<Secret<CStrBytes<SHORT_STRING_CAP>>>(),
        ),
        (
            "Secret<CStrBytes<127>>",
            copy_size::<Secret<CStrBytes<LONG_STRING_CAP>>>(),
        ),
        (
            "Secret<LenBytes<255>>",
            copy_size::<Secret<LenBytes<255>>>(),
        ),
        ("BitsError", copy_size::<BitsError>()),
        ("CapacityError", copy_size::<CapacityError>()),
        ("StringError", copy_size::<StringError>()),
    ]);
    for (name, size) in sizes {
        if size > COPY_SIZE_LIMIT {
            problems.push(format!("{name}: Copy type of {size} bytes"));
        }
    }

    fail_if_any("Copy non-record types", &problems);
}

/// C field names whose Rust field is not their plain `snake_case` form: C type, C field, Rust field.
const FIELD_RENAMES: &[(&str, &str, &str)] = &[
    // `type` is a Rust keyword.
    ("em_policy_id_t", "type", "kind"),
    ("em_op_class_id_t", "type", "kind"),
    // The Rust name keeps "bandwidth" as one word.
    (
        "wifi_BeaconReport_t",
        "wideBandWidthChannelPresent",
        "wide_bandwidth_channel_present",
    ),
];

/// `camelCase` to `snake_case`; names that are already `snake_case` are unchanged.
fn snake_case(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

#[test]
fn rust_field_names_follow_the_c_field_names() {
    let mut problems = Vec::new();
    let mut used_renames = BTreeSet::new();
    for table in record_tables() {
        for field in table.fields {
            let c_field = field.map.field;
            let renamed = FIELD_RENAMES
                .iter()
                .find(|(ty, name, _)| *ty == table.c_type && *name == c_field);
            let expected = match renamed {
                Some((ty, name, rust)) => {
                    used_renames.insert((*ty, *name));
                    (*rust).to_owned()
                }
                None => snake_case(c_field),
            };
            if field.rust_field != expected {
                problems.push(format!(
                    "{}.{} is mapped to C field {c_field}, which should be named {expected}",
                    table.rust_name, field.rust_field
                ));
            }
        }
    }
    for (ty, name, rust) in FIELD_RENAMES {
        if !used_renames.contains(&(*ty, *name)) {
            problems.push(format!("rename {ty}.{name} -> {rust} matches no mapping"));
        }
        if snake_case(name) == *rust {
            problems.push(format!("rename {ty}.{name} -> {rust} is not a rename"));
        }
    }
    fail_if_any("Rust field names and C field names differ", &problems);
}

/// Inventory fields that hold credentials. Each must be a `Secret<_>` so that `Debug` output of
/// the record never shows the bytes.
const CREDENTIAL_FIELDS: &[(&str, &str)] = &[
    ("em_network_ssid_info_t", "pass_phrase"),
    ("em_bss_info_t", "mesh_sta_passphrase"),
    ("em_ext_device_info_t", "client_id"),
    ("em_ext_device_info_t", "client_secret"),
];

#[test]
fn credential_fields_are_secrets_and_nothing_else_is() {
    let rows = load_inventory();
    let mut problems = Vec::new();

    // A field that looks like a credential must be on the list.
    for row in &rows {
        let name = row.field.as_str();
        let looks_secret = ["pass", "secret", "psk", "client_id", "key"]
            .iter()
            .any(|word| name.contains(word));
        if looks_secret && !row.is_alias() && !row.is_typedef() && !row.is_enumerator() {
            let listed = CREDENTIAL_FIELDS.contains(&row.key());
            let counted_length = name.ends_with("_len");
            if !listed && !counted_length {
                problems.push(format!(
                    "{:?} looks like a credential but is not listed",
                    row.key()
                ));
            }
        }
    }
    for (c_type, c_field) in CREDENTIAL_FIELDS {
        let Some(table) = record_tables().into_iter().find(|t| t.c_type == *c_type) else {
            problems.push(format!("{c_type}: no record for a credential field"));
            continue;
        };
        let Some(field) = table.fields.iter().find(|f| f.map.field == *c_field) else {
            problems.push(format!("{c_type}.{c_field}: not mapped"));
            continue;
        };
        if !squash(field.rust_type).starts_with("Secret<") {
            problems.push(format!(
                "{}.{}: credential must be a Secret, not {}",
                table.rust_name, field.rust_field, field.rust_type
            ));
        }
    }
    // Nothing else is wrapped: a stray Secret would hide data the logs need.
    for table in record_tables() {
        for field in table.fields {
            let listed = CREDENTIAL_FIELDS.contains(&(table.c_type, field.map.field));
            if !listed && squash(field.rust_type).starts_with("Secret<") {
                problems.push(format!(
                    "{}.{}: Secret on a field that is not a listed credential",
                    table.rust_name, field.rust_field
                ));
            }
        }
    }
    fail_if_any("credential fields", &problems);
}

// ---------------------------------------------------------------------------
// Sizes of the owned records
// ---------------------------------------------------------------------------

/// `size_of` of every owned record on a 64-bit target, in bytes. The crate README lists the same
/// numbers so that Step 3 can choose the storage form of each (inline, or `Box<T>`).
#[cfg(target_pointer_width = "64")]
const OWNED_SIZES: &[(&str, usize)] = &[
    ("NetworkInfo", 376),
    ("ExtDeviceInfo", 520),
    ("DeviceInfo", 1552),
    ("NetworkSsidInfo", 472),
    ("RadioInfo", 408),
    ("RadioWifi6CapData", 24),
    ("Wifi7FreqRecords", 24),
    ("Wifi7MloCapRecords", 192),
    ("Wifi7AgentCap", 240),
    ("EhtOperationsRadio", 56),
    ("EhtOperations", 24),
    ("ScanCapOpClassInfo", 32),
    ("ChannelScanCapRadio", 48),
    ("CacOpClass", 32),
    ("CacCapMethod", 32),
    ("CacCapRadio", 32),
    ("RadioCapInfo", 528),
    ("OpClassInfo", 128),
    ("CacCompInfo", 40),
    ("BssInfo", 768),
    ("TrafficSeparationPolicy", 24),
    ("QosMgtPolicy", 48),
    ("Policy", 464),
    ("LinkReport", 72),
    ("StaInfo", 2728),
    ("ScanResult", 304),
    ("AffiliatedApInfo", 48),
    ("ApMldInfo", 72),
    ("AffiliatedBstaInfo", 40),
    ("BstaMldInfo", 48),
    ("AssocStaMldInfo", 40),
    ("TidToLinkInfo", 32),
    ("UnassocQueryChannel", 32),
    ("UnassocQueryOpclass", 32),
    ("UnassocQueryList", 24),
    ("UnassocStaMetricsRsp", 24),
];

#[cfg(target_pointer_width = "64")]
#[test]
fn owned_record_sizes_match_the_documented_table() {
    let mut problems = Vec::new();
    let mut seen = BTreeSet::new();
    for table in record_tables().into_iter().filter(|t| !t.is_copy) {
        seen.insert(table.rust_name);
        match OWNED_SIZES
            .iter()
            .find(|(name, _)| *name == table.rust_name)
        {
            Some((_, size)) if *size == table.size => {}
            Some((_, size)) => problems.push(format!(
                "{}: size_of is {} but the table says {size}",
                table.rust_name, table.size
            )),
            None => problems.push(format!(
                "{}: owned record without a size entry (size_of is {})",
                table.rust_name, table.size
            )),
        }
    }
    for (name, _) in OWNED_SIZES {
        if !seen.contains(name) {
            problems.push(format!("{name}: size entry without an owned record"));
        }
    }
    fail_if_any("owned record sizes", &problems);
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

#[test]
fn every_classified_type_is_accounted_for() {
    let classification = load_classification();
    let rows = load_inventory();
    let records: BTreeSet<&str> = record_tables().iter().map(|t| t.c_type).collect();
    // Wrapper structs folded into a list of their parent record have no record of their own.
    let folded_wrappers: BTreeSet<&str> = record_tables()
        .iter()
        .flat_map(|t| t.fields.iter())
        .filter_map(|f| f.map.cross_fold.map(|(wrapper_type, _)| wrapper_type))
        .collect();
    let em_dm_enums: BTreeSet<&str> = enum_tables().iter().map(|t| t.c_type).collect();
    let externals = external_enums();
    let external_enum_types: BTreeSet<&str> = externals.iter().map(|e| e.c_type).collect();
    let external_types: BTreeSet<&str> = EXTERNAL_STRUCTS
        .iter()
        .map(|e| e.0)
        .chain(external_enum_types.iter().copied())
        .chain(["em_interface_name_t"])
        .chain(BYTE_ALIASES.iter().map(|(alias, _)| *alias))
        .chain(TYPE_ALIASES.iter().map(|(alias, _)| *alias))
        .collect();
    let inventory = inventory_types(&rows);
    let mut problems = Vec::new();

    let em_dm_accounted = |ty: &str| {
        records.contains(ty)
            || folded_wrappers.contains(ty)
            || em_dm_enums.contains(ty)
            || STRING_ALIASES.iter().any(|(a, _, _)| *a == ty)
            || TYPEDEFS.iter().any(|(n, _, _, _)| *n == ty)
            || PRIMITIVES.iter().any(|(c, _, _)| *c == ty)
            || ty == "char"
            || DEFERRED.iter().any(|(d, _)| *d == ty)
    };
    for class in &classification {
        let ty = class.ty.as_str();
        match class.owner.as_str() {
            "em-dm" if !em_dm_accounted(ty) => problems.push(format!(
                "{ty}: em-dm type is neither implemented nor deferred"
            )),
            "em-types" if !external_types.contains(ty) => {
                problems.push(format!("{ty}: em-types type has no external mapping"));
            }
            "em-dm" | "em-types" => {}
            _ => {
                if records.contains(ty) || em_dm_enums.contains(ty) {
                    problems.push(format!(
                        "{ty}: owned by {} but defined in em-dm",
                        class.owner
                    ));
                }
            }
        }
    }
    let classified: BTreeSet<&str> = classification.iter().map(|c| c.ty.as_str()).collect();
    let owner_of = |ty: &str| {
        classification
            .iter()
            .find(|c| c.ty == ty)
            .map(|c| c.owner.as_str())
    };
    for ty in records.iter().chain(em_dm_enums.iter()) {
        if owner_of(ty) != Some("em-dm") {
            problems.push(format!("{ty}: implemented but not classified em-dm"));
        }
    }
    for (ty, _) in DEFERRED {
        if inventory.contains(*ty) {
            problems.push(format!("{ty}: deferred but present in the inventory"));
        }
        if owner_of(ty) != Some("em-dm") {
            problems.push(format!("{ty}: deferred but not classified em-dm"));
        }
    }
    for ty in &external_types {
        if !classified.contains(ty) {
            problems.push(format!(
                "{ty}: external mapping without a classification row"
            ));
        }
    }
    fail_if_any("classification table and Rust types differ", &problems);
}
