# em-core

Synchronous protocol engines and node state for the EasyMesh port. S1 implements
AP-Autoconfiguration Search and Response (note [0012](../../docs/0012-s1-discovery.md)
D1–D3, D5–D6 and D10). The role actor owns orchestration, time, transport and
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
  pending, records the peer, and stops before WSC M1. DPP Chirps, M1/M2, the
  Early AP Capability Report, model commits, transport and timer orchestration
  belong to later work.

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

## Upstream source

- `src/em/em.cpp`, `src/em/em_mgr.cpp`, `src/em/config/em_configuration.cpp`
- `src/agent/em_agent.cpp`, `src/ctrl/em_ctrl.cpp`, `inc/em_base.h`

Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.
