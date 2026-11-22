# em-cmd

Shared command data and a read-only orchestration view. S1 implements only
`Command::DevInit`, its AL-insert / radio-insert plan, node keys, orchestration
and status enums, and `CommandView` / `NoCommands`. A chain owns its command;
consumers borrow it through the view. `RadioSpec` preserves the platform's
radio order. See [0012 D1 and D10](../../docs/0012-s1-discovery.md) and
[0002 D2–D4](../../docs/0002-cmd-core-orch.md).

## Upstream source

- `src/cmd/em_cmd_dev_init.cpp:38-55` (the two-stage plan)
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

## Status

S1 command and view types are implemented; other commands and plan variants
remain outside this slice.
