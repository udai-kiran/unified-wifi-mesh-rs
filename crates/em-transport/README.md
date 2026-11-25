# em-transport

AL-SAP client: carries CMDUs to and from an external IEEE 1905 daemon (such as
`rdkcentral/ieee1905-rs`) over two UNIX sockets. One tokio actor (`spawn`), no CMDU parsing: the payload of an
`Sdu` is opaque bytes. Design: [note 0011](../../docs/0011-em-transport-s0.md) (slice S0), D1 to D3.
No `unsafe`, no clones. The same transport also runs on the event loop (`TransportHandler` and
`TransportIo`, 0016 D8 step 4b-3).

## What is here

| Item | Holds | Upstream |
|---|---|---|
| `spawn` | Starts the actor and returns a `Transport` of owned fields: `sdus: mpsc::Sender<OutboundSdu>`, `received: mpsc::Receiver<Sdu>`, `link: watch::Receiver<LinkState>`, `counters: watch::Receiver<TransportCounters>`, `task: TransportTask` (0011 A5, A6) | `AlServiceAccessPoint` (`al_service_access_point.cpp`) |
| `TransportConfig`, `Role` | Socket paths, role (service type 0x01 agent, 0x02 controller), reconnect delay (default 2 s) | `al_service_registration_enums.h:7-14`, `em_ctrl.cpp:1797-1798`, `em_agent.h:34-35` |
| `Sdu { local, remote, cmdu }` | One CMDU, no Ethernet header. `local` is wire MAC A, `remote` MAC B, both ways | `al_service_data_unit.cpp:40-97`, `em.cpp:799-813`, `em_mgr.cpp:410-419` |
| `OutboundSdu { session, sdu }` | An SDU for the actor to send, with the `LinkState::session` it was built for. The role actor copies `link.borrow()` when it builds the SDU; the actor writes it only into that session (0011 A6) | |
| `LinkState` | `Copy { session: u64, al_mac: Option<MacAddr> }`: a latest value. `session` counts registrations; `al_mac` is `Some` while registered | |
| `TransportError` | A rejected registration, or a socket error that a reconnect cannot fix | `em_ctrl.cpp:1800-1803` |
| `TransportCounters` | Drops and totals; errors go to `tracing`, there is no error event. `dropped_receive_too_large` counts fragment sets refused over the 1 MiB cap. `dropped_send_overflow` (0016 U12) is counted only by `TransportCore`: the actor never sets it | |
| `wire` (private) | Length framing, registration, SDU header, fragment split and join | `al_service_utils.cpp:68-82`, `al_service_access_point.cpp:138-196`, `:224-300` |
| `TransportCore`, `TransportPolicy`, `TransportAction`, `TransportOutcome`, `RegistrationStep`, `Frame` | The sync transport core (0016 D4, step 4b-2b); see "TransportCore" below. The handler and the tokio `Io` run it on the event loop (4b-3). `em-agent` runs on the loop (4c). The old actor stays until `em-ctrl` moves to the loop (step 5). `Frame` is public because `registration_frame` takes the registration response as one | `actor.rs` (the behaviour it ports) |
| `policy` (private) | The numbers, in one place (0016 U15, U31): reconnect delay 2 s, received queue 1024 SDUs and 4 MiB, SDU cap 1 MiB, outbound buffer 4 MiB, write stall 5 s, stop write limit 900 ms, linger 100 ms. `TransportPolicy::easymesh()` gathers them; the actor uses `LINGER`, `TransportConfig::new` the reconnect delay | |
| `transport_core` (private module) | `TransportCore` and its types, built on `FrameSplitter`, `Reassembler`, `ReceivedQueue` and `OutboundBuffer`. Tests in `src/transport_core/tests.rs` | |
| `handler` (private module; `TransportHandler` and its types are public) | Synchronous composition of `TransportCore` into an `em-event-loop` application (0016 D2, D3b). `TransportSources` gives the ids of the read, registration and write sources and the timer. `TransportEvent`s come in; `TransportIoAction`s go out (`Connect`, `CloseSession`, `Read`, `Write(Vec<u8>)`). Reads are armed by `Read` only. One `Write` is in flight at a time, and its result is judged at the time of the pass that issued it. Tests in `src/handler/tests.rs` | |
| `tokio_io` (private module; `TransportIo`, `IoEvent`, `IoAction` are public) | The tokio `Io` of the loop (0016 U19, D2). It connects and registers, reads and writes the data socket when told to (`try_write` runs in `perform`), and receives from the application channel once per `RecvApp`. A failed registration step keeps its `io::Error`, so `TransportIo::into_result` builds `TransportError::Io` with the same message and `source()`. `em-event-loop` is a workspace crate, not a new dependency | |
| `frame_reader` (private) | Cancel-safe async frame reader that skips an oversize frame by its length; delegates the byte-to-frame logic to `frame_splitter` | |
| `frame_splitter` (private) | `FrameSplitter` and `Frame`: sync byte-to-frame logic (`push_bytes`, `next_frame`, `has_frame`). No tokio. Built in 0016 D8 step 4b-2a; `FrameReader` and `TransportCore` use it | |
| `received_queue` (private) | `ReceivedQueue`: sync queue of received SDUs with an SDU limit and a byte limit (sum of `cmdu.len()`), drop oldest, an SDU above the byte limit dropped alone (0016 U1, E6, U34). Difference `rx-queue-1024-4mib`. Built in 4b-2a, used by `TransportCore`; the actor keeps its own 256-SDU queue | |
| `outbound_buffer` (private) | `OutboundBuffer`: sync per-session write queue with partial-write tracking, a byte limit, drop oldest not started (an SDU whose write is in flight is kept, 0016 E12 item 9), and the session-end counts (0016 U12, D3b). Difference `tx-buffer`. Built in 4b-2a, used by `TransportCore`; the actor still writes with `send_sdu` | |

