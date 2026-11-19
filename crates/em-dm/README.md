# em-dm

EasyMesh data model: network, device, radio, BSS, STA, op-class, policy, scan results and MLD objects. Each process (controller, agent) holds its own instances; they are kept in step only by CMDUs.

## Upstream source

- `inc/em_base.h` (the record and enum definitions)
- `src/dm/*.cpp`
- `inc/dm_*.h`
- HAL types `wifi_WideBWChannel_t`, `wifi_BeaconReport_t` and `wifi_ieee80211Variant_t`, `wifi_channelBandwidth_t`: `include/wifi_hal_extender.h` and `include/wifi_hal_generic.h` in `rdkb-halif-wifi` @ `1deddfc`

Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.

## Status

Step 2 of note 0007 (records and enums) is implemented. The crate has no models, operations (Step 3), JSON, database or wire codecs. Dependencies: `em-types` only. See [STATUS.md](../../STATUS.md).

## Records

75 records: 73 for the `em_base.h` record types classified `em-dm` (three more classified types are deferred, see Deferrals), and two HAL types (`WideBwChannel`, `BeaconReport`). Rust names are idiomatic; each doc comment cites the C type and line. 10 enums are open newtypes; 3 more (`MediaType`, `FreqBand`, `ProfileType`) and `Interface`, `MacAddr` come from `em-types`.

| Group (module) | Records |
|---|---|
| network | `InterfacePreference`, `NetworkInfo`, `NetworkSsidInfo`, `Ssid2VidMapInfo` |
| device | `DeviceId`, `Ieee1905Security`, `ExtDeviceInfo`, `DeviceInfo`, `Ieee1905SecurityCap`, `Ieee1905SecurityInfo` |
| radio | `RadioId`, `MediaSpecData`, `RadioInfo`, `MetricCltnInterval`, `OpClassId`, `OpClassInfo`, `CacCompRprtPair`, `CacCompInfo` |
| radio_cap | `ApHtCap`, `ApVhtCap`, `ApHeMcsMaps`, `ApHeCap`, `Wifi6CapRoleHead`, `Wifi6CapRoleTail`, `Wifi6RoleWire`, `RadioWifi6CapData`, `Wifi7MloCapSupport`, `Wifi7FreqRecord`, `Wifi7FreqRecords`, `Wifi7MloCapRecords`, `Wifi7AgentCap`, `EhtOperationsBss`, `EhtOperationsRadio`, `EhtOperations`, `ScanCapOpClassInfo`, `ChannelScanCapRadio`, `ApRadioAdvancedCap`, `Profile2ApCap`, `CacOpClass`, `CacCapMethod`, `CacCapRadio`, `RadioCapInfo` |
| bss | `BssId`, `BssInfo` |
| policy | `PolicyId`, `Ieee8021qSettingsPolicy`, `TrafficSeparSsidInfo`, `TrafficSeparationPolicy`, `QosMgtPolicy`, `BackhaulBssConfigPolicy`, `LinkStatsAlarmCfg`, `ClientFiltersCfg`, `Policy` |
| sta | `AlarmSamples`, `LinkReport`, `WideBwChannel` (`wifi_WideBWChannel_t`), `BeaconReport` (`wifi_BeaconReport_t`), `StaInfo` |
| scan | `ScanResultId`, `Neighbor`, `ScanResult` |
| mld | `AffiliatedApInfo`, `ApMldInfo`, `AffiliatedBstaInfo`, `BstaMldInfo`, `AffiliatedStaInfo`, `AssocStaMldInfo`, `TidToLinkMapInfo`, `TidToLinkInfo`, `MloPolicy` |
| unassoc | `UnassocQueryChannel`, `UnassocQueryOpclass`, `UnassocQueryList`, `UnassocStaMetricEntry`, `UnassocStaMetricsRsp` |

## Representation rules (0007 D3)

