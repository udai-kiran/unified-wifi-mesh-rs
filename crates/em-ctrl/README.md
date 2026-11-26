# em-ctrl

Controller binary (`em-ctrl`). The upstream counterpart `onewifi_em_ctrl` (controller manager, network-wide data model, topology, TR-181 / rbus) is not ported; S1 implements AP-Autoconfiguration discovery with an in-memory peer model; S2 answers each M1 with an M2 and drives `em_config` to the agent's Topology Response, with the `NetworkSSIDList` from a reset file.

## Upstream source

- `src/ctrl/*.cpp`, `src/ctrl/tr_181/`
- `inc/em_ctrl.h`, `inc/dm_easy_mesh_ctrl.h`, `inc/em_cmd_ctrl.h`, `inc/em_network_topo.h`, `inc/tr_181.h`

Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.

## Status

S1 library part (synchronous core, tested without sockets) and the tokio event loop with `main` and the command line implemented. The controller runs on the loop since D8 step 5 (note 0016). The Docker exit checks are in `docker/run-s1.sh` (0012 D8). The S2 core flow (note [0013](../../docs/0013-s2-onboarding.md) D2, D3, D7) is implemented and tested without sockets; its Docker checks are in `docker/run-s2.sh` (0013 D10). See [STATUS.md](../../STATUS.md).

## S1 library part (note 0012 D3, D9, D10)

`CtrlCore` is the synchronous controller role: a `NodeTable<CtrlState>`, the process
`MsgIdCounter`, `em_cmd::NoCommands` as the command view, and an in-memory map of peer
models by AL MAC. It has no sockets and no runtime.

- Inputs: `registered(al_mac)` and `received(remote, cmdu, now)`; the output is the
  `Frame`s to send.
- The first `registered` creates the AL node with the registered AL MAC (a temporary
  difference until the database exists). The same AL MAC later changes nothing; a
  different one returns `AlMacChanged` (fatal for the process).
- `received` routes a Search (`route_search`), lets the AL node handle it
  (`handle_search`), then commits the peer model: created with the routed profile (1 when
  absent, malformed or 4 or more) and `is_emplus_agent` when none exists, otherwise only
  `is_emplus_agent` is updated. The commit happens whatever the handler did, as upstream
  requests it in routing.
- Peer models are capped at 256. When the table is full, a new AL MAC evicts the model
  heard from least recently (`intentional-difference:peer-model-lru-eviction`, see M1 checks
  and eviction below). Upstream keeps them without limit. S2 keeps the map in memory, as
  nothing is persisted (0013 D2).
- Log lines for the Docker checks: `peer model created` (agent, profile), `existing peer
  model` (agent), `Response sent` (band, mid, profile). `em-core` also logs
  `autoconfiguration Response handed off`; the wording differs on purpose.

## S2 flow (note 0013 D2, D3, D7, D10)

