# 0007: Scope and design rules for `em-dm`

| | |
|---|---|
| Status | **Accepted** (rev 3; final fixes after review round 2 checked against the pinned source by an independent check, not reviewed: two-round limit) |
| Date | 2026-11-19 |
| Upstream | `rdkcentral/unified-wifi-mesh` @ `ba4180ba36748629f10c20babc4320dd69c9db4f` |
| Affects | `em-dm`; `em-msg`, `em-db`, `em-ctrl`, `em-agent`, `em-easyconnect` (boundaries); note 0002; `docs/em_base-coverage.md` |

Rev 3 changes:

- Corrected the list count, host-interface callers, assignment defect and persistence wording.
- Clarified test ownership and recorded the defect-fix decision.
- Record ownership decided by the user: field by field, large records on the heap (D3).
- Controller-side ownership decided by the user: one controller actor owns all node records and
  the network SSID configuration (D2).
- Type classification decided by the user: Step 1 produces the per-type table and the user
  approves it before Step 2 (D7 item 2).
- AP MLD exact-capacity SSID decided by the user: option 3 (D7 item 4).

Rev 3, review round 1 fixes:

- Defined AP MLD C-string lookup and transitive stored-type inventory.
- Bounded large-record handoffs and specified STA frame ingress, storage and report reads.
- Added late-join SSID coverage and an unambiguous security-label mapping.

Rev 3, final fixes after review round 2:

- Defined the external header baseline, target ABI and approval before inventory generation.
- Named the model roots and required classification of every existing `em-dm` coverage row.
- Clarified unresolved AP MLD updates and placed size checks with the types they check.

## Context

`STATUS.md` maps `em-dm` to `src/dm/`. A fact-finding pass and review round 1 over the pinned tree
found:

- **35 class definitions** in `inc/dm_*.h`; 33 `src/dm` sources (15 487 lines), plus
  `dm_easy_mesh_ctrl.cpp` (10 896 lines) and `dm_easy_mesh_agent.cpp` (1 337) outside `src/dm`.
- **19 record-like classes.** Most wrap one `em_*_info_t` from `em_base.h` with a copy
  constructor, `operator=`, `operator==` and cJSON `decode`/`encode`. Exceptions: `dm_dpp_t` wraps
  `ec_data_t` (which holds OpenSSL key and big-number pointers, `inc/ec_base.h:638-663`),
  `dm_scan_result_t` wraps `em_scan_result_t`, and `dm_ssid_2_vid_map_t` is a DB wrapper
  (`set_config`/`get_config`).
- **12 specialised list classes defined** (`dm_*_list_t`, inheriting the record, `db_easy_mesh_t`
  and, except `dm_neighbor_list_t`, `tr_181_t`); 11 are built into the controller
  (`dm_neighbor_list.cpp` is not), plus the topology list `dm_easy_mesh_list_t`
  (`inc/dm_easy_mesh_ctrl.h:44-48`, `src/ctrl/Makefile.am:124-152`).
- **`dm_easy_mesh_t`** (`inc/dm_easy_mesh.h:49`, 2768-line header, 236 member functions
  counting constructors and overloads): one node's view, with fixed arrays of records, five raw
  `hash_map_t *` of heap records (`:79-83`), a back-pointer `em_t *m_em` (`:87`), a OneWifi
  `webconfig_subdoc_data_t *` (`:60`) and one mutex (`:56-57`).
- **`dm_easy_mesh_list_t`**: all node models, inserted and looked up by **network id and AL MAC**
  (`dm_easy_mesh_list.cpp:1720, 1800-1815`), with `em_mgr_t *` and controller-interface
  back-pointers (`inc/dm_easy_mesh_list.h:28-33`).
- `docs/em_base-coverage.md` assigns 47 `em_base.h` entries to `em-dm` (10 enums, 37 structs).
  Two are direct wire overlays (`data_elem_attr_t`, `em_ieee_1905_security_cap_t`), and some enums
  are wire codes too (`attr_id_vendor_ext`, the vendor-extension haul type, extended-device
  product class and role bytes; `em_configuration.cpp:1301-1332, 4437-4438, 5917-5924`).
  Conversely, the model **stores** records that coverage gives to `em-msg` (for example the
  native scan row `em_scan_result_t` and its key, filled from a decoded
  `em_channel_scan_result_t`, `em_channel.cpp:2426-2450`).
