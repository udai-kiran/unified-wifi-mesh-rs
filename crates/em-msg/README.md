# em-msg

IEEE 1905 CMDU header, TLV framing, message builder, message validation (spec §17) and the TLV body reader and writer.
Synchronous, depends only on `em-types`, no `unsafe`, no clones.

## What is here

| Module | Holds | Upstream |
|---|---|---|
| `header` | 8-byte CMDU header, `default_relay` | `em_cmdu_t` (`em_base.h`), `add_1905_header` (`em_msg.cpp`) |
| `tlv` | `TlvIter`, a borrowing walk over the TLV area | `em_tlv_t`, `get_tlv`, `get_first_tlv`, `get_next_tlv`, `validate` walk (`em_msg.cpp`) |
| `builder` | `MsgBuilder` with a size budget, End of Message added by `finish` | `em_msg_builder_t` (`em_msg.h`, `em_msg.cpp`), `add_tlv` |
| `wsc` | WSC Message Type selector (M1 = 4, M2 = 5); WSC attribute walker `WscAttrIter` and writer `write_wsc_attr` | `em_configuration_t::get_wsc_msg_type` (`em_configuration.cpp`); the walker and writer are new (note 0009, D5) |
| `bodies` | `TlvBody`, `TlvBodyLenient`: the traits every typed TLV body implements; the seven IEEE 1905.1 identity and neighbor bodies in `bodies::ieee1905`; the six link metric and push button bodies in `bodies::linkmetric`; the fifteen autoconfiguration, search, topology, profile, WSC and operational BSS bodies in `bodies::autoconfig`; the fifteen capability bodies in `bodies::capability`; the ten channel selection and scan bodies in `bodies::channel` | the typed TLV bodies, added in batches (note 0009) |
| `tables` | The 70 validation tables as static data | the table functions `autoconfig_search` .. `i1905_ack` (`em_msg.cpp`) |
| `validate` | `validate(cmdu, profile)` | `em_msg_t::validate` (`em_msg.cpp`) |
| `spec_min` | Spec minimum lengths that replace upstream's stricter ones (26 rows) | - |
| `body` | `Reader`, `Writer` for TLV bodies | hand-written readers and writers replace upstream's packed C structs copied into the frame (note 0008) |
| `error` | `FramingError`, `BuildError`, `DecodeError`, `EncodeError` | - |

Design: [note 0005](../../docs/0005-em-msg-scope.md); typed TLV bodies and the WSC attribute
walker: [note 0009](../../docs/0009-tlv-bodies.md). The tables equal the golden fixture
`tests/data/upstream_msg_tables.tsv`, which is generated from upstream by `tools/gen-upstream-msg-tables`.

Upstream source: `src/em/em_msg.cpp`, `inc/em_msg.h`, and `get_wsc_msg_type` in
`src/em/config/em_configuration.cpp`. Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.

## What is not here

- The Ethernet header: `em-transport` (it keeps the MAC addresses next to the CMDU buffer).
  `CmduHeader::parse` and `validate` take the whole CMDU (from the 8-byte header); `TlvIter::new`
  and `wsc_msg_type` take the TLV area after the header; `MsgBuilder` produces the whole CMDU.
- The TLV bodies other than the IEEE 1905.1 identity, neighbor, link metric, push button,
  autoconfiguration, capability and channel sets (see below) and the field readers (`get_radio_id`, `get_profile`, ...): ported in batches per
  note 0009. Here are the `Reader` and `Writer` the bodies are built on, the body traits, and the
  WSC attribute walker and writer.
- The policy for a received Profile TLV: `em-core`.
- SDU fragmentation and CMDU reassembly.

## Reader and Writer (TLV bodies)

Design: [note 0008](../../docs/0008-tlv-body-codec.md), D1 and D3.

- `Reader<'a>` borrows one TLV value (`Tlv::value`). Reads: `u8`, `be_u16`, `be_u24` (returns `u32`),
  `be_u32`, `le_u16`, `le_u32` (only for fields upstream writes in host order), `bytes(n)`,
  `array::<N>()`, `mac()`, `skip(n)`, `rest()` (a tail bounded by the TLV length), `remaining()`.
  A read of `n > 0` bytes past the end is `DecodeError::Truncated { needed, remaining }`; a zero-byte
  read at the end succeeds; a failed read leaves the reader where it was. `finish()` fails with
  `DecodeError::TrailingBytes` if input remains.
