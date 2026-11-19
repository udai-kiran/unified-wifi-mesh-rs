# 0002: The `em-cmd` / `em-core` / `em-orch` boundary

| | |
|---|---|
| Status | **Accepted** (rev 6). The design choices in D10 were decided by the user, 2026-11-19. Revs 1–3 went through a plan review and two review rounds. Rev 4 was a revised design (user decisions D10.5–6) and had two review rounds of its own. The rev-6 fixes after the final round were checked by the coordinator against the pinned source but **not re-reviewed** (two-round limit; user decision). |
| Date | 2026-11-19 |
| Upstream | `rdkcentral/unified-wifi-mesh` @ `ba4180ba36748629f10c20babc4320dd69c9db4f` |
| Affects | `em-cmd`, `em-core`, `em-orch`, `em-ctrl`, `em-agent`, `em-transport`, `em-db`, `em-easyconnect`. Notes: 0001 D7, 0003 (deadline ownership), 0004 R2/R3/R4/R6 (R6 amended, D9), 0007 D2 (amended) and D5. Docs: `docs/em_base-coverage.md` (`pending (0002)` rows), `docs/bus-event-inventory.md`. `STATUS.md` rows 9–13 and the `em-core` README. |

## Context

Note 0001 D7 deferred three questions to this note:
- how upstream `em_cmd_t` and its command classes divide;
- where the engine→host (category C) payloads live;
- the crate graph and porting order of `em-cmd`, `em-core` and `em-orch`.

Note 0004 (R2, R3, R6) and note 0007 D5 add binding rules and required handoffs.

The evidence comes from five read-only investigations of the pinned source: commands,
orchestrator, engine, managers, and a trace of stage order. Their reports are kept outside the
repository. Rev 1 was plan-reviewed, and the review checked 16 citations at the pin. Statements
marked *inference* were read from code, not observed at run time. No decision depends on an
inference alone.

### Upstream structure (facts)

**The command object is three things at once.** `em_cmd_t` (`inc/em_cmd.h:34-52`) is:
- a **request**: `m_type`, plus a parameter union `m_param` (`em_base.h:3513-3526`). The union
  aliases wire bodies (`em_scan_params_t`, `em_beacon_metrics_query_t`) and holds a raw
  `em_network_node_t *`.
- an **orchestration plan**: up to 16 `em_orch_desc_t {op, submit}` (`em_base.h:3311-3314`), the
  current op index, a queue of candidate `em_t *`, and a start time.
- an **engine context**: a full `dm_easy_mesh_t` copied by value (`em_cmd.h:42`), and
  `m_cmd_ctx`.

There are 42 subclasses plus the base: 43.

**Stages are separate commands.**
- An `analyze_*` function builds the first command, then one `clone_for_next` per remaining plan
  op (`em_cmd.cpp:226-251`; 30 call sites), and passes the whole chain to `submit_commands`
  (`em_orch.cpp:42-70`).
- `pre_process_orch_op` runs **for every stage at submit time**. Stages with `submit = false`
  perform their pre-processing (database and model writes, node creation) and are destroyed there.
  The rest are queued as separate pending commands, in plan order.
- `clone_for_next` builds each stage as a plain base `em_cmd_t`, so the derived type's overrides
  are lost. It deep-copies the data model twice per stage.
- The copy (`operator=`, `dm_easy_mesh.cpp:62-165`) leaves out the scan, associated and
  disassociated station maps. Some paths add them back explicitly (`clone_hash_maps`).
- Five controller commands get an identity-only model from a converting constructor
  (`dm_easy_mesh.cpp:4318-4328`; `dm_easy_mesh_ctrl.cpp:3302, 3413, 3891, 4943, 4958`).
- The model copy carries per-command data that the engines read:
  - the changed policy set (`em_policy_cfg.cpp:54-58, 490-495`);
  - an echoed message id (`dm_easy_mesh_agent.cpp:442` → `em_channel.cpp:1512`);
  - tracked query message ids (`em_metrics.cpp:1213, 1323, 1361, 2441-2481`).

**Activation is oldest-first, one command per node.**
- `queue_push` adds at the head (OneWifi `source/utils/collection.c:39-56`; the queue library is
  not in the pin, so it was read from the OneWifi tree).
- The pending scan in `handle_timeout` (`em_orch.cpp:511-528`) walks from the tail, so the oldest
  entry comes first. It activates **the oldest command whose candidates are all idle**, skipping
  blocked ones, and at most one command per tick.
- **Refusal is per entry point**, not an orchestrator rule. Some bus handlers ask
  `is_cmd_type_in_progress` (`em_orch.cpp:483-509`) and reply `prev_cmd_in_progress`.
  Others submit without asking: `handle_config_renew`, `handle_m2_tx`, `handle_sta_assoc_event`
  (`em_ctrl.cpp:267-293`), and `handle_client_metrics_req` and `handle_bsta_cap_req`
  (`:574-590`). For example, a tri-band agent's three M1s yield three concurrent `em_config`
  chains, one per radio.
- **On the agent**, most handlers check while the type is in progress (`em_agent.cpp:83, 97, 114,
  131, 148, 211, 229, 259, 276, 295, 314, 376, 751, 1203, 1327, 1390`).
  - Ten of them reply on the agent's command socket (`em_cmd_agent.cpp:43-116`) with
    `send_result`: `:98, 115, 132, 149, 212, 230, 260, 277, 296, 315`.
    - `ap_cap_query`, `radio_config` and `vap_config` reach their handlers only through that
      socket.
    - `dev_init` also comes from OneWifi (`:1869`).
    - `channel_pref_query`, `channel_sel_req` and `m2ctrl_configuration` come from engines
      (`em_channel.cpp:2013, 2048`; `em_configuration.cpp:4848, 5710`).
    - The `onewifi_*` events come from callbacks (`:2286-2294`).

    When an event that did not come from the socket is refused, upstream writes the status to a
    stale socket.
  - The other six handlers only log (`:83, 376, 751, 1203, 1327, 1390`).
- **Successful reads return data.** On success the reply carries a JSON `Result`
  (`em_cmd.cpp` `status_to_string`):
  - for the eleven model reads (ten `get_*` plus `scan_result`), the model document
    (`em_ctrl.cpp:499-512, 862-873`);
  - for `get_dev_test`, a test document (`:461-484`);
  - for `set_*` requests, an echo of the request.

  Until both the model and the network topology are initialised, the controller's listener
  replies `Error_Other` to everything
  except reset and get_reset (`em_cmd_ctrl.cpp:104-124`).
- **The reply depends on the submit count.** `success` if at least one stage was queued, else
  `not_ready` (`em_ctrl.cpp:125-135, 444-458`). `remove_device` and `reset` have only
  `submit = false` stages (`em_cmd_remove_device.cpp:45-51`), so all their effects run at submit
  and the reply is `not_ready`.
