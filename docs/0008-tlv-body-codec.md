# 0008: How `em-msg` encodes and decodes TLV bodies

| | |
|---|---|
| Status | **Accepted** (rev 4; two review rounds on rev 3, round 2 by the Claude correctness reviewer; rev 4 not reviewed: two-round limit). Codec choice, D6, D7 and D8 decided by the user (2026-11-19). Amendment A1 (2026-11-20, note 0009) |
| Date | 2026-11-19 |
| Upstream | `rdkcentral/unified-wifi-mesh` @ `ba4180ba36748629f10c20babc4320dd69c9db4f` |
| Affects | `em-msg` (TLV bodies, added per consumer under 0005 D1); every consumer crate that ports a TLV reader or writer |

## Context

Note 0005 ported the CMDU header, TLV framing, builder and validation, and left TLV *bodies* to be
added later, each with the consumer that uses it (0005 D1, decision 2). `docs/em_base-coverage.md`
assigns 216 `em_base.h` types to `em-msg`; most are TLV body layouts. Each body needs a decoder and
an encoder. Upstream builds bodies by filling packed C structs and copying them into the frame, so
the bytes follow upstream (D6) except where the spec states a point explicitly (D7, D8).

The framing in `em-msg` is hand-written on byte slices (`split_at_checked`, `u16::from_be_bytes`).
It stays that way: it is small, verified against 76 upstream frames, and its edge cases are pinned
by tests. This note only decides how bodies are written.

Rules that apply: 0004 R1 (pure crates synchronous and dependency-light), R2 (no `.clone()`,
`.cloned()`, `clone_from`; `Clone` only on `Copy` types), R7 (`clippy.toml` enforcement), no
`unsafe`, Rust 1.85 support.

### Evidence (investigation, 2026-11-19)

A standalone prototype workspace compared ten approaches. Three real bodies from the pinned
`em_base.h` were implemented where each approach could express them:

- **AP Radio Basic Capabilities** (`em_ap_radio_basic_cap_t`, `em_base.h:552-557`; `em_op_class_t`,
  `:529-534`): nested counted lists.
- **BSTA MLD Configuration** (`em_bsta_mld_t`, `:3057-3071`; `em_affiliated_bsta_mld_t`,
  `:3049-3055`): bit-fields in three byte positions, reserved arrays of 17 and 19 bytes, a counted
  list of 32-byte entries. GCC packs the first declared bit-field into the least significant bit,
  and upstream declares reserved bits first (`reserved1 : 6` then `ap_mld_mac_addr_valid`, then
  `bsta_mld_mac_addr_valid`), so the last declared flag lands in `0x80`. A GCC probe of the pinned
  structs confirmed `bsta_mld_mac_addr_valid = 0x80` and `ap_mld_mac_addr_valid = 0x40` in byte 0.
- **AP MLD SSID record** (`em_ap_mld_ssids_t`, `:3013-3016`): a length byte and a variable tail.
  Defined but not referenced at the pin; used as a layout sample only.

Every prototype ran the same test suite per body: decode of known bytes; byte-exact encode of an
example built from the C layout; round trips of edge cases; every strict prefix is `Truncated`; an
over-long count is `Truncated`; an under-long count or extra byte is `TrailingBytes`; encoding more
than 255 elements is `CountTooLarge`; reserved bits ignored on decode and written as zero.
178 tests passed on stable and on 1.85.0 (rerun by the coordinator).

| Approach | Code lines (3 bodies) | Read / write | Notes |
|---|---|---|---|
| **In-house reader/writer** | 220 (63-line helper + 157) | both | No dependency, macro or `unsafe`; error kinds are ours |
| binrw 0.15 | 126 | both (derive) | `pad_*` seeks past the end of input and silently succeeds on truncated data; truncation kinds differ only in message text; warnings on 1.85 only |
| deku 0.20 | 112 | both (derive) | 28-crate tree incl. `bitvec`; every short input is `Incomplete` |
| winnow 1.0 | 202 | read only | Writer by hand: two idioms |
| nom 8 | 225 | read only | No release since 2025-01-26; awkward bit API |
| bytes / scroll | 193 / 222 | both, manual | Thin cursors; `bytes` `get_*` panic on short input; scroll cannot append |
| zerocopy / packed_struct / modular-bitfield | — | fixed layouts or flags only | Cannot express counted nested lists; zerocopy's derive puts `unsafe impl` into the user crate |