- `Writer` owns the bytes: the same scalars, `bytes`, `mac`, `zeros(n)`, and `count_u8(len)` /
  `count_u16(len)` (the latter writes 2 bytes, big-endian), which return `EncodeError::CountTooLarge`
  instead of truncating.
  `be_u24` returns `EncodeError::ValueTooLarge` above `0xFF_FFFF`. A call that fails appends nothing.
- Each body implements `TlvBody` (below) with two entry points: `decode_value(&'a [u8]) ->
  Result<Body<'a>, DecodeError>` (reader over `Tlv::value`, decode, `finish`) and
  `encode_value(&self) -> Result<Vec<u8>, EncodeError>` (bytes only on success; the caller, or
  `MsgBuilder::body`, passes them to `MsgBuilder::tlv`). The pattern is in the `body` module docs.
- The D3 test helpers (golden round trip, strict prefixes, trailing byte, count too large, reserved
  bits, alternate and empty values, lenient forms) live in the test-only module `body_testkit`,
  with three sample bodies (see Test support below). Each helper is shown to fail on a wrong body
  or fixture.

## Body traits and entry points

Design: [note 0009](../../docs/0009-tlv-bodies.md), D2 and D4.

- `TlvBody<'a>` (module `bodies`): `const TLV_TYPE`, a strict `decode_value(&'a [u8])` that follows
  the spec, and `encode_value(&self)`.
- `TlvBodyLenient<'a>`: `decode_value_lenient` for a non-spec form that an upstream sender writes.
  It returns `Ok(None)` only for an empty value, which the consumer treats as "TLV absent". Every
  other lenient form is `Ok(Some(body))`.
- `Tlv::decode::<B>()` and `Tlv::decode_lenient::<B>()` check the TLV type first. A different type
  is `DecodeError::WrongType { expected, found }`; otherwise the decoder runs on `Tlv::value`.
- `MsgBuilder::body(&B)` encodes the body and appends it with `B::TLV_TYPE`. An encode failure is
  kept as the builder's first error, `BuildError::Encode(EncodeError)`. A builder that already
  holds an error does not encode.
- New error kinds: `DecodeError::InvalidLength { len }` (a length or set count, or the length of
  the whole value, that cannot describe the bytes) and `DecodeError::ValueOutOfRange { value }` (a
  field value that changes the layout).

Crate-private macros in `macros.rs`:

- `open_code!`: an open newtype over an integer, in the shape of `em-types`' `wire_code!` (`Copy`,
  named constants, `name()`, `ALL`, a `Debug` that prints the name). For field codes that
  `em-types` does not have.
- `tlv_newtype!`: a distinct TLV body around an inner layout that several TLVs share, with its own
  `TLV_TYPE`.

## IEEE 1905.1 identity and neighbor bodies (B01)

Design: [note 0009](../../docs/0009-tlv-bodies.md), D2, D3 and D8. The bodies are in
`bodies::ieee1905`. Each doc comment cites the spec section and table and the upstream struct,
sender and receiver lines.

| Body | TLV (type) | Spec |
|---|---|---|
| `VendorSpecific<'a>` | Vendor Specific (0x0B) | §6.4.2, Table 6-7 |
| `AlMacAddress` | AL MAC Address (0x01) | §6.4.3, Table 6-8 |
| `MacAddress` | MAC Address (0x02) | §6.4.4, Table 6-9 |
| `DeviceInformation<'a>`, with `LocalInterface<'a>` | Device Information (0x03) | §6.4.5, Table 6-10 |
| `DeviceBridgingCapability`, with `BridgingTuple` | Device Bridging Capability (0x04) | §6.4.6, Table 6-11 |
| `Non1905NeighborDeviceList` | Non-1905 Neighbor Device List (0x06) | §6.4.8, Table 6-14 |
| `Ieee1905NeighborDevice`, with `Ieee1905Neighbor` | 1905.1 Neighbor Device (0x07) | §6.4.9, Table 6-15 |

- Counts are derived from the lists on encode (`EncodeError::CountTooLarge` above 255) and checked
  on decode. A zero count is accepted and decodes to an empty list.
- A list that runs to the end of the value (Non-1905 neighbors, 1905.1 neighbors) takes all the
  bytes that remain. A partial entry is `DecodeError::Truncated`.
- A counted list reserves no more memory than the value's remaining bytes allow, whatever the count
  says: `capacity × size_of ≤ remaining` (see Test support). The list can still grow past that
  reservation when its records are smaller on the wire than in memory.
- Media-specific information of a Device Information interface is kept as the length-prefixed raw
  bytes, for any media type. The codec does not interpret it (D2).
- The 1905.1 neighbor flags: bit 7 is the bridge flag, and the reserved bits 6-0 are dropped on
  decode and written as zero.
