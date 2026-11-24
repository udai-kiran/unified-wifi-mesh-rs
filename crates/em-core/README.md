# em-core

Synchronous protocol engines and node state for the EasyMesh port. S1 implements
AP-Autoconfiguration Search and Response (note [0012](../../docs/0012-s1-discovery.md)
D1–D3, D5–D6 and D10). S2 adds the WSC M1/M2 engine, the Renew builder and reader, and the
topology messages (note [0013](../../docs/0013-s2-onboarding.md) D4, D6 and D7). The role actor owns orchestration, time, transport and
session stamping; this crate does not start tasks or open sockets.

## S1 interface

- `Node<S>` carries a role state, its key, local profile and optional radio band.
  Agent radios record the controller AL MAC and effective profile after a
  Response. `NodeTable<S>` keeps insertion order and distinguishes AL and radio
  keys even when their MACs match.
- `AgentState` and `CtrlState` use the relevant upstream `em_state_t` values.
  `MsgIdCounter` is owned once per process, starts at 1 and skips 0 on wrap.
- `EngineCtx<'_, S>` borrows the node table, node key, `CommandView`, counter,
  invocation time and ordered `Outputs`. `Output::Frame` contains a remote MAC
  and CMDU without an Ethernet header; `Output::Progress` identifies a state
  change. The role actor adds its local AL MAC and current session.
- `configuration` builds Searches and Responses, exposes routing helpers, gates
  agent proto ticks through the active `dev_init`, resends Searches with fresh
  IDs, and handles valid routed messages. It validates received CMDUs inside the
  handler, after routing. The controller answers each valid Search using its
  current profile and MID. The agent accepts a Response with any MID while
  pending, records the peer and moves the radio to `wsc_m2_pending`, where the agent core sends M1
  (0013 D4). DPP Chirps, the Early AP Capability Report, model commits, transport and timer
  orchestration belong to later work.

The controller's 1905 Layer Security Capability is all zero in S1. Its
Controller Capability requests Early AP Capability and sets the KiB/MiB bit;
receiving that bit does not implement counter scaling (S5). The Response
SupportedService is `{controller, EM+ controller}` for an EM+ agent and
`{controller}` for other peers, as upstream (`em_configuration.cpp:4416-4432`).
The handler logs a self-validation failure on its Response and still hands it
off, as upstream.
A node's state changes once its frame is handed to `Outputs`, since AL-SAP has
no per-frame completion (0012 D9). This differs from upstream
`em_configuration.cpp:6471-6478,6727-6733`.

## S2 WSC and topology interface

`wsc::build_m1` builds the agent M1 from `M1Params`, a process-wide
`MsgIdCounter` and a fallible `RandomSource`. It installs a `Session` for that
radio only after construction succeeds. `wsc::accept_m2` checks the radio state,
controller AL MAC and RUID and returns `BssConfig` values for the platform.
The session is dropped on acceptance; the caller must clear it on timeout,
renew, radio removal and stop. A rejected M2 leaves the exchange available.
The record uses `em-crypto`'s zeroizing DH, nonce, M1 and derived-key containers;
the returned network keys are zeroizing as well.

`wsc::parse_m1` exposes the controller routing RUID, advertised maximum BSS
count, band, enrollee MAC, authentication flags and capped AKMs. The role core
must check that the sender has an existing peer model and apply the M1 state
routing in 0013 D7. `wsc::build_m2` takes borrowed `NetworkSsid` views in
NetworkSSIDList order. `authentication_type` and `network_band` convert the
upstream reset-file fields; `haul_types` are the `em-dm` haul enum values. The
borrowed view avoids another dependency and does not copy passphrases. Apply
`M2Output::post_handoff_effects` only after the M2 is successfully handed to
transport, in the returned order.

`topology::build_query`, `topology::build_response`, `topology::handle_query`
and `topology::parse_response` cover the S2 topology messages. The Query gate
uses one state for each supplied radio, in the same order. The response includes
Device Information, Supported Service, AP Operational BSS, Multi-AP Profile,
BSS Configuration Report, empty AP MLD Configuration and TID-to-Link Mapping,
and upstream's Vendor Operational BSS. It derives Device Information media bytes from the supplied BSS interface MAC,
band and VAP mode, as upstream does. Parsing reports `SsidMismatch` distinctly; state
changes and the 120 s mismatch path belong to the role cores.

`configuration::build_renew` builds the controller's Renew, and `configuration::route_renew`
reads a Renew's band and AL MAC for the agent, validated at profile 2.