`CtrlCore::new(networks, drop_first_m2)` takes the reset file's rows (`em_db::load_reset_file`)
and the test switch, and owns an `Orchestrator<CtrlRules>` for `em_config`, a `CtrlNodes`
table (the node table plus each radio's agent AL MAC and `ssid_mismatch` flag), a proto
deadline per radio node and the 250 ms orchestrator deadline. `tick(now)`,
`next_deadline()` and `unregistered()` mirror the agent's.

`CtrlCore<R>` takes its random source as a type parameter, `R = OsRandom` by default.
`CtrlCore::new` is the production constructor (OS randomness and the real clock).
`CtrlCore::with_sources` takes any random source and a measurement clock, which the replay
tests use. The clock is read only for the M2 build timing (`build_us`).

- **M1 routing** (`em_ctrl.cpp:1240-1270`). The radio ID in an AP-Autoconfiguration WSC
  message selects the node. The M1 is checked before anything changes (see M1 checks and
  eviction below). The sender needs a peer model (a Search made it), for a known radio too;
  without one the M1 is dropped and logged. An existing node goes to `ctrl_wsc_m1_pending`
  unless it is in `wsc_m2_sent`, where the M1 is ignored; a known radio of another agent is
  moved (see M1 checks and eviction below). A new node records its agent's AL MAC and the
  band from the M1's RF Bands. The M1 is handled only in `ctrl_wsc_m1_pending`.
- **M2.** `em_core::wsc::build_m2` builds one WSC TLV per haul from the rows, the frame is
  validated at profile 3 and handed to the transport, and then, in upstream's order
  (`em_configuration.cpp:6262-6275`), `RemoveChainForNode`, `SetState(wsc_m2_sent)` and
  `SubmitEmConfig` apply. A failed hand-off applies none of them.
  `--test-drop-first-m2` makes the first M2's hand-off a no-op that reports success.
- **`em_config`** (`CtrlRules`, `em_orch_ctrl.cpp`): candidate the radio; ready to start
  in `unconfigured`, `wsc_m2_sent`, `topo_synchronized` or `misconfigured`, with the move
  `wsc_m2_sent` to `topo_sync_pending` as a readiness effect; finishes at
  `topo_synchronized`, and a radio regressed to `wsc_m2_sent` is moved to
  `topo_sync_pending` (`:196-203`).
- **Topology Query.** On each idle second (the proto tick, only while `em_config` is
  `Progress` on the radio, `em.cpp:538-546`) a radio in `topo_sync_pending` sends a Query
  when every radio of its agent is in `topo_sync_pending`, and only the first radio sends
  it; the controller's AL node then enters `topo_sync_pending`.
- **Topology Response.** Accepted while any radio of the agent is in `topo_sync_pending`;
  `em_core::topology::parse_response` checks every reported SSID against the list. Success
  moves every radio of the agent to `topo_synchronized`. An unknown SSID flags the
  agent's first radio and is logged (`ssid mismatch`); the 120 s mismatch path is S3.
- **40 s cancel.** A radio in `topo_sync_pending` more than 40 s into `em_config`, with no
  mismatch flagged, sets every radio of its agent `misconfigured`, starts a Renew retry
  episode for each (below) and cancels their `em_config` (`cancel_scoped`). A radio in
  another state past 40 s cancels the `em_config` of its agent's radios only; the cancel
  hook then sets `misconfigured` and starts an episode for the radios whose stage was
  active. No command is left, so no Query follows.

Log lines for the Docker checks: `radio node created` (radio, agent, band), `M2 sent`
(radio, mid, m2s, build_us), `M2 dropped by test switch`, `Topology Query sent` (agent, radio),
`Topology Response handled` (agent), `topo_synchronized` (radio), `ssid mismatch`,
`em_config cancelled`, `renew sent` (radio, agent, band). An M1 without a peer model logs
`M1 dropped: no peer model for this agent`.

### Renew retry and per-agent age (0014 D18 R1, R2, S2-04)

`CtrlRules::transient` (the 40 s path of a radio in `topo_sync_pending`) and
`CtrlRules::pre_process_cancel` (every cancel that reaches an active stage) set a cancelled
radio `misconfigured` and emit `RoleEffect::Renew`; the core turns each one into
`start_renew_episode`, the only place a first Renew is sent. It sends one Renew (count 1),
and the radio's count lives in `CtrlCore::renew_counts` (a `SortedVecMap` keyed by radio; an
entry stays once its episode has sent five, and a move or an eviction removes it; no entry
means no episode). On the 1 s proto tick of a `misconfigured` radio with a count of 1 to 4
the core sends one more Renew (`renew sent`) and counts it, so a radio gets five Renews,
then none. An M1 sets the radio to another state, which ends the retry. A move or an eviction
is not a cancel: `reset_radio_per_radio_state` drops the count and nothing is sent. The retry is upstream's
(`em_orch_ctrl.cpp:75-92,171-175,403-420`, `em_configuration.cpp:850-938,6865-6866`,
`EM_MAX_RENEW_TX_THRESH` 5 at `em_base.h:115`).

- `intentional-difference:renew-retry-counter-per-episode`: upstream carries the count from
  one episode to the next and clears it only at the threshold and in the `cfg_renew` cancel
  hook (`em_orch_ctrl.cpp:171-175,418-420`); a cancel here always starts at 0 (R1).