- Vendor Specific is a 3-octet OUI and the data after it. An OUI-only value is spec-valid and
  decodes strictly. The empty value is a lenient form: `decode_value_lenient` gives `Ok(None)`,
  which upstream's capability report writes when the Airties capability is missing
  (`em_capability.cpp:184-190`). The strict decoder rejects the empty value.

## Link metric and push button bodies (B02)

Design: [note 0009](../../docs/0009-tlv-bodies.md), D2, D3 and D8. The bodies are in
`bodies::linkmetric`. Each doc comment cites the spec table and, where one exists, the upstream
struct. Upstream has no sender or receiver for these six TLVs at the pin, so the structs are not
used on the wire.

| Body | TLV (type) | Spec |
|---|---|---|
| `LinkMetricQuery` | Link Metric Query (0x08) | §6.4.10, Table 6-16 |
| `TransmitterLinkMetric`, with `TransmitterLinkMetricPair` | Transmitter Link Metric (0x09) | §6.4.11, Tables 6-17 and 6-18 |
| `ReceiverLinkMetric`, with `ReceiverLinkMetricPair` | Receiver Link Metric (0x0A) | §6.4.12, Tables 6-19 and 6-20 |
| `LinkMetricResultCode` | Link Metric Result Code (0x0C) | §6.4.13, Table 6-21 |
| `PushButtonEventNotification<'a>`, with `PushButtonMedia<'a>` | Push Button Event Notification (0x12) | §6.4.19, Table 6-27 |
| `PushButtonJoinNotification` | Push Button Join Notification (0x13) | §6.4.20, Table 6-28 |

- Link Metric Query has two forms. With selector 0 the value is `[selector, metrics]` (2 octets),
  and the encoder always writes that form (note 0009 D3, amended by A4). The decoder also accepts
  an 8-octet value with selector 0, and drops its EUI-48 field as it would a reserved octet. With
  any other selector the value is `[selector, EUI-48, metrics]` (8 octets). `LinkMetricQuery::new`
  returns `None` for a non-zero selector without a MAC, and for selector 0 with a MAC, so neither
  kind of query can be built in a form the encoder would not write. Selector and metrics values
  are kept raw, including reserved values.
- Upstream's validation minimum for this TLV is 11 octets, which rejects the 2-octet form. The
  `spec_min` override lowers it to 5, so `validate` accepts a 2-octet query in a Link Metric Query
  message (note 0005 A1). A selector-0 value of 3 to 7 octets is `Truncated`, because the decoder
  reads a MAC; a non-zero selector with 2 octets is `Truncated`.
- Transmitter and Receiver Link Metric have no count octet. The number of pairs follows from the
  length: 12 + 29 × n octets for transmitter, 12 + 23 × n for receiver. A value with no pairs is
  `DecodeError::InvalidLength { len: 12 }`, because the spec says "one or more", and `new` returns
  `None` for an empty list. A partial pair is `DecodeError::Truncated`.
- Bridge flag, link availability (which is carried raw above 100), media type and result code are
  kept as the wire gives them.
- Push Button Event Notification may have zero media, as the spec allows. Each media's information
  is the `k` raw octets that follow its length byte, borrowed from the value (`PushButtonMedia::info`)
  whatever the media type. The codec does not interpret it.
- Push Button Join Notification is exactly 20 octets. A shorter value is `Truncated`, and a longer
  one is `TrailingBytes`.

## Autoconfiguration, search, topology and profile bodies (B03)

Design: [note 0009](../../docs/0009-tlv-bodies.md), D2, D3, D4, D5, D6 and D8. The bodies are in
`bodies::autoconfig`. Each doc comment cites the spec section and table, the audit rows, and the
upstream struct, sender and receiver lines.

