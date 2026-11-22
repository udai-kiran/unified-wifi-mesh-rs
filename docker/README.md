# Docker harness (S0 and S1)

Two harnesses share one image, one compose file and two nodes: S0 (below, `run-s0.sh`) and S1 (section
"S1 harness", `run-s1.sh`). `HARNESS_MODE` in the compose file selects what the entrypoint runs; the scripts
set it.

# S0 Docker harness

Test harness for slice S0 (`docs/0011-em-transport-s0.md` D4, exit checks 1 to 5). It runs two nodes, each
with the real `ieee1905-rs` and the `alsap_probe` example of `em-transport`, on one Docker bridge network.
Not part of `cargo test`. x86_64 with Docker and Docker Compose v2 only.

## Run

From anywhere:

```sh
docker/run-s0.sh            # build, start, run checks 1 to 5, tear down
KEEP=1 docker/run-s0.sh     # leave the containers up afterwards
```

The script exits non-zero if any check fails. On failure it prints the last 30 lines of each log file of both
nodes. Waits are bounded: 60 s for registration, 10 s for each probe line that a check reads (`SENT`, `RECEIVED`,
`DISCONNECTED`), and learning ends 35 s after both probes have printed `REGISTERED` (in check 5, 35 s after the
`REGISTERED` line that follows the restart). Output is one line per check:

```
PASS 1 registered
PASS 2 learning (node1: discovery logged; node2: discovery logged)
PASS 3 unicast node1->node2 ap-cap-query
...
FAIL 4 multicast-search: no 'RECEIVED ...' in the probe log of em-s0-node2 within 10s
```

Logs inside each container are in `/var/log/s0/`: `probe.log` (probe stdout, the events the checks read),
`probe.err`, `ieee1905.log`, `supervisor.log`. The probe takes commands from the FIFO `/run/s0/probe.fifo`:

```sh
docker exec em-s0-node1 bash -c 'echo "send multicast search" > /run/s0/probe.fifo'
```

The third node (`node3`, an agent) is behind the compose profile `relay` for S3 and is not started by the
script: `docker compose -f docker/compose.yaml --profile relay up -d`.

## S1 harness

`run-s1.sh` runs exit checks 1 to 6 of `docs/0012-s1-discovery.md` D8 with the real binaries: `em-agent` in node 1
(radios `02:00:00:00:01:01` on 2.4 GHz and `02:00:00:00:01:02` on 5 GHz) and `em-ctrl` in node 2, over the real
`ieee1905-rs`. Check 7 of D8 (workspace checks on stable and 1.85.0) is cargo, not this script.

```sh
docker/run-s1.sh            # build, start, run checks 1 to 6, tear down (about 4 minutes)
KEEP=1 docker/run-s1.sh     # leave the containers up afterwards
```

Output is one line per check, printed at the end with the evidence (line numbers, times, counts) or the reason:

```
PASS 1 both register (em-agent registered at line 3 with AL MAC ..., em-ctrl at line 3 with ...)
FAIL 5 each radio stops resending: radio 02:00:00:00:01:01 sent a Search after it found the controller: ...
```

The script exits non-zero if any check fails and then prints the last 30 lines of every log of both nodes. It
reads the roles' own log lines (`RUST_LOG=info`; tracing prints UTC timestamps, so times come from the logs, not
from when the script looked). Every search of a log starts after a line number or timestamp that the event under
test follows, and a check that finds no line fails.

Log reads fail closed. `read_log` returns the status of `docker exec`, and every helper that reads a log (`scan`,
`wait_scan`, `total_lines`, `expect_none`) passes a read failure on. A check that asserts that something did not
happen (`expect_none`, check 5's late Search, check 6b's second `peer model created`) fails when the log cannot be
read, so an unreadable log never counts as a quiet one. The absence checks of one observation window read each role
log once and judge all their needles against that snapshot. Check 2's search for `dev_init cancelled` ends at the
later of the two `radio found controller` lines, because the normal cancel comes after discovery.

### Role processes and the supervisor