- `intentional-difference:renew-retry-without-cfg-renew-command`: upstream sends the retries
  only while a `cfg_renew` command is in progress (`em.cpp:546-559`) and cancels it type-wide
  after 10 s (`em_orch_ctrl.cpp:157-163`). The controller has no such command (R2).
- `intentional-difference:em-config-age-per-agent`: the 40 s test reads the stage's own age
  (`TransientOut::stage_elapsed`), and an accepted M1 restarts the age of the agent's
  `em_config` stages (`Orchestrator::reset_stage_start`). Upstream keeps one time for the
  command type and resets it for the whole type (`em_orch.cpp:72-88,129-138,222`), so a
  busy agent kept every other agent's timeout from firing. The M2 hand-off needs no second
  reset: it runs inside the same call at the same time.
- `intentional-difference:em-config-cancel-per-agent`: the cancel of a stuck radio in a state
  other than `topo_sync_pending` covers the radios of its agent, where upstream cancels the
  whole type (`em_orch_ctrl.cpp:142-150`); only the candidates of active stages are marked
  and renewed, by the cancel hook, as upstream.

The two cancel branches mark different radios. Rule (a): a radio in `topo_sync_pending` past 40 s
sets every radio of its agent `misconfigured` and sends it a Renew, then cancels their `em_config`;
upstream's 40 s path does the same itself (`em_orch_ctrl.cpp:75-92`). Rule (b): a radio in any
other state past 40 s only cancels `em_config` for the radios of its agent (`transient`). Upstream
cancels the whole type there (`em_orch_ctrl.cpp:142-150`), and its cancel hook marks and renews
the candidates of the active stages only; a stage still pending is dropped silently
(`em_orch.cpp:278-302`). Here `pre_process_cancel` is that hook. So a radio whose `em_config` was
still queued is left with no chain, Query or Renew until its agent sends a new M1, as upstream. A
sibling whose finished stage is still in the orchestrator's active list is hooked, as upstream;
once its chain is destroyed it is left alone. The tests reach rule (b) by setting a
radio's state directly, or through the test-only hand-off failure, which leaves a radio in
`wsc_m1_pending` with its stage active. A failed M2 build no longer does this, since the M2 is
built before any state change (see M1 checks and eviction below).

### M1 checks and eviction (0014 D18 S2-03, S2-06, S2-08, Q1, R3)

`handle_wsc` checks an M1 in full before it changes anything: parse it (`M1 rejected`); the
routing RUID (`readers::radio_id`, the first TLV that carries one) must equal the RUID of
AP Radio Basic Capabilities (`M1 refused: the routing radio differs ...`); the sender must
have a peer model, also for a known radio (`M1 dropped: no peer model ...`); a new radio or a
move must leave the sender within `EM_MAX_RADIO_PER_AGENT` = 4 radios (`M1 refused: the
agent has the most radios allowed`). A refusal changes no node, link, owner, band, mismatch
flag, chain or deadline. The `wsc_m2_sent` rule (`M1 ignored`) comes after the checks and
applies to the radio's owner only and changes nothing, model order included. Then the M2 is
built and validated, still before any change: an M1 whose M2 cannot be built (`M2 could not
be built`, for example `max_bss` = 0) or fails validation (`M2 failed validation, not sent`)
changes no node, link, owner, mismatch flag, chain, deadline, retry counter, age, state or
model order. The peer model is refreshed in the eviction order only after a successful build.

- `intentional-difference:m1-validated-before-state`: upstream creates the node from the
  routing RUID with an unknown band before it parses the M1 (`em_ctrl.cpp:1259-1265`); here
  the M2 is also built and validated before any state change.
- `intentional-difference:radio-cap-per-agent`: upstream defines `EM_MAX_RADIO_PER_AGENT`
  (`inc/em_base.h:90`) and does not enforce it.
- `intentional-difference:radio-owner-move`: an M1 for a known radio from another agent
  moves it in one step (owner, profile, band, mismatch cleared, the old `em_config` removed,
  `wsc_m1_pending`, `reset_radio_per_radio_state`; log `radio moved to another agent`).
  Upstream sends M2 to the sender and Query and Renew to the first owner
  (`em_configuration.cpp:483,959`). Any agent with a peer model can take a radio until
  Stage 3 (accepted risk R4).