No candidate's macros generated `.clone()` or required `Clone`; all build on Rust 1.85.0. Not
covered by the prototypes: big-endian multi-byte fields and 16-bit counts (see D4).

### Upstream senders build the wire bytes, not the structs (review round 1)

The packed C structs are not a reliable wire layout. Several senders assemble bytes by hand, and
where they do, the sender is authoritative:

- HE Capabilities: the sender packs DL OFDMA into bit 0 of byte 2 (`b2 |= dl_ofdma_cap & 0x1`,
  `src/em/em.cpp:1443-1451`), while the struct declares a reserved bit first
  (`inc/em_base.h:1911-1927`), so a mask derived from the struct layout would be wrong.
- CAC Capabilities: the duration is written as 3 bytes, most significant first
  (`src/em/em.cpp:1972-1976`).
- Steering Request mixes byte orders in one body: `steering_opportunity_window` is assigned in
  host order (little-endian on the x86_64 target) while `btm_dissoc_timer` uses `htons`
  (`src/em/steering/em_steering.cpp:483-487`). TID-to-Link Mapping assigns `num_mapping` directly
  and leaves its payload as a TODO (`src/em/config/em_configuration.cpp:1666-1694`).
- Some bodies have accepted alternate lengths: the Profile parser accepts 1 or 4 bytes
  (`src/em/em_msg.cpp:139-141`); the BSS Configuration Response sender can emit a present,
  zero-length Backhaul STA MLD Configuration TLV (`em_configuration.cpp:1437-1442, 3771-3773`;
  `add_tlv` always writes the header, `em_msg.cpp:491-501`), so that body (`em_bsta_mld_t`, the
  prototype body above) must also accept an empty value.
- Some counts are independent of any serialized list: AP Radio Basic Capabilities has `num_bss`
  next to `op_class_num` (`em_base.h:552-557`; the sender counts BSS records separately,
  `em.cpp:1096-1113`); AP Metrics writes `num_sta` without a STA list
  (`src/em/metrics/em_metrics.cpp:1948-1961`).
- Vendor Specific is a 3-byte OUI followed by bytes bounded only by the TLV length
  (`em_base.h:1803-1806`); discovery emits the OUI-only form (`src/em/disc/em_discovery.cpp:74-79`).

## Decision

### D1. In-house reader and writer (decided by the user, 2026-11-19)

TLV bodies are decoded and encoded by hand-written functions over two small `em-msg` types:

- `Reader<'a>` borrows a TLV value: `u8`; `be_u16`, `be_u24`, `be_u32`; `le_u16`, `le_u32` (only
  for fields upstream writes in host order, see D6); `bytes(n) -> &'a [u8]`; `array::<N>()`;
  `mac`; `skip(n)`; `remaining() -> usize`; `rest() -> &'a [u8]` (takes everything left, for
  tails bounded by the TLV length); `finish()` fails with `TrailingBytes` if input remains. Every
  read uses `split_at_checked`: a read of `n > 0` bytes past the end is `Truncated`; a zero-byte
  read at the end succeeds.
- `Writer` owns a `Vec<u8>` with the same scalar set plus `bytes`, `zeros(n)`, and
  `count_u8(len)` / `count_u16(len)` that return `CountTooLarge` instead of truncating.
- Entry points per body: `Body::decode_value(value: &'a [u8]) -> Result<Body<'a>, DecodeError>`
  creates the reader over `Tlv::value`, decodes, and calls `finish()`; `Body::encode_value(&self)
  -> Result<Vec<u8>, EncodeError>` returns the bytes only if encoding succeeded, and the caller
  passes them to `MsgBuilder::tlv` (`crates/em-msg/src/builder.rs`). A failed encode publishes
  nothing.