| Body | TLV (type) | Spec |
|---|---|---|
| `SupportedService`, over `ServiceList` | Supported Service (0x80) | §17.2.1, Table 23 |
| `SearchedService`, over `ServiceList` | Searched Service (0x81) | §17.2.2, Table 24 |
| `ApRadioIdentifier`, over `MacValue` | AP Radio Identifier (0x82) | §17.2.3, Table 25 |
| `ApOperationalBss<'a>`, with `OperationalRadio<'a>` and `OperationalBss<'a>` | AP Operational BSS (0x83) | §17.2.4, Table 26 |
| `AssociatedClients`, with `AssociatedBss` and `AssociatedClient` | Associated Clients (0x84) | §17.2.5, Table 27 |
| `MultiApProfile` | Multi-AP Profile (0xB3) | §17.2.47, Table 69 |
| `Profile2ErrorCode` | Profile-2 Error Code (0xBC) | §17.2.51, Table 73 |
| `Bssid`, over `MacValue` | BSSID (0xB8) | §17.2.74, Table 96 |
| `AgentList`, with `Agent` | Agent List (0xD5) | §17.2.77, Table 99 |
| `SearchedRole` and `SupportedRole`, over `RoleValue` | Searched Role (0x0D), Supported Role (0x0F) | 1905.1 §6.4.14 and §6.4.16, Tables 6-22 and 6-24 |
| `AutoconfigFreqBand` and `SupportedFreqBand`, over `FrequencyBand` | Autoconfig Freq Band (0x0E), Supported Freq Band (0x10) | 1905.1 §6.4.15 and §6.4.17, Tables 6-23 and 6-25 |
| `Wsc<'a>` | WSC (0x11) | 1905.1 §6.4.18, Table 6-26 |
| `VendorOperationalBss`, with `VendorRadio` and `VendorBss` | Vendor operational BSS (0xF2) | none; upstream `VENDOR_OPERATIONAL_BSS` |

- Every count field accepts zero. Counts are derived from the lists on encode
  (`EncodeError::CountTooLarge` above 255, or above 65535 for the client count) and checked on decode.
- Multi-AP Profile: the strict form is one octet, and the raw value is kept, reserved values
  included. The lenient four-octet form that upstream's sender writes (`em_configuration.cpp:3614`)
  gives `Ok(Some(profile))` with the first octet. The three pad octets are ignored, as upstream's
  receiver ignores them (`em_msg.cpp:131-150`). Two, three and five octets are `TrailingBytes`, and an
  empty value is `Truncated`.
- WSC: the value is validated as a complete attribute list (`WscAttrIter`). An attribute whose
  length runs past the end of the value is `Truncated`. An empty value is valid and holds no
  attributes (note 0009 D5). The bytes are borrowed; typed attributes are not here yet.
- Profile-2 Error Code: the reason octet decides the layout. Reasons `0x07`, `0x08` and `0x0e` are
  followed by a 6-octet BSSID, `0x01` and `0x02` by a 4-octet rule ID, and `0x0b` by a 2-octet QoS
  management ID (both big-endian). Every other reason, reserved values included, has nothing after
  it. A cut or missing conditional field is `Truncated`, and extra octets are `TrailingBytes`.
  `Profile2ErrorCode::new` returns `None` when the conditional field does not match the reason, and
  its fields are private. Decode builds the field from the reason, so no invalid pair can be built or
  decoded.
- AP Operational BSS: each SSID is its raw bytes, borrowed from the TLV, with the length given by
  its length octet. A final NUL is kept (note 0009 D4), and a zero-length SSID is valid.
- Agent List: each entry is 8 octets (AL MAC, profile, security). Octets after the `Num_Agent`
  entries, including a partial entry, are `TrailingBytes`. An entry cut short within the count is
  `Truncated`.
- Vendor operational BSS (0xF2) has no spec table. Its haul type and VAP mode are two-octet
  little-endian fields (note 0009 D6), matching upstream's host-order writes at the pin.
- A cut or overstated value is `Truncated`. Extra octets after a complete fixed layout are
  `TrailingBytes`.

## Capability bodies (B04)

Design: [note 0009](../../docs/0009-tlv-bodies.md), D2, D3, D4, D8 and amendments A3 and A5. The
bodies are in `bodies::capability`. Each doc comment cites the spec section and table, the audit
rows, and the upstream struct, sender and receiver lines.

| Body | TLV (type) | Spec |
|---|---|---|
| `ApRadioBasicCapabilities<'a>`, with `BasicOperatingClass<'a>` | AP Radio Basic Capabilities (0x85) | §17.2.7, Table 29 |
| `ApHtCapabilities` | AP HT Capabilities (0x86) | §17.2.8, Table 30 |
| `ApVhtCapabilities` | AP VHT Capabilities (0x87) | §17.2.9, Table 31 |
| `ApHeCapabilities`, with `HeMcsSets` and `HeMcsSet` | AP HE Capabilities (0x88) | §17.2.10, Table 32 |
| `ClientInfo` | Client Info (0x90) | §17.2.18, Table 40 |
| `ClientCapabilityReport<'a>` | Client Capability Report (0x91) | §17.2.19, Table 41 |
| `ApCapability` | AP Capability (0xA1) | §17.2.6, Table 28 |
| `ApWifi6Capabilities`, with `Wifi6Role` | AP Wi-Fi 6 Capabilities (0xAA) | §17.2.72, Table 94 |
| `CacCapabilities<'a>`, with `CacRadio<'a>`, `CacMethod<'a>` and `CacOperatingClass<'a>` | CAC Capabilities (0xB2) | §17.2.46, Table 68 |
| `Profile2ApCapability` | Profile-2 AP Capability (0xB4) | §17.2.48, Table 70 |
| `ApRadioAdvancedCapabilities` | AP Radio Advanced Capabilities (0xBE) | §17.2.52, Table 74 |
| `BackhaulStaRadioCapabilities` | Backhaul STA Radio Capabilities (0xCB) | §17.2.65, Table 87 |
| `DeviceInventory<'a>`, with `InventoryRadio<'a>` | Device Inventory (0xD4) | §17.2.76, Table 98 |
| `ControllerCapability` | Controller Capability (0xDD) | §17.2.94, Table 116 |
| `Wifi7AgentCapabilities`, with `Wifi7Radio`, `Wifi7ModeCapabilities` and `Wifi7FrequencyRecord` | Wi-Fi 7 Agent Capabilities (0xDF) | §17.2.95, Table 117 |

