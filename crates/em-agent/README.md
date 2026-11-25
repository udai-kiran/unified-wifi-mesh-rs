# em-agent

Agent binary (`em-agent`). The upstream counterpart `onewifi_em_agent` (agent manager, agent data model, OneWifi bus integration, simulator) is not ported; S1 implements AP-Autoconfiguration discovery and S2 takes each radio through WSC M1/M2 to `topo_synchronized` against a fake platform.

## Upstream source

- `src/agent/*.cpp`
- `inc/em_agent.h`, `inc/dm_easy_mesh_agent.h`, `inc/em_cmd_agent.h`, `inc/em_simulator.h`

Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.

## Status

S1 library part (synchronous core, tested without sockets) and the tokio event loop with `main` and the command line implemented. The agent runs on the loop since D8 step 4c (note 0016). The Docker exit checks are in `docker/run-s1.sh` (0012 D8). The S2 core flow (note [0013](../../docs/0013-s2-onboarding.md) D3 to D5) is implemented and tested without sockets; its Docker checks are in `docker/run-s2.sh` (0013 D10). See [STATUS.md](../../STATUS.md).

## S1 library part (note 0012 D2, D9, D10)

`AgentCore` is the synchronous agent role: an `Orchestrator<AgentRules>`, a
`NodeTable<AgentState>`, the process `MsgIdCounter`, one proto deadline per node
(restarted on every event dispatched to the node, firing after 1 s idle) and the
250 ms idle orchestrator deadline. It has no sockets and no runtime; the event loop
(`run`) moves inputs in and frames out.

