# 0018: Backhaul link gate: no CMDUs and no running role timers while the uplink is down

| | |
|---|---|
| Status | **Deferred** (2026-11-25, user: "skip for our initial phase"), rev 2. User decisions L1 to L5; plan review done (rev 2 applies it); not accepted, not implemented |
| Date | 2026-11-25 |
| Follows | [0016](0016-sans-io-cores.md) D2, D3b, D5, U10, U27, E12, E15; [0012](0012-s1-discovery.md) D9; [0013](0013-s2-onboarding.md) D4, D5 |
| Amends | None while deferred. If accepted: 0012 D9 (what "registered" means to the role: AL-SAP registered **and** the backhaul link usable); 0016 D3b (the application channel also carries carrier samples) |
| Affects | `em-transport` (new pure gate module and policy constants), `em-agent` and `em-ctrl` (app, CLI, one agent core rule), `docker/` (role arguments, a new link check script) |

## Context

The role processes talk to the `ieee1905-rs` daemon over AL-SAP. The daemon forwards our CMDUs on one interface.
Today nothing in the chain tells the role that the link under that interface is down. The findings below were
collected on 2026-11-25 (tracking folder `link-state/contract.md`, "Step 1 result").

**The daemon** (`ieee1905-rs` at `a6443225`, the commit `docker/Dockerfile` pins):
- It reads its interface once at start-up and opens its packet sockets once (`main.rs:119-121`, `:182`, `:235`). It
  has no netlink, carrier or operstate watch.
- A failed send is logged inside the daemon only (`ethernet_subject_transmission.rs:110`); the caller has already
  been told Ok (`:163-176`). AL-SAP has no message for link state (`registration_codec.rs:28-31`, `al_sap.rs:364+`).
- It sends a Topology Discovery every 30 s (`cmdu_proxy.rs:44`) and removes a neighbour not heard for 60 s
  (`topology_manager.rs:775-780`).

**Upstream** (`ba4180b`) has no carrier, netlink or operstate handling anywhere. Under `AL_SAP` it never touches the
interface (`src/em/em.cpp:789-813`). There is no upstream behaviour to keep.

**The specs** (read through usp-spec):
- EasyMesh §5.2.6 (p.30): "If an Enrollee Multi-AP Agent detects that the link status of a Multi-AP Logical Ethernet
  Interface transitions to LINK_UP, the Enrollee Multi-AP Agent shall search for the Multi-AP Controller by sending
  1905 AP-Autoconfiguration Search messages per section 6.1." On LINK_DOWN an agent onboarded over Ethernet associates
  to the backhaul BSS (Wi-Fi backhaul is not ported).
- IEEE 1905.1 §7.2 to §7.4 (pp.58-59): a CMDU is transmitted only on interfaces "in the state of 'PWR_ON' or
  'PWR_SAVE'".
- IEEE 1905.1 §10.1.1, §10.1.2 (pp.70-71): the registrar search starts after an interface is authenticated, and
  restarts if configuration does not complete. No interval.
- EasyMesh §14.1 (p.97): a backhaul STA uses four-address frames once onboarded.
- Silent: pausing timers while a link is down, hold-down (debounce) values, a Search retry interval, controller
  aging of an agent that disappeared.

**The Docker experiment** (scenarios s1 to s7, both roles, `ip link set eth0 down/up`, the host veth peer, `docker
network disconnect`):
- The roles never learn about the link. AL-SAP registration survived every scenario.
- The agent keeps sending 2 Searches a second during an outage; all are lost. No burst of sends after the link
  returns, because no timer paused.
- Recovery cost: about 1.5 to 3 s; or the 10 s M2 wait (0013 D5) when an M2 was lost; or about 30 s after an
  outage over 60 s (the daemon aged the neighbour out and drops the controller's Responses with "node not found"
  until the next Topology Discovery).
- After an admin up the daemon reads again only 1.1 to 1.8 s later. With carrier loss and the interface still
  enabled the daemon logs nothing at all.
- A removed and re-added interface breaks the daemon until it restarts; the new interface has a new MAC.
- `/sys/class/net/<if>/carrier`: `1` when up; `0` on carrier loss with the interface enabled; the read fails with
  "Invalid argument" (EINVAL) when the interface is disabled; the file is missing when the interface is removed.

**Wired and wireless:** a Linux bridge does not take a Wi-Fi station interface as a port unless it is in
four-address mode, and the backhaul STA is a plain three-address client while it onboards. A bridge also shows
carrier while any port forwards, including the fronthaul AP interfaces. So the bridge's carrier says nothing about
the uplink. The gate watches the uplink interfaces themselves.

