# 0011: Slice S0: the `em-transport` AL-SAP client and the Docker harness

| | |
|---|---|
| Status | **Accepted** (2026-11-21, user): open choices 1 to 4 as recommended. Rev 2 follows a plan review and was not re-reviewed |
| Date | 2026-11-21 |
| Upstream | `rdkcentral/unified-wifi-mesh` @ `ba4180ba36748629f10c20babc4320dd69c9db4f` |
| 1905 layer | `rdkcentral/ieee1905-rs` @ `a6443225db82f7e8aed1c29644d2ed0b947b4d6e` |
| Follows | [0010](0010-vertical-slices-and-transport.md) D1 (slice S0), D2, D3, D13; [0004](0004-runtime-and-ownership.md) R1 to R6 |
| Amends | 0010 D1: the platform fake moves from S0 to S1 (user decision) |
| Affects | new crate `em-transport`; new `docker/` directory; workspace `Cargo.toml` |

## Context

Slice S0 builds the transport every later slice uses, and the container setup they run in. Nothing
above the transport exists yet, so S0's test client is a probe program, not `em-ctrl` or `em-agent`.

Unless marked as upstream, `file:line` cites are in `ieee1905-rs`'s `ieee1905-core/src/`. Statements
about runtime behaviour are predictions from reading the code; S0's exit checks test them.

### The AL-SAP protocol, from both sides

| Item | Format | Upstream client | `ieee1905-rs` |
|---|---|---|---|
| Framing, both sockets | 4-octet big-endian length, then that many octets | `al_service_utils.cpp:68-82` | `LengthDelimitedCodec` defaults (`al_sap.rs:179-181`); no size limit below its 8 MiB default |
| Registration request (control) | `service_operation` (0x01 enable, 0x02 disable), `service_type` (0x01 agent, 0x02 controller) | `al_service_registration_request.cpp:35-47`, `al_service_registration_enums.h:7-14` | `registration_codec.rs:29-93` |
| Registration response (control) | local AL MAC (6), `result` (0x00 unknown, 0x01 success, 0x02 not supported) | `al_service_registration_response.cpp:31-57` | always success (`al_sap.rs:283-287`); all-zero AL MAC if it cannot find its interface (`:277-282`) |
| SDU (data) | MAC A (6), MAC B (6), `is_fragment`, `is_last_fragment`, `fragment_id`, payload = one CMDU without an Ethernet header | `al_service_data_unit.cpp:40-97` | `sdu_codec.rs` |
| SDU size | 65 517 payload octets per frame (65 536 with the 19-octet header and prefix); longer payloads become numbered fragments | `al_service_access_point.cpp:138-146`, `al_service_utils.h:11`, `al_service_access_point.h:17` | `al_sap.rs:374-379` for its own sends |
| CMDU validity | a single-frame SDU must parse as a CMDU whose last TLV is a zero-length End of Message; a fragmented one is checked after joining. Anything else is logged and **dropped silently**, and the session continues | — | `sdu_codec.rs:72-106`, `al_sap.rs:596-644` |
| Connection order | connect control, then data, then send the request and read the response | constructor, then `al_sap_register` (`em_ctrl.cpp:1692-1697`, `em_agent.cpp:2999-3004`) | accepts control, then data, then reads the request (`al_sap.rs:176-181`, `:238-241`) |

**Address fields.** In both directions MAC A is the **local** AL MAC and MAC B the **remote** one.
- **Send:** upstream fills A from the frame's source and B from its destination (`em.cpp:799-813`). The
  service ignores A for routing: B is the key, and the frame's source is always its interface MAC
  (`cmdu_proxy.rs:905-921`). For multicast, B is the 1905 group address 01:80:C2:00:00:13, set by the frame
  builder (e.g. upstream `em_configuration.cpp:4546-4548`) and recognised by the service (`cmdu_proxy.rs:913`).
