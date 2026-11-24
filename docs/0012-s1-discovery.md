# 0012: Slice S1: controller discovery (AP-Autoconfiguration Search and Response)

| | |
|---|---|
| Status | **Accepted** (2026-11-21, user), rev 7 after S1 review round 1 (user decisions on R1 Searches, when to answer, SupportedService, dependencies) |
| Date | 2026-11-21 |
| Upstream | `rdkcentral/unified-wifi-mesh` @ `ba4180ba36748629f10c20babc4320dd69c9db4f` |
| Follows | [0010](0010-vertical-slices-and-transport.md) D1 (slice S1), D5, D7, D8; [0011](0011-em-transport-s0.md); [0002](0002-cmd-core-orch.md) D1 to D4, D8; [0004](0004-runtime-and-ownership.md) R5 |
| Amends | [0005](0005-em-msg-scope.md) D3 and [0008](0008-tlv-body-codec.md) D8 (validation rows, D6 below); 0004 R5 (log subscriber, D7 below); [0002](0002-cmd-core-orch.md) D1 and 0004 R4 (deadlines without `DelayQueue`, D11 below); 0002 D2 and D4 (`OrchRole` shape and the two-phase submit), D1 "When effects apply" and D3 (where the orchestration state and `OpContext` live), all in D10 below |
| Affects | new crates `em-cmd`, `em-orch`, `em-core`, `em-easyconnect`, `em-agent`, `em-ctrl` (each minimal); `em-msg` validation tables; `docker/` |
| Spec | EasyMesh §6.1, §13.1, §17.1.1, §17.1.2, §17.2.47; IEEE 1905.1 §10.1.1 |

## Context

S1 is the first slice with real role processes: an agent that finds its controller, and a controller
that answers. It ends where WSC M1 would start (S2). Upstream cites are to the pin; `ieee1905-rs` cites are to
its pin, as in 0011.

### Upstream behaviour

**Agent startup.** OneWifi sends `dev_init` once (`em_agent.cpp:1869`). Its chain creates the AL node and one
node per radio, each with **profile 3** hard-coded (`em_orch_agent.cpp:255-279`).

**Agent: Search.**
- A radio node searches only while its orchestrator state is `progress` with an active `dev_init` (or
  `cfg_renew`) command (`em.cpp:450-468`). In `unconfigured` it builds and sends a Search, then moves to
  `autoconfig_rsp_pending` (`em_configuration.cpp:6695-6737`). That state sends again (`:6770-6774`), on each
  idle-based 1 s proto tick (`em.cpp:644-680`), with a new MID each time (`:4548-4549`).
- **The search stops after 10 s.** The agent orchestrator cancels `dev_init` once it is older than
  `EM_MAX_CMD_GEN_TTL` (10 s, `em_base.h:112`; `em_orch_agent.cpp:42-74`, checked every 250 ms). The node returns
  to `unconfigured`, the gate closes, and `dev_init` is never sent again (only a DPP-onboarding reset extends it,
  `:52-62`).
- Content (`create_autoconfig_search_msg`, `:4542-4591`), to 01:80:C2:00:00:13 with the relay bit: AL MAC
  Address; SearchedRole 0x00; AutoconfigFreqBand (the radio's band); SupportedService {agent}; SearchedService
  {controller}; Multi-AP Profile (always 3, from node creation); an optional DPP Chirp. Validated against
  profile 3 before sending (`:6720`).

**Agent: Response.** Routed to the first radio node whose band equals SupportedFreqBand and whose state is not
in progress (`em_agent.cpp:2366-2396`); accepted only in `autoconfig_rsp_pending` (`em_configuration.cpp:6540-6546`).
Handling sets the peer profile (falling back to 1), validates against that profile, forwards a Chirp, reads the
Controller Capability, may send an Early AP Capability Report, then sends **M1** and moves to `wsc_m2_pending`
(`:6284-6391`).

**Controller.**
- The controller's AL node exists only after the database is initialised (`em_ctrl.cpp:1120-1147`).
- Routing of a Search (`em_ctrl.cpp:1197-1238`, 0002 D8): read the AL MAC TLV and SupportedService; with no model
  for that AL MAC, request a deferred model commit with the peer profile; otherwise update the model (its
  profile is not changed). The AL node handles the frame.
- Handling (`em_configuration.cpp:6393-6484`): set the peer profile from the Search (falling back to 1),
  validate, read band and AL MAC, forward a Chirp, build and send the Response, move to `ctrl_wsc_m1_pending`.
  SearchedRole and SearchedService values are never checked.
- Response (`:4361-4498`), unicast to the AL MAC from the TLV, with the Search's MID: SupportedRole registrar;
  SupportedFreqBand = the Search's band; SupportedService {controller}; 1905 Layer Security Capability (from the
  data model, `dm_ieee_1905_security.cpp:40-48`); Multi-AP Profile **always 3**; Controller Capability with
  **only** Early_AP_Capability; optional Chirp and extension TLVs. Its own validation result is ignored
  (`:6465-6469`).