- **Database I/O is synchronous on the manager thread:**
  - in pre-processing at submit (`em_orch_ctrl.cpp:463-528`: `set_config`, `reset_config`,
    `update_tables`, `load_net_ssid_table`);
  - in the 250 ms dirty flush (`em_ctrl.cpp:798-802` → `dm_easy_mesh_ctrl.cpp:5799-5810`);
  - at commit (`em_ctrl.cpp:106`);
  - at policy finish (`em_orch_ctrl.cpp:286`);
  - at analyze time, where op-class rows are deleted before the command is built
    (`dm_easy_mesh_ctrl.cpp:4501, 4647`, from `em_ctrl.cpp:208, 224`);
  - at startup (`init_tables`, `load_tables`, `dm_easy_mesh_ctrl.cpp:10759-10781`).

  The list is not exhaustive. The rule is "every synchronous database call upstream makes on the
  manager thread".
- **Failures are not propagated**, and the effect differs by path:
  - The network SSID list changes in memory only if its write succeeds
    (`dm_network_ssid_list.cpp:120-123`).
  - A BSS delete (`dm_easy_mesh_ctrl.cpp:6004-6017`) works like this:
    - It is removed from the flat BSS list whatever the write's result.
    - If any write fails, the node model's `bss_list_delete` flag and its match criterion stay set.
      The next flush then retries the delete against the node model's matching BSS entries.
  - `update_tables`' return is ignored.
  - Every row commits separately: there is no transaction (`begin_transaction` has no caller), and
    the SQLite busy timeout is 5 s (`db_client_local.cpp:274`).
- The stages of one chain share their candidate, so they run strictly one after another, in plan
  order. A traced `em_config` run sends Topology Query, AP Capability Query, Channel Preference
  Query, Policy Config Request, Channel Selection Request, then Channel Scan Request. A traced
  `sta_assoc` run sends Topology Query, Client Capability Query, then Beacon Metrics Query.

**Progress is polled, on starving ticks.**
- `orch_execute` stores the command pointer in `em_t::m_cmd` (`em.cpp:86-92`) and changes the
  node's state only when that op's precondition holds (`:157-193`).
- There is no completion message. Readiness predicates are polled on a 250 ms tick, and some of
  them write node state and counters (`em_orch_ctrl.cpp:166-325`, `:196-203`). The tick runs only
  after 250 ms without events, because `em_mgr.cpp:546-553` restarts the wait on every event, so
  steady traffic starves it.
- Nodes resend their current query every proto tick (1 s, likewise starvable) until a reply
  arrives. A state advances on a received reply.

**Timeouts.**
- Only active commands are timed (`em_base.h:112-114`). The timer is a per-type maximum over all
  commands of that type (`em_orch.cpp:72-91`), reset by `reset_cmd_time` after a cancel
  (`:219, 268`).
- **Controller `em_config`, topology-sync stage**: when the stage's op is `topo_sync` **and** the
  node is in `topo_sync_pending` (`em_orch_ctrl.cpp:66`; covers `:60-145`):
  - No SSID mismatch: after 40 s, `cancel_command(type, all_em_radios)` cancels only **that AL's
    radios** (`em_orch.cpp:175-220`; it runs `pre_process_cancel` on active candidates and resets
    the type timer), then marks them misconfigured and sends them Autoconfig Renew.
  - Before the no-mismatch scoped cancel, both per-AL timestamps are cleared
    (`em_orch_ctrl.cpp:75-81`).
  - SSID mismatch: the window is 120 s from first detection.
    - **On expiry:** each radio's mismatch flag is cleared, both per-AL timestamps are cleared, and
      the scoped cancel and renew follow (`:106-128`).
    - **While the mismatch persists:** a Topology Query is sent at once if none has been sent
      (last-query time unset), then every 10 s. Each time, every sibling radio is moved to
      `topo_sync_pending`, one query is sent from the first sibling (`all_em_radios.front()`), and
      the send time is recorded (`:132-143`).
  - The mismatch timers (`ssid_mismatch_check_time`, `last_topo_query_sent_time`) live in the
    per-AL model and are written by this hook.
- **Controller `em_config`, any other case** (`:146-151`): after 40 s, the mismatch timers are
  cleared and the unscoped `cancel_command(type)` (`em_orch.cpp:271-304`) cancels **every**
  `em_config`, with no direct renew and no timer reset. `pre_process_cancel` (`em_orch_ctrl.cpp:395-447`) runs for
  each active candidate: it marks the node misconfigured and sends a `cfg_renew` through the
  controller's own command socket (`:404-416`).
- **Controller, other types:** cancelled after 10 s.
- **Agent** (`em_orch_agent.cpp:42-74`): `dev_init`'s timer is reset while the AL node is
  DPP-onboarding (`:52-61`). Otherwise commands are cancelled after 10 s, and the node is set to
  unconfigured or `topo_synchronized` (`:64-71`).
- Pending commands never time out.

**Engine** (`em_t`, `src/em/`):
- **Threads and sub-engines.** There is one thread per node (radio, or the AL node). The
  sub-engines are base classes of `em_t` (`em.h:51-56`), and their state lives in `em_t`.
- **Data model.** Engines use a borrowed `dm_easy_mesh_t *`: about 223 `get_data_model()` calls,
  plus wrapper accessors. On the agent every node shares one model (`em_agent.h:862`).
- **Command reads.** Engines read the command at about 64 sites. Some of those reads and writes
  reach another node's command and state (`em_metrics.cpp:641, 2441-2481`;
  `em_capability.cpp:1005-1012`).
- **Calls into the manager.** `get_next_msg_id` (39 calls); `get_all_em_for_al_mac` (27, without
  a lock).
- **Output.**
  - Frames are sent from the node thread (61 sites).
  - Category C actions go to the manager through `io_process` into a 400 KB raw buffer. Five of
    them carry wire-shaped data.
  - `sta_cap_update` is the **controller's** handling of a received Client Capability Report.
    `handle_client_cap_report` (`em_capability.cpp:926-1014`) borrows the frame body from the
    received frame, and `queue_cap_update` (`:907-923`) ships the device, station, BSSID, radio
    and frame body. `handle_sta_cap_update` (`em_ctrl.cpp:297-340`) stores the body in the
    station records. The agent's report generation from its stored frame (0007 D5,
    `em_capability.cpp:542-581`) is a different path.
