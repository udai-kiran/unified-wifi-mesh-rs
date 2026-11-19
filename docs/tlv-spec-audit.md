# TLV spec audit: upstream versus EasyMesh and IEEE 1905.1

| | |
|---|---|
| Date | 2026-11-19 |
| Upstream | `rdkcentral/unified-wifi-mesh` @ `ba4180ba36748629f10c20babc4320dd69c9db4f` |
| Specs | Wi-Fi EasyMesh §17.2 (105 TLVs), IEEE 1905.1 §6.4 (19 TLVs), read through the project's spec server |
| Data | [`data/tlv-spec-audit.tsv`](data/tlv-spec-audit.tsv): 343 rows, one per finding: 288 differences and 55 `OK` rows. Each row has the spec text, upstream behaviour, file:line evidence, and for each difference the port decision (`port_rule`, `port_rule_reason`) |
| Status | Findings, with a port decision on every non-OK row (`port_rule`). The user's rule ([0008](0008-tlv-body-codec.md) D7, D8): where the spec is explicit the port follows the spec; where it is silent the port keeps upstream's bytes (0008 D6) |

## Method

1. Two independent extractions of every TLV table from the spec. Fast agents wrote
   one row per field: size, bit range, repeat or presence condition, and values.
2. A script compared the two extractions per row. After normalising wording, all 124 TLVs
   agree on every field size, bit position and repeat. The 11 remaining wording differences were
   read by the coordinator; none is a layout difference.
3. One stronger agent per batch of four TLVs compared the agreed spec rows with the pinned
   upstream sender and receiver. It recorded field order, sizes, bit positions (GCC packs the first
   declared bit-field into the least significant bit), byte order, counts, presence conditions
   and validation, with file:line evidence.
4. The coordinator re-checked every high-severity finding against the code. Each one was read
   at the cited lines, or the agent quoted the lines; the coordinator read most of them directly.

Limits: the comparison reads code; it does not run it. Rows marked `UNSURE` in the notes need a
second look before a consumer relies on them. Medium and low rows were not re-checked one by one.

## Summary

| Class | High | Medium | Low |
|---|---:|---:|---:|
| BIT_LAYOUT | 2 | 1 | 5 |
| BYTE_ORDER | 1 | 7 | 2 |
| FIELD_LAYOUT | 14 | 34 | 41 |
| VALIDATION | 1 | 58 | 76 |
| NOT_IMPLEMENTED | 0 | 2 | 44 |
| OK (no difference) | | | 55 |

All 124 type codes match the spec, except that 0xE3 (MLD Structure) has no code at the pin.
Upstream has no sender and no receiver for 46 TLVs. Most validation findings are receivers
that trust a length or count read from the wire, such as unchecked `tlv->len`, counts not
bounded by arrays, or unsigned length subtraction that wraps. The Rust port gets these checks
from `Reader` (0008 D1); they matter for interop only where upstream accepts what the spec
forbids, or rejects what it allows.

## High-severity findings (all checked against the code)

