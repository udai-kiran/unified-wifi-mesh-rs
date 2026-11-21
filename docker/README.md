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

## Layout

- `Dockerfile`: stage A builds `ieee1905-rs` at commit `a6443225db82f7e8aed1c29644d2ed0b947b4d6e`, fetched by
  hash and verified with `git rev-parse`, `cargo build --release --locked -p ieee1905` (default features) on
  `rust:1.91`. Stage B builds the probe on `rust:1.85` from the repository as build context. The runtime stage
  is `debian:bookworm-slim` with both binaries. All three share Debian bookworm, so the glibc matches.
  `Dockerfile.dockerignore` trims the build context.
- `entrypoint.sh`: runs `ieee1905` in a restart loop (logging to `$LOG_DIR/ieee1905.log`, pid in
  `ieee1905.pid`), then execs the probe. Environment: `ROLE` (`agent` or `controller`, required), `IFACE`
  (default `eth0`), `CONTROL_PATH`, `DATA_PATH`, `LOG_DIR` (default `/var/log/s0`) and `FIFO` (default
  `/run/s0/probe.fifo`). The socket defaults are upstream's per role: agent `/tmp/al_control_socket` and
  `/tmp/al_data_socket`, controller `/tmp/al_em_ctrl_control_socket` and `/tmp/al_em_ctrl_data_socket`.
- `compose.yaml`: `node1` (agent, `02:42:c0:a8:65:02`) and `node2` (controller, `02:42:c0:a8:65:03`) on the
  bridge `em-s0-net` (192.168.101.0/24). `cap_add: NET_RAW, NET_ADMIN`, not privileged; `init: true` reaps the
  `ieee1905` processes that check 5 kills. `ieee1905-rs` takes its AL MAC from the interface MAC, so the fixed
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
