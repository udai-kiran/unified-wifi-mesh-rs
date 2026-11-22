# 0010: Vertical slices, the AL-SAP transport, and rules for the engine port

| | |
|---|---|
| Status | **Accepted** (2026-11-21, user): open choices 1 to 6 decided as recommended, with Docker on x86_64 as the first test platform (D13). Choices 7 to 11 wait for their slice. Rev 2 was not re-reviewed after the plan review's fixes |
| Date | 2026-11-21 |
| Upstream | `rdkcentral/unified-wifi-mesh` @ `ba4180ba36748629f10c20babc4320dd69c9db4f` |
| 1905 layer | `rdkcentral/ieee1905-rs` @ `a6443225db82f7e8aed1c29644d2ed0b947b4d6e` (read, not built or run) |
| Amends, if accepted | 0002 D1 (send completion), D4 (M2 send), D11 (order); 0004 R6; `STATUS.md` porting order. 0009 A10 only if open choice 7 changes it |
| Affects | `em-transport`, `em-cmd`, `em-orch`, `em-core`, `em-db`, `em-easyconnect`, `em-agent`, `em-ctrl` |

## Context

`em-types`, `em-utils`, `em-crypto`, `em-msg` and `em-dm` are ported, about 62k lines of Rust with
tests. Nothing runs end to end yet: no controller has exchanged a message with an agent. About 60k
lines of upstream C++ remain. The bottom-up order in 0002 D11 leaves all integration risk until
the end of that work.

This note rests on the design notes, on Wi-Fi EasyMesh and IEEE 1905.1 read through the project's
spec server, on the `ieee1905-rs` source at the commit above, and on the pinned upstream lines cited
below. Nothing was built, run or measured. Statements about runtime effects are predictions from
reading the code.

### What the specs require that no note covers

| Requirement | Source | State in the notes |
|---|---|---|
| Respond within one second (1905 Ack or the named response) | EasyMesh §7.3, 7.4, 8.2, 8.2.3, 10.2.1, 11.7, 19.1.3, 20.2.2 | Not mentioned. 0002 D1 accepts database stalls of up to the 5 s busy timeout |
| Fragment above 1500 octets; reassemble before delivery | 1905 §7.1.1, 7.1.2; EasyMesh §15.2 | 0005 assumes AL-SAP / `ieee1905-rs` does it |
| Relay a relayed multicast exactly once | 1905 §7.6 | Not assigned |
| A device can identify repeats by AL MAC and MID | 1905 §7.5 to 7.7; EasyMesh §15.1 | Not assigned |
| New MIDs increment by one per entity | 1905 §7.8 | 0002 D1 has one counter in the role actor; the 1905 layer's own counter is not considered |
| A resend uses a **new** MID | EasyMesh §15.1 | 0002 D4 says "resent", with no MID rule |
| For a DPP-onboarded device, receive in the order reassemble, MIC, decrypt, process. Transmit in the order encrypt, MIC, fragment | EasyMesh §15.3 | No note places MIC and decryption |
| Agent stores channel preferences in non-volatile storage. After a reboot it may use or flush them | EasyMesh §8.2 | 0002 D2 gives `em-db` to the controller only |

### What `ieee1905-rs` does