## TransportCore

A synchronous state machine, no tokio, no clock, no I/O (0016 D1). The caller passes `now`
(`std::time::Instant`), reports what happened on the sockets, and performs the `TransportAction`s that go
into its `&mut Vec`: `Connect` (start one registration attempt: connect control, connect data, send the
request, read one framed response), `CloseSession` (close the sockets of the session or attempt) and
`Finish(TransportOutcome)` (`Stopped`, `Rejected { result }` or `IoFailed { operation }`; the caller keeps
the original `io::Error` and builds `TransportError::Io` from it).

States: `Disconnected` (reconnect due at a time; `new` starts here, due at once, so the first `tick`
returns `Connect`), `Registering`, `InSession`, `StopWriting`, `StopLingering`, `Finished`.

| Input | Effect |
|---|---|
| `tick(now)` | Acts on every due deadline: the reconnect (`Connect`), the write stall, the stop write limit, the end of the linger. `next_deadline()` is afterwards after `now` |
| `registration_frame`, `registration_failed(step, kind)` | The verdict of `actor.rs` `register`, `classify_connect` and `classify_registration_io`: retry after `reconnect_delay`, or `Finish(Rejected)` or `Finish(IoFailed)`. A fatal verdict counts the received queue in `dropped_receive_stopped` |
| `bytes_read`, `read_ready`, `wants_bytes`, `serve_read` | Octets go to the `FrameSplitter` (in a session that is not stopping). `wants_bytes` is true in a session while no complete frame waits, so the caller may read. One `serve_read` handles one frame as `handle_frame` did, into the `ReceivedQueue`. Frames left in the splitter keep `read_ready` true |
| `read_ended` | End of file or a read error ends the session |
| `received_ready`, `take_received` | The role takes SDUs from the received queue |
| `send` | Written only into the session it was built for; else `dropped_send_disconnected`. Buffer full: `dropped_send_overflow`; more than 256 fragments: `dropped_send_failed` |
| `write_ready(now)`, `write_chunk(now)`, `wrote(now, n)`, `write_blocked`, `writable`, `write_failed` | The caller writes `write_chunk` and reports the octets written, with the time the write was performed. `write_ready` is false while the socket is known to be blocked (0016 E10). Both refuse when the stall deadline (in a session) or the stop write limit (stopping) is due at `now`, so a due deadline wins over a write at the same time (0016 E11) |
| `stop` | In a session: written for at most `stop_write_limit`, the rest counted, sockets open for `linger`, then `CloseSession` and `Finish(Stopped)`. Without a session: `Finish(Stopped)` at once. The received queue is counted in `dropped_receive_stopped` at the stop |

