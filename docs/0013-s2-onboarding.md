# 0013: Slice S2: onboarding one radio (WSC M1 and M2)

| | |
|---|---|
| Status | **Accepted** (2026-11-22, user), rev 5 after four plan reviews. User decisions: D2, D4, D5, D6, D8, D9, D10 and the dependency in D2 |
| Date | 2026-11-22 |
| Upstream | `rdkcentral/unified-wifi-mesh` @ `ba4180ba36748629f10c20babc4320dd69c9db4f` |
| Follows | [0010](0010-vertical-slices-and-transport.md) D1 (slice S2), D4, D6, D7, D8; [0012](0012-s1-discovery.md); [0006](0006-em-crypto-scope.md) D5; [0002](0002-cmd-core-orch.md) D1, D6 |
| Amends | 0010 D1 (S2 row: the minimum `em-db` is an in-memory store seeded from a reset file, D2; the lost-M2 check, D10); 0012 D10 (orchestrator effects applied per candidate, readiness hooks get effects, D3); 0012 D2 and D8 check 6a (the `dev_init` 10 s cancel becomes unreachable in S2 flows, and a second `dev_init` is allowed only for the D4 recovery, D5) |
| Affects | `em-cmd`, `em-orch`, `em-core` (WSC engine, topology), `em-agent`, `em-ctrl`, `em-db` (first code), `em-msg` (only if a codec gap appears), `docker/` |
| Spec | EasyMesh §5.2.2, §7.1, §17.1.3, §17.2.47, §17.2.94; IEEE 1905.1 §10.1.1, §10.1.2 |

## Context

S1 stops in the agent's `wsc_m2_pending` without sending M1 (0012 D2) and with the controller's AL node in
`ctrl_wsc_m1_pending`. S2 continues from there until the agent's radios reach `topo_synchronized`. Upstream cites
are to the pin, read from the pinned sources.

### Upstream, agent side

- After the Controller Capability, `handle_autoconfig_resp` sends an Early AP Capability Report when the
  controller asks (`em_configuration.cpp:6351-6365`), builds **M1** (`create_autoconfig_wsc_m1_msg`,
  `:4266-4359`; attributes in `create_m1_msg`, `:3305-3580`), validates it at profile 3 (`:6376-6380`), sends it
  with a **new MID** (`:4292`) and enters `wsc_m2_pending` (`:6388`).
- M1 TLVs: AP Radio Basic Capabilities; WSC (M1); Profile-2 AP Capability and AP Radio Advanced Capabilities with
  type and length but **no value written** (`:4319-4333`); an optional Airties vendor TLV (`:4335-4348`); End of
  Message.
- M1 attributes, in order: Version, Message Type, UUID-E, **MAC Address = the radio MAC** (`:3346-3353`), Enrollee
  Nonce, Public Key (192 octets), **Authentication Type Flags = 0** (`:3376-3409`), Encryption Type Flags,
  Connection Type Flags (0), Configuration Methods, WSC State (0), Manufacturer, Model Name, Model Number, Serial
  Number, Primary Device Type, Device Name, RF Bands (`1 << band`), then Association State, Device Password ID and
  Configuration Error with **length 2 but their 2 value octets never written** (`:3536-3563`; the buffer at
  `:6286` is not zeroed, so the octets are whatever the stack held), OS Version (zeros).
- DH: RFC 3526 1536-bit MODP, generator 2 (`em_crypto.cpp:66`, `:84`); one key pair is copied into both the
  enrollee and registrar slots (`:210-234`).
- **M2** is accepted in `wsc_m2_pending` (`handle_autoconfig_wsc_m2`, `:4762-4869`). For **each** WSC TLV it sets
  E-MAC to its own radio MAC (`:4804`), reads the attributes (`handle_wsc_m2`, `:4593-4634`, no bounds checks),
  runs `compute_keys` (`:4069-4137`; DHKey = SHA-256(DH secret), KDK = HMAC-SHA256(DHKey, N1 || E-MAC || N2), WPS
  KDF "Wi-Fi Easy and Secure Key Derivation" to AuthKey 32, KeyWrapKey 16, EMSK 32), **overwriting the keys each
  time**, and compares the Authenticator (HMAC-SHA256(AuthKey, M1 || M2), 8 octets): a mismatch is **only logged**
  (`:4820-4823`). After the loop it decrypts **every** Encrypted Settings with the **last** KeyWrapKey
  (`:4889-4895`), which works only because upstream's controller uses one N2 for all M2s. Inside, the haul type
  defaults to the WSC index (`:4908`) unless a Comcast haul-type vendor attribute is present (`:4911-4919`);
  SSID, Authentication Type and Network Key are copied **without bounds**; the **Key Wrap Authenticator is never
  verified** (`:4941-4943`); the **WFA Multi-AP Extension is never read**.
