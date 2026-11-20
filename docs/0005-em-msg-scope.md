# 0005: Scope and design rules for `em-msg`

| | |
|---|---|
| Status | **Accepted** (2026-11-18, user), rev 2 after two review rounds plus round-2 wording fixes. Amendment A1 (2026-11-20, note 0009) |
| Date | 2026-11-18 |
| Upstream | `rdkcentral/unified-wifi-mesh` @ `ba4180ba36748629f10c20babc4320dd69c9db4f` |
| Affects | `em-msg`; boundaries with `em-transport` (Ethernet header, size budgets) and `em-core` (TLV bodies, profile policy) |

## Context

`STATUS.md` maps `em-msg` to `src/em/em_msg.cpp` (1503 lines) and `inc/em_msg.h` (1190 lines).
[`docs/em_base-coverage.md`](em_base-coverage.md) assigns 216 entries of `inc/em_base.h` to
`em-msg`: the framing structs `em_tlv_t`, `em_cmdu_t` and `em_attrib_t`, validation metadata, and
the rest mostly TLV body structs plus some enums whose wire role is still to be confirmed (e.g.
`em_mlo_mode_bitmap_t`, `em_channel_pref_reason_t`). (`em_raw_hdr_t`, the Ethernet header, is assigned to `em-transport`.)

Upstream `em_msg.cpp` does four separate jobs:

| Job | Upstream | What it does |
|---|---|---|
| CMDU header | `em_cmdu_t` (`em_base.h:464`), `em_msg_t::add_1905_header` (`em_msg.cpp:503-538`) | The 8-byte CMDU header. `add_1905_header` first writes the 14-byte Ethernet header (`dst`, `src`, EtherType `0x893A`), then the CMDU: version 0, reserved byte 0, big-endian message type and id, fragment id 0, `last_frag_ind = 1`, and the relay flag (see below). |
| TLV framing | `em_tlv_t` (`em_base.h:458`), `get_tlv`, `get_first_tlv`, `get_next_tlv`, `add_tlv` (`em_msg.cpp:387-501`) | 1 type byte, a 2-byte big-endian length, then the value. A message ends with End of Message (type 0). |
| Message builder | `em_msg_builder_t` (`em_msg.h:1110`, `em_msg.cpp:540-641`) | Appends to a buffer with a capacity (`EM_MAX_MSG_SZ`, Ethernet header included: 1514 raw, 65531 with `AL_SAP`; `em_base.h:62-70`) and a TLV value limit of 65535. |
| Validation | `em_tlv_member_t`, `em_msg_t::validate`, 70 table functions for 69 message types (`em_msg.cpp:643-1176`; AP-Autoconfiguration WSC has two, M1 and M2) | Per message type, a list of TLVs, each **mandatory**, **optional** or **bad** (must not be present), often depending on the Multi-AP profile, plus a minimum length. 179 rows; 6 tables are empty. |

Plus field readers (`get_radio_id`, `get_profile`, ..., `em_msg.cpp:34-386`) and the TLV bodies.

### Upstream facts that shape the port

- **No CMDU reassembly in the mesh layer.** No mesh code reads `em_cmdu_t::frag_id`; the built-in
  senders set `last_frag_ind = 1`. The C++ AL-SAP client splits and reassembles SDUs with its own
  fragment fields (`src/al-sap/al_service_access_point.cpp:146-282`).
- **Relay flag.** `add_1905_header` sets it for exactly AP-Autoconfiguration Search,
  AP-Autoconfiguration Renew and Topology Notification (`em_msg.cpp:513-522`). But the Renew
  sender in `em_configuration.cpp:850-885` builds its own header with
  `relay_ind = (dst_mac[0] & 0x01) ? 1 : 0`, so a unicast Renew is not relayed. The rule is a
  property of the call site, not of the message type alone.
- **Bit layout.** In the flags byte, last-fragment is `0x80` and relay is `0x40` (GCC,
  little-endian; checked with a probe by the reviewer). The other six bits are reserved and zero.
