# em-msg

IEEE 1905 CMDU header, TLV framing, message builder and message validation (spec §17).
Synchronous, depends only on `em-types`, no `unsafe`, no clones.

## What is here

| Module | Holds | Upstream |
|---|---|---|
| `header` | 8-byte CMDU header, `default_relay` | `em_cmdu_t` (`em_base.h`), `add_1905_header` (`em_msg.cpp`) |
| `tlv` | `TlvIter`, a borrowing walk over the TLV area | `em_tlv_t`, `get_tlv`, `get_first_tlv`, `get_next_tlv`, `validate` walk (`em_msg.cpp`) |
| `builder` | `MsgBuilder` with a size budget, End of Message added by `finish` | `em_msg_builder_t` (`em_msg.h`, `em_msg.cpp`), `add_tlv` |
| `wsc` | WSC Message Type selector (M1 = 4, M2 = 5) | `em_configuration_t::get_wsc_msg_type` (`em_configuration.cpp`) |
| `tables` | The 70 validation tables as static data | the table functions `autoconfig_search` .. `i1905_ack` (`em_msg.cpp`) |
| `validate` | `validate(cmdu, profile)` | `em_msg_t::validate` (`em_msg.cpp`) |
| `error` | `FramingError`, `BuildError` | - |

Design: [note 0005](../../docs/0005-em-msg-scope.md). The tables equal the golden fixture
`tests/data/upstream_msg_tables.tsv`, which is generated from upstream by `tools/gen-upstream-msg-tables`.

Upstream source: `src/em/em_msg.cpp`, `inc/em_msg.h`, and `get_wsc_msg_type` in
`src/em/config/em_configuration.cpp`. Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.

## What is not here

- The Ethernet header: `em-transport` (it keeps the MAC addresses next to the CMDU buffer).
  `CmduHeader::parse` and `validate` take the whole CMDU (from the 8-byte header); `TlvIter::new`
  and `wsc_msg_type` take the TLV area after the header; `MsgBuilder` produces the whole CMDU.
- TLV bodies and the field readers (`get_radio_id`, `get_profile`, ...): ported later, each with
  the `em-core` area that uses it.
- The policy for a received Profile TLV: `em-core`.
- SDU fragmentation and CMDU reassembly.

## Intentional differences from upstream

- Validation is stateless: no presence flags or error counters survive between calls (upstream
  defect 1), and lengths are checked before subtracting (defect 2).
- Errors are returned as values, not printed.
- The size budgets count the CMDU only: 1500 bytes (raw socket) and 65517 bytes (AL-SAP).
- Stricter checks (length of every instance, length of optional TLVs) are not done, because
  upstream's own senders would fail them (note 0005 D3).

## Status

Step 2 (crate) implemented; see [STATUS.md](../../STATUS.md).