- Lenient forms (A5, D4). An empty value of AP HT, VHT, HE and Wi-Fi 6 Capabilities, and of
  Backhaul STA Radio Capabilities, decodes leniently to `Ok(None)` (absent). The upstream senders
  write it when the radio data is missing (`em.cpp:1283-1293`, `:1376-1386`, `:1403-1413`,
  `:1466-1470`), or, for Backhaul STA, when there is no backhaul BSS (`em_capability.cpp:619-659`).
  The strict decoders reject it.
  Device Inventory with `Num_Radio` 0 (`em.cpp:1851-1854`) decodes leniently to `Ok(Some)` with
  no radios. The strict decoder and `DeviceInventory::new` require one. A leniently decoded
  zero-radio value does not re-encode: encoding fails with
  `EncodeError::CountTooSmall { len: 0, min: 1 }`, because the spec does not allow `Num_Radio` 0.
- AP Radio Basic Capabilities: the operating class groups are contiguous. Upstream's sender leaves
  holes when the data model holds other operating class entries for the same radio
  (`em.cpp:1105-1130`, audit 40). The holes cannot be told apart from groups, so there is no
  lenient form. Max TX power is signed (audit 43).
- AP HE Capabilities: the MCS field is 4, 8 or 12 octets (one, two or three bandwidth sets), and
  any other length is `InvalidLength`. Each set is the receive map, then the transmit map, each a
  big-endian 16-bit value (0009 A3). Upstream writes the transmit map first (`em.cpp:1429-1432`).
  The second capability octet uses the spec bits, SU beamformer bit 7 through DL OFDMA bit 1, with
  bit 0 reserved. Upstream's sender packs each of these flags one bit lower (`em.cpp:1444-1450`,
  audit 2).
- AP Wi-Fi 6 Capabilities: each role has its own MCS sets, receive map then transmit map, each
  little-endian (0009 A3; upstream writes transmit first, `em.cpp:1515-1517`). The MCS NSS length is
  an octet count (audit 222), and a length other than 4, 8 or 12 is `InvalidLength`. Role codes 2 and 3 are kept as reserved, and role counts are not
  capped at upstream's two (audit 220). Flag and MCS length mismatches are carried unchanged.
- AP VHT Capabilities: the MCS maps are big-endian (audit 48), where upstream copies host-order
  structs (`em.cpp:1388`).
- Profile-2 AP Capability is four octets: maximum prioritization rules, a reserved octet, the
  flags, and maximum VIDs. The upstream `create_prof_2_tlv` vector `00 00 10 00` (DPP onboarding
  only) decodes to that body and encodes back to the same bytes (test
  `upstream_create_prof_2_vector_decodes_to_dpp_only`). Upstream's M1 path writes no value bytes
  (audit 170); the encoder writes all four.
- Client Capability Report: result 0 carries the frame to the end of the value. Any other result,
  reserved codes included, carries no frame. The upstream zero-octet form is never sent (audit 80),
  so strict decoding requires the result octet and there is no lenient form.
- Backhaul STA Radio Capabilities: the MAC is present exactly when Included (bit 7) is set. Reserved
  bits are written as zero (audit 208).
- AP Radio Advanced Capabilities: one record per TLV, and the flags are written as given. Upstream's
  normal sender writes every flag as zero (audit 183). The port reads one record, where upstream
  accepts several (audit 185).
- CAC Capabilities: the duration is three octets, big-endian (audit 8). Counts are one octet each
  and the groups are packed, with no fixed upstream arrays (audit 7, 9–10).