- Errors are `em-msg` enums: `DecodeError::{Truncated, TrailingBytes, ...}` and
  `EncodeError::{CountTooLarge, ...}`; kinds may be added per body (for example an invalid
  enumerated value) but never replaced by message text.

No codec crate is added; `em-msg` keeps `em-types` as its only dependency (0005, 0004 R1).

### D2. Shape of one body

- **The wire layout comes from the pinned sender and receiver, field by field.** For each body the
  doc comment cites the struct lines and the sender (and receiver, if any) lines; byte order, bit
  positions, widths and reserved bits are taken from the code that writes and reads the bytes. Where
  the sender writes the struct directly, the struct layout is measured with a GCC probe of the pinned
  declaration (x86_64 LP64), not assumed. Mask and shift constants cite their source line.
- A body is `struct Body<'a>` with `impl<'a> Body<'a> { fn decode(r: &mut Reader<'a>) -> ... }`
  and `fn encode(&self, w: &mut Writer) -> ...`. Short-lived fields borrow from the frame with the
  named lifetime `'a`.
- **Counts:** a count that sizes a serialized collection is derived from the collection length on
  encode and checked on decode. A count that sizes nothing in the body (for example `num_bss`, AP
  Metrics `num_sta`) is an ordinary scalar field.
- **Accepted lengths** follow upstream per body: alternate accepted lengths (Profile: 1 or 4 bytes)
  and present-but-empty values (BSTA in BSS Configuration Response) are part of the body's
  contract and its tests.
- Reserved bits and bytes are skipped on decode and written as zero on encode.
- **Ownership boundary:** a stored body moves into owned data at a named function (0004 R2,
  0007 D3), and large stored records get the 0007 D3 size check at that decode handoff. Each body
  module gets the manual R2 duplication review (0004 R7).

### D3. Tests every body must pass

Shared test helpers, used per body with fixture-specific expectations:

1. a golden fixture from upstream's sender decodes to asserted field values, and encodes back to the
   same bytes;
2. truncation: every strict prefix that upstream does not accept as an alternate length is an
   error, and the expected error is stated per fixture (`Truncated` for a cut field; a nested count
   change may shift record boundaries, so its expected error is asserted for that fixture, not
   assumed);
3. an extra trailing byte is `TrailingBytes` unless the body has a remainder-bounded tail;
4. encoding a collection longer than its count field allows is `CountTooLarge`;
5. reserved bits: a fixture with nonzero reserved bits decodes to the same values and re-encodes
   with zeros (tested separately from the byte-exact round trip of item 1);
6. alternate accepted lengths and empty values (D2) each have a fixture.

### D4. Step 1 is the helper; bodies come with their consumer

Step 1 adds `Reader`, `Writer`, the error enums and the shared D3 test helpers to `em-msg`, with
unit tests. Typed bodies stay out of `em-msg` until their consumer is ported, as 0005 D1 requires.

The first consumer is expected to be the channel area of `em-core` (`src/em/channel/`). When it is
ported, its note must settle, for each body, the questions this review found:

- **Channel Scan Result**: sender `src/em/channel/em_channel.cpp:645-809`, receiver
  `fill_scan_result` (`em_channel.cpp:2291-2402`, called at 2439). They differ: the receiver accepts
  a body cut after the fixed header when `scan_status` is not success (2301-2310) and checks no
  lengths; the sender writes no bandwidth-length byte for bandwidths outside the five it knows
  (`default:` at 767) while the receiver always reads one (2348-2352); channel utilisation and a big-endian
  16-bit station count are present only when bit 7 of the BSS-colour byte is set (sender 772-793,
  receiver 2370-2385); the trailing scan-type byte uses only bit 7 (`EM_SCAN_TYPE_BIT`,
  `em_base.h:223`). Fixtures cover each branch.
- **CAC Status Report**: sender `em_channel.cpp:1367-1464`, no receiver at the pin; the active
  list carries a 3-byte big-endian countdown (1448-1450, `em_cac_active_t`, `em_base.h:1540-1544`);
  upstream's 8-bit counts wrap above 255 where the port returns `CountTooLarge` (to be recorded
  as an intentional difference there).
