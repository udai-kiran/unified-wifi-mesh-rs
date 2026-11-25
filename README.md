# unified-wifi-mesh-rs

A Rust port of [rdkcentral/unified-wifi-mesh](https://github.com/rdkcentral/unified-wifi-mesh),
the RDK Wi-Fi EasyMesh **Controller** and **Agent**.

> **Status:** `em-types`, `em-utils`, `em-crypto` and `em-msg` are ported (`em-msg`: framing, validation, all TLV bodies, the typed WSC attributes and the field readers); `em-dm` is ported (records, enums, models and operations done); `em-transport` is done as slice S0 (AL-SAP client and Docker harness; review pending); S1 (controller discovery) and S2 (onboarding through WSC M1/M2 to `topo_synchronized`) are implemented with minimal `em-cmd`, `em-orch`, `em-easyconnect`, `em-core`, `em-db`, `em-agent` and `em-ctrl`; `em-event-loop` has its pure core written and proved, and its async driver written; the transport's `TransportCore`, `TransportHandler` and tokio `Io` run on the loop (0016 D8 step 4b-3; the actor stays until `em-agent` moves, step 4c); the others have not been started. Progress is
> tracked in [STATUS.md](STATUS.md).

## Porting base

| | |
|---|---|
| Upstream | `rdkcentral/unified-wifi-mesh` |
| Branch | `develop` |
| Commit | `ba4180ba36748629f10c20babc4320dd69c9db4f` |

Every component README names the upstream files it ports, at this commit.

## Approach

- **Component by component, bottom-up.** Leaf components (types, utilities, the CMDU/TLV codec)
  are ported first so each one can be built and tested in isolation before anything depends on it.
- **One Cargo crate per component** under `crates/`. A crate gets its `Cargo.toml` only when its
  port starts. Until then the directory holds a README describing its scope.
- **Behavioural parity first.** Each crate is tested against the upstream behaviour it replaces.
  Redesign is a separate, explicit step.
- **Hard rules** ([docs/0004](docs/0004-runtime-and-ownership.md)), enforced by the compiler and
  clippy:
  - no `unsafe`
  - no `.clone()` anywhere, tests included, and that covers `Arc`, `Rc` and channel senders.
    Ownership is moved or borrowed instead.
- **Async on tokio, as actors.** Each stateful component is a task that owns its state and talks
  to the others only through channels. Pure data and logic crates stay synchronous.
- **AL-SAP transport.** The 1905 layer is [rdkcentral/ieee1905-rs](https://github.com/rdkcentral/ieee1905-rs),
  run as a separate process; the port talks to it over AL-SAP (docs/0010 D2). A raw-socket backend is
  deferred.

## Layout

```
crates/
  em-types/        MacAddr, InterfaceName, Interface; wire codes MsgType, TlvType, FreqBand,
                   MediaType, ProfileType, ServiceType   (part of inc/em_base.h, see docs/0001)
  em-utils/        channel table, AKM map, UTC timestamps, hex dump  (src/utils, see docs/0003)
  em-msg/          CMDU / TLV encode, decode, validation         (src/em/em_msg.cpp)
  em-crypto/       1905 / AES-SIV / key-derivation crypto        (src/util_crypto, src/em/crypto)
  em-dm/           EasyMesh data model                           (src/dm)
  em-db/           SQLite persistence (controller)               (src/db)
  em-transport/    AL-SAP client to ieee1905-rs                  (src/al-sap, see docs/0011)
  em-easyconnect/  DPP / Wi-Fi Easy Connect                      (src/em/prov/easyconnect)
  em-cmd/          management requests, commands, IPC execution  (src/cmd)
  em-core/         protocol engine: em_t + per-area handlers     (src/em)
  em-orch/         command orchestration                         (src/orch)
  em-agent/        Agent binary + OneWifi integration            (src/agent)
  em-ctrl/         Controller binary + TR-181                    (src/ctrl)
  em-cli/          CLI backend                                   (src/cli C++ glue)
docs/              design notes and porting decisions
```

The slice crates `em-easyconnect`, `em-cmd`, `em-orch`, `em-core`, `em-db`, `em-agent` and `em-ctrl`
implement only what controller discovery and onboarding need (notes [0012](docs/0012-s1-discovery.md) D1,
[0013](docs/0013-s2-onboarding.md) D1). The rest of each upstream source is not ported yet; `em-db` holds only
the reset-file loader.

## License

Apache License 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).

This is a derivative work of `rdkcentral/unified-wifi-mesh`. Ported files are modified from
the original, and upstream copyright and attribution are kept in `NOTICE`.