**Validation tables** (`em_msg.cpp:719-729`, ported unchanged to `crates/em-msg/src/tables.rs`): in a Search the
Multi-AP Profile is allowed only above profile 1; in a Response the 1905 Layer Security Capability only above
profile 2 and the Multi-AP Profile only above profile 1.

### Where upstream differs from the spec (0010 D7 rule 1: the spec wins where explicit)

| Point | Spec | Upstream | Port |
|---|---|---|---|
| Multi-AP Profile in the Response | §6.1: match the agent's Search (Profile-1 if absent) | always 3 (`:4447`) | follows the spec, from the **current** Search |
| Reserved profile value in a Search | §17.2.47: assume the receiver's own profile | falls back to 1 (`:6409-6418`) | assumes 3 (the controller's own) |
| KiBMiB Counter bit | §6.1: shall be set | not set (`:4457-4458`) | set |
| SupportedService in the Response | §6.1: shall be included when SearchedRole is Registrar and SearchedService is Controller; §17.1.2 allows zero or one | always included | always included, as upstream (user decision, rev 4). The controller answers every valid Search, as upstream (user decision, rev 3) |
| Response without a Multi-AP Profile TLV | §17.1.2: one Profile TLV and one Security Capability TLV are required | accepted below profile 2 and 3, profile 1 assumed (`:6297-6304`) | rejected by validation (D6); tagged `response-profile-required` |
| Validation rows above | §17.1.2: the Profile TLV and the 1905 Layer Security Capability TLV are required in the Response; §6.1 handles a Search without a Profile TLV | forbidden below profile 2 and 3 | Response: mandatory for every profile. Search: optional, an absent TLV means Profile-1 (D6, user decision rev 3) |
| Reserved profile value in a Response | §17.2.47: assume the receiver's own profile | the agent falls back to 1 (`:6297-6304`) | the agent assumes its own `--profile` |

Each row gets a test tagged `intentional-difference:<id>`.

## Decision

### D1. Crates, minimal for S1