- **Size budgets.** `send_frame` sends the whole buffer on the raw socket and strips the 14-byte
  Ethernet header on AL-SAP, passing the MAC addresses separately (`src/em/em.cpp:789-835`). So the
  CMDU-only budgets are **1500 bytes (raw)** and **65517 bytes (AL-SAP)**, CMDU header and End of
  Message included.
- **Validation input.** `validate` takes the whole frame, Ethernet header included, and reads the
  TLVs after the 22 header bytes (`em_msg.cpp:649-655`).
- **Profile.** Callers choose the profile used for the table. Most pass `em_profile_type_3`
  (e.g. `em_capability.cpp:260`); Client Steering Request passes `em_profile_type_2`
  (`em_steering.cpp:550`); on **receive**, Autoconfiguration Search/Response, Topology Response and AP
  Metrics Response use the peer's profile, falling back to profile 1 (`em_configuration.cpp:2414-2434`,
  `6304-6314`, `6409-6420`; `em_metrics.cpp:1035-1037`, `2948-2956`), while their **transmit**-side
  validators pass profile 3 (`em_configuration.cpp:1860`, `6465`, `6720`). The table compares numerically: `m_profile > em_profile_type_1` or
  `> em_profile_type_2`.
- **WSC M1 or M2.** For AP-Autoconfiguration WSC, the constructor reads the first WSC Message Type
  attribute (id `0x1022`, value 4 = M1, 5 = M2) inside the WSC TLV
  (`em_configuration.cpp:6053-6092`) and installs the M1 or M2 table. Any other value, or no
  attribute, installs no rows. The same selector drives `em-core` dispatch.
- **What validation accepts** (`em_msg.cpp:643-708`):
  - The TLV walk stops at the first TLV whose **type** is End of Message; End of Message must have a complete 3-byte header,
    but its length field and any bytes after it are not checked (on the raw socket, Ethernet padding
    may follow).
  - A TLV whose length runs past the buffer is rejected; no End of Message is rejected.
  - TLVs that are not in the table are accepted.
  - **Length is checked only for mandatory TLVs, and only on the first instance of a type.** Upstream
    senders rely on this: Topology Response always sends AP MLD Configuration, which is 1 byte with
    no AP MLDs, below the optional row's minimum of 64 (`em_configuration.cpp:1380-1425`, appended
    unconditionally at `1813-1820`); AP Metrics Response can send a zero-length AP Metrics TLV after a
    valid one (`em_metrics.cpp:1508-1592`, zero length at `1933-1990`).
  - The minimum length counts the 3-byte TLV header: the check is `3 + value_len < min_len`.
  - Message types with an empty table, and types with no `case` (e.g. BSTA MLD Configuration
    Request/Response, Available Spectrum Inquiry), get framing checks only.

### Upstream defects found

1. **State reuse.** `validate` sets `m_present` and increments `m_num_errors` and never resets them.
   Calling it again (e.g. after `set_m1`) can report stale presence, and repeated failures can
   overrun the 64-slot error array (`em_msg.cpp:670-705`, `em_msg.h:76-79`).
2. **Unchecked subtraction.** The WSC case subtracts the 22 header bytes from `len` before any
   length check (`em_msg.cpp:1195-1197`).
3. **Cleanup only:** `htons` is used where `ntohs` is meant in the length check (same result on all
   targets).

## Decision

### D1. Scope of `em-msg`, in steps

This note covers **Steps 1 and 2**.

| Step | Contents |
|---|---|
| 1 | Golden data: the 70 validation tables, extracted from `em_msg.cpp` by a generator |
| 2 | Crate: CMDU header, TLV reading and writing, message builder, validation, and the WSC Message Type selector |
| later | TLV bodies and the other field readers, each ported with the `em-core` area that uses it |