- `intentional-difference:mismatch-cleared-on-m1`: an accepted M1 clears `ssid_mismatch` on
  every radio of its agent.
- `intentional-difference:peer-model-lru-eviction`: peer models are evicted least recently
  heard from. A Search, an M1 whose M2 was built and validated (an M1 that is refused or
  that the owner's `wsc_m2_sent` rule ignores does not count) and a Topology Response
  refresh the model. Eviction removes the agent's `em_config` chains, then its radio nodes,
  links and per-radio state (`reset_radio_per_radio_state`), and logs
  `peer model evicted, the table is full`.

## Running (note 0012 D7, D9)

```sh
cargo run -p em-ctrl --release -- \
    --control /tmp/al_em_ctrl_control_socket --data /tmp/al_em_ctrl_data_socket \
    --reset-file /nvram/Reset.json
```

- `--control` and `--data`: the AL-SAP sockets; the defaults are upstream's controller paths
  (`/tmp/al_em_ctrl_control_socket`, `/tmp/al_em_ctrl_data_socket`).
- `--reset-file`: the `Reset.json` whose `NetworkSSIDList` supplies the M2 rows; default
  `/nvram/Reset.json`. A missing or invalid file is logged and the process exits 1
  (`intentional-difference:reset-file-required`; upstream continues with no SSIDs).
- `--test-drop-first-m2`: test only. The first M2 is built and its outputs apply as after a
  real write, but it is not handed to the transport (logged `M2 dropped by test switch`).
  The Docker lost-M2 check uses it.
- A bad argument prints the usage on stderr and exits 2.
- Logs go to stdout as plain text; the level comes from `RUST_LOG` (default `info`).
- SIGTERM or SIGINT: the controller stops as described in "The loop" below and exits 0. A
  rejected registration, an ended transport or a changed AL MAC logs an error and exits 1.

### The loop (`src/actor.rs`)

One `em-event-loop` loop per process (0016 D2, D3b) drives two handlers: the transport
(`TransportHandler`, `em-transport`) and `CtrlCore`. The sources, in order: the data socket
read (up to 64 frames per pass), the registration, the application channel (it carries
nothing and is closed at the stop), a received SDU for the role (one per pass), and the data
socket write. The timers are the transport's and the role's `next_deadline()`.

- **Link changes.** After every turn the loop compares the transport's link with the one the
  role was last told, and tells the role the difference in order. A new session of a registered
  link gives `unregistered()` and then `registered()`, also when the loop never saw the link
  lost in between.
- **Frames.** A role frame goes straight into the transport's outbound buffer for the current
  session, stamped with that session and the recorded AL MAC. The buffer holds 4 MiB of
  stored encoded octets. When a frame does not fit, whole SDUs are dropped, oldest first,
  never one that has started or whose write is in flight (0016 E12 item 9; difference
  `tx-buffer`). If dropping cannot make room (the frame alone, or the frame and the protected
  SDU, are over 4 MiB), the new frame is refused and nothing is dropped. There is no separate pending queue.
- **Stop.** A future polled in the same task drops the application channel's sender when
  `stop` completes; the closed channel is the stop signal. The role gets no further input. In a
  session the transport then writes the frames it has queued for at most 900 ms and keeps its
  sockets open for 100 ms; without a session it finishes at once. `run` returns `Ok` and logs
  `transport stopped`.
- **Errors.** A transport that ends without a stop returns `ActorError::Transport` with its
  error. A changed AL MAC returns `ActorError::AlMacChanged` at once, without the transport
  stop.

Log lines the Docker checks can match (target `em_ctrl::actor`): `registered` (session,
al_mac), `new session, same AL MAC`, `link lost, waiting to register`, `transport stopped`, and
`transport ended with an error while stopping` (warn). The S2 lines above come from the core.
`em-ctrl stopped` (error) is logged by `main`.

### Tests

The package has 74 library tests, 7 in `tests/actor.rs` and 2 in `tests/binary.rs`.

`tests/actor.rs` runs the controller against a fake AL-SAP service over UNIX sockets in a
temporary directory (a trimmed copy of `em-transport`'s test support), in paused time: a
Search answered with a Response on the live session, the same AL MAC on a new session (the
log shows the lost link first), a different AL MAC (fatal), a rejected registration (error
result, also on a reconnect), the stop path (under 1 s), and an M1 answered with an M2 followed
by a Topology Query on the role's own deadline. `tests/binary.rs` runs the binary with a
missing and an invalid reset file (exit 1).

The unit tests in `src/actor.rs` check the link-change function on its own (including a loss
the loop never saw), a role frame reaching the transport, the reconnect order, a protocol
deadline that waits for the reconnect (no frame during the outage), a changed AL MAC, no turn
after the transport has finished or the AL MAC changed, the role not running its pending
deadlines after the stop, and a stop with a frame in flight (within 1 s), on a real `LoopCore` with a
clock the test moves.

The core tests (`src/s2_tests.rs`, virtual time, `em-db`'s `load_reset_str` for the rows)
cover M1 routing (new node, reset, no peer model, a radio in `wsc_m2_sent`), an M2 that the
agent side accepts with the reset file's SSIDs, the output order (a radio in
`topo_sync_pending` after a second M1), a failed hand-off, `--test-drop-first-m2`, the
device-wide Query gate, a Topology Response, an SSID mismatch, the 40 s cancel with five
Renews per radio, the retry (an M1, a move and an eviction stop it; a second cancel starts a
fresh five; counts per radio), the per-agent 40 s age, the cancel scoped to one agent, and a
new `em_config` after the cancel.

The replay tests (`replay_same_seed_gives_identical_output`, `replay_core_seed_reaches_m2`, 0016
AC3) run a scripted session through `CtrlCore::with_sources`, with a fixed clock, the `SeededRandom`
source of `em-crypto` for the core, and a seeded test agent that builds the M1 (feature
`test-random`, enabled only in dev-dependencies). Two runs with the same seeds must give the same
frames and deadlines (the controller has no platform, so no platform requests). A run with a
different core seed must change the core's M2. They check output determinism only.

## Open points

- A Search that arrives before the first registration is dropped without a model, since
  no AL node exists to handle it (upstream creates the AL node before it can receive).
- A Search whose AL MAC or band cannot be read is dropped without a model, as upstream's
  routing returns before the commit.
- Messages other than a Search, an M1 and a Topology Response are ignored.
- The stop in a session takes at most 1 s (900 ms of writes, then 100 ms of linger; 0016 D3b,
  U8); without a session it finishes at once. Tests run it on the virtual clock.
- A transport that ends without a stop is always an error exit (1).
- `--help` is not special: it is an unknown option and exits 2.
- `CtrlConfig` has no reconnect delay: the transport's default of 2 s applies.
- `OrchRole::on_start` sees no node state, so `em.cpp:173-175` (`wsc_m2_sent` to
  `topo_sync_pending` when `topo_sync` starts) is a readiness effect of
  `ready_for_exec`, which the orchestrator applies just before it starts the node.
- Upstream's cancel hook also posts a `cfg_renew` bus event (`em_orch_ctrl.cpp:404-416`);
  the port sets the radio `misconfigured` and sends the first renew itself, and the
  `misconfigured` proto tick sends the rest, five renews in all (see "Renew retry").
- The Topology Response is validated at profile 3, and the SSIDs are compared with every
  row of the list, enabled or not. Upstream uses the peer's profile.
- A radio node's profile is the one in the agent's peer model. The M2 hand-off in the core
  only places the frame in the output queue and cannot fail outside the tests; the M2 is
  built and validated before any change.
- While an SSID mismatch is flagged the port sends no Query, and the 40 s topology path does
  not cancel that agent's `em_config` (the S3 path); a mismatched agent stays in
  `topo_sync_pending`.
- A radio node leaves the table only when its agent is evicted from the peer model table; a
  move keeps the node (0014 D18 Q1).
- `CtrlCore::new` takes the reset-file rows and the test switch; `CtrlConfig` gained both.