- Inputs: `registered(al_mac, now)`, `unregistered()`, `received(remote, cmdu, now)`,
  `tick(now)`. Outputs: `Frame { remote, cmdu }` (the loop adds `local` and the current
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

## S2 flow (note 0013 D4, D5)

`AgentCore` keeps its inputs and gains `platform_confirmed(bss, now)` and
`take_platform_requests()`. After each input the loop hands the requests to the
`Platform` and feeds confirmations back as inputs.

- **M1.** A Response that moves a radio to `wsc_m2_pending` makes the core build M1 with
  `em_core::wsc::build_m1` (new MID, MAC Address = the agent's AL MAC), send it to the
  controller found in the Response and record the hand-off time. If M1 cannot be built
  the radio goes back to `autoconfig_rsp_pending` and keeps searching, as upstream
  leaves the state unchanged (`em_configuration.cpp:6376-6391`). The fake radios'
  identity, capabilities and maximum BSS count (4) are fixed values in `src/wsc.rs`.
- **M2.** Accepted only in `wsc_m2_pending`, from the recorded controller, for the radio
  its AP Radio Identifier names (`em_core::wsc::accept_m2`). A rejected M2 is logged and
  the radio keeps waiting with its session. An accepted M2 queues an
  `ApplyRadioConfig { radio, bss }` for the platform and puts the radio in
  `owconfig_pending`.
- **Platform** (`src/platform.rs`). `Platform::apply_radio_config(radio, &[BssConfig])`
  and `Platform::confirmations()`, the channel on which confirmations reach the loop.
  `FakePlatform` records each radio's BSSs with fake BSSIDs and sends one
  `Confirmation` once every radio has a configuration, and again after each later
  configuration (a renewed radio).
- **`onewifi_cb`** (`intentional-difference:onewifi-cb-per-device`). `platform_confirmed`
  stores the BSS list and submits one `onewifi_cb` for all radios; each radio in
  `owconfig_pending` moves to `onewifi_bssconfig_ind` when it starts. It is dropped while
  another `onewifi_cb` runs, as upstream (`em_agent.cpp:276-277`), and deferred while a
  radio still waits for its own configuration: the platform confirms again after that
  radio, so one `onewifi_cb` covers the device.
- **Topology Query.** Answered, whatever the sender, only when every radio is at least
  `onewifi_bssconfig_ind`, with the confirmed BSS list in the Response; then every radio
  is `topo_synchronized` and `onewifi_cb` finishes.
- **D5 restart** (`restart-discovery-on-m2-timeout`). On the proto tick, under the same
  gate as the Search, a radio in `wsc_m2_pending` whose whole seconds since its M1 hand-off
  exceed 10 (`as_secs() > 10`, so the restart comes at 11 s) clears its session and returns to `unconfigured`; the
  `search-until-found` reset of `dev_init` now also covers `wsc_m2_pending`, so the
  same `dev_init` searches again and its 10 s cancel cannot happen in an S2 flow.
- **`onewifi_cb` timeout** (`onewifi-cb-timeout-restart`). More than 10 whole seconds with
  no Query cancels it, returns every radio to `unconfigured`, clearing their sessions, and
  cancels any queued or running `cfg_renew` of those radios (`em-orch`'s scoped cancel
  reaches pending stages), so a renew accepted earlier cannot start after the device is
  synchronized, send an M1 no Query answers and time out into a third `dev_init`.
- **Recovery** (`renew-timeout-restart`). When a radio is `unconfigured` and no `dev_init`
  is active or pending, the core cancels any `cfg_renew` of the radios and submits a new
  one. This follows the `onewifi_cb` timeout
  and a renewed radio's lost M2 or `cfg_renew` timeout. A new `dev_init` starts every
  radio in `unconfigured`, as upstream (`em.cpp:112-113`), so a radio that was already
  configured is onboarded again. The `restart discovery` line carries a reason only when the
  pass that cancelled `onewifi_cb` or `cfg_renew` left a radio `unconfigured`; otherwise the new
  `dev_init` logs `a radio is unconfigured and no dev_init is active`.
- **Renew** (`cfg_renew`). `AgentCore` picks the first radio of the Renew's band whose
  recorded controller AL MAC is not another one, refuses while a `cfg_renew` is in
  progress or the radio is in `autoconfig_renew_pending`, `wsc_m2_pending` or
  `wsc_m2_processed` (`em_agent.cpp:2400-2447`), and otherwise submits `cfg_renew`. Its
  start clears the session and sets `autoconfig_renew_pending`; the proto tick sends M1
  and enters `wsc_m2_pending`; it finishes at `owconfig_pending`.
- **WSC sessions** live in `AgentRules` and are cleared when a radio is set to
  `unconfigured` or `autoconfig_renew_pending` by any effect, when M2 is accepted, when an
  M1 fails validation and at the D5 restart.
- The orchestrator destroys a finished stage one pass later and starts the next one a pass
  after that, so a follow-up command (`onewifi_cb` after the confirmation) starts about
  500 ms after its trigger.

Log lines for the Docker checks: `M1 sent` (radio, mid, mac, build_us), `M2 accepted` (radio, mid,
bss, check_us), `M2 rejected` (radio, mid, reason), `platform confirmed` (bss), `onewifi_cb
submitted`, `Topology Query answered` (controller), `topo_synchronized` (radio),
`restart discovery` (reason `no M2 within 10 s of M1`, `onewifi_cb timeout` or
`cfg_renew timeout`), `renew accepted` and `renew refused` (radio, state), `platform
applied a radio configuration` (radio, bss). `dev_init submitted` is logged for each
`dev_init`, so a recovery shows as a second line.

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
- SIGTERM or SIGINT: the agent stops as described in "The loop" below and exits 0. A rejected
  registration, an ended transport or a changed AL MAC logs an error and exits 1.

### The loop (`src/actor.rs`)

One `em-event-loop` loop per process (0016 D2, D3b) drives two handlers: the transport
(`TransportHandler`, `em-transport`) and `AgentCore`. The sources, in order: the data socket
read (up to 64 frames per pass), the registration, the application channel (platform
confirmations, and the stop), a received SDU for the role (one per pass), and the data socket
write. The timers are the transport's and the role's `next_deadline()`.

- **Link changes.** After every turn the loop compares the transport's link with the one the
  role was last told, and tells the role the difference in order. A new session of a registered
  link gives `unregistered()` and then `registered()`, also when the loop never saw the link
  lost in between.
- **Frames.** A role frame goes straight into the transport's outbound buffer for the current
  session, stamped with that session and the recorded AL MAC. The buffer holds 4 MiB and drops
  the oldest frame not yet started (difference `tx-buffer`). There is no separate pending queue.
- **Stop.** A forwarder future moves the platform's confirmations into the application channel
  and drops its sender when `stop` completes; the closed channel is the stop signal. The role
  gets no further input. The transport then writes the frames it has queued for at most 900 ms
  and keeps its sockets open for 100 ms. `run` returns `Ok` and logs `transport stopped`. A
  closed platform channel is not a stop.
- **Errors.** A transport that ends without a stop returns `ActorError::Transport` with its
  error. A changed AL MAC returns `ActorError::AlMacChanged` at once, without the transport
  stop.

Log lines the Docker checks can match (target `em_agent::actor`): `registered` (session,
al_mac), `new session, same AL MAC`, `link lost, waiting to register`, `transport stopped`, and
`transport ended with an error while stopping` (warn). `dev_init submitted`, `Search sent` and
`radio found controller` come from the core. `em-agent stopped` (error) is logged by `main`.

### Tests

The package has 45 library tests and 10 in `tests/actor.rs`.

`tests/actor.rs` runs the agent against a fake AL-SAP service over UNIX sockets in a
temporary directory (a trimmed copy of `em-transport`'s test support), in paused time:
registration and Searches with the recorded AL MAC, a Response per radio stopping the
Searches, the same AL MAC on a new session (the log shows the lost link first), a different
AL MAC (fatal), a rejected registration (error result, also on a reconnect), the stop path
(under 1 s), a stop with frames queued (under 1 s), and an M2 per radio reaching the
platform, whose single confirmation lets the agent answer a Topology Query.

The unit tests in `src/actor.rs` check the link-change function on its own (including a
loss the loop never saw), a role frame reaching the transport, the reconnect order, a changed
AL MAC, the role getting no input after the stop, and a stop with a frame in flight (within
1 s), on a real `LoopCore` with a clock the test moves.

The core tests (`src/tests.rs`, virtual time) add a scripted controller that builds M2 with
`em_core::wsc::build_m2`: two radios through to `topo_synchronized`, one lost M2 and the
D5 restart with exactly one `dev_init`, the `> 10 s` boundary (none at 10.9 s, restarted by
12 s), no `dev_init` cancel in a 60 s lost-M2 run (the rewritten S1 test
`after_both_responses_the_cancel_resets_only_the_last_candidate`), a rejected then valid
M2, the `onewifi_cb` timeout restart, a renew refused in each listed state and accepted
otherwise, a renew followed by a lost M2, and two renews from the controller's 40 s
timeout completing through the `onewifi_cb` restart, no stale `cfg_renew` in 60 s after
that restart synchronizes, and a renew queued behind a running `dev_init` that runs once that `dev_init` is done
with its radio, when no recovery is involved (upstream behaviour).

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
- The stop takes at most 1 s (900 ms of writes, then 100 ms of linger; 0016 D3b, U8). Tests
  run it on the virtual clock.
- A transport that ends without a stop is always an error exit (1), even when it returned
  `Ok`: the agent did not ask it to stop.
- `--help` is not special: it is an unknown option and exits 2.
- `AgentConfig` has no reconnect delay: the transport's default of 2 s applies.
- A renew compares the controller AL MAC in its AL MAC TLV with the one the radio
  recorded, as upstream (`em_agent.cpp:2411-2430`), not the transport's remote address;
  the controller sends both equal. A radio with no recorded controller records the
  Renew's sender, so its M1 has an address.
- Upstream drops a callback while an `onewifi_cb` runs; the port also defers the
  submission while a radio waits for its configuration. A confirmation that finds a
  radio in `autoconfig_renew_pending` or below `owconfig_pending` therefore starts no
  command, and the platform's next confirmation does.
- `FakePlatform` confirms after every apply once all radios have a configuration, so a
  restart of both radios sends an early confirmation after the first radio; the core
  defers it as above.
- The confirmation channel holds 8 entries and the fake drops a confirmation if it is
  full (logged at `warn`). The fake sends at most one per applied configuration.
- A Topology Query is answered from the stored confirmed list; a radio with no BSS (a Tear
  Down M2) is reported with none.
- A `cfg_renew` still queued or running when the recovery starts is cancelled for every
  radio (`onewifi-cb-timeout-restart`, review finding S2-ADV-01); a renew queued behind the
  first `dev_init` with no recovery runs after onboarding, as upstream.
- The recovery's new `dev_init` also restarts configured radios (see above); a
  renew test shows the healthy radio going through discovery again.
- `AgentRules::new` is no longer `const`, since it owns the session table.
- `Platform` gained two methods, so an adapter written for S1 needs them.