**Not in `em-msg`:** the Ethernet header (in `em-transport`, which keeps the MAC addresses next to
the CMDU buffer); SDU fragmentation (AL-SAP / `ieee1905-rs`); typed TLV bodies until their consumer
is ported; the policy for a received Profile TLV (in `em-core`, see D5).

### D2. Rust API (sketch)

Synchronous, depends only on `em-types`, no `unsafe`, no clones (0004 R1, R2). Parsing borrows the
input and never copies TLV values.

- **Header.** `struct CmduHeader { msg_type: MsgType, msg_id: u16, frag_id: u8, last_fragment: bool, relay: bool }`
  (version and reserved bits are written as 0 and ignored on read).
  `CmduHeader::parse(&[u8]) -> Result<(CmduHeader, &[u8]), FramingError>`; `write(&self, &mut Vec<u8>)`.
  `fn default_relay(MsgType) -> bool` reproduces `add_1905_header`'s rule; the caller decides the
  final flag (so the unicast Renew rule stays with its sender in `em-core`).
- **TLVs.** `struct Tlv<'a> { tlv_type: TlvType, value: &'a [u8] }`. `struct TlvIter<'a>` walks
  the TLV area and yields `Result<Tlv<'a>, FramingError>`. It stops after End of Message, and after
  yielding one error it yields nothing more.
- **Builder.** `MsgBuilder::new(header: &CmduHeader, budget: usize)` writes the header first, so a
  message always has exactly one header. `tlv(TlvType, &[u8])` appends; it refuses End of Message
  and values over 65535 bytes. `finish(self) -> Result<Vec<u8>, BuildError>` appends End of Message
  and moves the buffer out. The first error is kept and returned by `finish`. Budgets: 1500 (raw)
  and 65517 (AL-SAP), CMDU only.
- **WSC selector.** `fn wsc_msg_type(tlvs: &[u8]) -> Option<u8>`, with exactly upstream's walk
  (`em_configuration.cpp:6053-6092`):
  - TLVs are walked in order until End of Message; a TLV whose length runs past the buffer ends the
    walk with `None`.
  - Inside each WSC TLV, attributes (2-byte id, 2-byte length, value; big-endian) are walked in
    order. An attribute whose length runs past the TLV ends the walk of **that TLV only**; the next
    WSC TLV is still searched.
  - The first Message Type attribute (`0x1022`) with length ≥ 1 wins, and its first value byte is
    returned. A Message Type attribute with length 0 is skipped.
  - Validation uses the M1 table for 4 and the M2 table for 5. Any other value or `None` installs
    no table rows (framing checks only), and upstream dispatch calls neither handler.
- **Validation.** `enum Requirement { Mandatory, Optional, Bad }`. Each table row is
  `{ tlv_type, requirement: Rule, min_len: u16, spec_ref: &'static str }`, where
  `Rule` is either a fixed requirement or "if profile > N then A else B" (N is 1 or 2), exactly as
  upstream writes it. `ProfileType` is compared by its raw number, so profile 0 behaves like 1 and
  4 or more like 3, as upstream's `>` does.
  `fn validate(cmdu: &[u8], profile: ProfileType) -> Result<(), ValidateError>` reads the message
  type from the header (one source), picks the table (the WSC selector for AP-Autoconfiguration
  WSC), and returns `ValidateError::Framing(FramingError)` or
  `ValidateError::Table(Vec<Violation>)`. A `Violation { tlv_type, kind: Missing | TooShort | NotAllowed, spec_ref }`
  is reported at most once per table row. Validation keeps no state between calls.

### D3. Behavioural parity rules

Default behaviour matches upstream, including what it accepts:

- Wire format identical: header layout, `0x80` / `0x40` flag bits, big-endian lengths, End of
  Message `00 00 00` when building.
- Validation: same rows, order, per-profile rule and minimum length (except the 25 spec minimums
  below); mandatory TLVs are
  length-checked on their first instance only; optional TLVs are not length-checked; unknown TLVs
  are accepted; walk stops at End of Message by type, ignoring its length and trailing bytes; empty
  tables and unlisted types get framing checks only; WSC with no M1/M2 selector gets framing checks
  only.

