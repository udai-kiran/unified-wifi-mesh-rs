# 0001: Scope and design rules for `em-types`

| | |
|---|---|
| Status | **Accepted: revision 4** (2026-11-18; revisions 1–3 superseded) |
| Date | 2026-11-18 |
| Upstream | `rdkcentral/unified-wifi-mesh` @ `ba4180ba36748629f10c20babc4320dd69c9db4f` |
| Affects | `em-types`; ownership of every type in `inc/em_base.h` ([coverage table](em_base-coverage.md)); porting order in `STATUS.md` |

Revisions 2 and 3 address two external reviews. The [review resolution](#review-resolution)
section at the end maps each finding to the change that fixes it.

## Context

`inc/em_base.h` (3,922 lines, Apache-2.0) is the header nearly every upstream file includes. It
contains only declarations: 365 typedefs (289 structs, 48 enums, 28 aliases including one
function-pointer typedef), about 260 `#define`s,
and no functions. Different concerns are **interleaved** through the file rather than grouped into
sections. For example, `em_state_t` ends at line 2410 and `em_cmd_type_t` starts at 2412,
`em_interface_t` sits at 2512 among event types, and bus events start at 3140. Ownership is
therefore decided **per type**, not by line range. Every typedef is listed with its owner in
[`em_base-coverage.md`](em_base-coverage.md).

`inc/ieee80211.h` (1,744 lines) is FreeBSD net80211 code under BSD-2-Clause-FreeBSD.

`em_base.h` also depends on types from outside unified-wifi-mesh: OneWifi/HAL (`wifi_webconfig.h`,
`wifi_BeaconReport_t`, `wifi_channelBandwidth_t`, `wifi_ieee80211Variant_t`; `mac_address_t` and
`mac_addr_str_t` are also defined there), OpenSSL (`EVP_PKEY`, `EVP_PKEY_CTX`, `EC_KEY`), libuuid
(`uuid_t`), and `ec_base.h`.

### Upstream properties that must not be copied

1. **Bitfields in packed wire structs.** `em_base.h` has 198 bitfield declarations (191
   `unsigned char`, 6 `uint8_t`, 1 `unsigned int`). An example is the CMDU header
   (`em_base.h:464-473`):

   ```c
   typedef struct {
       unsigned char ver; unsigned char reserved;
       unsigned short type; unsigned short id; unsigned char frag_id;
       unsigned char reserved_field:6;
       unsigned char relay_ind:1;
       unsigned char last_frag_ind:1;
   } __attribute__((__packed__)) em_cmdu_t;
   ```

   C leaves bitfield allocation order to the compiler and ABI. On the GCC little-endian ABIs that
   upstream is built for, `last_frag_ind` ends up in bit 7, which matches the wire format. The
   header has no byte-order guards, and the layout is not portable across compilers, ABIs or byte
   orders.

2. **Implicitly numbered wire enums.** Only 5 of the 72 `em_msg_type_t` enumerators have explicit
   values. The rest follow from declaration order, so inserting or reordering an enumerator
   upstream silently changes values that go on the wire.

3. **Sentinels in the same enums as protocol codes.** Bounds and placeholder markers share the
   enums with real codes: `em_service_type_none` (0xA2, `:490`), `em_profile_type_max` (`:498`),
   `em_freq_band_unknown` (`:506`), `em_media_type_max` (`:2240`), `em_tlv_type_max` (0xF3, `:934`).
   For example, `em_tlv_type_t` has 122 enumerators: 121 TLV codes plus `em_tlv_type_max`.

4. **Fixed-size C strings without enforced bounds.** For example, `em_interface_name_t` is
   `char[32]` (`:410`), but `MAX_INTF_NAME_SZ` is 16 (`:46`). Some code truncates safely
   (`dm_easy_mesh.cpp:2173-2174`), while other code copies `strlen(name)+1` bytes with no bound
   (`dm_device.h:142`, `dm_easy_mesh.cpp:2245`).

## Decision

### D1. Every type in `em_base.h` has exactly one owner

The full per-type assignment is in [`em_base-coverage.md`](em_base-coverage.md) (365 of 365 types
assigned, each to exactly one owner or disposition). Summary:

| Owner | Types | What |
|---|---|---|
| `em-types` | 8 | Shared basics (D2) |
| `em-transport` | 6 | Link-layer headers: `llc_hdr_t`, `snap_hdr_t`, `em_raw_hdr_t`, `em_raw_ext_hdr_t`, frame info/event |
| `em-msg` | 216 | CMDU/TLV/attribute framing (`em_cmdu_t`, `em_tlv_t`, `em_attrib_t`), WSC attribute and message codes, `em_tlv_requirement_t`, vendor TLV codes, and all TLV body layouts |
| `em-dm` | 47 | Data-model records (`em_*_info_t`, policy records, Data Elements attribute IDs) |
| `em-cmd` | 16 | Request-side types not affected by the D7 deferral (e.g. steering, disassoc and list-reason parameters); provisional, see D7 |
| pending | 13 | Command, request and action payload types (`em_cmd_params_t`, the `em_bus_event_type_*_params_t` structs, …): owner decided by note 0002 (D7) |
| `em-core` | 15 | State machine and events (D7), network-node tree and its editor callback |
| replaced | 2 | `em_bus_event_type_t` and `em_bus_event_t`: not ported as-is; split into native types (D7) |
| `em-db`, `em-orch`, `em-crypto`, `em-cli`, `em-ctrl`, `em-agent` | 3–7 each | Their own types |
| `em-easyconnect` | 1 | `em_advertise_cce_t` |
| deferred | 2 | 802.11 element tags, decided with `ieee80211.h` (D6) |
| do not port | 17 | C idioms: fixed `char[N]` string aliases, zero-length arrays, `em_enum_type_t`, `em_queue_t` |

Each type is ported together with the code that uses it, never ahead of it. Assignments marked
"verify" or "confirm" in the coverage table are provisional. The owning crate's port confirms or
reassigns each one and updates the table's Status column. A type with no owner blocks that port.

### D2. Contents of `em-types`

| Rust type | Replaces (upstream) | Kind |
|---|---|---|
| `MacAddr` | `mac_address_t`, `mac_addr_str_t`, `dm_easy_mesh_t::macbytes_to_string` / `string_to_macbytes` | value type |
| `InterfaceName` | `em_interface_name_t` | validated string (D5) |
| `Interface { name: Option<InterfaceName>, mac: MacAddr, media: MediaType }` | `em_interface_t` | plain struct; `None` = name not known (D5) |
| `MsgType(u16)` | `em_msg_type_t` | open wire code (D3) |
| `TlvType(u8)` | `em_tlv_type_t` | open wire code |
| `FreqBand(u8)` | `em_freq_band_t` | open wire code: 1905.1 Table 6-23 plus EasyMesh Table 15, which adds 0x03 = 6 GHz (checked against the spec v6.0 text, §6.1) |
| `MediaType(u16)` | `em_media_type_t` | open wire code: 1905.1 media type, 2 octets |
| `ProfileType(u8)` | `em_profile_type_t` | open wire code: Multi-AP Profile TLV (spec Table 69) |
| `ServiceType(u8)` | `em_service_type_t` | open wire code: SupportedService/SearchedService (spec Table 23), plus upstream's 0xA0/0xA1 |

`em-types` has no dependencies, uses the 2024 edition, sets `rust-version = "1.85"` (the first
release with edition 2024), contains no `unsafe` (`#![forbid(unsafe_code)]`), and uses `std`.

### D3. Every code that comes off the wire is an open newtype

All six code types above are read directly from received bytes upstream:

| Type | Read in upstream code at |
|---|---|
| frequency band | `em_msg.cpp:346-355` |
| profile | `em_msg.cpp:131-150` |
| service | `em_msg.cpp:103-123` |
| all of band, service and profile on transmit | `em_configuration.cpp:4559-4581` |

The spec also requires unknown values to be tolerated in places. For example, a receiver of a
reserved Multi-AP Profile value "shall assume the sender implements the same profile implemented by
the receiver" (Table 69), and SupportedService codes 0x02–0xFF are reserved (Table 23). A closed Rust
`enum` can't hold those values, so **every wire code type is a newtype over its wire integer**:

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ServiceType(pub u8);
impl ServiceType {
    /// Multi-AP Controller (spec Table 23). Upstream: `em_service_type_ctrl`.
    pub const MULTI_AP_CONTROLLER: Self = Self(0x00);
    // …
}
```

Unknown values round-trip unchanged. **The codec decides** whether to reject, ignore or interpret
an unknown value, using the spec rule for that field. That's `em-msg`/`em-core`'s job, and these
types don't make that decision.

Upstream values that are not protocol-defined codes are not named in these types. Amendment A1
explains that such a value can still be received; it then round-trips as a raw value:

- **Sentinels** (`*_max`, `*_none`, `*_unknown`) are excluded. "No value" is expressed as
  `Option<T>`.
- **Internal codes** stay with their real owner. For example, `em_service_type_cli` (0x02, an IPC
  endpoint, reserved on the wire) belongs to `em-cmd`'s IPC endpoint type, not `ServiceType`.

### D4. Codes come from one declarative table, pinned by golden data

**Value classes.** Every upstream enumerator of the six D2 enums is given one class:

| Class | Meaning | In the Rust API? |
|---|---|---|
| `spec` | Defined by EasyMesh or 1905.1 (spec table cited) | yes |
| `vendor` | Not in the spec but sent on the wire by upstream (e.g. `ServiceType` 0xA0/0xA1, `TlvType` 0xF2 `vendor_operational_bss`) | yes, documented as non-standard |
| `internal` | An upstream value that never goes on the wire (e.g. `em_service_type_cli`) | no; ported by the owning crate |
| `sentinel` | Bound or placeholder (`*_max`, `*_none`, `*_unknown`) | no |

**Classification file (Step 1 output).** The class of each enumerator is a human judgement and
can't be generated from the header. It is recorded in a hand-maintained file,
`crates/em-types/tests/data/classification.tsv`.

Format:

- UTF-8, tab-separated, `\n` line endings, with exactly one header line:

  ```
  upstream_type	upstream_identifier	class	rust_name	spec_reference	reason
  ```

- `upstream_type`: the C enum name, e.g. `em_msg_type_t`.
- `upstream_identifier`: the enumerator, e.g. `em_msg_type_autoconf_search`.
- `class`: one of `spec`, `vendor`, `internal`, `sentinel`.
- `rust_name`: the associated-const name for `spec`/`vendor` rows. `-` for `internal`/`sentinel`
  rows, which aren't in the Rust API.
- `spec_reference`: table and section (e.g. `Table 23, §17.2.1`) for `spec` rows, otherwise `-`.
- `reason`: required for every non-`spec` row; `-` for `spec` rows.
- `-` is the only marker for an absent field. No field may be empty or contain a tab.
- `(upstream_type, upstream_identifier)` is unique. `(upstream_type, rust_name)` is unique among
  rows whose `rust_name` is not `-`.
- Rows are sorted in fixture order: enum by enum, then declaration order.

The generator checks the classification file against the extracted fixture. Every fixture entry
must have exactly one classification row, and no row may be left over. The generator fails if
either check fails.

Step 1 produces the fixture and this file. The classification file is reviewed against the spec
text before Step 2. In Step 2 the Rust declarative tables are written from it, and the golden test
checks that they agree with it.

**One declarative table per type.** A `macro_rules!` table lists, for each code: the Rust name
(spec name), the value, the class, the upstream identifier, and the spec reference. The macro
generates the associated consts, `name()`, an `ALL` slice and the doc comments. Because `ALL`
comes from the same table as the consts, the test can check for extra values, which it couldn't
by reflecting over consts.

**Golden data** (`crates/em-types/tests/data/upstream_enums.tsv`):

1. A generator script (`tools/gen-upstream-enums`) reads the header **from the pinned commit**
   (`git -C <upstream> show ba4180ba36748629f10c20babc4320dd69c9db4f:inc/em_base.h`). It fails if
   that object is missing.
2. It extracts each of the six `typedef enum { … } <name>;` blocks, asserts that exactly one block
   matches each name, and asserts the expected enumerator count: msg 72, tlv 122, freq band 5,
   media 10, profile 5, service 6.
3. It compiles the blocks in a standalone C program that prints `type, upstream identifier, value`.
   These six blocks use only literals and implicit increments, so no OneWifi headers are needed.
4. The fixture has a header carrying only stable provenance: the upstream commit, the source path,
   the generator command, and `SPDX-License-Identifier: Apache-2.0`. Nothing that depends on the
   environment goes in the fixture. The compiler and version used for the committed generation are
   recorded once in a separate, hand-maintained `crates/em-types/tests/data/README.md`, which is not
   regenerated.
5. `tools/gen-upstream-enums --check` regenerates the fixture and compares it byte for byte with
   the committed copy. Because the fixture holds only identifiers, values and stable provenance,
   any C compiler that produces the same values gives an identical file. It needs the upstream
   checkout, so it is a manual/CI step, not part of `cargo test`.

**The golden test** (part of `cargo test`) checks that:

- every golden entry is in the Rust table with the same value and a matching upstream identifier,
  or is listed in an explicit `EXCLUDED` list with its class (`internal`/`sentinel`) and a reason
- every Rust table entry matches a golden entry, or is in an explicit `ADDITIONS` list with a spec
  reference (a spec value that upstream lacks)
- no value appears twice and no name appears twice within a type

Where the spec and upstream disagree, the test makes the disagreement visible. Resolving it is a
separate, recorded decision.

### D5. Value-type rules

**`MacAddr`**
- `[u8; 6]` newtype. Derives `Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default`.
- Conversions: `From<[u8; 6]>`, `From<MacAddr> for [u8; 6]`, `AsRef<[u8; 6]>`, `AsRef<[u8]>`,
  `TryFrom<&[u8]>` (exactly 6 bytes).
- `Display`: lowercase, colon-separated (`aa:bb:cc:dd:ee:ff`), matching upstream
  `macbytes_to_string`. `Debug` uses the same form.
- `FromStr` accepts both forms that upstream accepts: colon-separated and compact 12-hex
  (`dm_easy_mesh.cpp:2070-2078`). Hex is case-insensitive.
- `FromStr` rejects everything else: wrong octet count, single-digit octets, mixed or other
  separators, whitespace, trailing data. On error it returns `Err` and produces no value.
  **Intentional difference:** upstream's unchecked `sscanf` leaves undefined bytes on malformed
  input. We don't reproduce that.
- Constants and methods: `ZERO`, `BROADCAST`, `IEEE1905_MULTICAST` (`01:80:c2:00:00:13`),
  `is_multicast()`, `is_zero()`.

**`InterfaceName`**
- Its invariant is chosen by the Rust API, not inherited, because upstream enforces no bound (see
  Context, item 4).
- The rule follows Linux's `dev_valid_name`: 1 to 15 **bytes** (`IFNAMSIZ` − 1), valid UTF-8, no
  NUL, `/`, `:` or whitespace, and not `.` or `..`. This fits both upstream's `char[32]` and its
  `MAX_INTF_NAME_SZ` of 16. A name the kernel can't create is rejected.
- Linux interface names that aren't valid UTF-8 are rejected (documented). `Display`/`AsRef<str>`
  are provided.
- **Unknown names.** Upstream treats `em_interface_t.name` loosely. Zero-initialised or
  not-yet-known interfaces have an empty name. For that reason `Interface.name` is
  `Option<InterfaceName>`, and "not known" is `None`, never an empty or placeholder string.
- **Runtime kernel names.** Upstream copies kernel names from `getifaddrs()` without validating
  them (`src/dm/dm_easy_mesh.cpp:2219-2245`, `:2301-2319`). Linux names are byte strings, so a
  name that isn't valid UTF-8 is possible at runtime. **Intentional incompatibility:** such a name
  produces an `InterfaceNameError` with context (the interface's MAC address and its raw bytes as
  hex). It is not silently dropped or mangled.
- **Compatibility.** No hard-coded name or shipped configuration in upstream `src/` or `config/`
  violates the rule. The
  generated veth names in `config/openwrt/banana-pi/setup_veth_for_em.sh` fit for the documented
  `eth0`/`eth1` inputs. Some upstream unit tests use placeholder labels that are not kernel names,
  such as `"AgentAL_Interface"` and `"agent_interface_1"` (17 bytes), or names with whitespace
  (`tests/test_l1_em_cmd.cpp`, `tests/test_l1_dm_network.cpp`, `tests/test_l1_dm_device.cpp`).
  **Intentional difference:** we don't port those cases. Any place that loads names from
  persisted or external data (DB, JSON config) must report an invalid name as an error with
  context, and must not store it. Whether to reject the record or map the name to `None` is
  decided in `em-db` / the config loader's own design note.

**Code newtypes** (`MsgType`, `TlvType`, `FreqBand`, `MediaType`, `ProfileType`, `ServiceType`)
- Derive `Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord`, with a `pub` inner integer and
  `From` conversions to and from it.
- `name() -> Option<&'static str>`.
- `Debug`: `AP_AUTOCONFIGURATION_SEARCH(0x0007)` for known values, `MsgType(0x1234)` for unknown
  ones. Hex is zero-padded to the wire width.
- No `Display`. These codes are logged with `Debug`.
- Byte-order encoding is done in `em-msg`, never on these types.

**Errors.** One error enum per validated type (`MacAddrParseError`, `InterfaceNameError`).
They are non-exhaustive and implement `Display` and `std::error::Error`.

**No C layout.** `em-types` has no `#[repr(C, packed)]`, no bitfields, no zero-length arrays and no
`unsafe`. Wire encoding, including bit positions such as `last_frag_ind` = bit 7, is explicit
in `em-msg`.

### D6. `ieee80211.h` is not part of `em-types`

It is used where 802.11 frames and elements are built or parsed (action frames, beacon/BTM, IE
tags `tag_type_t`/`ieee80211_tagvalue_t`), which is in `em-core`/`em-agent`. It will be decided
when those crates are ported: either use a maintained Rust 802.11 crate, or port only the
definitions actually used and keep the BSD-2-Clause-FreeBSD notice. No BSD-licensed material goes
into `em-types`.

### D7. Events, requests and commands: constraints now, design in note 0002

This decision doesn't affect `em-types`. It is recorded here because the `em_base.h` split (D1)
touches it, and because two review rounds showed that the boundary between `em-cmd`, `em-core` and
`em-orch` needs its own analysis.

**Facts established (upstream @ `ba4180b`):**

- `em_bus_event_t` is the bus-event payload carried through each manager's `em_event_t` input
  queue. `em_mgr_t::io_process(type, …)` allocates an `em_event_t`, fills `evt->u.bevt`, and calls
  `push_to_queue(evt)` (`src/em/em_mgr.cpp:55-75`). The same queue also carries frame,
  northbound and command events (`inc/em_base.h:3600-3608`).
  `em_bus_event_type_t` has 71 enumerators: 70 values plus `_max`.
  [`bus-event-inventory.md`](bus-event-inventory.md) categorises every one by its real producer and
  consumer:
  - A: 27 management requests (CLI / rbus / TR-181 → manager)
  - B: 15 platform notifications (OneWifi callback → agent)
  - C: 12 engine → host actions (`src/em/*` asks its manager to act)
  - D: 5 manager-internal events
  - E: 7 unused or sentinel; this includes `radio_config`, `vap_config` and `vendor_data`, which
    have no producer at this commit
  - 5 values with two roles (e.g. `set_policy` A+C, `scan_result` A+B)
- Command, engine and orchestration are coupled in both directions:
  - `em_cmd_params_t` (`em_base.h:3513-3526`) embeds aliases of wire bodies
    (`em_scan_params_t`, `em_beacon_metrics_query_t`) and an `em_network_node_t *` pointer.
  - `em_cmd_t` (`inc/em_cmd.h:39-46`) holds `em_event_t *`, an `em_orch_desc_t[]` array, a
    candidate queue and a `dm_easy_mesh_t` snapshot.
  - `em_t` holds `em_cmd_t *m_cmd` (`inc/em.h:62`).
  - `em_cmd.h` includes `em_ctrl.h` and `dm_easy_mesh.h` (`inc/em_cmd.h:22-28`).
- Some category C actions forward TLV bodies directly: `bss_tm_req` sends `em_steering_req_t`
  (`em_steering.cpp:555-562`), `channel_scan_params` sends `em_scan_params_t`
  (`em_channel.cpp:2276-2282`), and `beacon_query` sends `em_beacon_metrics_query_t`
  (`em_metrics.cpp:465-468`).

**Constraints (binding):**

1. The upstream unions and queue envelope (`em_event_t`, `em_bus_event_t`) are not ported as-is.
   Each category, and each role of a two-role value, gets its own native type with a single owner.
2. The crate dependency graph is acyclic. Upstream's two-way `em_cmd_t` ↔ `em_t` ↔ orchestrator
   coupling is split, not reproduced.
3. OneWifi payloads are seen only by `em-agent`. `em-core` sees platform-neutral events.
4. Request types that cross the IPC boundary (category A) don't contain pointers into engine
   state, and don't alias wire bodies.

**Deferred to design note 0002**, written and reviewed before the `em-cmd` port starts:

- how upstream `em_cmd_t` and the 43 command classes in `src/cmd/` divide between a request
  type, an execution/orchestration object and the per-node operation context that `em_t` reads
  through `m_cmd`
- where the category C payloads live (`em-core`, with an `em-core → em-msg` edge, or native
  action payloads)
- the dependency graph and porting order for `em-cmd`, `em-core` and `em-orch`

Until then, the coverage-table rows for command, request and action payload types are
`pending (0002)`. The inventory's replacement owners and `STATUS.md`'s order for those three
crates are provisional.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Port `em_base.h` 1:1 into `em-types` as `#[repr(C, packed)]` | Creates a ~4,000-line crate that everything depends on. Carries over non-portable bitfield layout. Packed field references require `unsafe`. |
| `bindgen` | Needs the OneWifi, OpenSSL and uuid headers. Produces C layout and bitfield accessors and no idiomatic API. |
| Closed Rust `enum`s for wire codes | Can't hold reserved or unknown values that the spec requires receivers to tolerate (D3). |
| Golden test that requires every upstream enumerator as a Rust const | Would turn sentinels and internal codes into protocol API (D4). |
| `em-agent` owns all bus events | Most of them are management requests, engine actions or manager-internal events, not OneWifi notifications, so `em-agent` would be the wrong owner (D7). |
| Settle the full `em-cmd` / `em-core` / `em-orch` design inside this note | Unrelated to `em-types`. Each review round found more coupling (`em_cmd_t` ↔ `em_t` ↔ orchestrator), which needs its own analysis. Deferred to note 0002 under binding constraints (D7). |
| Zero-copy structs for fixed TLVs | Possibly useful inside `em-msg`; deferred to its design note. |

## Consequences

- `em-types` is small and can be finished on its own.
- `STATUS.md`: the order and dependencies of `em-cmd`, `em-core` and `em-orch` are marked
  provisional until note 0002. Note 0002 is a prerequisite of the `em-cmd` port.
- `README.md` and `crates/em-types/README.md` list exactly the D2 types.
- `docs/em_base-coverage.md` and `docs/bus-event-inventory.md` are kept up to date by every port
  that touches them.
- The spec-backed classification of every enumerator in Step 1 (`spec` vs `vendor` vs `internal`)
  is reviewed against the spec text before the fixture is accepted. A reviewer without access to
  the spec cannot verify those entries.

## Plan for the `em-types` port

1. **Golden data.** Write the generator, the generated fixture and the hand-maintained
   `classification.tsv` (D4). Classify every enumerator against the spec text. Review both files
   before writing any Rust.
2. **Crate.** Add the workspace `Cargo.toml` and `crates/em-types` as specified in D2–D5.
3. **Verification.** Run the acceptance criteria below, then a review, a commit and a push. Update
   `STATUS.md` and the coverage table.

## Acceptance criteria

1. Workspace builds. All of these pass on the stable toolchain and on `rust-version` 1.85:
   - `cargo build --all-targets`
   - `cargo test --all-targets`
   - `cargo clippy --all-targets -- -D warnings`
   - `cargo fmt --check`
2. `em-types` contains exactly the D2 types, has no dependencies, and is `#![forbid(unsafe_code)]`.
3. Golden test (D4) passes for all six enums, with explicit `EXCLUDED` and `ADDITIONS` lists, and
   the Rust tables agree with `classification.tsv`. `tools/gen-upstream-enums --check` reproduces
   the fixture byte for byte and confirms the classification file covers the fixture one-to-one.
4. Code newtypes: unknown values round-trip unchanged. `Debug` is tested for known and unknown
   values. Every value in `ALL` maps back to a name via `name()`.
5. `MacAddr`:
   - formatting round-trips
   - colon and compact forms are accepted, upper and lower case
   - rejection cases: 5 or 7 octets, single-digit octets, mixed separators, `-` separators,
     leading or trailing whitespace, trailing data, non-hex, empty
   - multicast, broadcast and zero helpers
6. `InterfaceName`:
   - lengths of 0, 1, 15 and 16 bytes
   - a multibyte UTF-8 name that is 15 bytes but fewer characters
   - embedded NUL, `/`, `:`, whitespace, `.` and `..`
7. `README.md`, `crates/em-types/README.md`, `STATUS.md` and `docs/em_base-coverage.md` are updated
   to match.

## Resolved questions

1. **Naming of associated consts.** Use **spec names** (e.g. `AP_AUTOCONFIGURATION_SEARCH`). The doc
   comment gives the upstream identifier and the spec section.
2. **Recording the profile or release that introduced each value.** **Not now.** Add it if `em-msg`
   validation needs it.

## Amendments

### A1 (2026-11-18, during Step 1): sentinels may still appear on the wire

D3 and D4 describe sentinels as values that "never go on the wire". Step 1 found that this is
true by spec but not by upstream behaviour. Upstream creates its controller and agent AL nodes with
`em_freq_band_unknown` (0x04) and copies that band into the AutoconfigFreqBand and
SupportedFreqBand TLVs (`src/ctrl/em_ctrl.cpp:1144`, `src/orch/em_orch_agent.cpp:259`,
`src/em/config/em_configuration.cpp:4404-4411`, `:4559-4562`). The extended Autoconfig Response
is built from `get_band()` and sent (`:4504-4507`). Table 15 marks 0x04 as reserved.

**Clarified meaning.** `sentinel` means "not a value defined by the protocol, so not named in the
Rust API". It does **not** guarantee the value is never received. Because every code type is an
open newtype (D3), such values still round-trip unchanged. How to treat them is decided by the
`em-msg` codec, together with the spec rule for that field. The decision itself is unchanged.

**Recorded for `em-msg`** (not acted on here): upstream rejects a received Multi-AP Profile value
≥ `em_profile_type_max` (`src/em/em_msg.cpp:144-148`), whereas Table 69 says a receiver of a
reserved value "shall assume the sender implements the same profile implemented by the receiver".
Upstream also maps a received 0x00 to Profile-1 (`src/em/config/em_configuration.cpp:6307`,
`:6412`).

### A2 (2026-11-18, during Step 1): IEEE 1905.1 rows are provisional

31 classification rows come from IEEE 1905.1 only, and 1905.1 is not available to this project:

- 5 message types: `0x0000`, `0x0004`–`0x0006`, `0x000A`
- 17 TLVs in `0x00`–`0x11`
- 9 media types

Their **values** are fixed: they are the values upstream's compiler assigns at the pinned commit,
which the fixture records and the Step 2 golden test will enforce. Not every one is used by
upstream code; for example, `em_media_type_ieee80211af` appears only in its enum declaration.
Their **Rust names and spec references** are provisional until checked against 1905.1. Renaming
them later is an API change to `em-types`, recorded in `STATUS.md` when it happens.

This is an **explicit exception** to the D4 / Consequences requirement that the classification be
reviewed against the spec text before Step 2. It applies to these 31 rows only. All EasyMesh rows
were checked against the EasyMesh v6.0 text.

### A3 (2026-11-18, Step 2): public API beyond the D5 list

The Step 2 crate exposes a few conveniences that D5 does not list. They are intentional and part
of the API:

- **`MacAddr`:**
  - `const fn new([u8; 6])` and `const fn octets(self)`, for const construction and access to
    the raw bytes
  - `is_broadcast()` (already covered by acceptance criterion 5)
- **`InterfaceName`:**
  - `MAX_LEN` (15) and `as_str()`
  - `FromStr`, `TryFrom<&str>` and `TryFrom<String>`, all sharing one validation function
  - `from_bytes(&[u8])`, which maps non-UTF-8 input to `InterfaceNameError::NotUtf8`
  - a `Debug` that prints `InterfaceName("eth0")`
- **`Interface`:** derives `Debug, PartialEq, Eq, Hash`. `name: None` means "the name is not
  known", as D2 says.
- **`Clone`:** following [0004](0004-runtime-and-ownership.md) R2, `Clone` is derived only on
  `Copy` types (`MacAddr` and the six code newtypes). `InterfaceName` derives
  `PartialEq, Eq, Hash, PartialOrd, Ord`, and `Interface`, `InterfaceNameError` and
  `MacAddrParseError` do not implement `Clone`. This overrides the `Clone` that D5 lists for
  `MacAddr`'s derives only where a type is not `Copy`. `MacAddr` is `Copy`, so it keeps `Clone`.

Two clarifications of D5:

- **Whitespace.** `InterfaceName` rejects every character for which `char::is_whitespace` is
  true, including Unicode whitespace such as U+00A0. This is stricter than the kernel's
  byte-oriented check, and it is intentional.
- **MAC context in errors.** `from_bytes` does not know the interface's MAC address. The caller
  that loads names from the kernel or from persisted data attaches it when it reports the error.

## Review resolution

### Review of revision 4

Verdict: "the em-types plan is ready to start Step 1". The reviewer judged the D7 deferral sound,
reproduced the inventory counts, and confirmed 14 sampled categories against live upstream code.
It raised four documentation findings. All were confirmed and fixed in place; none affects
correctness:

| # | Finding | Resolution |
|---|---|---|
| 1 | `classification.tsv` had no exact schema | D4 now specifies header, columns, absent-field marker (`-`), uniqueness and ordering |
| 2 | D1 summary grouped `em-easyconnect` (1 type) under "3–7 each" | Listed separately |
| 3 | `link_quality_report` still listed the commented-out agent producer | Moved to *other references* as commented out |
| 4 | D7 said `em_bus_event_t` *is* the queue | Corrected: it is the payload inside `em_event_t`, which `io_process` queues (`src/em/em_mgr.cpp:55-75`) |

### Revision 4 (review of revision 3)

Five findings. Each was checked against the upstream source before being accepted.

| # | Finding | Resolution |
|---|---|---|
| 1 | Blocker: the stated `em-cmd → em-dm → em-types` graph is impossible. `em_cmd_params_t` embeds wire-body aliases and an `em_network_node_t *`; `em_cmd_t` holds `em_event_t *` and `em_orch_desc_t[]` | Confirmed (`em_base.h:3510-3526`, `inc/em_cmd.h:39-46`; also `em_t` holds `em_cmd_t *`, `inc/em.h:62`). It concerns the design of crates 9–11, not `em-types`. D7 now records the facts and binding constraints, removes the unsupported graph, and defers the boundary to note 0002. The 13 affected coverage rows are marked `pending (0002)` |
| 2 | Category C actions carry TLV bodies, so "`Action` depends on `em-dm`" is wrong; the bus-event parameter structs were labelled request parameters | Confirmed (`em_steering.cpp:555-562`, `em_channel.cpp:2276-2282`, `em_metrics.cpp:465-468`). The `Action` dependency claim was removed; payload location deferred to 0002; parameter rows are `pending (0002)` |
| 3 | `radio_config`, `vap_config` and `vendor_data` have no producer; `link_quality_report`'s OneWifi producer is commented out | Confirmed (`em_agent.cpp:2162`; only a `BUS_EVENT_TYPE_2S` string-table use at `em_cmd.cpp:525-526`). Reclassified: three to E, one to C. Counts now A 27 / B 15 / C 12 / D 5 / E 7 / two-role 5 |
| 4 | Step 1 has no file in which to record the manual classification | Confirmed. D4 adds the hand-maintained `classification.tsv`, checked by the generator for complete one-to-one coverage of the fixture, and reviewed against the spec before Step 2 |
| 5 | The `InterfaceName` compatibility claim was too broad: `getifaddrs` names are copied unvalidated | Confirmed (`dm_easy_mesh.cpp:2219-2245`). Claim narrowed to hard-coded names and shipped configuration; names that aren't valid UTF-8 are an intentional incompatibility that produces a contextual error |

### Revision 3 (review of revision 2)

The second review confirmed that 13 of the 15 earlier findings were resolved, re-opened #2 and #6,
and raised 6 new findings. Each was checked against the upstream source before being accepted.

| # | Finding | Resolution |
|---|---|---|
| 2 (re-opened) | D7's premise "only three bus event types are OneWifi callbacks" was false; at least 17 come from OneWifi callbacks in `em_agent.cpp` | Confirmed. D7 rewritten from a full inventory of all 71 enumerators (`bus-event-inventory.md`): five categories and one replacement type per role |
| 6 (partial) | Typedef total was 364; it is 365, because the scan missed the function-pointer typedef `em_editor_callback_t` (`em_base.h:3850`) | Confirmed. Counts corrected (365 / 289 / 48 / 28), row added to the coverage table, scan description updated |
| 16 | "Exactly one owner" contradicted the `em-cmd + em-agent` rows | Confirmed. Both bus-event typedefs are now "replaced (D7)", and each replacement type has one owner |
| 17 | Strict `InterfaceName` breaks upstream placeholder and unset names | Confirmed (e.g. 17-byte `agent_interface_1` in the tests). `Interface.name` is now `Option<InterfaceName>`; the placeholder tests are dropped as an intentional difference; loaders must report invalid names |
| 18 | `--check` isn't reproducible if the fixture embeds the compiler version | Confirmed. The fixture holds only stable provenance; compiler details go in a separate README that isn't regenerated |
| 19 | "`em_cmd.cpp` refers to `em_t`" is false: the only occurrence is a comment at `src/cmd/em_cmd.cpp:185` | Confirmed. Claim removed; D7 cites the real coupling (`inc/em_cmd.h:22-28`) |
| 20 | "71 bus event types" counts the `_max` sentinel | Confirmed. Now "71 enumerators: 70 values plus `_max`" |
| 21 | Spec claims (Table 15 6 GHz, Table 23 reserved values) can't be verified without the spec | The reviewer didn't have the spec. I checked both against the spec v6.0 text: Table 15 defines `0x03: 802.11 6 GHz`, and Table 23 lists `0x02 – 0xFF: Reserved`. Consequences now require the Step 1 classification to be checked against the spec |

### Revision 2 (review of revision 1)

The review of revision 1 returned 15 findings. Each was checked against the upstream source before
being accepted.

| # | Finding | Resolution |
|---|---|---|
| 1 | Golden test would turn sentinels into protocol constants | D3 and D4: value classes; sentinels and internal codes go in `EXCLUDED` |
| 2 | `em_event_t` ownership creates a dependency cycle | D7. The suggested fix ("`em-agent` owns bus events") was also wrong, because bus events are general management requests. Requests are owned by `em-cmd`, and OneWifi callbacks are translated by `em-agent` |
| 3 | Band, profile and service are read directly from the wire | D3: all six are open newtypes, and the codec decides how to handle unknown values |
| 4 | "No extras" can't be enforced without reflection | D4: declarative table generates `ALL`, plus an explicit `ADDITIONS` list |
| 5 | Golden data must come from the pinned commit | D4: `git show <hash>:…`, count assertions, `--check` regeneration |
| 6 | Counts were wrong or ambiguous | Context: 122 TLV enumerators (121 + `_max`), 198 bitfields; typedef total corrected again in revision 3 |
| 7 | Line ranges were misleading | Context and D1: per-type coverage table instead of ranges |
| 8 | "≤ 31 bytes, upstream's limit" was wrong | D5: invariant chosen by the Rust API, following the Linux `dev_valid_name` rule |
| 9 | READMEs contradict the note | Updated in the same change |
| 10 | Some header families had no owner | D1 and the coverage table: link-layer → `em-transport`; WSC codes and validation metadata → `em-msg` |
| 11 | `MacAddr` parsing doesn't match upstream | D5: accepts both upstream forms; rejecting malformed input is an intentional difference |
| 12 | API traits and conversions unspecified | D5 |
| 13 | Acceptance criteria too thin | Acceptance criteria 1 and 3–6 |
| 14 | Bitfield explanation too broad | Context, item 1: allocation depends on compiler, ABI and byte order |
| 15 | Golden data provenance | D4: provenance and SPDX header in the fixture |