- **Easy Connect.**
  - It sends frames from its own threads and timers, and makes synchronous callbacks into `em_t`
    and `em_mgr_t` (`ec_enrollee.cpp:114, 505, 2122-2130`; `em.cpp:3074-3110`).
  - Some callbacks are reads (`can_onboard_additional_aps`, `get_backhaul_sta_info`). Others
    act: `bsta_connect_bss` changes live BSS credentials (`em.cpp:940-962`), and
    `trigger_sta_scan` starts a scan and returns whether it started (`:965-981`).
  - It waits inside handlers:
    - a 500 ms sleep after the Peer Discovery Response (`ec_1905_encrypt_layer.cpp:184`);
    - up to 3 s for station-scan results (`ec_enrollee.cpp:180-190`; the results arrive on
      another input, `:2086`);
    - announcement loops with 2 s, 5 s and 30 s sleeps (`:207-330`);
    - a thread join (`:342`).

**Manager** (`em_mgr_t`, `em_ctrl_t` / `em_agent_t`):
- **Threads and locks.** There are five threads: the manager loop, input/CLI, the node listener,
  the node threads, and platform callbacks. Locks cover only the queues and the node map.
- **Node lifecycle.**
  - Nodes are created from received WSC and Early AP Capability frames on the listener thread
    (`em_ctrl.cpp:1266-1268, 1344`), from a model operation (`dm_easy_mesh_list.cpp:470-471`), and
    in pre-processing (`em_orch_ctrl.cpp:490`; `em_orch_agent.cpp:259, 279`).
  - `delete_node` (`em_mgr.cpp:176-195`) frees a node without telling the orchestrator, leaving
    dangling candidate pointers (*inference*: use-after-free).
- **Command sources.**
  - Category-A requests arrive over TLS or a UNIX socket. The reply means "accepted"
    (`em_ctrl.cpp:129-130`) and goes through one shared reply slot (`em_cmd_exec.h:34-35`).
  - `cfg_renew` from the CLI never replies (`em_ctrl.cpp:267-275`).
  - Many commands are internal, with no reply: `m2_tx` → `em_config`, `sta_assoc`, and most agent
    commands, which come from platform or engine events (`em_agent.cpp:1410-1460`).
- **Paths that bypass the orchestrator.**
  - The agent pushes an AP-metrics command straight to a node (`em_agent.cpp:1347-1386`), and the
    node swaps `m_cmd` while handling it (`em.cpp:415-427`).
  - The agent's `handle_set_policy` writes the model and OneWifi directly, and uses the
    orchestrator only for the in-progress check (`em_agent.cpp:1194-1208`).

### Binding rules this note must meet

- **0001 D7:**
  1. No ported unions or event envelope.
  2. An acyclic crate graph.
  3. OneWifi payloads only in `em-agent`.
  4. Category-A requests carry no engine pointers and no wire-body aliases.
- **0004 R2.** No `clone` of anything, including `Sender`, so there is one producer per channel,
  wired at startup.
- **0004 R3.** Actors own their state.
- **0004 R4.** Timers live in the owning actor.
- **0004 R6.** A single frame sender (amended in D9).
- **0007 D5.** Every handoff listed there is answered (D6).

## Decision

### D1. One role actor per process; engines are synchronous (user decision)

Each process runs one role actor, in `em-ctrl` or `em-agent`.

