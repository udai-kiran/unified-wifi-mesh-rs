# em-cmd

Orchestrated command types from every source (IPC, internal, platform, engine): one native struct per command, plan variants, origin and status codes. The IPC bridges that decode requests live in the role crates (note [0002](../../docs/0002-cmd-core-orch.md) D2, D3, D7).

## Upstream source

- `src/cmd/*.cpp` (43 files)
- `inc/em_cmd*.h`

Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.

## Status

not started. See [STATUS.md](../../STATUS.md).
