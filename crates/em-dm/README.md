# em-dm

EasyMesh data model: network, device, radio, BSS, STA, op-class, policy, scan results and MLD objects. Each process (controller, agent) holds its own instances; they are kept in step only by CMDUs.

## Upstream source

- `inc/em_base.h` (the record and enum definitions)
- `src/dm/*.cpp`
- `inc/dm_*.h`
- HAL types `wifi_WideBWChannel_t`, `wifi_BeaconReport_t` and `wifi_ieee80211Variant_t`, `wifi_channelBandwidth_t`: `include/wifi_hal_extender.h` and `include/wifi_hal_generic.h` in `rdkb-halif-wifi` @ `1deddfc`

Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.

## Status

Steps 2 and 3 of note 0007 are implemented. Step 2 is the records and enums. Step 3 is the models and their operations, described in [Models](#models). The crate has no JSON, database or wire codecs: those belong to the consumers (0007 D1). Dependencies: `em-types` only. See [STATUS.md](../../STATUS.md).

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
- **Large records are boxed in collections.** Records above 256 bytes (see the table below) are stored as `Box<Record>` in any collection or map, so the container stays small and moves are cheap. The models do this. Smaller owned records may be stored inline. The doc comment of each large record repeats this.
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

## Models

The models are in `src/model/`. `NodeModel`, `NetworkModel`, `MeshModel`, `AgentModel` and `ModelDelta` are neither `Clone` nor `Copy`; compile-fail doctests check this.

| Type | Role | Holds |
|---|---|---|
| `NodeModel` | one node | its device, radios and radio capabilities, BSSs, op classes, policies, scan results, the three station maps, the MLD records and the dirty set. Its network is not in it. |
| `NetworkModel` | one network | the network record and its SSID table, stored once per network (plan U1). |
| `AgentModel` | the agent | one `NetworkModel` and one `NodeModel`. |
| `MeshModel` | the controller | nodes keyed by `NodeId`, networks keyed by `NetId` (at most 5), and the global side maps: radio capabilities, 1905 security, SSID-to-VID. |

`NodeModel` holds the stored members of `dm_easy_mesh_t` (`inc/dm_easy_mesh.h:50-101`). The table under "Not stored" in `src/model/node/mod.rs` lists the members it leaves out.

### Role-agnostic access

`ModelAccess` (`src/model/access.rs`) is implemented by `AgentModel` and `MeshModel`. It has `node_ctx(&NodeId)`, which gives `NodeCtx`, `node(&NodeId)`, which gives `NodeRef`, and `nodes()`, which walks the nodes in key order. `NodeCtx` holds `&mut NetworkModel` and `&mut NodeModel`; `NodeRef` holds the shared references. They are split borrows of separate fields, in safe Rust. Both also carry `id`, the `NodeId` the node is stored under. It does not follow later writes to the device's AL MAC, so `NodeRef::id()` can be passed back to `node()`.

`AgentModel` ignores the `NodeId` and returns its one node, as the agent's `get_data_model` ignores its arguments (`inc/em_agent.h:862`). A handler that needs another node's data borrows it again through `node()` between its own uses; it never holds two at once. em-core decides whether `EngineCtx.model` is `&mut dyn ModelAccess`, generic or an enum. em-dm only provides the trait.

### Typed keys and key order

Keys are typed and never formatted (`src/model/keys.rs`): `NetId`, `NodeId { net, al }`, `StaKey { sta, bssid, radio }`, `ScanKey`, `PolicyKey`, `OpClassKey`, `BssKey` and `SsidKey`. Matching is exact, so `Network1` does not match `Network10`. Node lookups are scoped by `NodeId`.

The radio, station and op-class finds by bare MAC are global, as upstream's callers expect (the STA and op-class tables have no network column): `MeshModel::get_radio`, `get_sta`, `put_sta`, `get_op_class` and `put_op_class` return the first match in key order. The method map points to these.

Every collection is a `BTreeMap` or a bounded list, so "first" and "next" walks use key order where upstream uses hash order (plan C3). The SSIDs of one network are the exception: they walk in insertion order, as upstream's array does (plan C3, amended by D1). The method map notes each result that depends on that order.

### Walk cursors

The `get_first_*` and `get_next_*` walks of `MeshModel` (`src/model/mesh/walk.rs`, `records.rs`, `radios.rs`, `networks.rs`) return the row with its owner. The cursor depends on the table:

- Array-backed tables (radios, BSSs and op classes, and their per-agent and pre-set variants, and the network SSIDs) return `Positioned { at, index, item }`, and the matching `next_*` takes `(at, index)`. For SSIDs, `at` is the `NetId`. These tables can hold two rows with one key: `commit_bss` matches the BSSID alone, `update_cac_status_id` can make two op-class keys equal, and an edit through `network_ssid_mut` can give two SSIDs one id. A key cursor would find the first of the two rows again and loop forever, so each row is visited once by its position.
- Map-backed tables (stations, policies and scan results) return `Located { at, key, item }`, and the matching `next_*` takes `at` and the key. The walk yields the key the row is stored under, not one rebuilt from its fields, so a `&mut` edit of a key field does not break the walk (plan D1a).

### Capacities and errors

Each bounded table keeps its upstream limit. An insert into a full table returns `ModelError::Full { table, capacity }` and changes nothing. An update of an existing row always succeeds. Limits:

| Table | Limit | Constant |
|---|---:|---|
| radios | 3 | `MAX_RADIOS` |
| BSSs | 24 | `MAX_BSS` |
| op classes | 64 | `MAX_OP_CLASSES` |
| policies | 32 | `MAX_POLICIES` |
| AP MLDs, and affiliated APs and STAs of one MLD | 64 | `MAX_AP_MLD` |
| associated STA MLDs | 64 | `MAX_ASSOC_STA_MLD` |
| unassociated STAs | 64 | `MAX_UNASSOC_STA` |
| MLO policy | 2 | `MLO_POLICY_HAUL_COUNT` |
| preferences | 5 | `MAX_PREFERENCES` |
| interfaces | 8 | `MAX_INTERFACES` |
| networks | 5 | `MAX_NETWORKS` |
| network SSIDs | 5 | `MAX_NET_SSIDS` |
| neighbours of a scan result | 16 | `ScanResult::neighbor` (`src/records/scan.rs`) |

The STA maps, the scan results and the nodes are unbounded, as upstream's are. Upstream's `set_num_*` setters have no counterpart: the count is the length of the collection.

### Node creation and network lifecycle

`NodeModel::new(NodeInit)` takes the network, the AL MAC, the profile, the role (`NodeRole::Controller` or `NodeRole::Agent`) and the co-located flag. The host lookup that sets the flag stays outside em-dm (0007 D1). The constructor seeds the 8 default policies and the 6 preset op classes (`src/model/node/mod.rs`, `op_class.rs`, `policy.rs`). `NodeModel::from_controller_identity` builds the controller's own node from its identity.

`put_radio` seeds `radio_metrics_rep` and `steering_param` when they are absent. The seeded policies use the node's network id; upstream uses `GLOBAL_NET_ID`, so the two differ only off the global network.

### Radio capabilities

A radio's capability record sits in the slot with the radio's index (upstream's `m_radio_cap[i]` for radio `i`), so the two tables keep the same index. Only `NodeModel::commit_radio` (and so `ModelDelta::EmInsertRadio`) writes a slot for a new radio: the supplied record if its `ruid` is the radio's MAC, else a zeroed default; `commit_radio` returns a `RadioCommit { radio, cap }` that says which (plan D3). `put_radio` appends a radio without a slot. `put_radio_cap` also fills earlier slots that do not exist yet with zeroed placeholders. `remove_radio` removes the slot at the radio's index if that slot exists. `NodeModel::put_radio_cap` finds the stored radio by the `ruid` MAC of the record and writes that radio's slot. A capability for a radio that is not stored is refused with `ModelError::NotFound`; store the radio first.

`MeshModel::create_data_model` and `delete_data_model` create and remove nodes. A node created in a network with no model gets a default one: a zeroed record with that id (plan C7). The 5-network cap applies on every path that creates a network. `remove_network` is refused with `ModelError::NetworkInUse` while the network has nodes.

### Node needed

`MeshModel::put_radio` returns `RadioPutOutcome { put, node_needed }`. Upstream calls `m_mgr->create_node` after each successful put (`dm_easy_mesh_list.cpp:470`). The port returns the arguments of that call as a `NodeNeeded` (the node id, the radio MAC, the band, and the profile and service the call passes) and creates no node. The caller creates the node. The database path reaches the same call, so `MeshModel::update_radio_list` returns `Result<Option<RadioPutOutcome>, ModelError>`: `Some` for an insert, `None` for an update or a delete.

### Dirty set

`DirtySet` (`src/model/dirty.rs`) replaces `m_db_cfg_param`. It holds the upstream `db_cfg_type_t` bits (`DbCfgType`, 23 values, `inc/em_base.h:3316-3341`) and the one criterion upstream callers pass, the radio of `bss_list_delete`. Its methods are `mark`, `mark_bss_delete(Option<MacAddr>)`, `clear`, `is_set`, `any`, `bss_delete_filter` and `bits`.

- Operations do not mark the set; their callers do, as upstream's callers do. The two list-layer marks, the controller-ID mismatch in `put_network` and the co-located creation in `create_data_model`, are made by the mesh operations themselves.
- `bss_delete_filter()` is `None` when the `bss_list_delete` mark carries no radio, which deletes every BSS row of the node. Otherwise it is the radio whose BSS rows are deleted. `BssInfo::matches_radio_filter` ports upstream's `match_criteria`.

### Station maps

`StaMapTarget` (`src/model/node/sta.rs`) names the three station maps: `Assoc` (the pending association map), `Disassoc` and `Consolidated`. The pending map is a staging queue. `NodeModel::take_sta_rows` drains it for the database flush.

The walks take the position of a row, not a key rebuilt from its fields (plan D1). The node-level `first_sta_info`, `next_sta_info`, `first_sta` and `next_sta` yield a `StaEntry { key, row }`, and the mesh walks over stations, policies and scan results yield the stored key in `Located::key`. The array walks (radios, BSSs, op classes and network SSIDs) yield `Positioned::index`. A `&mut` edit of a key field does not move a row. The bulk deletes remove from the node and position they visit. The `MeshModel` module doc lists the APIs that find a row from a record or key by design.

### Deltas

`ModelDelta` (`src/model/delta.rs`) is a model change as a value, for changes that cross an actor or command boundary (plan C1). Its variants and the upstream write each one carries:

| Variant | Upstream write |
|---|---|
| `CsaChannel` | the op-class channel of one radio and type. This fixes the lost update upstream writes to a discarded copy (`src/agent/dm_easy_mesh_agent.cpp:672-674, 764`). |
| `StaLinkMetrics` | `merge_sta_link_metrics` (`dm_easy_mesh.cpp:4215-4227`) |
| `AssocStaMldRefresh` | `replace_assoc_sta_mld_info` (`dm_easy_mesh.cpp:3927-3954`) |
| `StaAggregate { assoc, disassoc }` | `merge_assoc_sta_updates` (`dm_easy_mesh.cpp:4173-4213`) |
| `UnassocResults` | the unassociated-STA metrics response: memset, then fill (`src/agent/dm_easy_mesh_agent.cpp:937-981`) |
| `ScanResults` | `update_scan_results` (`dm_easy_mesh.cpp:3599-3614`) |
| `AlInsert { network, device }` | `commit_config` for the AL target and `al_insert` (`dm_easy_mesh.cpp:207-321`) |
| `EmInsertRadio { radio, cap, op_classes }` | `commit_config` for the radio target and `em_insert` |
| `EmInsertBss { radio, bss, ap_mld }` | `commit_config` for the BSS target |
| `Policies` | `set_policy` for each policy (`dm_easy_mesh.cpp:3393-3426`) |
| `StaCapUpdate` | `apply_sta_cap_to_maps` for each resolved link (`dm_easy_mesh.cpp:4104-4124`) |

`ModelDelta::apply` takes the delta by value and returns a `DeltaOutcome`:

- `applied`: the rows inserted, replaced or removed. Companion rows written by the same operation count too: a new radio counts twice, for the radio and its capability slot; `AlInsert` counts its network record and its device separately; `StaCapUpdate` counts the pending row of each link and its consolidated row when that exists; `EmInsertBss` counts each AP MLD and each affiliated AP written into it.
- `skipped`: one `SkippedRow { table, index, reason }` per row that was not written. `SkipReason` is `Rejected(ModelError)`, `NotAttempted` (the radio table was full, so upstream returns before the op classes; or a capability record was supplied with a radio that is already stored, which upstream never reads; or an affiliated AP whose AP MLD was rejected, so the AP was never tried, with its row in `Table::AffiliatedAp`), `WrongRadio` (an op class or BSS of another radio, or a capability record whose `ruid` is not the radio's MAC), `NoAffiliatedAps`, or `AffiliatedApFull { ap_mld }`. The last one is an affiliated AP that is not stored because its AP MLD already has 64; its row is in `Table::AffiliatedAp`, and `ap_mld` is the position of the AP MLD in the delta. The AP MLD and the other affiliated APs are still written.
- `links_updated`: for `StaCapUpdate`, the number of links written. The caller marks the station list as upstream does.

The row rule (plan C1): the rows that land and the rows that are skipped match upstream. A skipped row is reported and is never written partially. There is no variant for `set_ssid`, `set_radio` or `dev_test`, and `reset` is `delete_all_data_models` (0002 rev 7 erratum).

`StaCapUpdate::new(links, frame)` takes the link keys that the caller has resolved. For an MLD station the caller resolves the links (`src/ctrl/em_ctrl.cpp:328-363`); for a single station it passes one key.

### Sanctioned copies (F1, F1b)

Upstream keeps a Client Capability Report frame in the pending station row and again in the consolidated row, once per link. The port makes that copy in one place, `copy_frame_f1` (`src/model/node/sta.rs`), which is the only function in em-dm that duplicates frame bytes this way. `copy_sta_row` (same file) copies a whole station row into the pending map on the association path (decision F1b). Both use the crate-private `BoundedVec::copy_of`. Nothing else in em-dm duplicates owned data.

### E4 table

`E4_TABLE` (`src/model/e4.rs`) holds the 27 rows of the channel and op-class table that the derived reads `get_*_by_opclass` and `get_*_by_bandwidth` use (`dm_easy_mesh.cpp:2577-2622`). Those reads take no model state, so they are free functions.

## Intentional differences

Each tag below marks rows of `tests/data/upstream_test_map.tsv`; the count is the number of rows with that tag. Each test's doc comment names the upstream behaviour it departs from.

| Tag | Rows | Difference |
|---|---:|---|
| `u1-network-stored-once` | 2 | The network record and SSID table are stored once per network, so every node of the network reads one row (U1). |
| `u2-removes-implemented` | 5 | Upstream's `remove_network`, `remove_radio`, `remove_sta` and `remove_op_class` are empty, and `remove_device` uses a key that never matches. Every remove deletes its record (U2). |
| `c3-key-order` | 3 | Walks use key order, not hash order (C3). |
| `c4-exact-keys` | 2 | Matching is exact, with no prefix match (C4). |
| `c4-global-finds` | 1 | Radio, station and op-class finds by bare MAC are global, as upstream's callers expect (C4). |
| `c4-keys-from-records` | 1 | A key is derived from the record, not kept apart from it (C4). |
| `c4-typed-keys` | 1 | A typed key is not parsed, so a network id with `@`, `#` or `$` is ordinary text (C4). |
| `c6-capacity` | 1 | A bounded table refuses rows beyond its capacity (C6). |
| `c6-count-rule` | 26 | No `set_num_*` setter; the count is the length of the collection. An over-range count becomes the capacity bound (C6). |
| `c7-network-lifecycle` | 1 | A node creates its network when there is none, and a network with nodes cannot be removed (C7). |
| `index-bounds` | 6 | `None` at and beyond the count, where upstream returns a zeroed or stale slot. |
| `a4-ssid-identity` | 1 | An SSID row's identity is its own id, not the key of the put (plan A4). |
| `a5-policy-net-id` | 1 | Seeded policies carry the node's network id, not `GLOBAL_NET_ID` (plan A5). |
| `a6-node-needed` | 1 | `put_radio` returns a `NodeNeeded` and creates no node (plan A6). |
| `commit-bss-source-by-value` | 1 | The source BSS is passed by value, so an out-of-range index cannot occur. |
| `defect3-ctor-identity` | 1 | The identity fields are set one by one, and no `memcpy` overwrites the network id (0007 defect 3). |
| `vacuous-upstream-test` | 3 | Upstream's assertions cannot run from its fixture, or the test is a smoke test. The port asserts real state. |

Differences without a test tag, each with a test or a doc comment:

- Dirty set (C5): clearing a bit also clears its radio filter. Upstream keeps a stale `radio=` string, so a later mark with `""` still filters. The unterminated copy and the loop on type 0 cannot occur (`src/model/dirty.rs`; tested in `model::tests::dirty`).
- Station record decode (`src/records/sta.rs`): an SSID element above 127 bytes leaves `ssid` empty, where upstream overflows its field (defect 2). An element too long for the hex text leaves its field unchanged.
- Frame bodies: a body above 11 426 bytes is rejected and never clamped, and the old frame is kept (0007 D4).
- `update_scan_results`: the null dereference of upstream (`dm_easy_mesh.cpp:3601`) cannot occur.
- Bulk removal and walk routing (plan D1b, tag `d1-storage-route` in `dm_method_map.tsv`): `delete_radio_list`, `delete_op_class_list`, `delete_policy_list`, `delete_scan_result_list`, `delete_network_ssid_list` and `apply_sta_mld_association` remove from the node and row they visit, by position or stored key. Upstream rebuilds the key from the record there.
- Co-located controller mark (plan D4(b), tag `d4-controller-by-role`): the write that marks the co-located controller goes to the controller node stored under the network record's controller MAC. If there is none, it goes to the first controller node of that network in key order, and if there is none of those, nothing is marked. Upstream looks the node up by the controller MAC and asserts (`dm_easy_mesh_list.cpp:1738-1739`). The lookup falls back because the record's controller MAC and the node's key can part ways.
- Scan-list index (review finding DM3-02): `update_scan_result_list` rejects an index that is neither a neighbour index nor `SCAN_RESULT_SELF_INDEX` with `ModelError::NotFound`, before any change. A self-index insert on an existing result writes no neighbour and appends only the station's self report. Upstream reads `neighbor[0xFFF]` out of bounds there (`dm_easy_mesh_list.cpp:1565-1583`, `inc/dm_scan_result_list.h:27`).
- Self-scan beacon report (plan D5(b), tag `d5-self-report-cleanup`): `delete_scan_result_list` also removes a station scanner's zero-BSSID beacon report, once per removed result. It removes the first such report with the op class and channel of that result. The neighbour loop skips zero BSSIDs, so this is the only bulk path that removes the self report. Upstream's bulk delete leaves it in place.
- Neighbour beacon reports of a bulk clear (review finding DM3-07, part of the D1b difference, tag `d1-storage-route`): for each neighbour of a removed result, `delete_scan_result_list` removes the first report with the neighbour's BSSID and the op class and channel of the result. Upstream's BSSID-only match can remove the report of another channel. The single-row `remove_scan_result` keeps the BSSID-only match and removes the first report (`dm_easy_mesh_list.cpp:1493-1520`).
- Self-index delete (review finding ADV4-01, tag `d5-self-report-cleanup`): `update_scan_result_list` with `Delete` at `SCAN_RESULT_SELF_INDEX` removes the zero-BSSID report with the op class and channel of the result. Upstream matches the zero BSSID alone and removes the first one.
- Rejected AP MLD (review finding DM3-09): when `EmInsertBss` has an AP MLD that the model rejects, each affiliated AP it offered is listed as `SkippedRow { table: AffiliatedAp, reason: NotAttempted }`, as the radio path lists the rows it does not reach. Upstream reports nothing.

## Method map and test map

The data files are in `tests/data/`. The coverage tests in `src/model/tests/maps.rs` read them.

- `dm_upstream_methods.tsv`: the 232 definitions of `dm_easy_mesh_t`, one row per definition. Overloads are split (A3).
- `dm_list_methods.tsv`: the 91 list rows: the 62 members of `dm_easy_mesh_list_t` and the model residue of the `dm_*_list_t` classes.
- `dm_method_map.tsv`: 327 rows, with the columns `upstream`, `scope`, `disposition` and `notes`. Scope `node` has 236 rows: the 232 definitions, plus `decode_sta_capability`, `copy_sta_info`, `set_frame_body` and `dm_bss_t::match_criteria`. Scope `list` has 91 rows. A disposition is `rust:<file>::<Type>::<fn>`, which a test checks against the source, or `not-ported:<reason>`.
- `upstream_test_map.tsv`: 334 rows, one per upstream test in the node triage (179) and the list triage (155). The disposition is `rust:<module path>`, which names a Rust test, or `skip:<reason>`. The tag is `parity` or `intentional-difference:<id>`.
- `node_test_triage.tsv`: the 179 upstream node tests that are ported or decided, with the verdict PORT (173), PORT-CTOR (5) or SKIP-TRIVIAL(ambiguous-check) (1). The 28 rows with method `(unmatched)` were triaged by hand. The node rows of the test map match this file exactly (checked by a test in `model::tests::maps`).
- `list_test_triage.tsv`: the 155 list tests, one class each: PORT, PORT-REWRITE, SKIP-UB or SKIP-STALE-API.
- `tools/check-test-map` (repository root) lists every Rust test named in the test map that `cargo test -p em-dm --lib -- --list` does not list. It exits 0 when none is missing, 1 when some are missing, and 2 on a usage or I/O error.

## Deferrals

Not implemented here, by design:

- `em_neighbor_info_t`: not reached from the stored roots. It belongs to its consumer.
- `SecurityTypeMap`: a static table, for the consumer that ports the security-mode strings with the 0007 D4 labels.
- `update_bsta_mld_info`: an empty upstream `TODO` (`dm_easy_mesh.cpp:3822-3826`), not ported. The BSTA MLD record is written through `NodeModel::bsta_mld_info_mut`.
- The partial `reset` of `dm_easy_mesh_t`: it has no caller in the port. The owner drops the node, and the command path is `delete_all_data_models`.
- JSON, the database (rows, `sync_db`, `update_tables`), TR-181, DPP, webconfig, host interface lookups (`getifaddrs`, `SIOCGIFHWADDR`) and wire or TLV decoding stay with their consumers (0007 D1).

Deferrals from Steps 2 and 3, each resolved or re-deferred:

- **List insert capacity: resolved.** A full table refuses the insert with `ModelError::Full` and keeps its contents. The refusal is in `BoundedVec::push` wherever a fixed array is filled, and in the explicit checks of `put_radio`, `put_policy`, `put_bss`, `put_scan_result`, `put_network_ssid` and `put_network`. The limits are in the table above.
- **AKM concatenation helper: re-deferred to em-db.** Upstream joins the front and back AKM names of a BSS into comma-separated text with `strncat` into an `em_long_string_t` (`src/dm/dm_bss_list.cpp:230-238`). That text is a database row, and database rows are outside em-dm (0007 D1). In em-dm the AKM lists are bounded lists of 10 entries (`src/records/bss.rs`, `src/records/network.rs`), so the model cannot overflow. em-db must bound its own join.
- **Key truncation: resolved.** Keys are typed and never formatted (`src/model/keys.rs`). The 64- and 256-byte truncation of the formatted keys (`dm_easy_mesh_list.cpp:1643-1660`) has no counterpart, and the exact matching also removes the prefix match.
- **`em_target_sta_map_t`: resolved** by `StaMapTarget` (`src/model/node/sta.rs`). The Step 2 golden test still lists the type in its `DEFERRED` table (`src/golden.rs:68`); that table is test code, which Step 3 did not change.


## Sizes of the owned records

`size_of` on a 64-bit target, asserted by `golden::owned_record_sizes_match_the_documented_table`. "Box" marks records above 256 bytes. The models store those records boxed (`NodeModel` holds `Box<StaInfo>`, `Box<BssInfo>` and `Box<RadioInfo>` values, for example), and the delta and handoff types carry the box. The others may be stored inline, and the models do so.

The model types are not records, so this table does not list them. `src/model/tests/sizes.rs` asserts that `ModelDelta`, its parts, the keys, `NodeCtx`, `NodeRef`, `AgentModel`, `NetworkModel`, `MeshModel` and every insert, removal and result type are at most 256 bytes, and that `NodeModel` is above 256 bytes and is handed off boxed. The comments in `sizes.rs` and `agent.rs` give 680 bytes for `NodeModel` and 160 for `NetworkModel`; the tests assert only the limits.

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
