# 0004: Runtime, concurrency and ownership rules

| | |
|---|---|
| Status | **Accepted** (2026-11-18). The core decisions are the user's. The details of timers, logging and the transport API are refined in each crate's own note. |
| Date | 2026-11-18 |
| Upstream | `rdkcentral/unified-wifi-mesh` @ `ba4180ba36748629f10c20babc4320dd69c9db4f` |
| Affects | Every crate. It is a prerequisite of `em-transport`, `em-easyconnect` and `em-core`, and replaces 0003 D4's deferral of `ThreadedTimer` and logging. |

## Context

Upstream is built on OS threads:

- Each manager (`em_mgr_t`) has a single input queue: `io_process` pushes `em_event_t`
  (`src/em/em_mgr.cpp:55-75`).
- Tick handlers run every 250 ms, 1 s, 2 s and 5 s (`handle_*_tick`, `inc/em_mgr.h`).
- Every `ThreadedTimer` spawns its own thread (`src/utils/timer.cpp`).
- State is shared through raw pointers. Examples: `em_t::m_mgr`, `em_t::m_data_model`, and the
  `em_cmd_t *m_cmd` / `em_event_t *m_evt` back-references (0001 D7).

`rdkcentral/ieee1905-rs`, the 1905 layer this port can sit on, uses tokio.

## Decision

### R1. Runtime: tokio

- Async throughout, on **tokio**.
- **Library crates** depend only on the tokio features they actually use, e.g. `sync`, `time`,
  `macros`, `net`, `io-util`. They never start a runtime.
- **Binaries** (`em-agent`, `em-ctrl`, `em-cli`) own the runtime, via `#[tokio::main]` or an
  explicit `Runtime`.
- Crates that are pure data or logic do **not** depend on tokio: `em-types`, `em-utils`,
  `em-msg`, `em-crypto`, `em-dm`. They stay synchronous and dependency-light.

### R2. No `.clone()`, anywhere

This rule is **binding for all code, including tests and examples**:

| Construct | Rule |
|---|---|
| `x.clone()`, `Clone::clone(&x)`, `T::clone(&x)` for any `T`, **including `Arc`, `Rc` and tokio `Sender`** | **Banned** |
| `clone_from` | **Banned** |
| `Iterator::cloned`, `Option::cloned`, `Result::cloned` and other `.cloned()` adapters | **Banned** |
| `.copied()` | Allowed. It is a bitwise copy of a `Copy` type. |
| Using a `Copy` value by value | Allowed |
| `to_owned()`, `to_string()`, `to_vec()`, `String::from(&str)` | Allowed **only at a real boundary where borrowed data becomes owned**, for example storing a parsed name, or building an error that owns its input. Never as a way to get a second copy of owned data to work around the borrow checker. Reviews check this. |
| `#[derive(Clone)]` | Only on types that are also `Copy` (it is required for `Copy`). Other types do not implement `Clone`. |

**Consequences for design:**

- **Ownership is moved, not duplicated.** Functions take `&T` or `&mut T`, or take `T` by value
  when they consume it.
- **No shared ownership.** There is no `Arc<T>` or `Rc<T>` to share state between tasks or
  components (see R3). `Arc` may appear only where an external API forces it on us, and even then
  it is never cloned.
- **One producer per channel sender.** Because `Sender::clone` is banned, every producer is given
  its own channel when the system is wired at startup. A consumer with several inputs merges them
  with `tokio::select!` or `tokio_stream::StreamMap`. A reply goes back on a `oneshot::Sender`
  carried inside the request.

### R3. Concurrency model: actors that own their state

- Each stateful component is an **actor**: one tokio task that owns its state outright and
  processes messages from its own inputs one at a time. This matches upstream's model of one
  input queue per manager, without the shared pointers.
- Components never share memory. They talk only through messages: `mpsc` for streams of
  requests or events, `oneshot` for replies, and `watch` for publishing the latest value of
  something such as topology. A `watch::Receiver` is not cloned either; each subscriber is given
  its own receiver at wiring time with `Sender::subscribe()`.
- Upstream's raw back-pointers become explicit messages or borrowed parameters:
  - `em_t → em_mgr_t`: an output message
  - `em_t → dm_easy_mesh_t`: a borrowed parameter or a message
  - `em_cmd_t ↔ em_t`: settled by note 0002 under these rules
- Upstream's ticks become `tokio::time::interval` timers inside the actor that owns the work, and
  are selected alongside its message inputs.

### R4. Timers replace `ThreadedTimer`

- **One-shot timers** (`execute_after`) become a `tokio::time::sleep` or `sleep_until` branch in
  the owning actor's `select!`, or a `tokio_util::time::DelayQueue` when an actor needs many keyed
  deadlines.
- **Periodic timers** (`start_periodic`, which upstream stops when the callback returns `false`)
  become a `tokio::time::interval` branch that the actor stops by leaving the branch or dropping
  the interval.
- **Callbacks become messages.** Instead of a closure running on another thread, the actor reacts
  to the timer branch firing. Nothing is shared across threads, and nothing needs `Send + 'static`
  closures holding cloned state.