- For each body: the consumer entry point, a round trip through `TlvIter` and `MsgBuilder`
  including the 65 535-byte and budget limits, and the 0007 D3 size check at the decode handoff.
  Upstream-sender fixtures need a regeneration driver with `--check` (the CMDU vectors tool has
  none yet), or hand-derived fixtures with an independent recomputation.

### D5. Fallback

If ported bodies show the hand-written cost is too high, binrw is the fallback, subject to a new
decision covering its `pad_*` behaviour, error-kind fidelity, the 1.85-only warnings and the
proc-macro dependency.

### D6. Fields upstream writes in host byte order (decided by the user, 2026-11-19)

Upstream writes some multi-byte fields without `htons`/`htonl` (Steering Request
`steering_opportunity_window`, `src/em/steering/em_steering.cpp:483-485`; TID-to-Link
`num_mapping`, `src/em/config/em_configuration.cpp:1666-1694`), so on the x86_64 target they go out
little-endian, next to big-endian fields in the same body. **The port matches upstream's bytes:**
such fields are read and written with `le_u16` / `le_u32`. Each one is marked in the body's doc
comment as "host order in upstream (little-endian on x86_64), kept for interoperability; may be an
upstream defect", with the sender line, and listed in `STATUS.md` follow-ups so it can be revisited
if the EasyMesh specification shows network order. Tests pin the little-endian bytes with a
non-symmetric value.

### D7. Spec wins for bit layouts (decided by the user, 2026-11-19)

Where the pinned upstream sender places bits differently from the EasyMesh or IEEE 1905.1
table (bit positions inside an octet, reserved bits, field order inside a bit-field octet), the
port follows the specification on the wire and records the difference as an intentional
difference with the spec section and the upstream line. First case: AP HE Capabilities
(EasyMesh 17.2.10, Table 32), where upstream's sender (`src/em/em.cpp:1443-1451`) puts each
second-octet flag one bit too low (DL OFDMA at bit 0, SU beamformer at bit 6; the spec has them at
bits 1 and 7, bit 0 reserved); upstream's receivers read the value into a fixed struct
(`src/em/capability/em_capability.cpp:1582-1597`, `:1861-1875`). This narrows D2's "the
sender is authoritative" to what the spec leaves unstated; D6 (host-order fields kept
little-endian where the spec states no byte order) is unchanged. The spec audit of every TLV
records the remaining differences ([tlv-spec-audit.md](tlv-spec-audit.md)).

### D8. Spec wins wherever it is explicit (decided by the user, 2026-11-19)

D7 extends from bits to every point the specification states:
- field order and size;
- presence conditions and counts;
- string and SSID lengths;
- the TLV header itself;
- byte order where a TLV table names it;
- value ranges.

Where upstream differs, the port follows the spec. Where the spec text is silent on the point, the
port keeps upstream's bytes (D6).

Each body that differs from upstream records the difference in its doc comment, with the spec
section and the upstream line. Its tests pin the spec bytes.

This replaces earlier rules wherever the spec is explicit:
- D2's "taken from the code that writes and reads the bytes";
- D2's "accepted lengths follow upstream";
- the "sender is authoritative" rule in the Context section;
- D3 item 1's round trip of upstream's sender bytes;
- D3 items 2 and 6, which treat upstream's alternate lengths as the contract.

The fixtures for those points are spec bytes. Upstream's alternate lengths become receive-side
tolerances, which the consumer decides. A tolerance kept by the consumer gets its D3 item 6
fixture, and its truncation test (item 2) excludes it. Examples are the Profile TLV's 4 octets and the empty
Backhaul STA MLD value.

The decision for every audit finding is in the `port_rule` column of
[`data/tlv-spec-audit.tsv`](data/tlv-spec-audit.tsv):
- `SPEC`: the spec settles the point;
- `UPSTREAM`: the spec is silent;
- `UNSURE`: the wording needs a second look; the consumer decides.

