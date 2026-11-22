# em-cmd

Shared command data and a read-only orchestration view. The S1 `DevInit` plan
and S2 `OneWifiCb`, `CfgRenew` and `EmConfig` plans share node keys and
orchestration and status enums. A chain owns its command; consumers borrow it
through `CommandView`. See [0012 D10](../../docs/0012-s1-discovery.md) and
[0013 D3–D4](../../docs/0013-s2-onboarding.md).

## Upstream source

- `src/cmd/em_cmd_dev_init.cpp:38-55` (the two-stage plan)
- `src/cmd/em_cmd_onewifi_cb.cpp:38-54`, `src/cmd/em_cmd_cfg_renew.cpp:29-45`,
  `src/cmd/em_cmd_em_config.cpp:38-71` (S2 command plans)
- `src/orch/em_orch_agent.cpp:347-353,397-405`,
  `src/orch/em_orch_ctrl.cpp:530-545,593-599` (candidate and BSS-delete inputs)
- `inc/em_base.h:3667-3674,3694-3704` and `src/em/em.cpp:283-293` (states and status)

Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.

## Open points

- The plan exposes its upstream `submit` flag as `PlanStage::submit`; the
  role's `pre_process` return determines whether that stage is admitted, as in
  `src/orch/em_orch.cpp:52-64` and `src/orch/em_orch_agent.cpp:317`. An S1
  role should return the plan's flag after performing its stage's effects.
- `ActiveOp::stage` is the zero-based index into the complete plan, including
  stages that only pre-process. Its command becomes visible through `active`
  when execution starts, matching the pointer assignment in
  `src/em/em.cpp:86-92`.
- `OneWifiCb` carries radio node keys in platform order; confirmed BSS records
  live in the role's model. The per-device list follows 0013 D4, while upstream
  selects one candidate from the first BSS. `CfgRenew` carries the selected
  radio key. `EmConfig` carries a radio key and the agent AL MAC for BSS
  deletion. Callers are responsible for supplying radio-kind keys.
- `EmConfig` stops after `TopoSync`; the rest of upstream's plan is deferred
  by 0013 D3.

## Status

S1 and S2 command and view types are implemented. The S2 role flows that use
them are in `em-agent` and `em-ctrl` (0013 D3 and D4).