D6 differences from `em_configuration.cpp` are tested with
`intentional-difference` tags: AL E-MAC (`:3346-3353`), WPA2/SAE flags
(`:3376-3415`), initialized short attributes (`:3537-3563`), filled capability
TLVs (`:4319-4333`), new M2 MID (`:4177-4182`), unique N2 per TLV (`:2970-2976`),
filled M2 flags (`:2995-3030`), authentication negotiation (`:5884-5893`),
unterminated Network Key (`:5963-5967`), searching enabled rows
(`dm_easy_mesh.cpp:2853-2874`), a Tear Down M2 (`:4223-4227`), bounded parsing
(`:4605-4629,4668-4755`), authenticator rejection (`:4820-4823`), Key Wrap
Authenticator checking (`:4941-4943`), Multi-AP Extension reading
(`:4909-4943`), capped AKMs (`:4701-4711`), and separate enrollee/registrar DH
keys (`em_crypto.cpp:210-234`). Each M2's Authenticator is checked before CBC
and its Key Wrap Authenticator before interpreting settings. Padding and key
wrap failures return the same `RejectedSettings` error. An upstream-style shared
N2 and a final NUL in a Network Key are accepted. With unique N2s, the recorded
multi-M2 vector fails with upstream's last-key-for-all decryption.

## Open points

- D10 writes `EngineCtx<'a>` while the same interface requires role-specific
  `NodeTable<S>`; the context is generic over `S` so the table retains its
  role-specific state type without type erasure. Its fields follow D10.
- D10 requires insertion order but does not specify duplicate insertion:
  inserting an existing key replaces the node in place and returns the old one;
  removal keeps the remaining order.
- Upstream's profile reader accepts one or four octets. Search routing preserves
  values 0–3 and uses 1 when the profile is missing, malformed or 4–255. The
  Search handler still uses 1 when the profile is missing or malformed; for a
  reserved value in an accepted length, it assumes the controller's profile 3.
  The Response handler rejects malformed profile lengths and validation rejects
  an absent Profile TLV. A reserved value in an accepted length uses the agent's
  own profile. These rules follow 0012 D3 and its rev 7 erratum.
- Controller Search routing and agent Response routing follow upstream
  `get_freq_band`, accepting the first Supported or Autoconfig Frequency Band
  TLV before message validation (`em_msg.cpp:346-363`,
  `em_agent.cpp:2367-2371`). A Search's SupportedService identifies an EM+
  agent for the Response service list (`em_configuration.cpp:4416-4432`);
  malformed optional SupportedService is treated as absent, matching upstream's
  failed read.
- `Progress` is emitted on a state change; a resend or repeated Search Response
  produces a frame without an additional state-change event.

- S2 inputs do not specify WSC configuration methods; the engine advertises
  `0x0080` (push button) and AES, and accepts caller-supplied identity and radio
  capability fields. The 6 GHz selection applies upstream's WPA2/open/transition
  to SAE conversion before checking the M1 flags (`em_configuration.cpp:5884-5893`).
- `NetworkSsid` is a borrowed adapter rather than a direct `em-dm` record because
  this brief permits only the `em-crypto` dependency. Unknown `AuthType` strings
  follow upstream's SAE fallback (`em_configuration.cpp:6018-6025`); unknown band
  strings have no matching radio. A zero advertised BSS maximum cannot carry
  even the Tear Down M2, so `build_m2` returns `NoBssCapacity`.
- The current upstream AKM mapping produces fewer than ten distinct names; the
  ten-entry truncation and warning are tested with a longer input at the mapping
  boundary. A zero-flag upstream M1 parses, but no configured authentication is
  advertised, so the controller emits Tear Down rather than guessing support.
- Topology Response fields the fake platform cannot supply (MLD and mapping
  policy) are empty; Device Information uses upstream's fixed 802.11ax media
  type and band mapping (`em_configuration.cpp:6786-6825`). BSS report flags are deterministic zeros where the upstream sender
  leaves those fields unwritten (`em_configuration.cpp:1139-1155`). A response
  with no local interfaces remains subject to `em-msg`'s upstream-derived
  Device Information minimum (19 bytes including TLV header).
- Lifecycle tests exercise accepted, rejected, timeout, renew, removal and stop
  paths and replacement. Safe Rust cannot inspect memory after it has been freed;
  the `em-crypto` secret tests check the `ZeroizeOnDrop` contracts separately.

## Upstream source

- `src/em/em.cpp`, `src/em/em_mgr.cpp`, `src/em/config/em_configuration.cpp`
- `src/agent/em_agent.cpp`, `src/ctrl/em_ctrl.cpp`, `inc/em_base.h`

Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.
