# em-msg

IEEE 1905 CMDU header, TLV framing, message builder, message validation (spec §17) and the TLV body reader and writer.
Synchronous, depends only on `em-types`, no `unsafe`, no clones.

## What is here

| Module | Holds | Upstream |
|---|---|---|
| `header` | 8-byte CMDU header, `default_relay` | `em_cmdu_t` (`em_base.h`), `add_1905_header` (`em_msg.cpp`) |
| `tlv` | `TlvIter`, a borrowing walk over the TLV area | `em_tlv_t`, the walk of `validate` (`em_msg.cpp`). `TlvIter` does not stop at a zero-length TLV; upstream's `get_first_tlv` and `get_next_tlv` are in `readers` |
| `readers` | The field readers `al_mac_address`, `supported_services`, `profile`, `sta_mac`, `bss_id`, `radio_id`, `freq_band`, `find_tlv`; the offset walker `first_tlv` and `next_tlv` | `em_msg_t` readers (`em_msg.cpp:71-480`): `get_al_mac_address`, `get_supported_service`, `parse_profile_tlv`, `get_profile`, `get_profile_type`, `get_sta_mac`, `get_bss_id`, `get_radio_id`, `get_freq_band`, `get_tlv`, `get_first_tlv`, `get_next_tlv` (note 0009 D6, A10) |
| `builder` | `MsgBuilder` with a size budget, End of Message added by `finish` | `em_msg_builder_t` (`em_msg.h`, `em_msg.cpp`), `add_tlv` |
| `wsc` | WSC Message Type selector (M1 = 4, M2 = 5); WSC attribute walker `WscAttrIter` and writer `write_wsc_attr`; typed attributes `WscAttribute` and ids `WscAttrId`, with the Multi-AP and haul-type Vendor Extensions | `em_configuration_t::get_wsc_msg_type` (`em_configuration.cpp`); the walker, writer and typed attributes are new (note 0009, D5 and A9) |
| `bodies` | `TlvBody`, `TlvBodyLenient`: the traits every typed TLV body implements; the seven IEEE 1905.1 identity and neighbor bodies in `bodies::ieee1905`; the six link metric and push button bodies in `bodies::linkmetric`; the fifteen autoconfiguration, search, topology, profile, WSC and operational BSS bodies in `bodies::autoconfig`; the fifteen capability bodies in `bodies::capability`; the ten channel selection and scan bodies in `bodies::channel`; the eleven CAC, anticipated channel, spatial reuse and spectrum inquiry bodies in `bodies::cac`; the fourteen metrics bodies in `bodies::metrics`; the twelve steering, association control, backhaul steering and code bodies in `bodies::steering`; the ten policy and QoS bodies in `bodies::policy`; the eleven BSS configuration and Wi-Fi 7 MLD bodies in `bodies::mld`; the thirteen DPP, security and encapsulation bodies in `bodies::security` | the typed TLV bodies, added in batches (note 0009) |
| `tables` | The 70 validation tables as static data | the table functions `autoconfig_search` .. `i1905_ack` (`em_msg.cpp`) |
| `validate` | `validate(cmdu, profile)` | `em_msg_t::validate` (`em_msg.cpp`) |
| `spec_min` | Spec minimum lengths that replace upstream's stricter ones (26 rows) | - |
| `body` | `Reader`, `Writer` for TLV bodies | hand-written readers and writers replace upstream's packed C structs copied into the frame (note 0008) |
| `error` | `FramingError`, `BuildError`, `DecodeError`, `EncodeError`, `ReaderError` | - |

Design: [note 0005](../../docs/0005-em-msg-scope.md); typed TLV bodies, the WSC attribute
walker and the field readers: [note 0009](../../docs/0009-tlv-bodies.md) (D6 and A10 for the readers). The tables equal the golden fixture
`tests/data/upstream_msg_tables.tsv`, which is generated from upstream by `tools/gen-upstream-msg-tables`.

Upstream source: `src/em/em_msg.cpp`, `inc/em_msg.h`, and `get_wsc_msg_type` in
`src/em/config/em_configuration.cpp`. Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.

## What is not here

- The Ethernet header: `em-transport` (it keeps the MAC addresses next to the CMDU buffer).
  `CmduHeader::parse` and `validate` take the whole CMDU (from the 8-byte header); `TlvIter::new`,
  `wsc_msg_type` and the field readers take the TLV area after the header; `MsgBuilder` produces the
  whole CMDU.
- The policy for a received Profile TLV, and the fallback for raw profile 0: `em-core` (note 0005 D5.3).
- The pre-checks and return codes of the callers of `first_tlv` and `next_tlv` (note 0009 A10).
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
  It returns `Ok(None)` for an empty value, which the consumer treats as "TLV absent", and for the
  nine zero octets of Associated Wi-Fi 6 STA Status Report (B07, note 0009 A7). Every other lenient
  form is `Ok(Some(body))`.
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

## CAC, channel usage, spatial reuse and spectrum inquiry bodies (B06)

Design: [note 0009](../../docs/0009-tlv-bodies.md), D2, D3, D5 and D8. The bodies are in
`bodies::cac`. Each doc comment cites the spec section and table, the audit rows, and the
upstream struct, sender and receiver lines.

| Body | TLV (type) | Spec |
|---|---|---|
| `CacRequest`, with `CacRequestRadio` | CAC Request (0xAD) | §17.2.42, Table 64 |
| `CacTermination`, with `CacTerminationRadio` | CAC Termination (0xAE) | §17.2.43, Table 65 |
| `CacCompletionReport`, with `CacCompletionRadio` and `CacDetectedPair` | CAC Completion Report (0xAF) | §17.2.44, Table 66 |
| `CacStatusReport`, with `CacAvailableChannel`, `CacNonOccupancyChannel` and `CacActiveChannel` | CAC Status Report (0xB1) | §17.2.45, Table 67 |
| `AnticipatedChannelPreference<'a>`, with `AnticipatedOperatingClass<'a>` | Anticipated Channel Preference (0xD6) | §17.2.87, Table 109 |
| `AnticipatedChannelUsage<'a>`, with `AnticipatedUsage<'a>` | Anticipated Channel Usage (0xD7) | §17.2.88, Table 110 |
| `SpatialReuseRequest`, with `SpatialReuseParameters` | Spatial Reuse Request (0xD8) | §17.2.89, Table 111 |
| `SpatialReuseReport`, with `SpatialReuseParameters` | Spatial Reuse Report (0xD9) | §17.2.90, Table 112 |
| `SpatialReuseConfigResponse` | Spatial Reuse Config Response (0xDA) | §17.2.91, Table 113 |
| `AvailableSpectrumInquiryRequest<'a>`, over `SpectrumInquiryObject<'a>` | Available Spectrum Inquiry Request (0xE8) | §17.2.104, Table 126 |
| `AvailableSpectrumInquiryResponse<'a>`, over `SpectrumInquiryObject<'a>` | Available Spectrum Inquiry Response (0xE9) | §17.2.105, Table 127 |

