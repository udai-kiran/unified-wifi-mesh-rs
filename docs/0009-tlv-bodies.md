# 0009: Every TLV body, the WSC attributes and the field readers in `em-msg`

| | |
|---|---|
| Status | **Accepted** (2026-11-20, user), rev 3, after two plan review rounds; rev 3 fixes not re-reviewed: two-round limit. Scope and four design questions decided by the user (2026-11-20); remaining open points (D10) are decided per batch |
| Date | 2026-11-20 |
| Upstream | `rdkcentral/unified-wifi-mesh` @ `ba4180ba36748629f10c20babc4320dd69c9db4f` |
| Affects | `em-msg` (new `bodies`, `wsc` attribute and `readers` modules); `em-types` (two TLV codes); notes 0001 (amended: two added codes), 0005 (amended, A1) and 0008 (amended, A1); `docs/em_base-coverage.md`; `tools/` (one generator, one fixture check) |

## Context

Notes 0005 (D1, decision 2) and 0008 (D4) put TLV bodies and the message-level field readers
with their consumer: each body was to be ported with the `em-core` area that uses it. On
2026-11-20 the user chose to **complete `em-msg` now**: every TLV body, before any consumer
exists. This note records that change and how the work is done.

What exists: CMDU header, TLV framing, builder, validation (70 tables, spec minimums), the WSC
M1/M2 selector, and the body `Reader`/`Writer` with the shared D3 test helpers (`body.rs`,
`body_testkit.rs`). No upstream body is ported yet.

Inputs (task research, 2026-11-20, kept in the tracking folder):

- **Inventory of all 124 TLVs** (EasyMesh §17.2: 105; IEEE 1905.1 §6.4: 19). For each TLV it
  records the `em_base.h` structs, the upstream senders and receivers (file:line, resolved to the
  enclosing function), the audit rows by `port_rule`, the shape and complexity. Complexity:
  43 small, 53 medium, 28 large.
- **27 TLVs have no upstream sender or receiver** at the pin: §17.2.5, 14, 32, 33, 34, 42, 53, 54,
  55, 56, 68, 69, 71, 81, 87, 93, 99, 101; 1905.1 §6.4.6, 8, 9, 10, 11, 12, 13, 19, 20.
- **Codes.** `em-types` names 123 codes: 122 spec codes, End of Message included, plus vendor
  0xF2.
  - It has MLD Structure (0xE3) and Affiliated AP Metrics (0xE5); upstream's enum has neither.
  - Push Button Event Notification (0x12, 1905.1 §6.4.19) and Push Button Join Notification
    (0x13, §6.4.20) are in neither `em-types` nor upstream.
- **`em_base.h` rows assigned to `em-msg`: 219** (0005 and 0008 say 216; the coverage table and
  its summary say 219, after the three wire-only types moved from `em-dm`).
  - 8 are framing or validation metadata.
  - 11 are enums or codes used in body fields.
  - 196 are body structs and their nested records.
  - 3 are not wire types: the three `em_unassoc_work_*` agent work lists. A fourth,
    `em_ap_mld_ssids_t`, is a wire layout that nothing at the pin uses.
- **Field readers** (`em_msg.cpp:34-422`). Production callers are frame routing in
  `em_ctrl.cpp`/`em_agent.cpp` and `em_configuration.cpp`/`em_provisioning.cpp`. Callers pass the
  TLV area after the 22 header bytes, with an unchecked `len - 22`.
  - Only `get_supported_service`, `get_sta_mac` and the walk of `get_radio_id` are bounded.
  - `get_al_mac_address` copies the wire length into a 6-byte buffer (`:79`). The other walks
    wrap their unsigned length on a TLV that runs past the buffer.
  - `get_tlv(em_tlv_t*)` (`:34`) and `get_client_mac_info` (`:52`) have no production caller.
- **WSC.** M1/M2 are attribute lists built from `data_elem_attr_t` (`em_base.h:2256-2260`),
  with ids from `data_elem_attr_id_t` (`:2262-2330`) and the vendor extension sub-ids
  `vendor_ext_attr_id_t` (`:2332-2342`). The `em_attrib_t`/`em_attrib_id_t` pair (`:452`,
  `:773-790`) is used only by `dm_easy_mesh_t::commit_config`.
  - Built by `create_m1_msg` (`em_configuration.cpp:3305`) and `create_m2_msg` (`:2896`).
  - Encrypted Settings, the Key Wrap Authenticator and the Authenticator are computed at `:5842`
    and `:6028-6048` and checked at `:4871-4950`, with AES-128-CBC and HMAC-SHA256 (the primitives
    are in `em-crypto`).
  - `wsc.rs` has only the Message Type selector.
- **Golden sources.**
  - `tools/upstream-crypto-vectors` extracts pinned definitions by line range, compiles them
    against a declaration shim, and has `--check`.
  - Seven upstream TLV senders need only parameters and `em_base.h` types:
    `create_prof_2_tlv` (`em.cpp:1772-1797`), `create_error_code_tlv` (`em_configuration.cpp:3847-3869`;
    `em_metrics.cpp:2384-2411`), `create_client_info_tlv` (`em_capability.cpp:586-600`),
    `create_backhaul_bss_conf_policy_tlv` (`em_policy_cfg.cpp:336-349`), and the unassociated
    STA link metrics query and response (`em_metrics.cpp:2702-2734`, `2807-2867`).
    `create_btm_report_tlv` (`em_steering.cpp:500-525`) reads the current command
    (`get_current_cmd()`, `:504-505`), so it would need an `em_cmd_t` shim.
  - The field readers depend only on `em_base.h` types.
  - The whole `em_base.h` cannot be included: it pulls in OneWifi, cJSON, libuuid and OpenSSL
    headers, and cJSON is not installed on the build host. `tools/upstream-cmdu-vectors/vectors.cpp:7-10`
    records the same limit. Both existing generators use a shim.

