# em-orch

Synchronous S1 orchestration with caller-supplied `Instant`s. `submit` calls
role pre-processing in plan order and returns node-creation effects; the caller
applies them before `admit` builds candidates. A pass activates at most one
stage, choosing the newest eligible one, checks active stages newest first,
and checks each stage's candidates from last to first. Timers keep the maximum
elapsed whole seconds per command type. Transient resets and cancellation take
effect inside the pass; role effects are returned in order for application after the pass.
`on_event` performs activation and readiness without timer or transient work.
See [0012 D2 and D10](../../docs/0012-s1-discovery.md) and
[0002 D2–D4](../../docs/0002-cmd-core-orch.md).

## Upstream source

- `src/orch/em_orch.cpp:42-157,271-360,511-574`
- `src/orch/em_orch_agent.cpp:42-87,172-175,235-283,320-345`
- `src/em/em.cpp:86-114,283-293,3221-3229`

Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`. The whole-seconds calculation
intentionally differs from upstream's subtraction of monotonic-clock second fields:
it may reach a threshold up to one second later (0012 D2).

## Open points

- The role's `pre_process` return controls admission, matching upstream's
  pre-processing result; the standard result is the plan stage's `submit`
  flag. All stages are pre-processed before any candidates are built.
- A repeated `admit` on a still-existing admitted chain returns `true` without
  queueing it twice; an unknown or destroyed chain returns `false`. Upstream's
  one-phase submission has no corresponding repeated call.
- `active(node)` becomes available on `Pending` → `Progress` and remains
  available through `Fini` until destruction, matching upstream's command
  pointer lifetime (`src/em/em.cpp:86-92`, `src/orch/em_orch.cpp:159-168`).
- An event pass also destroys a stage whose candidates were already `Fini`,
  while skipping elapsed updates and transient hooks. This uses the same
  readiness and cleanup pass as a tick (`src/orch/em_orch.cpp:316-345,549-572`).
- Callers should pass nondecreasing instants. If an earlier instant is passed,
  elapsed duration saturates at zero; upstream uses a monotonic clock
  (`src/orch/em_orch.cpp:81-85`).

## Status

S1 chain, admission, activation, readiness, per-type timer and cancellation
are implemented. Role rules, other commands, controller orchestration and
node removal belong to later work.