| TLV | Class | Upstream does | Spec says | Evidence |
|---|---|---|---|---|
| 17.2.10 AP HE Capabilities | BIT_LAYOUT | second octet packed one bit low: DL OFDMA bit 0 … SU beamformer bit 6, bit 7 never set | SU beamformer bit 7 … DL OFDMA bit 1, bit 0 reserved | `em.cpp:1443-1451` |
| 17.2.10 AP HE Capabilities | FIELD_LAYOUT | receivers cast to fixed `em_ap_he_cap_t` (12 MCS octets), so the flags are read at the wrong offset when the MCS field is shorter; also over-reads | MCS field is `k` octets (4–12) | `em_capability.cpp:1584-1595, 1863-1873` |
| 17.2.46 CAC Capabilities | FIELD_LAYOUT | early-report receiver casts to fixed `em_cac_cap_t`; only the first radio's RUID and count line up; over-reads about 3.4 KB | nested variable-length lists | `em_capability.cpp:1611-1633` (full receiver and sender are correct) |
| 17.2.102 TID-to-Link Mapping Policy | FIELD_LAYOUT | TID-to-Link_Mapping octets never written (TODO), fixed 19-octet records | 2 octets per set bit of the presence indicator | `em_configuration.cpp:1687-1690` |
| 17.2.4 AP Operational BSS | FIELD_LAYOUT | `ssid_len = strlen + 1`, NUL sent; a 32-byte SSID goes out as 33 | SSID length is the SSID's octets | `em_configuration.cpp:1100-1103` |
| 17.2.75 BSS Configuration Report | FIELD_LAYOUT | same `strlen + 1` | same | `em_configuration.cpp:1149-1155` |
| 17.2.75 BSS Configuration Report | BIT_LAYOUT | flags and reserved octets never written; uninitialised stack in Topology Response | backhaul/fronthaul/R1/R2/multi-BSSID/transmitted flags | `em_configuration.cpp:1143-1157` |
| 17.2.7 AP Radio Basic Capabilities | FIELD_LAYOUT | pointer advances for every matching-RUID op class but `len` only for capability ones: holes, last groups beyond the TLV length | contiguous `Num_OpClass` groups | `em.cpp:1105-1130` |
| 17.2.9 AP VHT Capabilities | BYTE_ORDER | Tx/Rx MCS maps memcpy'd in host (little-endian) order | "reordered … into big-endian order" | `em_base.h:1891-1904`, `em.cpp:1388` |
| 17.2.27 Beacon Metrics Query | FIELD_LAYOUT | `Num_Ids` omitted unless Reporting Detail = 1 with elements | `Num_Ids` always present (0 otherwise) | `em_metrics.cpp:1885-1892` |
| 17.2.73 Associated Wi-Fi 6 STA Status Report | FIELD_LAYOUT | writes nothing, returns 9: zero MAC, Num_TID 0, 2 stray octets, in every Profile-3 AP Metrics Response | MAC, Num_TID, TID/queue pairs | `em_metrics.cpp:2299-2306` |
| 17.2.83 DPP Chirp Value | FIELD_LAYOUT | Hash Length read and written as 2 octets, big-endian | 1 octet | `ec_util.cpp:300-302, 337, 357` |
| 17.2.96 Agent AP MLD Configuration | FIELD_LAYOUT | receiver steps entries by `sizeof(em_ap_mld_t)` (fixed 32-octet SSID) while the SSID is `ssid_len` octets: short SSIDs misparse every later field | SSID is `SSID Length` octets | `em_configuration.cpp:2772, 2776, 2792, 2803, 2854` |
| 17.2.96 Agent AP MLD Configuration | FIELD_LAYOUT | AP MLD Configuration Request/Response: value written but no type/length; callers skip a header that was never written | TLV with header | `em_configuration.cpp:1916-1920, 2059-2063` |
| 17.2.96 Agent AP MLD Configuration | FIELD_LAYOUT | BSS Configuration Response/Result append the value only (`add_buff_element`) | TLV with header | `em_configuration.cpp:3768-3769, 3802-3805` |
| 17.2.104 Available Spectrum Inquiry Request (and 17.2.105 Response) | FIELD_LAYOUT | length = `sizeof` a pointer struct (8), value never written: uninitialised bytes, no JSON | JSON object | `em_channel.cpp:1649-1663` |
| 17.2.85 BSS Configuration Response | FIELD_LAYOUT (UNSURE) | bare JSON string | "One or more JSON encoded DPP Configuration Object attributes (see 8.1 of [18])"; the request TLV wraps its JSON in an attribute. The spec wording is ambiguous | `em_configuration.cpp:4063-4068, 5322` |
| 6.4.3 AL MAC Address | VALIDATION | `memcpy(mac, value, tlv->len)` into a 6-byte buffer: stack overflow from a peer-supplied length | 6 octets | `em_msg.cpp:78-80` |

## Port decisions

The port decisions follow 0008 D7 and D8. Each of the 288 rows that differ from the spec has a `port_rule`:

| port_rule | Rows | Meaning |
|---|---:|---|
| SPEC | 268 | The spec settles the point; the port follows it |
| UPSTREAM | 5 | The spec is silent; the port keeps upstream's bytes |
| UNSURE | 15 | The spec text can be read two ways, or defers to a document not available; the consumer decides and records it in its note |

How the rules were assigned:
- **First pass:** six parallel agents decided the rows from the spec text, reading sections
  where needed.
- **Second pass:** three agents re-read every non-SPEC row under sharper rules:
  - doubt about upstream's own code does not make a row UNSURE;
  - a count whose field width is given but which has no smaller maximum is SPEC, so the port
    accepts the full range;
  - UPSTREAM only where the spec has no text at all.