- **`Copy` records**: at most 256 bytes (`COPY_SIZE_LIMIT`), made only of `Copy` parts. Checked at compile time by `record!` and again by the golden test.
- **Owned records**: larger than 256 bytes, or holding a `BoundedVec` or an `em_types::Interface`. They are neither `Clone` nor `Copy`; a compile-fail doctest per record proves both, next to a compiling positive control. Move or borrow them; there is no `.clone()` in the workspace.
- **Large records are boxed in collections.** Records above 256 bytes (see the table below) should be stored as `Box<Record>` in any collection or map, so the container stays small and moves are cheap. Smaller owned records may be stored inline. The doc comment of each large record repeats this.
- **Large inner arrays are never inline.** C arrays with a count field (`qos_mgt[256]`, `vendor_elements[2310]`, `frame_body`, ...) are a `BoundedVec<T, CAP>`: the C count field is the length, `CAP` is the C array size, and `push` at capacity is an error that leaves the list unchanged (upstream defect 1).
- **Cross-struct fold**: `em_scan_cap_op_class_info_t` holds its channels in a one-field wrapper struct (`em_per_op_class_channels_list_t`). `ScanCapOpClassInfo::channels` is one `BoundedVec<u8, 64>` that stands for `num`, `channels` and the wrapper's `channel`, so a count above 64 cannot be represented (upstream clamps the count when it reads, `src/em/capability/em_capability.cpp:1448-1453`, and loops over it when it writes, `src/em/em.cpp:1757-1761`). There is no `PerOpClassChannelsList` type.
- **STA frame body**: `StaInfo::frame_body` is a `BoundedVec<u8, EM_MAX_FRAME_BODY_LEN>` with `frame_body_len` folded in. `EM_MAX_FRAME_BODY_LEN` = 11 426 (`inc/em_base.h:74`). The constructor (`BoundedVec::try_from_vec`) and `StaInfo::set_frame_body` accept 11 426 bytes and reject 11 427; a rejected set keeps the old frame.
- **Bit-fields**: 1-bit fields are `bool`; wider ones are `UBits<WIDTH>`, which rejects values of `2^WIDTH` or more.
- **Enums**: open newtypes (`HaulType(u16)` with associated consts). Unknown codes round-trip. Sentinels (`*_max`, `*_unknown`, `*_none`, reserved) are not named consts. Width: the widest wire field that feeds the enum, or for enums with no wire field the narrowest integer that holds every inventory value. `HaulType` and `VapMode` are 16 bits because upstream reads both from `unsigned short` fields of the packed `em_ap_vendor_operational_bss_t` (`inc/em_base.h:1617`, `src/em/config/em_configuration.cpp:1638`, `:1639`); `wifi_ieee80211Variant_t` is 16 bits because its values reach `0x200`. The golden test keeps a per-enum table of the widest wire field (or the inventory range) with upstream cites. The other dual-use enums (`ProductClass`, `DeviceRole`) are read from one byte (`em_configuration.cpp:1324`, `:1327`); the stored-only enums are decoded from text or JSON, not from a wire field.
- **`float`** members are `Float32`, which compares and hashes by bit pattern so records can stay `Eq` and `Hash`. `BOOL` (HAL `unsigned char` flag) is `bool`; `time_t` is `i64`.
- `StaInfo` and `LinkReport` derive `Default` (the zeroed C record) as a starting point; other records are built field by field, since every field is public.

## Byte-string kinds and NUL rules

| Type | Meaning | Embedded NUL | Capacity rule |
|---|---|---|---|
| `CStrBytes<CAP>` | NUL-terminated C string payload, any bytes except NUL. The default for HAL, wire and credential strings, and every `ssid_t` member. | rejected | `CAP` = C `N - 1` |
| `LenBytes<CAP>` | length-delimited bytes with the C `*_len` field folded in | accepted | `CAP` = C `N`, or `N - 1` when the reader appends a NUL (cited) |
| `Text<CAP>` | UTF-8 text; used only where upstream treats the member as text, with the cite in the mapping | rejected | `CAP` = C `N - 1` |

Over-long input is an error, never truncated (upstream defects 1 and 2). The stored length is one byte, so `CAP` is at most 255; `LenBytes<255>` is exactly 256 bytes.

