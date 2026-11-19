# em-orch

Command orchestration as a synchronous library: pending/active queues, oldest-first activation, deadlines, cancel and node removal. Controller and agent rules come through `OrchRole` hooks that return effects (note [0002](../../docs/0002-cmd-core-orch.md) D2, D4).

## Upstream source

- `src/orch/em_orch.cpp`, `em_orch_ctrl.cpp`, `em_orch_agent.cpp`
- `inc/em_orch*.h`

Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.

## Status

not started. See [STATUS.md](../../STATUS.md).