- Counts are one octet each. Zero is accepted and decodes to an empty list. Counts are derived
  from the lists on encode (`EncodeError::CountTooLarge` above 255). The codec does not apply
  upstream's fixed array sizes, such as the data model's four detected pairs
  (`EM_MAX_CAC_METHODS`, `em_base.h:97`).
- CAC Status Report: the codec returns `EncodeError::CountTooLarge` for more than 255 entries in
  a list. This is an intentional difference (0008 D4). Upstream's sender keeps each list count
  in an 8-bit counter, so the 256th entry wraps the count to 0 (`em_channel.cpp:1404`, `:1430`,
  `:1458`). Minutes and non-occupancy durations are big-endian 16-bit values. The active
  countdown is three big-endian octets. A value above `0xFF_FFFF` is
  `EncodeError::ValueTooLarge`, an intentional difference: upstream writes only the low 24 bits, so
  the value would wrap (`em_channel.cpp:1449-1451`).
- CAC Request: the method is three bits (bits 7 to 5) and the completion action is two bits
  (bits 4 and 3), both kept raw, including reserved codes. Bits 2 to 0 are reserved: they are
  ignored on decode and written as zero. Upstream's completion-action field is three bits
  (`em_base.h:1048-1050`), one bit wider than the spec.
- CAC Completion Report: the status is kept raw, and each radio keeps its own detected pairs.
  Whether the status and the pairs agree is left to the consumer.
- Anticipated Channel Preference: the four reserved octets after each class's channel list are
  ignored on decode and written as zero.
- Anticipated Channel Usage: each RU bitmask is carried with its length octet, so any length
  from 0 to 255 round-trips. The bitmask is borrowed from the value. Upstream's struct fixes it
  at 10 octets (`em_base.h:988`). The power level is signed (`-1` is unknown), and the reason
  and transmitter are kept raw. The four reserved octets are ignored and written as zero.
- Spatial reuse: the BSS colour is six bits. In the request flags, bit 4 is the HESIGA value-15
  flag, bit 3 the SRG information flag, bit 2 the non-SRG offset flag and bit 0 the PSR flag. In
  the report, bit 6 of the colour octet is the partial BSS colour flag. Other bits are reserved,
  ignored and written as zero. The offsets and bitmaps are present even when the matching
  validity flag is clear. The request is 29 octets and the report 37 octets, and a shorter value
  is `Truncated`. The two trailing reserved octets of each are ignored and written as zero.
- Spatial Reuse Config Response: the response code is kept raw, reserved codes included.
- Spectrum inquiry: the object is the supplied octets, borrowed from the value, and is not parsed
  as JSON (0009 D5). An empty value is `DecodeError::Truncated` on decode and
  `EncodeError::CountTooSmall { len: 0, min: 1 }` on encode. Upstream's sender writes the object
  as uninitialised pointer-sized bytes, eight octets on a 64-bit build (`em_channel.cpp:1649-1663`).
- No body in this batch has a lenient form. Those upstream octets are opaque to the codec: the
  strict decoder reads any non-empty spectrum inquiry value (0009 D5), so no non-spec form needs a
  lenient decoder.

## Metrics bodies (B07)

Design: [note 0009](../../docs/0009-tlv-bodies.md), D2, D3, D4, D5, D6, D8 and amendment A7. The
bodies are in `bodies::metrics`. Each doc comment cites the spec section and table, the audit
rows, and the upstream struct, sender and receiver lines.

| Body | TLV (type) | Spec |
|---|---|---|
| `ApMetricsQuery` | AP Metrics Query (0x93) | §17.2.21, Table 43 |
| `ApMetrics` | AP Metrics (0x94) | §17.2.22, Table 44 |
| `StaMacAddress` | STA MAC Address Type (0x95) | §17.2.23, Table 45 |
| `AssociatedStaLinkMetrics`, with `AssociatedStaBssMetrics` | Associated STA Link Metrics (0x96) | §17.2.24, Table 46 |
| `UnassociatedStaLinkMetricsQuery`, with `UnassociatedQueryChannel` | Unassociated STA Link Metrics Query (0x97) | §17.2.25, Table 47 |
| `UnassociatedStaLinkMetricsResponse`, with `UnassociatedStaMetrics` | Unassociated STA Link Metrics Response (0x98) | §17.2.26, Table 48 |
| `BeaconMetricsQuery<'a>`, with `ApChannelReport<'a>` | Beacon Metrics Query (0x99) | §17.2.27, Table 49 |
| `BeaconMetricsResponse<'a>`, with `MeasurementReportElement<'a>` | Beacon Metrics Response (0x9A) | §17.2.28, Table 50 |
| `AssociatedStaTrafficStats` | Associated STA Traffic Stats (0xA2) | §17.2.35, Table 57 |
| `MetricCollectionInterval` | Metric Collection Interval (0xC5) | §17.2.59, Table 81 |
| `RadioMetrics` | Radio Metrics (0xC6) | §17.2.60, Table 82 |
| `ApExtendedMetrics` | AP Extended Metrics (0xC7) | §17.2.61, Table 83 |
| `AssociatedStaExtendedLinkMetrics`, with `ExtendedLinkMetricsBss` | Associated STA Extended Link Metrics (0xC8) | §17.2.62, Table 84 |
| `AssociatedWifi6StaStatusReport`, with `Wifi6TidQueueSize` | Associated Wi-Fi 6 STA Status Report (0xB0) | §17.2.73, Table 95 |

- Counts are one octet each, and zero is accepted where the spec allows it; a zero count decodes
  to an empty list. Counts are derived from the lists on encode (`EncodeError::CountTooLarge`
  above 255). The codec does not apply upstream's fixed sizes: 16 channels and 64 stations per
  channel (`em_base.h:237-238`), and 64 unassociated entries (`em_base.h:650`).
- Upstream's validator wants at least one entry for AP Metrics Query (`em_msg.cpp:953`), and at
  least one BSS for Associated STA Link Metrics and Associated STA Extended Link Metrics (audit rows
  94 and 202). The codec accepts zero in each, as the spec allows.
- An Unassociated STA Link Metrics Query carries one operating class per TLV. Octets after the last
  channel are `TrailingBytes`, where upstream's receiver parses further blocks (audit row 97).
- Multi-octet counters, rates and times are big-endian, as upstream writes them with `htonl` and
  `htons` (for example `em_metrics.cpp:1625`). The exception is Metric Collection Interval, which
  is little-endian because upstream copies the host-order value (`em.cpp:1899`, audit row 195,
  0009 D6).