- `ssid_t` is a 32-byte `CStrBytes` (0007 A1; C payload 31). `em_sta_info_t.ssid` is an `em_long_string_t` and keeps 127 bytes.
- `ExtDeviceInfo::client_id` / `client_secret` are `char[256]`; the reader copies at most 255 bytes and writes a NUL after them (`src/em/config/em_configuration.cpp:1308`, `:1321`), so they are `LenBytes<255>` (C payload `N - 1`).
- `TrafficSeparSsidInfo::ssid` is `char[MAX_WIFI_SSID_LEN]` (33) with `ssid_len`: `LenBytes<33>`.
- **Credentials** (`pass_phrase`, `mesh_sta_passphrase`, `client_id`, `client_secret`) are wrapped in `Secret<T>`: `Debug` prints `Secret(<redacted>)` for any record that holds one, and the bytes are reachable only through `Secret::expose`. `Secret<T>` is `Copy` only when `T` is. A golden test fails if a listed credential is not a `Secret`, or a credential-looking inventory field is not listed.

## Mapping and golden test

`record!` and `open_enum!` (`src/macros.rs`) generate each type together with a `#[cfg(test)]` field table (`TABLE`) from the same declaration, so the table cannot drift from the struct. The golden test (`src/golden.rs`) reads `tests/data/dm_inventory.tsv` and `dm_type_classification.tsv` and checks that:

- every inventory row (fields, `<alias>`, `<mapping>`, enumerators) has exactly one Rust-side mapping, and every mapping has a row; the counts come from the TSV;
- every native field type matches its row (bit-field widths, array bounds, counted lists, enum types, pointer rows);
- each string field records its kind, its C payload (through `<alias>` rows), its wire length (a number with a cite, `none at pin`, or `consumer: <crate>, <cite>`) and its native capacity; native must equal the C payload unless the row carries a decision tag (`0007-A1`, `0001`);
- every enum code matches the inventory; sentinels are marked and not named; each enum is at least as wide as the widest wire field that feeds it (per-enum table with cites);
- every count-like integer member (`num`, `num_*`, `numberof*`, `number_of_*`, `*_num`, `*_count`, `*_len`) is either folded into a `BoundedVec` or `LenBytes` length, or listed in `INDEPENDENT_SCALARS` with a reason and an upstream cite. A new count-like inventory row fails the test until one of the two is done;
- every `none at pin` wire length is backed by a sweep result (`NO_WIRE_SWEEP`): where upstream writes the field instead, or `no writer`. A `text` field's cite must be the writer of that sweep result;
- every mapped Rust field name is the snake_case of its C field name, except the listed renames (`type` to `kind`, `wideBandWidthChannelPresent` to `wide_bandwidth_channel_present`), so a swapped mapping fails;
- every `Copy` type that is not a record (the 10 enums, whose size equals their repr, `UBits`, `Float32`, strings and `Secret` wrappers, the error types) is at most 256 bytes;
- every classified `em-dm` type is implemented or deferred, and types owned by `em-types` are matched by a hand-written external mapping (exhaustive destructuring of `Interface`, enum codes through the public consts).

Only the three types in `golden::DEFERRED` are classified but unmapped; each is listed with its reason.

### Count-like members kept as independent scalars

These are integers whose name looks like a count or a length but that do not size an array of their own struct. Each has its reason and cite in `golden::INDEPENDENT_SCALARS`.

- `em_ap_he_cap_t.sprt_mcs_len` and `em_wifi6_cap_role_head_tlv_t.mcs_nss_num`: byte lengths of the MCS list (the writer derives `len / EM_MIN_HE_MCS_LEN`, clamped to `MAX_MCS`, `src/em/em.cpp:1421`, `:1509`); the maps stay a fixed `[ApHeMcsMaps; 3]`.
- `em_network_info_t.num_of_devices`, `em_radio_info_t.number_of_bss`, `number_of_unassoc_sta`, `em_bss_info_t.numberofsta`, `em_radio_cap_info_t.num_op_classes`: statistics or database values; the counted records live outside the struct.
- `em_profile_2_ap_cap_t.max_vid_count`: a capability value.
- `em_sta_info_t.retrans_count` and `em_neighbor_t.sta_count`: measurements read from the wire.
- `em_op_class_info_t.channel_pref` is not count-like, but note that it shares `num_channels` with `channels` in C. Only `channels` is folded (a `BoundedVec<u32, 64>`); `channel_pref` stays a fixed `[u8; 64]` parallel array, so the Rust record does not tie its used length to `channels.len()`.

### Wire-length sweep