In `HARNESS_MODE=s1` the entrypoint keeps `ieee1905-rs` under its existing restart loop and adds a second loop for
the role process (`em-agent` for `ROLE=agent`, `em-ctrl` for `ROLE=controller`). The role process does not start with
the container. `role-ctl` (this directory's `role-ctl.sh`, installed in the image) tells the loop what to do through
state files in `/var/log/s0`: `role.args` (the arguments), `role.stop` (present while the process is meant to be
down), `role.stopped` (the supervisor's acknowledgement of `role.stop`) and `role.pid`. After the entrypoint has
removed any old `role.args`, `role.pid`, `role.stopped` and `role.ready` and created `role.stop`, it writes the marker `role.ready` in the same directory; `run-s1.sh` waits for it on each
node (at most 30 s, otherwise check 1 fails) before the first `role-ctl start`. Use it with `docker exec`:

```sh
docker exec em-s0-node1 role-ctl start --radio 02:00:00:00:01:01,0 --radio 02:00:00:00:01:02,1 --profile 3
docker exec em-s0-node1 role-ctl restart --radio 02:00:00:00:01:01,0 --radio 02:00:00:00:01:02,1 --profile 2
docker exec em-s0-node1 role-ctl stop
docker exec em-s0-node1 role-ctl pid
```

`stop` creates `role.stop`, sends SIGTERM (SIGKILL after 5 s) and returns only when the supervisor has acknowledged
by writing `role.stopped` (it times out with an error after 15 s). The supervisor writes it only after it has
reaped its child and seen `role.stop`, and does not launch the role while `role.stop` exists, so the role cannot
start after `stop` has returned, even if `stop` raced a restart. `start` removes `role.stopped` and `role.stop`
and returns when the new process runs; `restart` is both. Neither touches `ieee1905-rs` or the container. A role process that exits by itself is
restarted after 1 s and the exit is recorded in `supervisor.log`. The role's log is appended to
`/var/log/s0/em-agent.log` or `em-ctrl.log` across restarts (each process logs `... starting` first), so the checks
anchor to line numbers. `ieee1905-rs` keeps logging to `ieee1905.log`.

### What each S1 check proves

1. **Both register.** Each role process logs `registered session=... al_mac=...` with its container's MAC.
2. **Searching past 10 s.** The script starts `em-agent`, waits for its first `Search sent`, waits 16 s more, then
   starts `em-ctrl`. The log times must show `em-ctrl starting` at least 15 s after that first Search. For each
   radio, before it found the controller: at least 15 `Search sent` lines, consecutive gaps between 0.5 s and
   1.6 s, MIDs strictly increasing, and more than 10 s from its first Search to its `radio found controller`. No
   `dev_init cancelled` or `Response rejected` may appear before then.
3. **Peer model, Responses and found controller within 60 s.** After `em-ctrl` registered: exactly one `peer model
   created` for node 1's AL MAC, at least one `Response sent` per band (`BAND_2_4_GHZ`, `BAND_5_GHZ`), and one
   `radio found controller ... controller=<node 2 AL MAC>` per radio, each after the controller registered and
   at most 60 s after the later registration (the later one is the controller's).
4. **Profiles.** The agent's parsed Responses (`controller found ... profile=3 capability=Some(ControllerCapability
   { kib_mib_counter: true, ... })`, one per radio) show profile 3 and the KiBMiB bit, and every `Response sent` has
   `profile=Some(3)`. Later the script restarts `em-agent` with `--profile 2` through `role-ctl restart`; after
   that restart both radios must find the controller with `profile=2` in the parsed Response and the radio line,
   every `Response sent` after the restart carries `profile=Some(2)`, and there is no `Response rejected`. The
   existing `controller found` line logs the profile and the Controller Capability.
5. **Resending stops.** For each radio, no `Search sent radio=<ruid>` after its `radio found controller` line,
   read 14 s after that line. The 13 s window covers the `dev_init` time limit: the cancel comes 10 s after the
   last tick that saw a searching node, on a 250 ms tick in whole seconds (0012 D2, up to 11.25 s), and a Search
   from it would show within one more resend period.
6. **Restarts.** (a) Before any kill, the agent log holds exactly one `dev_init submitted` (the agent logs it at
   `info` on every submission, with the chain id). Then, for each node in turn, after both radios found the
   controller: the script takes a baseline line number in both role logs, kills `ieee1905-rs` (its loop restarts
   it), waits for `new session, same AL MAC` with that node's AL MAC in the role log, then waits 8 s (several 1 s
   proto ticks). In the agent log, after the baseline taken before that kill (not after the new-session line):
   no `Search sent`, no `radio found controller`, no `controller found`, no `dev_init submitted`, no `dev_init
   has no candidates`, no `em-agent starting`, no `Response rejected`. In the controller log, after its baseline:
   no `peer model created`, no `Response sent`, no `em-ctrl starting`. The `ieee1905` pid changed, and the pids
   of both role processes did not.
   (b) The script restarts `em-agent` (profile 3): the first `Search sent` after its `em-agent starting`
   line has `mid=1`; the controller logs `existing peer model agent=<node 1 AL MAC>` with a log time after that
   `em-agent starting` and no `peer model created` for it; both radios log `radio found controller`.

The script restarts `em-agent` twice, once with `--profile 2` (check 4) and once with the default profile (6b).

## Layout

- `Dockerfile`: stage A builds `ieee1905-rs` at commit `a6443225db82f7e8aed1c29644d2ed0b947b4d6e`, fetched by
  hash and verified with `git rev-parse`, `cargo build --release --locked -p ieee1905` (default features) on
  `rust:1.91`. Stage B builds the probe, `em-agent` and `em-ctrl` on `rust:1.85` (the workspace MSRV) from the
  repository as build context. The runtime stage
  is `debian:bookworm-slim` with both binaries. All three share Debian bookworm, so the glibc matches.
  `Dockerfile.dockerignore` trims the build context.
- `entrypoint.sh`: runs `ieee1905` in a restart loop (logging to `$LOG_DIR/ieee1905.log`, pid in
  `ieee1905.pid`), then, in `HARNESS_MODE=s0` (default), execs the probe; in `s1` it runs the role supervisor
  described above (it writes `role.ready` after it has created `role.stop`, and `role.stopped` whenever it is idle with `role.stop` present). `role-ctl.sh` is the control script
  of that supervisor. Environment: `HARNESS_MODE` (`s0`
  or `s1`), `ROLE` (`agent` or `controller`, required), `IFACE`
  (default `eth0`), `CONTROL_PATH`, `DATA_PATH`, `LOG_DIR` (default `/var/log/s0`) and `FIFO` (default
  `/run/s0/probe.fifo`). The socket defaults are upstream's per role: agent `/tmp/al_control_socket` and
  `/tmp/al_data_socket`, controller `/tmp/al_em_ctrl_control_socket` and `/tmp/al_em_ctrl_data_socket`.
- `compose.yaml`: `node1` (agent, `02:42:c0:a8:65:02`) and `node2` (controller, `02:42:c0:a8:65:03`) on the
  bridge `em-s0-net` (192.168.101.0/24). `cap_add: NET_RAW, NET_ADMIN`, not privileged; `init: true` reaps the
  `ieee1905` processes that S0 check 5 and S1 check 6a kill. `HARNESS_MODE` comes from the environment of the
  `docker compose` call. `ieee1905-rs` takes its AL MAC from the interface MAC, so the fixed
  MACs are the AL MACs the checks expect.

## What each check proves

1. **Registered.** The probe registered with the service on its role's sockets and got the AL MAC, which must
   equal the container's fixed MAC. Proves the registration handshake against the real service.
2. **Learning.** Waits until each service logs `Inserted node from Discovery` (debug level, so the entrypoint
   starts `ieee1905` with `--filter ieee1905=debug`), for at most 35 s after both probes have printed `REGISTERED`. It
   fails if a service logs no such line by then. The service delivers nothing from a sender it has not learned
   (0011, "Peers must be learned"), so this wait comes before check 3.
3. **Unicast.** An AP Capability Query (0x8001) goes from each node to the other. The receiving probe must
   print `RECEIVED` with `remote` equal to the sender's AL MAC and the message id of the command just sent
   (the id comes from the sender's `SENT` line). Proves SDU encode, decode and address mapping
   through the real service and a real bridge, in both directions.
4. **Multicast.** The agent probe sends an AP-Autoconfiguration Search (0x0007) to `01:80:c2:00:00:13`, and
   node 2 must receive it from node 1's AL MAC. Proves the Docker bridge forwards that group address.
5. **Service restart.** Kills `ieee1905` in node 2 (the loop restarts it). The probe must print
   `DISCONNECTED` and then `REGISTERED` again, then, after a relearn wait like check 2, both unicast
   directions of check 3 pass again. Proves the reconnect path against a real restart. The relearn result is
   printed as an `INFO 5` line and does not fail the run.

## Known limits

- Checks 3, 4 and 5 match the probe log's `RECEIVED` line by `local`, `remote`, `type` and the CMDU message id
  of the command just sent, so an older message cannot satisfy them.
- Check 4 sees only that node 2 received the Search. It does not test the Search's TLV content.
- `ieee1905-rs` needs Rust 1.91 or later: it calls `str::floor_char_boundary` (`ieee1905-core/src/cmdu_codec.rs:1231`,
  0011 erratum E1). Stage A is pinned to `rust:1.91`.
- Two recorded transport limits do not come up here. An SDU whose reserved permit is still held when the 100 ms
  linger ends is lost without a count (0011 A6); in a session the linger starts after the stop's writes finish.
  The probe sends each command at once and holds no permit. A
  write can block without limit if the service stays alive and stops reading (0011 A7), for example a paused
  or wedged `ieee1905-rs`; a dead service makes the write fail. An aborted transport task skips its final
  counts.
- Bridge networking only. No real radio, no VLAN, no ARM. Relay (three nodes) is for S3.
- The build needs network access to github.com and the crates.io index.
