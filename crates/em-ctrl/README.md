# em-ctrl

Controller binary (`em-ctrl`). The upstream counterpart `onewifi_em_ctrl` (controller manager, network-wide data model, topology, TR-181 / rbus) is not ported; S1 implements AP-Autoconfiguration discovery with an in-memory peer model; S2 answers each M1 with an M2 and drives `em_config` to the agent's Topology Response, with the `NetworkSSIDList` from a reset file.

## Upstream source

- `src/ctrl/*.cpp`, `src/ctrl/tr_181/`
- `inc/em_ctrl.h`, `inc/dm_easy_mesh_ctrl.h`, `inc/em_cmd_ctrl.h`, `inc/em_network_topo.h`, `inc/tr_181.h`

Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.

## Status

S1 library part (synchronous core, tested without sockets) and the tokio role actor with `main` and the command line implemented. The Docker exit checks are in `docker/run-s1.sh` (0012 D8). The S2 core flow (note [0013](../../docs/0013-s2-onboarding.md) D2, D3, D7) is implemented and tested without sockets; its Docker checks are in `docker/run-s2.sh` (0013 D10). See [STATUS.md](../../STATUS.md).

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
  created longest ago; an update does not change that order. Upstream keeps them without
  limit. S2 keeps the map in memory, as nothing is persisted (0013 D2).
- Log lines for the Docker checks: `peer model created` (agent, profile), `existing peer
  model` (agent), `Response sent` (band, mid, profile). `em-core` also logs
  `autoconfiguration Response handed off`; the wording differs on purpose.

## S2 flow (note 0013 D2, D3, D7, D10)

