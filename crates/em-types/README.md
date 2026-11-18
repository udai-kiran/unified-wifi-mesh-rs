# em-types

Shared basics used by more than one crate. Exactly the types listed in
[design note 0001](../../docs/0001-em-types-scope.md), D2:

- `MacAddr`
- `InterfaceName`
- `Interface`
- wire code newtypes: `MsgType`, `TlvType`, `FreqBand`, `MediaType`, `ProfileType`, `ServiceType`

No dependencies and no `unsafe`. Wire encoding lives in `em-msg`, not here.

## Upstream source

The following parts of `inc/em_base.h` (each type's owner is listed in
[`docs/em_base-coverage.md`](../../docs/em_base-coverage.md)):

- `em_interface_name_t`, `em_interface_t`
- `em_msg_type_t`, `em_tlv_type_t`, `em_freq_band_t`, `em_media_type_t`, `em_profile_type_t`,
  `em_service_type_t`, without sentinels and internal codes
- MAC helpers `dm_easy_mesh_t::macbytes_to_string` / `string_to_macbytes` (`src/dm/dm_easy_mesh.cpp`)

Not here: the rest of `em_base.h` (owned by other crates), and `inc/ieee80211.h` (deferred, D6).

Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.

## Status

Step 2 (crate) implemented; see [STATUS.md](../../STATUS.md).