- The configuration goes to OneWifi (`em_agent.cpp:250-264` → `dm_easy_mesh_agent.cpp:285-307`) and the radio
  enters `owconfig_pending` (`:4849`), where `dev_init` can finish (`em_orch_agent.cpp:76-87`).
- OneWifi's asynchronous "private" callback (`em_agent.cpp:2284-2286`) is **dropped while any `onewifi_cb` command
  is in progress** (`em_agent.cpp:276-277`); otherwise it creates an `onewifi_cb` command whose only candidate is
  the radio named by its first BSS, in `owconfig_pending` (`em_orch_agent.cpp:397-405`). The command sets
  `onewifi_bssconfig_ind` (`em.cpp:181-182`), finishes at `topo_synchronized` (`em_orch_agent.cpp:88-91`) and is
  cancelled after 10 s, resetting a radio below `topo_synchronized` to `unconfigured` (`:64-71`). The callback
  carries the BSS list the agent then reports (`analyze_onewifi_vap_cb`, `dm_easy_mesh_agent.cpp:309-385`).
- **The Topology Query is answered per device**: it is ignored while **any** radio of the AL is below
  `onewifi_bssconfig_ind` (and not `wsc_m2_processed`); one Topology Response (`:1699`) moves **every** radio to
  `topo_synchronized` (`em_configuration.cpp:6567-6592`). `configured` is reached only by later commands.
- **No M1 resend**: `wsc_m2_pending` has no timer (`:6781-6784`); the `dev_init` TTL cancels; a renew is refused
  in `wsc_m2_pending` (`em_agent.cpp:2438-2447`).

### Upstream, controller side

- With `--db-type none`, tables read empty, `bootstrap_controller_network` inserts the network for
  `GLOBAL_NET_ID` (ControllerID = the AL-SAP MAC, `dm_easy_mesh_ctrl.cpp:10811-10848`), and **only the
  NetworkSSIDList** is taken from `/nvram/Reset.json` (`:10850-10870`; the file's network entry, ID and
  ControllerID included, is not applied, `:10864-10866`). A missing file is logged and the controller continues
  (`:10855-10858`).
- An AP-Autoconfiguration WSC message is routed by the radio ID in it: an existing radio node is set to
  `ctrl_wsc_m1_pending` unless it is in `wsc_m2_sent`; otherwise a **radio node is created** for that RUID
  (`em_ctrl.cpp:1240-1270`). The M1 is accepted in `ctrl_wsc_m1_pending` (`em_configuration.cpp:6553-6556`),
  validated (`:6211-6214`) and walked without bounds checks (`handle_wsc_m1`, `:4636-4760`). After the M2 is sent,
  the radio's previous `em_config` is removed (`:6268-6271`), then the radio enters `wsc_m2_sent`.
- M2 (`create_autoconfig_wsc_m2_msg`, `:4139-4264`) carries **the M1's MID**, an AP Radio Identifier and one WSC
  TLV per haul index up to `min(number_of_bss, 5)` (`:4197-4221`); with none it sends nothing (`:4223-4227`). Each
  M2 (`create_m2_msg`, `:2896-3165`) uses the **same N2** (`:2970-2976`) and sends Authentication Type Flags,
  Encryption Type Flags, Configuration Methods and the three short attributes with length set and value octets
  left **zero** (the buffer at `:6194` is zeroed, `:2995-3030`, `:3108-3136`). Encrypted Settings (`:5842-6015`):
  the WFA Multi-AP Extension (BSS-type bits), the Comcast haul-type attribute, SSID, Authentication Type (from the
  row, not checked against the M1), Encryption Type AES, Network Key (**`strlen + 1` octets, with a trailing NUL**,
  `:5963-5967`), the radio MAC and the Key Wrap Authenticator, AES-128-CBC under KeyWrapKey with a random IV.
- The row per haul type is the **first row with that haul type** (`dm_easy_mesh.cpp:2853-2874`); the M2 is
  skipped if that row is disabled or lacks the radio's band (`em_configuration.cpp:2908-2941`); the next row is not
  tried.
- After a successful socket write: `wsc_m2_sent` (`:6273`), then `m2_tx` submits `em_config`
  (`dm_easy_mesh_ctrl.cpp:3642-3673`): `bss_delete` (not submitted) then `topo_sync`, etc. The readiness check
  moves a radio from `wsc_m2_sent` to `topo_sync_pending` (`em_orch_ctrl.cpp:196-203`). In `topo_sync_pending`
  the controller sends a Topology Query each idle second, **only once every radio node of that AL is in
  `topo_sync_pending`**, from the first radio (`em_configuration.cpp:6869-6914`). A Topology Response without an AP
  Operational BSS TLV is rejected (`:2466-2469`); a BSS whose SSID is not in the NetworkSSIDList returns -2
  (`:2270-2286`), which sets `ssid_mismatch` and enters the 120 s mismatch path (`em_orch_ctrl.cpp:95-150`); a good
  Response moves **every** radio node of the AL to `topo_synchronized` (`:6606-6622`).