One counting rule for every session end (write error, end of file, write stall, stop limit): a partly
written SDU (some octets left) is `dropped_send_failed`, SDUs not started (no octet left, 0016 E11) are `dropped_send_disconnected`, an unfinished
fragment set is `dropped_receive_fragments`, the splitter starts again. A session that ends on its own then
clears `link.al_mac`, returns `CloseSession` and reconnects after `reconnect_delay`; the received queue
survives. Every send is counted once in `sdus_sent`, `dropped_send_failed`, `dropped_send_disconnected` or
`dropped_send_overflow`, and `sdus_received` is the SDUs taken by the role plus `dropped_receive_overflow`
plus `dropped_receive_stopped`. After `Finish` every input is ignored; a `send` is only counted. The write
stall (0016 U4) runs while queued bytes make no progress for `write_stall` in a session.

## Behaviour in short

- Connects control, then data, sends an enable request, reads one whole framed response.
- Retries after `reconnect_delay` on a missing socket file, "connection refused", and end of file or
  reset before the response. A `result` other than success ends the actor with an error. An all-zero
  AL MAC is retried.
- After a registration the actor publishes `LinkState { session, al_mac: Some(..) }`. End of file or an
  error on the data socket publishes `al_mac: None`, then the actor registers again after the delay and
  `session` goes up by one. SDUs sent while not registered are dropped and counted.
- Sends at most 65 517 payload octets per frame and numbers longer payloads as fragments (at most
  256). Joins received fragments.
- Receive errors keep the session: an oversize frame is skipped by its length, a fragment out of order
  or an unfinished set is dropped (fragment id 0 or an unfragmented frame starts anew), a frame too
  short for an SDU header is dropped.
- A continuation fragment (id above 0) must carry the addresses of the set it joins, or it and the set
  are dropped (0011 A8). This only separates SDUs from different peers: two fragmented SDUs from the
  same peer have identical addresses and the frame has no message id, so interleaved sets still join
  wrongly.
- A fragmented set whose joined payload would pass 1 MiB (`policy::RX_SDU_CAP`) is refused when the
  fragment that would pass it arrives (a fragment 0 alone over the cap is refused at once). It counts once in
  `dropped_receive_too_large`, not in `dropped_receive_fragments` or `sdus_received`, and none of its frames is
  delivered. The `Reassembler` then skips the rest of the set without counting, until its last fragment; a frame
  with id 0 or an unfragmented frame ends the skip and is handled as usual, and a continuation that does not match
  the set (id or addresses) ends it and is a stray. The session goes on (0016 D4, U2, U6; amends 0011 D3 step 6).
  The cap applies to fragmented sets only (0016 E5, U33): an unfragmented frame is at most 65 536 octets.
- The `received` channel carries only SDUs. When the role actor does not read, up to 256 wait in the actor
  beyond the channel's 64; when the queue is full the oldest is dropped and counted.
- A failed write ends the session without reading further (0011 A7). Every received frame not yet read,
  whether still in the socket or in the frame reader's buffer, is lost and not counted. SDUs already in the
  actor's queue are dropped, but counted in `dropped_receive_stopped` when the actor returns.
- Ordering (0011 A5, A6): the actor publishes the new `LinkState` at registration before it reads any frame
  of the session. So when the role actor handles a received SDU, `link.borrow()` shows that SDU's session
  or a later one. A link change is not ordered against received SDUs in the role actor's `select!`: it may
  see a new `session` before it reads SDUs of the old one. Read `link.borrow()` for the AL MAC, and watch
  `session` for reconnects.
- Sending (0011 A6): build each SDU from a copy of `link.borrow()`: `local` from its AL MAC, `session` into
  the `OutboundSdu`. In session `n` the actor writes only SDUs with `session == n`; any other, including one
  that waited for room in the channel across a reconnect, is dropped and counted in
  `dropped_send_disconnected`. There is no drain at registration.
