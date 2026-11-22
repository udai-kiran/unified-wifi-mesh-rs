# em-agent

Agent binary (`em-agent`). The upstream counterpart `onewifi_em_agent` (agent manager, agent data model, OneWifi bus integration, simulator) is not ported; S1 implements only AP-Autoconfiguration discovery.

## Upstream source

- `src/agent/*.cpp`
- `inc/em_agent.h`, `inc/dm_easy_mesh_agent.h`, `inc/em_cmd_agent.h`, `inc/em_simulator.h`

Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.

## Status

S1 library part (synchronous core, tested without sockets) and the tokio role actor with `main` and the command line implemented. The Docker exit checks are in `docker/run-s1.sh` (0012 D8). See [STATUS.md](../../STATUS.md).

## S1 library part (note 0012 D2, D9, D10)

`AgentCore` is the synchronous agent role: an `Orchestrator<AgentRules>`, a
`NodeTable<AgentState>`, the process `MsgIdCounter`, one proto deadline per node
(restarted on every event dispatched to the node, firing after 1 s idle) and the
250 ms idle orchestrator deadline. It has no sockets and no runtime; the tokio role
actor (`run`) moves inputs in and frames out.

- Inputs: `registered(al_mac, now)`, `unregistered()`, `received(remote, cmdu, now)`,
  `tick(now)`. Outputs: `Frame { remote, cmdu }` (the actor adds `local` and the
  session) and `next_deadline()`.
- The first `registered` records the AL MAC and submits and admits `dev_init`. The same
  AL MAC later continues; a different one returns `AlMacChanged` (fatal for the process).
  While unregistered no proto tick is dispatched and no proto deadline is reported.
- `AgentRules` implements `OrchRole` as `em_orch_agent.cpp` does, with the "until found"
  reset in `transient` (intentional differences `search-until-found` and
  `elapsed-whole-seconds`).
- Log lines for the Docker checks: `dev_init submitted` (chain), `Search sent` (radio, band,
  mid), `radio found controller` (radio, controller, profile), `dev_init cancelled: time limit
  exceeded`.
  `em-core` already logs `autoconfiguration Search handed off` and `controller found`
  (without the radio); the core's messages are worded differently so one event never
  matches the same grep twice.

## Running (note 0012 D4, D7, D9)

```sh
cargo run -p em-agent --release -- \
    --control /tmp/al_control_socket --data /tmp/al_data_socket \
    --radio 02:00:00:00:01:01,0 --radio 02:00:00:00:01:02,1 --profile 3
```

- `--radio <ruid>,<band>`: repeatable, at least one. Band `0` = 2.4 GHz, `1` = 5 GHz,
  `2` = 60 GHz, `3` = 6 GHz.
- `--control` and `--data`: the AL-SAP sockets; the defaults are upstream's agent paths
  (`/tmp/al_control_socket`, `/tmp/al_data_socket`).
- `--profile <2|3>`: default 3. A port test switch; upstream always sends 3.
- A bad argument prints the usage on stderr and exits 2.
- Logs go to stdout as plain text; the level comes from `RUST_LOG` (default `info`).
- SIGTERM or SIGINT: the actor drops its transport channels, waits at most 1 s for the
  transport task, aborts it, and exits 0. A rejected registration, an ended transport or a
  changed AL MAC logs an error and exits 1.

### The actor (`src/actor.rs`)

One `select!` loop owns the `AgentCore` and the transport. Arms: the stop future (`break`,
then the 1 s stop and exit 0); the transport task ending, `received` closing, `link`
closing and a failed `reserve()` (each waits at most 1 s for the task and exits 1 with its
error); a received SDU (fed to the core whatever its session); a link change (`Some` calls
`registered`, a different AL MAC is fatal; `None` calls `unregistered`); the core's next
deadline through `sleep_until` (a proto or orchestrator tick); and `sdus.reserve()`, only
while frames are pending, which sends the oldest pending frame. No handler awaits a send.
Every handler first brings the core up to date through `sync_link`, which reads `link`
once and returns that value, because a received SDU can be selected before its link
change. Frames are stamped with the session of that same value and the recorded AL MAC,
and queue in at most 64 pending frames (oldest dropped, counted, logged at `warn`).

Log lines the Docker checks can match: `registered` (session, al_mac), `new session, same
AL MAC`, `link lost, waiting to register`, `dev_init submitted`, `Search sent`, `radio found
controller`,
`pending frames full, dropped the oldest`, `em-agent stopped` (error), `transport stopped`.

### Tests

`tests/actor.rs` runs the actor against a fake AL-SAP service over UNIX sockets in a
temporary directory (a trimmed copy of `em-transport`'s test support), in paused time:
registration and Searches with the recorded AL MAC, a Response per radio stopping the
Searches, the same AL MAC on a new session, a different AL MAC (fatal), a rejected
registration (error result) and the stop path (under 1 s).

## Open points

- Every node, the AL node included, gets a proto deadline, as every upstream `em_t`
  has its own thread timeout. The AL node's tick does nothing in S1.
- A node's proto deadline starts at its creation, as upstream's thread timeout starts
  when the thread does, so the first Search leaves 1 s after registration.
- The orchestrator deadline restarts after each orchestrator pass and after each event
  pass, so it is idle-based; the 250 ms value is upstream's idle wait.
- Proto deadlines that fall due while unregistered are not restarted: they fire at
  the first `tick` after the next registration.
- A rejected or unroutable Response is logged and dropped. The routed node's proto
  deadline restarts when a Response was dispatched to it, whether or not it was accepted.
- `ready_for_fini` follows upstream's exact test (`owconfig_pending`, `wsc_m2_processed`,
  or `topo_synchronized` and later), not "owconfig_pending or later".
- Pre-processing gives the AL node the configured profile as well as the radios
  (0012 D10); upstream hard-codes 3 for both.
- The stop and "transport ended" waits are 1 s each (0011 A7), both on the virtual clock in
  tests. The brief names no time for the ended case; the same 1 s is used.
- Before the transport task is awaited, an ended transport is always an error exit (1),
  even when the task returned `Ok`: the actor did not ask it to stop.
- `--help` is not special: it is an unknown option and exits 2.
- `AgentConfig` has no reconnect delay: the transport's default of 2 s applies.