- **Lost M2.** Nothing resends M2. After 40 s in `topo_sync_pending` with no mismatch, `em_config` is cancelled
  for all radios of the AL, each becomes `misconfigured` and sends **one** renew (`em_orch_ctrl.cpp:66-92`); no
  command is left, so no further renew is sent (`em.cpp:543-546`). An agent still in `wsc_m2_pending` refuses it. *(Corrected by erratum E2: upstream's cancel hook also submits a `cfg_renew`.)*

### Spec

- §7.1: the agent sets M1's **MAC Address to its 1905 AL MAC**, valid Authentication Type Flags, and sends a
  Profile-2 AP Capability TLV and an AP Radio Advanced Capabilities TLV (also §17.1.3). The controller answers
  within one second with one M2 per BSS (or one with Tear Down), **each with a unique N2**, an Authentication Type
  consistent with the M1, the Multi-AP Extension subelement, at least PSK on one fronthaul BSS, and no more M2s
  than the radio's maximum BSS count. The agent validates each M2, configures one BSS per M2, tears down
  non-matching BSSs, and on Tear Down tears down all BSSs on the radio.
- §5.2.2: with Early_AP_Capability set, the agent sends an Early AP Capability Report before M1.
- 1905 §10.1.2: **every AP-autoconfiguration WSC message carries a new MID** (§7.8); "if the parameter
  configuration phase is not completed successfully, the AP enrollee shall restart the registrar discovery
  phase".

## Decision

### D1. Scope

S2 takes the agent's radios from `wsc_m2_pending` to `topo_synchronized` and the controller from the first M1 to
the Topology Response, **per device**: the Topology Query/Response gates are device-wide, as upstream. The Docker
agent has two radios.

### D2. Controller configuration store (user decisions "In-memory + Reset.json", "serde + serde_json")

- `em-db` gets its first code: a loader for upstream's `Reset.json` shape that returns the NetworkSSIDList rows as
  `em-dm` `NetworkSsidInfo` records, accepting both `PassPhrase` and `Passphrase` as upstream's file uses. New
  dependencies **`serde` (derive) and `serde_json`, in `em-db` only**.
- Upstream's `db-type=none` path: the network is `GLOBAL_NET_ID` with ControllerID = the controller's AL MAC from
  registration; the file's network entry (ID, ControllerID, ColocatedAgentID) is ignored, as upstream. Nothing is
  persisted in S2; SQLite comes later by its own decision.
- `em-ctrl --reset-file <path>`, default `/nvram/Reset.json`. A missing or unreadable file is fatal at startup
  (tag `reset-file-required`; upstream continues with no SSIDs and can then onboard nothing).
- The Docker reset file's first Fronthaul row is WPA2-Personal (so §7.1's "PSK on at least one fronthaul BSS"
  holds), and its Backhaul row is SAE; one M2 per haul index means only the first row of each haul type is used.
  The controller does not enforce the PSK rule; it logs a warning when no fronthaul M2 carries PSK. A unit test
  covers SAE selection and the warning.
- The controller's AL node is still created at registration (0012 D3). The STATUS `em-db` follow-ups (BSS-delete
  filter, STA staging, AKM join) are out of scope for S2.

### D3. Commands and the orchestrator (resolves the S1 follow-up)

- `em-cmd` adds `CommandType::OneWifiCb`, `CommandType::CfgRenew` and `CommandType::EmConfig` with upstream's
  plans: `onewifi_cb` and `cfg_renew` one stage each; `em_config` with `bss_delete` (pre-processed, not submitted) then `topo_sync` only (the later steps are S3
  and on).
- **`em-orch` applies role effects per candidate, as 0002 D1 requires.** `tick`/`on_event` take `&mut R::Nodes`
  and apply each hook's effects through `OrchRole::apply` (below) as the hook returns, so the next candidate sees
  the change. `ready_for_exec` and `ready_for_fini` get an effects output (upstream's controller readiness
  writes `topo_sync_pending`, `em_orch_ctrl.cpp:196-203`). Cancel can be scoped to a candidate set
  (`cancel_command(type, radios)`). `remove_chain_for_node` ports `remove_em_config_cmd_for_em`
  (`em_orch.cpp:222-260`). This amends 0012 D10 and closes the STATUS follow-up.
- `em-ctrl` gets an orchestrator and a controller rules struct for `em_config`: candidates the radio, readiness as
  upstream, the 40 s `topo_sync` transient that cancels for all radios of the AL and sends one renew per radio.