- **Delivery:** the service fills A with its own AL MAC and B with the frame's Ethernet source
  (`cmdu_handler.rs:994-995`). B equals the peer's AL MAC because `ieee1905-rs` uses its interface MAC as its
  AL MAC. Upstream rebuilds an Ethernet header with A as destination and B as source (`em_mgr.cpp:410-419`).
- A delivered SDU does not show whether it arrived by multicast.

### How the service behaves around a session

- **One client at a time.** After accepting one control and one data connection it drops both listeners
  (`al_sap.rs:165-181`), so a second client gets "connection refused" until the session ends.
- **Ending a session.** After registration the service never reads control again (`al_sap.rs:130-145`).
  Only end of file on data ends the session. It then waits 2 s, removes and re-binds both socket files, and
  listens again (`al_sap.rs:96-107`, `:158-168`). Between sessions the files exist with no listener.
- **SDUs just before a close can be lost.** Each received SDU is sent from a spawned task that first checks
  the session is still active (`al_sap.rs:713-720`, `cmdu_proxy.rs:892`). A close can win that race.
- **Peers must be learned.** Delivery requires the frame's source to be a known node (`cmdu_handler.rs:980`).
  A node is learned from Topology Discovery (sent every 30 s, first at start, `cmdu_proxy.rs:44`), from a
  Query, or from an AP-Autoconfiguration Search carrying an AL MAC TLV (`cmdu_handler.rs:748-769`).
- **Passive eviction (0010 D6).** A Topology Query received or sent through the client puts the node in a
  converging state, and a node in that state for 5 s is removed (`topology_manager.rs:749-772`, `:911-922`;
  `cmdu_proxy.rs:973-986`). Test traffic must therefore not use Topology Query.
- **Interleaving.** The service takes its write lock once per fragment (`al_sap.rs:426-431`), so two large
  deliveries could interleave their fragments. Its own deliveries are never fragmented in practice (each body
  is at most 65 532 octets).

### Upstream client defects not ported

- The registration response is read with one `recv`, which can return a partial frame
  (`al_service_access_point.cpp:119-121`).
- A closed socket, a fragment out of order, or an oversize length throws out of the manager loop
  (`em_mgr.cpp:427-433`), and the process stops. An oversize length throws before the body is read, so the
  stream would also fall out of step.

**Socket paths.** Upstream's controller uses `/tmp/al_em_ctrl_data_socket` and
`/tmp/al_em_ctrl_control_socket` (`em_ctrl.cpp:1797-1798`); its agent uses `/tmp/al_data_socket` and
`/tmp/al_control_socket` (`em_agent.h:34-35`), which are also `ieee1905-rs`'s defaults (`main.rs:57-61`).

## Decision

### D1. Crate `em-transport`

- A library crate, async on tokio (0004 R1). Dependencies: `tokio` (features `net`, `io-util`, `sync`,
  `time`, `macros`, `rt`) and `tracing` (0004 R5 already decides `tracing` for libraries). Dev-dependency:
  `tokio` with `test-util` for paused time. Temporary directories in tests use a small std-only helper, not
  `tempfile`. The length framing is written here rather than adding `tokio-util`.
- Depends on `em-types` (`MacAddr`). It does not parse CMDUs: the payload is opaque bytes. **Contract for
  callers:** every SDU is exactly one CMDU ending in End of Message; the service drops anything else without
  telling the client.
- The probe (D5) uses `em-msg`, a workspace crate, to build its CMDUs.

### D2. Shape: one actor task (0004 R6, 0002 D9)

> Superseded in part by A5 (link state on a `watch`, SDU-only received channel, one `spawn` returning
> `Transport`, drain-then-linger stop). The text below is the original decision.

```rust
pub struct TransportConfig {
    pub control_path: PathBuf,
    pub data_path: PathBuf,
    pub role: Role,                    // Agent | Controller
    pub reconnect_delay: Duration,     // default 2 s, matching the service's restart
}
pub struct Sdu { pub local: MacAddr, pub remote: MacAddr, pub cmdu: Vec<u8> }
pub enum TransportEvent { Registered { al_mac: MacAddr }, Received(Sdu), Disconnected }

pub fn spawn(config: TransportConfig)
    -> (mpsc::Sender<Sdu>, mpsc::Receiver<TransportEvent>, JoinHandle<Result<(), TransportError>>);
```