`CtrlCore::new(networks, drop_first_m2)` takes the reset file's rows (`em_db::load_reset_file`)
and the test switch, and owns an `Orchestrator<CtrlRules>` for `em_config`, a `CtrlNodes`
table (the node table plus each radio's agent AL MAC and `ssid_mismatch` flag), a proto
deadline per radio node and the 250 ms orchestrator deadline. `tick(now)`,
`next_deadline()` and `unregistered()` mirror the agent's.

- **M1 routing** (`em_ctrl.cpp:1240-1270`). The radio ID in an AP-Autoconfiguration WSC
  message selects the node. An existing node goes to `ctrl_wsc_m1_pending` unless it is in
  `wsc_m2_sent`. Otherwise the agent needs a peer model (a Search made it); without one
  the M1 is dropped and logged. A new node records its agent's AL MAC and the band from
  the M1's RF Bands. The M1 is handled only in `ctrl_wsc_m1_pending`.
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
- **40 s cancel.** A radio in `topo_sync_pending` more than 40 s into `em_config` cancels
  it for every radio of its agent (`cancel_scoped`), sets each to `misconfigured` and
  requests one renew per radio (`RoleEffect::Renew`, built by the core with its message-ID
  counter and sent to the agent). No command is left, so no Query and no further renew
  follow.

Log lines for the Docker checks: `radio node created` (radio, agent, band), `M2 sent`
(radio, mid, m2s, build_us), `M2 dropped by test switch`, `Topology Query sent` (agent, radio),
`Topology Response handled` (agent), `topo_synchronized` (radio), `ssid mismatch`,
`em_config cancelled`, `renew sent` (radio, agent, band). An M1 without a peer model logs
`M1 dropped: no peer model for this agent`.

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
- SIGTERM or SIGINT: the actor drops its transport channels, waits at most 1 s for the
  transport task, aborts it, and exits 0. A rejected registration, an ended transport or a
  changed AL MAC logs an error and exits 1.

### The actor (`src/actor.rs`)

One `select!` loop owns the `CtrlCore` and the transport. Arms: the stop future (`break`,
then the 1 s stop and exit 0); the transport task ending, `received` closing, `link`
closing and a failed `reserve()` (each waits at most 1 s for the task and exits 1 with its
error); a received SDU (fed to the core whatever its session); a link change (`Some` calls
`registered`, a different AL MAC is fatal; `None` calls `unregistered`, which stops the proto
ticks); the core's next deadline through `sleep_until` (a proto or orchestrator tick); and
`sdus.reserve()`, only while frames are pending. Every handler first brings the
core up to date through `sync_link`, which reads `link` once and returns that value,
because a received SDU can be selected before its link change. Frames are stamped with the
session of that same value and the recorded AL MAC, and queue in at most 64 pending frames
(oldest dropped, counted, logged at `warn`).

Log lines the Docker checks can match: `registered` (session, al_mac), `new session, same
AL MAC`, `link lost, waiting to register`, `peer model created`, `existing peer model`,
`Response sent`, the S2 lines above, `pending frames full, dropped the oldest`, `em-ctrl stopped` (error),
`transport stopped`.

### Tests

`tests/actor.rs` runs the actor against a fake AL-SAP service over UNIX sockets in a
temporary directory (a trimmed copy of `em-transport`'s test support), in paused time: a
Search answered with a Response on the live session, and again after a reconnect (so it
carries the new session), a different AL MAC (fatal), a rejected registration (error
result) and the stop path (under 1 s), and an M1 answered with an M2 followed by a Topology
Query on the actor's own deadline. `tests/binary.rs` runs the binary with a missing and an
invalid reset file (exit 1).

The core tests (`src/s2_tests.rs`, virtual time, `em-db`'s `load_reset_str` for the rows)
cover M1 routing (new node, reset, no peer model, a radio in `wsc_m2_sent`), an M2 that the
agent side accepts with the reset file's SSIDs, the output order (a radio in
`topo_sync_pending` after a second M1), a failed hand-off, `--test-drop-first-m2`, the
device-wide Query gate, a Topology Response, an SSID mismatch, the 40 s cancel with one
renew per radio, the cancel scoped to one agent, and a new `em_config` after the cancel.

## Open points

- A Search that arrives before the first registration is dropped without a model, since
  no AL node exists to handle it (upstream creates the AL node before it can receive).
- A Search whose AL MAC or band cannot be read is dropped without a model, as upstream's
  routing returns before the commit.
- Messages other than a Search, an M1 and a Topology Response are ignored.
- The stop and "transport ended" waits are 1 s each (0011 A7), both on the virtual clock in
  tests. The brief names no time for the ended case; the same 1 s is used.
- Before the transport task is awaited, an ended transport is always an error exit (1),
  even when the task returned `Ok`: the actor did not ask it to stop.
- `--help` is not special: it is an unknown option and exits 2.
- `CtrlConfig` has no reconnect delay: the transport's default of 2 s applies.
- `OrchRole::on_start` sees no node state, so `em.cpp:173-175` (`wsc_m2_sent` to
  `topo_sync_pending` when `topo_sync` starts) is a readiness effect of
  `ready_for_exec`, which the orchestrator applies just before it starts the node.
- Upstream's cancel hook also posts a `cfg_renew` bus event (`em_orch_ctrl.cpp:404-416`);
  the port sets the radio `misconfigured` and the 40 s hook sends the renew itself. The
  `misconfigured` proto tick that resends a renew (`em_configuration.cpp:6866`) needs a
  running `em_config` and none runs after the cancel, so it is not ported.
- The Topology Response is validated at profile 3, and the SSIDs are compared with every
  row of the list, enabled or not. Upstream uses the peer's profile.
- A radio node's profile is the one in the agent's peer model. The M2 hand-off in the core
  is placing the frame in the output queue; it fails only when the M2 does not validate.
- While an SSID mismatch is flagged the port neither resends Queries nor cancels (the S3
  path); a mismatched agent stays in `topo_sync_pending`.
- A radio node in the controller's table is never removed.
- `CtrlCore::new` takes the reset-file rows and the test switch; `CtrlConfig` gained both.