- **Coordinator, then review round 1:** the coordinator changed rows 76, 114, 136, 176 and 192;
  review rounds 1 and 2 changed rows 3, 200, 203, 221, 258, 305, 311, 332, 333, 337 and 340;
  the em-msg minimum-length review changed rows 314, 321 and 340, and the text of rows 320 and 324.
  - **76:** a count with no minimum is SPEC.
  - **114 and 192:** TLVs are found by type, not position.
  - **136:** reserved bits are ignored (EasyMesh §3.1.2).
  - **176 and 305:** every read is bounds-checked through `Reader`, and no minimum beyond the spec
    is required.
  - **258:** not a wire point; the port does not copy upstream's inverted success check.
  - **311:** EasyMesh Table 14 (§6) extends 1905.1 Table 6-12 with `0x0108` (Wi-Fi 6) and
    `0x0109` (Wi-Fi 7), both with n=0. Upstream's `0x0108` is valid; its 10 media-specific octets
    are not.
  - **332:** EasyMesh Table 15 (§6) extends AutoconfigFreqBand (1905.1 Table 6-23) with `0x03` = 6
    GHz. Upstream's `0x03` is valid; `0x04` ("unknown") is reserved, and upstream can send it.
  - **337:** Table 15 does not extend SupportedFreqBand (Table 6-25), so 6 GHz there is UNSURE.
    An informative NOTE in §6.1 expects the Controller to echo the AutoconfigFreqBand value, which
    favours `0x03`.
  - **314:** Table 6-10 repeats the interface group "k – 1 times", so one interface is always
    present. Upstream's 19-octet minimum is the spec minimum.
  - **321, 340:** see UNSURE below.
  - The finding text of rows 311, 314, 332 and 337 over-states the defect.

**UPSTREAM (5):** byte order the TLV table does not name (D6 keeps upstream's bytes):
- CAC Duration (row 8, big-endian upstream);
- `Steering_Opportunity_Window` (12, little-endian);
- `BTM_Disassociation_Timer` (15, big-endian);
- TID-to-Link `Num_Mapping` (19, little-endian);
- Metric Collection Interval (195, little-endian).

**UNSURE (15):**
- HE and Wi-Fi 6 MCS set order, and MCS NSS Length meaning, which are defined only in IEEE 802.11
  (rows 3, 221, 222). Row 3's byte order is still big-endian per the spec.
- TID-to-Link Direction values, from IEEE 802.11be (23);
- the Channel Scan Reporting Policy length (128);
- Fresh=1 with `Num_OpClass` 0 (138);
- VLAN 0 as the Primary VLAN ID (174);
- which SSIDs `Num_SSID` counts (180);
- reporting a BSS outside the requested radios (200);
- which BSSs `Num_BSS` counts (231);
- 1905.1 bridging tuple count `L` (315);
- Link Metric Query length: Table 6-16 fixes 8 octets but omits the EUI-48 for "all neighbors" (321);
- WSC minimum length, which 1905.1 leaves to WSC v2.0.7 Tables 8/9, not available (340);
- the neighbor-device group repeat (317);
- 6 GHz in SupportedFreqBand (337).

Row numbers are line numbers in the TSV, header = line 1.

## Consequences for the port

- **Bit layouts and explicit field layouts follow the spec** (0008 D7, D8). Examples:
  - HE Capabilities second octet;
  - BSS Configuration Report flags;
  - SSID Length without the NUL;
  - 1-octet chirp Hash Length;
  - `Num_Ids` always present;
  - AP MLD TLVs with a header;
  - TID-to-Link mapping octets;
  - big-endian VHT MCS maps;
  - contiguous Radio Basic Capabilities groups;
  - exactly 6-octet AL MAC.
- **Interoperability:** on these TLVs the port's bytes differ from an unpatched upstream peer's.
  Each consumer note records the effect, and any receive-side tolerance it chooses.
- **Note 0007 A1 (AP MLD SSID):** upstream's receiver also misparses every AP MLD entry whose
  SSID is shorter than 32 octets (row above). A1's length rule is unaffected, but the AP MLD
  consumer must step by `SSID Length`, not by the struct size.
- **The 31 IEEE 1905.1 names** in `em-types` (`classification.tsv`, 0001 A2) were checked against
  1905.1:
  - message types against Table 6-4;
  - TLVs against §6.4.1–§6.4.18;
  - media types against Table 6-12.

  All 31 names and values match. Their `spec_reference` texts now cite those sections.
- **Validation:** `Reader` already rejects truncated and over-long values. Consumers add the
  per-TLV count and range checks their rows mark SPEC.