- Device Inventory: the serial, software, execution environment and chipset vendor strings are each
  at most 64 octets. All radio records are decoded, not capped at upstream's three (audit 234–236).
- Wi-Fi 7 Agent Capabilities: frequency separation is five bits (0–31), and record counts are not
  capped at upstream's eight (audit 274). Link code 15 stays raw instead of meaning zero links
  (audit 277).
- Controller Capability: bit 7 is KiB/MiB counters and bit 6 is Early AP Capability. Reserved
  extension octets are ignored, and the encoder omits them.

## Channel selection and scan bodies (B05)

Design: [note 0009](../../docs/0009-tlv-bodies.md), D2, D3, D4, D8 and amendment A6. The bodies are
in `bodies::channel`. Each doc comment cites the spec section and table, the audit rows, and the
upstream struct, sender and receiver lines.

| Body | TLV (type) | Spec |
|---|---|---|
| `ChannelPreference<'a>`, with `PreferenceOperatingClass<'a>` | Channel Preference (0x8B) | §17.2.13, Table 35 |
| `RadioOperationRestriction`, with `RestrictedOperatingClass` and `RestrictedChannel` | Radio Operation Restriction (0x8C) | §17.2.14, Table 36 |
| `TransmitPowerLimit` | Transmit Power Limit (0x8D) | §17.2.15, Table 37 |
| `ChannelSelectionResponse` | Channel Selection Response (0x8E) | §17.2.16, Table 38 |
| `OperatingChannelReport`, with `OperatingChannel` | Operating Channel Report (0x8F) | §17.2.17, Table 39 |
| `ChannelScanReportingPolicy` | Channel Scan Reporting Policy (0xA4) | §17.2.37, Table 59 |
| `ChannelScanCapabilities<'a>`, with `ScanCapabilityRadio<'a>` and `ScanOperatingClass<'a>` | Channel Scan Capabilities (0xA5) | §17.2.38, Table 60 |
| `ChannelScanRequest<'a>`, with `ScanRequestRadio<'a>` and `ScanOperatingClass<'a>` | Channel Scan Request (0xA6) | §17.2.39, Table 61 |
| `ChannelScanResult<'a>`, with `ScanResultDetails<'a>`, `ScanNeighbor<'a>` and `ScanBssLoad` | Channel Scan Result (0xA7) | §17.2.40, Table 62 |
| `Timestamp<'a>` | Timestamp (0xA8) | §17.2.41, Table 63 |

- Lenient forms (D4, A6). Channel Scan Result: upstream's sender writes the full field set after
  any Scan Status (`em_channel.cpp:662-681`, `797-809`). After a nonzero status, the lenient
  decoder accepts that extra set only when its octets parse as the complete field set. It returns
  the status-only body, so the fields are dropped and the value re-encodes as the nine-octet spec
  form. Any other extra octets are an error. One stray octet after status 0x01 is still
  `TrailingBytes` in lenient mode. The strict decoder rejects the full form.
- Empty values (lenient only). Channel Scan Reporting Policy: the sender writes an empty value when
  no policy exists (`em_policy_cfg.cpp:273-302`, `:590-599`). Channel Scan Capabilities: the
  sender returns no value on its error path (`em.cpp:1721-1728`; caller `em_capability.cpp:196-202`).
  Both decode leniently to `Ok(None)` (absent). The strict decoders reject them.
- Channel Scan Request: Fresh=0 with `Num_OpClass` above 0 is `ValueOutOfRange`, which is the
  spec rule (Num_OpClass is 0 when Fresh is 0). Upstream's receiver rejects the same request
  (`em_channel.cpp:2206-2209`). Fresh=1 with no classes is accepted. The upstream sender never
  writes it (`em_channel.cpp:229-231`).
- Intentional differences from upstream (0008 D4): the bandwidth length byte is always present,
  including for an empty string. Upstream omits it for an unknown bandwidth (`em_channel.cpp:767`).
  Channel Scan Result details are decoded only for Scan Status 0x00. Upstream's sender writes them
  after any status (`em_channel.cpp:662-681`, `797-809`). The decoder keeps every neighbor, with
  no 16-neighbor cap (`em_channel.cpp:683-685`, `:2325-2327`), and Channel Scan Capabilities keeps
  every radio and class, with no 3-radio or 64-class limit (`em.cpp:1711`,
  `em_capability.cpp:1446`).
- Spec rules that upstream already follows: channel utilisation and station count are present
  only when bit 7 of the BSS colour octet is set (`em_channel.cpp:785-794`, receiver
  `:2376-2388`). The scan type is bit 7 alone (`em_channel.cpp:803-807`, `:2400`).