- Raw values: RCPI (221-255 are reserved), noise and utilization, Reporting Detail, TIDs and
  queue sizes, and channel numbers are kept as given, reserved values included (0009 D2). Byte
  counters are kept in the sender's units (audit rows 120 and 199).
- Measurement Report elements are raw (0009 D5): the element ID is not checked against 39 and the
  body is not parsed. A length above the remaining octets is `Truncated`, which upstream's receiver
  does not check (audit row 106). The reserved octet of Beacon Metrics Response is ignored on
  decode and written as zero.
- AP Metrics flags: bit 7 is `IncludeBE`, bit 6 `IncludeBK`, bit 5 `IncludeVO` and bit 4
  `IncludeVI`. Bits 3-0 are ignored on decode and written as zero. The optional estimated service
  parameter fields follow in BK, VO, VI order, not bit order. The BE field is always on the wire,
  so a clear `IncludeBE` sizes nothing and is kept as read (audit row 87).
- Beacon Metrics Query: the value rules (a Channel Number other than 255 with reports, `Num_Ids`
  with a Reporting Detail other than 1, the 802.11 detail codes, an SSID over 32 octets) are for
  the consumer. A Report Length of zero is `InvalidLength`, as upstream's receiver also rejects it
  (`em_metrics.cpp:389-393`).

Lenient forms (0009 D4, A7):

- AP Metrics (0x94): an empty value is `Ok(None)`. Upstream sends it for a BSS whose radio was not
  requested (`em_metrics.cpp:1946`).
- Metric Collection Interval (0xC5): an empty value is `Ok(None)`. Upstream sends no value when
  there is no radio capability information (`em.cpp:1886-1896`).