- `em-agent`'s rules add `onewifi_cb` and `cfg_renew` (D4), with upstream's candidate, readiness and finish rules.
- **Interfaces** (fixed before any brief starts):
  - `trait OrchRole` gains `type Out` and `fn apply(&mut self, nodes: &mut Self::Nodes, effect:
    RoleEffect<Self::State>, out: &mut Self::Out)`; `tick`, `on_event` and the pass take `&mut R`. Node effects
    (`CreateNode`, `SetState`) change the node table at once. `RoleEffect` gains `Renew(NodeKey)` (send one
    AP-Autoconfiguration Renew from that radio) and `RestartProto(NodeKey)`; `apply` records these in `out`, a
    request buffer the core drains **after the pass, in order**, building the renew frame with its own message-id
    counter and restarting proto deadlines. `em-orch` never names a core type.
  - `submit` still returns the pre-processing effects to the core, which applies them before `admit`.
  - The controller's M1 handler does not set the radio's state after M2. It returns, in order, the M2 frame and
    the outputs `RemoveChainForNode(radio)`, `SetState(radio, wsc_m2_sent)` and `SubmitEmConfig { radio, agent_al }`,
    which the core applies in that order after handing the frame to the transport: upstream's order
    (`em_configuration.cpp:6262-6275`: send, `remove_em_config_cmd_for_em`, `wsc_m2_sent`, `m2_tx`). If the hand-off
    fails, none of the three is applied, as upstream (`:6263-6266`). A test checks the
    radio is in `topo_sync_pending` after a second M1.
  - `remove_chain_for_node` ports `remove_em_config_cmd_for_em` (`em_orch.cpp:222-260`) with its side effects: the
    node's role state becomes `ctrl_unconfigured` and its orchestration state `Fini`; it is removed from pending
    chains, and a chain is destroyed only when no candidate remains; the type's elapsed time is reset. The scoped
    cancel also resets the type's elapsed time (`em_orch.cpp:205-213`).

### D4. Agent flow (user decisions "One per device, after all radios", "Port cfg_renew", "Restart discovery" on the `onewifi_cb` timeout)

- After a Response (S1) the radio builds M1 (D6), hands it to the transport with a new MID and stays in
  `wsc_m2_pending`. No Early AP Capability Report (D8).
- An M2 is accepted only in `wsc_m2_pending`, from the controller recorded by S1, for the radio its AP Radio
  Identifier names. It is checked as in D6, per WSC TLV with its own keys. **A rejected M2 is not terminal** (user
  decision): one bad TLV rejects the whole M2, the radio stays in `wsc_m2_pending`, its WSC session is kept, and a
  later valid M2 is accepted; D5's timer still bounds the wait.
- An accepted M2 is applied through `Platform::apply_radio_config(radio, &[BssConfig])` (one BSS per M2, Tear Down
  per §7.1); the radio enters `owconfig_pending`.
- **The fake platform confirms once per device**, after it has a configuration for every radio. The confirmation
  is an actor input (a channel from the platform task) carrying the applied BSS list (fake BSSIDs, SSIDs, bands);
  `AgentCore::platform_confirmed(bss_list, now)` stores it in the agent's model, standing in for OneWifi's
  callback and its subdoc, and submits **one `onewifi_cb` for all radios**, each moving to
  `onewifi_bssconfig_ind`. Intentional difference (upstream: one command per radio, duplicates dropped,
  `em_agent.cpp:276-277`, `em_orch_agent.cpp:397-405`; tag `onewifi-cb-per-device`). `onewifi_cb` finishes at
  `topo_synchronized` as upstream.
- **`onewifi_cb` timeout** (10 s with no Topology Query answered): every radio of the device clears its WSC session
  and returns to `unconfigured`, and a new `dev_init` is submitted so the search gate reopens (tag
  `onewifi-cb-timeout-restart`; upstream resets one radio and has no recovery, `em_orch_agent.cpp:64-71`). This is
  one of the two cases with a second `dev_init` (with the renew case below); the Docker checks never trigger either,
  and unit tests do.
- A Topology Query is answered when every radio is at least `onewifi_bssconfig_ind`, with upstream's Topology
  Response content and the confirmed BSS list in its AP Operational BSS TLV; every radio then enters
  `topo_synchronized` (`:6567-6592`). `ieee1905-rs` forwards Queries and Responses to the role process in passive
  mode and does not rewrite AP Operational BSS (it rewrites Device Information, Bridging Capability and the
  neighbor TLVs, `cmdu_proxy.rs:252`, `:935`).