- Raw values: Channel Preference keeps reserved preference and reason values, and the preference
  octet of an empty channel list (audit rows 61–65). Upstream's controller sender skips preference
  15 (`em_channel.cpp:302`). Channel Scan Result keeps bandwidth strings as bytes (so `320` and an
  empty string are kept), SSID lengths above 32, and reserved scan-status values. Timestamps are
  bytes. Upstream writes the current local time, with a literal `Z`, and no fractional seconds
  (`utils/util.cpp:68-80`); the codec neither generates nor checks them.
- Zero counts are accepted where the spec allows them: an empty Channel Preference channel list
  (audit row 61), zero classes in an Operating Channel Report (row 76), zero radios or classes in
  Channel Scan Capabilities (row 132), and zero channels in a Channel Scan Request class, which
  means all channels (row 137).

## WSC attributes

The `wsc` module walks and writes the attributes of a WSC TLV value (2-byte id, 2-byte length,
value, all big-endian):

- `WscAttrIter` yields `Result<WscAttr, DecodeError>`. Empty input yields nothing. A header or
  value cut short is `DecodeError::Truncated`, and nothing is yielded after an error. A zero-length
  attribute is yielded with an empty value.
- `write_wsc_attr(&mut Writer, id, value)` writes one attribute. A value longer than 65535 bytes is
  `EncodeError::CountTooLarge { len, max: 65535 }`, and nothing is written.
- `wsc_msg_type` keeps upstream's quiet behaviour. Typed WSC attributes are not here yet (note 0009,
  D5).

## Lints

`lib.rs` denies `large_types_passed_by_value`, `large_stack_arrays` and `large_enum_variant` at
clippy's default thresholds, as `em-dm` does. There is no crate-local `clippy.toml`: it would replace
the workspace one, which carries the clone ban. `large_types_passed_by_value` skips `pub` functions,
so it does not check the public body API. Note 0009 (D9) asks each batch for a size test instead.

## Test support and golden data

- `body_testkit` (test only): `body_checks!` generates one test module per body from its golden
  bytes and expected value. The golden, `message_round_trip` (through `MsgBuilder::body` and
  `TlvIter`, which checks that exactly the End of Message TLV, `00 00 00`, follows the one TLV) and
  wrong-type tests always run. Each optional item adds its test: strict prefixes, a
  trailing byte, count too large, reserved bits, alternate values, lenient absent (`Ok(None)`),
  lenient values (`Ok(Some)`) and lenient rejects (`Err`). The lenient items also check that the
  golden bytes (the spec form) decode leniently to exactly the golden body, that a lenient fixture
  is rejected by the strict decoder, and prefix agreement: no non-empty prefix of the golden bytes,
  the whole golden included, decodes leniently to `Ok(None)`, and where the strict decoder accepts
  a prefix, the lenient decoder gives the same body. The sample bodies are `Sample`, `TailSample`
  and `LenientSample`, on the TLV types 0xF8, 0xF9 and 0xFA, which `em-types` does not name.
- Coverage is row-driven. Each body's `every_fixture_row_decodes_and_encodes_as_its_kind_says` test
  calls `sweep_group`, which takes every row of the group file (`body_fixtures::all_rows`) and
  passes it to the body's dispatcher by TLV code. A hand-kept list cannot leave a row out, and a
  row whose TLV code has no dispatcher arm fails. Each row is checked by its `kind` field:
  - `spec` (the default) decodes strictly through `decode_value` and `Tlv::decode`, and re-encodes
    to its own bytes, or to the canonical sibling row when one is named. A row with nonzero
    reserved bits may re-encode to other bytes. A lenient body also decodes the row leniently to
    the same body.
  - `invalid` is rejected by both entry points with the error variant named in `error=`. A lenient
    body must reject the row through both lenient entry points too, unless the row is named in the
    lenient inputs (the last argument of `sweep_row_lenient`). The listed rows are non-spec forms
    that the lenient decoder accepts on purpose: the empty values, and Device Inventory with zero
    radios in the capability group.
  - `lenient` is rejected by the strict decoders, decodes leniently through both lenient entry
    points to the same body, and re-encodes to bytes that the strict decoder reads back as that body.
    A `lenient` row marked `decoded=absent` (the Backhaul STA empty value) decodes to `Ok(None)`
    through both lenient entry points instead. The capability tests check it with
    `check_lenient_absent`.
- Every `sweep_row` and `sweep_row_lenient` check starts with `check_tlv_column`. It fails unless
  the row's `tlv` column (`0xNN`) is the body's TLV type, so a row cannot be checked by a body of
  another TLV type that has the same layout.