| Topic | Behaviour at the pin | Where (`ieee1905-core/src`) |
|---|---|---|
| Interface | Two UNIX stream sockets (control, data), each length-delimited. Registration names the role (agent 0x01, controller 0x02) and returns the local AL MAC | `al_sap.rs`, `registration_codec.rs` |
| Connection order | It accepts control, then data, and only then reads registration. A client must connect both before waiting for the response. Only end of file on the data socket ends a session | `al_sap.rs:176-181`, `:238-241`, `:672-688` |
| SDU | 6-octet source, 6-octet destination, `is_fragment`, `is_last_fragment`, `fragment_id`, then the CMDU. At most 65 517 payload octets per frame, which matches the 0005 budget | `sdu_codec.rs`, `al_sap.rs:374-379` |
| Clients | One at a time. After a disconnect it restarts about 2 s later | `al_sap.rs:96-107`, `:176-181` |
| Fragmentation | Yes. TLV boundary by default, octet boundary once a WSC message's Profile-2 AP Capability advertises DPP, **including our own outgoing one**. A TLV too large for a TLV-boundary peer fails with a log line | `cmdu_codec.rs:2453-2523`, `al_sap.rs:480-518`, `cmdu_proxy.rs:967-971` |
| Reassembly | Yes, keyed by (source, MID), without message type. Expires after about 3 to 6 s | `cmdu_reassembler.rs:77-87`, `:116` |
| Relay | **No relay code.** Forwarding is delegated to a Linux bridge | no `relay` in the source; its ADR-0007 |
| Duplicate filtering | **None** beyond duplicate fragments | `cmdu_handler.rs:935-1030` |
| MIC and encryption | **Not implemented.** Unticked on its roadmap; `crypto_engine` is feature-gated and unused | README, `lib.rs:29-30` |
| Send result | **None.** The send is spawned as a task and reports `Ok`. Failures, including an unknown destination AL MAC, are only logged. Separate tasks mean write order is not guaranteed wire order | `al_sap.rs:691-728`, `cmdu_proxy.rs:919-921` |
| MIDs | Its own global counter for the CMDUs it originates | `cmdu_message_id_generator.rs` |
| Peer learning | Nodes are inserted from Discovery (every 30 s), Query or Search. An Autoconfiguration **Response does not insert a node** | `cmdu_proxy.rs:44`, `topology_manager.rs:1080-1113` |
| Delivery to the client | Only when the frame's source is a known **AL MAC** key, so a peer sending from an interface MAC is not delivered. Topology Discovery, Link Metric and Higher Layer Query messages are consumed | `cmdu_handler.rs:150-226`, `:978-980` |
| Peer eviction | A node left in `ConvergingLocal` or `ConvergingRemote` for **5 s is removed**. In passive mode a received Query, or a sent Query, starts that state | `topology_manager.rs:749-772`, `:911-920`, `:986-998` |
| Delivered header | Rebuilt: version 0, fragment 0, flags 0x80, so the relay bit is lost. The SDU's *source* field holds the **local** AL MAC and its *destination* field the **remote** one | `cmdu_handler.rs:993-1009` |
| Topology Query | In passive mode (its default) it reaches the client, which must answer | `topology_manager.rs:911-922` |
| Rewrites on send | Replaces Device Information, Bridging Capability, 1905 Neighbor and L2 Neighbor TLVs in a client's Topology Response. Adds a role TLV to Search and Response only if missing | `cmdu_proxy.rs:304-490`, `:930-963`, `:1021-1055` |

## Decision

### D1. Deliver in vertical slices

> **Amended by [0014](0014-init-config-and-al-mac.md) (2026-11-22, user decision):** DPP, 1905 keys and encryption (S7) come **before** S3 to S6, after the admission and per-AP work; the order is in the roadmap recorded in `STATUS.md`.

> **Amended by [0013](0013-s2-onboarding.md) D2 and D10 (2026-11-22):** in S2 the minimum `em-db` is an in-memory store seeded from a reset file (`--reset-file`, default `/nvram/Reset.json`), and the S2 exit checks include a lost-M2 run.

Each slice ends with a controller and agent that **run** and complete one more protocol flow.
Crates grow only as far as the slice needs. Each slice still gets a scope note, parity cites, golden
data where it applies, reviews and a `STATUS.md` entry.