- The role actor holds the only `Sender<Sdu>` (single frame sender, 0004 R6) and the only receiver.
- `local` and `remote` are the only address names above this crate (0010 D3).
- **Full event channel (user decision: drop the oldest).** Received SDUs wait in a bounded queue inside the
  actor, which forwards them with `Sender::reserve()` in its `select!` and never awaits a full channel while
  reading. When the queue is full, the oldest `Received` is dropped and counted. `Registered` and
  `Disconnected` are never dropped and stay in order with the `Received` events around them. The actor never
  awaits the event channel in a way the role actor could block, so there is no send-send cycle (0002 D1).
- **Errors** (a dropped SDU, a bad fragment) go to `tracing` and to counters the actor exposes; there is no
  error event.
- The actor ends normally when the `Sender<Sdu>` is dropped, after a short linger so its last writes leave
  (see "SDUs just before a close").

### D3. Session lifecycle

1. Connect control, then data, then send the request (always enable; the client never sends disable) and
   read exactly one framed response.
2. **Retry**, after `reconnect_delay`, on: missing socket file or "connection refused" on either connect;
   end of file, reset or a short frame before the response arrives. These are expected while the service
   restarts or another client holds the session.
3. **A complete response that does not succeed (user decision):**
   - `result` other than success ends the actor with a `TransportError`, as upstream's controller exits
     (`em_ctrl.cpp:1800-1803`);
   - an all-zero AL MAC is logged and retried after `reconnect_delay`, because the service's interface may
     not be up yet. **Intentional difference:** upstream would use the zero address.
4. After `Registered`, end of file or a read error on data ends the session: `Disconnected` is sent, both
   sockets are closed, and the actor returns to step 1. **Intentional difference:** upstream stops the
   process (`em_mgr.cpp:427-433`); 0010 D2 requires reconnecting.
5. SDUs sent while disconnected are dropped and counted, not queued. The protocol's resends recover
   (0010 D3, D4).
6. **Receive errors keep the session, intentional differences from upstream:**
   - a length above 65 536 octets: skip exactly that many octets and drop the frame;
   - a fragment out of order, or an incomplete fragment set: drop it; fragment id 0 or an unfragmented frame
     starts a new SDU;
   - a frame that does not parse as an SDU header: drop it.

### D4. Docker harness (0010 D13)

- **One image per node** (keeps 0010 D13), built in stages:
  - `ieee1905-rs` at the pinned commit, fetched by hash, built with `--locked`, default features (no rbus,
    so no clang), on a Rust image of 1.88 or later (it uses let-chains);
  - the workspace's probe, on Rust 1.85;
  - a small runtime image with both binaries and an entrypoint that keeps `ieee1905-rs` running in a loop, so
    check 5 can kill it without stopping the probe.
- `docker/compose.yaml`: two nodes on one bridge network with fixed MACs and `NET_RAW` and `NET_ADMIN` (not
  `privileged`; widened only if S0 shows it is needed). A third node is a compose profile for S3.
- `docker/run-s0.sh` runs the exit checks and prints one pass or fail line each. Not part of `cargo test`.

### D5. Probe

`em-transport/examples/alsap_probe.rs` runs for the whole test: it registers with a given role and paths,
prints every event (with `local`, `remote` and the CMDU in hex), and sends SDUs on command, read line by line
from a FIFO. It stays connected across sends, so it never triggers the service's one-client refusal or the
loss at close. Test tooling, not a product binary.

### D6. Tests without Docker