Rules that still apply: 0004 (no `unsafe`, no clones, `Clone` only on `Copy`, no new
dependencies, MSRV 1.85); 0008 D1 (in-house `Reader`/`Writer`), D2 (shape of a body), D3 (tests),
D6 (host-order fields little-endian), D7/D8 (the spec wins wherever it is explicit; silent points
keep upstream's bytes); the audit's `port_rule` for each row.

## Decision

### D1. Scope (user, 2026-11-20)

`em-msg` gets:

1. A typed body for **each of the 124 spec TLVs** (D2). That covers:
   - the 27 TLVs with no upstream code, written from the spec tables alone and marked
     "spec-only, no upstream reference" (user decision);
   - 0x12 and 0x13, added to `em-types` as `spec` codes with their 1905.1 references (the
     `ADDITIONS` table in `codes.rs`; the generated upstream enum fixture is unchanged), recorded
     as an amendment to 0001.
   End of Message (0x00) has no body type: framing handles it (`MsgBuilder` writes it,
   `TlvIter` stops at it).
   Vendor 0xF2 (`VENDOR_OPERATIONAL_BSS`, upstream's own code) also gets a body, in B03:
   upstream sends it (`em_configuration.cpp:1546`) and reads it (`:2443`).
   Not ported: 0xDE (vBSS), which EasyMesh Table 22 lists only by reference to the informative
   Appendix B.5. Neither upstream nor `em-types` has it.
2. **WSC attributes** (D5): an attribute walker and writer and the plain attributes.
3. **Field readers** (D6): the upstream readers that have production callers.

Not in `em-msg`:
- policy on received values (Profile fallback, which band or role values are acted on, which
  BSSs a sender includes);
- Encrypted Settings and authenticator computation and verification;
- M1/M2 assembly;
- DPP, EAPOL and IEEE 802.11 frames carried inside TLVs;
- the three `em_unassoc_work_*` rows, which move to `em-agent` in `docs/em_base-coverage.md`;
  `em_ap_mld_ssids_t` is marked "not needed: unused at the pin" (the AP MLD bodies define their
  own SSID records).

**Amendments.** This replaces 0005 D1 decision 2 and 0008 D4 ("bodies come with their
consumer"); both notes get an amendment A1 that points here. The questions 0008 D4 left for the
`em-core` channel step are settled per body in batch B05/B06 (D8) instead. 0005 and 0008's
"216" becomes 219.

### D2. Shape of the body API

Each TLV gets one type in `crates/em-msg/src/bodies/<group>.rs`, named after the spec TLV
(`ApHeCapabilities<'a>`, `ChannelScanResult<'a>`). The doc comment cites the spec section and
table and the upstream struct, sender and receiver lines. Every type implements:

```rust
pub trait TlvBody<'a>: Sized {
    const TLV_TYPE: TlvType;
    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError>;   // strict, spec (D4)
    fn encode_value(&self) -> Result<Vec<u8>, EncodeError>;          // spec bytes
}
```

Bodies with a lenient form (D4) also implement:

```rust
pub trait TlvBodyLenient<'a>: TlvBody<'a> {
    /// `Ok(None)`: upstream's empty value for a TLV whose spec fields are mandatory.
    fn decode_value_lenient(value: &'a [u8]) -> Result<Option<Self>, DecodeError>;
}
```

`Ok(None)` is returned only for an empty value, and only by bodies whose D4 row lists the empty
form. The consumer treats it as "TLV absent". Every other lenient form decodes to
`Ok(Some(body))` (user, 2026-11-20).

Convenience functions, additive to the existing API:
- `Tlv::decode::<B>()` and `Tlv::decode_lenient::<B>()` check the type and call the decoder;
  the wrong type is `DecodeError::WrongType`.
- `MsgBuilder::body(&B)` encodes and appends, and keeps the builder's first-error rule. An
  encode failure is kept as the new `BuildError::Encode(EncodeError)`.

API decisions:
- **Error enums stay exhaustive.** B00 adds every new kind at once: `DecodeError::{WrongType,
  InvalidLength, ValueOutOfRange}` and `BuildError::Encode`. A later batch that needs another
  kind adds it and updates every match in the workspace; there are no outside users.
- **Codes.** Body fields reuse the `em-types` codes where they exist (`FreqBand`, `ProfileType`,
  `MediaType`, `ServiceType`, …). Codes that `em-types` lacks are open newtypes over the raw
  integer, made by a crate-private `open_code!` macro in `em-msg` with the same shape as
  `em-types`' `wire_code!`: `Copy`, `From<u8>` both ways, named constants, `name()`. `em-types`
  stays unchanged, apart from the two codes in D1.
- **Shared layouts.** TLVs with the same layout (Supported and Searched Service; Autoconfig and
  Supported Freq Band; the Error Code pair; …) share one inner struct. Each TLV is a distinct
  newtype around it, declared by a crate-private `tlv_newtype!` macro that implements
  `TlvBody` with that TLV's `TLV_TYPE`.
- **Naming.** Types use the spec TLV name in UpperCamelCase without the "TLV" suffix. Modules
  follow the batch groups (`bodies/ieee1905.rs`, `bodies/autoconfig.rs`, `bodies/capability.rs`,
  …). Bodies are re-exported from `em_msg::bodies`.

Representation rules:

- **The codec does not decide policy.** A field whose meaning is a policy question carries the
  raw wire value: a reserved enum value, a band code, a VLAN id, a status or reason code. The
  type uses an open newtype (as `em-types` codes do) or a raw integer, and decoding never rejects
  a value for being reserved. Exception: a value that changes the layout. For example, a length
  or set-count field whose value cannot describe the bytes is a `DecodeError`. Flags that only
  say whether optional content *should* be present, while the bytes are sized by their own
  length field, are carried as found; a mismatch between them is for the consumer (for example
  HE160/HE8080 against the MCS NSS length in 0xAA).
- **Zero counts.** An empty repeated group is accepted wherever the count field can be 0. That
  includes "repeated k-1 times" groups with k = 0 in 1905.1 §6.4.5, 6.4.6, 6.4.8 and 6.4.9,
  read as "no entries".
- Variable-length byte strings borrow from the frame (`&'a [u8]`). SSIDs are bytes, not `str`.
- Repeated records are `Vec<Record>` of `Copy` records, or of records that borrow `'a`. Counts
  that size a list are derived from the list on encode and checked on decode (0008 D2).
- Bit-fields become named `bool` or small-integer fields. Reserved bits are ignored on decode and
  written as zero (0008 D2).
- `DecodeError` and `EncodeError` gain kinds as needed (`WrongType`, `InvalidLength`,
  `ValueOutOfRange`); messages never replace kinds (0008 D1).
- `Clone` is derived only on `Copy` types (0004). Bodies derive `Debug` and `PartialEq` for the
  test kit.

### D3. Wire rules per body

- The audit row's `port_rule` decides each recorded difference: `SPEC` follows the spec,
  `UPSTREAM` keeps upstream's bytes (0008 D6, D8). Each body's doc comment lists its rows by
  audit line, and a test pins every `SPEC` row whose bytes differ from upstream's.
- Fields upstream writes in host order where the spec names no byte order are little-endian
  (0008 D6), marked in the doc comment and listed in `STATUS.md`.
- **UNSURE rows.** All 15 are answered below; the user accepted the answers on 2026-11-20. Lines 3 and 221 stay open until B04 (D10).

| Audit line | TLV | Question | Answer |
|---|---|---|---|
| 3 | 0x88 AP HE Capabilities | MCS map Tx/Rx order | **Not silent**: §17.2.10 Table 32 defines the field by reference to the IEEE 802.11 "Tx Rx HE MCS Support field", reordered into big-endian. D8 applies by reference. The 802.11 text is not available, so the B04 plan gathers secondary evidence (Linux `ieee80211.h`, hostapd, Wireshark dissector) and **the user** decides the order (D10). Maps are big-endian (SPEC row 3) |
| 221 | 0xAA AP Wi-Fi 6 Capabilities | HE-MCS set order | Same evidence step and user decision as line 3. The byte order is decided separately: §17.2.72 copies the 802.11 field (Figure 9-772d) and says nothing about reordering it, so the bytes are the 802.11 field as defined there, not line 3's big-endian rule |
| 222 | 0xAA | MCS NSS Length: set count or octet count | Octet count (4, 8 or 12), as upstream reads it. Any other value cannot describe the bytes: `DecodeError::InvalidLength`. HE160/HE8080 flags against the length: consumer (D2) |
| 23 | 0xE6 TID-to-Link Mapping | Direction values | Carry all four 2-bit values (codec does not decide policy) |
| 128 | 0xA4 Channel Scan Reporting Policy | one octet or extensible | Decode reads the policy octet and rejects trailing octets (`TrailingBytes`). Upstream's only sender writes 1 octet, or 0 octets when the data model has no channel-scan policy (`em_policy_cfg.cpp:273-302`, added at `:591-598`). No sender writes extra octets, so trailing octets get no lenient form. The empty value is a lenient form (D4) |
| 138 | 0xA6 Channel Scan Request | Fresh=1 with Num_OpClass 0 | The codec accepts it; meaning is the consumer's |
| 174 | 0xB5 Default 802.1Q | VLAN 0 as a placeholder | The codec carries any VLAN id; meaning is the consumer's |
| 180 | 0xB6 Traffic Separation | which SSIDs Num_SSID counts | The count is the list length; which SSIDs to include is the consumer's |
| 200 | 0xC7 AP Extended Metrics | BSSs outside the requested radios | Consumer (which BSSs are sent); the codec has no question here |
| 231 | 0xB7 BSS Configuration Report | which BSSs Num_BSS counts | Consumer, as line 180 |
| 315 | 1905.1 Device Bridging Capability | repeat count L | **Settled by the spec**: Table 6-11 says "m is the total number of bridging tuples", so L = m |
| 317 | 1905.1 Neighbor Device | what repeats | **Settled by the spec**: "The above three fields are repeated 0 or more times" covers the AL MAC, bit 7 and bits 6-0, so the local interface MAC appears once (audit row 318 is SPEC) |
| 321 | 1905.1 Link Metric Query | 8 octets always, or 2 for "all neighbors" | Table 6-16 says both "tlvLength 8" and "if the value is 0, the EUI-48 field is not present". The strict decoder accepts both readings: 2 octets with selector 0, and 8 octets with any selector (the MAC kept as found). **Encoder (user, 2026-11-20): 2 octets for "all neighbors"** (row 320's SPEC rule); 8 octets with the MAC for "specific neighbor". Audit row 321 is amended to record this. Consequence: validation's Link Metric Query row keeps upstream's minimum of 11 (0005 D3 left it unlowered, because the table contradicts itself), which would reject our own 2-octet query. B02 therefore adds a `spec_min.rs` override, 11 → 5 (3 + 2), with an "explicit" zero case ("the EUI-48 field is not present"). It raises the pinned override count from 25 to 26 (`spec_min.rs:34`, `:295-298`, `:304`; `validate.rs:14`). 0005 D3's "Not lowered" bullet drops Link Metric Query, recorded as an amendment to 0005. Interop: an unpatched upstream peer rejects a 2-octet query in `validate`; recorded in `STATUS.md` |
| 337 | 1905.1 SupportedFreqBand | 0x03 (6 GHz) | Raw band byte; the codec does not decide policy |
| 340 | 1905.1 WSC | minimum length | No minimum in the body (an attribute list, D5); validation keeps its 264 (0005 D3) |

### D4. Strict decoders and named lenient decoders (user, 2026-11-20)

- `decode_value` follows the spec exactly. Every spec form, including optional and conditional
  fields that are absent, is accepted by the strict decoder and has its own D3 item 6 fixture.
- A body gets `decode_value_lenient` only for a **non-spec** form that a **production upstream
  sender at the pin writes**. The doc comment cites that sender line. The lenient decoder
  normalises the form to the spec representation. Where that loses bytes, the doc comment says
  so.
- A sender form whose bytes also parse as spec bytes gets no lenient decoder. The strict decoder
  accepts it, and its meaning is for the consumer. Example: an SSID length that counts a NUL
  (`em_configuration.cpp:1100-1103`, `1149-1155`) decodes as an SSID whose last octet is 0x00.
  Stripping the NUL is a consumer decision, because a real SSID may end in 0x00.
- Forms that upstream only *accepts* on receive, which no upstream sender writes, are not
  tolerated.
- The encoder always writes spec bytes; there is no lenient encoder.

Lenient forms (each batch re-checks the sender line before implementing):

| TLV | Non-spec form | Upstream sender |
|---|---|---|
| 17.2.47 Profile | 4 octets (profile plus 3 more) | `em_configuration.cpp:3614` (`sizeof(em_profile_type_t)`); the other senders (`:984`, `:1786`, `:4445`, `:4581`) write 1 octet |
| 17.2.97 Backhaul STA MLD Configuration | empty value | `em_configuration.cpp:3771-3773` |
| 17.2.65 Backhaul STA Radio Capabilities | empty value when there is no backhaul STA | `em_capability.cpp:619-659`, added at `:861`; the second sender `em_configuration.cpp:3871-3911` is checked in B04 |
| 17.2.40 Channel Scan Result | full fields after a non-success Scan Status (spec: fields present only for status 0x00) | `em_channel.cpp:660-700` |
| 17.2.27 Beacon Metrics Query | `Num_Ids` omitted unless Reporting Detail is 1 with elements | `em_metrics.cpp:1885-1892` |
| 17.2.7 AP Radio Basic Capabilities | op-class groups with holes | `em.cpp:1105-1130`. B04 decides whether the bytes are unambiguous enough to accept; if not, no lenient form |
| 17.2.12 Metric Reporting Policy | empty value | `em_policy_cfg.cpp:44-145` (`return 0` at `:66-75`), added at `:545-555` |
| 17.2.37 Channel Scan Reporting Policy | empty value | `em_policy_cfg.cpp:273-302`, added at `:591-598` |
| 17.2.49 Default 802.1Q Settings | empty value | `em.cpp:2108-2136`, added at `em_policy_cfg.cpp:561-569` |
| 17.2.58 Unsuccessful Association Policy | empty value | `em_policy_cfg.cpp:304-331`, added at `:604-613` |

The four policy rows come from the Multi-AP Policy Config Request sender when the data model holds
no such policy. Upstream seeds default policies when it creates a data model
(`dm_easy_mesh_list.cpp:1666-1700`), so these forms are probably unreachable. They are kept
because the sender can write them (user, 2026-11-20). All empty forms decode to `Ok(None)` (D2).

Not lenient, because the form is spec-valid or no sender writes it: OUI-only Vendor Specific
(1905.1 Table 6-7, "3+m" with m = 0); a 13-octet BTM Report (§17.2.30, Target BSSID "0 or 6
octets"); a 1-octet Channel Scan Reporting Policy with extra octets (no sender); and the
2-octet Link Metric Query (spec form, line 321).

Tests:
- Each lenient form gets a D3 item 6 fixture.
- The strict decoder rejects the same bytes.
- Item 2's truncation test runs against the strict decoder.

### D5. WSC attributes and other inner content (user, 2026-11-20)

- **WSC** (`bodies/wsc.rs`, next to the existing selector):
  - an attribute walker (`WscAttrIter`, 2-byte id, 2-byte length, value, big-endian) and an
    attribute writer. These land in **B00**, because the WSC TLV body in B03 is an attribute
    list;
  - typed readers and writers for the plain attributes upstream's M1/M2 use (version, message
    type, UUID-E/R, MAC, nonces, public key, auth/encryption types and flags, methods, state,
    manufacturer, model, serial, device type, device name, RF bands, association state, password
    id, configuration error, OS version, vendor extension with the `vendor_ext_attr_id_t`
    sub-elements).
  - Encrypted Settings and the authenticators are carried as raw attribute values. Computing and
    checking them is `em-core` with `em-crypto` (0006). Once `em-core` has decrypted the
    Encrypted Settings, the plaintext is again an attribute list (SSID, Network Key,
    authentication and encryption type, MAC, Key Wrap Authenticator; built at
    `em_configuration.cpp:5842`). The same walker and the same typed attribute accessors in
    `em-msg` read it.
  - WSC v2.0.7 is not available, so a value's width and byte order cannot be checked against
    its spec. **B12 starts with a WSC attribute audit**: one row per attribute upstream builds
    or reads, giving id, width, byte order, the upstream line and a proposed decision. D6 is not
    inherited by default: upstream writes Authentication Type Flags with `htons`
    (`em_configuration.cpp:3409-3415`), but copies Encryption Type Flags (`:3425`) and
    Configuration Methods (`:3446`) from host-order `unsigned short` fields that nothing
    assigns. Those host-order cases go to the user before B12 is implemented.
- **DPP and EAPOL** (1905 Encap EAPOL, 1905 Encap DPP, DPP Message, DPP Chirp Value, DPP
  Bootstrapping URI Notification, DPP CCE Indication, BSS Configuration Request/Response):
  - `em-msg` decodes the fields the EasyMesh table defines (flags, optional MAC, frame type,
    lengths, hash length, CCE octet).
  - Carried frames, URIs and JSON objects are `&'a [u8]` for `em-easyconnect`.
  - The Chirp hash length is 1 octet (0008 D8; upstream uses 2).
- **IEEE 802.11 fields** inside the HT, VHT, HE, Wi-Fi 6, Wi-Fi 7, BTM and beacon TLVs: fields the
  EasyMesh table itemises are decoded. Element bodies the table passes through (beacon report
  elements, BTM frame bodies, the Tunneled frame) are `&'a [u8]`.
- **Vendor Specific**: OUI plus `&'a [u8]`. Sub-formats (Airties, Comcast) belong to their consumer.
- **MIC and Encrypted Payload**: the spec table fields, written from the spec alone (no upstream
  code).

### D6. Field readers

`crates/em-msg/src/readers.rs`, over the TLV area that `CmduHeader::parse` returns:

| Upstream | Rust | Notes |
|---|---|---|
| `get_tlv(type)` `:387`, static `:404` | `find_tlv(area, TlvType) -> Result<Option<&[u8]>, FramingError>` | first instance in frame order |
| `get_al_mac_address` `:71` | `al_mac_address` | value must be exactly 6 octets (spec, 0008 D8); upstream overflows on longer values (intentional difference) |
| `get_supported_service` `:89` | `supported_services` | upstream's checks: `len == 1 + num`, `num <= 8` (`EM_MAX_SERVICE`) |
| `parse_profile_tlv` `:131`, `get_profile` `:154`, `get_profile_type` `:365` | `profile` | 1 or 4 octets; raw values of 4 or more rejected; raw 0 accepted as upstream. Fallback policy stays in `em-core` (0005 D5.3) |
| `get_sta_mac` `:177` | `sta_mac` | upstream's per-type offsets and minimums |
| `get_bss_id` `:223` | `bss_id` | upstream's per-type offsets. A value too short for the read is an error, not an over-read (intentional difference) |
| `get_radio_id` `:251` | `radio_id` | upstream's type list and quirks are kept and tested: Channel Scan Request shorter than 9 fails the whole walk, checked before the End of Message test; Operational BSS with 0 radios continues the walk; Scan Request with 0 radios ends with "not found" |
| `get_freq_band` `:346` | `freq_band` | first Supported or Autoconfig Freq Band; an empty value is an error (upstream reads the next header) |

Rules:

- The first matching TLV in frame order wins, as upstream; there is no priority by type.
- Each reader returns `Result<Option<T>, ReaderError>`: `Ok(None)` is upstream's "not found".
  `ReaderError` is a framing error, a value too short for the read, or a value rejected by
  upstream's own check.
- The walk is `TlvIter` and stops at the first framing error (not `flatten`).
  - A frame without End of Message is a framing error where upstream returns "not found". The
    difference is recorded; such a frame is not spec-valid.
  - Ethernet padding after End of Message is ignored, as now.
- `get_tlv(em_tlv_t*)` and `get_client_mac_info` have no production caller and are not ported.
  `find_tlv` covers the first. The test map records both as "not ported: no production caller".
- Readers are message-level helpers. They do not use the typed bodies, so their offsets stay
  upstream's even where a body follows the spec. Each reader's doc comment says so.

### D7. Golden data (user, 2026-11-20)

1. **Spec fixtures, built twice.** For each body, one agent writes the fixture bytes and the
   expected field values from the spec table. A second agent derives the bytes from the same table
   without seeing the first one's bytes. A script compares the two, and the coordinator settles
   any mismatch against the spec. Fixtures live in `crates/em-msg/tests/data/bodies/<group>.tsv`
   (hex bytes plus the case name). Each batch's fixture TSVs record how they were made and are
   never hand-edited after the comparison.
   The builders' input for each TLV is the spec table (read through the spec server) **plus the
   TLV's audit rows with their port rules**, because `UPSTREAM` rows (D6 little-endian fields,
   silent points) cannot be derived from the table alone. Each build is a TSV
   (`case`, `hex`, `fields`). A committed tool, `tools/check-body-fixtures`, compares the two
   builds and writes the fixture. Its `--check` mode verifies that the committed fixture equals
   both builds, and it has Python tests like the other generators. The two builds are kept next
   to the fixture as its provenance.
2. **Upstream sender and reader vectors.** A new tool, `tools/upstream-tlv-vectors`, follows the
   `upstream-crypto-vectors` model:
   - it extracts the pinned definitions by line range with `git --no-replace-objects show`;
   - it compiles them with `c++` against a declaration shim. The shim holds the `em_base.h`
     definitions the code needs, extracted verbatim by line range, and their sizes are checked
     with a GCC `sizeof`/`offsetof` probe. The whole `em_base.h` is not included (see
     Context). Host prerequisites are `c++` and the C++ standard library only; these are host
     tools, not crate dependencies;
   - it writes `crates/em-msg/tests/data/upstream_tlv_vectors.tsv`;
   - it has `--check` and Python tests like the other generators.
   It covers:
   - the seven self-contained senders listed in Context (`create_btm_report_tlv` only if a small
     `em_cmd_t` shim is enough; otherwise it is left out, with the reason recorded);
   - every field reader, over a corpus of well-formed TLV areas, some built from
     `tests/test_l1_em_msg.cpp` cases.
   Malformed inputs, where upstream's behaviour is undefined, get Rust-only tests named after the
   upstream behaviour.
3. Where an upstream sender vector differs from the spec bytes, the test asserts the difference
   and names the audit row; the `SPEC` port rule makes the port write the spec bytes.

### D8. Batches

Batch B00 is split in two, so that a defect in the tooling does not hold up the API review, or
the other way round. B00a holds the Rust API and the parts it needs; B00b holds the two tools and
must be done before B01's fixtures are built. Together they add:
- B00a: the `TlvBody` and `TlvBodyLenient` traits;
- `Tlv::decode`, `Tlv::decode_lenient`, `MsgBuilder::body`, the new error kinds, and the
  `open_code!` and `tlv_newtype!` macros (D2);
- 0x12/0x13 in `em-types` (the `ADDITIONS` table and its tests; 0001 amendment);
- the WSC attribute walker and writer (D5);
- B00b: `tools/check-body-fixtures` (D7.1);
- B00b: `tools/upstream-tlv-vectors` with its first vectors (`create_prof_2_tlv`, `create_error_code_tlv`);
- B00a: a per-body test macro over `body_testkit`, so each body states its fixtures and
  expectations and the D3 checks run uniformly.

The macros, the `em-types` codes and the WSC walker and writer are also in B00a.

| Batch | TLVs (spec section) | Weight |
|---|---|---|
| B01 | 1905.1 identity and framing: 6.4.2, 3, 4, 5, 6, 8, 9 (6.4.1 End of Message has no body, D1) | 12 |
| B02 | 1905.1 link metrics and push buttons: 6.4.10, 11, 12, 13, 19, 20 | 16 |
| B03 | Autoconfiguration, search, topology, profile: 17.2.1, 2, 3, 4, 5, 47, 51, 74, 77; 6.4.14 to 6.4.18 (WSC body = attribute list, D5); vendor 0xF2 | 26 |
| B04 | Capability: 17.2.6, 7, 8, 9, 10, 18, 19, 46, 48, 52, 65, 72, 76, 94, 95 | 35 |
| B05 | Channel selection and scan: 17.2.13 to 17, 37 to 41 | 27 |
| B06 | CAC, channel usage, spatial reuse: 17.2.42 to 45, 87 to 91, 104, 105 | 25 |
| B07 | Metrics: 17.2.21 to 28, 35, 59 to 62, 73 | 24 |
| B08 | Steering, association control, codes: 17.2.20, 29 to 34, 36, 53, 57, 63, 64 | 20 |
| B09 | Policy: 17.2.11, 12, 49, 50, 58, 66, 70, 71, 92, 93 | 22 |
| B10 | BSS configuration and Wi-Fi 7 MLD: 17.2.75, 84, 85, 96 to 103 | 30 |
| B11 | DPP, security, encapsulation: 17.2.54 to 56, 67 to 69, 78 to 83, 86 | 24 |
| B12 | WSC attributes (D5) | — |
| B13 | Field readers (D6) and their upstream vectors | — |

Weights: small 1, medium 2, large 4. The shapes come from a mechanical first pass, and each batch
plan re-checks them. B04 may be split in two. B13 does not depend on the bodies and may run
earlier.

**Per batch:**
1. A batch plan in the tracking folder lists each TLV, its audit rows and `port_rule`s, the
   lenient-form candidates confirmed or dropped, and its fixtures. A batch plan may find new
   lenient candidates (non-spec forms written by an upstream sender); the user decides on each
   before implementation.
2. Spec fixtures are built twice (D7.1).
3. An implementer writes the bodies.
4. The coordinator runs the checks.
5. A verification engineer runs mutation checks: one mutation per body, in a disposable copy.
6. Documentation pass.
7. Review: Codex, correctness and adversarial reviewers, with at most two rounds. The same rules
   apply as in earlier steps.
8. `STATUS.md` gets a log entry per batch.

Each batch is committed only when the user asks.

### D9. Size and allocation

- Bodies borrow from the frame; decoding allocates only `Vec`s of records.
- `em-msg` gets em-dm's `large_*` lints (`large_types_passed_by_value`, `large_stack_arrays`,
  `large_enum_variant`) as `deny`. They do not catch everything: `large_types_passed_by_value`
  skips public functions (`avoid-breaking-exported-api`), and the body API is public. So each
  batch also has a size test that asserts `size_of` for every body and record type against a
  stated bound.
- The 0007 D3 size check applies when a consumer moves a body into owned storage. `em-msg` does
  not store bodies.

### D10. Open points for the user

Decided by the user on 2026-11-20:
1. The UNSURE answers in D3 are accepted, apart from lines 3 and 221.
2. Line 321: the encoder writes 2 octets for "all neighbors".
3. 0xDE (vBSS) and the two readers with no production caller are left out.

Open, decided when the batch's evidence is ready:
4. B04: the HE-MCS Tx/Rx order for 0x88 and 0xAA (lines 3 and 221).
5. B12: the WSC attributes with host-order values (D5).

Follow-up recorded, not in scope: the 1905.1 §6.4 text reserves the two most significant bits of
tlvLength. The existing framing reads all 16 bits, as upstream does.

### Amendments

- **A1 (2026-11-20, B01; user decision):** D4 gains a lenient row. Vendor Specific (IEEE 1905.1
  §6.4.2) may have an empty value: upstream's AP Capability Report sender writes one when the
  Airties capability is missing (`em_capability.cpp:184-190`, `em.cpp:1330-1352`). The lenient
  decoder returns `Ok(None)` for it; the strict decoder rejects it as `Truncated`. An OUI-only
  value stays a spec form.
- **A2 (2026-11-20, B01; coordinator):** no typed IEEE 802.11 media-specific-info helper is
  added. Media-specific information stays raw bytes (D2), and no consumer needs the typed form
  yet.
- **A3 (2026-11-20, before B04; user decision, D3 lines 3 and 221):** HE-MCS map order. Evidence
  (the IEEE 802.11 text is not available): Linux `include/linux/ieee80211-he.h`
  `struct ieee80211_he_mcs_nss_supp` and Wireshark's `dissect_he_capabilities` both lay out each
  bandwidth set as the Rx map, then the Tx map, each a little-endian 16-bit value.
  - 0x88 AP HE Capabilities (§17.2.10, "reordered into big-endian"): Rx map then Tx map per set,
    each **big-endian**. This differs from upstream (Tx then Rx, little-endian).
  - 0xAA AP Wi-Fi 6 Capabilities (§17.2.72, the 802.11 field as defined): Rx map then Tx map per
    set, each **little-endian**. This differs from upstream's order (Tx then Rx).
- **A4 (2026-11-20, B02 review; user decision, D3 line 321 clarified):** a Link Metric Query with
  selector 0 never carries a MAC. An 8-octet value with selector 0 is still accepted on decode, but
  its MAC octets are ignored like reserved octets, and the body always encodes as 2 octets. (B02 had
  kept the MAC; the reviewers found that this did not match the decision.)
- **A5 (2026-11-20, B04 plan; user decision):** more D4 lenient rows. AP HT, VHT, HE and Wi-Fi 6
  Capabilities (0x86, 0x87, 0x88, 0xAA) may have an empty value: the senders return 0 when the
  radio capability data is missing (`em.cpp:1283-1293`, `:1376-1386`, `:1403-1413`, `:1466-1470`),
  and the AP Capability Report and Early AP Capability Report callers still add the TLV
  (`em_capability.cpp:144-169`, `:359-383`). The lenient decoder returns `Ok(None)`. Device
  Inventory (0xD4) with `Num_Radio` = 0 (spec: at least 1; sender `em.cpp:1851-1854`) decodes
  leniently to `Ok(Some)` with an empty radio list. The strict decoder rejects all of these.
- **A6 (2026-11-20, B05 plan; user decision):** Channel Scan Capabilities (0xA5) may have an
  empty value (sender `em.cpp:1703-1768`, returning 0 at `:1721-1728`; caller
  `em_capability.cpp:196-202`). The lenient decoder returns `Ok(None)`. Coordinator rule for the
  D4 Channel Scan Result lenient form: it is accepted only when the octets after a non-success
  status parse as the full field set upstream writes (`em_channel.cpp:645-809`). The lenient
  decoder then returns the status-only body; any other extra octets are an error.
- **A7 (2026-11-20, B07 plan; user decisions):** more D4 lenient rows. The empty values of AP Metrics
  (0x94; `em_metrics.cpp:1933-1990`, committed at `:1521`) and of 0xC5 (`em.cpp:1879-1902`, callers
  `em_capability.cpp:231`, `:427`) decode leniently to `Ok(None)`. Associated Wi-Fi 6 STA Status
  Report (0xB0) is an exception to "`Ok(None)` only for an empty value": upstream writes 9 zero
  octets in every Profile-3 AP Metrics Response (`em_metrics.cpp:2299-2306`). They carry no
  information and are not spec-valid (2 stray octets), so the lenient decoder returns `Ok(None)` for
  exactly that 9-zero-octet value. The strict decoder rejects all three forms.
- **A8 (2026-11-20, B08 plan; user decision):** Steering Request (0x9B) and Profile-2 Steering
  Request (0xC3) with Request Mode 0 (steering opportunity) and a non-zero target count are invalid
  for the strict decoder (`ValueOutOfRange`): the spec says the count shall be 0, as for the 0xA6
  Fresh rule. Upstream's only 0x9B sender always writes one target (`em_steering.cpp:488-493`). A
  lenient 0x9B decoder accepts that form and drops the target list, which the spec leaves unused
  in opportunity mode. 0xC3 has no production sender, so it has no lenient form.
- **A9 (2026-11-20, B10-B12 plans; user decisions):**
  - 0xE6 TID-to-Link Mapping Policy: no lenient form for upstream's fixed 19-octet mapping records
    without TID-to-Link entries (`em_configuration.cpp:1687-1690`, audit row 18).
  - 0xD3 DPP Chirp Value: a lenient decoder accepts upstream's 2-octet big-endian Hash Length
    (`ec_util.cpp:337`, `:356-358`; upstream's reader also expects 2 octets). The strict decoder and the
    encoder keep the spec's 1 octet (0008 D8).
  - WSC attributes (D5 audit, `b12/wsc-audit.md` in the tracking folder): Encryption Type Flags and
    Configuration Methods are big-endian `u16`. Upstream copies them from host-order fields that nothing
    sets (`em_configuration.cpp:3425`, `:3446`), so it always sends `00 00`. Association State, Device
    Password ID and Configuration Error are big-endian `u16`. Upstream sets length 2 but copies no value
    octets (`sizeof(val[0])`, `:3539-3542` and siblings), and the port names that as an intentional
    difference. The Comcast haul-type Vendor Extension in the Encrypted Settings is written with
    upstream's 11-octet length (5 meaningful octets, then 6 zeros, `:5917-5924`) and read when it has 5
    or more octets.
- **A10 (2026-11-21, B13 plan review; user decisions):**
  - **TLV walker (added to D6).** `get_first_tlv` and `get_next_tlv` (`em_msg.cpp:424-480`) were missing
    from D6, but four production loops use them, on five call paths (`em_capability.cpp:1214-1230`, called
    twice by `handle_bsta_cap_report`; `em_configuration.cpp:5141-5242`, `:5570-5612`, `:5748-5776`).
    `readers.rs` adds an offset-addressed first/next pair that keeps "first TLV failed" separate from
    "walk ended". Each caller's pre-checks and return codes stay in `em-core`.
  - **Zero-length stop kept (user decision).** As upstream, the walker ends at any zero-length TLV,
    including a normal End of Message. Known upstream consequences, named in the docs and tests: an empty
    Backhaul STA MLD Configuration TLV hides the EHT Operations TLV after it
    (`em_configuration.cpp:3771-3777`), and an empty first TLV in a Backhaul STA Capability Report makes
    both single-TLV lookups fail, so Client Info is not processed (`em_capability.cpp:1242-1255`).
    `TlvIter` remains the non-stopping walk if `em-core` later decides to fix this.
  - **Intentional difference.** `get_next_tlv` holds the TLV size in a `uint16_t` (`em_msg.cpp:466`), which
    wraps for lengths 0xFFFD to 0xFFFF. The port computes it without wrapping, so a walk always advances
    or ends.
  - **Channel Scan Request with 0 radios (user decision; clarifies D6).** In `radio_id`, the exact
    2-octet value with `Num_Radio` = 0 (spec-valid, §17.2.39) returns `Ok(None)`. Any other value shorter
    than 9 octets returns `ReaderError`. Both match upstream's `false`.
  - **Vector mapping.** An upstream `false` is `Ok(None)`, except for a pinned list of cases that are
    `Err`: the four malformed Profile values or lengths and `supported-service-9`. Outputs are compared
    only on `true`. `find_tlv` is compared by the position of its value slice.
    This mapping covers the well-formed vector corpus only. Outside it, D6's rule applies: a value too short
    for the read, or rejected by upstream's own check, is a `ReaderError` (for example a short `sta_mac`
    value, or a Supported Service value whose length is not `1 + num`).
  - **Walker and End of Message.** Like `get_first_tlv`, the walker does not look at the type, so an End of
    Message with a non-zero length is returned as a TLV. It stops only at a zero-length TLV.

- **A11 (2026-11-21, B13 coverage review):** The 35 remaining `em-msg` rows of
  `em_base-coverage.md` (33 `planned`, 2 `partly ported`) are resolved against the pinned sources and
  the crates. `em-msg` now owns 194 of the 365 rows and none is `planned`; 25 rows moved: 9 to
  `em-dm`, 2 to `em-cmd`, 6 to `em-agent`, 7 to `em-core` and 1 to `do not port`.
  - **Ported, status only (6).** `em_tlv_t` (`Tlv`), `em_cmdu_t` (`CmduHeader`),
    `em_tlv_requirement_t` (`tables::Requirement`), `em_op_class_t` (`BasicOperatingClass`),
    `em_assoc_sta_traffic_stats_t` (`AssociatedStaTrafficStats`), `em_1905_mac_addr_t` (`MacAddress`).
  - **Not needed (3, reason in the row).** `em_attrib_t` (defined but unused at the pin; `WscAttrIter`
    reads the attribute header) and `em_cac_method_type_t` (no upstream code names an enumerator;
    `CacMethod::method` is a raw `u8`) stay `em-msg` rows. `em_attrib_id_t` is only the parameter type
    of the stub `commit_config` overload (`dm_easy_mesh.cpp:346-349`). That overload is `em-dm`'s and
    is recorded there as not ported, but the row cannot be `em-dm`: `tools/gen-dm-inventory` requires
    every `em-dm` row to have a classification row, so it is `do not port`. This corrects the
    Context sentence that the pair is "used only by `commit_config`": `em_attrib_t` is not used at all.
  - **Moved to `em-dm`, already there (9).** `em_mlo_policy_t` (`MloPolicy`), `em_scanner_type_t`
    (`ScannerType`), `em_scan_result_id_t` (`ScanResultId`), `em_neighbor_t` (`Neighbor`),
    `em_scan_result_t` (`ScanResult`), `em_alarm_samples_t` (`AlarmSamples`), `em_link_report_t`
    (`LinkReport`), `em_link_stats_alarm_cfg_t` (`LinkStatsAlarmCfg`), `em_client_filters_cfg_t`
    (`ClientFiltersCfg`). All have rows in `dm_type_classification.tsv` with owner `em-dm` and the same line.
  - **Moved to `em-cmd` (2).** `em_scan_params_t` (aliased by `em_cmd_scan_params_t`,
    `em_base.h:3510`) and `em_vendor_policy_t` (member of `em_policy_cfg_params_t`, `em_base.h:2196`).
  - **Moved to `em-agent` (6).** The three `em_unassoc_work_*` types (work list for the OneWifi subdoc,
    `em_agent.cpp:893-944`) and the three `em_80211_btm_*`/neighbor report types (BTM Request frame
    built for OneWifi, `dm_easy_mesh_agent.cpp:820-860`).
  - **Moved to `em-core` (7), tracked, not "not needed".** Vendor formats and flags with their
    consumers (D5): `em_vendor_airties_tlv_type_t`, `em_assoc_sta_vendor_link_metrics_t`,
    `em_assoc_vendor_link_metrics_t` (unused at the pin; named only in a comment at
    `em_base.h:1301`), `em_radio_capability_vendor_t`, `em_radio_metrics_t`,
    `em_radio_device_metrics_vendor_t` and `em_mlo_mode_bitmap_t` (`em_configuration.cpp:224-326`).
  - **D5 narrowed.** D5 lists the vendor extension with the `vendor_ext_attr_id_t` sub-elements as
    typed attributes of `em-msg`. Only sub-id 0x01 (`HaulTypeExtension`, B12) is. Sub-ids 0x02, 0x04
    and 0x05 are used only in `em_policy_cfg.cpp` (the Vendor Specific policy TLV) and 0x03 and 0x06
    only in `em_metrics.cpp`, so they are `em-core`'s. The `vendor_ext_attr_id_t` row stays an `em-msg` row, marked ported for 0x01 and
    pointing to `em-core` for the rest, because `dm_type_classification.tsv` records `em-msg` as its
    owner and `tools/gen-dm-inventory` checks that against the coverage table.
  - **`em_wsc_msg_type_t`.** Ported: `WSC_M1` (4) and `WSC_M2` (5) and `wsc_msg_type`, which returns
    the raw octet. Upstream compares only with `em_wsc_msg_type_m1` and `_m2` (`em_msg.cpp:1198`,
    `em_configuration.cpp:2956`, `:3330`, `:4613`, `:4675`, `:6549`, `:6553`) and returns `_none`
    when no Message Type is found. The other 13 values are not named anywhere, so they are not needed.
  - **Closing criterion corrected.** The final sentence of Acceptance criteria says the 219 rows are
    "ported, moved (the 4 non-wire rows) or marked not needed". The count was wrong: 25 rows moved, as
    listed above. The criterion reads: of the 219 `em-msg` rows, 194 stay `em-msg` (ported or not
    needed with a reason) and 25 moved, with each destination named in its row.
- **A12 (2026-11-21, B10-B13 review round 1):** two A9 points are stated exactly.
  - The Comcast haul-type Vendor Extension needs its fifth octet only for sub-id 0x01. A 4-octet value with
    another sub-id is ignored (`Ok(None)`), as upstream reads `val[4]` only for 0x01
    (`em_configuration.cpp:4914-4919`). A 3-octet value, and sub-id 0x01 with no haul octet, stay errors.
  - The lenient 0xD3 decoder reads upstream's Hash Length as a full big-endian `u16`, because the Proxy Agent
    forwards the length of a received attribute (`ec_pa_configurator.cpp:54`). A hash above 255 octets
    decodes leniently and cannot be re-encoded (`CountTooLarge`); the strict form keeps its 1-octet length.
  - **SSID lengths stay raw (user decision).** The 0xE0 body keeps SSID lengths above the spec's 32 octets, like
    the other SSID bodies. Enforcing the limit is the consumer's.

### Errata

- **E1 (2026-11-20, B00b review):** D7.1 gives a build's columns as (`case`, `hex`,
  `fields`). The tool built in B00b, `tools/check-body-fixtures`, uses
  `tlv case fields hex`: the TLV code comes first, because one group file holds several TLVs.
  The tool is right and D7.1 is corrected by this erratum. A row's `fields` is the case
  specification (`name=value;…`). Builder A writes it, and builder B copies it verbatim and
  derives only `hex`.

## Alternatives considered

- **Bodies with their consumer** (0005 D1, 0008 D4): replaced by the user's decision.
- **Spec-only fixtures, no upstream generator**: cheaper. But it would leave no executable check
  of upstream's bytes where the spec is silent, and the readers would be checked against no
  upstream code at all. Not chosen (user).
- **Lenient by default, or strict only**: the first hides spec violations from consumers; the
  second leaves every consumer to reimplement the same upstream tolerances. Not chosen (user).
- **Opaque WSC, DPP and 802.11 content in `em-msg`**: smaller, but every consumer would then
  parse the EasyMesh-defined parts again. Not chosen (user).
- **binrw fallback (0008 D5)**: kept as the fallback if B01 to B03 show that hand-written cost is
  too high.

## Acceptance criteria

Per batch:

1. Every TLV in the batch has a body type implementing `TlvBody`. Its doc comment cites the spec
   section and table, the upstream struct, sender and receiver lines (or "spec-only, no upstream
   reference"), and every audit row with its port rule.
2. The 0008 D3 tests pass for every body, through the shared macro: golden decode and encode;
   strict prefixes; the trailing byte; `CountTooLarge`; reserved bits; lenient forms.
3. A round trip through `MsgBuilder::body` and `Tlv::decode` passes for each body, plus one body
   per batch at the 65 535-byte limit where its shape allows it.
4. The spec fixtures are double-built and match (D7.1). Upstream vectors, where they exist, are
   asserted equal, or asserted different with the audit row named.
5. Every UNSURE and lenient decision for the batch is implemented as recorded here, and tested.
6. On stable and 1.85.0: `cargo test -p em-msg`, workspace tests, clippy pedantic `-D warnings`,
   fmt, and rustdoc `-D warnings` pass. No `unsafe`, no clones, no new dependencies.
7. Mutation check: for each body, one mutation of its decode or encode fails a test.
8. `STATUS.md` (log and follow-ups), the `em-msg` README and `docs/em_base-coverage.md` (status
   of the ported rows) are updated.

Criteria 1–5 and 7 apply to the body batches (B01–B11). Criteria 6 and 8 apply to every batch.
The other batches have their own criteria:

- **B00a**:
  - The traits, `Tlv::decode`/`decode_lenient`, `MsgBuilder::body` and the new error kinds have
    unit tests, including `WrongType`, `BuildError::Encode` as the kept first error, and `Ok(None)`
    from a test-only lenient body.
  - The 0x12/0x13 `ADDITIONS` tests pass, and the generated upstream enum fixture is unchanged
    (`--check`).
  - The WSC walker rejects a truncated attribute header and an attribute length past the end, and
    the writer round-trips.
  - The per-body test macro runs the D3 checks on the existing test-only sample bodies.
- **B00b**: both tools pass `--check` on the committed fixtures and have Python tests (stale
  fixture, a mismatch between the two builds, validate before write); the upstream vectors
  compile from the pin with the shim, and the `sizeof`/`offsetof` probe matches.
- **B12**:
  - The WSC attribute audit is complete, and the user's host-order decisions are recorded before
    implementation.
  - Each typed attribute has decode, encode and truncation tests.
  - The attributes inside the decrypted Encrypted Settings parse through the same accessors (a
    test with plaintext bytes).
- **B13**:
  - Every reader equals the upstream vectors on the well-formed corpus.
  - Each D6 quirk and each intentional difference has a test named after the upstream behaviour.
  - The test map records the two readers that are not ported.

At the end: all 124 TLVs, the WSC attributes and the readers are done; `em-msg` is marked ported
in `STATUS.md`, the root README and the tracking page; and the 219 `em-msg` coverage rows are
ported, moved (the 4 non-wire rows) or marked not needed with a reason.

## Revision history

- rev 1 (2026-11-20): first draft after the inventory. User decisions: complete `em-msg` now;
  port spec-only TLVs; strict plus named lenient decoders; split inner content by owner; spec
  fixtures plus upstream sender and reader vectors.
- rev 2 (2026-11-20, plan review: 15 findings, all confirmed against the code and the spec):
  - D4 rewritten: lenient decoders only for non-spec forms that an upstream sender writes, with
    sender lines. Spec forms (OUI-only Vendor Specific, the 13-octet BTM Report, the 2-octet
    Link Metric Query) move to the strict decoder. The Scan Result and Backhaul STA Radio
    Capabilities entries are corrected. SSID with NUL is left to the consumer.
  - UNSURE lines 3, 221, 315, 317 and 321 reworded: 315 and 317 are settled by the spec; 3 and
    221 are spec-by-reference and go to the user with evidence in B04; 321 accepts both forms,
    and the user picks the encoder form.
  - WSC: corrected cites (`data_elem_attr_t`); a per-attribute byte-order audit before B12; the
    walker moved to B00; decrypted inner attributes use the same accessors.
  - Golden data: shim by default, `create_btm_report_tlv` dropped from the self-contained list,
    fixture builders take the audit rows, and a committed `check-body-fixtures` with
    `--check`.
  - API decisions added to D2: `TlvBodyLenient`, `BuildError::Encode`, exhaustive errors, the
    newtype and shared-layout macros, naming. End of Message has no body. 0xF2 is ported in
    B03; 0xDE is excluded. A zero-count rule and a per-batch size test are added. 0001 is
    listed as amended. D10 lists the open points.
- rev 3 (2026-11-20, plan review round 2: 5 findings, all confirmed):
  - Empty-value lenient forms return `Ok(None)` (user decision; trait signature changed).
  - Four Policy Config empty forms added to D4 (user decision). The line 128 rationale is
    corrected.
  - Batch plans may add lenient candidates, which the user decides.
  - Criteria added for B00a, B00b, B12 and B13.
  - B00 split into B00a (API) and B00b (tools).
  - The Link Metric Query override's touch list is completed (25 → 26, the 0005 "Not lowered"
    bullet, an explicit zero case).
  - Not reviewed after these fixes: two-round limit.
- Accepted by the user, 2026-11-20 (rev 3). Amendments recorded: 0001 A4, 0005 A1, 0008 A1.