- **Record sizes vary widely** (estimated from the pinned declarations): STA about 4 KiB plus ten
  HAL beacon reports, plus a heap-owned frame body up to 11 426 bytes (`EM_MAX_FRAME_BODY_LEN`,
  `em_base.h:74`; IEs parsed into `malloc`ed tags, `dm_sta.cpp:494-502`); BSS about 1.9 KiB plus
  vendor IEs; radio capability about 10 KiB; **policy about 49 KiB** (`qos_mgt[EM_MAX_STA_PER_AGENT]`,
  `em_base.h:3808-3848`). Exact HAL-dependent sizes cannot be verified from these declarations.
- **String fields** are NUL-terminated `char[N]` (N−1 payload bytes) in most places, but not
  uniformly: `MAX_WIFI_SSID_LEN` is `32 + 1`; AP MLD copies a wire length equal to
  `sizeof(ssid_t)` and forces the last byte to NUL; the extended-device reader accepts up to 255
  raw bytes into 256-byte buffers without UTF-8 checks and stores unknown product-class/role codes
  (`em_configuration.cpp:1298-1332, 2776-2804`, `dm_network_ssid.cpp:45-51`).
- **Tests:** 1 460 `TEST` + 155 `TEST_F` in 20 dm test files, mostly field-level `EXPECT_EQ`. They
  need OneWifi/HAL headers, so they are a reading oracle here, not a runnable one.

### Upstream ownership, and the clash with 0004

`em_cmd_t` holds the whole model by value (`inc/em_cmd.h:42`); `init(dm)`, `clone()` and
`clone_for_next()` assign it again (`src/cmd/em_cmd.cpp:196-250`). The assignment is selective:
it copies policy and consolidated STA rows, omits pending association/disassociation and scan
maps, and copies `m_em` as a pointer (`dm_easy_mesh.cpp:62-164`). The copy constructor is deleted
because a member-wise copy would double-free the hash maps (`inc/dm_easy_mesh.h:2045-2049`).
0004 R2 (no `.clone()`) and R3 (one owner per state) rule this pattern out; note 0002 must decide
what a command carries instead (see D5).

### Upstream defects found (`src/dm/`, corrected after review)

1. **Bounds:** BSS (`dm_easy_mesh_list.cpp:665-670`), network (`:157-158`) and repeated
   preference writes (`dm_easy_mesh.cpp:919-946`) have no capacity check;
   `get_interfaces_list` checks capacity only after the first write, so a zero capacity overflows
   (`:2245-2261`).
2. **Strings:** `:157` (`dm_easy_mesh_list.cpp`) and `dm_easy_mesh.cpp:4157` can omit the
   terminator; `dm_easy_mesh_list.cpp:954` and `:1755` can overflow their destinations; the AKM
   concatenations can overflow (`dm_bss_list.cpp:230-238`); the 64-byte create/delete key can
   truncate a name that lookup formats into 256 bytes (`dm_easy_mesh_list.cpp:1643-1660, 1720,
   1810-1814`).
3. **Constructor `memcpy`:** copies `sizeof(em_interface_t)` from the controller id over the start
   of `m_device_info.id.net_id` (wrong type); the two backhaul copies are type-correct
   (`dm_easy_mesh.cpp:4318-4327`).
4. **`"WEP_64"` for WPA-PSK** in `securitymode_to_str`, mapped back the same way by the reverse
   helper; no production caller of the formatter at the pin (`dm_easy_mesh.cpp:2104-2139`).
5. STA-map accessors run without the mutex others take (`dm_easy_mesh.cpp:3006-3025` and more);
   four list-owned hash maps are never destroyed (`dm_easy_mesh_list.cpp:1819-1834`,
   `dm_ieee_1905_security_list.cpp:256`, `dm_radio_cap_list.cpp:260`, `dm_ssid_2_vid_map.cpp:213`);
   the five node maps are destroyed in `deinit()`.

