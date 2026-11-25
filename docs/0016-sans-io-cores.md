# 0016: Pure cores in one verifiable event loop (`em-event-loop`) for the agent and controller

| | |
|---|---|
| Status | **Accepted** (2026-11-23, user), rev 5, errata E1 to E12. User decisions U1 to U38; verification workflow in [0017](0017-verification-workflow.md), amended by its A2 and A3 (proofs only for `LoopCore`, frozen; the `Reassembler` is tested, not proved; Codex review plus the user, a second reviewer only for security code; this replaces D8's "plan review and the full review set"); open: C7 (deferred), C11 (later) |
| Date | 2026-11-23 |
| Follows | [0004](0004-runtime-and-ownership.md) R1 to R4, R6; [0002](0002-cmd-core-orch.md) D1, D7, D10; [0011](0011-em-transport-s0.md) D2, D3, A5 to A8; [0012](0012-s1-discovery.md) D1, D9, D11; [0013](0013-s2-onboarding.md) D9; [0014](0014-init-config-and-al-mac.md) D16; [0010](0010-vertical-slices-and-transport.md) D6, D7 |
| Amends | 0004 R3, R4, R6 (D2); 0002 D1 (role-actor wording, `reserve()` forwarding, database I/O, bridges), D7 (reply ownership), D10 item 6 (database I/O through `Offload`, same "no other role input" rule, U7); 0011 D2, D3 step 6 (U2 refusal), A5 (U1), A6 (D4), A7 (U4, counting kept); 0012 D9 (session stamps, pending frames, transport ended, stopping with the 1 s bound kept, U8); 0014 D16 (admin tasks become loop sources, replies by id, the save uses `Offload`) |
| Affects | new crate `em-event-loop`; `em-transport`, `em-agent`, `em-ctrl`; `examples/alsap_probe.rs` and `em-transport/tests/` (U20); `docker/run-s2.sh` (AC4); `clippy.toml` (U27 bans) |
| Number | 0015 stays reserved for the threat model (secure-onboarding plan, Stage 5) |

## Context

The goal is code whose decisions can be checked exhaustively: by deterministic tests now, and by machine-checked proofs
later. The candidate proof tool is Aeneas, which translates **safe, sequential** Rust into Lean. It does not handle async,
concurrency or `unsafe`, and every call into another crate needs a hand-written model. So proofs can cover only pure code.

An event loop in the libae style (`aeProcessEvents`: find the nearest deadline, **poll**, dispatch ready sources once, run
due timers) is pure except for the poll. Today the port has no such loop: the transport and the role are separate tokio
actors joined by two `mpsc` and two `watch` channels, and their ordering rules live inside `tokio::select!`, where no tool
can see them.

### Where the code is today

| Crate | Shape | Evidence |
|---|---|---|
| `em-agent` | Sync `AgentCore` (inputs plus `Instant`, returns frames, platform requests, `next_deadline()`) under a tokio actor `select!` | `lib.rs:5-9`, `:160-262`; `actor.rs:249-266` |
| `em-ctrl` | Same shape (`CtrlCore`) | `lib.rs:136-255`; `actor.rs` |
| `em-transport` | A separate actor. Pure parts: SDU encoding, frame decoding, `Reassembler` (`wire.rs:52-270`). Decisions inside async code: registration verdicts (`actor.rs:135-165`, `:436-508`), session numbering and per-session write check (`:293-297`, `:321-327`), stop path (`:247-275`), frame handling and the received queue (`:392-433`), teardown (`:347-362`). The frame splitter `take_frame` is pure but is a method of the async `FrameReader<R>` (`frame_reader.rs:69`). The control socket is read only for the registration response (`actor.rs:93-96`) | `actor.rs`, 865 lines |
| binaries | `multi_thread`, 2 workers (`em-agent/src/main.rs:15`, `em-ctrl/src/main.rs:14`), relied on by 0002 D1 (`block_in_place` stalls only the role actor) and 0012 D1 | |

Of the 0011 A5 to A7 mechanics, only some come from the channels. The **100 ms linger** guards a race inside the daemon
(a close can beat SDUs it is still forwarding, 0011 "SDUs just before a close", `actor.rs:36-38`). The **missing write
time limit** was a user decision (0011 A7, `STATUS.md` log 2026-11-21). This note changes the second by user decision (U4)
and keeps the first (U3).

Leaks inside the existing cores:

- **Clock:** the 0013 D9 timing reads `Instant::now()`/`elapsed()` at `em-agent/src/s2.rs:53-55`, `em-agent/src/lib.rs:557-569`,
  `em-ctrl/src/s2.rs:114-116`; the values go into log fields (`build_us`, `check_us`) that `docker/run-s2.sh:209-221` collects.
- **Randomness:** `AgentCore` and `CtrlCore` hold `OsRandom` (`em-agent/src/lib.rs:112`, `em-ctrl/src/lib.rs:122`); `em-crypto`
  has `RandomSource` (`rand.rs:26-32`), and `build_m1`/`build_m2` take `&mut impl RandomSource` (`em-core/src/wsc.rs:233, 598, 649`).
- **Hash seeds:** `std::collections::HashMap` seeds itself from OS randomness (`em-orch/src/lib.rs:233-237`,
  `em-ctrl/src/lib.rs:118`, `em-ctrl/src/rules.rs:28`, `em-agent/src/wsc.rs:32`). No output depends on iteration order today.
- **Logging:** `tracing` macros inside cores (12 calls in `em-core` and `em-orch`, more in the role crates).

## Decision

### D1. The core rule

A **core** is a synchronous type that takes an input plus the current time and returns what to do. A core:

- does not depend on tokio, and uses no `std::net`, `std::fs`, `std::thread` or `std::process`;
- never reads a clock (`Instant::now`, `elapsed`, `SystemTime`, tokio's `Instant`): `now` is a parameter, the core reports
  its next deadline, and measurement time comes through an injected clock (C2);
- never reads OS randomness: it takes a `RandomSource` (C3); hash maps follow C4;
- never sends, spawns, sleeps or blocks: every action is a returned value;
- keeps the 0004 rules (no `unsafe`, no clone).

Behaviour that the daemon or a peer can observe changes only where listed in **Intentional differences**.

### D2. One loop per process: `em-event-loop`

- **`em-event-loop`** is a new, **general-purpose** crate with no dependencies and nothing specific to EasyMesh (U13):
  a libae-style event loop. Its core, `LoopCore`, knows only:
  - **sources**, registered by the application at start-up, each with a position in the order and a **budget** (the most
    events served per pass);
  - **readiness**, reported by the driver for external sources and by the application for internal ones (a queue another
    source fills), and checked again when each source's turn comes;
  - **pause**: the application may pause sources and deadlines, which are then served zero times; a pause or resume takes
    effect at once, for every later turn in the same pass;
  - **deadlines**: each handler reports `next_deadline()`; every due, unpaused one runs at the end of every pass;
  - **budgets**: a number of turns per pass, or "until not ready";
  - **actions**: the application's own type, returned in the order produced and never inspected;
  - **time**: a generic type `T: Ord + Copy` (U16).
  It holds no queue, no buffer and **no number or drop rule**: every budget, limit and drop policy is the application's
  policy, passed in (U15).
- **Turn-by-turn API (U28).** `LoopCore` never calls the application. The application asks `next_turn()` and gets
  `Serve(source)`, `Deadline(handler)` or `PassDone`; it serves that turn, routes actions between its own handlers at once
  (so a role frame is in the outbound buffer before the write source's turn), and reports readiness and pause changes back.
  Only external actions (I/O, `Offload`, `Reply`) leave the pass. No callbacks, so no `&mut` passes through a generic.
- **Driver.** The crate also has a small async driver (U14) written against an `Io` trait (wait until a source is ready or
  the deadline passes; perform these actions). It needs no tokio: the `Io` implementation does the I/O. **Driver rule:**
  between two passes it performs every external action (including the writes) and refreshes readiness, without blocking.
  The driver is outside the proof scope (async) and has its own test; `LoopCore` is inside.
- **Handlers.** In `em-agent` and `em-ctrl` the transport (`TransportCore`) and the role core are handlers of one loop.
  Handlers **return actions**; they never call each other. The application routes actions between them (D3b). The
  composition is generic and defined in each role crate (`em-event-loop` cannot depend on them); no `dyn`, owned events, and
  no `&mut` passed through a generic (an Aeneas limit).
- **Other sources stay tasks.** Admin connections (0014 D16), the IPC and rbus bridges (0002 D1, D7), platform
  confirmations (`em-agent/src/platform.rs:98-112`) and the probe's FIFO reader stay small tasks that feed the loop through
  bounded channels. Those channels are loop **sources**; "no inner channels" applies only between the transport and the
  role.
- **Replies.** A request that wants an answer arrives with an id; the `Io` implementation keeps its reply sender in a map keyed
  by that id, the core returns `Reply(id, value)`, and the `Io` implementation sends it. No core holds a tokio sender (amends 0002 D7, where the
  role actor held the `oneshot::Sender`, and the 0014 D16 wording).
- **The tokio `Io` implementation** waits in one `select!` on the data socket (`ready(Interest)`, `try_read`,
  `try_write`), the source channels, the stop signal, job completions and `sleep_until` the deadline `LoopCore` reports,
  and performs the returned I/O. The control socket is used only for registration, as today. It is a module of `em-transport` (U19). 0004 R1
  stands: tokio, existing features, no new dependency, no `unsafe`.
- **Runtime.** The binaries keep `multi_thread` with 2 workers (0012 D1 unchanged); the loop is one task.
- **Amended:** 0004 R3 (actors joined by channels → handlers in one loop, for these two components), R4 (timers as
  `select!` branches in each actor → the loop's deadline, as 0012 D11 already does for the role), R6 (the transport hands
  `mpsc` receivers to the role → actions routed by the loop); 0002 D1 (the role actor never awaits a send and forwards with
  `reserve()` → the loop routes; bridges → sources). The rule of 0002 D1 that one owner holds the data model per process is
  kept: the loop owns it through the role handler.

### D3. One pass of the loop (mechanism, `em-event-loop`)

After each driver wake, `LoopCore` runs one pass, then another at once if anything is still ready:

1. serve each ready, unpaused source in its configured order, at most its budget, one turn at a time (U28); readiness and
   pause are checked when each turn comes, so changes made earlier in the pass count;
2. run every due, unpaused deadline;
3. return the external actions in the order produced.

Paused sources and deadlines are served zero times; nothing else is decided here. Property P6 states this for any
application.

### D3b. How `em-agent` and `em-ctrl` configure the loop (policy, in the applications)

**Ownership.** `TransportCore` (`em-transport`) owns the frame splitter, the `Reassembler`, the received queue, the outbound
buffer, the transport timers and the counters; it enforces a `TransportPolicy` that the application passes in and holds no
number of its own (U15). The role core owns its own timers. The **numbers** below are constants of `em-agent` and `em-ctrl`.
Proof targets follow ownership: P0 to P5 belong to `TransportCore` (stated for any policy values), P6 to `LoopCore`.

**Sources and budgets (U11):**

| Order | Source | Budget | Paused while a job is out (U7) |
|---|---|---|---|
| 1 | data-socket read (bytes the driver read before the pass, through the splitter and reassembly into the received queue; frames left in the splitter keep the source ready; end of file or an error becomes a session event) | 64 frames | no |
| 2 | registration verdict | 1 | no |
| 3 | stop signal | 1 | no |
| 4 | `JobDone` | 1 | no |
| 5 | platform, admin, bridges, probe FIFO, each a source | 1 each | yes |
| 6 | received queue → role (internal source: ready while the queue is not empty) | 1 SDU | yes |
| 7 | data-socket write | until not ready: one `Write` action per pass; the bytes accepted come back from the driver as an event | no |

Deadlines: the role's `tick` (its timers in upstream order, 0012 D10, D11; paused while a job is out) and the transport
timers (reconnect delay, write stall, stop limit, linger; never paused). The application routes actions at once: role frames into
the outbound buffer, `Offload` and `Reply` to the `Io` implementation, link changes to the role in order. **While a job is
out (U29)**, link changes for the role are held in order and delivered after `JobDone`, collapsed to at most one
`unregistered()` followed by one `registered()` with the latest AL MAC.

**Policy home (U31):** the values in this section live in one `policy` module of `em-transport`, separate from
`TransportCore` (which still holds no number, U15), shared by `em-agent`, `em-ctrl`, the probe and the tests.

**Received queue (U1, U2):** at most 1024 SDUs and 4 MiB, measured as the sum of `cmdu.len()`; when either limit would be
passed, the oldest SDUs are dropped (`dropped_receive_overflow`). An SDU above 1 MiB is refused during reassembly (D4).

**Outbound buffer:** per session, with partial writes tracked; at most 4 MiB (sum of `cmdu.len()`); when full while connected, the oldest SDU not yet started is dropped and counted in `dropped_send_overflow` (U12). **One counting rule for
every session end** (write error, end of file, write stall, stop limit): a partly written SDU counts as
`dropped_send_failed`; every SDU not yet started counts as `dropped_send_disconnected` (as 0011 A7 counts SDUs behind a
failed write).

**Write stall (U4):** if queued outbound bytes make no progress for 5 s in a session, the session ends (counted by the rule
above) and the transport reconnects.

**Stop (U8):** when the stop signal is handled, the role gets no further input and the data socket is no longer read
(as today, `actor.rs:247-275`). Queued outbound bytes are written for at most **900 ms**; then whatever is left is counted by
the rule above; then the sockets stay open for the **100 ms linger** (U3), also after a failed write (as today,
`actor.rs:268-275`); then they close. No reconnect. The whole stop takes at most 1 s, as `TRANSPORT_STOP_WAIT` bounds it
today (`em-agent/src/actor.rs:27, 200`).

### D4. `em-transport`: `TransportCore`

Into the core, behaviour unchanged unless D3b or U1 to U8 change it:

- registration: response decoding, rejection (fatal), all-zero AL MAC (retry), error kinds mapped to retry or fatal
  (`actor.rs:436-508`). The core returns the verdict; the `Io` implementation keeps the original `io::Error` and builds
  `TransportError::Io` from it, so the message and `source()` are unchanged (`lib.rs:153, 162`);
- session numbering and link-state values (`:293-297`, `:361`);
- per outbound SDU: write, or drop and count (another session `:321-327`; more than 256 fragments `:372-378`; while
  disconnected `:190-195`; at a session end or stop, D3b); write outcome counted after the bytes leave or the write fails
  (`:380-386`);
- per received frame: the splitter's buffer and skip state as a sync type (from `frame_reader.rs:69`), oversize, malformed,
  reassembly, counters, the received queue (`:392-433`, `wire.rs:182-270`);
- **size cap (U2, U6):** when a set's joined payload would pass 1 MiB, the set is refused: counted once in a new counter
  `dropped_receive_too_large`, not in `dropped_receive_fragments` and not in `sdus_received`. The `Reassembler` then skips
  the rest of that set (frames with the set's addresses and the next ids) until its last fragment, an id 0 or an
  unfragmented frame, without counting them. This is a new receive error that keeps the session (amends 0011 D3 step 6);
- session end: reconnect or stop (`:347-356`), the abandoned-fragment count, AL MAC cleared (`:359-361`),
  `dropped_receive_stopped` at the end (`:125-130`);
- policy: every limit and timer value above comes from `TransportPolicy` (U15);
- counters: a plain field (U18). The counter docs (`lib.rs:170-199`) are rewritten for the new rules, and the receive balance becomes
  `sdus_received` = handed to the role + `dropped_receive_overflow` + `dropped_receive_stopped`.

Follow-ups this affects:

- **Closed by construction:** the A6 held-permit loss (no permits exist).
- **Closed by a new policy:** the A7 no-time-limit write and the one-direction stop hang (U4, U8); the stale-session path
  (per-session outbound buffer counted at session end); the inexact 100 ms cutoff (the cutoff becomes "bytes queued when
  the stop is handled").
- **Not closed:** A8 same-peer interleaving is a limit of the AL-SAP wire format (no message id); the 65 536-octet frame
  limit; the registration-response timeout stays an S0 follow-up in `STATUS.md`.

### D5. `em-agent` and `em-ctrl`

- The role cores become handlers; their inputs and outputs stay as today (`registered`, `unregistered`, `received`, `tick`,
  `platform_confirmed`, `next_deadline`).
- **Link events arrive in order.** On a reconnect the role always sees `unregistered()` before `registered()`; today
  `sync_link` can skip the first because it compares latest values (`em-agent/src/actor.rs:99-122`). The effect is the same,
  since `unregistered()` only clears a flag (`lib.rs:179-181`).
- **0012 D9 items:** session stamps become unnecessary (frames go into the current session's buffer); `PendingFrames` (64,
  drop oldest, `pending.rs:9-29`) is replaced by the outbound buffer (U12, difference `tx-buffer`); "transport ended" becomes
  a loop event; "stopping" becomes the D3b stop with the same 1 s bound.
- **Log lines the harness reads stay word for word**, for example "new session, same AL MAC" (`em-agent/src/actor.rs:112`,
  `run-s2.sh:521`) and the D9 fields.
- **Clock:** an injected measurement clock (U22); the D9 fields `build_us`, `check_us` stay per M1 or M2 and per radio.
- **Randomness:** a type parameter (U23). `new` keeps its signature on the production type (`AgentCore<OsRandom>`); tests pass a seeded,
  test-only source in `em-crypto` (no dependency).
- **Hash maps:** sorted `Vec<(K, V)>` (U32, E4; U24's `BTreeMap` superseded).
- **Logging:** `tracing` kept for now; decided after the spike (U25).

### D6. Blocking work: `Offload` (U5, U7)

- A handler returns `Offload(job)`; the `Io` implementation runs the job with `spawn_blocking`, moving its owned state in and back (no
  clone; the core types are `Send`), and returns `JobDone(result)` as an event. A job that needs randomness gets its own
  `RandomSource`. Replay tests record when each job completes.
- **While a job is out (U7):** the role handler pauses its sources (D3b); the loop keeps reading the transport into the received queue (bounded by U1), keeps writing
  and keeps running the transport timers, but gives the role **no** input (no SDU, no source message, no `tick`) until
  `JobDone`. This keeps 0002 D1 and D10 item 6 ("the actor handles no other input until the call returns"), so handler code
  and upstream parity are unchanged; only the transport no longer stalls. Link changes during a job are held and collapsed
  (U29). A replay test covers a job held across frames, a due role deadline and a session loss.
- **Built with its first consumer**, not before: the `em-db` SQLite store or the 0014 D16 config save, whichever comes
  first. Until then the mechanism is decided but not written.
- **Crypto stays inline** until D9 numbers show the loop stalls too long (0010 D6; user, 2026-11-23). Parity: per-node
  message order matches upstream; cross-node order already differs (0002 D1 trade-off, upstream runs a thread per node).

### D7. Verification

- **Model-based tests** (no dependency): reference models of `LoopCore` (with toy handlers, no EasyMesh), the received queue and `Reassembler`, driven by
  seeded random event sequences.
- **Aeneas spike first, outside the repository** (scratch crate, time-boxed), before `em-event-loop` is written, so its findings
  shape the API (C6 logging, `BTreeMap`/`VecDeque` support). Properties, for the uncapped case unless stated:
  - (P0) round trip: `push` over `encode_sdu_frames(s)` on an empty reassembler delivers exactly `s` with `dropped` 0;
  - (P1) `push` never panics;
  - (P2) a fragmented delivery is the concatenation of the maximal run of consecutive frames with ids 0..n ending at
    `is_last_fragment == 1`, all with the same addresses; an unfragmented frame (`is_fragment == 0`, `is_last_fragment == 1`)
    is delivered alone whatever its id;
  - (P2', expected false) merging `encode(s)` and `encode(t)` from one peer can deliver an SDU equal to neither (A0 B0 A1),
    shown as a counter-example;
  - (P3) within `push`, `dropped` equals strays plus broken continuations counted 2 (`wire.rs:243-249`); the session-end
    `abandon_set` is counted by the caller;
  - (P4) for any policy values, the received queue never holds more than the SDU limit or the byte limit (sum of
    `cmdu.len()`); with the D3b values, 1024 SDUs and 4 MiB;
  - (P5) capped case, for any cap: the partial payload never passes the cap; a refused set is counted once and none of its frames is
    delivered;
  - (P6) `LoopCore`, for any configuration: in every pass, every ready, unpaused source is served at most its budget and in
    order, readiness and pause are checked at each turn, paused sources and deadlines are served zero times, every due,
    unpaused deadline runs, and external actions come out in the order produced. "One SDU per pass" follows from the D3b
    configuration; "reads between passes" and "timers never held back" also need the driver rule (D2), checked by the
    driver test.
- Output: a report in the tracking folder (what extracted, proof hours, toolchain versions). Adopting Aeneas in CI is C11.

### D8. Order of work

1. Accept this note. 2. Aeneas spike (D7), first (U30); it also checks the turn-by-turn API shape (U28). 3. `em-event-loop`
with its model tests. 4. `TransportCore`, the
`Io` implementation and `em-agent` on the loop; the probe and `em-transport` tests move to the loop (U20); `run-s0/s1/s2` pass. 5. `em-ctrl` on the
loop. 6. D5 leaks. 7. Then secure-onboarding Stage 2. `Offload` is built with its first consumer (D6). Steps 3 to 5 are
HIGH_RISK briefs, each with plan review and the full review set.

**Tests retired or retargeted at step 4** (their mechanism no longer exists): held send permit, close-and-length cutoff,
linger timing via permits, `abort()` skipping the final counts. Each is listed in the step-4 brief with its replacement test.

## Intentional differences (observable by the daemon or a peer)

| Id | Change | Was |
|---|---|---|
| `rx-queue-1024-4mib` | Received queue: 1024 SDUs and 4 MiB (sum of `cmdu.len()`), drop oldest (U1) | 256 SDUs, count only (0011 A5) |
| `rx-sdu-cap-1mib` | An SDU over 1 MiB is refused during reassembly, own counter, rest of the set skipped (U2, U6) | No cap below the framing limit |
| `rx-read-between-passes` | Reads (64 frames per pass) happen between passes, interleaved with role calls in one task (D3b, driver rule) | Transport read in parallel with the role |
| `tx-write-stall-5s` | A write making no progress for 5 s ends the session (U4) | No time limit (0011 A7) |
| `tx-buffer` | Role frames wait in the per-session outbound buffer: 4 MiB, drop the oldest not yet started, `dropped_send_overflow` (U12) | `PendingFrames` 64, drop oldest, plus 64 in the channel (0012 D9) |
| `tx-failed-write-count` | An SDU whose write fails before any octet left counts as `dropped_send_disconnected`; only a partly written SDU is `dropped_send_failed` (E11, the D3b rule) | `dropped_send_failed` for the failed write (the actor) |

Kept, no difference: the 100 ms linger (U3); the 1 s stop bound (U8); no role input during a blocking call (U7).

## Alternatives considered

| Alternative | Why not chosen |
|---|---|
| Pure cores under the current tokio actors (rev 1) | Keeps the channel mechanics unprovable and two hand-written `select!` loops |
| libae loop on `mio`/`epoll` now | New dependency or `unsafe`; the `Io` implementation can be swapped later (C11) |
| Keep the current shape | Decisions stay inside async code; the S0 follow-ups stay hard to reason about |
| Model checking the async code (`loom`) | Explores interleavings of primitives, does not prove protocol rules; adds a dependency |
| A larger count-only queue (10 240) | Bounds no memory (one SDU can be 16.7 MB at the framing limit) and turns drops into late answers past the 1 s and 5 s limits |
| Continuations at database call sites | More concurrency, but every call site changes and parity needs new tests (U7 chose holding role input) |

## Acceptance criteria (for D8 steps 3 to 6)

1. `em-event-loop` has no dependencies; no core reads a clock, randomness or a socket, by the U27 mechanism.
2. Existing test assertions keep their meaning, except the tests retired in D8 (each with a replacement); harnesses and
   constructors may change; tests may move from actors to cores.
3. Model-based tests for `LoopCore` (P6), the received queue (P4) and `Reassembler` (P0 to P3, P5; P2' as a
   counter-example); the driver test (driver rule); the outbound buffer bound (`tx-buffer` test); a replay test per role core
   with a fixed clock and seeded source (these check output determinism only); the U7 replay test; a test per intentional
   difference, tagged with its id.
4. `run-s0.sh`, `run-s1.sh`, `run-s2.sh` pass; `run-s2.sh` fails if any of the three D9 lines has no sample.
5. Standard checks on stable and 1.85.0 (`cargo test`, `clippy -D warnings` pedantic, `fmt`, `doc`).
6. Each D4 follow-up is closed with a test or reported open; `STATUS.md`, crate READMEs and the root README status line
   updated.

## User decisions

| # | Date | Decision |
|---|---|---|
| U1 | 2026-11-23 | Keep a received queue (not direct delivery): at most 1024 SDUs and 4 MiB in total, dropping the oldest when either limit would be passed |
| U2 | 2026-11-23 | Refuse a single SDU above a size cap, with its own counter, enforced during reassembly |
| U3 | 2026-11-23 | Keep the 100 ms linger after the drain |
| U4 | 2026-11-23 | A 5 s write-stall timer ends the session (reverses the 0011 A7 "no time limit" decision) |
| U5 | 2026-11-23 | Blocking work through `Offload` and `spawn_blocking`; the runtime stays `multi_thread`; crypto inline until measured |
| U6 | 2026-11-23 | SDU size cap 1 MiB (the daemon's largest CMDU is about 373 KiB: 256 fragments of at most 1500 octets, `ieee1905-rs` `cmdu_handler.rs:48, 79`, one-octet fragment id `cmdu_codec.rs:2339`) |
| U7 | 2026-11-23 | While a job is out, the role gets no input; the transport keeps reading and writing (keeps 0002 D10 item 6) |
| U8 | 2026-11-23 | A stop takes at most 1 s, as today: 900 ms to drain, then the 100 ms linger |
| U9 | 2026-11-23 | Crate name `em-event-loop` (D2) |
| U10 | 2026-11-23 | Only the transport and the role are handlers; admin connections, the IPC and rbus bridges, platform confirmations and the probe's FIFO stay tasks feeding the loop through bounded channels (D2) |
| U11 | 2026-11-23 | The D3 pass order, with a read budget of 64 frames and one SDU to the role per pass |
| U12 | 2026-11-23 | Outbound buffer (C8): 4 MiB; when full while connected, the oldest SDU not yet started is dropped and counted in a new counter `dropped_send_overflow` |
| U13 | 2026-11-23 | `em-event-loop` is a general-purpose libae-style loop, not specific to EasyMesh; the applications instantiate and configure it |
| U14 | 2026-11-23 | The small async driver (over an `Io` trait, no tokio) lives in `em-event-loop` |
| U15 | 2026-11-23 | Every number and drop rule (budgets, queue and buffer limits, size cap, timer values, what to drop) is the applications' policy, passed in; `em-event-loop` and `TransportCore` hold none |
| U16 | 2026-11-23 | C12: generic time type `T: Ord + Copy` in `em-event-loop`; the applications use `Instant` |
| U17 | 2026-11-23 | D4 accepted: `TransportCore` makes every transport decision; the tokio `Io` implementation only does I/O |
| U18 | 2026-11-23 | C5: counters are a plain field of `TransportCore`, read by tests and the probe; no `watch` |
| U19 | 2026-11-23 | C9: the tokio `Io` implementation is a module of `em-transport`, shared by both binaries and the probe |
| U20 | 2026-11-23 | C10: the probe becomes a loop user (FIFO reader as a source) and the 63 integration tests move to the loop; no `spawn()` adapter |
| U21 | 2026-11-23 | D5 accepted: the role cores become handlers with unchanged inputs and outputs; link events in order; `PendingFrames` and session stamps removed; harness log lines kept word for word |
| U22 | 2026-11-23 | C2: an injected measurement clock; the D9 fields `build_us`, `check_us` stay per M1/M2 and per radio |
| U23 | 2026-11-23 | C3: a type parameter `AgentCore<R: RandomSource>` / `CtrlCore<R>`; `new()` stays on the production type |
| U24 | 2026-11-23 | C4: `BTreeMap` in cores where the key is `Ord`, decided per map (**superseded by U32, E4**) |
| U25 | 2026-11-23 | C6: keep `tracing` for now; decide after the Aeneas spike |
| U26 | 2026-11-23 | D6, D7, D8 accepted: `Offload` with paused role sources, built with its first consumer; model-based tests plus the Aeneas spike first (P0 to P6); the order of work |
| U27 | 2026-11-23 | C1: `em-event-loop` is its own dependency-free crate (compiler-enforced); the other cores are modules checked by workspace `clippy.toml` bans, allowed by `#[allow]` only in I/O and test modules, after a check that the bans fire on 1.85 clippy |
| U28 | 2026-11-23 | `LoopCore` is a turn-by-turn scheduler: the application asks for the next turn, serves it, routes actions at once and reports readiness and pause back; no callbacks |
| U29 | 2026-11-23 | While a job is out, link changes for the role are held and delivered after `JobDone`, collapsed to one `unregistered()` then one `registered()` |
| U30 | 2026-11-23 | The Aeneas spike runs first, before `em-event-loop` is written |
| U31 | 2026-11-23 | The EasyMesh policy values live in one `policy` module of `em-transport`, separate from `TransportCore` |
| U32 | 2026-11-23 | (E4) Maps inside cores are sorted `Vec<(K, V)>`, replacing U24's `BTreeMap` |
| U33 | 2026-11-23 | (E5) The size cap applies to fragmented sets only |
| U34 | 2026-11-23 | (E6) An SDU larger than the byte limit is dropped alone, counted; the queue is unchanged |
| U35 | 2026-11-23 | (E7) `Serve` clears ready and `Deadline` clears the deadline; the caller re-arms them |
| U36 | 2026-11-23 | C6: logging in proven cores goes through a project macro compiled out under `cfg(aeneas)`; the real `tracing` crate is not probed |
| U37 | 2026-11-23 | (E10) No "until not ready" budget; a source or handler number that does not exist panics; `begin_pass` during a pass abandons it; `has_ready_source` ignores budget-0 sources (`em-event-loop` spec rev 3) |
| U38 | 2026-11-23 | Timers: one deadline per handler in `em-event-loop`, set by the application wiring with a fixed `HandlerId`; "none" cancels. No timer ids in the loop. A separate `TimerTable` with ids is a possible later component. The wiring syncs every handler's `next_deadline()` after every turn |

## Open choices (the user's)

| # | Choice | Options | Recommendation |
|---|---|---|---|
| C1 | Enforcement | decided: U27 | — |
| C2 | Measurement clock | decided: U22 | — |
| C3 | Random source | decided: U23 | — |
| C4 | Hash maps in cores | decided: U32 (E4; was U24) | — |
| C5 | Counters | decided: U18 | — |
| C6 | Logging in cores | decided: U36 | — |
| C7 | Crypto offload | when D9 numbers require it | deferred (user) |
| C8 | Outbound buffer when full while connected | decided: U12 | — |
| C9 | Where the tokio `Io` implementation lives | decided: U19 | — |
| C10 | Probe and `em-transport` tests | decided: U20 | — |
| C12 | Time type in `em-event-loop` | decided: U16 | — |
| C11 | Later | a `mio`/`epoll` `Io` implementation; Aeneas in CI | after D8 step 2 |

## Errata after the Aeneas spike (2026-11-23)

The spike (D8 step 2; report in the tracking folder, `aeneas-spike/report.md`) proved P0 to P5 on a copy of the
`Reassembler` (from `b4f8343`) with prototypes of U1 and U2, and P6 on a turn-by-turn `LoopCore` prototype: 39 theorems,
no `sorry`, only Lean's standard axioms. These errata change the text above where the spike showed it wrong or incomplete.

- **E1 (P1).** P1 holds under two hypotheses: a reassembler state invariant (at most 256 fragments of at most 65 536
  octets in progress; true for `default`, kept by `push`) and frames of at most 65 536 octets (the frame splitter's limit).
- **E2 (P2).** P2 is two theorems: a run delivers its concatenation, and every delivery is such a run. "Maximal" is not
  needed: a run starts at id 0, where `push` restarts.
- **E3 (P6).** "External actions come out in the order produced" is not a `LoopCore` property (actions live outside it);
  it moves to the driver and application tests (AC3). P6's "timers never held back" holds when every budget is a count
  (all of D3b); with `UntilNotReady` it needs the caller to report not ready eventually.
- **E4 (U24 → U32).** Maps inside cores are sorted `Vec<(K, V)>`, not `BTreeMap`: Aeneas has no `BTreeMap` model, and the
  maps are small. Deterministic as before.
- **E5 (U2, D4 → U33).** The 1 MiB cap applies to fragmented sets only; an unfragmented frame is already at most
  65 536 octets.
- **E6 (U1 → U34).** A received SDU larger than the byte limit (or any SDU when a limit is 0) is dropped alone and counted
  in `dropped_receive_overflow`; nothing already queued is dropped for it.
- **E7 (U28 → U35).** `Serve` clears the source's ready flag and `Deadline` clears the handler's deadline; the caller
  sets them again when there is more work. This removes the "never reported not ready" loop.
- **E8 (Rust style for verified cores).** Index loops, not iterator chains (`collect`, `chunks`, `for` over a `Vec`
  need hand-written specs); `Vec`, not `VecDeque` (or a reviewed list model, as in the spike); small `-> bool` helpers
  instead of `&&`/`||` with early returns or `if let` joins in long functions; no `match` that yields a comparison inside
  a long function; counters with invariants, since every `+` is checked. `#[cfg(aeneas)]` variants are allowed and tested
  both ways. A "no axioms" check runs `#print axioms` per theorem.
- **E9 (C6, C11 inputs).** Logging: a project macro expanding to `#[cfg(not(aeneas))] { tracing::… }` keeps logging out of
  the proofs (the real `tracing` crate was not tried). CI cost: about 19 GB of cached image and volume, a pinned Aeneas
  commit with its own Rust nightly next to the 1.85 MSRV, about 1 minute per proof check. C6 and C11 stay open.
- **E10 (budgets, write source → U37).** The budget "until not ready" is removed (`em-event-loop` spec rev 3): every budget
  is a number, so a pass always ends. In D2 and D3b read the write source as `Count(1)`. Its ready condition is defined in
  the `TransportCore` brief; it must not stay ready while the socket is known to be blocked, or the driver would spin.
- **E11 (D8 step 4b-3: `em-transport` on the loop).** Decisions of the step 4b-3 brief, recorded here (D2, D4, D8 step 4):
  1. **Write time.** The tokio `Io` calls `try_write` in `perform`, right after the pass, and the core takes the time of that pass (`wrote(now, n)`). `write_ready(now)` and `write_chunk(now)` refuse once the stall deadline (in a session) or the stop write limit (stopping) is due at `now`, so a due deadline wins over a write at the same `now`. This fixes the two `TransportCore` deadline-order defects of step 4b-2b (core tests `a_due_stall_deadline_wins_over_a_write_at_the_same_now` and `a_due_stop_limit_wins_over_a_write_at_the_same_now`).
  2. **Write copy.** The `Write` action carries a copy of the chunk (at most one frame), instead of lending the buffer out and back.
  3. **Arming.** Reads and app-channel receives happen only when an action asks for them (`Read`, `RecvApp`), once per action, so the splitter and the application input stay bounded. One application channel serves all application sources for now.
  4. **Old actor.** `spawn` and `tests/session.rs` stay until `em-agent` and `em-ctrl` leave the actor (steps 4c and 5), then both are deleted. The U20 move of the tests to the loop is met by `tests/loop_session.rs`.
  5. **Driver (amends D2).** When a source is still ready after a pass, the driver calls `Io::yield_now` and runs the next pass, instead of going straight to it. Before this, a source that stayed ready (a producer that kept the application channel full) held a current-thread runtime, and frames were delayed for tens of seconds; `continuously_ready_sender_does_not_starve_reads_on_a_current_thread_runtime` pins it. `LoopCore` and the frozen proofs are unchanged (0017 A2).
  6. **Failed write count.** An SDU whose write fails before any octet left counts as `dropped_send_disconnected`, under the D3b rule. The actor counted it as `dropped_send_failed`. This is an observable difference, row `tx-failed-write-count`, pinned by `failed_write_ends_the_session_and_counts_the_unstarted_sdu_as_disconnected` in `tests/loop_session.rs`.
  7. **Yield polls without waiting (review round 1).** The driver delivers the events found during `Io::yield_now` before the next pass, as after a wait. `TransportIo::yield_now` yields to tokio and then polls the branches of `wait` (registration attempt, readable, writable, application receive) without blocking, so a busy application channel cannot hold back a registration or a blocked write (`registration_completes_while_the_application_channel_is_always_busy`, `blocked_write_resumes_while_the_application_channel_is_always_busy`).
  8. **Limits kept (review round 1).** A read armed before a stop may take up to 64 KiB during the stop; the core ignores it, as the actor left it unread (nothing counted either way). End of file and a stop handled in the same pass: the read turn comes first, so the session ends without the linger and its queued SDUs count as `dropped_send_disconnected` (the actor took its stop path). Items still in the application channel when the transport finishes on a fatal registration verdict are not counted: they never reached the transport (the probe and the tests; in step 4c the role is in the loop and has no such channel).
  9. **Writable before readable (review round 2).** In the shared `select!` the blocked-write branch is polled before the armed-read branch, so an inbound flood cannot keep a blocked write from resuming until the 5 s stall. Writable is enabled only while a write is blocked and cleared once stored, so it cannot starve reads. This is a fix by reasoning: `blocked_write_resumes_while_the_service_floods_the_client` guards the scenario but passed with the old order too (the starvation needs a flood that matches the loop's pass rate). Not reviewed by a third Codex round (two-round limit). Kept as low: an already-due deadline returns after one tokio yield rather than on the first poll; the busy-channel write test checks that the socket filled, not that a `WriteBlocked` event occurred.

- **E12 (D8 step 4c: `em-agent` on the loop).** Decisions of the step 4c brief, recorded here (D2, D3b, D5, D8 step 4):
  1. **Stop and platform share the application channel.** `TransportIo` has one application channel. A forwarder future, polled in the same task as the loop, moves platform confirmations into it and drops its sender when `stop` completes; `AppClosed` is the stop signal. D3b orders the stop (3) before the platform (5); here a stop waits behind at most the confirmations already queued (channel of 8). A closed platform channel is not a stop, as before.
  2. **Link changes by comparison.** After every turn the app compares `link()` with the link the role was last told and emits the changes in order; a session change with both AL MACs present gives `unregistered()` then `registered()`. This replaces the watch-based `link` copy of the actor. The function `link_changes` is pure and tested on its own, including the case where the loop never saw the loss.
  3. **AL MAC change ends at once**, without the transport stop, as the `?` of the actor did.
  4. **`ActorError` (the variants that can no longer occur are removed).** `TransportFailed(JoinError)` and `TransportStopped` go, since the transport is no longer a task that can fail to join or stop by itself. The other variants and their text stay.
  5. **Role frames.** A frame goes straight into the current session's outbound buffer, stamped with that session and the recorded AL MAC. `PendingFrames` is removed; its difference is the `tx-buffer` row.
  6. **Log lines.** Kept: `transport stopped` (info) and `transport ended with an error while stopping` (warn), both with the target of the actor module. Removed, because they cannot occur in the loop: `transport task failed while stopping`, `transport did not stop within 1 s, aborting it` (the loop has no task to abort), and the pending-frames warning.
  7. **Left for step 6.** The replay tests and the clock, randomness, `HashMap` and logging leaks of the role (the "D5 leaks") are not changed by this step.
  8. **The old actor stays for `em-ctrl`.** `spawn` and `tests/session.rs` stay until step 5, as item 4 of E11 says for step 5 alone.
  9. **A write in flight is kept (review round 1, a defect of 4b-3).** The `Io` writes a chunk after the pass, but a later turn of the same pass (the role timer runs after the write source) may send. With the buffer full, that send could drop the SDU whose bytes were already handed out, and the write result then advanced the next SDU (wrong octets on the wire, or a panic). `TransportCore::write_issued` now marks that SDU; the outbound buffer keeps it, like a started SDU, until the result (`wrote`, `write_blocked`) or the session end. A session end still counts it as not started unless octets of it were reported written. So `tx-buffer` drops the oldest SDU that is neither started nor being written.
  10. **No turn after the end (review round 1).** Once the transport has an outcome or the AL MAC changed, the application serves no further turn of that pass, so a stop in the same pass no longer turns a fatal transport error into `Ok`, and an AL MAC change never runs the transport stop.

## Revision history

- errata E12 (2026-11-25): D8 step 4c (`em-agent` on the loop, decisions 1 to 4 of the step 4c brief; items 9 and 10 from the Codex review round 1); the log and error changes, and the facts that keep the replay tests and the D5 leaks for step 6. Recorded as an erratum, not by editing the text above.

- errata E11 (2026-11-25): D8 step 4b-3 (`em-transport` on the loop, decisions 1 to 6); a row added to the intentional differences; D2's driver amended. Recorded as an erratum, not by editing the text above, except the new table row and the status line.

- 0017 A2 (2026-11-24): D7's transport proof targets narrowed to the `Reassembler` (P0 to P3, P5); the `LoopCore` (P6)
  proofs stay, frozen; P4 (received queue) dropped, its
  model-based test stays. D8 steps 4
  and 5: Codex review plus the user, plan review on request. Recorded in 0017 A2, not by editing the text above.

- errata E1 to E9 (2026-11-23): after the Aeneas spike; U32 to U36.
- rev 5 (2026-11-23, accepted): the step-by-step walk with the user: step 1 accepted: U9 (name `em-event-loop`), U10 (handler scope); step 2 accepted: U11 (pass order), U12 (outbound buffer); U13 to U15 (general-purpose loop, driver in the crate, all policy in the applications): D2 and D3 split into mechanism (D3) and EasyMesh configuration (D3b); P4 to P6 stated for any policy. Step 3 accepted: U16 to U20 (time type, D4, counters, `Io` in `em-transport`, probe and tests on the loop). Step 4 accepted: U21 to U25 (D5, measurement clock, random type parameter, `BTreeMap`, logging after the spike). Step 5 accepted: U26 (D6 to D8), U27 (enforcement). Walk complete. Final review applied: U28 to U31; deadlines pausable and pauses immediate (from U7); readiness reported by the application for internal sources; the driver rule; leftover "backend" and D3 references fixed; AC3 attribution and the outbound measure.
- rev 4 (2026-11-23): user decisions U7 (no role input while a job is out) and U8 (1 s stop); plan review of rev 3 applied:
  D3 rewritten as one bounded pass that serves every source and every due deadline each pass (P6); ownership of the queue
  and buffer set (`TransportCore`); one counting rule for every session end; the stop rule; the U2 refusal's after-state,
  counters and the 0011 D3 step 6 amendment, with P5; the 4 MiB measure; replies by id (0002 D7); 0002 D10 item 6 and the
  0012 D9 stop bound in Amends; `tx-buffer` added to the differences; `Offload` built with its first consumer; the
  registration-response timeout returned to `STATUS.md`; cites fixed (`platform.rs:98-112`, 0012 D1, 63 tests).
- rev 3 (2026-11-23): user decisions U1 to U6; plan review of rev 2 applied.
- rev 2 (2026-11-23): redesigned around one verifiable loop per process (`em-event-loop`, user proposal); plan review of rev 1
  applied.
- rev 1 (2026-11-23): first draft: pure cores under the existing tokio actors.
