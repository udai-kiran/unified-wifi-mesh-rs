# em-dm

EasyMesh data model: network, device, radio, BSS, STA, op-class, policy, scan results, MLD objects, and the per-device `dm_easy_mesh_t` container. Each process (controller, agent) holds its own instances; they are kept in step only by CMDUs.

## Upstream source

- `src/dm/*.cpp`
- `inc/dm_*.h`

Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.

## Status

not started. See [STATUS.md](../../STATUS.md).