- **Renew** (user decision): upstream's `cfg_renew` is ported as a command. The renew's radio is chosen by its
  SupportedFreqBand, skipping radios whose recorded controller AL MAC is not the sender (`em_agent.cpp:2400-2447`).
  A renew is refused while a `cfg_renew`
  is in progress (`em_agent.cpp:376-377`) and for a radio in `autoconfig_renew_pending`, `wsc_m2_pending` or
  `wsc_m2_processed` (`em_agent.cpp:2438-2447`). Otherwise `cfg_renew` starts: `on_start` clears the radio's WSC
  session and sets `autoconfig_renew_pending` (`em.cpp:116-118`); the proto tick, under the same gate as `dev_init`
  (`em.cpp:463-467`), sends M1 (`em_configuration.cpp:6739-6768`) and enters `wsc_m2_pending`; it finishes at
  `owconfig_pending` (`em_orch_agent.cpp:93-99`).
- **A renewed radio's M2 lost or its `cfg_renew` timed out:** D5's timer (or the 10 s TTL) returns it to
  `unconfigured`, and with no `dev_init` active or pending the agent submits a new `dev_init` for recovery, as for the
  `onewifi_cb` timeout (tag `renew-timeout-restart`; upstream's radio stays `unconfigured`). A unit test covers a renew
  followed by a lost M2.
- **After a renewed M2 is applied** the fake platform confirms the device again, and a new `onewifi_cb` runs for the
  radios below `onewifi_bssconfig_ind`.
- **What the controller's 40 s path actually yields with two radios** (traced at the pin, no further difference): the
  controller sends one renew per radio back to back (`em_orch_ctrl.cpp:84-90`); the agent refuses the second while
  the first `cfg_renew` runs (`em_agent.cpp:376-377`), so only one radio re-onboards. The other controller radio
  node stays `misconfigured` with no `em_config`, so the device-wide Query gate (D7) stays closed; the agent's
  `onewifi_cb` then times out and the D4 restart (a second `dev_init`) completes onboarding. A two-radio unit test
  with a renew from the controller timeout asserts this path.

### D5. Lost M2: the radio restarts discovery (user decision)

- Intentional difference (1905 §10.1.2; tag `restart-discovery-on-m2-timeout`): a radio in `wsc_m2_pending` with
  no accepted M2 for **more than 10 s, measured from the M1's hand-off to the transport as
  `(now - handed_off).as_secs() > 10`** on the proto tick, clears its WSC session and returns to `unconfigured`; the S1 search gate then sends Searches again (new
  MIDs, 0010 D8). Upstream has no such timer.
- **`dev_init` stays active throughout**: S1's "until found" reset now also applies while any radio is in
  `wsc_m2_pending` (tag `search-until-found`, extended). Every state S2 reaches below `owconfig_pending` is in that
  set, so **`dev_init`'s 10 s cancel is unreachable in S2 flows** (amends 0012 D2). `dev_init` finishes when every
  radio is at least `owconfig_pending`, as upstream (`em_orch_agent.cpp:76-87`). Apart from the two D4 recoveries
  (`onewifi_cb` timeout, renewed radio), there is no second `dev_init` (0012 check 6a holds; Docker triggers
  neither).
- Tests: S1's `after_both_responses_the_cancel_resets_only_the_last_candidate` is rewritten to assert that no
  `dev_init` cancel happens in a 60 s lost-M2 run; the single-node cancel itself stays covered by `em-orch`'s own
  tests; D5's `> 10` boundary is pinned (no restart at 10.9 s after hand-off, restart by 12 s).
- A radio already in `owconfig_pending` waits; the per-device confirmation (D4) comes after the restarted radio's
  M2. The controller's restart M1 arrives in `topo_sync_pending`, which upstream already accepts (D7); no
  controller difference is needed.
- The controller keeps upstream's 40 s `topo_sync` timeout and one renew per radio.

### D6. M1 and M2 content and checks (user decisions "Spec content + security fixes", "Unique N2", "New MID", "WPA2 + WPA3 Personal")

Each item is tagged with its upstream cite in its test.
- **M1:** MAC Address = the agent's AL MAC (`m1-mac-al`); Authentication Type Flags = WPA2-Personal | SAE for S2's
  fake radios (`m1-auth-flags`); the three short attributes with deterministic value 0 instead of uninitialized
  octets (`m1-short-attrs`); Profile-2 AP Capability and AP Radio Advanced Capabilities with real content
  (`m1-capability-tlvs`); no Airties TLV.
- **E-MAC:** the agent uses the MAC it put in M1; the controller uses M1's MAC Address attribute (upstream's
  controller does too, `:4681-4683`), so either agent works with the port's controller. *(See erratum E1: key derivation works with either agent, but an upstream agent gets only a Tear Down M2.)*
