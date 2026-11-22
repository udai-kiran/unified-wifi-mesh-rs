# em-orch

Synchronous orchestration with caller-supplied `Instant`s. `submit` calls
role pre-processing in plan order and returns node-creation effects; the caller
applies them before `admit` builds candidates. A pass activates at most one
stage, choosing the newest eligible one, checks active stages newest first,
and checks each stage's candidates from last to first. Timers keep the maximum
elapsed whole seconds per command type. Transient resets and cancellation take
effect inside the pass. Each hook's role effects are applied through
`OrchRole::apply` immediately; the role records core requests in an output
buffer, which the caller drains after the pass.
`on_event` performs activation and readiness without timer or transient work.
See [0012 D2 and D10](../../docs/0012-s1-discovery.md),
[0013 D3](../../docs/0013-s2-onboarding.md), and
[0002 D2–D4](../../docs/0002-cmd-core-orch.md).

## Upstream source

- `src/orch/em_orch.cpp:42-157,175-360,511-574`
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
- `submit` accepts the mutable node table for the shared role interface but
  leaves it untouched: the caller applies its returned pre-processing effects
  before `admit`. Passes and scoped cancellation receive a caller-owned `Out`
  buffer, avoiding a `Default` bound on role outputs.
- `cancel_command` takes a node-key slice; `TransientOut::cancel_scoped`
  takes an owned list so the request remains ordered with other transient
  actions. An empty slice cancels no nodes but resets the elapsed value, as the
  upstream scoped overload does. A reset clears the per-type statistic, not
  individual active stages' start times.
- `remove_chain_for_node` takes the role's unconfigured state and returns its
  `SetState` effect only for an active match, as upstream changes role state
  only in the active queue. The caller applies the effect. Pending matches are
  removed from their candidate lists; active matches remain until the pass
  destroys their finished stage. Upstream also resets three controller query
  counters at removal (`em_orch.cpp:257-260`); those counters are not present
  in the S1 node model and the future controller must reset them when applying
  this removal's returned effect.

## Status

S1 orchestration and the S2 additions (per-hook effects, scoped cancellation and
`em_config` node removal) are implemented. The S2 command rules are in `em-agent`
and `em-ctrl` (0013 D3).
