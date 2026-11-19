# em-core

Protocol engine library: the node table with each node's state machine, and synchronous per-area handlers (configuration, discovery, provisioning, capability, channel, metrics, steering, policy, vendor) over borrowed state (`EngineCtx`). The `em_mgr_t` event loop is not here: it becomes the role actor in `em-ctrl` / `em-agent` (note [0002](../../docs/0002-cmd-core-orch.md) D1, D9).

## Upstream source

- `src/em/em.cpp`, `em_sm.cpp`, `em_mgr.cpp`, `em_net_node.cpp`, `em_onewifi.cpp`, `em_vendor.cpp`
- `src/em/{config,disc,capability,channel,metrics,steering,policy_cfg}/`
- `src/em/prov/em_provisioning.cpp`
- `inc/em*.h`

Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.

## Status

not started. See [STATUS.md](../../STATUS.md).