| Crate | S1 content |
|---|---|
| `em-cmd` | `Command::DevInit` only, with its plan (`al_insert` F, `em_insert` T) and the status codes it needs (0002 D2, D3) |
| `em-orch` | The synchronous library of 0002 D2 with what `dev_init` needs: chain record, pending and active queues, activation, the per-type timer and cancel. No tokio |
| `em-easyconnect` | Boundary types `em-core` needs (0010 choice 5): the onboarding query of `handle_state_config_none`, which returns "not onboarding" in S1 |
| `em-core` | `Node`, `NodeTable`, `OpContext`, `EngineCtx` and `Outputs` in their 0002 shape, with the fields S1 uses (D10); the configuration engine's Search and Response builders and handlers |
| `em-agent` | Binary. Role actor: `em-transport` client, platform fake (D4), `dev_init` submission, the timers, dispatch |
| `em-ctrl` | Binary. Role actor: `em-transport` client, an in-memory peer-model map (`em-dm` records come with S2's database), its AL node, routing, dispatch |

- Handlers are synchronous over `EngineCtx` (0002 D1). Only the role actors and `em-transport` use tokio; the
  role actors use the multi-threaded runtime with at least 2 workers (0002 D1), which adds tokio's
  `rt-multi-thread` feature to the binaries.
- `em-core`, `em-cmd` and `em-orch` emit through `tracing` (0004 R5).
- Received CMDUs are validated with `em-msg`'s `validate` inside the `em-core` handler, after routing, as upstream
  (`em_configuration.cpp:6419-6422`); routing reads only the TLVs it needs (band, AL MAC, profile, SupportedService). The
  model commit and the existing-model update carry `is_emplus_agent` from SupportedService, as upstream
  (`em_ctrl.cpp:1210-1217`, `1226`, `1230`). The security and duplicate-filter stages of 0010 D5
  are pass-through from S1.
- No database: the controller keeps its model in memory until S2.

### D2. Agent

> **Amended by [0013](0013-s2-onboarding.md) D5 (2026-11-22):** the until-found reset also covers `wsc_m2_pending`, so `dev_init`'s 10 s cancel is unreachable in S2 flows; a radio that waits more than 10 s for its M2 restarts discovery under the same `dev_init`.

- The AL MAC comes from AL-SAP registration. The platform fake (D4) supplies the radios. The agent submits
  `dev_init`, whose chain creates the AL node and one node per radio, as upstream.
- Searching follows upstream (gate, states, resend on the proto tick, new MID), with one **intentional
  difference (user decision):** it continues **until a Response arrives**. On each orchestrator tick, while
  any radio node is in `unconfigured` or `autoconfig_rsp_pending`, `orch_transient` resets the command's start
  time and its type timer, exactly as upstream's DPP-onboarding branch does (`em_orch_agent.cpp:52-62`). The
  10 s therefore counts from the last Response. When it expires, `dev_init` is cancelled as upstream: only the
  radio node being iterated returns to `unconfigured` (`:64-71`): `cancel_command` marks every candidate fini at
  once (`em.cpp:283-293`), so the later candidates of that tick finish without calling `orch_transient`
  (`em_orch.cpp:316-341`). Candidates are iterated from the last, so the reset node is the last in candidate
  order. S1 never reaches `dev_init`'s finish state
  (`owconfig_pending`, `:76-87`), so this cancel always happens after the last Response. **Threshold:** the
  elapsed time is `(now - start).as_secs()` compared with `> 10`, on the 250 ms orchestrator tick, measured from
  the last tick that saw a searching node. Upstream subtracts `tv_sec` values (`em_orch.cpp:85`), which can reach
  11 up to 1 s earlier; `Instant` has no wall-clock second, so the port's cancel can come up to 1 s later.
  Intentional difference, tagged `elapsed-whole-seconds`. Tests pin the cancel, the single-node reset of the
  last candidate, and the boundary: not cancelled at 10.9 s, cancelled by 12 s. Reason: `ieee1905-rs` delivers a Response only after it has heard the
  controller's Topology Discovery, sent every 30 s, so upstream's 10 s search can miss the controller for good.
- A Response is routed and accepted as upstream, whatever its MID (0010 D8: upstream accepts any MID,
  `em_configuration.cpp:6284-6391`; with a resend every second a Response often answers an older one). A reserved
  profile value in a Response means the agent's own `--profile` (§17.2.47). In S1 the handler stops **before M1** (user decision): it
  records the controller's AL MAC and profile, reads the Controller Capability, logs it, and moves the node to
  upstream's `wsc_m2_pending` without sending M1. Routing already skips that state and the resend stops. S2
  adds the M1 send at that point.
- No Early AP Capability Report (S3) and no DPP Chirp (S7) in S1.

### D3. Controller

- **Temporary difference:** `em-ctrl` creates its AL node at registration, with the AL MAC the service returns,
  because there is no database yet. S2 restores upstream's database-driven creation.
- A Search is routed to the AL node as upstream and every valid Search is answered, as upstream (user decision,
  rev 3). The SupportedService TLV {controller} must be included when SearchedRole is Registrar and SearchedService is
  Controller (§6.1); it is included in every Response, as upstream (user decision, rev 4). A Search without a Multi-AP Profile TLV is answered with Profile-1
  (§6.1). The controller validates its own Response; a failure is logged and the Response is still sent, as
  upstream (`:6465-6469`).
- The Response follows upstream's content and order, with the spec rows above. The profile comes from the
  current Search, not the stored model. The controller validates its Response against the profile it sends.
- **Peer model (rev 4, parity).** Upstream requests the deferred commit in routing (`em_ctrl.cpp:1219-1229`),
  before the AL node's handler runs, so a Search that later fails validation or whose Response is not sent still
  creates the model. `CtrlCore` routing does the same: when no model exists for the AL MAC it emits the commit
  whatever the handler then does. The commit is applied after the handler's frames are handed over, keeping
  upstream's order. Its profile is routing's `get_profile` result, the raw value (a reserved value is stored
  unchanged), or 1 when the TLV is absent, as upstream; not the handler's assumed profile. An existing model is
  updated, as upstream (its profile is not changed). A `CtrlCore` test sends a Search with a readable AL MAC and
  band but an invalid TLV set: the model is created and no Response is sent.
- The AL node moves to `ctrl_wsc_m1_pending`. Nothing follows in S1.

### D4. Platform fake (moved here from S0, 0011)

- Command-line arguments of `em-agent` (user decision): `--radio <ruid>,<band>` (repeatable; band as
  `em_freq_band_t`: 0 = 2.4 GHz, 1 = 5 GHz, 2 = 60 GHz, 3 = 6 GHz), the AL-SAP socket paths, and
  `--profile <2|3>`, default 3.
- `--profile` is a **port test switch**: upstream always sends 3. Profile 1 is not offered, because §6.1 then
  also requires a Profile-2 AP Capability TLV in the Search, which neither upstream nor S1 builds.
- A small `Platform` trait in `em-agent`, so the OneWifi adapter can replace the fake later (0001 D7 rule 3).

### D5. 1905 Layer Security Capability in S1

The controller sends the all-zero value (onboarding protocol 0x00, MIC algorithm 0x00, encryption algorithm
0x00), the only values §13.1 defines. It is a constant in S1; S2 reads it from the data model, and S7 makes it
true by implementing the security it claims.

### D6. Amendment to the validation tables (user decision)

In `em-msg`'s tables, the rows for the Multi-AP Profile TLV and the 1905 Layer Security Capability TLV in the
AP-Autoconfiguration Response become **mandatory for every profile** (§17.1.2). In the Search, the Multi-AP
Profile TLV becomes **optional for every profile** (rev 3, user decision): §6.1 says what an absent TLV means, and
upstream answers such legacy agents. The override is a layer applied in `validate()`, like `spec_min`; `TABLES`
and the generated fixture stay as upstream, so `--check` is unaffected. Existing tests that pin upstream's rows
(`search_profile_tlv_is_not_allowed_up_to_profile_1`, `search_profile_tlv_is_mandatory_above_profile_1`,
`response_security_capability_flips_at_profile_3`, and the Profile parts of the Search report tests) become
`intentional-difference` tests, and the module docs of `validate.rs` and `tables.rs` say where the port differs. This amends 0005 D3 ("same rows as upstream") and is an application of 0008 D8. The
upstream rows stay in the table generator's fixture; the override is recorded in `em-msg` like the spec
minimums of `spec_min.rs`, with a test per row.

### D7. Logging (user decision; amends 0004 R5's deferral)

`em-agent` and `em-ctrl` install `tracing-subscriber` (features `fmt` and `env-filter`): plain text on stdout,
level from `RUST_LOG`. Libraries keep using only the `tracing` facade. Upstream's per-module log files remain a
later decision for the role crates.

### D8. Docker exit checks for S1

> **Amended by [0013](0013-s2-onboarding.md) D4 and D5 (2026-11-22):** a second `dev_init` is allowed only for the D4 recovery (the `onewifi_cb` or renew timeout restart); check 6a still expects exactly one in an S1 run.

`em-agent` runs in node 1 (radios 2.4 GHz and 5 GHz) and `em-ctrl` in node 2, in place of the probe. Each role
process runs under the entrypoint's supervisor, which can stop and restart it with new arguments while
`ieee1905-rs` keeps running (an `exec`ed role process would take the container, and the service, down with it).
1. Both register.
2. **Searching past 10 s.** `em-ctrl` is started at least 15 s after the agent's first Search. The agent logs a
   Search per radio, resent about each second with increasing MIDs, and finds the controller only after more
   than 10 s of searching.
3. Within 60 s of the later registration, the controller logs a peer model created for the agent's AL MAC and a
   Response sent per band, and the agent logs the controller found on both radios, with the controller's AL MAC.
4. The agent logs the parsed Response: Multi-AP Profile 3 and the KiBMiB bit set. After the supervisor restarts
   the agent with `--profile 2`, the Responses carry profile 2 and are accepted.
5. After its Response, each radio stops resending.
6. (a) Killing `ieee1905-rs` on either node, after both radios found the controller: the role process logs a
   new session with the same AL MAC, and afterwards logs no `dev_init` resubmission, no Search and no second
   "controller found". (A radio returned to `unconfigured` by the `dev_init` cancel, D2, does not search again:
   the gate needs an active `dev_init`.) (b) Restarting `em-agent`: the new process logs Searches starting again at MID 1; the controller logs
   an existing-model line with a time after the restart and no second "created" line; the new process logs the
   controller found.
7. The workspace checks pass on stable and 1.85.0.

### D9. Link state and reconnects (rev 3; follows 0011 A5-A8)

- **AL MAC.** The agent submits `dev_init` only after the first registration (`link` shows an AL MAC). The
  controller creates its AL node on the first registration only. A later session with the **same** AL MAC
  resets nothing: searching and the node states continue. A **different** AL MAC is fatal: logged, and the
  process exits non-zero (the node table is keyed by it).
- **Session stamps.** Before each handler call or timer dispatch the role actor copies the session from
  `link.borrow()`. Every frame in that call's `Outputs` gets that session, and as `local` the AL MAC recorded at
  the first registration, which D9 fixes for the life of the process (0011 A6). A frame built while the link shows
  `None` (a received SDU of an old session handled late, 0011 A5) is still stamped this way; the transport drops
  and counts it, so no extra rule is needed. Such a handler still moves its node (for example the controller's
  AL node to `ctrl_wsc_m1_pending`); this falls under `state-on-handoff`.
- **While unregistered** (`al_mac` is `None`) the role actor does not dispatch proto ticks, so no proto tick builds
  a frame it cannot send. Upstream moves a state only after
  `send_frame` succeeds (`:6727-6733`, `:6471-6478`); the port moves it when the frame is handed to `Outputs`,
  because AL-SAP gives no per-frame result (0010 D4). Intentional difference, tagged `state-on-handoff`.
- **Bounded pending frames.** Frames waiting for `reserve()` are kept in a queue of at most 64; when it is full
  the oldest is dropped and counted. A blocked transport (0011 A7) therefore cannot grow memory.
- **Transport ended.** The role actor also selects on `&mut task`, on `received` closing and on `link` closing.
  Any of these means the transport has ended (for example a rejected registration, fatal for both roles, 0011
  A3). The actor logs the result and exits non-zero, as upstream's controller exits (`em_ctrl.cpp:1718-1738`).
  Tested on the role actor with a fake service that rejects the registration.
- **Stopping.** On SIGTERM or SIGINT the role actor drops its transport senders and receivers, waits on
  `&mut task` for at most 1 s, then aborts it (0011 A7), and exits 0.

### D10. Interfaces between the crates (rev 4; fixed before any brief starts)

> **Amended by [0013](0013-s2-onboarding.md) D3 (2026-11-22):** role effects are applied per candidate through `OrchRole::apply` with a request buffer drained after the pass, and the readiness hooks can emit effects. This closes the S1 follow-up on effects applied after the pass.

Shared keys and command types live in `em-cmd`, so `em-orch` and `em-core` both see them without depending on
each other (0002 D2). The graph stays acyclic: `em-orch` → `em-cmd`; `em-core` → `em-cmd`, `em-msg`,
`em-easyconnect`, `em-types`; the role crates → all of them.

**`em-cmd`**
- `NodeKey { mac: MacAddr, kind: NodeKind }`, `NodeKind { Al, Radio }` (Copy).
- `ChainId(u64)` (Copy); `CommandType` (Copy; S1 has `DevInit`); `OrchState { None, Idle, Pending, Progress,
  Fini }` (upstream `em_orch_state_t`, `em_base.h:3667-3674`; a cancel is stored as `Fini`, `em.cpp:288-290`, so
  there is no `Cancel` value); `OrchOp { AlInsert, EmInsert }` (the `dev_init` plan); `CmdStatus`.
- `Command::DevInit(DevInit { al_mac: MacAddr, radios: Vec<RadioSpec> })`, `RadioSpec { ruid: MacAddr, band:
  FreqBand }`; `Command::kind() -> CommandType`.
- `trait CommandView { fn command(&self, chain: ChainId) -> Option<&Command>; fn orch_state(&self, node: NodeKey)
  -> OrchState; fn active(&self, node: NodeKey) -> Option<ActiveOp>; }`, with `ActiveOp { chain, stage, kind:
  CommandType }` (Copy).

**`em-orch` owns the orchestration state of every node** (0002 D3: the per-node status lives in the
`Orchestrator`). Nodes do not store an orch state or an `OpContext`; `em-core` reads both through `CommandView`.
- `Orchestrator<R: OrchRole>`; time is always passed in as `Instant`.
- **Two-phase submit**, because upstream pre-processes at submit and builds candidates from the nodes that
  pre-processing created (`em_orch.cpp:42-69`, `142-157`; `em_orch_agent.cpp:240-283`, `338-348`):
  1. `submit(&mut R, Command, Instant) -> (ChainId, Vec<RoleEffect<R::State>>)` runs `pre_process` for each
     stage of the plan; for `dev_init` this yields `CreateNode` effects;
  2. the role applies them to its node table;
  3. `admit(&R, &R::Nodes, ChainId)` builds the candidates; with none, the chain is destroyed, as upstream.
- `tick(&mut R, &R::Nodes, Instant) -> Vec<RoleEffect<R::State>>` runs upstream's pass (`em_orch.cpp:296-345`,
  `352-360`, `532-566`) in upstream's order, and applies the orchestrator's own changes **at once, inside the
  pass**: activation (candidate `Pending`, then `Progress` when `ready_for_exec` holds; the start time is set at
  activation, `:538`), `Fini` when `ready_for_fini` holds, the per-type `Elapsed` update, `ResetStartTime` and
  `Cancel` requested by `transient`, and destroy with every candidate back to `Idle`. So a cancel requested for
  one candidate makes the later candidates of the same pass `Fini` before their `transient` runs, as upstream.