A scripted fake service (two UNIX listeners in a temporary directory) covers:
- registration bytes for both roles; each `result` value; the response split across two reads;
- retry on a stale socket file with no listener, on end of file before the response, and on a zero AL MAC;
- SDU encode and decode, with bytes derived by hand from `ieee1905-rs`'s `sdu_codec.rs` field order and test
  values (its tests build structs, not byte literals);
- fragment split at 65 517 and join; out-of-order fragment; oversize length skipped and the stream still in
  step;
- the address mapping in both directions;
- reconnect after the service closes data, with paused time for the delay;
- SDUs sent while disconnected are dropped, not delivered late;
- a full event channel drops the oldest `Received` and never a lifecycle event.

## Exit checks for S0 (0010 D1)

Test messages are **AP Capability Query** (0x8001) with only End of Message. The service passes this type
through without tracking it (`cmdu_handler.rs:209-211`), so it cannot trigger eviction.

On x86_64 with Docker:
1. Both nodes register and report their AL MACs.
2. **Learning.** The script waits until each node has learned the other: more than 30 s after the later
   service start, or earlier if the service log shows the peer's Topology Discovery processed.
3. Node 1 sends a unicast AP Capability Query to node 2's AL MAC, and node 2's probe prints it with `remote`
   equal to node 1's AL MAC. Then the reverse direction.
4. Node 1's probe, registered as an agent, sends a multicast AP-Autoconfiguration Search carrying its own AL
   MAC; node 2 receives it. This confirms the bridge forwards 01:80:C2:00:00:13.
5. Killing `ieee1905-rs` in node 2 produces `Disconnected`, then `Registered` again after the restart. After
   the learning wait of check 2, check 3 passes again.
6. The workspace checks of `CLAUDE.md` pass on stable and 1.85.0.

## Open choices

**Decided by the user (2026-11-21), all as recommended:**
1. The platform fake moves to S1 (amends 0010 D1).
2. Default socket paths are upstream's per role, always overridable; the compose file passes them to
   `ieee1905-rs`.
3. A full event channel drops the oldest received SDU (mechanism in D2).
4. Registration that does not succeed: a rejection is fatal; a zero AL MAC is retried (D3 step 3).

## Alternatives considered

| Alternative | Why not chosen |
|---|---|
| `tokio-util`'s `LengthDelimitedCodec` | A new dependency for about 30 lines |
| Link `ieee1905-rs`'s `sdu_codec` | Not published; pulls its whole dependency tree (0010 D2) |
| Queue SDUs while disconnected | Replays stale protocol messages after a restart; the protocol already resends |
| `broadcast` channel for drop-oldest | Needs `Clone` on events, banned by 0004 R2 |
| A probe that sends one SDU and exits | Each exit restarts the service's session, blocks other clients while connected, and can lose the SDU |
| Topology Query as the test message | Starts the service's 5 s eviction, and in active mode never reaches the client |
| Two containers per node sharing a socket volume | Amends 0010 D13; one image with a supervising entrypoint is enough |
| `privileged: true` as in `ieee1905-rs`'s compose | Broader than raw sockets need |

## Acceptance criteria

1. `em-transport` builds with only the listed dependencies, passes D6's tests on stable and 1.85.0, and
   passes the workspace clippy, fmt and rustdoc checks.
2. The D4 harness passes exit checks 1 to 5 on x86_64, with the run's output recorded in `STATUS.md`.
3. Every upstream difference in D3 (steps 3, 4 and 6) is tagged `intentional-difference:<id>` in a test.
4. `STATUS.md`, the root README and the crate README are updated.

## Revision history

- rev 1 (2026-11-21): first draft.
- User decisions (2026-11-21): open choices 1 to 3 as recommended.
- rev 2 (2026-11-21, plan review: 10 findings, checked against the sources by the coordinator):
  - exit checks use AP Capability Query and wait for peer learning (Topology Query would have triggered
    eviction);
  - a long-running probe driven by a FIFO, because the service allows one client and can lose an SDU sent
    just before a close;
  - retryable versus fatal registration failures; the service's session behaviour;
  - one image per node with a supervising entrypoint; Rust 1.88 for `ieee1905-rs`; no clang;
  - the drop-oldest mechanism; lifecycle events never dropped; errors to `tracing`;
  - the CMDU validity contract and receive-error handling, with intentional-difference tags;
  - `tracing` as a dependency, a std temporary-directory helper, the probe using `em-msg`;
  - corrected cites.