## Decision

### D1. What `em-dm` contains

Synchronous, depends on `em-types` (and `em-utils` only if needed), no `unsafe`, no clones
(0004 R1, R2).

| Part | Contents | Replaces |
|---|---|---|
| Records | native Rust records for model state, including scan results and their keys; allocation of referenced types, including MLO and unassociated-STA state, follows Step 1 and D7 | stored `em_*_info_t` and friends; the record halves of `dm_*_t` |
| Node model | `NodeModel`: one node's network, device, radios, BSSs, op classes, MLD records, policy, CAC, STA tables, scan results | the data of `dm_easy_mesh_t` |
| Mesh model | `MeshModel`: topology keyed by **(network id, AL MAC)**; one controller actor owns all controller-side node records and the network SSID configuration (D2) | the data of `dm_easy_mesh_list_t` |
| Operations (Step 3) | find / insert / update / remove / merge, as `&mut self` methods | the matching `dm_easy_mesh_t` / list methods |

**Boundaries:**

| Upstream | Goes to |
|---|---|
| TLV framing, wire-only codes, TLV body ↔ record conversion | the per-consumer boundary of note 0005 (`em-msg` bodies + the `em-core` area that uses them); native and dual-use types are classified in Step 1 (D7) |
| `data_elem_attr_t` (WSC attribute header) | `em-msg` (already walked by its WSC selector) |
| SQL rows (`db_easy_mesh_t`, columns, `sync_db` hydration) and the SSID-to-VID DB wrapper | `em-db` (depends on `em-dm`; upstream's inheritance runs the other way); the SSID-to-VID **record** stays in the model |
| cJSON `decode`/`encode`, `get_config` JSON, TR-181 | consumers: `em-ctrl`, `em-cli` (decided: not in `em-dm`) |
| DPP data (`ec_data_t`, keys, bootstrap material) | `em-easyconnect`; the controller passes an owned payload or reference id across the note 0002 boundary |
| OneWifi webconfig (`m_wifi_data`), `analyze_*` | `em-agent` / `em-cmd` |
| Host-interface discovery: `get_interfaces_list` (`getifaddrs`) is called during controller AL-SAP registration (`em_ctrl.cpp:1708`) and on the controller's `decode_config` `"Reset"` path (`dm_easy_mesh_ctrl.cpp:5636-5639`, `dm_easy_mesh.cpp:853-864, 894-901`). `name_from_mac_address` also calls `getifaddrs` and is used in model creation (`dm_easy_mesh.cpp:2301-2308, 4318-4327`, `dm_easy_mesh_list.cpp:1731-1734`). | a shared host-interface adapter at the integration boundary; `em-dm` keeps only pure selection logic over the discovered list |
| Mutex, back-pointers `m_em`, `m_mgr` | gone: one actor owns the model; messages instead (0004 R3) |

### D2. Ownership rules

- `NodeModel` and `MeshModel` implement neither `Clone` nor `Copy`; mutable model state has one
  actor owner, and `&`/`&mut` stay inside that owner (0004 R3).
- **Controller side (decided by the user, 2026-11-19):** one controller actor owns `MeshModel`:
  every controller-side node record and the network-wide SSID configuration. The SSID
  configuration is stored once per network and node records refer to it by key, so upstream's
  fan-out copy (`*pnet_ssid = *net_ssid`, `dm_easy_mesh_list.cpp:959-986`) and the copy on node
  add (`dm_easy_mesh_list.cpp:1782-1801`) have no Rust counterpart: an SSID change is one update
  in the owner and is visible to every matching node. `em-core` per-node tasks hold no records;
  they send read and update requests to the owner, and the message that starts their work
  carries a small `Copy` snapshot of the fields it needs. Any index outside the owner holds keys,
  not records. `em-db` persistence is requested by the owner.
- **Agent side:** the agent's own actor owns its single `NodeModel`.
- Upstream's "copy the model into the command" is not ported; D5 lists what note 0002 must cover.
- Fixed upstream arrays become collections whose upstream limits are **enforced**: inserting a new
  row at capacity is an error; updating an existing row is not.

### D3. Record representation: ownership chosen field by field

Decided by the user (2026-11-19). It replaces rev 1's "all records `Copy`" (option A, withdrawn):
that does not fit the estimated sizes from the pinned declarations, and a record holding a `Vec`
cannot be `Copy` at all.

- **Small values are `Copy`:** MACs, ids and keys, enums, counters, and short fixed-capacity
  strings/byte strings. 0004 R2 allows `Clone` only on such `Copy` types.
- **Large records are owned, neither `Clone` nor `Copy`:** STA, BSS, radio capability, policy and
  any record above about 256 bytes or holding a variable-length buffer. They are moved at
  ownership boundaries (decode → model insert) and borrowed for reads and serialisation.
- **Large records live on the heap, with bounded values at handoffs:** a collection's buckets
  live on the heap, but inserting an inline `Record` still moves the whole value. A record whose
  inline representation would exceed 256 bytes is stored as `Box<Record>` (for example,
  `HashMap<Key, Box<Record>>`), or split so its inline stored and transferred representation is
  at most 256 bytes and its large payloads are out of line. Allocate the large payload before
  the decode-to-insert handoff; the same bound applies to values moved into or out of the
  collection. Large inner arrays, such as the policy's per-STA QoS array, are a `Vec` or boxed
  slice, not inline arrays. Step 2 checks record representations and treats clippy's
  `large_types_passed_by_value`, `large_stack_arrays` and `large_enum_variant` as errors. Step 3
  checks the actual stored, insert and removal value types; each consumer checks its own decode
  handoff type when it is introduced (criterion 4).
- **Variable data is owned and moved:** the STA frame body is a `Vec<u8>` (bounded by
  `EM_MAX_FRAME_BODY_LEN`) moved into the model, with IEs parsed into borrowed views, not copies.
- **Static tables** (e.g. the security-type map) use `&'static str`.
- **Named ownership boundaries** only (decode → insert, query → borrow, command handoff per note
  0002). No conversion exists merely to duplicate owned data (0004 R2).

### D4. Input limits and parity

- For every string/byte field, the Step 1 inventory records three numbers separately: the
  **C payload capacity** (N−1 for a NUL-terminated `char[N]`), the **wire length** accepted by the
  consuming reader, and the **native capacity** chosen in Rust. Byte-oriented ingress (SSIDs,
  extended-device credentials, ...) keeps raw bytes: these fields are byte strings, not UTF-8
  strings, subject to the AP MLD boundary decision in D7. Text-oriented fields may be UTF-8 where
  upstream treats them as text.
- Unknown enum codes are **preserved** where a consumer stores them (open newtype, as in
  `em-types`), not rejected.
- Subject to the type classification and AP MLD decision in D7, field sets, retained enum values
  and accepted limits match upstream except for the intentional differences below.
- **Intentional differences:** capacity is enforced with an error (defect 1); over-long input is an
  error where it exceeds the approved native limit (defect 2); a 33-byte AP MLD SSID whose last
  byte is not NUL is rejected where upstream truncates it (D7 item 4, subject to Step 1 confirming
  `sizeof(ssid_t)`); identity fields are initialised explicitly from
  the controller identity, including the network id where the consumer needs it (defect 3, fixed
  in Step 3 after reading the controller command consumers); records preserve every field, including
  `ctrl_id.media` (which upstream's assignment copies, `dm_network.cpp:192`); no shared mutable
  state, and owned maps are released with their owner (defect 5). Defect 4: if a consumer ports
  the security-mode strings, it uses the mappings below and tests both directions (decided by the
  user; no production caller of the formatter at the pin).

- **STA frame-body overflow:** the Client Capability Report ingress rejects a body longer than
  `EM_MAX_FRAME_BODY_LEN` (11 426 bytes) before sending any model update. The native constructor
  and every storage update independently enforce the same limit and return an error before
  replacing an existing frame; neither boundary stores a prefix on failure. Both accept exactly
  11 426 bytes. This intentionally differs from upstream, whose ingress clamps before forwarding
  (`em_capability.cpp:969-982`) and whose setter stores the clamped prefix while returning false
  (`dm_sta.cpp:671-701`). The report's separate output limit is specified in D5.

If the security-mode helper is ported, its supported single-mode labels and exact reverse mapping
are the following. Each canonical label parses to the mode in the same row; formatting a mode
emits that label (`em_base.h:126-136`, `dm_easy_mesh.cpp:2104-2149`):

| Mode | Canonical label |
|---|---|
| `EM_AUTH_OPEN` | `OPEN` |
| `EM_AUTH_WPAPSK` | `WPA-PSK` |
| `EM_AUTH_SHARED` | `WEP_128` |
| `EM_AUTH_SAE` | `SAE` |
| `EM_AUTH_WPA` | `WPA-Personal` |
| `EM_AUTH_WPA2` | `WPA2-Personal` |
| `EM_AUTH_WPA2PSK` | `WPA2-PSK` |
| `EM_AUTH_SAE_AKM8` | `WPA-WPA3-Personal_AKM8` |
| `EM_AUTH_DPP_AKM` | `WPA-WPA3-Personal_DPP` |
| `EM_AUTH_SAE_AKM24` | `WPA-WPA3-Personal_AKM24` |

For compatibility with the pinned reverse helper, also accept `WPA-WPA2-Personal` as an input
alias for `EM_AUTH_WPA2PSK` and `WPA-WPA3-Personal` as an input alias for `EM_AUTH_SAE_AKM8`;
formatting still emits the canonical labels. `WEP_64` is not an alias for WPA-PSK. Other labels
and numeric mode values not listed in the table have no mapping. The consumer tests every
canonical round trip, both aliases, and rejection of `WEP_64`, including separate WPA-PSK and
WPA2-PSK round trips and the existing `WPA-Personal` and `WPA2-Personal` mappings.

### D5. Requirements for note 0002 (before Step 3)

Note 0002 must say, for each handoff below, whether a stage uses a fresh query to the owning
actor, carries an owned delta or snapshot, and how that data survives fan-out, commit and
persistence:

- **Agent direct model assignments** (`dm_easy_mesh_agent.cpp:234, 674, 787`): renew (controller
  and radio identities with overrides), CSA (local BSS/radio/op-class read plus channel update),
  STA link metrics (decoded station changes); STA-list fan-out and link metrics transfer all three
  STA maps (`:219, 793`); scan and unassociated results come from the live agent (`:989, 1048`).
- **Controller constructions** via the `dm_network_t` converting constructor
  (`dm_easy_mesh_ctrl.cpp:3302, 3413, 3891, 4943, 4958`): target identity plus the specific
  request; removal passes an existing peer model (`:4881`); policy completion consumes staged rows
  and requests persistence (`em_orch_ctrl.cpp:272-286`, where the `update_tables` return value is
  ignored); failure handling is left to note 0002. DPP calls `init(dm)` (`dm_dpp.cpp:42-53`).
- **Next-stage fan-out** (`dm_easy_mesh_agent.cpp:103, 218, ...`; `dm_easy_mesh_ctrl.cpp:3393, ...`)
  with per-operation payloads and progress state; the agent orchestrator uses both the array index
  and staged model changes (`em_orch_agent.cpp:245-276, 331-362`).
- **Client Capability Report read:** the actor owning the station handles the request and invokes
  the consumer's encoder while borrowing the stored frame inside that actor. Only the owned
  serialized TLV or report crosses the actor boundary for this read; no copied station record or
  borrowed frame does.
  Preserve the pinned report output rule: emit at most the prefix that fits the separate
  `EM_CLIENT_CAP_MAX_FRAME_BODY` report limit, without modifying or truncating the stored frame.
  That limit is the smaller of the native storage cap and the message limit less report overhead
  (`em_capability.cpp:48-56, 542-581`). Note 0002 must specify this handoff before Step 3, and
  the report consumer must test a nonempty stored frame at the output limit and, when that limit
  is below the storage cap, one byte beyond it, checking the emitted bytes and that the stored
  frame is unchanged.

### D6. Steps

Step 1 starts from these stored model declaration roots, including the network SSID configuration
and topology keys in D1 and D2:

- `em_interface_preference_t`, `em_interface_t`, `em_long_string_t`, `mac_address_t`,
  `em_network_info_t`, `em_device_info_t`, `em_ieee_1905_security_info_t`,
  `em_network_ssid_info_t`, `em_ssid_2_vid_map_info_t`;
- `em_radio_info_t`, `em_radio_cap_info_t`, `em_bss_info_t`, `em_op_class_info_t`, `em_policy_t`,
  `em_cac_comp_info_t`, `em_sta_info_t`, `em_scan_result_t` and its key `em_scan_result_id_t`;
- `em_ap_mld_info_t`, `em_bsta_mld_info_t`, `em_assoc_sta_mld_info_t`, `em_tid_to_link_info_t`,
  `em_unassoc_sta_metrics_rsp_t`, `em_unassoc_query_list_t`, `em_unassoc_sta_metric_entry_t` and
  `em_mlo_policy_t`.

The node's wrapper and direct members are in `inc/dm_easy_mesh.h:62-101`; the scan key and row are
in `inc/em_base.h:1064-1094`, and the SSID-to-VID record is in `inc/dm_ssid_2_vid_map.h:25-26`.
Follow each root's stored fields transitively. In addition, include every existing `em-dm` row in
`docs/em_base-coverage.md` in the classification table, even if no stored field reaches it. For
each row classify the type and, if reassigned, record its destination and reason. For example,
classify wire-only `vendor_ext_attr_id_t`, which is used by wire output rather than reached from
the stored roots (`inc/em_base.h:2332-2342`, `em_configuration.cpp:5917-5924`). No coverage row
may be omitted before the user approves the table.

Before generating the inventory, Step 1 selects the `rdkcentral/OneWifi` commit, on the default
branch that upstream CI clones (`.github/workflows/build.yml:53-56`, no revision given), with the
newest commit date at or before `2026-11-17T11:13:51-08:00`, the pinned upstream commit's commit
date.
Record its full hash and, for every HAL source needed, the repository and immutable revision or
versioned artifact and digest, plus header paths; record platform header and toolchain versions
and preprocessing settings needed to reproduce the declarations. The target ABI for platform
typedefs is **x86_64 Linux, LP64**. Resolve and cite the external manifest against those sources,
and confirm all header-dependent values, including the assumed 33-byte `ssid_t`, from that pin.
If a required source or assumption cannot be confirmed, stop and report the discrepancy instead
of generating the inventory. The user approves the OneWifi hash before the inventory is generated;
the separate approval of the classification table remains required before Step 2.

| Step | Contents |
|---|---|
| 1 | **Inventory generator**: inventory every field of the stored roots above, with its original C type and bound expression. Follow stored fields transitively through referenced declarations, typedefs and array elements, inventorying their fields or recording an explicit mapping that accounts for every stored leaf. For example, follow `em_policy_t` → `em_client_filters_cfg_t` → `mac_addr_t` (`em_base.h:2174-2178, 3841-3848`), even though coverage assigns the nested type to `em-msg`. Resolve local constants and declarations from the pinned headers; resolve external types, their relevant nested fields and constants from the pinned, cited manifest above. Its required inputs include `mac_address_t`, distinct `mac_addr_t`, `ssid_t`, `bssid_t`, `wifi_ieee80211Variant_t`, `wifi_BeaconReport_t`, `WIFI_AP_MAX_VENDOR_IE_LEN`, `wifi_channelBandwidth_t`, and the external `uint8_t`, `uint32_t`, `time_t` and `size_t` used in stored device, STA and BSS fields (`em_base.h:2618-2623, 2738, 2834-2835`), plus every additional external dependency found by traversal. Fail on any unresolved type or bound, including external dependencies absent from the manifest. Retain enum numeric values; mark zero-length tails (`val[0]`) as wire tails, not zero-capacity fields, and mark pointers. Produce the per-type classification and coverage reconciliation in D7, including all existing `em-dm` coverage rows; obtain user approval before Step 2. |
| 2 | Records and enums (`em-dm`), with golden tests covering every inventoried field against its Rust type (or an explicit mapping), every retained enum code's numeric value, and the capacity triple of D4; native-constructor and representation tests and clippy lint enforcement (criterion 4). |
| — | Note 0002 (D5). |
| 3 | `NodeModel`, `MeshModel` and their operations, with tests ported from the upstream dm tests and size checks on the actual model storage, insert and removal types (criterion 4). |

### D7. Decisions and open questions

Decided by the user (2026-11-19): JSON outside `em-dm`; fix defect 3 to its evident intent (made
precise in D4); correct the WPA-PSK string if ported (D4); note 0002 before Step 3. The port fixes
the upstream defects listed above, with a test for each distinct failure mode showing the corrected
behaviour and noting upstream's behaviour.

Answered by the user (2026-11-19):

1. *(Decided 2026-11-19, see D3: field by field, large records on the heap; option A withdrawn.)*
2. *(Decided 2026-11-19: approved as recommended below.)* **Classify types before Step 2.** Step 1 produces a per-type table classifying
   each inventoried and referenced type as wire-only, stored-native, or dual-use with a native
   counterpart. Classify every existing `em-dm` row in `docs/em_base-coverage.md`, recording the
   destination and reason for any reassignment, including types reached only through wire output.
   Obtain the user's approval of the complete table before Step 2. Include MLO and unassociated-STA
   state (`inc/dm_easy_mesh.h:89-101`); reserve ownership
   of command-shaped payloads for note 0002. Keep the split between the wire scan result and the
   native scan row and key (`em_channel.cpp:2426-2450`). Dual-use examples include
   `emex_product_class_t` (stored in `em_base.h:2546-2548, 2566, 2626`; read from the wire at
   `em_configuration.cpp:1324-1332`), `em_haul_type_t` (stored at `em_base.h:2660`; emitted at
   `em_configuration.cpp:5917-5924`), and `em_ieee_1905_security_cap_t` (stored at
   `em_base.h:2629-2638`; used as a TLV body at `em_configuration.cpp:4434-4438`).
3. *(Decided 2026-11-19, see D2: one controller actor owns all controller-side node records and the
   network SSID configuration.)* Original question: **Choose the authoritative controller-side owner.** Upstream fans out network-SSID updates to
   matching node models and copies SSIDs from a reference model when adding a node
   (`dm_easy_mesh_list.cpp:959-986, 1782-1801`). Recommended: one controller actor owns all
   controller-side node records and network-wide SSID configuration; `em-core` obtains reads and
   sends updates through messages, not model copies. Any external index contains keys, not
   authoritative records, and `&`/`&mut` stay inside the owner.
4. *(Decided 2026-11-19: option 3.)* **AP MLD exact-capacity SSID.** The length rule applies to the
   decoded AP MLD SSID field; accepting that field does not itself write an AP MLD model row.
   Upstream accepts `ssid_len == sizeof(ssid_t)` and overwrites the last byte with NUL before haul lookup
   (`em_configuration.cpp:2776-2779, 2803-2808`), silently changing an SSID whose last byte is not
   NUL. The port accepts `ssid_len` up to 32; it accepts `ssid_len == sizeof(ssid_t)` (33, assuming
   OneWifi's `ssid_t` is `32 + 1`) only if the last byte is NUL. A 33-byte field with a non-NUL
   last byte is rejected as malformed (intentional difference: upstream truncates); longer
   lengths are rejected as upstream does. Step 1 must confirm the assumed `sizeof(ssid_t)` at the
   selected OneWifi pin. For every accepted length, normalize the decoded SSID field to the bytes
   before the first NUL within the field, or all field bytes if none is present: this is the
   upstream C-string interpretation, including for embedded NULs and padding. Haul lookup compares
   those bytes against the configured SSID's bytes before its first NUL, as upstream's `strncmp` does;
   the first matching configuration with at least one haul supplies its first haul; an empty SSID
   or no eligible match remains unresolved (`dm_easy_mesh.cpp:2876-2893`). An unresolved haul
   leaves all AP MLD rows unchanged: neither insert nor replace a row. Upstream passes the decoded
   row with the unresolved haul to the model, which returns before a row write
   (`em_configuration.cpp:2803-2808, 2836`, `dm_easy_mesh.cpp:3709-3717`). Continue processing
   affiliated-BSS updates independently, as upstream does (`em_configuration.cpp:2838-2851`).
   The pinned sender emits `sizeof(ssid_t)` and `strncpy` pads NUL-terminated short names
   (`em_configuration.cpp:1393-1394`); the configuration path
   terminates the source SSID (`em_configuration.cpp:332-334`). Option 3 accepts all such
   upstream-sender traffic. The consumer tests a short field with an embedded NUL, a 32-byte field
   with no NUL, 33 with a final NUL, 33 with a non-NUL final byte, and longer lengths; it also
   tests a sender fixture containing `mesh` plus NUL padding with `ssid_len == 33`, resolving to
   the configured haul. For length-valid empty and unmatched SSIDs, and a matching configuration
   with no hauls, assert that the decoded field is accepted but the AP MLD row count and existing
   rows are unchanged. With an affiliated BSS that upstream would update, assert its update still
   occurs despite the unresolved haul.
   Spec validity is not verified; an 802.11 SSID is at most 32 bytes.

## Acceptance criteria

1. **Step 1 generator:** strict parsing; `--check`; exit codes 0/1/2; failures for duplicate or
   missing fields, malformed syntax, unresolved types or bounds and a stale fixture; unit tests
   like the other generators; the external manifest is pinned and cited as in D6. A nested policy
   fixture asserts the `em_policy_t` → `em_client_filters_cfg_t` → `mac_addr_t` field and mapping
   are inventoried, and an unresolved nested dependency fails the check. A coverage fixture
   requires a classification for every existing `em-dm` row, including wire-only
   `vendor_ext_attr_id_t`, and records the destination and reason for each reassignment; omission
   fails the check.
2. **Step 2 crate:** workspace member; dependencies limited to `em-types` (and `em-utils` if used);
   build, test and clippy with `--all-targets`, `fmt --check`, rustdoc with `-D warnings`, on stable
   and 1.85.0; manual R2 review for duplication the lint cannot see.
3. Golden assertions cover every inventoried field against its Rust type or an explicit mapping
   (including intentional splits), and the numeric value of every retained enum code (for example,
   `em_haul_type_t`). Step 2 tests native constructors at exact limits, with a reserved terminator,
   non-UTF-8 bytes and unknown codes. Wire-reader tests belong to the consumer implementing each
   reader under 0005, including the AP MLD case in D7.
4. No `Clone` on non-`Copy` types; large records are neither `Clone` nor `Copy` (tested in Step 2),
   and both models are neither `Clone` nor `Copy` (tested in Step 3). Every `Copy` type is at most
   256 bytes, with a `size_of` test per type in the step or consumer that introduces it. Step 2
   checks record representations and makes clippy's `large_types_passed_by_value`,
   `large_stack_arrays` and `large_enum_variant` errors. For each large record, Step 3 asserts the
   `size_of` of the actual stored value type and each insert and removal handoff type is at most
   256 bytes; each consumer asserts the same `size_of` bound on its actual decode handoff type when
   introduced. Use a box or split payloads as in D3. Step 2 native-constructor and Step 3 update
   tests accept a frame body at 11 426 bytes and reject 11 427 bytes, preserving any existing frame
   on the rejected update; the ingress consumer tests the same boundary and unchanged state on
   rejection.
5. **Step 3 gate** (later): same MAC in different networks; insert at capacity vs update of an
   existing row; failed batch updates; STA association/disassociation with frame ownership;
   network-SSID updates visible to all matching nodes; a decoded scan-result insert/update fixture.
   Configure an SSID in network A, then add a late-joining node in A and a node with the same AL
   MAC in network B with different configuration. Through the authoritative lookup, assert the
   late node immediately sees A's SSID without a fan-out copy and the B node sees only B's; after
   removing A's SSID, assert A no longer sees it and B's configuration remains visible.
6. `STATUS.md`, `docs/em_base-coverage.md`, the crate README and the root README are updated.
