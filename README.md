# unified-wifi-mesh-rs

A Rust port of [rdkcentral/unified-wifi-mesh](https://github.com/rdkcentral/unified-wifi-mesh),
the RDK Wi-Fi EasyMesh **Controller** and **Agent**.

> **Status:** `em-types` and `em-utils` are ported, `em-msg` is in progress (framing and validation done); the others have not been started. Progress is
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
- **Transport-agnostic core.** IEEE 1905 transport sits behind a trait so the core can run either
  on raw sockets or over AL-SAP to
  [rdkcentral/ieee1905-rs](https://github.com/rdkcentral/ieee1905-rs).

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
  em-transport/    1905 transport: raw socket + AL-SAP client    (src/em/em.cpp socket code, src/al-sap)
  em-easyconnect/  DPP / Wi-Fi Easy Connect                      (src/em/prov/easyconnect)
  em-cmd/          management requests, commands, IPC execution  (src/cmd)
  em-core/         protocol engine: em_t + per-area handlers     (src/em)
  em-orch/         command orchestration                         (src/orch)
  em-agent/        Agent binary + OneWifi integration            (src/agent)
  em-ctrl/         Controller binary + TR-181                    (src/ctrl)
  em-cli/          CLI backend                                   (src/cli C++ glue)
docs/              design notes and porting decisions
```

## License

Apache License 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).

This is a derivative work of `rdkcentral/unified-wifi-mesh`. Ported files are modified from
the original, and upstream copyright and attribution are kept in `NOTICE`.