- User decision (2026-11-21): open choice 4 as recommended. Accepted.

## Amendments

- **A1 (2026-11-21, implementation; coordinator):** D2's counters need a reader. Sharing them through an
  `Arc` would need a clone (0004 R2), so `spawn_with_counters` returns the same three values plus a
  `watch::Receiver<TransportCounters>`. `spawn` keeps D2's signature and drops the watch.

- **A2 (2026-11-21, S0 mutation check; coordinator):** the actor also ends `Ok` when its event receiver is
  dropped, in every wait, not only when the `Sender<Sdu>` is dropped. The first implementation noticed a
  dropped receiver only when an event was queued; that defect is fixed and tested.
  Exception: while a write to the data socket is blocked (the service has stopped reading), the actor does
  not see either drop until the write finishes. This is the open follow-up on blocking writes in `STATUS.md`.
- **A3 (2026-11-21, S0 review round 1; coordinator):** two differences D3 did not record. A rejected
  registration is fatal for **both** roles: upstream's controller exits (`em_ctrl.cpp:1718-1738`), but its
  agent logs the failure and starts anyway (`em_agent.cpp:3006-3018`). And retrying on a connect failure or
  an early end of file differs from both upstream binaries: the controller exits, and the agent's
  constructor throws uncaught (`al_service_access_point.cpp:36-55`). Both are tagged in tests
  (`agent-rejection-fatal`, `retry-connect`).
- **A4 (2026-11-21, S0 review round 1; coordinator):** D2 changes in four ways.
  - The linger runs inside the session while it still owns the sockets (the first version closed them
    first, so the linger did nothing). A dropped event receiver cuts it short.
  - The actor's `select!` is no longer biased, so a sender that is always ready cannot starve socket reads.
    SDUs still queued when a registration completes were sent while disconnected; they are dropped and
    counted (D3 step 5), not sent into the new session.
  - "`Registered` and `Disconnected` are never dropped" is narrowed: when nobody reads events across
    several reconnects, an undelivered `Registered` followed by `Disconnected` is folded away, so the backlog
    stays bounded. Order is kept and the last state is always delivered.
  - Before returning a fatal registration error, the actor hands over the queued events that fit without
    waiting.