- `on_event(&mut R, &R::Nodes, Instant) -> Vec<RoleEffect<R::State>>` runs activation and readiness only, never
  the `Elapsed` update or `transient` (0002 D1: only activation and readiness run on events).
- `is_type_in_progress(CommandType) -> bool`; `impl CommandView`.
- `trait OrchRole { type Nodes; type State: Copy;`
  `fn pre_process(&mut self, cmd: &Command, op: OrchOp, out: &mut Vec<RoleEffect<Self::State>>) -> bool;`
  `fn build_candidates(&self, cmd: &Command, op: OrchOp, nodes: &Self::Nodes) -> Vec<NodeKey>;`
  `fn ready_for_exec(&self, cmd: &Command, node: NodeKey, nodes: &Self::Nodes) -> bool;`
  `fn ready_for_fini(&self, cmd: &Command, node: NodeKey, nodes: &Self::Nodes) -> bool;`
  `fn on_start(&self, cmd: &Command, node: NodeKey, out: &mut Vec<RoleEffect<Self::State>>);`
  `fn transient(&self, cmd: &Command, node: NodeKey, nodes: &Self::Nodes, elapsed: u64, out: &mut
  TransientOut<Self::State>);`
  `fn pre_process_cancel(&self, cmd: &Command, node: NodeKey, out: &mut Vec<RoleEffect<Self::State>>); }`.
  `TransientOut` collects role effects and the two orchestrator requests (`reset_start_time()`,
  `cancel(CommandType)`), which the orchestrator applies before it reads `elapsed` again, as upstream checks
  `stats->time` right after the DPP reset (`em_orch_agent.cpp:52-65`). `on_start` is upstream's `orch_execute`
  state change (`em.cpp:86-114`: `dev_init` sets `agent_unconfigured`).