- There is no `ThreadedTimer` type. The pattern is documented here, and the `em-core` and
  `em-easyconnect` notes say which deadlines each actor owns.

### R5. Logging: `tracing`

- Library crates emit events through the **`tracing`** facade (`tracing::{debug, info, warn,
  error}`, plus spans for per-node context). They never write files or set up a subscriber.
- Upstream's module split (`EM_AGENT`, `EM_CTRL`, `EM_MGR`, `EM_DB`, `EM_PROV`, `EM_CONF`,
  `EM_STDOUT`) becomes `tracing` **targets**, one per crate or module.
- **Binaries** install the subscriber. Whether to keep upstream's on-device behaviour (one log
  file per module under `/tmp`, debug level turned on by the presence of a `/nvram/<module>Dbg`
  file) is decided when `em-agent`/`em-ctrl` are ported. A `tracing-subscriber` layer can do both.

### R6. Transport shape (outline; details in the `em-transport` note)

> **Amended by [0002](0002-cmd-core-orch.md) D9 (2026-11-19):** the role actor in `em-ctrl` /
> `em-agent` owns the single frame sender and the receiver of frames. `em-core` is a library of
> handlers that the role actor calls. Read "`em-core`" below as "the role actor".
>
> **Amended by [0010](0010-vertical-slices-and-transport.md) D2 (2026-11-21):** the transport is an AL-SAP
> client of `ieee1905-rs`, run as a separate process. The raw `AF_PACKET` path is deferred.

`em-transport` is an actor, or a pair of tasks, that owns the socket: the raw `AF_PACKET` socket
or the AL-SAP UNIX sockets to `ieee1905-rs`.

- It gives `em-core` a `mpsc::Receiver` of received frames.
- It takes an `mpsc::Receiver` of frames to send, which `em-core`'s single sender feeds.
- There is no shared socket handle and no cloned senders.

### R7. Enforcement

These rules are enforced by the compiler and linter, not by memory:

- `unsafe`: `[workspace.lints.rust] unsafe_code = "forbid"`. This is already in place since
  `em-types`.
- `.clone()` and its look-alikes: a workspace-root **`clippy.toml`**:

  ```toml
  disallowed-methods = [
    { path = "core::clone::Clone::clone",     reason = "0004 R2: no .clone(); move or borrow instead" },
    { path = "core::clone::Clone::clone_from", reason = "0004 R2: no clone_from" },
    { path = "core::iter::Iterator::cloned",  reason = "0004 R2: no .cloned(); use .copied() for Copy types or borrow" },
    { path = "core::option::Option::cloned",  reason = "0004 R2: no .cloned()" },
    { path = "core::result::Result::cloned",  reason = "0004 R2: no .cloned()" },
  ]
  ```

  CI and the verification step run `cargo clippy --all-targets -- -D warnings`, so any call to
  one of the **listed methods** fails the build. This was proven by a bite test on 2026-11-18,
  on stable and on 1.85.0. Clippy rejected all nine of these with `disallowed_methods`:
  - `.clone()` on a `Copy` type, on a `String`, on an `Arc`, and on an `mpsc::Sender`
  - `Arc::clone(&a)`
  - `Iterator::cloned`, `Option::cloned` and `Result::cloned`
  - `clone_from`

  `.copied()` and `to_owned()` were not flagged. tokio's `Sender::clone` resolves to the same
  `Clone::clone` path.
- **Cases left to review** (a method denylist cannot see them):
  - a method that duplicates data under another name, e.g. a custom `fn dup(&self) -> Self`
  - a clone performed inside a library API that we call
  - `to_owned()`, `to_string()`, `to_vec()` and `Cow::into_owned()`, which R2 allows only at a
    real borrowed-to-owned boundary
  - `ToOwned::clone_into`, which can either duplicate owned data or legitimately fill a reused
    buffer

  Banning these outright would also reject legitimate ownership boundaries.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| OS threads plus `std::sync::mpsc`, mirroring upstream | User decision: async. It would also mean a second concurrency model next to `ieee1905-rs`. |
| Shared state behind `Arc<Mutex<T>>` | Banned by R2 (no `Arc` clones). It also brings back upstream's shared-pointer coupling. |
| Allowing `Arc::clone` or `Sender::clone` only | User decision: no clone of any kind. The per-producer-channel pattern (R2) covers the need. |
| `async-std` or `smol` | `ieee1905-rs` and most of the ecosystem use tokio. |

## Consequences

- `em-types` is brought into line with R2 by a follow-up commit to `9e213e1` (which was committed
  before the rule was applied to the code):
  - the `.clone()`/`.cloned()` calls in its tests are removed
  - `Clone` is removed from `InterfaceName`, `Interface`, `InterfaceNameError` and
    `MacAddrParseError`
  - `clippy.toml` is added
  - 0001 A3's list of derives is updated to match
- 0003 D4: `ThreadedTimer` and logging are now decided here (R4, R5). `em-utils` stays free of
  them.
- Note 0002 (the cmd/core/orch boundary) must be written under R2 and R3, as actors and messages
  with no shared pointers.
- `STATUS.md` lists 0004 as accepted, and lists these rules in the Approach section of the root
  `README.md`.