- **A5 (2026-11-21, S0 review round 2 and contrarian check; user decisions "Watch channel", "Drain then
  stop"):** replaces D2's event model and the lifecycle and stop parts of A2 and A4. Round 2 showed the A4
  fold still let the lifecycle backlog grow when sessions carry traffic (drop-oldest evicts from the middle,
  leaving pairs the tail-only fold never sees), could deliver `Disconnected` twice, and that dropping both
  channel ends at once lost queued SDUs and cut the linger short.
  - **Link state is a latest value, not an event.** The actor publishes a `Copy`
    `LinkState { session: u64, al_mac: Option<MacAddr> }` on a `watch` channel (0004 R3). `session` counts
    completed registrations (it equals `TransportCounters::registrations`); `al_mac` is `Some` while
    registered and `None` otherwise. A role actor that sees `session` change knows a reconnect happened even if
    it missed the `None` in between. `TransportEvent` is removed.
  - **The received channel carries only `Sdu`.** Its queue inside the actor is a plain drop-oldest queue of at
    most 256 SDUs; the fold, the double-`Disconnected` guard and the hand-over before a fatal error go away.
  - **Ordering given up:** a link change is not ordered against received SDUs as the role actor's `select!`
    sees them. A role actor may see a new `session` before it reads SDUs received in the old one. Note 0012
    needs only the AL MAC and the fact of a reconnect (D2, D3, exit check 6(a)); upstream has no reconnect
    (`em_mgr.cpp:427-433`).
  - **Ordering kept (plan review F1):** at each registration the actor first drops and counts the SDUs left
    in the send channel from the disconnected period, then publishes the new `LinkState`, and only then
    reads frames of that session. So when a role actor handles a received SDU, `link.borrow()` shows that
    SDU's session or a later one. Role actors read `link.borrow()` when they need the AL MAC for an SDU, not
    the order in which `changed()` fires; S1's `em-ctrl` gets its AL MAC this way.
  - **API.** One `spawn(config) -> Transport`, a struct of owned fields: `sdus: mpsc::Sender<Sdu>`,
    `received: mpsc::Receiver<Sdu>`, `link: watch::Receiver<LinkState>`,
    `counters: watch::Receiver<TransportCounters>`, `task: TransportTask`. `spawn_with_counters` (A1) is
    removed. The role actor may move the fields apart; no clone is needed.
  - **Stopping.** Dropping the `Sender<Sdu>` stops the actor as in D2. Dropping the received `Receiver` still
    stops it (A2), but through the same path: in a session the actor stops reading, writes the SDUs already
    in the channel (at most its capacity, without waiting for more), lingers the full 100 ms, then ends `Ok`.
    The linger no longer races the receiver drop. While not registered it ends at once and counts any queued
    SDUs as dropped (D3 step 5). Dropping only the `link` or `counters` watch stops nothing. A2's exception
    for a blocked write stands.
    Any sign that the received receiver closed (`closed()`, or `reserve()` returning an error) enters this
    same path. Every drain takes at most the SDU channel's capacity, so a sender that keeps sending cannot
    hold the actor. SDUs sent during the linger are not written and are counted as
    `dropped_send_disconnected`. In the waits while not registered (connect, the response, the reconnect
    delay) the actor ends at once; a registration that completes in the same poll as the drop starts a
    session that finds the receiver closed, drains and lingers. The probe ends its loop when `changed()`
    returns an error, as when `received` closes (plan review F4).
  - **Harness.** Exit check 2's deadline starts when both probes have printed `REGISTERED`, not at
    `compose up`, matching "after the later service start". The script is stricter than exit check 2 as
    written (plan review F5): it passes only on the service's log line for the peer's Topology Discovery,
    never on time alone, so a pass is evidence of learning (the round 1 fix). The probe derives its `REGISTERED` and
    `DISCONNECTED` lines from the watch: leaving a registered state (to `None`, or to a new `session`)
    prints `DISCONNECTED`; reaching a new `session` with an AL MAC prints `REGISTERED`.
  - **Tests** added (plan review F2), all on virtual time where they measure the linger:
    - 64 SDUs sent without yielding, then both ends dropped in the same poll: all 64 reach the fake service,
      no end of file before 100 ms, then `Ok`;
    - the receiver alone dropped in a session, sender held: the queued SDUs reach the service, no end of file
      before 100 ms, then `Ok`; and with a sender that keeps sending: the actor still ends;
    - `link` and `counters` dropped: SDUs still flow both ways;
    - SDUs queued while disconnected are dropped and counted at registration, not sent into the new session;
    - the first received SDU of a session: `link.borrow()` already shows that session and its AL MAC;
    - many reconnects with an unread receiver, each session delivering SDUs (more than 64 + 256 in total)
      and a different AL MAC per session: `dropped_receive_overflow` equals received minus 320, draining
      yields, in order, the first 64 (already in the channel) and then the newest 256, and `link` is the last session with the last AL MAC.
- **A6 (2026-11-21, review round 1 of A5; user decision "fix all of them"; revised after plan review):**
  closes the gaps the three reviewers confirmed in A5. Where A5 and A6 differ, A6 applies.
  - **The caller names the session (S0-01).** A5's "left-over SDUs are dropped at registration" cannot hold:
    a sender on another thread, or one already waiting for room, can enqueue after the drain and before the
    publish. And in S1 an SDU is built in a handler and forwarded later (0002, `Outputs` and `reserve()`
    branches), so a stamp taken at send time could label old content with the new session. So
    `Transport::sdus` is an `mpsc::Sender<OutboundSdu>`, with `OutboundSdu { session: u64, sdu: Sdu }`. The
    role actor copies `link.borrow()` when it **builds** the SDU, takes `local` from it, and carries its
    `session` with the SDU. In session `n` the actor writes an SDU only if `session == n`; every other SDU is
    dropped and counted in `dropped_send_disconnected`. The guarantee: **an SDU is written only into the
    session it was built for.** It stays a plain sender, so `reserve()` works as 0002 requires. The drain at
    registration is removed: stale SDUs fail the comparison when dequeued.
  - **Stopping closes admission (S0-03, S0-04).** When the role actor has gone, or on a fatal registration
    error, the actor calls `close()` on the send channel, so later sends fail at the caller. In a session it
    then writes the SDUs in the channel at that moment that match the session. Everything else is counted in
    `dropped_send_disconnected`, reading until `recv()` returns `None`, because tokio keeps a channel open for
    a send permit already reserved. That wait is limited to [`LINGER`] (100 ms): in a session it is the
    linger itself, and while not registered it ends at once unless a permit is outstanding. **Limit:** an SDU
    whose permit is still held when that time ends is lost uncounted. This replaces A5's "SDUs sent during
    the linger are counted": they now return an error to the sender.
  - **A socket failure does not hide a stop (S0-02).** When a read or a write ends the session, the actor
    checks whether the role actor has gone (`received` closed, or the send channel closed). If so it takes the
    stop path, not the reconnect path. On this path only, the drain's writes are limited to 1 s in total, so
    a peer that closed one direction and stopped reading cannot turn a stop that ended into one that hangs.
    A2's blocked-write exception is otherwise unchanged.
  - **One teardown for every session exit (S0-05, S0-06).** After a stop as after a disconnect, the
    unfinished fragment set is counted and `al_mac` is published as `None`; on a stop this happens after the
    linger. `session` keeps its last value.
  - **Received SDUs discarded at the end are counted.** A new counter `dropped_receive_stopped` counts the
    SDUs still in the actor's internal queue on every return of the actor, including a fatal error. Then
    `sdus_received` = SDUs moved into the channel (read or not) + `dropped_receive_overflow` +
    `dropped_receive_stopped`. Aborting the task skips this count.
  - **Probe (S0-07).** FIFO commands copy the current `link.borrow()`, using its AL MAC and session.
    `shown_link` only drives the `REGISTERED` and `DISCONNECTED` lines.
  - **Tests.** Stamp and teardown cases are in-crate tests that place items in the channel before the session
    runs, so they do not depend on task scheduling:
    - an SDU for session `n - 1` is not written in session `n`; an SDU for session `n` is;
    - a send already waiting for room while disconnected, built for the old session, is not written;
    - end of file and a role-actor drop ready together, with a peer that still reads, take the stop path
      (repeated enough that a random `select!` order cannot hide a miss);
    - a peer that closed one direction and stopped reading: the stop still ends;
    - after the stop began, sends return an error and are not written; a permit held across `close()` and
      used within the linger is counted;
    - SDUs queued during a fatal registration are counted;
    - final link is `{last session, None}` after each stop path; an unfinished fragment set is counted on a stop;
    - the received-SDU equation after a sender drop and a fatal error, asserting `dropped_receive_stopped > 0`.

    These replace `tests/session.rs` tests that relied on the registration drain or on sends being accepted
    during the linger. Each new test for a fix must fail with that fix removed.
- **A7 (2026-11-21, review round 2 of A6 and contrarian check; user decision "Remove the limit"):** removes
  A6's 1 s limit on a stop's writes. Where A6 and A7 differ, A7 applies.
  - **Why.** The limit keyed on the wrong signal. A socket failure means the peer closed, and then writes fail
    quickly. A live service that stops reading produces no socket failure, so the limit never applied to it,
    whichever `select!` arm won. The pinned service never closes one direction of the data socket
    (`ieee1905-core/src/al_sap.rs` keeps both halves in statics, lines 55-56), and a dead process makes a
    blocked write fail. S0-02 itself did not need a limit: its peer could still read.
  - **Kept from A6.** A read or write failure with the role actor gone still takes the stop path.
  - **Removed.** `STOP_WRITE_LIMIT`, `stop_write_limit`, `send_sdu_by` and the test of the mapping. A6's
    sentence that a peer "cannot turn a stop that ended into one that hangs", and its test for that peer, no
    longer apply. A peer that closes one direction and stops reading can now hang a stop; the pinned service
    never does this.
  - **Test replaced, not deleted (plan review F1).** The old limit test was also the only check of two kept
    behaviours. Its replacement blocks a stop's write on a full socket with two matching SDUs queued, asserts
    a send is refused while blocked, then closes the peer fully, and asserts the stop ends with one
    `dropped_send_failed` and one `dropped_send_disconnected`.
  - **Recorded limit (A2's exception, the blocked-write follow-up in `STATUS.md`).** If the service stays alive
    and stops reading, a write can block without limit, in a session or during a stop. A caller that must
    bound its own stop times out on `&mut task` (or keeps an `AbortHandle`), then calls `abort()`: a timeout
    that takes the `JoinHandle` by value drops it on expiry and leaves the actor running. An aborted actor
    returns a cancelled `JoinError` and skips the linger, the counts for the blocked SDU, the SDUs still queued,
    the unfinished fragment set and `dropped_receive_stopped`, and does not publish `al_mac: None`. The send
    and receive counts do not balance after an abort.
  - **Stop cutoff (S0-R2-03).** A stop writes the matching SDUs counted by the channel's length, read just
    after `close()`. A send completed before that read is in the batch. A send in progress during the read may
    fall on either side. A later send through a held permit is counted if it arrives before the linger ends;
    otherwise A6's uncounted-permit limit applies. A plain `mpsc` sender cannot give an exact cutoff.
  - **No write after a failed write (A7 review round 1, S0-A7-01).** The session exit distinguishes a failed
    read from a failed write. After a failed write the stop path writes nothing more: the partial frame may
    have left the stream out of step, and the SDUs still queued are counted in `dropped_send_disconnected`.
    It still closes the channel, lingers and runs the common teardown. After a failed read it still writes,
    as S0-02 needs.
  - **Recorded limit corrected (S0-A7-ADV-01).** A session that ends on a failed write stops reading, so every
    received frame not yet read, in the socket or the frame reader's buffer, is lost and not counted.
    Reassembled SDUs already in the actor's queue are counted in `dropped_receive_stopped`.

## Errata

- **E1 (2026-11-21, S0 harness build):** D4 says `ieee1905-rs` needs Rust 1.88 or later. It needs
  **1.91**: `ieee1905-core/src/cmdu_codec.rs:1231` calls `str::floor_char_boundary`, and the build failed
  with E0658 on 1.90. The harness builds it on `rust:1.91`. The workspace itself stays on MSRV 1.85.
- **E2 (2026-11-21, S0 documentation pass; coordinator checked `em_mgr.cpp:427-433`):** "Upstream client
  defects not ported" says an oversize length throws out of the manager loop and the process stops. It does
  not: an oversize length and a malformed header are `InvalidMessage`, which the manager catches, logging
  "Dropping packet". Only the other errors (a closed socket, a fragment out of order) are rethrown. For an
  oversize length upstream has already refused to read the body, so its stream falls out of step; the port
  skips the body (D3 step 6). Upstream's limit is `SOCKET_MTU - 4` = 65 532 octets
  (`al_service_access_point.cpp:235-237`); the port's is 65 536 (follow-up in `STATUS.md`).