- Stopping: dropping the `Sender<OutboundSdu>`, the `received` receiver or both ends the actor with `Ok`; a
  fatal registration error closes the send channel too. The actor first closes the send channel, so later
  sends fail at the caller. In a session it writes the SDUs that were in the channel and belong to the
  session, then stays 100 ms with the sockets open, then ends. The 100 ms is counted from the end of those
  writes. A failed write stops the writing, because the stream may be out of step: the failed write counts in
  `dropped_send_failed`, and the other queued SDUs in `dropped_send_disconnected`. After a failed read the
  stop still writes. Also counted in `dropped_send_disconnected`: queued SDUs built for another session, and
  SDUs that arrive through a permit held across the close. An SDU whose permit is still held when the 100 ms
  ends is lost uncounted. While not registered it counts the queued SDUs and ends at once, unless a permit is
  outstanding: then it waits for it, at most 100 ms. A read or write failure with the role actor gone takes
  this stop path too. A stop writes the matching SDUs counted by the channel's length,
  read just after the close: a send completed before the read is in the batch, one in progress may fall on
  either side (0011 A7). Every session exit
  counts the unfinished fragment set and publishes `al_mac: None` (after the linger on a stop); `session`
  keeps its value. Received SDUs still in the actor's queue when it returns are counted in
  `dropped_receive_stopped`: `sdus_received` is the SDUs moved into the channel plus
  `dropped_receive_overflow` plus `dropped_receive_stopped`. Dropping only `link` or `counters` stops nothing.
  Writes have no time limit (0011 A7): if the service stays alive and stops reading, a write can block
  without limit, in a session or during a stop (open follow-up in `STATUS.md`). To bound a stop, time out on
  `&mut task` (or keep an `AbortHandle`) and call `abort()`; a timeout that takes the `JoinHandle` by value
  drops it on expiry and leaves the actor running. An aborted actor returns a cancelled `JoinError` and skips
  the linger, the counts for the blocked SDU and the queued SDUs, the unfinished fragment set and
  `dropped_receive_stopped`, and does not publish `al_mac: None`, so the send and receive counts do not
  balance after an abort.

**Contract for callers:** every SDU is exactly one CMDU ending in End of Message. The service drops
anything else without telling the client.

## Tests