**The role cores** hold timers in two forms: due times (`proto_deadlines`, `orch_deadline`) and stored start times
compared with `now`: the 10 s command age (`em-agent/src/rules.rs:13`), the 10 s M2 wait from the M1 hand-off
(`em-agent/src/wsc.rs:26`, `lib.rs:557`), the orchestrator's stage start (`em-orch/src/lib.rs:494`, `:520-526`), the
controller's 40 s topology-sync limit (`em-ctrl/src/rules.rs:12`). The orchestrator pass runs while unregistered
(`em-agent/src/lib.rs:255`), so a command can expire during an outage today.

## Decision

### D1. One "link usable" signal: the one kick (user)

The role is **registered** when both hold:
1. the transport is registered with the daemon (today's `LinkState.al_mac` is `Some`), and
2. the backhaul gate (D2) says **up**.

The app gives the role exactly one `registered()` when both become true and one `unregistered()` when either stops
being true. It does this by masking: the app's `current` link for `link_changes` is the transport's `LinkState`
with `al_mac` set to `None` while the gate is down. The rule of 0016 E12 stays: `unregistered()` before
`registered()`, in order, after every turn. The first `registered()` (the agent's `dev_init`, the controller's AL node)
therefore happens only after the link is usable, as asked: the timers start after link-up.

**While the gate is down the role gets nothing from the link** (plan review F1). Masking alone would not do it: the
transport stays registered and keeps receiving, and the cores' `received` does not look at `registered`. So while the
gate is down the app takes each received SDU from the handler and drops it (one `debug` line and a counter, no role
call), as if there were no session. No frame from the role reaches the transport while the gate is down, and the
role's state does not move on input that arrives during the up hold-down.

### D2. The gate: carrier of the listed backhaul interfaces, with a hold-down (L2, L3, L5)

- **Interfaces** come from `--backhaul <ifname>` (D6). The **sample** is up when **any** listed interface's
  `/sys/class/net/<ifname>/carrier` reads `1`. Anything else is down: `0`, a read error (EINVAL when the interface
  is disabled), a missing file (interface removed), other contents.
- **Polling:** one sample every `CARRIER_POLL` = 200 ms.
- **Hold-down (L2):** the gate goes up after the samples have been up for `UP_HOLD` = 2 s, and down after they have
  been down for `DOWN_HOLD` = 500 ms. Counted in samples: up after `ceil(UP_HOLD / CARRIER_POLL)` = 10 consecutive up
  samples, down after `ceil(DOWN_HOLD / CARRIER_POLL)` = 3 consecutive down samples. The 2 s also covers the daemon's
  own 1.1 to 1.8 s read recovery after an admin up.
- **Start:** the gate starts **down** and needs the full up hold-down, so the role waits about 2 s after start even
  on a healthy link.
- **`--backhaul none`:** no polling; the gate is always up (today's behaviour).
- **Mechanism vs policy:** the three numbers are constants in `em-transport/src/policy.rs` with the others (0016 U15,
  U31). The counting is a pure type in `em-transport`, `CarrierGate` (L4), with no clock: it takes one boolean sample
  at a time and returns whether the gate changed. Its tests need no time at all. Reading sysfs is a small function in
  the same module, `carrier_sample(root: &Path, interfaces: &[IfName]) -> bool`; `root` is `/sys/class/net` in
  production and a temporary directory in tests (F4). The app config carries the root, defaulting to production.
- **The samples must be spaced** (F3): the poll uses `tokio::time::interval` with
  `MissedTickBehavior::Delay`, so a stalled task (for example a slow DH key build in the same task) never takes a burst
  of samples microseconds apart. The hold-down is therefore at least 2 s / 500 ms. The poll sits in the app's I/O
  module under `#[allow(clippy::disallowed_methods, reason = "0016 U27: I/O module; ...")]`, as the actors' `run` does.

### D3. Where the samples enter the loop (0016 U10)

Each app already has one future that owns the only sender of the application channel: `forward_confirmations` in
`em-agent/src/actor.rs:358-378`, and in the controller a future that only waits for the stop and drops the sender
(`em-ctrl/src/actor.rs:328-334`, channel capacity 1, polled in the same task as the loop). The controller's future
becomes a feeder like the agent's, with capacity 8. That future also takes the carrier samples: every 200 ms it
calls `carrier_sample` and sends `AppEvent::Carrier(bool)` **only when the debounced gate changes** (the `CarrierGate`
runs in the task, so at most one event per change reaches the loop). The application channel's item becomes an enum
in each app: agent `{ Confirmation, Carrier(bool) }`, controller `{ Carrier(bool) }` (today it carries nothing).
Nothing else changes: the channel stays bounded (capacity 8), it still closes when the stop completes, and there is
still one sender (no clone, 0004).

`em-event-loop` is unchanged (frozen, 0017 A3). `TransportIo` is unchanged: it already delivers application events.
The sysfs read is a short blocking `std::fs` read of an in-memory file, done in the task; tokio's `fs` feature is not
added. A send on a full channel waits inside the same `select!` as the stop, as `forward_confirmations` does, so a
stop never waits for the loop.

### D4. The role's time stops while the link is down (L1)

While the role is unregistered (D1), **the role's time stands still**. The app keeps a `RoleClock` (pure, in
`em-transport` next to the gate): `paused_since: Option<Instant>` and `paused_total: Duration`.
- The clock **starts paused** at the loop's start time (F7): the role is unregistered from the start.
- On `unregistered()` the app records `paused_since = now`.
- On `registered()` it adds `now - paused_since` to `paused_total`.
- Every `now` the app passes to the role is `role_now = now - paused_total`; while paused, `role_now` stays at
  `paused_since - paused_total`.
- `role.next_deadline()` is converted back with `+ paused_total`. While paused the role timer is off: the role gets
  no `tick`.
- Events that still reach the role while paused (a platform confirmation) are handled at the frozen `role_now`.
  Received CMDUs do not reach it (D1).
- Only the role gets `role_now`. The transport (`handler.send`, its timers) keeps getting the real `now`; the app
  holds both in separate variables so they cannot be swapped.

So every timer pauses together, without touching each stored time: due times, the 10 s command age, the M2 wait,
the stage start, the 40 s topology sync, and any timer added later. An exchange in progress resumes with the time it
had left. The measurement clock of the D9 timings (`measure_now`, 0016 E15) is real time and is not shifted. The
transport's timers (reconnect, write stall, stop) keep real time.

This replaces today's partial rule ("no proto tick while unregistered; the orchestrator pass still runs"). It applies
to every unregistered period, so an AL-SAP reconnect (the daemon restarted) pauses the role too.

Arithmetic: `Instant - Duration` can panic below the platform's earliest instant; `paused_total` never exceeds
`now - start`, but the code uses `checked_sub` and keeps the last value on `None` (documented, tested).

### D5. Agent: radios still waiting for the controller start again from Search on link-up (L1, EasyMesh §5.2.6)

On every `registered()` after the first, each radio in `Unconfigured`, `AutoconfigRspPending` or `WscM2Pending` is
restarted at once: its WSC session cleared, its state set to `Unconfigured`, its proto deadline due now. This is the
path the M2 wait already uses (`lib.rs:560-566`), with `reason = "link up"`. A Search therefore goes out at the
first tick after the gate opens, as §5.2.6 asks, instead of after the remaining M2 wait.

Radios at `WscM2Processed` or later keep their paused timers: their configuration arrived, and what they wait for
next (the platform, the controller's Topology Query) resumes when the controller's time resumes. The controller has
no such rule: its timers pause and resume (D4).

This applies to AL-SAP reconnects too. A `WscM2Pending` radio under `cfg_renew` goes through the existing
`reopen_search` path ("a radio is unconfigured and no dev_init is active", then `dev_init submitted`), as the M2 wait
does today. Otherwise `dev_init` stays active (its start is reset while a radio is before M2, `rules.rs:218`), so no
second `dev_init` is submitted.

### D6. `--backhaul <ifname>` on both roles (L5)

- **Required** on `em-agent` and `em-ctrl`. Repeatable. `--backhaul none` turns the gate off and cannot be combined
  with an interface name.
- Agent: the uplink interfaces (Ethernet uplink port; later the backhaul STA). Controller: its LAN-side port or
  ports toward the agents.
- **Validation** (the name becomes a sysfs path; the rules of the kernel's `dev_valid_name`): 1 to 15 bytes
  (`IFNAMSIZ - 1`); not `.` or `..`; no `/`, no `:`, no NUL, no ASCII whitespace; no duplicates. A bad name is a usage error at start-up (exit 2, as other usage errors).
- `AgentConfig` and `CtrlConfig` get `backhaul: Backhaul` (`None` or a list of validated names).
- The interface is **not** checked to exist at start-up: an interface that appears later is picked up by the next
  sample.

### D7. Logging

- The app logs each gate change at `info`, target `em_agent::actor` / `em_ctrl::actor`: `backhaul link up
  interfaces=...` and `backhaul link down interfaces=...`. With `none`, one line at start: `backhaul gate off`.
- The existing lines stay word for word (`registered session=`, `new session, same AL MAC`, `link lost, waiting to
  register`); a carrier down now also gives the last one, and a carrier up on the same session gives `new session,
  same AL MAC` with an unchanged session number (F8). The `backhaul link up` line just before it tells the two apart.
- Each SDU dropped while the gate is down (D1): one `debug` line.

### D8. Known limits (not fixed here)

1. **Outage over 60 s:** the daemon has aged the neighbour out and drops the controller's Responses until the next
   Topology Discovery, up to 30 s more. Daemon behaviour; to raise with `ieee1905-rs`.
2. **Interface removed and re-added:** the daemon keeps sockets on the old interface and never recovers; the new
   interface may have a new MAC, which changes the AL MAC at the daemon's restart (`AlMacChanged`, exit 1, 0012 D9).
   The gate shows down for as long as the interface is missing. A daemon restart with a different AL MAC while the
   gate is down is reported (`AlMacChanged`, exit 1) only when the gate comes up, because the role is told nothing
   until then.
3. **Wireless backhaul:** the daemon forwards on one interface. A backhaul STA that is not in the bridge (three-address,
   during onboarding) cannot carry CMDUs through it. To be decided with the Wi-Fi backhaul slice; for a station,
   carrier is expected to follow association and authentication (not verified).
4. **Frames already handed over:** CMDUs sent in the up to 500 ms (+ one poll) before the gate notices a down are
   lost, as today.
5. **Carrier is not reachability:** an up link with no path to the controller is not detected.

## Intentional differences

| id | Upstream | Port | Test names the upstream behaviour |
|---|---|---|---|
| `backhaul-link-gate` | No link state; sends whenever AL-SAP is registered (`em.cpp:789-813`) | D1, D2: role registered only while the backhaul carrier is up after the hold-down | yes |
| `role-time-paused` | Timers run on wall time throughout | D4: role time stands still while unregistered | yes |
| `link-up-research` | No link-up rule | D5: radios waiting for the controller restart from Search on link-up (EasyMesh §5.2.6) | yes |

## Alternatives considered

- **Watch the bridge's carrier** (the first plan): the bridge is up while any port forwards, including fronthaul APs;
  a station cannot be a bridge port before four-address mode. Rejected for the reasons in Context.
- **netlink notifications** (L3): immediate, but a new dependency or a hand-written netlink socket. Polling adds at
  most 200 ms to a 500 ms hold-down. Rejected.
- **Shift every stored time on link-up** (the first L1 design): needs every timestamp found and shifted, in three
  crates, and any timer added later must remember it. Replaced by D4, which gives the same result in one place.
- **Pause everything, including onboarding radios** (L1 as first decided): a radio whose M1 was lost would wait up to
  10 s more after link-up for an M2 that never comes. Replaced by D5.
- **The gate in the role cores:** the cores would need a carrier input and a hold-down timer. Rejected: the cores
  only need "registered", which they already have; the gate is I/O policy, kept in the apps and `em-transport`.
- **A separate gate copy per app** (L4): rejected, one copy in `em-transport` used by both.
- **No gate on the controller:** rejected by the user; same flag, its LAN-side ports.

## Acceptance criteria

1. `em-transport`: `CarrierGate` (hold-down counting), `carrier_sample` (sysfs read, any-up, errors are down),
   `RoleClock` (D4), `IfName` validation; the three constants in `policy.rs`. Unit tests, in samples: up after exactly
   10 up samples, down after exactly 3 down samples; 2 consecutive down samples and 9 consecutive up samples change
   nothing; start state down. `carrier_sample` with a temporary directory as `root` (built under
   `std::env::temp_dir`, no new dependency): `1`, `0`, a `carrier` that is a directory (a read error even as root),
   missing, two interfaces one up. `RoleClock`: starts paused, pause and resume, two outages summed, deadlines
   converted back (deadline equal to `now` is due), no panic near the earliest instant.
2. Both apps: masking (D1), dropping received SDUs while gated, the event, the `RoleClock`. App tests with the
   existing test rigs:
   - carrier down sends `unregistered()` once and stops the role timer; carrier up sends `registered()` once;
   - a carrier change and an AL-SAP reconnect in the same turn give the E12 order;
   - a CMDU arriving while the gate is down (a Search for the controller, a Topology Query for the agent) gives no
     write and no role state change;
   - the transport gets the real `now` while the role gets `role_now`;
   - with `none` the behaviour is today's.
   Feeder tests (tokio test-util, paused time): exactly one `Carrier` event per debounced change; a stop completes
   while the channel is full and a `Carrier` send is pending; the channel closes after the stop (both apps).
3. Agent core (D5): a radio in `WscM2Pending` restarts from Search on a later `registered()`; a radio at
   `WscM2Processed` or later does not; the first `registered()` does not restart anything.
4. Timer pause (D4), with a fixed clock: an M2 wait with 4 s left resumes with 4 s left after an outage of 60 s; a
   command 3 s old is still 3 s old after the outage; the controller's topology-sync limit likewise.
5. CLI: `--backhaul` required, repeatable, `none` alone; each validation rule rejects its bad name. Existing tests are
   kept with the argument added (F6): em-agent `cli.rs` `defaults_are_upstreams_agent_sockets_and_profile_3` and
   `every_option_is_read_and_radios_repeat`; em-ctrl `cli.rs` `defaults_are_upstreams_controller_sockets`; em-ctrl
   `tests/binary.rs` (both tests keep exit 1 and "the reset file is required" with `--backhaul none` added); the
   `AgentApp`/`CtrlApp` test rigs and `tests/actor.rs` use `Backhaul::None` except the new gate tests. Every role
   start in `docker/` gains `--backhaul eth0`: `run-s1.sh` (the `role-ctl restart` of check 4 and both `role-ctl
   start` lines), `run-s2.sh` (both `role-ctl start` lines).
6. Planted bugs, each shown to fail its test: no masking; received SDUs passed to the role while gated; the gate
   counts one sample instead of 10; `role_now` not paused; the transport given `role_now`; D5 restarting a
   `WscM2Processed` radio; a `/` accepted in a name.
7. Docker: `run-s0.sh`, `run-s1.sh`, `run-s2.sh` pass with `--backhaul eth0`. `run-s1.sh` check 6a (no Search after
   an `ieee1905` kill) relies on both radios being past `WscM2Pending` at that point (F10); the implementer confirms it
   from the log, and if not, the check is changed with the user's agreement. A new `docker/run-link.sh`; every flap
   runs inside one in-container shell, so the timing is not stretched by `docker exec`:
   1. Agent started with eth0 down: no `Search sent` before `backhaul link up`, which is at least 2 s after the up;
      then both radios `topo_synchronized`.
   2. M2 lost deterministically (F5): em-ctrl with `--test-drop-first-m2`; the agent's eth0 goes down right after the
      first radio's `M1 sent` and stays down 15 s. Expected: no `restart discovery` during the outage; `restart
      discovery reason="link up"` for that radio after `backhaul link up`; both radios `topo_synchronized` within
      6 s of `backhaul link up`.
   3. Five dips of 300 ms down, 300 ms up in one shell loop: no `backhaul link down` line.
   4. Controller pause (F2): em-ctrl's eth0 goes down while a radio is `TopoSyncPending` (right after its `M2 sent`),
      for 50 s (more than the 40 s limit). Expected: `backhaul link down` and `backhaul link up` on em-ctrl; no
      `Topology Query sent` and no `em_config cancelled: topology sync did not complete within 40 s` during the
      outage; both radios `topo_synchronized` after the up.
8. Checks green on stable and 1.85.0; workspace tests pass; clippy bans of 0016 U27 still hold (no clock read in the
   gate, the `RoleClock` or the cores).
9. Docs: this note accepted; 0012 D9 and 0016 D3b amendments recorded (0012 A-note, 0016 erratum E16); STATUS (log,
   follow-ups for D8 limits 1 to 3), the crate READMEs, `docker/README.md`, the root README status line.

## User decisions

- **L1** (2026-11-25): role timers pause while the link is down and resume on link-up; revised the same day after
  the experiment: radios still waiting for the controller restart from Search on link-up (D5).
- **L2:** hold-down up 2 s, down 500 ms, as named constants.
- **L3:** poll sysfs carrier every 200 ms.
- **L4:** the gate once in `em-transport`, used by both apps.
- **L5:** revised after "we cannot bridge a wired and wireless interface": `--backhaul <ifname>`, repeatable, any one
  up means usable, required on both roles, `none` turns the gate off; the controller names its LAN-side ports.

## Revision history

- rev 1 (2026-11-25): first version, after the step 1 investigation and the user's L1 to L5.
- rev 2 (2026-11-25): plan review (REVISE, ten findings, all confirmed by the coordinator in the code): received SDUs
  dropped while gated (F1); controller Docker check moved to `TopoSyncPending` (F2); spaced samples (F3); sysfs root
  as a parameter and feeder tests (F4); deterministic M2 loss and in-container flaps (F5); existing tests and Docker
  call sites listed (F6); clock starts paused (F7); log and delayed `AlMacChanged` (F8); `:` and ASCII whitespace
  rejected (F9); run-s1 check 6a precondition (F10).