**Intentional differences** (only where upstream misbehaves, never stricter on valid input):

- Validation is stateless (fixes defect 1) and checks lengths before subtracting (defect 2).
- Errors are returned as values, not logged.

Stricter checks (length of every instance, optional TLV lengths) are **not** added now. Upstream's
own senders would fail them. Recorded as a follow-up to revisit with the spec.

**Intentional difference ([0008](0008-tlv-body-codec.md) D8, 2026-11-19): spec minimum lengths.**
For 25 mandatory rows, upstream's minimum length rejects TLVs that the EasyMesh or IEEE 1905.1
table allows. Usually a count is 0, a conditional field is absent, or a failed scan carries no
results. `spec_min.rs` lowers those minimums to the spec minimum, with the arithmetic for each.

The rows in `tables.rs` stay upstream's, still checked by the golden test against the fixture.
Tests tie each override to its upstream row, so a fixture change fails loudly.

Rules:
- **Lower only.** No minimum is raised; the 18 (TLV, minimum) pairs where upstream is laxer than the spec (they match 24 upstream rows, 9 of which can be mandatory) stay
  as they are.
- **The zero case.** A count with no stated minimum is taken to allow 0. Each entry says whether
  its zero case is explicit in the spec text or inferred this way.
- **Not lowered.** Rows whose spec minimum is unsure keep upstream's value: WSC (its content is
  defined in WSC v2.0.7, not available), Link Metric Query (Table 6-16 contradicts itself), and
  rows that depend on DPP or IEEE 802.11 documents that are not available.
- **Body decoders.** A TLV that passes validation may be shorter than upstream's struct, so body
  decoders (note 0008) bound every read themselves.

Data: the audit in [tlv-spec-audit.md](tlv-spec-audit.md).

### D4. Golden data for the validation tables (Step 1)

- `tools/gen-upstream-msg-tables` reads `src/em/em_msg.cpp` at the pinned SHA with
  `git --no-replace-objects show`, and `inc/em_base.h` for one constant.
- **Tables.** Every `void em_msg_t::<name>()` between `autoconfig_search` and `i1905_ack` is a
  table, including empty ones. Rows are `em_tlv_member_t(<tlv type>, <requirement>, "<spec>", <min len>)`
  in order, duplicates kept (Combined Infrastructure Metrics lists Transmitter Link Metric and
  Receiver Link Metric twice each).
- **Grammar.** TLV type: any `em_tlv_type_t` name in the em-types fixture (one row uses
  `em_tlv_eht_operations`, which lacks the usual `em_tlv_type_` prefix). Requirement: `mandatory` | `optional` | `bad` | `(m_profile > em_profile_type_N) ? A : B`
  with N in {1, 2}. Minimum length: a decimal literal, `EMEX_MIN_EXT_DEV_INFO_SZ` (read from
  `inc/em_base.h:271`, 16), or `static_cast<int>(sizeof(em_tlv_t) + 2)` (5). Whitespace is free.
  Anything else is an error.
- **Mapping.** From the constructor's `switch` (`em_msg.cpp:1187-1485`): each `case <msg type>:`
  to the function it calls (names differ, e.g. `em_msg_type_ap_mld_config_resp` →
  `ap_mld_config_rsp`). The `em_msg_type_autoconf_wsc` case maps to two tables keyed `m1` and `m2`.
- **Checks before writing:** 70 tables, 69 message types, 179 rows, 6 empty tables; every table is
  mapped once; every TLV and message type name exists in the em-types fixture.
- **Output:** `crates/em-msg/tests/data/upstream_msg_tables.tsv`, with a provenance header, then
  the columns `msg_type wsc table row tlv_type rule min_len spec`: one line per row (`row` is 0-based)
  and one line of `-` fields for each empty table, in switch-case order. The WSC M1/M2 key comes
  from the `if` guard constants, not the function names. `--check` compares byte for byte.
  Exit codes 0 / 1 / 2.