- **M2:** a new MID (1905 §10.1.2; `m2-new-mid`); a fresh N2 per M2 (`m2-unique-n2`) — **an upstream agent then
  fails whenever it receives two or more M2s** (its single-key decrypt, `:4889-4895`); recorded, with a vector;
  Authentication Type Flags, Encryption Type Flags and Configuration Methods filled (`m2-flags`); the
  Authentication Type checked against the M1's flags, a row the agent does not support being skipped
  (`m2-auth-from-m1`); the Comcast haul-type attribute kept, so upstream agents map haul types correctly (parity);
  Network Key sent without a trailing NUL (`network-key-no-nul`); at most the radio's maximum BSS count; the first
  enabled row with the haul type and band (`m2-row-search`, upstream takes only the first row with the haul type);
  a Tear Down M2 when no row matches (`teardown-m2-no-row`; upstream sends nothing).
- **The agent accepts** a shared N2 (upstream's controller), a trailing NUL on the Network Key (stripped), and
  derives keys per M2.
- **AKM limit** (user decision): the controller maps the M1's flags to at most 10 AKMs, keeping the first 10 and
  logging a warning (tag `akm-clamp-10`; upstream writes the list size unclamped, `em_configuration.cpp:4701-4711`).
- **Checks, in this order (`em-crypto` cbc.rs):** every length bounded before use (`bounded-wsc-parse`); the M2
  Authenticator verified before decrypting, a mismatch rejecting the M2 (0006 D5(c), `m2-authenticator`); padding
  failure and Key Wrap Authenticator failure give one opaque rejection (`key-wrap-auth`); the WFA Multi-AP
  Extension read for BSS type and Tear Down (`multiap-ext`).
- STATUS follow-up carried here: the DH shared-secret length (leading zeros) is checked against a vector from
  upstream's computation.
- **WSC session state** (0006 D5(3)): one record per radio and exchange in `em-core`, secrets zeroizing, cleared on
  every terminal path (M2 accepted, D5 timeout, `onewifi_cb` timeout, renew, node removal, process stop); a
  rejected M2 is not terminal (D4). The 0006 D5.3 key
  lifecycle tests are S2 acceptance tests. Each side has its own DH key pair (`separate-dh-keys`).

### D7. Controller flow

- **Radio-to-agent link:** each controller radio node records its agent's AL MAC and a per-radio
  `ssid_mismatch` flag; "every radio of the AL" means the radio nodes with that AL MAC, in insertion order
  (upstream `get_all_em_for_al_mac`, `em_mgr.cpp:294-310`).
- M1 with no peer model for its sender (never searched, or evicted by S1's 256-model cap) is dropped and logged, as
  upstream (`em_ctrl.cpp:1253-1257`). The new radio node's band comes from the M1's RF Bands attribute and is used
  for the M2 row choice.
- M1 routing ports `em_ctrl.cpp:1240-1270`: the radio node is created from the AP Radio Basic Capabilities' RUID or
  reset to `ctrl_wsc_m1_pending` unless in `wsc_m2_sent`. M1 is accepted in `ctrl_wsc_m1_pending`; M2 is built (D6)
  and handed to the transport; then, in upstream's order, the radio's previous `em_config` is removed, the radio
  enters `wsc_m2_sent` and `m2_tx` submits `em_config` (D3 interfaces).
- `topo_sync`: `on_start` moves the radio from `wsc_m2_sent` to `topo_sync_pending` (upstream's `orch_execute`,
  `em.cpp:173-175`); readiness moves it there too if it regressed to `wsc_m2_sent` (`em_orch_ctrl.cpp:196-203`). The
  controller's per-node state handling runs only while the radio's orchestration state is `Progress`
  (`em.cpp:538-546`), so Queries stop after the 40 s cancel. When a Query is sent, the controller's own AL node also
  enters `topo_sync_pending` (`em_configuration.cpp:6900-6902`); once every radio node of the AL is there, a
  Topology Query is sent each idle second from the first radio. The Topology Response is parsed for what S2
  needs (device, radios, BSSs); the AP Operational BSS check and the SSID-match check are ported; a mismatch sets
  `ssid_mismatch` and is logged (the 120 s mismatch path is S3, tag `ssid-mismatch-path-deferred`); success moves
  every radio of the AL to `topo_synchronized`.

### D8. End state and Early AP Capability (user decision)

- S2's "agent reports configured" means every radio of the agent reaches `topo_synchronized`.
- The Early AP Capability Report is deferred to S3 (tag `no-early-ap-report-s2`); §5.2.2's "shall send before M1"
  is a known S2 gap in `STATUS.md`.

### D9. Timing (0010 D6)

- The harness asserts that each M2 leaves within one second of its M1 (§7.1), and that the Topology Response
  arrives within 5 s of the last radio reaching `onewifi_bssconfig_ind` (Queries sent earlier are unanswered by
  design, D4).
- S2 records the role actor's longest stall for DH key generation, the shared secret, the KDF and AES on the x86_64
  Docker host, in `STATUS.md`.

### D10. Docker exit checks for S2 (user decision "Test switch + eviction run")

On top of S1's checks:
1. Each radio sends M1 after its Response, with a new MID and MAC Address = the agent's AL MAC.
2. The controller sends M2 within one second, with a new MID; the agent accepts it and the fake platform records
   the BSSs from the reset file's matching rows.
3. One confirmation, both radios at `onewifi_bssconfig_ind`, the Topology Response within the D9 limit, both radios
   at `topo_synchronized`; the controller logs the Topology Response and `topo_synchronized` for both radio nodes.
4. **Lost M2 (gating):** `em-ctrl --test-drop-first-m2` (test-only, documented) replaces the first M2's hand-off
   with a no-op that reports success, so the three D3 outputs (`wsc_m2_sent`, `em_config`) apply as after a real
   write that the network then loses. The agent
   restarts discovery after 10 s and both radios reach `topo_synchronized`; there is exactly one `dev_init
   submitted`.
5. **Eviction run (evidence, not gating):** 1905 CMDUs are raw Ethernet frames (0x893A), and the agent's service
   reads them through an `AF_PACKET` tap that sees frames before any ingress filter, so the block is on the
   **controller's egress**: a `tc clsact` egress filter on the controller container's interface for ether types
   0x893A and 0x88CC (`iproute2` in the runtime image, `NET_ADMIN` already granted; `ieee1905-rs` sends through the
   qdisc). It is installed when `em-ctrl --test-drop-first-m2` logs the dropped M2, which leaves the agent's radio in
   `wsc_m2_pending` for at least 10 s, and lasts 70 s, so the agent's service stops hearing the controller and
   evicts it after its 60 s unseen limit (`topology_manager.rs:749-783`). Predicted (traced at the pin): the radio restarts discovery once (D5) and then sends a Search every idle second
   with no answer, while the other radio waits in `owconfig_pending` (or, if its M2 left after the block was
   installed, both radios restart). Inside the block the controller's `em_config` passes its 40 s limit, both
   controller radio nodes become `misconfigured` and both renews are lost. After the block lifts, the controller's
   next Topology Discovery (at most 30 s) is learned again; the restarted radio re-onboards, but the other controller
   node stays `misconfigured`, so no Query is sent; the agent's `onewifi_cb` times out and the D4 restart (a second
   `dev_init`) completes onboarding. The run records the times in `STATUS.md`. (Queries the agent ignores do not evict it: each sent or
   received Query refreshes the peer, `cmdu_proxy.rs:977-989`, `topology_manager.rs:911-920`; eviction follows only
   when they stop unanswered or a Response's MID does not match, `cmdu_handler.rs:484-494`.)
6. Restarting `ieee1905-rs` on either node after configuration: the role processes reconnect and keep their state.
7. The workspace checks pass on stable and 1.85.0; the key lifecycle tests pass.

## Alternatives considered

| Alternative | Why not chosen |
|---|---|
| SQLite now | User decision: in-memory and a reset file for S2 |
| Hand-written JSON parser | User decision: `serde_json` |
| Upstream's M1/M2 content byte for byte | User decision: the spec wins where explicit |
| Shared N2, echoed MID | User decisions: unique N2 and a new MID, as the spec says |
| Per-radio confirmations like upstream | User decision: they block each other with two radios |
| Upstream's lost-M2 recovery only | User decision: it fails if the peer stays evicted |
| A second `dev_init` on restart | Resets configured radios and breaks 0012 check 6a |
| Stop at `owconfig_pending` | User decision |

## Acceptance criteria

1. D2 to D7 implemented with only the dependencies decided here; workspace checks pass on stable and 1.85.0.
2. Tests cover M1 and M2 bytes in both directions, interoperability vectors with upstream's field choices
   (including the multi-M2 failure with an upstream agent), every D6 tag, the KDF against known vectors, every
   terminal path of the WSC session, the D5 restart and its boundary, per-candidate orchestrator effects, the
   per-device confirmation with two radios and with one lost M2, the device-level Topology gates, the SSID-match
   check, and the controller's 40 s timeout with one renew per radio.
3. The D10 exit checks pass on x86_64; the D9 stall measurement and the eviction run are recorded.
4. `STATUS.md`, the root README and the crate READMEs are updated; 0010 and 0012 get amendment pointers.

## Errata and review round 1 changes

- **E1 (2026-11-22, correctness review round 1, F2):** D6's "either agent works with the port's controller" holds
  for key derivation only. Upstream's M1 carries Authentication Type Flags = 0 (`em_configuration.cpp:3379-3415`, the AKM loop is commented out),
  so the port's row match (`m2-auth-from-m1`) finds no row and the port's controller sends only a Tear Down M2
  (`teardown-m2-no-row`). An upstream agent is therefore not configured by the port's controller in S2. The test
  `upstream_radio_mac_and_zero_flags_use_the_m1_emac_for_m2_keys` pins this. Recorded in `STATUS.md` as an
  interoperability gap, next to the multi-M2 failure of `m2-unique-n2`.
- **R1 (S2-ADV-01, medium):** the agent's recovery (the `onewifi_cb` timeout restart and the recovery `dev_init`)
  left a queued `cfg_renew` in place. After the device synchronized it ran again, sent a fresh M1, and caused a
  second `onewifi_cb` timeout and a third `dev_init`. The recovery now cancels the `cfg_renew` of the radios it
  re-onboards (part of `onewifi-cb-timeout-restart`, a difference upstream lacks).
- **R2 (found by the Docker harness):** after the `onewifi_cb` timeout the agent logged its restart without the
  reason, because a cancelled command still counts as in progress until a later pass destroys it. The reason now
  comes from the radios the orchestrator pass itself made `unconfigured`.

- **E2 (2026-11-22, adversarial review round 2):** the Context statement that upstream sends one renew and then
  no more is wrong. Upstream's `em_config` cancel hook posts a `cfg_renew` bus event (`em_orch_ctrl.cpp:404-416`),
  which submits a controller `cfg_renew` (`em_ctrl.cpp:267-275`, `dm_easy_mesh_ctrl.cpp:3427-3458`). Its radio stays
  `misconfigured`, whose proto tick sends a renew (`em_configuration.cpp:6865-6866`), up to `EM_MAX_RENEW_TX_THRESH`
  = 5 (`em_orch_ctrl.cpp:171-175`). This is traced from the source, not run. The port sends one renew per radio, an
  undecided difference with no `intentional-difference` tag. If every renew is lost after the agent reached
  `topo_synchronized`, the two sides stay split until a process restart (reproduced in simulation). Recorded in
  `STATUS.md` for a user decision.
- **R1 limit (round 2):** the cancel runs when the recovery `dev_init` is submitted. A Renew that arrives later,
  during the recovery search, is still queued and causes one extra re-onboarding after synchronization. It needs an
  outage ending in a window under 1 s around the controller's 40 s timeout, heals by itself and does not repeat.

## Revision history

- rev 1 (2026-11-22): first draft after two traces of the pinned sources. User decisions: in-memory store and reset
  file; restart discovery on a lost M2; spec content and security fixes; end at `topo_synchronized`, no Early AP
  report.
- rev 5 (2026-11-22, fourth plan check: 5 findings): the eviction run's prediction and the two-radio renew outcome
  corrected (both complete through the `onewifi_cb` restart); a failed hand-off applies no outputs; the test switch
  replaces the hand-off; renew radio chosen by band; recovery submits `dev_init` only when none is active or pending.
- Accepted by the user (2026-11-22), including the two-radio renew outcome through the agent's restart.
- rev 4 (2026-11-22, third plan check: 4 findings): the controller's M2 outputs in upstream's order (send, remove
  `em_config`, `wsc_m2_sent`, submit); `cfg_renew` ported as a command with its gate, TTL, refusals and finish, and
  the recovery of a renewed radio; one `OrchRole::apply` signature with a request buffer drained after the pass; the
  eviction run blocks the controller's egress, triggered by the test switch, with a corrected prediction.
- rev 3 (2026-11-22, second plan review: 9 findings): `dev_init`'s cancel unreachable in S2, the S1 test rewritten
  and 0012 amended; orchestrator hand-offs (`Renew`, `RestartProto`, `RemoveChainForNode`, `SubmitEmConfig`) and
  `remove_chain_for_node`'s side effects; the radio-to-agent link, the `Progress` gate, `on_start`, the band source
  and the no-peer-model case; a layer-2 eviction run with its predicted outcome; user decisions: port `cfg_renew`,
  restart discovery on the `onewifi_cb` timeout, a rejected M2 keeps the session, clamp AKMs to 10; the reset file's
  row order and the PSK warning; the platform interface and hand-off timing.
- rev 2 (2026-11-22, plan review: 17 findings, the main ones checked by the coordinator at the pin): device-level
  Topology gates; one confirmation per device (user decision); `em-cmd` and `em-orch` changes, including
  per-candidate effects and readiness effects; unique N2 breaks multi-M2 with an upstream agent (user decision:
  unique N2) and a new MID for M2 (user decision); E-MAC, haul-type, Network Key NUL rules; M1 routing and node
  creation ported, no controller restart difference; the confirmation carries the BSS list and the SSID check is
  ported; one renew per radio, not five; D5 under the active `dev_init`; test switch plus an eviction run (user
  decision); `serde_json` (user decision); fake radios advertise WPA2 and SAE (user decision); Context corrected
  (short attributes, row choice, reset file).
