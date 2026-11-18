# em-core

Protocol engine: the per-node `em_t` with its state machine and per-area handlers (configuration, discovery, provisioning, capability, channel, metrics, steering, policy, vendor), plus the `em_mgr_t` event loop.

## Upstream source

- `src/em/em.cpp`, `em_sm.cpp`, `em_mgr.cpp`, `em_net_node.cpp`, `em_onewifi.cpp`, `em_vendor.cpp`
- `src/em/{config,disc,capability,channel,metrics,steering,policy_cfg}/`
- `src/em/prov/em_provisioning.cpp`
- `inc/em*.h`

Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.

## Status

not started. See [STATUS.md](../../STATUS.md).