Each string field with a wire reader at the pin carries `<bytes> @ <cite>` (29 fields), or `consumer: em-msg, <cite>` where the reader is in the consumer (7 fields). The 39 fields without a reader carry `none at pin` and a sweep result. The sweep searched `src/em`, `src/agent`, `src/ctrl` and the TLV decoders in `src/dm` for writes into the field from a wire buffer. Findings worth knowing for the wire codecs: `StaInfo` text members are filled by `dm_sta_t::decode_sta_capability` from the stored frame body (hex text, at most 63 IE bytes; `ssid` is copied unchecked up to 255 bytes into 128, upstream defect 2); `ScanResult::timestamp` is copied with a one-byte length into 128 bytes; `em_policy_t.traffic_separ` is never written at the pin (the policy reader fills a same-shaped local struct, accepting 33 bytes and writing a NUL one byte past the member); the BSS AKM lists are filled from AKM Suite Capabilities (count clamped to 10) and from WSC M1 auth-type flags (count not clamped); the vendor policy members `collection_start_time`, `collect_duration` and `managed_sta_marker` are decoded from the wire into a local struct and not stored from it at the pin. The JSON path does store `collection_start_time` and `managed_sta_marker` (`src/dm/dm_policy.cpp:167`, `:88`).

## Deferrals

- **Not implemented in Step 2**: `em_neighbor_info_t` (not reached from the stored roots; belongs to its consumer), `em_target_sta_map_t` (model-map selector; Step 3), `SecurityTypeMap` (static table; to the consumer that ports the security-mode strings with the 0007 D4 labels).
- **Step 3**: list insert capacity, the AKM concatenation helper, key truncation. Step 2 covers the bounded in-record collections and the string constructors only.
- Models and operations (`NodeModel`, `MeshModel`), JSON, database and wire codecs are out of scope.

## Sizes of the owned records

`size_of` on a 64-bit target, asserted by `golden::owned_record_sizes_match_the_documented_table`. "Box" marks records above 256 bytes, which Step 3 should store as `Box<Record>`; the others may be stored inline.

| Record | Bytes | Storage |
|---|---:|---|
| `StaInfo` | 2728 | Box |
| `DeviceInfo` | 1552 | Box |
| `BssInfo` | 768 | Box |
| `RadioCapInfo` | 528 | Box |
| `ExtDeviceInfo` | 520 | Box |
| `NetworkSsidInfo` | 472 | Box |
| `Policy` | 464 | Box |
| `RadioInfo` | 408 | Box |
| `NetworkInfo` | 376 | Box |
| `ScanResult` | 304 | Box |
| `Wifi7AgentCap` | 240 | inline |
| `Wifi7MloCapRecords` | 192 | inline |
| `OpClassInfo` | 128 | inline |
| `LinkReport` | 72 | inline |
| `ApMldInfo` | 72 | inline |
| `EhtOperationsRadio` | 56 | inline |
| `ChannelScanCapRadio` | 48 | inline |
| `QosMgtPolicy` | 48 | inline |
| `AffiliatedApInfo` | 48 | inline |
| `BstaMldInfo` | 48 | inline |
| `CacCompInfo` | 40 | inline |
| `AffiliatedBstaInfo` | 40 | inline |
| `AssocStaMldInfo` | 40 | inline |
| `ScanCapOpClassInfo` | 32 | inline |
| `CacOpClass` | 32 | inline |
| `CacCapMethod` | 32 | inline |
| `CacCapRadio` | 32 | inline |
| `TidToLinkInfo` | 32 | inline |
| `UnassocQueryChannel` | 32 | inline |
| `UnassocQueryOpclass` | 32 | inline |
| `RadioWifi6CapData` | 24 | inline |
| `Wifi7FreqRecords` | 24 | inline |
| `EhtOperations` | 24 | inline |
| `TrafficSeparationPolicy` | 24 | inline |
| `UnassocQueryList` | 24 | inline |
| `UnassocStaMetricsRsp` | 24 | inline |

Most owned records are small because their large arrays live on the heap.

## Lints

The crate denies `large_types_passed_by_value`, `large_stack_arrays` and `large_enum_variant` at clippy's default thresholds (256 bytes, 16 384 bytes, 200 bytes). There is no crate-local `clippy.toml`; the workspace one carries the clone ban.