- `RoleEffect<S> { CreateNode { key: NodeKey, band: Option<FreqBand>, profile: u8, state: S }, SetState(NodeKey,
  S) }`: the only effects the role applies. Node creation for `dev_init` uses the AL MAC and the radios of
  `DevInit`, the profile from `--profile` (default 3, upstream's fixed value, `em_orch_agent.cpp:259`, `279`), and
  the initial state upstream gives a new node. The agent validates its own Search against profile 3 before
  sending, as upstream (`em_configuration.cpp:6720`).
- A node em-orch has never seen reads as `Idle` (upstream creates every node idle, `em.cpp:3228`); destroy returns
  candidates to `Idle`.
- **Role effects are applied after the pass (S1 only).** `tick` returns the role effects of the whole pass. This
  departs from 0002 D1, where each candidate's effects apply before the next hook runs. In S1 no later hook can see
  the difference: activation sends every `dev_init` candidate straight to `Progress` (`ready_for_exec` is always
  true, `em_orch_agent.cpp:174-175`), and after a cancel the later candidates are already `Fini`. **A later slice
  must restore per-candidate application and give `ready_for_fini` an effects output before the controller's
  `em_config` chain** (`em_orch_ctrl.cpp:196-203` writes node state inside readiness).

**`em-core`**
- `AgentState` and `CtrlState` (the role state enums, upstream's `em_state_t` values S1 uses); `Node<S> { key,
  state: S, profile, band }`; `NodeTable<S>` (insertion order kept for upstream's iteration order), which the role
  crates use as `OrchRole::Nodes` with `S` their state enum. Each role uses one state type.
- `MsgIdCounter` (one per process, upstream `em_mgr.cpp:525-531`), reached through `EngineCtx::msg_ids`.
- `EngineCtx<'a> { nodes, key, commands: &dyn CommandView, msg_ids, now: Instant, out }`. The Search handler reads
  the AL MAC from `commands.active(key)` and the chain's `DevInit`.
- `Outputs`, in order: `Frame { remote: MacAddr, cmdu: Vec<u8> }` (session and `local` added by the role actor,
  D9) and `Progress(NodeKey)`. The proto deadline of each node is owned by the role core, which restarts it on
  every event dispatched to that node (0002 D1); handlers do not set it. The controller's model commit is a
  `CtrlCore` action, not a handler output (D3).
- The controller submits no command in S1. `em-cmd` provides `NoCommands`, a `CommandView` with no chains and every
  node `Idle`, which `CtrlCore` passes as `commands`.
- `admit` returns `true` when the chain has candidates and `false` when it was destroyed; the agent logs the
  second as upstream's "not ready" (`em_agent.cpp:156-159`).
- `OrchRole` is implemented on a separate rules struct, never on `AgentCore`/`CtrlCore` (0002 D2), so the core can
  lend `&mut rules`, `&nodes` and `&orch` from disjoint fields.
- `em-core` → `em-easyconnect`: the onboarding query of `handle_state_config_none` is a free function
  `em_easyconnect::onboarding_in_progress(al_mac) -> bool`, which returns false in S1.

**Role logic without sockets.** The orchestrator rules (`impl OrchRole`), the proto-tick and orchestrator-tick
bookkeeping and routing of each role live in a synchronous `AgentCore` / `CtrlCore` in the role crate, driven by
explicit inputs (`registered`, `received`, `tick(Instant)`) and returning frames and the next deadline. The tokio
role actor only moves inputs in and frames out. The D2 and D3 differences, routing, the timer threshold and the
reconnect rules are tested on these cores with no sockets and no Docker.

### D11. Dependencies (rev 3, user decision "Minimal")

- Per-node and orchestrator deadlines use `tokio::time::sleep_until` on the earliest deadline the core reports,
  not `tokio_util::time::DelayQueue`. This amends 0002 D1 and 0004 R4 for S1; a later slice may add
  `tokio-util` by its own decision.
- Command-line arguments are parsed with `std` only.
- The binaries add tokio's `rt-multi-thread` and `signal` features, and `tracing-subscriber` (D7). The libraries
  add only `tracing`.

## Open choices

**All decided by the user (2026-11-21):**
1. `em-cmd` and `em-orch` are minimal in S1 (D1). This reverses the first answer, after the plan review showed
   that the Search depends on the `dev_init` command.
2. The platform fake takes command-line arguments (D4).
3. S1 stops in upstream's `wsc_m2_pending` (D2). This reverses the first answer (a temporary state).
4. The agent searches until it finds the controller (D2).
5. The validation rows follow the spec (D6).
6. `tracing-subscriber` in the binaries (D7).

## Follow-ups

- **S3, duplicate filter:** a restarted agent's MIDs start again at 1 (`em_mgr.cpp:525-531`). When the 0010 D3
  filter becomes real, its entries must expire, or a restarted agent's Searches are dropped.
- **S5, byte counters:** with the KiBMiB bit set, profile-2 and later agents may report KiB or MiB counters
  (the Byte Counter Units field, §17.2.61 and §17.2.101). The controller must scale by those units.
- **S2:** the Security Capability from the data model; the AL node from the database; M1 at the S1 stopping point.

## Alternatives considered

| Alternative | Why not chosen |
|---|---|
| Keep the probe and add only `em-core` tests | The slice must end with role processes that run (0010 D1) |
| Direct node creation without `em-cmd`/`em-orch` | The Search depends on the `dev_init` command; S2 would rework the gate, the timeout owner and the context types |
| Upstream's 10 s search | Can miss the controller for good behind `ieee1905-rs`'s 30 s Discovery |
| A temporary stopping state | Adds and later removes a state upstream does not have |
| Keep upstream's fixed Profile 3 and unset KiBMiB bit | The spec is explicit on both (0010 D7 rule 1) |
| Persist the controller's model now | `em-db` belongs to S2 (0010 D1) |

## Acceptance criteria

1. The new crates build with the decided dependencies only; the workspace checks pass on stable and 1.85.0.
2. Tests cover: the Search and Response bytes, every row of the spec table, the `dev_init` chain and its timer
   with the "until found" difference, routing by band and state, the resend with a new MID, and the stop after
   the Response.
3. The D8 exit checks pass on x86_64 in Docker.
4. `STATUS.md`, the root README and the crate READMEs are updated; 0002, 0004, 0005 and 0008 get amendment
   pointers.

## Review round 1 changes (rev 7, 2026-11-21)

- **Malformed Profile TLV in a Response (medium).** A Multi-AP Profile TLV whose body is neither 1 nor 4 octets is
  not "absent": upstream's `get_profile_type` fails, it validates at profile 1, where the present Profile TLV is
  not allowed, and rejects the Response (`em_msg.cpp:131-152`, `em_configuration.cpp:6304-6317`). The port rejects
  it too and the radio keeps searching. An absent TLV is still rejected by D6.
- **Erratum to D3.** Upstream routing stores 1 for a Search profile octet of 4 or more (`parse_profile_tlv` fails at
  `>= em_profile_type_max`, `em_msg.cpp:145-148`), and stores 0 as is. D3 said "a reserved value is stored
  unchanged", which holds only for 0. The port now stores 1 for 4-255.
- **One link snapshot (D9, medium).** The role actor copies the `LinkState` once, gives that value to the core
  (`registered`/`unregistered`) and stamps that call's frames with the same value's session.
- **Queue order (D10, user decision).** `em-orch` visits pending and active stages newest first, as upstream
  (`em_orch.cpp:388`, `:404`), and activates the newest eligible pending stage.
- **Peer-model cap (D3, user decision).** `CtrlCore` keeps at most 256 peer models; when full, a new AL MAC evicts
  the model created longest ago. Upstream keeps them without limit; S2 replaces the map with the database.
- **Harness (D8, medium and user decision).** The agent logs `dev_init submitted` at info on every submission; check
  6a requires exactly one before the kill and none after a baseline taken before the kill. The entrypoint marks
  the supervisor ready after it initialises its files, and `run-s1.sh` waits for that before the first start.

## Revision history

- rev 1 (2026-11-21): first draft. User accepted its three open choices.
- rev 2 (2026-11-21, plan review: 12 findings, the main ones checked against the pin by the coordinator):
  - upstream's search stops after 10 s with `dev_init`; the port searches until found (user decision);
  - the validation tables reject spec-correct Responses below profile 3; the rows follow the spec (user decision);
  - `em-cmd` and `em-orch` minimal now, and S1 stops in `wsc_m2_pending` (user decisions, reversing two rev 1
    answers);
  - the controller's AL node at registration; SearchedRole/SearchedService and the reserved-profile rule; the
    Security Capability values; `--profile` limited to 2 and 3; band numbering corrected; check 6 split;
  - `tracing-subscriber` (user decision) and `rt-multi-thread`.
- rev 3 (2026-11-21, second plan review: 11 findings, checked by the coordinator against the pin and the spec):
  - the "until found" timer resets the start time each tick, as upstream's DPP branch, and the cancel after the
    last Response resets one node, as upstream (D2);
  - user decisions: a Search without a Profile TLV is accepted as Profile-1; every valid Search is answered and
    SupportedService follows §6.1; minimal dependencies (D3, D6, D11);
  - link state and reconnects after 0011 A5-A8 (D9); the crate interfaces fixed before work starts (D10);
  - exit checks made deterministic, with a supervisor for the role processes (D8);
  - any-MID acceptance, the agent's reserved-profile rule, the controller's own-validation failure, and the
    em-msg tests D6 turns into intentional differences.
- rev 7 (2026-11-21, S1 review round 1; user decisions on the low items): see "Review round 1 changes"
  below.
- rev 6 (2026-11-21, fifth plan check: 5 findings): node profile from `--profile` (D8 check 4 depends on it);
  role effects after the pass recorded as an S1-only departure from 0002 D1; untracked nodes are `Idle`;
  `Node<S>`, `NoCommands`, `admit`'s result, the rules struct; `is_emplus_agent` in routing.
- rev 5 (2026-11-21, fourth plan review: 9 findings, checked against `em_orch.cpp` and `em.cpp` at the pin):
  em-orch owns each node's orchestration state and applies its own changes inside the pass (the single-node
  cancel depends on it); two-phase submit; effect mapping and `on_start`; `on_event` limited to activation and
  readiness; the elapsed-time difference; validation after routing; em-core → em-easyconnect; amendment pointers.
- rev 4 (2026-11-21, third plan review: 10 findings, checked by the coordinator):
  - SupportedService always included (user decision); a Response without a Profile TLV row;
  - the interfaces in D10 written out (OrchRole hooks, the node-creation effect, the state enums, the message-id
    counter, the elapsed count);
  - the transport ending, frames built while unregistered, the peer-model commit in routing as upstream, the
    timer threshold, check 6(a) evidence, and the amendment pointers.