Unit tests cover `TransportCore` (see below), the wire codec (bytes written out by hand from the field order in
`ieee1905-rs`'s `sdu_codec.rs`), the frame reader and the drop-oldest queue, and, in `src/actor.rs`, the session
stamp and the stop path (SDUs are placed in the channel before the session runs, so they do not depend on
scheduling).

`tests/session.rs` runs the actor against a scripted fake service (`tests/support/mod.rs`): two UNIX
listeners in a temporary directory that accept the client's connections, read its registration request
and write the responses and SDUs the test chooses. The reconnect delay is 2 s. The tests run with paused
time, except the six that run in real time. Each of those six stops the actor by dropping its received
receiver (0011 A5 stop path).

`tests/loop_session.rs` runs the scenarios that still have a meaning on the event loop (0016 D8 step 4b-3): a `LoopCore`, `TransportHandler` and `TransportIo` with real UNIX sockets and the fake service in `tests/loop_support/mod.rs`. Its header maps each test of `tests/session.rs` to its port, its new name or its replacement. Its stop signal is dropping the application sender; dropping `received` is not one. `tests/session.rs` and the actor stay until the actor is removed (0016 D8 step 5).

Tests tagged `intentional-difference:<id>` pin the places where this client differs from upstream's:
`agent-rejection-fatal`, `retry-connect`, `split-response`, `zero-al-mac`, `reconnect-after-disconnect`,
`fragment-resync`, `continuation-mac`, `oversize-skipped`, `bad-header-dropped`, `rx-sdu-cap-1mib`.
Four more are pinned by the building blocks and by `TransportCore`, which the handler runs on the loop (`tests/loop_session.rs` repeats their tags):
`rx-queue-1024-4mib` (1024 SDUs and 4 MiB; the old queue held 256 SDUs, count only), `tx-buffer` (4 MiB, drop
the oldest SDU not yet started, `dropped_send_overflow`; the old `PendingFrames` held 64), `rx-sdu-cap-1mib`
(also through the core) and `tx-write-stall-5s` (the old actor had no write time limit). The limits are
arguments; the numbers live in `policy`. `rx-read-between-passes` and `tx-failed-write-count` (0016 E11) are tagged only in `tests/loop_session.rs`.

`src/transport_core/tests.rs` has 64 tests: scenario tests for every rule above (registration verdict by
step and error kind, all-zero AL MAC, rejection, session numbering and link, session mismatch, send while not
registered, outbound overflow, write readiness with `write_blocked`/`writable`, the stall at exactly
`write_stall` and not 1 ms before, progress resets the stall, each session-end cause with the counting rule,
reconnect after the delay, the stop in a session (limit, linger, buffer empty early, after a failed write,
within `stop_write_limit + linger`), the stop without a session, the received queue counted at a stop, inputs
after `Finish`, `read_ready` gating) and a seeded random test (400 seeds of 200 steps, `test_prng`) that feeds
random registrations (ok, retry, fatal), byte streams (whole and fragmented SDUs, garbage, oversize), partial
writes, blocked/writable, sends for current and old sessions, read and write ends, stops and time steps, and
checks after every step: no panic; no deadline in the past after a tick; `Connect` only without sockets,
`CloseSession` only with sockets, `Finish` at most once and last; the buffer and queue limits; the receive
balance; and, once finished, the send balance.

`FrameSplitter`, `ReceivedQueue` and `OutboundBuffer` each have unit tests and a seeded model test
(`src/test_prng.rs`, xorshift, no dependency) against a plain reference model: random byte streams cut at
random points (frames and `has_frame` at every step); random push/pop with small limits; random push,
partial advance and `end_session` with small limits (chunks, finished SDUs and every count).

Every wait is bounded. `bounded` fails a test after 60 s of paused time, and `join_within` after its
own limit (5 s for the six real-time tests). A watchdog thread aborts the test process when a
guarded wait does not finish in real time: 20 s for `bounded`, the limit plus 5 s for `join_within`,
and 10 s for the retry tests. No timeout on the test's own thread can interrupt an actor that spins
without yielding, so the watchdog is the only stop for that case.

`src/wire/model_tests.rs` has a seeded model test of the `Reassembler` (a small xorshift, no dependency): 300
seeds of 200 random frames and `abandon_set` calls (six peers, each a pair of one of two local and one of three remote addresses, fragmented and whole frames, out-of-order ids, caps
of 1 to 8 octets) are fed to the `Reassembler` and to a reference model that keeps the joined parts in a `Vec` and
sums their lengths. Every step must give the same delivered SDU (local, remote and payload bytes, which count up so order shows), `dropped` and refusal flag, and `abandon_set` the same
count. The same file holds the properties of 0016 D7 (0017 A3): P0 round trip under the cap (up to 256 fragments
with the encoder's limit as cap), P2' (`A0 B0 A1` from one peer delivers an SDU equal to neither, as a
counter-example), P3 (abandoned set 1, stray 1, broken continuation 2) and P5 (the partial payload never passes
the cap, a refusal counts once, nothing of the set is delivered).

Test counts: 174 unit tests (`cargo test -p em-transport --lib`; 66 of them in `transport_core`, 20 in `handler`), 66 in `tests/loop_session.rs` and 66 in `tests/session.rs`.

Run `cargo test -p em-transport --locked`, and `cargo +1.85.0 test -p em-transport --locked` for MSRV.

## Probe

`examples/alsap_probe.rs` is test tooling for the Docker harness (note 0011 D5), built with `em-msg`:

```sh
cargo run -p em-transport --example alsap_probe -- \
    --role agent|controller --control <path> --data <path> --fifo <path>
```

stdout, one flushed line per event: `REGISTERED <al_mac>`, `RECEIVED local=<mac> remote=<mac>
type=0x<4 hex> cmdu=<hex>`, `DISCONNECTED`, and `SENT <remote> type=0x<4 hex> mid=<n>` for each send.
The probe runs the transport on the event loop. `REGISTERED` and `DISCONNECTED` come from the transport's link state, checked after every event and every turn: leaving a registered state (to no AL MAC, or to
a new `session`) prints `DISCONNECTED`; reaching a new `session` with an AL MAC prints `REGISTERED`. The
probe ends when the transport finishes (a stop, a rejected registration or an I/O error; the error goes to stderr). If the FIFO reader stops, the probe stops the transport and exits with a failure.
Commands, one per line on the FIFO (created if missing, reopened at end of file):
`send <remote_mac> ap-cap-query` and `send multicast search`. The FIFO is created with the `mkfifo`
program. A command received before registration is ignored with a message on stderr. Each command is built
from a copy of the current `link` (AL MAC and session), not from the state last printed (0011 A6).

The probe needs a running service on the two sockets, so it is meant for the Docker harness in
`docker/`. A session with a local `ieee1905-rs` looks like this:

```sh
cargo run -p em-transport --example alsap_probe -- --role agent \
    --control /tmp/al_control_socket --data /tmp/al_data_socket --fifo /tmp/alsap-probe.fifo
echo "send multicast search" > /tmp/alsap-probe.fifo
```

## Not here

- The raw `AF_PACKET` backend and the trait over both backends (deferred, 0010 D2).
- The platform fake, which moved to slice S1 (0011 header, amending 0010 D1).
- Duplicate filtering and security stages (role actor).
- CMDU parsing or validation (`em-msg`).

Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`; `ieee1905-rs` @ `a6443225`.