**What it owns:**
- the data model (`em-dm`'s Step 3 container);
- the node table (`em_core::NodeTable`): one `Node` per radio and per AL device, keyed by
  `NodeKey { mac, kind: Radio | Al }`. An AL node and a radio can share a MAC; upstream
  disambiguates by appending `_al` to the key (`em_mgr.cpp:211-213`).
- the orchestrator state (`em_orch::Orchestrator`);
- the Easy Connect state machines;
- a `DelayQueue` with every node, command and Easy Connect deadline (0004 R4; this actor owns
  each deadline, as 0003 requires);
- the message-id counter;
- the single frame sender to `em-transport`.

**Its inputs**, merged with `select!`:
- received frames;
- category-A requests (one `mpsc` per IPC bridge);
- platform events (agent: OneWifi, normalised in `em-agent`);
- the `DelayQueue`;
- idle timers with upstream's timing, kept as `DelayQueue` deadlines that **restart on input**,
  never as fixed-phase intervals:
  - **Proto timer, per node.** It restarts on every event dispatched to that node, and fires only
    after 1 s with none (`em.cpp:644-680`). Resends, and the renew and capability-query counts,
    therefore happen exactly when upstream's would; `em_configuration.cpp:938` counts a renew per
    send.
  - **Orchestrator timer.** It restarts on every role-actor input, and fires after 250 ms with none
    (`em_mgr.cpp:546-582`). It runs the time-based work: timeouts, `orch_transient` and the dirty
    flush.

  Under steady traffic both are starved, exactly as upstream is. A stall never causes a burst.
  Only activation and readiness also run on events (D4 change 1).

**Engine handlers are synchronous functions over disjoint borrows of the actor's fields:**

```rust
struct EngineCtx<'a> {
    nodes: &'a mut NodeTable,            // the handler's node and its AL siblings
    key: NodeKey,                        // the node being handled
    model: &'a mut Model,
    commands: &'a CommandView<'a>,       // read-only view of active requests (D3)
    msg_ids: &'a mut MsgIdCounter,
    easy_connect: &'a mut EasyConnectState,
    out: &'a mut Outputs,                // frames, actions, progress, node create/remove, deadlines
}
fn handle_frame(ctx: &mut EngineCtx<'_>, frame: Frame<'_>);
```

- Cross-node reads and writes (`get_all_em_for_al_mac`, per-AL state writes) operate on `nodes`
  in place, in upstream's iteration order.
- Nothing is shared and nothing is locked, because one task owns everything.
- **Easy Connect and WSC/DPP cryptography** run inline as synchronous state machines, with no
  sleeps or waits in a handler:
  - **Read callbacks** are values that `em-core` passes in. `em-easyconnect` defines a small trait
    for them, which `em-core` implements, so the dependency stays one-way.
  - **Acting callbacks** become `Outputs` requests: send an action frame, connect the backhaul
    STA, trigger a station scan. A result such as "scan started" or the scan results comes back
    as a later input event.
  - **Each upstream wait becomes an explicit state with a `DelayQueue` deadline:**
    - awaiting scan results (3 s, then continue without them, as upstream's timeout path does);
    - the post-Discovery-Response delay (500 ms) before EAPOL 1. It starts at the response's
      **send completion**: the frame asks `em-transport` for a completion event (defined in the
      transport note). If the send fails, EAPOL 1 is skipped, as upstream does on a failed send
      (`ec_1905_encrypt_layer.cpp:178-189`);
    - the announcement cycles (2 s, 5 s, 30 s);
    - reconfiguration authentication, instead of the thread join.
- **Database I/O runs synchronously in the role actor** (user decision, D10). It happens at every
  point where upstream's manager thread calls the database (see the Context), and the actor handles
  no other input until the call returns.
  - The connection is owned by the role actor, and each call runs through
    `tokio::task::block_in_place`. That is callable from the synchronous call sites (hooks,
    pre-processing, analyze) and hands the worker's other tasks to another thread. The runtime is
    multi-threaded with at least 2 workers.
  - Failures are handled as upstream handles them: memory is unchanged where upstream leaves it
    unchanged, the chain continues, and the failure is logged.
  - **This stalls more than upstream does, and is an intentional difference.** Upstream's node
    threads keep handling frames during a database call; in the port, frames, Easy Connect
    deadlines, resends and IPC replies all wait.
  - **Mitigations:**
    - rows commit one at a time, as upstream does (no transaction), so each row's success or
      failure and its dirty flag behave as upstream's;
    - the busy timeout stays at upstream's 5 s;
    - the dirty flush runs on the orchestrator idle timer, as upstream's does;
    - `block_in_place` on a multi-threaded runtime stalls only the role actor, not the transport or
      bridge tasks;
    - `em-transport`'s note sets its channel capacities and its policy when the actor's frame
      channel is full.
  - The `em-ctrl` note measures the stall on target hardware.
- **The actor never awaits a send inside a handler.** Handlers only append to `Outputs`. The
  `select!` loop forwards them with `reserve()` branches, and a peer that sends back to the actor
  never blocks on the actor (no send-send cycle).
- **When effects apply.** Hook effects (D2) are applied after each candidate's hook, in upstream's
  iteration order. Upstream's predicates change node state during the pass
  (`em_orch_ctrl.cpp:197-203`), and later candidates see those changes.
- **Trade-off.** All nodes are serialised; upstream runs a thread per node, but those threads share
  the model without locks. A later note may move a sub-engine out if profiling shows a hot spot.

### D2. Crates and graph

| Crate | Contents | Depends on |
|---|---|---|
| `em-cmd` | **Orchestrated command types from every source**: one native struct per command (IPC requests, internal submissions, platform- and engine-originated commands); no pointers and no wire aliases (0001 D7 rule 4). `Command` enum, `Origin` (`Ipc` or `Internal`, data only), status codes (upstream `em_cmd_out_status_t`), `OrchOp` (upstream `dm_orch_type_t`), each command's plan as a fixed set of named `PlanVariant`s (the analyze-time overrides, e.g. `dm_easy_mesh_ctrl.cpp:3582-3637`, choose a variant), and each command's candidate rule. No tokio types. | `em-types`, `em-dm` (value records as parameters) |
| `em-orch` | A synchronous library with no tokio types: chain records, pending and active stage queues, candidate sets of node keys, activation, deadlines (the caller passes `Instant`), the per-type timer, cancel (scoped to candidates, or by type), node removal, and an `is_type_in_progress` query for the entry points that refuse. Role rules come through `trait OrchRole { type NodeView; … }`. Every hook takes a read-only `&Self::NodeView` and returns a typed list of `OrchEffect`s (set a node's state, reset counters, send a renew, submit an internal command, write to the model or database), which the role actor applies. | `em-types`, `em-cmd` |
| `em-core` | `Node` and `NodeTable` (state per role, protocol counters, active `OpContext`), the sub-engines as modules of handler functions, `EngineCtx`, `Outputs`, `CoreAction` (D5), and the `NodeView` implementation the role crates pass to `em-orch`. | `em-types`, `em-msg`, `em-dm`, `em-cmd`, `em-crypto`, `em-easyconnect` |
| `em-ctrl` / `em-agent` | The role actor, `impl OrchRole for` a small role-rules struct (not for the actor itself, which avoids borrowing it twice), the IPC bridges, the database connection (ctrl), OneWifi (agent). | `em-core`, `em-orch`, `em-cmd`, `em-easyconnect`, `em-transport`, `em-db` (ctrl) |

The graph is acyclic: em-cmd → em-dm → em-types; em-orch → em-cmd; em-core → em-cmd, em-msg,
em-dm, em-crypto, em-easyconnect; the role crates depend on all of them.

- `em-orch` does not depend on `em-core`. The node view is an associated type supplied by the role
  crate.
- Upstream's `em_cmd.h → em_ctrl.h` include, which pulls the whole controller into every command,
  is not reproduced.

### D3. Upstream `em_cmd_t` is split three ways

| Upstream | Rust | Owner |
|---|---|---|
| `m_type`, `m_param` (minus `net_node` and the aliases), vendor `m_raw_data`, and per-command data the model copy carried (changed policies, an echoed message id, decoded targets) | `em_cmd::Command` variant, immutable once submitted. Owned **once per chain**: upstream's per-stage copies become one `ChainRecord` that owns the `Command`, and its stages reference it by `ChainId`. | `Orchestrator`, in a `ChainRecord` |
| the reply | a `oneshot::Sender<Reply>` on the **request envelope** (0004 R2), where `Reply { status, result: Option<ResultDoc> }` carries an owned result document, moved and never cloned. One request can produce several chains (`set_policy`, `remove_device`: one per device) and gets one reply, sent at submit. | the IPC bridge moves it into the role actor, which consumes it when replying |
| `m_orch_desc[]`, op index, `m_em_candidates`, `m_start_time` | per-stage entries `(ChainId, stage)` in the pending and active queues: candidate node keys, timing, per-node status | `Orchestrator` |
| `m_cmd_ctx`, `processed`, tracked query message id and transmit time, per-op retry counters | `em_core::OpContext { chain: ChainId, stage, … }`. It stores the chain id, not a borrow, and reads the `Command` through `EngineCtx::commands` | the `Node`, while its op is active |
| `m_data_model` snapshot | not ported; D6 says where each read goes | — |
| `m_evt` (400 KB), `m_name`/`m_svc` (derivable), unused `m_rd_*` and `m_db_cfg_type` | dropped | — |

- **Activation.** Activating a stage on a node is an effect the role actor applies
  (`Node::start(OpContext)`). This replaces `orch_execute` storing a pointer.
- **Progress.** Nodes report progress through `Outputs`. Hooks are evaluated on those events and
  on the tick.
- **Cross-node command reads.** Upstream reads another node's command (`em_metrics.cpp:641,
  2441-2481`). In the port the reading node finds the target in `EngineCtx::nodes`, reads the
  target's `OpContext` (tracked message id, transmit time), and changes the target's state, in
  place.
- **Direct-to-node AP metrics** (agent, `em_agent.cpp:1347-1386`). This becomes a node-local
  request, handled without the orchestrator. It runs beside the node's active `OpContext` and
  never replaces it.
- **The agent's `set_policy`** (`em_agent.cpp:1194-1208`) keeps its direct path: the role actor
  applies the model delta and maps it to OneWifi. The orchestrator is used only for the
  in-progress check, as upstream does.

Appendix A lists all 43 classes with their command variant, plan variants, role and handoff
kinds.

### D4. Orchestration semantics

**Kept**, because peers can observe them:
- **Plans and stages.** Each command's plans (Appendix A), including the analyze-time variants.
- **Pre-processing at submit.** Every stage is pre-processed at submit time, so `submit = false`
  stages perform their writes and node creation then, as upstream does.
  - **Database-backed stages** (`db_cfg`, `db_delete`, `db_reset`, `net_ssid_update`; in
    `set_ssid`, `set_policy`, `dev_test`, `reset`, `remove_device`) run synchronously within the
    submission (D1). Upstream changes the in-memory list only if the write succeeds
    (`dm_network_ssid_list.cpp:120-123`), reloads with `net_ssid_update` after the write, and
    continues the chain whatever the result. The port does the same.
- **Activation.**
  - The oldest pending stage whose candidates are all idle is activated first, skipping blocked
    ones.
  - One command per node at a time.
  - Stages of a chain run in plan order.
  - Chains on different radios run concurrently, for example a tri-band agent's three
    `em_config` chains.
- **Refusal and replies.** Each entry point keeps upstream's check and its reply, whatever the
  command's origin.
  - **Refuses with a reply:**
    - controller IPC handlers that check;
    - agent handlers that check and that the request reached through the agent's command socket
      (the ten in the Context).

    The reply is "previous command in progress", or the handler's other values, for example
    `no_change`.
  - **Refuses silently:** the same agent handlers when the event came from a callback or an
    engine (no envelope, no reply; upstream's write to a stale socket is not reproduced), and the
    six agent handlers that only log.
  - `cfg_renew` is checked per radio.
  - **Never refuses:** the controller's `handle_config_renew`, `handle_m2_tx`,
    `handle_sta_assoc_event`, `handle_client_metrics_req`, `handle_bsta_cap_req`.
  - **Reply value:** `success` if a stage was queued, else `not_ready`. That includes
    `remove_device` and `reset`, whose effects run at submit.
  - Each crate note checks its entry points against `em_ctrl.cpp` / `em_agent.cpp`.
- **Timeouts and escalation.** Every path is reproduced as described in the Context, including:
  - the per-type maximum timer, reset by the AL-scoped cancel but not by the global one;
  - the AL-scoped cancel plus direct renew when the op is `topo_sync` and the node is
    `topo_sync_pending` (40 s; with an SSID mismatch, 120 s from detection and a Topology Query
    resent every 10 s);
  - otherwise for `em_config`, the global cancel (mismatch timers cleared first);
  - in both cancels, `pre_process_cancel` on each active candidate (misconfigured, plus the
    self-submitted `cfg_renew`, an internal submission through an `OrchEffect`);
  - the 10 s cancel for other types;
  - the agent's `dev_init` extension while DPP onboarding, and its state reset on timeout.

  The SSID-mismatch timers remain per-AL state, written through `OrchEffect`s.
- **Retry thresholds.** Renew (5) and capability query (2).
- **Resends.** Queries are resent each proto tick (1 s) until answered.
- **Stage behaviour.** Each stage behaves as upstream's stage does, including that `clone_for_next`
  stages run with base-type behaviour. For example, `sta_assoc`'s `topo_publish` stage does not
  use the beacon-query retry override. Appendix A records this per stage.

**Changes peers can see** (user decisions, D10):
1. **Event-driven activation.** Activation and readiness also run on events (node idle, command
   submitted, progress), not only on a 250 ms tick that steady traffic starves. The order is
   unchanged, and stages start sooner. Upstream spends about 750 ms of ticks per stage. The tick
   remains for deadlines.
**Not changed:** pending stages never time out, as upstream (user decision, D10). Rev 1–3 proposed a deadline. Two
review rounds showed it would drop stages whose submit-time writes had already run, for example a
stored SSID never sent to the agents, so the user dropped it.

**Internal fixes** (not visible on the wire):
1. Removing a node cancels or detaches its commands before the node is dropped. There are no
   dangling candidates.
2. Hooks return effects. Only the role actor applies state changes.
3. The node's own call into the orchestrator (`em_configuration.cpp:6269`) becomes an effect, with
   upstream's ordering (`:6263-6278`):
   - only after M2's send succeeds (on failure, the previous chain is kept and nothing else
     happens);
   - then the previous `em_config` is removed;
   - then `wsc_m2_sent` is set;
   - then `m2_tx` is submitted.

   The removal keeps its upstream effects (`em_orch.cpp:222-269`): the radio is removed from pending `em_config`
   chains, the active candidate is set unconfigured and its three counters cleared (no
   `pre_process_cancel`), and the type timer is reset.
4. The wrong `m_type` in `get_channel` and `get_network`, and the svc arguments that `cfg_renew`
   and `channel_pref_query` ignore, are corrected through the command table. *Inference*: these
   classes have no production creation sites.
5. There are no unlocked cross-thread writes, because there is one owner.

### D5. Category C actions (user decision)

- `CoreAction` is an enum in `em-core`, one variant per action. The four values with two roles
  (`set_policy`, `cfg_renew`, `beacon_query`, `client_assoc_ctrl_req`) are split into an `em-cmd`
  command and a `CoreAction`.
- Actions that carry a TLV's content (`bss_tm_req`, `channel_scan_params`,
  `client_assoc_ctrl_req`, `beacon_query`, `sta_assoc`) hold the decoded `em-msg` body type
  (note 0008). `StaAssoc` also carries the peer AL MAC from the frame's own TLV
  (`em_configuration.cpp:2322-2352`), which selects the node (`em_orch_ctrl.cpp:661-673`). The
  others hold native structs.
- `em-agent` maps the actions it consumes to OneWifi (0001 D7 rule 3). `em-ctrl` consumes the
  controller's actions, mostly by submitting internal commands.
- `sta_cap_update` (controller, inbound) is not an action in the port. The controller's
  Client Capability Report handler already holds `&mut Model` (D1). It applies a `ModelDelta`
  with the device, station, BSSID, radio and the received frame body, at the point where upstream's
  manager does. The agent's outbound report is D6's last row.
- The three upstream intercept points (`em_channel.cpp:2043`, `em_configuration.cpp:4846, 5708`)
  become one optional `ActionHook` on the role actor's action sink.

### D6. Data-model handoffs (answers to 0007 D5)

The role actor owns the model, and handlers borrow it. Each piece of data upstream took from the
command's model copy goes to one of:
- **R:** a fresh borrowed read of the model;
- **P:** a field of the `Command`, set when it is built from what upstream put into the copy;
- **O:** `OpContext` state on the node;
- **Δ:** an explicit owned `ModelDelta` the role actor applies at the same point upstream writes the
  live model.

**Every model read through `get_current_cmd()`:**

| Upstream read | What it carries | Port |
|---|---|---|
| `em_policy_cfg.cpp:57, 163, 279, 310, 405, 495`; `em.cpp:2115` | set_policy: only the changed policies (the comment at `:490-495`); em_config reads the live model | set_policy: P (the changed-policy set); em_config: R |
| `em_channel.cpp:1512` | the Channel Preference Query's MID, echoed in the report (`dm_easy_mesh_agent.cpp:442`) | P |
| `em_metrics.cpp:1213-1216` | link-metrics station map and echoed MID | P (decoded stations and MID) |
| `em_metrics.cpp:1289-1365, 2441-2481` | the beacon query's tracked MID and transmit time (set, matched against ACKs, cleared) | O on the querying node; cross-node access per D3 |
| `em_metrics.cpp:1423, 1913, 2314` | AP-metrics station and radio data | P (AP-metrics parameters); R for live station data |
| `em_configuration.cpp:824` | associated-station map for the topology notification (filled by `clone_hash_maps` on the STA-list path) | P (the decoded station list) |
| `em_configuration.cpp:6747` | renew: controller MAC from the copy | P |
| `em_configuration.cpp:3464-3518, 4280`; `em_discovery.cpp:107, 129`; `em_provisioning.cpp:331` | identity (AL MAC, manufacturer, serial, device type) | R (device identity from the model) |
| `em.cpp:122` | DPP data for onboarding inside `orch_execute` | P (decoded DPP parameters) |
| `em.cpp:1069` (`get_radio_from_dm(command_dm=true)`) | the radio from the command's model | P where the command carries a decoded radio (set_radio), else R |
| set_ssid / set_radio decoded models (`dm_easy_mesh_ctrl.cpp:4675-4800`) | target configuration | P; Δ at the stage where upstream writes the live model |

**The 0007 D5 handoffs:**

| 0007 D5 handoff | Port |
|---|---|
| Agent renew (`dm_easy_mesh_agent.cpp:233-245`) | P: controller MAC and radio; R: identity |
| Agent CSA (`:670-777`), where upstream writes the channel into a copy that is then discarded, so the live model is never updated | Δ: the channel is applied when the action is handled. This fixes the lost update, which is an intentional difference. |
| Agent STA link metrics (`:780-804`), where the command is built before decode (`:789` before `:790`) | decode first; Δ: the decoded station changes; P: stations and MID |
| Agent STA-list fan-out (`:111-227`, `:219`) | Δ first: the live associated-STA MLD entries are refreshed from the decoded snapshot before fan-out (`dm_easy_mesh_agent.cpp:139-146`), because the Client Association Event encoder reads them (`em_configuration.cpp:414-429`). P: the **associated** and **disassociated** station lists, kept separate, as `ctrl_notify` reads them (`dm_easy_mesh_agent.cpp:146-219`, `em_configuration.cpp:824-845`); Δ: `sta_aggregate` (submit = false) is applied at submit-time pre-processing from the **full** decoded association and disassociation maps (every station and link, `dm_easy_mesh_agent.cpp:218-219`, `dm_easy_mesh.cpp:3244-3282`), not the per-radio notification lists, as upstream merges at `em_orch_agent.cpp:286-292` |
| Agent scan / unassociated results (`:989, 1048`) | Δ applied on decode, as upstream's live write does; R in the stage |
| Agent `arr_index` and candidates (`em_orch_agent.cpp:287, 306, 320-375`) | Each command keeps upstream's candidate rule (Appendix A): `dev_init` takes every radio node; `sta_link_metrics` takes radio nodes with an associated station; `sta_list` takes the named radio if its lists are non-empty; `cfg_renew` takes radio slot 0 only (`:347-353`). The `arr_index` merge target is P per command. |
| Agent `al_insert` / `em_insert` (`em_orch_agent.cpp:258, 274-276`) | Δ in the command, applied at submit-time pre-processing (as upstream does), together with the node-creation effect |
| Controller identity-only constructions (`dm_easy_mesh_ctrl.cpp:3302, 3413, 3891, 4943, 4958`) | P: target identity plus the request parameters. *Inference*: the converting constructor leaves the counts uninitialised; the port does not reproduce it. |
| Controller removal (`:4881`) | P: the peer's key; the node is removed by key with D4's cancel and detach |
| Controller policy (`em_orch_ctrl.cpp:272-286, 476-479`) | P: the changed-policy set. Its `db_cfg` stage writes the database at submit, as upstream (`em_cmd_set_policy.cpp:47-52`). At finish the policy Δ is applied to the model and `update_tables` runs synchronously. The return is logged, which upstream does not do; there is no retry. |
| Controller DPP (`dm_dpp.cpp:42-53`) | P: decoded DPP parameters; Easy Connect state per D1 |
| Next-stage fan-out | the chain's stages are `(ChainId, stage)` entries that share the one `Command` in the `ChainRecord` (D3); reads are R; no model copies |
| Client Capability Report read (`em_capability.cpp:48-56, 542-581`) | The capability handler runs in the role actor with `&Model`. It borrows the station's stored frame and encodes the TLV, emitting at most the prefix that fits `EM_CLIENT_CAP_MAX_FRAME_BODY`, without modifying the stored frame. Only the serialized TLV leaves, to the frame sender. (The controller's inbound handling is D5's `sta_cap_update` delta.) Upstream copies every station (`dm_easy_mesh.cpp:4229-4239`), and the port does not. Tests per 0007 D5. |
| **Message ids** | The counter (`get_next_msg_id`) is owned by the role actor. Echoed MIDs are P. Tracked MIDs are O. |

### D7. Commands and replies (user decision)

- **Requests and replies.** An IPC request arrives on an envelope carrying a `oneshot` reply
  sender, moved in and never shared; this fixes the shared reply slot. The role actor replies
  synchronously at submit, with upstream's status for that entry point (D4): `success`,
  `not_ready`, `prev_cmd_in_progress`, `no_change`, or a parse status. One request can produce
  several chains and still gets one reply.
- **Results.** Wherever upstream fills a result, the reply carries it as a `ResultDoc`:
  - the `get_*` reads (answered from `&Model`, with no chain);
  - `get_dev_test`;
  - the echo of `set_*` requests.

  Upstream's other replies are kept too: `Error_Other` until both the model and the network topology are initialised (except
  reset and get_reset), and `set_dev_test`, which always returns `success`.
- **Internal submissions** have no reply. They are direct calls inside the role actor: `m2_tx` →
  `em_config`, `sta_assoc`, agent platform and engine events, and the orchestrator's own
  `cfg_renew`. The actor never sends to its own IPC channel.
- **Meaning of the reply.** It means **"accepted"**, which is upstream parity. Every IPC request
  replies, including `cfg_renew`, which upstream never answers on the CLI path. It replies
  `success` when its chain is queued; that is an intentional difference.
- **Bridges.** The IPC listener (TLS / UNIX socket) and the rbus/TR-181 bridge are tasks that decode
  into request envelopes and forward them on one `mpsc` each.

### D8. Node creation and removal

- **Creation** is an effect (`Outputs::create_node` or `OrchEffect`) that the role actor applies.
  It covers every source, with upstream's initial states:
  - received WSC and Early AP Capability frames (`em_ctrl.cpp:1266-1268, 1344`);
  - pre-processing (`em_orch_ctrl.cpp:490`; `em_orch_agent.cpp:259, 279`).
- Model operations that create nodes upstream (`dm_easy_mesh_list.cpp:470-471`) return
  "node needed" results instead of calling a manager.
- **Frame routing** (`find_em_for_msg_type`) is a function that borrows the frame, the node table
  and the model (`&Model`). It returns an ordered effect set plus a dispatch target:
  - For Early AP Capability (`em_ctrl.cpp:1327-1359`) it looks up the peer model, creates every
    missing radio node from the peer's profile, then dispatches the frame. The target is the last
    radio node created, or the first existing one if none was created (`:1336-1354`).
  - For Autoconfig Search (`:1220-1237`) it requests a model commit. As upstream, the commit is
    **deferred**: it runs after the frame has been dispatched, so the Autoconfig Response does not
    wait on its database write.

  The role actor applies the effects in order, then calls the target's handler.
- **Removal** follows D4's first internal fix.
- **Keys.** Every node is keyed by `NodeKey { mac, kind }` (D1).

### D9. Amendment to 0004 R6

- R6 said `em-core` owns the frame sender and receives the transport's frames. Under D1 the
  **role actor** owns both: `em-core` is a library of handlers, and the run loop (upstream
  `em_mgr_t`) lives in `em-ctrl` / `em-agent`.
- The single-sender rule is unchanged.
- `STATUS.md` rows 11, 12 and 13 and the `em-core` README are updated to match.

### D10. User decisions (2026-11-19)

The user accepted the recommendations:
1. **Actor layout:** one role actor with synchronous engines (D1).
2. **Category C payloads:** decoded `em-msg` bodies for actions that carry TLV content, native
   structs for the rest (D5).
3. **Reply meaning:** "accepted" (D7).
4. **Activation:** oldest-first activation, which the trace showed is upstream's order (D4).
5. **Pending deadline dropped** (rev 4). Pending stages never time out, as upstream.
6. **Database I/O synchronous in the role actor** (rev 4), at upstream's points (D1).

### D11. Porting order

Each crate gets its own scope note:
1. `em-cmd`: command types, plan variants, candidate rules, status.
2. `em-orch`: the synchronous library, tested without tokio.
3. `em-core`: node, `OpContext`, `EngineCtx`, `Outputs`, then one sub-engine per step
   (configuration first).
4. `em-agent` / `em-ctrl`.

`em-transport` (its own note, R6/D9) depends on neither `em-cmd` nor `em-core`. It follows
`STATUS.md`'s order, ahead of them.

`em-easyconnect` comes before `em-core`, because `em-core` depends on it (D2). Either it is
ported first, or `em-core`'s first steps cover only the sub-engines that do not need it, with
provisioning last.

`em-dm` Step 3 (the model container and its operations) is unblocked. D6 fixes what the model must
offer: borrowed reads, `ModelDelta` application, dirty marking for persistence, and "node needed"
results.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| One actor per node (upstream's thread per node) | A sender per node created at run time (R2 forbids cloning), fan-in at the transport, a registry, and messages for the 27 AL-wide call sites and the cross-node writes. Workable with `StreamMap`, but more moving parts for no isolation upstream actually has. |
| Port `em_cmd_t` as one type | One object cannot be request, plan and engine context under R2/R3. It would also repeat the per-stage model copies. |
| Keep the model snapshot in the command | Copies the model twice per stage. Upstream's snapshots are often near empty or miss maps, and the Client Capability Report copy contradicts 0007 D5. |
| Native payloads for every category C action | More types and conversions. `em-core` depends on `em-msg` anyway. |
| "Completed" replies | Changes the CLI contract; a reply can take up to 120 s. Could be added later as a separate status query. |
| Offload Easy Connect to its own tasks | Needs channels created at run time, the same problem as one actor per node, and it uses synchronous callbacks. |

## Consequences

- `em-cmd`, `em-orch` and `em-core` can be ported and tested as libraries. Only the role crates
  need tokio.
- 0004 R6 is amended (D9).
- The `pending (0002)` rows of `docs/em_base-coverage.md` are updated by the `em-cmd` scope note,
  under this rule:
  - command and request payload types go to `em-cmd`;
  - category-C payloads go to `em-core` (D5);
  - `em_cmd_ctx_t` goes to `em-core` (`OpContext`);
  - `em_cmd_stats_t` goes to `em-orch`;
  - `em_commit_info_t` goes to `em-ctrl`. Its producer and consumer are both in the controller
    role (`em_ctrl.cpp:1220-1229, 65-84`).
- `STATUS.md` rows 9–13 lose their *provisional* marks.
- Every intentional difference is listed in D1, D4, D6 and D7.

## Appendix A: command classes

Rows marked † have no production creation site at the pin (*inference*: CLI or tests only).

**This table is an inventory, not normative.** The rules in D3–D7 are binding. Each row is checked
against the pin by that command's own crate note, which also records the entry point's refusal and
reply (D4).

| Class | Command variant | Plan (op, submit) | Created on / runs on | Handoffs |
|---|---|---|---|---|
| ap_cap_report | ApCapReport | ap_cap_report T | agent | P (decoded report) |
| ap_metrics_report | ApMetricsReport | node-local (D3) | agent | P |
| beacon_query | BeaconQuery | beacon_query T | ctrl | P; O (tracked MID) |
| beacon_report | BeaconReport | beacon_report T | agent | P |
| bsta_cap | BstaCapQuery | bsta_cap_query T | ctrl | P |
| btm_report | BtmReport | sta_steer_btm_report T | agent | P |
| cfg_renew | CfgRenew | tx_cfg_renew T; agent candidate: radio slot 0; refused per radio on the agent's entry points; never refused on the controller | agent and ctrl | P |
| channel_pref_query | ChannelPrefQuery | channel_pref T | agent | P (echoed MID) |
| client_assoc_ctrl_req | ClientAssocCtrl | client_assoc T | ctrl | P |
| client_cap_report † | ClientCapReport | client_cap_report T | agent | — |
| dev_init | DevInit | al_insert F, em_insert T; candidates: every radio node | agent | Δ at submit; create nodes |
| dev_test | DevTest | db_cfg F, al_insert F, net_ssid_update F, em_test T | ctrl | P; Δ |
| em_config | EmConfig | bss_delete F, topo_sync T, ap_cap_query T, channel_pref T, policy_cfg T, channel_sel T, channel_scan_req T, topo_update F, topo_publish T. One chain per radio; never refused. | ctrl (internal, from m2_tx) | R |
| get_channel †, get_device †, get_mld_config †, get_network †, get_radio †, get_ssid † | Get* (read requests) | none (answered from the model) | ctrl / agent | R |
| link_quality_report | LinkQualityReport | link_quality_report T | agent | P |
| mld_reconfig | MldReconfig | mld_reconfig T | ctrl | R |
| onewifi_cb | OneWifiCb | owconfig_cnf T | agent | P |
| op_channel_report | OpChannelReport | op_channel_report T | agent | P |
| remove_device | RemoveDevice | db_delete F, em_delete F, dm_delete F (all run at submit; reply `not_ready`); one chain per device | ctrl | P (key); removal |
| reset | Reset | em_reset F, dm_delete_all F, db_reset F, db_cfg F (all run at submit; reply `not_ready`) | ctrl | P; Δ |
| scan_channel | ScanChannel | channel_scan_req T | ctrl | P |
| scan_result | ScanResult | channel_scan_res T | agent → ctrl | Δ; P |
| set_channel | SetChannel | channel_sel T | ctrl | P |
| set_policy | SetPolicy | one stage per device: the first descriptor only. The class declares `db_cfg T, policy_cfg T`, but upstream's controller submits each device's command with no `clone_for_next` (`dm_easy_mesh_ctrl.cpp:4373-4384`), so the port does not add a second stage (which would send an extra Policy Config Request). Refused at the entry point. | ctrl (agent: direct path, D3) | P (changed set); Δ at finish |
| set_radio | SetRadio | em_update T | ctrl | P; Δ |
| set_ssid | SetSsid | db_cfg F, em_update T, net_ssid_update F | ctrl | P; Δ |
| sta_assoc | StaAssoc | four variants (`dm_easy_mesh_ctrl.cpp:3582-3637`): (a) default: topo_sync T, sta_cap T, topo_publish T; (b) disassociation, or reassociation of a known non-MLO station: topo_update F, topo_publish T; (c) reassociation of a known MLO station: the default with sta_cap's submit set to false; (d) new non-MLO station with a resolvable BSS: sta_cap T, topo_publish T. Stages after the first use base-type behaviour (the retry override is lost). Never refused. | ctrl (internal) | P: the Client Association Event body and the peer AL MAC from its own TLV (`em_configuration.cpp:2322-2352`) |
| sta_disassoc | StaDisassoc | sta_disassoc T | ctrl → agent | P |
| sta_link_metrics | StaLinkMetrics | sta_link_metrics T; agent candidates: radio nodes with an associated station | agent and ctrl | P (stations, MID); Δ |
| sta_list | StaList | ctrl_notify T, sta_aggregate F; candidate: the named radio | agent | P (assoc and disassoc lists); Δ at submit |
| start_dpp | StartDpp | dpp_update T (upstream: `num_orch_desc = 0` but `desc[0]` set; the port uses one stage) | ctrl | P |
| sta_steer | StaSteer | sta_steer T | ctrl → agent | P |
| topo_sync † | TopoSync | topo_sync T | ctrl | R |
| unassoc_sta_query | UnassocStaQuery | unassoc_sta_link_req_query T | ctrl | P |
| unassoc_sta_result | UnassocStaResult | unassoc_sta_result T | agent | Δ; P |
| vendor_data | VendorData | vendor_data T | agent | P (raw bytes) |
| generic † (no constructor) | — | — | — | — |
| em_cmd_t (base; used directly by the CLI for get requests) | — | — | — | — |

Each command's own port re-reads its upstream class and checks this row.

## Revision history

- **rev 1:** decision draft with options.
- **rev 6 (fixes after review round 2 of the rev-4 design; not re-reviewed: two-round limit,
  user decision):**
  - idle timers restart on input, for exact upstream timing;
  - rows commit individually, as upstream;
  - `block_in_place` with at least 2 workers;
  - the BSS-delete flag and its retry;
  - M2 send-success ordering;
  - the SSID-mismatch first query, sender and timestamp clears;
  - the topology-ready gate;
  - the STA MLD refresh before fan-out;
  - Autoconfig Search's commit is deferred until after dispatch;
  - the Early AP Capability target;
  - `m2ctrl_configuration`'s source;
  - "eleven model reads";
  - 0007 D2 amended.
- **rev 5 (review round 1 of the rev-4 design):**
  - replies carry result documents;
  - agent refusal is split by source (socket replies, callback and engine events drop);
  - routing borrows the model and returns ordered effects (Early AP Capability, Autoconfig Search);
  - database points are complete (analyze-time deletes, startup), with failure effects per path;
  - the database stall is stated as an intentional difference, with mitigations;
  - the SSID-recovery transitions and the effects of removing an em_config command;
  - `MissedTickBehavior::Delay`;
  - the 500 ms delay is anchored to send completion;
  - `StaAssoc` carries the AL MAC;
  - a citation and row-number fix.
- **rev 4 (new design after two rounds; user decisions D10.5–6):**
  - the pending deadline is dropped;
  - database I/O is synchronous in the role actor, so the hold is removed;
  - refusal and replies follow upstream per entry point (agent refusals, `not_ready` for
    all-`submit = false` chains);
  - the reply rides on the request envelope;
  - `OpContext` is keyed by chain;
  - the D6 fan-out row is corrected;
  - em_config's timeout branch guard and timer reset are stated exactly;
  - set_policy's submit-time write, sta_aggregate's full maps, and sta_assoc's peer AL MAC are
    recorded;
  - the 500 ms delay starts at the send hand-off;
  - `em_commit_info_t` goes to `em-ctrl`;
  - 30 `clone_for_next` sites;
  - the transport order is corrected;
  - Appendix A is an inventory, not normative.
- **rev 3 (review round 1):**
  - em_config's two timeout paths (AL-scoped cancel with renew vs global cancel);
  - refusal per entry point;
  - pending-stage clock starts at chain predecessor completion;
  - database-backed submit stages hold the chain until confirmed;
  - Easy Connect waits and acting callbacks as states and `Outputs`;
  - `NodeKey` with AL/radio kind;
  - one `Command` per chain, with the reply in the role actor;
  - set_policy is one stage;
  - sta_assoc's four plan variants;
  - per-command agent candidate rules;
  - `sta_cap_update` is the controller's inbound delta;
  - STA-list associated and disassociated lists;
  - port order and `STATUS` dependencies;
  - 29 `clone_for_next` sites;
  - coverage-row owners.
- **rev 2:** plan-review findings resolved, and the user's decisions recorded:
  - message ids and the changed-policy set as command data;
  - the handler signature reaches sibling nodes;
  - hooks take a node view and return effects;
  - commands from every source, with an `Origin`;
  - Easy Connect inline;
  - node creation;
  - the R6 amendment;
  - peer-visible changes separated from internal fixes.

  A stage-order trace corrected rev 1's "LIFO activation": upstream activates the oldest eligible
  command (`queue_push` adds at the head).