- The strict arm of `body_checks!` (a body without the `lenient;` marker) adds a `body_is_not_lenient`
  test. It calls `assert_not_lenient!`, which uses autoref specialization to detect a
  `TlvBodyLenient` implementation, so a lenient body fails it. A lenient body therefore cannot get
  only the strict checks by leaving out the marker. The `probe_refuses_a_lenient_body_without_the_marker`
  test shows that the probe fails on a lenient body.
- Allocation bound: `bodies::bounded_vec` sets the reservation of each counted list. It is at most
  `remaining / max(wire size, size_of the record)` records, so `capacity × size_of ≤ remaining` for
  the input's remaining bytes. Its unit test checks that bound, and
  `largest_bridging_count_decodes_within_the_input_size` checks a 255-tuple value.
- The `lenient;` marker goes on the line after `golden`. A body with a lenient form must carry it,
  and the three `lenient_*` items are a compile error without it. The marker also generates a
  `lenient_forms` test: the lenient decoder must give exactly the strict body on `golden` and on
  every `alternates:` and `reserved_bits:` fixture.
- `body_fixtures` (test only) reads `tests/data/bodies/<group>.tsv` through `include_str!`, by the
  `(tlv, case)` key, and decodes the hex column (`-` is the empty value). A missing or duplicate row
  panics. `assert_all_used` checks that the rows of a group and the keys its tests use are the same
  set, so every row is used. The `ieee1905` group has 48 rows, the `linkmetric` group 51, the
  `autoconfig` group 137, the `capability` group 167 and the `channel` group 155.
- `upstream_tlv_vectors` (test only) loads `tests/data/upstream_tlv_vectors.tsv`, and checks its
  case list and the bytes of `create_prof_2_tlv` and `create_error_code_tlv`.
- `tools/check-body-fixtures` compares two independent builds of a body group,
  `tests/data/bodies/<group>.a.tsv` and `<group>.b.tsv`. It writes `<group>.tsv` only if the builds
  agree on every row, with the build hashes as provenance. Without `--group`, every group with a
  file in the directory is processed, so a lone file is an error. A group name cannot be empty,
  cannot end in `.a` or `.b`, and cannot contain a path separator, a control character, or U+2028
  or U+2029. Build files are split on LF only, and one CR before each LF is removed; a bare CR is
  an error. Control characters (Unicode category Cc), U+2028 and U+2029 are rejected on every line,
  comments included (tabs excepted). The tool docstring gives the case, field and 65535-byte value
  rules. `--check` compares a fresh build with the
  committed fixture, and succeeds when there are no group files. Exit 0 is success, 1 is a missing
  build, malformed input, disagreement or a stale fixture, and 2 is usage or I/O. Python tests:
  `tools/test_check_body_fixtures.py`.
- `tools/upstream-tlv-vectors/regen` compiles the pinned upstream senders against a declaration
  shim and writes `tests/data/upstream_tlv_vectors.tsv`; `--check` rebuilds it in memory. The
  [tool README](../../tools/upstream-tlv-vectors/README.md) covers the extraction and how to add a
  sender. Python tests: `tools/test_upstream_tlv_vectors.py`.
- Generated fixtures are never edited by hand.

## Intentional differences from upstream

- Validation is stateless: no presence flags or error counters survive between calls (upstream
  defect 1), and lengths are checked before subtracting (defect 2).
- Errors are returned as values, not printed.
- The size budgets count the CMDU only: 1500 bytes (raw socket) and 65517 bytes (AL-SAP).
- Stricter checks (length of every instance, length of optional TLVs) are not done, because
  upstream's own senders would fail them (note 0005 D3).
- 26 mandatory-row minimum lengths are lowered to the spec minimum where upstream rejects
  spec-valid TLVs (`spec_min`, note 0005 D3 and 0008 D8). The upstream tables themselves are kept
  verbatim.

## Status

Step 2 (crate) implemented. Batches B00a and B00b of note 0009 (body traits and entry points, error
kinds, WSC attribute walker and writer, test macro, lints, and the two fixture tools) are
implemented and awaiting review. B01 (the seven IEEE 1905.1 identity and neighbor bodies and the
`ieee1905` fixture), B02 (the six link metric and push button bodies and the `linkmetric`
fixture) and B03 (the fifteen autoconfiguration, search, topology, profile and operational BSS
bodies and the `autoconfig` fixture) are implemented. B04 (the fifteen capability bodies and the
`capability` fixture) and B05 (the ten channel selection and scan bodies and the `channel` fixture)
are implemented and under review. See [STATUS.md](../../STATUS.md).