| Slice | Flow | Spec | Exit check |
|---|---|---|---|
| S0 | `em-transport` AL-SAP client; test harness with a scripted peer and virtual time; a **platform fake** for radio data and apply confirmation (open choice 4); the Docker setup (D13) | 1905 §6, 7 | Registers with a real `ieee1905-rs`, round-trips an SDU between two containers, recovers from its restart |
| S1 | Controller discovery: Search and Response | EasyMesh §6.1, 1905 §10.1.1 | Agent finds the controller over a veth pair. Search keeps retrying, and the timeout exceeds one 30 s Discovery interval (the agent's service drops a Response from a peer it has not learned) |
| S2 | Onboarding one radio: Search, Response, WSC M1, WSC M2, platform confirmation, with the minimum `em-db` | EasyMesh §5.2.6, 7.1 | Agent reports configured; the key lifecycle tests of 0006 D5.3 pass; M2 lost to an evicted peer recovers (D4); first timing measurement (D6) |
| S3 | Topology and capability reporting | EasyMesh §6.2, 7.2, 9; 1905 §8 | Controller's model matches the agent, comparing the TLVs the service rewrites; no peer eviction under a slow reply (D6); three nodes on a bridge exercise relay |
| S4 | Channel selection, agent persistence | EasyMesh §8 | One-second response tested |
| S5 | Metrics | EasyMesh §10 | Several STAs in one Associated STA Link Metrics exchange (D3); link-metric gap settled (D12) |
| S6 | Steering | EasyMesh §11 | |
| S7 | Easy Connect and 1905-layer security | EasyMesh §5.3, 13 | DPP onboarding; MIC and encrypted exchange; D5 blockers closed |
| S8 | Remaining commands, IPC bridges, TR-181 | 0002 Appendix A | Every Appendix A row closed |

This replaces the crate-by-crate order of 0002 D11. "Configuration first, provisioning last" still
holds. 0002 D2 makes `em-core` depend on `em-easyconnect`, which has no code, so S1 needs open
choice 5 settled.

### D2. Transport: AL-SAP to `ieee1905-rs` (open choice 1)

- `em-transport` is an **AL-SAP client** over the two UNIX sockets, with its own small framing code.
  It connects both sockets before awaiting registration and closes the data socket to end a session.
- `ieee1905-rs` is an external process pinned at the commit above, run in its default passive mode.
  It is **not** a crate dependency: it is `publish = false`, brings a large dependency tree, and
  holds global singletons.
- The raw `AF_PACKET` path named in 0004 R6 is deferred. Using it would mean writing fragmentation,
  reassembly, relay and topology discovery ourselves.
- The client handles the service restarting (its ADR-0017): reconnect, re-register, and rebuild peer
  state from fresh observations.
- **Known limit:** a conforming peer that sends from an interface MAC rather than its AL MAC is not
  delivered by the service.

### D3. What the port owns above the 1905 layer

- **Duplicate filtering (open choice 3).** A bounded per-peer filter with key (remote AL MAC, MID,
  message type) applies **only to message types that EasyMesh Table 21 or 1905 Table 6-4 send by
  multicast or reliable multicast**, which is where §15.1 creates repeat copies. The delivered header
  has lost its relay bit, so scope comes from the message type.
  - **Unicast is not filtered.** Upstream's agent sends one Associated STA Link Metrics Response per
    STA, all with the same MID and type (`em_metrics.cpp:1208-1219`), and a filter would drop all but
    the first. This is an intentional difference from a strict reading of 1905 §7.7, with a
    multi-STA test.
  - A filter entry is recorded **only after the message passes the security stage** (D5), so a forged
    frame cannot suppress a genuine one.
- **Address mapping.** On delivery, the client maps the SDU's swapped fields to `{ local, remote }`, so
  nothing above `em-transport` sees the swap. A pinned test uses bytes from the service's encoder.
- **Unknown peers.** A send to an AL MAC the service does not know vanishes silently, so protocol
  resends are the recovery path.
- **MIDs.** The role actor keeps one counter (0002 D1). `ieee1905-rs` has its own, and AL-SAP cannot
  share it, which does not meet the letter of 1905 §7.8. The sharpest effect is in the peer's
  reassembler, keyed by (source, MID), if two fragmented messages collide. **Accepted risk**, recorded
  as a follow-up.

### D4. "Send succeeded" means "handed to the socket"

AL-SAP reports no per-frame result. Amending 0002:
- **D1, the 500 ms delay** before EAPOL 1 starts once the frame is written to the data socket. Only a
  socket write failure triggers the EAPOL skip.
- **D4 internal fix 3, M2 ordering.** "M2's send succeeded" means the socket write succeeded. Upstream
  then records `wsc_m2_sent` and submits `m2_tx` (`em_configuration.cpp:6263-6278`). If the service
  drops M2 because the peer is unknown, the controller has advanced anyway.
- **Not yet established:** which path recovers from that loss. S2 must trace it in upstream and test
  it before relying on it.
- AL-SAP does not guarantee that frames leave in write order. A handler must not depend on the order
  of two frames sent back to back.

### D5. Fixed message pipeline (EasyMesh §15.3)

- **Receive:** `ieee1905-rs` reassembles; the role actor checks the MIC and decrypts; then the
  duplicate filter (D3) runs; then an engine handles the message.
- **Transmit:** the handler builds TLVs; the role actor encrypts and adds the MIC; `ieee1905-rs`
  fragments.
- The security stages are pass-through until S7, but they exist from S1 so handlers never change.
- **Blockers for S7**, each needing a change to `ieee1905-rs` or a different path (open choice 8):
  1. The Topology Response rewrite inserts **plaintext** TLVs beside an Encrypted Payload TLV, where
     §13.3.2 requires every TLV except End of Message to be replaced by it.
  2. Delivery rebuilds the header with version 0, while the MIC and SIV associated data cover the
     first six CMDU octets, so a sender using another version fails verification.
  3. §13.2.3 forbids relaying a message that fails the counter check, but the bridge relays below
     any such check.
  4. The service may use octet fragmentation toward a non-DPP peer (see its table row).

### D6. Timing limits are tested

Two limits apply:
- **One second** to respond to each message the spec times.
- **Under 5 s** to answer a Topology Query, and to have a sent Query answered. Otherwise the service
  evicts the peer, and sends and deliveries vanish until it is relearned.

From S2 the harness asserts both. S2 measures the longest role-actor stall on the target hardware for
database commit, dirty flush, and WSC/DPP cryptography. **0002 D1 stays as it is until that
measurement exists**, although its accepted 5 s database stall sits exactly at the eviction limit. If
a source breaks a limit, a follow-up note decides the fix. 0002 rev 3 tried holding chains on database
completion and rev 4 removed it, so that design is not reopened without evidence.

### D7. Parity rule for engines (open choice 2)

0008 D7/D8 ("the spec wins where it is explicit, upstream where it is silent") covers TLV bytes. It
extends to engine behaviour:
1. **Explicit in the spec:** follow the spec.
2. **Silent, and a peer can observe it:** keep upstream.
3. **Internal scheduling that no peer can observe:** free to change.

Every deliberate difference keeps the project rule: an upstream cite and a test tagged
`intentional-difference:<id>`.

This rule does **not** decide the zero-length TLV stop. The spec does not define an empty Backhaul STA
MLD value, so rule 2 keeps upstream, which matches the user's decision in 0009 A10. Changing that is
a separate choice (open choice 7).

### D8. Resends use a new MID

Per EasyMesh §15.1. The node's `OpContext` tracks the latest MID sent. Whether a reply to an earlier
MID of the same operation is accepted is decided in the `em-core` note, against upstream. It must
account for the service: a reply to an older MID does not clear `ConvergingLocal`
(`cmdu_handler.rs:484-494`), which can lead to eviction.

### D9. Timers stay as 0002 D1 for now

0002 D1 restarts the 250 ms orchestrator timer on every input, so steady traffic can postpone command
timeouts and the dirty flush. Changing this is **not** decided here, because:
- timeouts trigger cancels and renews that peers can observe, so D7 rule 2 keeps upstream;
- the idle-time flush is a listed mitigation for the synchronous database decision (0002 D10.6), and
  absolute deadlines could make it stall the actor during traffic.

S3 tests sustained traffic and records how long timeouts and the flush are postponed. A follow-up
decides from that evidence, treating timeouts and the flush separately.

### D10. Agent persistence (open choice 9)

EasyMesh §8.2 requires the agent to store channel preferences in non-volatile storage, and allows it
to flush them after a reboot. The agent gets a small store owned by its role actor, added in S4. The
format is decided in the `em-agent` note.

### D11. One client per 1905 instance (open choice 10)

A device running both a controller and an agent cannot share one `ieee1905-rs` instance at the pin.
The slices run them on separate devices or namespaces. Co-location is out of scope until a later note.

### D12. Backhaul link metrics are a known gap

EasyMesh §10.1 has the Controller and Agent send the 1905 Link Metric Query. The service consumes the
query and its response, so an AL-SAP client never receives the reply. S5 records this as a limitation
or arranges another interface. It does not block earlier slices.

### D13. Test platform: Docker on x86_64 first (user decision)

- Each role runs in its own container: the controller container runs `ieee1905-rs` and `em-ctrl`, and the
  agent container runs `ieee1905-rs` and `em-agent`. Each pair talks over the AL-SAP sockets inside its
  container, which also satisfies D11 (one client per instance).
- The containers share one Docker bridge network. This is the setup `ieee1905-rs` ships
  (`docker-compose.yaml`: two nodes on a bridge, `NET_RAW` and `NET_ADMIN`) and the Linux bridge its
  ADR-0007 relies on. 1905 frames use EtherType 0x893A, and the 1905 multicast address
  01:80:C2:00:00:13 lies outside the range a Linux bridge refuses to forward by default (to be confirmed
  in S0).
- A third container on the same bridge gives the three-node relay check of S3.
- The agent container has no radios. Its radio data and apply confirmation come from the platform fake
  (choice 4).
- The first platform is x86_64. ARM follows later, as a cross-built image or a real board.
- **Timing limits (D6):** numbers from a Docker host on x86_64 are a lower bound for the stall, not the
  measurement D6 asks for. D6's limits are asserted in the harness from S2, and measured again on an
  ARM target before they are relied on.

## Open choices

**Decided by the user (2026-11-21):** choices 1 to 5 as recommended; choice 6 as D13.

**Needed before S0 or S1:**
1. **Transport (D2).** Recommended: AL-SAP only for now.
2. **Parity rule (D7).** Recommended: accept.
3. **Duplicate filter (D3).** Recommended: multicast types only, after the security stage.
4. **Platform source for S0 to S2.** Recommended: a fake boundary, with OneWifi added in `em-agent`
   later.
5. **`em-easyconnect` in the graph.** Recommended: a minimal crate that holds only the boundary types
   `em-core` needs, which keeps the 0002 D2 graph. Alternative: amend the graph until S7.

**Needed before S2:**

6. **Target hardware for D6.** Decided: Docker on x86_64 first, ARM later (D13).

**Can wait for the slice that needs them:**

7. **Zero-length TLV stop (0009 A10).** Recommended: keep the decision. Revisit in the configuration
   engine, covering both data-loss cases and the difference between `TlvIter` and the walker at End of
   Message.
8. **S7 path (D5).** Patch `ieee1905-rs`, or carry secured messages another way.
9. **Agent persistence (D10).** Recommended: add it in S4.
10. **Co-location (D11).** Recommended: defer. If it is a near-term product need, it affects D2.
11. **Backhaul link metrics (D12).** Decide at S5.

## Alternatives considered

| Alternative | Why not chosen |
|---|---|
| Keep the crate-by-crate order | Integration risk stays until the last crate, and nothing runs for a long time |
| Depend on `ieee1905-rs` as a crate | Unpublished, heavy dependency tree, global singletons; AL-SAP is its stated boundary (its ADR-0005) |
| Write our own 1905 layer now | Adds fragmentation, topology and relay to the critical path before anything runs |
| Filter duplicates on all traffic | Drops upstream's per-STA link metric responses, which share a MID |
| Move database and crypto off the actor now | Unmeasured, and 0002 rev 4 removed a similar design after two review rounds |
| Absolute deadlines for timeouts and the flush now | Changes peer-visible timing and a database mitigation without evidence (D9) |
| Narrow `EngineCtx` before em-core | The broad context is deliberate (27 AL-wide call sites and cross-node writes, 0002 Context). The em-core note can refine it |

## Consequences

- Early slices need a running `ieee1905-rs`, network namespaces and veth pairs. Relay needs three nodes
  on a bridge.
- The port's first real interop result arrives at S2 rather than after all engines exist.
- 0008 notes that the port's bytes differ from an unpatched upstream peer on some TLVs. A slice that
  tests against upstream must record each such difference.
- Crates stay partial between slices. `STATUS.md` tracks slices as well as crates.

## Acceptance criteria

For this note:
1. The user decides open choices 1 to 5, and the decisions are recorded here.
2. 0002 and 0004 get amendment entries that point to this note. Their text is not silently edited.
3. `STATUS.md` shows the slice order.

For slice S0, which starts after acceptance:
1. A scope note exists for `em-transport`.
2. The client registers with `ieee1905-rs` at the pin, and sends and receives an SDU between two
   containers on a Docker bridge network (D13).
3. It recovers from a restart of the service.
4. Tests cover the connection order and address mapping, and pass on stable and 1.85.0.

## Revision history

- rev 1 (2026-11-21): draft from the design review in the tracking folder and a reading of
  `ieee1905-rs` at the pin.
- rev 2 (2026-11-21, plan review: 16 findings; the main ones checked against the sources by the
  coordinator):
  - the duplicate filter is limited to multicast types and runs after security (it would have dropped
    per-STA link metric responses);
  - platform data, a minimal `em-db` and the `em-easyconnect` dependency are placed in the slices;
  - added the service's 5 s peer eviction, its peer-learning rule and its connection order;
  - D9 no longer changes timers, because it conflicted with 0002 D1, D4 and D10.6;
  - the zero-length TLV stop is no longer presented as following from the parity rule;
  - the S7 blockers are restated, and the line cites corrected.
  - Not re-reviewed after these changes.
- Accepted by the user, 2026-11-21: choices 1 to 5 as recommended; Docker on x86_64 as the first test
  platform, ARM later (D13).