- Associated Wi-Fi 6 STA Status Report (0xB0): exactly nine zero octets are `Ok(None)`, meaning "no
  report". This is the one exception to "`Ok(None)` only for an empty value". Upstream sends nine
  zero octets for each station in a Profile-3 AP Metrics Response (`em_metrics.cpp:1547-1548` and
  `:2299-2306`, with the caller's buffer zeroed at `em_msg.cpp:607`). They are a zero MAC, no TIDs
  and two stray octets, so the strict decoder rejects them with `TrailingBytes { remaining: 2 }`.
  Other values are decoded as the spec form.
- Beacon Metrics Query (0x99): upstream's default query has no `Num_Ids` octet (audit row 101),
  because its sender writes `Num_Ids` only with Reporting Detail 1 and at least one element ID
  (`em_metrics.cpp:1885-1892`). The lenient decoder accepts a value that ends exactly after the AP
  Channel Report group and returns the body with no element IDs. Any other shortfall is
  `Truncated`, including one that ends inside a report's channel list. The encoder always writes
  `Num_Ids` (see below).

Intentional differences from upstream (B07):

- Beacon Metrics Query: the encoder always writes `Num_Ids`, even with no element IDs. Upstream's
  sender writes it only with Reporting Detail 1 and at least one element ID
  (`em_metrics.cpp:1885-1892`), and its receiver accepts a value without it
  (`em_metrics.cpp:416-418`, audit rows 101 and 105). The lenient decoder accepts upstream's shorter
  form.
- Associated Wi-Fi 6 STA Status Report: the strict decoder rejects upstream's nine-octet form. Only
  the lenient decoder maps it to `Ok(None)` (A7, `em_metrics.cpp:2299-2306`).
- Zero BSSID count in AP Metrics Query (0x93), Associated STA Link Metrics (0x96) and Associated STA
  Extended Link Metrics (0xC8): a count of zero is spec-valid and these bodies accept it. Upstream's
  message validator requires room for one BSSID (`em_msg.cpp:953`, `:975`, `:977`, and the
  length check for mandatory TLVs at `:683-688`; audit rows 84, 94 and 202).
- The fixed upstream limits are not applied: an Unassociated STA Link Metrics Query with more than
  16 channels or more than 64 stations per channel, which upstream's receiver rejects whole
  (`em_metrics.cpp:2581-2584`, `:2607-2610`, `em_base.h:237-238`, audit row 96), and more than 64
  entries in an Unassociated STA Link Metrics Response, which it drops after the 64th
  (`em_metrics.cpp:503-509`, `em_base.h:650`, audit row 100).

## Steering, association control and code bodies (B08)

Design: [note 0009](../../docs/0009-tlv-bodies.md), D2, D4, D6, D8 and amendment A8. The bodies are
in `bodies::steering`. Each doc comment cites the spec section and table, the audit rows, and the
upstream struct, sender and receiver lines.

| Body | TLV (type) | Spec |
|---|---|---|
| `ClientAssociationEvent` | Client Association Event (0x92) | §17.2.20, Table 42 |
| `SteeringRequest`, with `SteeringMode`, `SteeringTarget` and `SteeringFlags` | Steering Request (0x9B) | §17.2.29, Table 51 |
| `SteeringBtmReport` | Steering BTM Report (0x9C) | §17.2.30, Table 52 |
| `ClientAssociationControlRequest` | Client Association Control Request (0x9D) | §17.2.31, Table 53 |
| `BackhaulSteeringRequest` | Backhaul Steering Request (0x9E) | §17.2.32, Table 54 |
| `BackhaulSteeringResponse` | Backhaul Steering Response (0x9F) | §17.2.33, Table 55 |
| `HigherLayerData<'a>` | Higher Layer Data (0xA0) | §17.2.34, Table 56 |
| `ErrorCode` | Error Code (0xA3) | §17.2.36, Table 58 |
| `AssociationStatusNotification`, with `AssociationStatusEntry` | Association Status Notification (0xBF) | §17.2.53, Table 75 |
| `Profile2SteeringRequest`, with `Profile2SteeringTarget` | Profile-2 Steering Request (0xC3) | §17.2.57, Table 79 |
| `StatusCode` | Status Code (0xC9) | §17.2.63, Table 85 |
| `ReasonCode` | Reason Code (0xCA) | §17.2.64, Table 86 |

- `SteeringFlags` holds the three BTM flags of the Steering Request and Profile-2 Steering Request
  octet: bit 6 is BTM Disassociation Imminent, bit 5 Abridged and bit 4 Link Removal Imminent. Bit 7
  is Request Mode (1 is a mandate, 0 an opportunity), and bits 3-0 are reserved, ignored on decode
  and written as zero (`em_base.h:1168-1173`).
- Steering Request: in opportunity mode the spec sets the target count to zero, so the body has no
  targets there. A mandate may list any number of targets, zero included. The Steering Opportunity
  Window is carried in both modes. Upstream writes it as zero in a mandate (`em_steering.cpp:480-483`).
- Steering Request is the only body here with a lenient form (A8). In opportunity mode a non-zero
  target count is `ValueOutOfRange` for the strict decoder. The lenient decoder accepts upstream's
  form, which its only sender always writes with one target (`em_steering.cpp:488-493`), and drops
  the targets. Re-encoding gives the same request with no targets.
- Byte orders: the Steering Opportunity Window is little-endian, as upstream writes it with a plain
  assignment (`em_steering.cpp:485`, 0009 D6). The BTM Disassociation Timer is big-endian (`htons`,
  `em_steering.cpp:487`). Profile-2 Steering Request has no upstream sender, so both of its fields
  are big-endian. The Validity Period of Client Association Control Request is big-endian
  (`em_steering.cpp:79`), and so are Status Code and Reason Code (`em_configuration.cpp:631`,
  `:645`).
- Client Association Event: bit 7 of the last octet is the Association Type (1 joined, 0 left), and
  bits 6-0 are reserved, ignored on decode and written as zero. The strict decoder requires exactly
  13 octets. Upstream's receiver does not check the length (audit row 82).
- Steering BTM Report: the Target BSSID is 0 or 6 octets. Upstream writes only the 13-octet form,
  with the target commented out (`em_steering.cpp:500-524`, audit row 108). That form is
  spec-valid, so there is no lenient form. One to five octets after the status are `Truncated`,
  and more than six are `TrailingBytes`.
- Client Association Control Request: the control code is raw (0 block, 1 unblock, 2 timed block,
  3 indefinite block, 4-255 reserved). The count must be at least one: zero is `InvalidLength`, and
  an empty list does not encode.
- Backhaul Steering Request (14 octets) and Backhaul Steering Response (13 octets) have no
  production sender or receiver upstream, only a test generator (`validation_test.cpp:1232-1262`
  and `:1188-1230`). The response's result code is raw: 0 success, 1 failure, 2-255 reserved.
- Higher Layer Data: the protocol octet is raw (1 is the TR-181 transport protocol; 0 and 2-255 are
  reserved). The payload is the rest of the value and may be empty. Upstream's struct holds a
  pointer (`em_base.h:1131-1132`) and cannot describe an inline payload (audit row 118). There is no
  production sender or receiver.
- Error Code: 7 octets, the reason first and then the STA MAC. The MAC is always present, but a
  recipient uses it only for reasons 1 and 2. The reason is raw (0 and 7-255 are reserved). Upstream
  sends reason 0 with a zero MAC (audit row 123).
- Association Status Notification: the count may be zero, which the spec does not forbid. Each
  entry is a BSSID and an allowance octet (0 no more associations, 1 allowed, 2-255 reserved), kept
  raw. Upstream has no sender or receiver, and its struct has the wrong layout (audit row 187).
- Profile-2 Steering Request: each target has a reason code octet after its channel, so a target is
  9 octets. A non-zero target count in opportunity mode is `ValueOutOfRange` (A8), and there is no
  lenient form. Upstream has no builder or parser (`em_base.h:1212-1215`, test generator
  `validation_test.cpp:1366-1373`).
- Status Code and Reason Code: two big-endian octets, kept raw (IEEE 802.11 Tables 9-50 and 9-49).

Intentional differences from upstream (B08):

- Steering Request (0x9B) in opportunity mode with targets: the strict decoder returns
  `ValueOutOfRange`, where upstream's sender writes one target (`em_steering.cpp:488-493`). The
  lenient decoder accepts the form and drops the targets (A8).
- Client Association Control Request (0x9D): every control code and up to 255 stations are
  accepted. Upstream refuses codes other than 0 and 1 (`em_steering.cpp:667-699`) and more than 20
  stations (`em_steering.cpp:658`, `em_base.h:122`, audit rows 112 and 113).

## Policy bodies (B09)

Design: [note 0009](../../docs/0009-tlv-bodies.md), D2 to D5, D8 and D9. The bodies are in
`bodies::policy`. Each doc comment cites the spec section and table, the audit rows, and the
upstream struct, sender and receiver lines.

| Body | TLV (type) | Spec |
|---|---|---|
| `SteeringPolicy`, with `RadioSteeringPolicy` | Steering Policy (0x89) | §17.2.11, Table 33 |
| `MetricReportingPolicy`, with `RadioMetricReportingPolicy` | Metric Reporting Policy (0x8A) | §17.2.12, Table 34 |
| `Default8021QSettings` | Default 802.1Q Settings (0xB5) | §17.2.49, Table 71 |
| `TrafficSeparationPolicy<'a>`, with `TrafficSeparationSsid<'a>` | Traffic Separation Policy (0xB6) | §17.2.50, Table 72 |
| `UnsuccessfulAssociationPolicy` | Unsuccessful Association Policy (0xC4) | §17.2.58, Table 80 |
| `BackhaulBssConfiguration` | Backhaul BSS Configuration (0xD0) | §17.2.66, Table 88 |
| `ServicePrioritizationRule` | Service Prioritization Rule (0xB9) | §17.2.70, Table 92 |
| `DscpMappingTable` | DSCP Mapping Table (0xBA) | §17.2.71, Table 93 |
| `QosManagementPolicy` | QoS Management Policy (0xDB) | §17.2.92, Table 114 |
| `QosManagementDescriptor<'a>` | QoS Management Descriptor (0xDC) | §17.2.93, Table 115 |

- Byte orders: every multi-octet field is big-endian. That covers the VLAN IDs (0xB5, 0xB6), the
  4-octet maximum reporting rate (0xC4), the Rule Id (0xB9) and the QMID (0xDC). Upstream writes
  the VLAN IDs and the rate in network order (audit rows 172, 175 and 193). Nothing upstream writes
  the Rule Id or the QMID (audit rows 216 and 272), so their order follows the spec (§3.1.2).
- Reserved values are kept raw (0009 D2). These are Steering Policy 3-255 (0 disallowed, 1
  mandated and 2 allowed are the defined codes), RCPI thresholds 221-255, Precedence 0xFF, Output
  0x0A-0xFF, the DSCP table PCP values 0x08-0xFF and the VLAN IDs 0x0000-0x0002 and 0x0FFF-0xFFFF.
  Reserved bits of a flags octet are ignored on decode and written as zero.
- Lenient forms (`TlvBodyLenient`): the empty value of 0x8A, 0xB5 and 0xC4 decodes to `Ok(None)`,
  which is upstream's "no policy". Upstream's senders return 0 when there is no policy, and their
  callers still emit the TLV header with length 0 (`em_policy_cfg.cpp:546-556`, `:561-570` and
  `:604-616`; audit rows 57 and 173). The strict decoders reject the empty value with `Truncated`.
  No other lenient form exists.
- Bit-field octets: the 0x8A flags (bits 7-5), the 0xB5 PCP (bits 7-5), the 0xC4 report flag
  (bit 7), the 0xD0 profile flags (bits 7 and 6), and the 0xB9 add and always-match flags (bit 7).
  Each flag is tested alone in a zero value, so a fixture row that sets several flags cannot hide a
  swap.
- Fixed layouts: 0xB5 is exactly 3 octets, 0xC4 exactly 5, 0xD0 exactly 7 (one TLV per BSS, so a
  second entry is `TrailingBytes`), 0xB9 exactly 8 and 0xBA exactly 64. A value that ends early is
  `Truncated`.
- Variable layouts: the 0xB6 SSIDs are raw bytes, from 0 to 255 octets, and their lengths above 32
  are kept. On encode, the 0xB6 count is the list length (audit row 180, 0009 D3). The 0xDC
  descriptor element is the rest of the value as raw octets and may be empty (0009 D5); it is not
  parsed as an MSCS, SCS or QoS Management element here.
- 0xB5 carries VLAN 0 as given. Autoconfiguration sends an all-zero Default 802.1Q Settings TLV as
  a placeholder (`em_configuration.cpp:3758-3761`, audit row 174, 0009 D3).
- 0xDB: the 20 reserved octets are ignored on decode, and encode writes zeros.
- Encoders write only the bytes the strict decoder accepts. A PCP above 7 is
  `EncodeError::ValueTooLarge`. A list of more than 255 entries, and an SSID of more than 255
  octets, is `EncodeError::CountTooLarge`.
- 0xB9, 0xBA and 0xDC have no upstream sender or receiver. Their upstream structs appear only in
  the validator tables (`em_msg.cpp:1015-1017,1117-1119`) and in test generators
  (`validation_test.cpp:2544-2564`; audit rows 216, 218 and 272).

Intentional differences from upstream (B09):

- Steering Policy (0x89): up to 255 MACs per list and up to 255 radios are accepted. Upstream's
  receiver copies each count into a 16-entry MAC array and a 4-entry radio array
  (`em_policy_cfg.cpp:747-768`, `em_base.h:90` and `:101`), and overflows its stack object for a
  larger count (audit rows 53 and 54).
- Metric Reporting Policy (0x8A): up to 255 radios are accepted. Upstream copies each record into a
  4-entry array (`em_policy_cfg.cpp:770-784`, `em_base.h:90`), and overflows for more (audit row
  58).
- Traffic Separation Policy (0xB6): every SSID is parsed, up to 255. Upstream parses the entries
  only when Num_SSID is at most 5 (`em_policy_cfg.cpp:795-796`, `em_haul_type_max` at
  `em_base.h:2494`), so a TLV with more SSIDs is silently dropped (audit row 177). Upstream also
  skips an SSID longer than `MAX_SSID_NAME_LEN` without advancing past it (`em_policy_cfg.cpp:804`,
  audit row 178).
- QoS Management Policy (0xDB): lists of up to 255 MACs are accepted. Upstream's sender caps each
  list at 16 MACs (`em_policy_cfg.cpp:357-362`, `em_base.h:102-103`; audit row 271). Its receiver
  also keeps at most 4 of these TLVs per message (`em_policy_cfg.cpp:864`, `em_base.h:89`), and at
  most 16 Backhaul BSS Configuration TLVs (`em_policy_cfg.cpp:851`): message-level limits for the
  consumer, not body differences.
- Unsuccessful Association Policy (0xC4): the rate keeps all 32 bits. Upstream stores it in a 16-bit
  field through `static_cast<unsigned short>` (`em_policy_cfg.cpp:840-841`, audit row 194).
- Lenient empty forms (0x8A, 0xB5, 0xC4): upstream sends them, so they decode leniently. See the
  lenient forms above.

## BSS configuration and Wi-Fi 7 MLD bodies (B10)

Design: [note 0009](../../docs/0009-tlv-bodies.md), D2 to D5, D8, D9 and A8. The bodies are in
`bodies::mld`. Each doc comment cites the spec section and table, the audit rows, and the upstream
struct, sender and receiver lines.

| Body | TLV (type) | Spec |
|---|---|---|
| `BssConfigurationReport<'a>`, with `BssConfigurationReportRadio<'a>` and `BssConfigurationReportBss<'a>` | BSS Configuration Report (0xB7) | §17.2.75, Table 97 |
| `BssConfigurationRequest<'a>` | BSS Configuration Request (0xBB) | §17.2.84, Table 106 |
| `BssConfigurationResponse<'a>` | BSS Configuration Response (0xBD) | §17.2.85, Table 107 |
| `AgentApMldConfiguration<'a>`, with `ApMld<'a>` and `AffiliatedAp` | Agent AP MLD Configuration (0xE0) | §17.2.96, Table 118 |
| `BackhaulStaMldConfiguration`, with `AffiliatedBsta` | Backhaul STA MLD Configuration (0xE1) | §17.2.97, Table 119 |
| `AssociatedStaMldConfigurationReport`, with `AffiliatedSta` | Associated STA MLD Configuration Report (0xE2) | §17.2.98, Table 120 |
| `MldStructure` | MLD Structure (0xE3, spec only) | §17.2.99, Table 121 |
| `AffiliatedStaMetrics` | Affiliated STA Metrics (0xE4) | §17.2.100, Table 122 |
| `AffiliatedApMetrics` | Affiliated AP Metrics (0xE5, spec only) | §17.2.101, Table 123 |
| `TidToLinkMappingPolicy`, with `TidToLinkScope` and `TidToLinkMapping` | TID-to-Link Mapping Policy (0xE6) | §17.2.102, Table 124 |
| `EhtOperations`, with `EhtOperationsRadio` and `EhtOperationsBss` | EHT Operations (0xE7) | §17.2.103, Table 125 |

- Byte orders: every multi-octet field is big-endian, except the `Num_Mapping` of the TID-to-Link
  Mapping Policy (0xE6). Upstream writes that count in host order with no `htons`, so it is
  little-endian here (`em_configuration.cpp:1674`, audit row 19; note 0008 D6). `Expected_Duration`
  is 3 octets, big-endian. The counters of 0xE4 and 0xE5 are 4 octets, big-endian.
- "Valid" flags never change the layout. The field each flag qualifies is always on the wire and is
  kept as found. Reserved values are kept raw (for example Link ID 16 to 255 on 0xE0). Reserved bits,
  octets and blocks are ignored on decode and written as zero (audit rows 285, 286 and 290).
- Counts are the list lengths. Zero counts are accepted, except that the BSS Configuration Request
  and Response values need at least one octet, so an empty value is `Truncated`. SSIDs are raw
  bytes of 0 to 255 octets (0xB7, 0xE0). Encoders write only what the strict decoder accepts: a
  count or SSID above 255 is `EncodeError::CountTooLarge`, a Direction or Group Addressed BU
  Indication Exponent above 3 is `EncodeError::ValueTooLarge`, an `Expected_Duration` above
  `0xFF_FFFF` is `ValueTooLarge`, and an empty 0xBB or 0xBD value is `CountTooSmall`.
- TID-to-Link Mapping Policy (0xE6): `is_bSTA_Config` 0 carries the AP MLD mappings. `is_bSTA_Config`
  1 is the bSTA MLD form, which has `Num_Mapping` zero. `TidToLinkScope::BstaMld` holds no mappings,
  so a bSTA policy with mappings cannot be encoded. Decoding a nonzero `Num_Mapping` with
  `is_bSTA_Config` 1 is `DecodeError::ValueOutOfRange` with the count (note 0009 A8). A mapping is
  the add octet, the STA MLD MAC, the control octet, the presence indicator, the 3-octet
  `Expected_Duration` (always present), 2 raw octets for each set bit of the presence indicator in
  wire order, and 7 reserved octets. The bits are not mapped to TIDs here.
- 0xBB and 0xBD are raw bytes for `em-easyconnect` (note 0009 D5). No DPP attribute framing or JSON
  is parsed.
- 0xE4 and 0xE5: the octets after the counters are the spec's reserved expansion. Decoding ignores
  them, of any length, without `TrailingBytes`, and encoding writes none.
- 0xE7: 32 reserved octets come before the radios, 25 after each radio's BSSs and 16 after each BSS.
  The Basic EHT-MCS And NSS Set, Control, CCFS0, CCFS1 and the Disabled Subchannel Bitmap are raw
  octets.
- 0xE3 and 0xE5 are spec only: upstream has no type code, sender or receiver (audit rows 293 and 295).
- Lenient form (`TlvBodyLenient`): the empty value of 0xE1 decodes to `Ok(None)`, which is upstream's
  "no bSTA MLD" form. The strict decoder rejects it with `Truncated`. There is no other lenient form,
  and in particular none for an upstream TID-to-Link mapping with a nonzero presence indicator (user
  decision).
- Flag-alone tests: the flag octets of 0xB7, 0xE0, 0xE1, 0xE2, 0xE6 and 0xE7 are tested one flag at a
  time with `assert_each_flag_alone`, which is local to `bodies::mld`.

Intentional differences from upstream (B10):

- 0xB7: upstream sends the SSID length as `strlen + 1` and the trailing NUL as the last SSID octet
  (`em_configuration.cpp:1149-1155`, audit row 229). This body keeps those octets as raw SSID bytes.
- 0xE0: upstream always writes SSID Length 32 and a 32-octet SSID padded with NULs
  (`em_configuration.cpp:1393-1394`, audit row 279), and this body keeps those octets as the SSID.
  Upstream's receiver steps over each AP MLD by the fixed `sizeof(em_ap_mld_t)`
  (`em_configuration.cpp:2854`, audit row 278); this body steps by the length on the wire.
- 0xE1: upstream sends the TLV with an empty value when no bSTA MLD is configured
  (`em_configuration.cpp:1439-1442`, `3771-3773`, audit row 287). The lenient decoder reads it as
  `Ok(None)`, and the strict decoder rejects it.
- 0xE2: when the affiliated count does not fit the value, upstream logs "Malformed" and clamps the
  count (`em_configuration.cpp:2637-2644`, audit row 291). This body returns `Truncated`.
- 0xE6: upstream writes every mapping as a fixed 19-octet record and never writes the TID-to-Link
  entries (`em_configuration.cpp:1687-1690`, audit row 18). A mapping with a nonzero presence
  indicator therefore does not decode as spec bytes, and there is no lenient form (user decision).
  Upstream also copies `Num_Mapping` for a bSTA MLD without forcing it to zero
  (`em_configuration.cpp:1674`, audit row 22); the strict decoder returns `ValueOutOfRange` (A8).
- 0xE7: upstream's receiver rejects the whole TLV with more than 4 radios or more than 16 BSSs in a
  radio (`em.cpp:2163-2166`, `em.cpp:2183-2186`). Those are storage limits of upstream's data model,
  so this body accepts every count from 0 to 255.

## DPP, security and encapsulation bodies (B11)

Design: [note 0009](../../docs/0009-tlv-bodies.md), D2 to D5, D8, D9 and A8. The bodies are in
`bodies::security`. Each doc comment cites the spec section and table, the audit rows, and the
upstream struct, sender and receiver lines.

| Body | TLV (type) | Spec |
|---|---|---|
| `SourceInfo` | Source Info (0xC0) | §17.2.54, Table 76 |
| `TunneledMessageType` | Tunneled Message Type (0xC1) | §17.2.55, Table 77 |
| `Tunneled<'a>` | Tunneled (0xC2) | §17.2.56, Table 78 |
| `Ieee1905LayerSecurityCapability` | 1905 Layer Security Capability (0xA9) | §17.2.67, Table 89 |
| `Mic<'a>` | MIC (0xAB) | §17.2.68, Table 90 |
| `EncryptedPayload<'a>` | Encrypted Payload (0xAC) | §17.2.69, Table 91 |
| `AkmSuiteCapabilities`, with `AkmSuite` | AKM Suite Capabilities (0xCC) | §17.2.78, Table 100 |
| `Ieee1905EncapDpp<'a>` | 1905 Encap DPP (0xCD) | §17.2.79, Table 101 |
| `Ieee1905EncapEapol<'a>` | 1905 Encap EAPOL (0xCE) | §17.2.80, Table 102 |
| `DppBootstrappingUriNotification<'a>` | DPP Bootstrapping URI Notification (0xCF) | §17.2.81, Table 103 |
| `DppCceIndication` | DPP CCE Indication (0xD2) | §17.2.82, Table 104 |
| `DppChirpValue<'a>` | DPP Chirp Value (0xD3) | §17.2.83, Table 105 |
| `DppMessage<'a>` | DPP Message (0xD1) | §17.2.86, Table 108 |

- Byte orders: every multi-octet integer is big-endian (§3.1.2). The 6-octet counters of 0xAB and
  0xAC are `Counter48`, which refuses a value above 2^48 - 1. The 2-octet lengths of 0xAB, 0xAC and
  0xCD are big-endian, and so is the 0xCD frame length. The 0xD3 Hash Length is one octet (note
  0008 D8), not the 2 octets upstream writes.
- Raw bytes (note 0009 D5): the Tunneled body, the EAPOL frame, the DPP frame, the DPP URI, the MIC,
  the AES-SIV output and the 0xD3 hash are borrowed from the value. Nothing is parsed, decrypted or
  checked here, and the MIC is not computed.
- Derived lengths: encoders derive the 2-octet lengths of 0xAB, 0xAC and 0xCD, and the 0xD3 Hash
  Length, from the data. A length above 65535 (above 255 for 0xD3) is `EncodeError::CountTooLarge`.
  The AKM counts come from the lists, and a count above 255 is `CountTooLarge`.
- Layout flags: the MAC-present bit of 0xCD and 0xD3 decides whether the Destination STA MAC is on
  the wire. Encoders set the bit from the `Option`, so the bit and the field cannot disagree. The
  GAS bit of 0xCD and Hash Validity of 0xD3 never change the layout: the 0xD3 Hash Length and Hash
  Value are always present (audit row 248).
- Reserved values are kept raw: 0xC1 values 7 to 255, 0xA9 values 1 to 255 in each field, MIC
  Version 1 to 3, 0xD2 values 2 to 255, and the 0xCD frame type. Reserved bits are ignored on decode
  and written as zero. The 0xCC counts take 0 to 255 with no cap.
- Empty values are spec-valid where the table has no "one or more" rule: the Tunneled body, the
  EAPOL frame, the DPP URI, the DPP frame, the MIC and AES-SIV output, the 0xD3 hash, and the AKM
  lists. Fixed-size bodies (0xC0, 0xC1, 0xA9, 0xD2) are exact.
- Lenient form (`TlvBodyLenient`): the 0xD3 decoder also accepts upstream's 2-octet Hash Length. After
  the flags and the optional MAC, the form is a big-endian 2-octet length above 0 that equals the
  octets left, then the hash. It gives the same body as the spec form when the hash is at most 255
  octets. The length is read in full, so a longer hash (up to 65535 octets) also decodes, and that
  body cannot be encoded again (`CountTooLarge`). The strict decoder rejects the form with
  `TrailingBytes`. There is no absent form, so the lenient result is never `Ok(None)` (user
  decision).
- Flag-alone tests: the flags of 0xCD and 0xD3 are each tested on their own
  (`encap_dpp_flags_each_use_their_own_bit`, `chirp_flags_each_use_their_own_bit`), and the GTK Key Id
  and MIC Version of 0xAB are each tested on their own (`mic_key_id_and_version_each_use_their_own_bits`).
  These tests are local to `bodies::security` and do not use `assert_each_flag_alone`.
- No upstream sender or receiver exists at the pin for 0xC0, 0xC1, 0xC2, 0xAB and 0xAC (audit rows
  188 to 190, 214 and 215), and 0xCF has no production sender (audit row 245). These bodies follow the
  spec.

Intentional differences from upstream (B11):

- 0xCC: the upstream receiver keeps at most 10 AKMs per BSS (`em_configuration.cpp:5095,5105`, audit
  row 240). This body keeps every listed suite. The receiver also drops a suite whose selector it does
  not map (`em_configuration.cpp:5007-5012`) and takes a BSS's AKMs from its network profile
  (`:5043-5090`). This body does neither.
- 0xCD: the upstream receiver accepts a frame length up to 3 above the data (`ec_util.cpp:402`, audit
  row 242). With bit 7 set and fewer than 6 octets left it zeroes the MAC and reads on
  (`ec_util.cpp:381-388`, audit row 243). This body returns `Truncated` in both cases.
- 0xCE: the upstream sender (`em_provisioning.cpp:183`) and receiver (`em_provisioning.cpp:392-395`,
  audit row 244) refuse a zero-length EAPOL frame. This body accepts an empty value.
- 0xD1: the upstream receiver requires a non-zero frame length (`em_provisioning.cpp:540`, audit row
  257). This body accepts an empty frame.
- 0xD2: the upstream receiver treats every value but 1 as an error, so a Disable is never acted on
  (`em_provisioning.cpp:416-421`, audit row 246). This body keeps the octet raw.
- 0xD3: upstream writes and reads the Hash Length as 2 octets, big-endian (`ec_util.cpp:337,356-358`,
  receiver `:300-302`, audit row 247). The strict decoder rejects that form, the lenient decoder accepts
  it, and the encoder writes the spec form.
- 0xD3: the upstream receiver ignores the hash when Hash Validity is 0 (`ec_util.cpp:295-298`) and
  rejects a Hash Length of 0 when Hash Validity is 1 (`ec_util.cpp:303-307`; audit rows 248 and 250).
  This body keeps the hash for both validities and accepts a length of 0.

## WSC attributes

The `wsc` module walks and writes the attributes of a WSC TLV value (2-byte id, 2-byte length,
value, all big-endian):

- `WscAttrIter` yields `Result<WscAttr, DecodeError>`. Empty input yields nothing. A header or
  value cut short is `DecodeError::Truncated`, and nothing is yielded after an error. A zero-length
  attribute is yielded with an empty value.
- `write_wsc_attr(&mut Writer, id, value)` writes one attribute. A value longer than 65535 bytes is
  `EncodeError::CountTooLarge { len, max: 65535 }`, and nothing is written.
- `wsc_msg_type` keeps upstream's quiet behaviour.

Typed attributes (note 0009, D5 and A9): `WscAttribute<'a>` has one variant for each of the 32
attributes that upstream's M1, M2 and Encrypted Settings plaintext build or read, and `Other` for any
other id. `WscAttribute::decode` takes a `WscAttr` from the walker, and `encode` writes the attribute
back with `write_wsc_attr`. The ids are the `WscAttrId` constants.

- One-octet values: Version, Message Type, Connection Type Flags, WSC State and RF Bands.
- Big-endian `u16` values: Authentication Type Flags, Encryption Type Flags, Configuration Methods,
  Association State, Device Password ID, Configuration Error, Authentication Type and Encryption Type.
  Encryption Type Flags, Configuration Methods and Configuration Error are big-endian by decision
  (A9). Upstream copies the first two in host order, and writes Association State, Device Password
  ID and Configuration Error with no value octets (intentional differences, tested per attribute).
- Fixed raw arrays: UUID-E and UUID-R (16 octets), MAC Address (6), the two nonces (16), Primary
  Device Type (8), OS Version (4), Authenticator and Key Wrap Authenticator (8).
- Variable values, borrowed without a copy: Public Key, Manufacturer, Model Name, Model Number,
  Serial Number, Device Name, SSID, Network Key and Encrypted Settings. SSID and Network Key keep the
  trailing NUL that upstream sends.
- A fixed-width attribute of any other length is `DecodeError::InvalidLength`. Upstream ignores or
  overruns these lengths (intentional differences, tested per attribute).
- Vendor Extension (0x1049): `WscVendorExtension` is the 3-octet vendor id and the rest of the value.
  `MultiApExtension` reads and writes the Wi-Fi Alliance sub-element 0x06 (one flag octet). Reserved
  bits are ignored on decode and written as zero. Upstream writes this sub-element in the Encrypted
  Settings and never reads it. `HaulTypeExtension` reads and writes the Comcast haul type (sub-id
  0x01). It writes upstream's 11-octet value (5 meaningful octets, then 6 zeros) and reads sub-id 0x01
  from a value of 5 or more octets. Other Comcast sub-ids are `Ok(None)` at any length. A Comcast
  value with no sub-id, or with sub-id 0x01 and no haul type octet, is `DecodeError::InvalidLength`.
  This is the only Comcast sub-id typed here. Comcast sub-ids 0x02 to 0x06
  (`vendor_ext_attr_id_t`) are used by other TLVs (the Vendor Specific policy and metrics code),
  not by WSC, and `em-core` owns them (note 0009 A11). Other vendor ids are returned as
  `WscVendorExtension` without interpretation.
- A variable-length value longer than 65535 octets is `EncodeError::CountTooLarge`, and nothing is
  written.
- Encryption, authentication and key wrap are not computed here. They belong to `em-core` with
  `em-crypto` (note 0006).

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
    that the lenient decoder accepts on purpose: the empty values, the nine-octet Associated Wi-Fi 6
    STA Status Report (metrics group), and Device Inventory with zero radios in the capability group.
  - `lenient` is rejected by the strict decoders, decodes leniently through both lenient entry
    points to the same body, and re-encodes to bytes that the strict decoder reads back as that body.
    A `lenient` row marked `decoded=absent` (the Backhaul STA empty value, and the empty values of
    0x8A, 0xB5 and 0xC4 in the policy group) decodes to `Ok(None)` through both lenient entry
    points instead. Its value must be empty, so a lenient decoder cannot hide a non-empty body
    behind `Ok(None)`. The capability tests, and the `lenient_absent:` item of `body_checks!`,
    check it with `check_lenient_absent`.
- Flag-alone tests: the flags of a bit-field body are tested one at a time. Each test sets one
  flag alone in an otherwise zero value, and checks that it decodes to its own field and encodes
  back to the same bytes. A fixture row that sets several flags together cannot show that two
  flags are not swapped. The `assert_each_flag_alone` helper is in `bodies::capability`,
  `bodies::cac`, `bodies::steering`, `bodies::policy` and `bodies::mld`; `bodies::channel` and `bodies::metrics`
  have per-flag tests written out.
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
  `autoconfig` group 137, the `capability` group 167, the `channel` group 155, the `cac` group 140,
  the `metrics` group 166, the `steering` group 159, the `policy` group 165, the `mld` group 275 and the `security` group 213.
- `upstream_tlv_vectors` (test only) loads `tests/data/upstream_tlv_vectors.tsv`: 31 rows from seven
  upstream senders. It checks the exact case list and decodes and re-encodes each row through the
  body of its sender. The two rows of the unreachable empty-value guards are not decoded; a test
  checks that their bodies reject an empty value.
- `upstream_reader_vectors` (test only) loads `tests/data/upstream_reader_vectors.tsv`: 1421 rows
  from the field readers and walkers run on 73 TLV areas. An upstream `true` must be `Ok(Some)` with
  the same output. An upstream `false` must be `Ok(None)`, except for the pinned error rows, which
  must be exactly their `ReaderError`. `find_tlv` is compared by the position of its value, and the
  walkers by offset and TLV.
- `tests/data/upstream_test_map.tsv` (note 0009 A10) maps each reader to its Rust test. `get_tlv(em_tlv_t *)`
  and `get_client_mac_info` are recorded as `not ported: no production caller`. Each
  `intentional-difference:<id>` tag in `readers.rs` has a row naming its test. `tools/check-test-map`
  checks that every named test exists and that the tags in the source and the map agree. It also
  checks the map's shape: every disposition is `rust:<path>` or `skip:<reason>`, all 15 upstream
  reader rows are present with the two `not ported` rows unchanged, and a tagged row is a `rust:`
  reference whose test body contains its tag. Python tests: `tools/test_check_test_map.py`.
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
- `tools/upstream-tlv-vectors/regen` compiles the pinned upstream senders and `em_msg_t` readers
  against a declaration shim and writes `tests/data/upstream_tlv_vectors.tsv` and
  `tests/data/upstream_reader_vectors.tsv`; `--check` rebuilds both in memory. The
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
- Field readers (`readers`, note 0009 D6 and A10). Each difference has an id, which is its
  `intentional-difference:<id>` tag in `readers.rs`. The test with that tag is named after the
  upstream behaviour, and the test map lists it.
  - `al-mac-length`: an AL MAC Address value must be exactly 6 octets. Upstream copies `htons(len)`
    octets into a 6-octet buffer.
  - `bss-id-short-read`, `radio-id-short-read`: a value too short for the 6-octet read is
    `ValueTooShort`. Upstream reads past the value.
  - `channel-scan-request-short`: a Channel Scan Request value shorter than 9 octets fails the whole
    `radio_id` walk, before the End of Message test. The exact 2-octet form with 0 radios is spec
    valid (§17.2.39) and is `Ok(None)`.
  - `freq-band-empty`: an empty Supported or Autoconfig Frequency Band value is `ValueTooShort`.
    Upstream copies the first octet of the next TLV's header.
  - `missing-end-of-message`, `framing-error`: a framing error is `ReaderError::Framing`. Upstream
    returns "not found" for a frame without End of Message, and `false` for a TLV that runs past
    the end.
  - `walker-size-wrap`: `next_tlv` does not wrap the TLV size at 0xFFFD to 0xFFFF, as upstream's
    `uint16_t` does.
  - `walker-zero-length-stop`: not a fix. `first_tlv` and `next_tlv` stop at a zero-length TLV, End
    of Message included, as upstream does (user decision, A10). Two data-loss cases are tested: an
    empty Backhaul STA MLD Configuration TLV hides the EHT Operations TLV after it from the walks of
    BSS configuration, and an empty first TLV of a Backhaul STA Capability Report makes both of its
    lookups fail. `TlvIter` does not stop at a zero-length TLV, so `find_tlv` still finds the EHT
    Operations TLV.
  - `sta-mac-short-value`: a `get_sta_mac` value below its minimum is `ValueTooShort`. Upstream
    returns `false` (`em_msg.cpp:197-211`).
  - `supported-service-rejected`: an empty Supported Service value, a length other than `1 + num`,
    or a count above 8 is an error that gives the reason. Upstream returns `false`
    (`em_msg.cpp:104-117`).
  - `profile-rejected`: a Profile value whose length is not 1 or 4, or a raw value of 4 or more, is
    an error that gives the reason. Upstream returns `false` (`em_msg.cpp:139-148`). The A10 vector
    rows pin the Profile and Supported Service cases.
  - Malformed values that upstream rejects with `false` are a `ReaderError` here, not `Ok(None)`.
    Each of the three tags above is one such case.

## Status

Ported. The crate holds the CMDU header, TLV framing, builder and validation, every TLV body of
note 0009 (batches B01 to B11), the typed WSC attributes (B12) and the field readers and walker
(B13). B10 to B13 had two review rounds; see [STATUS.md](../../STATUS.md) for the results and the
low items left open.
