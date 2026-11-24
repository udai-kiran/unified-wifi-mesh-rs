# em-db

The first `em-db` code loads the `NetworkSSIDList` from an upstream-shaped `Reset.json` for the
controller's in-memory configuration ([0013 D2](../../docs/0013-s2-onboarding.md)). SQLite,
other tables, and persistence are outside this slice. `em-ctrl` calls the path loader at
startup.

`load_reset_file(path)` and `load_reset_str(json)` return `Result<Vec<Box<NetworkSsidInfo>>,
ResetError>`, preserving row order. The boxes are the `em-dm` large-record handoff. Missing files,
read failures, malformed JSON, invalid field values, and model bounds violations return typed
errors; field errors identify the row and field without including the rejected value, which may
be a credential. The path loader does not choose a default path or apply the configuration.

The upstream baseline is `rdkcentral/unified-wifi-mesh` @ `ba4180b`:

- `src/ctrl/dm_easy_mesh_ctrl.cpp:10811-10824,10841-10867` bootstraps `GLOBAL_NET_ID` and
  applies only the reset file's SSID list, not its network record. The file's `ID`,
  `ControllerID`, `ColocatedAgentID`, `Interfaces`, `NumberOfDevices`, and `TimeStamp` are
  ignored here, as are unknown fields.
- `src/dm/dm_easy_mesh.cpp:862-864,894-912,951-973` selects `wfa-dataelements:Reset` and
  its non-empty `NetworkSSIDList` and decodes rows in array order.
- `src/dm/dm_network_ssid.cpp:45-108` zeros each row before reading its optional fields, keeps
  the `Band` and `AKMsAllowed` strings in order, converts every `HaulType`, and constructs an
  ID from the first haul. Object lookups use cJSON's case-insensitive lookup, so both
  `PassPhrase` and `Passphrase` are accepted. Duplicate keys use the first matching key.
- `src/dm/dm_network_ssid.cpp:222-269` maps haul names by a case-sensitive prefix comparison
  against the input length; even an empty string matches `Fronthaul`. An unknown name maps to
  the upstream `em_haul_type_max` value (5), formatted as `Unknown` in the row ID. If the haul
  list is absent or empty, the zeroed first haul produces `Fronthaul` in the ID.
- `src/dm/dm_network_ssid.cpp:87-90` and `src/dm/dm_easy_mesh.cpp:2070-2080` decode the
  mobility domain as a MAC address; both the colon-separated and compact hexadecimal forms
  are supported.

Deliberate differences are tested using the corresponding `intentional-difference` tags:

- `reset-global-id`: while it never applies the network record, upstream's reset decoder uses
  the file's `ID` to construct each SSID ID (`src/dm/dm_easy_mesh.cpp:951-973`,
  `src/dm/dm_network_ssid.cpp:108`, `src/ctrl/dm_easy_mesh_ctrl.cpp:10861-10867`). D2 instead
  ignores that ID completely and always constructs `<first-haul>@OneWifiMesh`. The loader also
  does not require or parse `Interfaces`, which upstream checks at
  `src/dm/dm_easy_mesh.cpp:914-917` before processing the list.
- `reset-file-required`: upstream logs and continues when the file cannot be loaded
  (`src/ctrl/dm_easy_mesh_ctrl.cpp:10856-10858`); the loader returns the read error to its caller,
  as required by 0013 D2.
- `reset-invalid-document`: upstream's reset decoder reports malformed JSON, a missing list, or
  an empty list (`src/dm/dm_easy_mesh.cpp:903-912,953-963`), but the bootstrap caller ignores
  that return value (`src/ctrl/dm_easy_mesh_ctrl.cpp:10861-10867`). The loader returns the error,
  and requires the whole input to be JSON rather than accepting trailing non-JSON content.
- `reset-bounds`: instead of upstream's truncating `snprintf` and unchecked array writes
  (`src/dm/dm_network_ssid.cpp:45-59,67-71,75-84,92-106`,
  `src/dm/dm_easy_mesh.cpp:959-973`), inputs exceeding model string, list, or five-row bounds are
  errors. The model's SSID limit is 32 bytes; its exact-limit acceptance is covered by
  `reset-ssid-32` (`src/dm/dm_network_ssid.cpp:46-47`, 0007 A1).
- `reset-invalid-values`: embedded NULs, incorrect JSON types, malformed mobility-domain MACs,
  and VLAN IDs other than integers in `0..=65535` are errors. Upstream uses C strings, cJSON
  coercions, unchecked `sscanf`, and a numeric cast instead (`src/dm/dm_network_ssid.cpp:45-106`,
  `src/dm/dm_easy_mesh.cpp:2070-2080`).

The exact upstream fixture is in `tests/data/Reset.json`; its source and pin are recorded in
`tests/reset.rs`. No fixture credentials are included in loader error messages.