Consequences named by the audit ([tlv-spec-audit.md](tlv-spec-audit.md)):
- SSID Length counts the SSID octets with no NUL (AP Operational BSS, BSS Configuration Report).
- DPP Chirp Value Hash Length is 1 octet.
- Beacon Metrics Query always carries `Num_Ids`.
- Agent AP MLD Configuration is a full TLV, header included. Receivers step each entry by
  `SSID Length`.
- TID-to-Link Mapping Policy carries its mapping octets.
- VHT Capabilities MCS maps are big-endian.
- AP Radio Basic Capabilities op-class groups are contiguous.
- AL MAC Address is exactly 6 octets.

Byte order: EasyMesh §3.1.2 points to IEEE 1905.1 for bit and byte ordering "unless otherwise
stated". The 1905.1 text on the project's spec server states no general byte order, so D6 still
applies to fields whose TLV table names no order.

Interoperability: on these TLVs the port's bytes differ from an unpatched upstream peer's. Each
consumer note records the effect on interop, and any receive-side tolerance it chooses.

## Amendments

### A1 (2026-11-20, note 0009): all bodies now; lenient decoders

[0009](0009-tlv-bodies.md) replaces D4's "bodies come with their consumer". All TLV bodies are
ported in `em-msg` now, in batches (0009 D8). The channel questions D4 left for `em-core` are
settled in batches B05 and B06. 0009 also:
- adds the `TlvBody` and `TlvBodyLenient` traits (0009 D2);
- turns D8's receive-side tolerances into separately named lenient decoders, limited to non-spec
  forms written by an upstream sender. An empty value decodes to `Ok(None)` (0009 D4);
- makes the double-built spec fixtures and `tools/check-body-fixtures` the D3 item 1 golden
  source, with upstream sender vectors where a sender can be compiled (0009 D7).

Correction: the empty Backhaul STA MLD Configuration value is written at
`em_configuration.cpp:3771-3773`; lines 1437-1442, cited in the Context, are a presence check.
The count of `em-msg` rows is 219, not 216.

## Acceptance criteria (Step 1)

1. `Reader` and `Writer` in `em-msg`, no new dependencies, no `unsafe`, no clones; build, test,
   clippy `-D warnings`, fmt and rustdoc pass on stable and 1.85.0.
2. Unit tests of the helper: a read of `n > 0` bytes at the end is `Truncated` and a zero-byte read
   succeeds; `rest()` returns the remainder and leaves the reader empty; `finish` with leftover bytes
   is `TrailingBytes`; `count_u8(256)` and `count_u16(65536)` are `CountTooLarge`; every scalar
   width (`u8`, `be_u16`, `be_u24`, `be_u32`, `le_u16`, `le_u32`) round-trips with a non-symmetric
   value in its stated byte order; a failed encode publishes nothing.
3. The D3 helpers exist as one shared test module, exercised by a test-only sample body that
   covers a counted nested list, bit flags with reserved bits, a remainder-bounded tail and an
   accepted empty value.
4. `STATUS.md` and the `em-msg` README are updated; the D4 requirements are recorded as follow-ups
   for the `em-core` channel step.

## Revision history

- rev 1: initial draft with the user's codec decision.
- rev 2 (review round 1): wire layout taken from senders and receivers, not struct layout (F01);
  per-field byte order, 24-bit and host-order helpers, open question D6 (F02); accepted alternate
  lengths and empty values (F03); independent counts stay scalar (F04); remainder-bounded tails,
  entry points and publish-on-success (F05); named lifetime and ownership boundary (F06); test
  expectations per fixture, zero-byte reads, reserved-bit test separated (F07); first consumer and
  fixtures named (F08).
- rev 2, D6 decided by the user: host-order fields match upstream's bytes (little-endian).
- rev 3 (review round 2, by the Claude correctness reviewer because the usual reviewer was
  rate-limited): Step 1 narrowed to the helper so bodies stay with their consumer (0005 D1);
  Channel Scan Result and CAC Status questions recorded for the `em-core` channel step; Backhaul
  STA MLD empty value and citations corrected. Not reviewed after these fixes: two-round limit.
- rev 4, decided by the user after the TLV spec audit: D7 (spec wins for bit layouts) and D8
  (spec wins wherever it is explicit; silent points keep upstream's bytes per D6).
