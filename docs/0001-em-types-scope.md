# 0001: Scope and design rules for `em-types`

| | |
|---|---|
| Status | **Accepted** (2026-11-18) |
| Date | 2026-11-18 |
| Upstream | `rdkcentral/unified-wifi-mesh` @ `ba4180b` |
| Affects | `em-types`; also which crate owns each part of `inc/em_base.h` |

## Context

`STATUS.md` maps `em-types` to `inc/em_base.h` and `inc/ieee80211.h`. Neither header is a
self-contained component.

### `inc/em_base.h`: 3,922 lines, Apache-2.0

The header that nearly every upstream file includes. It has no functions, only declarations:
roughly 260 `#define`s, 48 enums, ~300 struct typedefs (208 lines mention `packed`) and 4 unions.
(Counts are grep-based approximations.) It mixes four layers:

| Lines (approx.) | Layer | Examples |
|---|---|---|
| 1–430 | Constants, string/buffer typedefs, raw frame headers | `em_interface_name_t`, `em_short_string_t`, `em_raw_hdr_t`, `llc_hdr_t` |
| 430–2340 | **Wire protocol**: message/TLV type codes and packed TLV body layouts | `em_msg_type_t` (72 values), `em_tlv_type_t` (121 values), `em_cmdu_t`, `em_tlv_t`, ~200 TLV body structs |
| 2340–2400 | State-machine states | `em_state_t` (59 values) |
| 2400–3150 | **Data-model records** (in memory, not on the wire) | `em_interface_t`, `em_device_info_t`, `em_radio_info_t`, `em_bss_info_t`, `em_sta_info_t` |
| 3150–3870 | **Commands, events, IPC, DB** | `em_cmd_type_t` (53), `em_bus_event_type_t` (70), `em_cmd_params_t`, `em_bus_event_t`, `em_event_t`, `db_*`, `em_cli_params_t` |

It also pulls in types from outside this codebase:

- **OneWifi / HAL:** `wifi_webconfig.h`, plus `wifi_BeaconReport_t`, `wifi_channelBandwidth_t`,
  `wifi_ieee80211Variant_t`. `mac_address_t` and `mac_addr_str_t` are not defined in
  unified-wifi-mesh either; they come from the OneWifi/HAL headers.
- **OpenSSL:** `EVP_PKEY`, `EVP_PKEY_CTX`, `EC_KEY`
- **libuuid:** `uuid_t`
- **EasyConnect:** `ec_base.h`

### Upstream properties that must not be copied

1. **Bitfields inside packed wire structs.** There are 194 bitfield members. One example is the CMDU header:

   ```c
   typedef struct {
       unsigned char ver; unsigned char reserved;
       unsigned short type; unsigned short id; unsigned char frag_id;
       unsigned char reserved_field:6;
       unsigned char relay_ind:1;
       unsigned char last_frag_ind:1;
   } __attribute__((__packed__)) em_cmdu_t;
   ```

   C bitfield bit order is implementation-defined. With GCC on a little-endian target,
   `last_frag_ind` lands in bit 7, which matches the wire format. The header has no
   `__BYTE_ORDER` guards, so this layout is only correct on little-endian ABIs.

2. **Implicitly numbered wire enums.** `em_msg_type_t` assigns only 5 of its 72 values
   explicitly, and the rest follow from declaration order. For example, it starts at
   `em_msg_type_topo_disc = 0x0000` and later jumps to `em_msg_type_avail_spectrum_inquiry = 0x8049`.
   An enumerator inserted or reordered upstream silently changes values that go on the wire.

3. **Fixed-size C string arrays** (`em_interface_name_t[32]`, `em_short_string_t[64]`, …) and
   zero-length trailing arrays (`value[0]`) as the general way to model strings and payloads.

## Decision

### D1. Split `em_base.h` by owner; `em-types` gets only the shared vocabulary