- The Rust test asserts the Rust tables equal the fixture.

### D5. Decisions taken in this revision, and open questions for the user

Taken (review round 1): the length rules stay as upstream (D3); the WSC selector is in Step 2; the
profile is an explicit parameter compared numerically.

Resolved (2026-11-18, user: yes to all three):

1. **Ethernet header in `em-transport`, not `em-msg`.** Accepted.
2. **TLV bodies and field readers later, per consumer.** Accepted.
3. **Received Profile TLV.** Upstream's reader accepts a 1- or 4-byte value and rejects raw values
   ≥ 4 (`em_msg.cpp:131-151`); some handlers then fall back to profile 1. 0001 A1 notes the spec
   says to assume our own profile. This is receive policy, so it moves to `em-core` with the
   reader. Accepted.

## Amendments

### A1 (2026-11-20, note 0009): TLV bodies and field readers now, not per consumer

The user chose to complete `em-msg` before its consumers exist. [0009](0009-tlv-bodies.md)
replaces D1 decision 2 and the "later" row of the D1 table:
- every TLV body, the WSC plain attributes and the field readers with production callers are
  ported in `em-msg` now (0009 D1, D6);
- two readers with no production caller (`get_tlv(em_tlv_t*)`, `get_client_mac_info`) are not
  ported;
- receive *policy* (including the Profile fallback, D5.3) stays in `em-core`.

D3 changes in one place. 0009 D3 (audit line 321; user decision) has the encoder write a
2-octet Link Metric Query for "all neighbors". Batch B02 therefore:
- lowers that row's minimum from 11 to 5 in `spec_min.rs`, with an explicit zero case;
- raises the override count from 25 to 26;
- removes Link Metric Query from D3's "Not lowered" list.

The count of `em_base.h` rows assigned to `em-msg` is 219, not 216 (three wire-only types moved
from `em-dm` on 2026-11-19).

## Acceptance criteria (Steps 1 and 2)

1. `crates/em-msg` is a workspace member depending only on `em-types`, using the workspace lints.
   On stable and 1.85.0: `cargo build --all-targets`, `cargo test --all-targets`,
   `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`,
   `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`. Review covers the R2 cases clippy cannot see.
2. Generator: `--check` reproduces the fixture; unit tests cover both requirement forms, both
   non-literal minimum lengths, the WSC M1/M2 mapping, a renamed mapping, an empty table, a
   duplicate row, a malformed row in a full-size source, counts, stale `--check` and
   validate-before-write.
3. The Rust tables equal the fixture; a bite test shows one changed row fails.
4. Framing tests: header round trip for every flag combination; `default_relay` for every message
   type; truncated header; truncated TLV; length past the buffer; no End of Message; a truncated End of
   Message (fewer than 3 bytes: rejected); a complete End of Message with a non-zero length
   (accepted); bytes after End of Message (accepted) — each as its own test; the iterator stops after one
   error; builder: budget exceeded at a TLV and at End of Message, then `finish`; End of Message
   refused in `tlv()`; the 65535 limit with a large budget.
5. Validation tests at profiles 0, 1, 2, 3 and 4, using real message types (AP-Autoconfiguration
   Search and Response, WSC M1 and M2, Topology Response, AP Metrics Response): valid passes;
   each applicable kind of violation is reported once per row;
   a present zero-length mandatory TLV whose minimum is 3 or less passes (upstream compares
   `3 + value_len`); calling `validate` twice on different input gives independent results; the two upstream sender cases in
   "What validation accepts" pass; unknown TLVs pass; an empty table and an unlisted type get
   framing checks only; WSC with no selector gets framing checks only.
6. Bytes match upstream: the CMDU part of `add_1905_header` output with relay on and with relay off,
   and one complete message, produced by upstream code compiled as a small C++ program.
7. `STATUS.md`, the crate README and the root README are updated; the stricter-length follow-up is
   recorded.