| Part of `em_base.h` | Owning crate | When ported |
|---|---|---|
| Shared vocabulary (see D2) | **`em-types`** | now |
| CMDU/TLV headers and TLV body layouts | `em-msg` | with `em-msg` |
| Data-model records (`em_*_info_t`, …) | `em-dm` | with `em-dm` |
| `em_state_t`, `em_event_t`, event types | `em-core` | with `em-core` |
| `em_cmd_type_t`, `em_cmd_params_t`, CLI params | `em-cmd` | with `em-cmd` |
| Orchestration enums (`em_orch_state_t`, `dm_orch_type_t`) | `em-orch` | with `em-orch` |
| `db_*` types | `em-db` | with `em-db` |
| `em_bus_event_*`, OneWifi-facing types | `em-agent` (OneWifi adapter) | with `em-agent` |
| OpenSSL / uuid-bearing types (`em_crypto_info_t`, …) | `em-crypto` / `em-easyconnect` | with those crates |

Each type is ported together with the code that uses it, never ahead of it.

### D2. Contents of `em-types`

These are types that two or more crates need, and that have no dependency outside the crate.

| Rust type | Replaces (upstream) | Notes |
|---|---|---|
| `MacAddr` (`[u8; 6]` newtype) | `mac_address_t`, `mac_addr_str_t` + `dm_easy_mesh_t::macbytes_to_string`/`string_to_macbytes` | `Display`/`FromStr` as `aa:bb:cc:dd:ee:ff`; `is_multicast`, `BROADCAST`, the 1905 multicast address `01:80:c2:00:00:13` |
| `InterfaceName` | `em_interface_name_t` (`char[32]`) | Validated: non-empty, ≤ 31 bytes (upstream's limit including NUL) |
| `Interface { name, mac, media }` | `em_interface_t` | Plain struct, not packed |
| `MsgType(u16)` | `em_msg_type_t` | Wire code, see D3 |
| `TlvType(u8)` | `em_tlv_type_t` | Wire code, see D3 |
| `FreqBand` | `em_freq_band_t` | 1905.1 Table 6-23 (2.4 / 5 / 60 GHz), plus 6 GHz |
| `MediaType(u16)` | `em_media_type_t` | 1905.1 media type, 2 octets on the wire |
| `ProfileType` | `em_profile_type_t` | Multi-AP Profile 1/2/3 |
| `ServiceType` | `em_service_type_t` | Controller / Agent / CLI; the 0xA0/0xA1 "EM+" vendor values are kept |

`em-types` is a leaf crate with **no dependencies**. It uses the 2024 edition and `std` (making
it `no_std` is not a goal at this point).

### D3. Codes received from the network are newtypes, not Rust enums

A value parsed from a received frame is untrusted. A Rust `enum` cannot hold a discriminant
that isn't listed, and converting an unknown integer into one is undefined behaviour. So:

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MsgType(pub u16);

impl MsgType {
    pub const TOPOLOGY_DISCOVERY: Self = Self(0x0000);
    // … one associated const per known value, each written out explicitly …
    pub const fn name(self) -> Option<&'static str> { /* match on known values */ }
}
impl core::fmt::Debug for MsgType { /* "TOPOLOGY_DISCOVERY(0x0000)" or "MsgType(0x1234)" */ }
```

Unknown values round-trip unchanged. Deciding what to do with an unknown value (ignore, error,
forward) is the job of `em-msg`/`em-core`, not `em-types`.

Types that never come straight off the wire, or that are always validated first (`FreqBand`,
`ProfileType`, `ServiceType`), are ordinary Rust `enum`s with `TryFrom<u8>` that returns an error
for unknown values.

### D4. Every wire value is written out explicitly and checked against upstream

Every associated const gets a literal value. No value depends on declaration order.

A **golden test** pins the values:

1. A small script extracts the `typedef enum { … } <name>;` blocks for the D2 enums from
   upstream `em_base.h` @ `ba4180b`. It doesn't need the OneWifi headers.
2. It compiles them in a tiny C program that prints `name = value` for every enumerator.
3. The output is committed as test data (e.g. `crates/em-types/tests/data/upstream_enums.txt`),
   along with the commit hash and the command that produced it.
4. A Rust test checks that every upstream enumerator has a Rust const with the same value, and
   that Rust defines no extra value. Any intentional difference is listed in the test with a reason.

Where the spec and upstream disagree on a value, the test makes the disagreement visible
instead of hiding it. Resolving it is a separate, recorded decision.

### D5. No C memory layout in `em-types`

- No `#[repr(C, packed)]`, no bitfields, no zero-length arrays, no `unsafe`.
- No fixed-size `char[N]` buffers in the public API. Length limits that the wire format or
  upstream enforce become validation in constructors and are checked at the codec boundary.
- Converting to and from wire bytes (byte order, bit positions such as `last_frag_ind` = bit 7)
  happens explicitly in `em-msg`, independent of the target's byte order.

### D6. `ieee80211.h` is not part of `em-types`

`inc/ieee80211.h` (1,744 lines) is FreeBSD net80211 code under BSD-2-Clause-FreeBSD: about 564
`#define`s and 79 802.11 frame structs. It is used where 802.11 frames are built or parsed
(action frames, beacon/BTM handling), which belongs to `em-core`/`em-agent`. It will be dealt
with when those crates are ported. The options then are to use an existing Rust 802.11 crate,
or to port only the definitions actually used and keep the BSD notice.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Port `em_base.h` 1:1 into `em-types` with `#[repr(C, packed)]` structs | Recreates a ~4,000-line crate that everything depends on. Carries over the non-portable bitfield layout. Packed structs in Rust require `unsafe` to take references to fields. |
| Generate it with `bindgen` | Needs the OneWifi, OpenSSL and uuid headers to build. Produces C-layout types and bitfield accessors, which have the same portability problem. Produces no idiomatic API. |
| Rust `enum` for message/TLV types | Cannot represent unknown values received from the network (D3). |
| Zero-copy structs (e.g. `zerocopy`) for fixed-size wire records | Possibly useful inside `em-msg` for fixed-layout TLVs. Deferred to `em-msg`'s design note; not relevant to `em-types`. |

## Consequences

- `em-types` is small (a few hundred lines) and can be finished and tested on its own.
- `STATUS.md` changes: `em-types` maps to *part* of `em_base.h`. `ieee80211.h` moves to
  "deferred, decided with `em-core`/`em-agent`".
- We need a way to make sure every type in `em_base.h` ends up somewhere. A coverage checklist
  (`docs/em_base-coverage.md`: upstream type → owning crate → status) will be added when the
  `em-types` port starts.

## Acceptance criteria for the `em-types` port

1. `crates/em-types` has a `Cargo.toml` and is a member of the workspace. `cargo build`,
   `cargo test`, `cargo clippy -- -D warnings` and `cargo fmt --check` all pass.
2. It contains exactly the types listed in D2, has no dependencies, and has no `unsafe`.
3. The golden test (D4) covers `em_msg_type_t`, `em_tlv_type_t`, `em_freq_band_t`,
   `em_media_type_t`, `em_profile_type_t` and `em_service_type_t`, and passes.
4. `MacAddr` parsing/formatting and `InterfaceName` validation have unit tests, including
   rejection cases (bad length, bad hex, empty or overlong name).
5. A test proves that an unknown `MsgType`/`TlvType` value round-trips unchanged.
6. `STATUS.md` and `docs/em_base-coverage.md` are updated.

## Resolved questions

1. **Naming of associated consts.** Resolved: use the **spec names** (e.g.
   `AP_AUTOCONFIGURATION_SEARCH`). Each const's doc comment gives the upstream identifier
   (e.g. `` `em_msg_type_autoconf_search` ``) and, where one exists, the spec section, so both
   names are searchable.
2. **Recording the profile or release that introduced each value.** Resolved: **not now**. Add it
   if `em-msg` validation needs it.
