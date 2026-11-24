# 0003: Scope and design rules for `em-utils`

| | |
|---|---|
| Status | **Accepted** (2026-11-18, user), rev 2 after two review rounds |
| Date | 2026-11-18 |
| Upstream | `rdkcentral/unified-wifi-mesh` @ `ba4180ba36748629f10c20babc4320dd69c9db4f` |
| Affects | `em-utils`; follow-ups for `em-types` (`MacAddr` compact form), `em-easyconnect` and the binaries |

Note numbering: 0002 is reserved for the `em-cmd` / `em-core` / `em-orch` boundary (0001 D7).

## Context

`STATUS.md` maps `em-utils` to upstream `src/utils/`:

- `src/utils/util.cpp` (530 lines) with `inc/util.h` (516 lines), namespace `util`
- `src/utils/timer.cpp` (41 lines) with `inc/timer.h` (44 lines), class `ThreadedTimer`

These files hold no single component. They mix logging, time helpers, string helpers, a Wi-Fi
channel/frequency table, an AKM map, and a timer. Callers were counted with a text search of
upstream `src/` and `inc/`, excluding `src/utils` itself and commented-out lines.

| Upstream item | Callers | What it is |
|---|---|---|
| `frequency_ranges`, `em_chan_to_freq`, `em_freq_to_chan` (`util.cpp:309-454`) | 2 forward and 3 reverse calls: 4 in `src/em/prov/easyconnect`, 1 in `src/agent` | A table of **68** op-class rows (30 global, 11 US, 11 EU, 9 JP, 7 CN) and lookups in both directions |
| `akm_to_oui`, `oui_to_akm`, `akm_to_bytes` (`util.cpp:474-512`) | 9 calls: 7 in `em.cpp`, 2 in `em_configuration.cpp` | AKM name ↔ suite selector, e.g. `"sae"` ↔ `000FAC08` |
| `get_date_time_rfc3399` (`util.cpp:68-80`) | 7 calls in `dm`, `ctrl`, `agent`, `em` (e.g. the Timestamp TLV, `em_channel.cpp:882`) | A timestamp string |
| `print_hex_dump` (`util.cpp:214-259`) | 23 calls, all in `src/em` (the `src/agent` one is commented out) | Debug hex dump: 16 bytes per line plus an ASCII column |
| `ThreadedTimer` (`timer.cpp`, `timer.h`) | `src/em`, `ec_enrollee.h` | One-shot and periodic callbacks, each on its own thread |
| Logging: `em_util_print`, `em_printf*`, `em_util_*_print`, `EM_ASSERT_*` (`util.cpp:145-213, 263-306`, `util.h:393-514`) | everywhere | See "Upstream logging sinks" below |
| `mac_to_string`, `macstr_to_vector`, `split_nodelim_hex_str` (`util.h:199-205, 240-256, 268-289`) | 30+ sites | MAC text conversion. The compact (`""`) form is used at 8 sites: formatters (`mac_to_string`) at `em.cpp:2313, 2522, 2527`, `ec_util.cpp:597`; parsers (`macstr_to_vector`) at `ec_util.cpp:706`, `em_configuration.cpp:5365, 5416, 5443` |
| `split_by_delim`, `deref_net_uint16_to_host`, `set_net_uint16_from_host`, `SWAP_LITTLE_ENDIAN`, `add_milliseconds`, `monotonic_now`, `monotonic_cond_init`, `print_stacktrace` | various | Generic helpers (see D5) |
| `remove_whitespace` | upstream tests only (`tests/test_em_crypto.cpp`) | Test helper |
| `delay` | none found | Unused |

### Upstream defect found

`get_date_time_rfc3399` formats **local time** (`localtime()`) and appends `Z`, the UTC designator
(`strftime(..., "%Y-%m-%dT%H:%M:%SZ", localtime(...))`). On a device whose time zone is not UTC,
the timestamp is wrong by the UTC offset.

### Upstream logging sinks (for 0004 R5, not ported here)

`get_module_log_fd_name` (`util.cpp:160-212`): `EM_STDOUT` writes to stdout. For other modules, if
`LOG_PATH_PREFIX<module>Dbg` is readable (prefix defaults to `/nvram/`, `util.h:35`), every level
goes to `/tmp/<module>`. Otherwise INFO and ERROR go to `/rdklogs/logs/<module>.txt` and DEBUG is
dropped. The file is opened and closed on every call.

## Decision

### D1. What `em-utils` contains now

Only pieces with real logic that do not depend on undecided architecture. **No dependencies**,
workspace lints and edition, no `unsafe`, and no allocation in the APIs below.

| Module | Rust API (sketch) | Replaces |
|---|---|---|
| `chan` | `struct FreqRange { op_class: u8, min_chan: u8, max_chan: u8, base_freq: u16, spacing: u16, region: &'static str }` (`""` = global); `const FREQ_RANGES: &[FreqRange]`; `fn chan_to_freq(op_class: u8, chan: u8, region: &str) -> Option<u32>`; `fn freq_to_chan(freq_mhz: u32, region: &str) -> Option<(u8, u8)>` | `frequency_ranges`, `em_chan_to_freq`, `em_freq_to_chan` |
| `akm` | `fn akm_to_suite(name: &str) -> Option<[u8; 4]>`; `fn suite_to_akm(suite: [u8; 4]) -> Option<&'static str>` | `akm_to_oui`, `oui_to_akm`, `akm_to_bytes` |
| `time` | `struct Rfc3339Utc` implementing `Display`; `fn Rfc3339Utc::new(t: SystemTime) -> Option<Rfc3339Utc>` | `get_date_time_rfc3399` |
| `hex` | `struct HexDump<'a>(pub &'a [u8])` implementing `Display` | `print_hex_dump` |

`region` is a plain `&str` with `""` meaning "no region", exactly like upstream. This keeps the
matching rule literal and avoids two spellings of "no region".

### D2. Behavioural parity rules

**`chan_to_freq(op_class, chan, region)`** (`util.cpp:405-423`). For each row, in table order:

1. `c` = `chan`, or the row's `min_chan` if `chan == 0` (per row, so different rows can use
   different minimums).
2. The row matches if `row.op_class == op_class`, `row.min_chan <= c <= row.max_chan`, and
   `row.region` is `""` or equals `region` (byte-exact, case-sensitive).
3. The first matching row returns `Some(base_freq + c * spacing)`, computed in `u32`.

No match returns `None` (upstream: `-1`).

**`freq_to_chan(freq, region)`** (`util.cpp:425-454`). Keep `global = None`. For each row, in
table order:

1. Skip unless `base_freq + min_chan * spacing <= freq <= base_freq + max_chan * spacing`. This
   check comes before any subtraction, so `freq - base_freq` cannot underflow.
2. Skip unless `(freq - base_freq) % spacing == 0`.
3. `c = (freq - base_freq) / spacing`; skip unless `min_chan <= c <= max_chan`.
4. If `row.region == region` (byte-exact), return `Some((op_class, c))` at once. With
   `region == ""` this means **the first global match wins**.
5. Otherwise, if the row is global, set `global = Some((op_class, c))` (so the **last** global
   match is kept).

After the loop return `global` (upstream: a zeroed pair for "not found").

All arithmetic is in `u32`. The largest table value is 56160 + 29 × 2160 = 118800, so nothing
overflows, and `u32::MAX` simply matches no row.

**Table quirks are preserved as data, not fixed:** class 83 is `{83, 1, 13, ...}` though its
comment says "channels 1-9"; class 1 starts at 36 in US/EU/CN but 34 in JP; class 136 is
`{136, 2, 2, 5935, 1}`, so channel 2 gives 5937. Whether these agree with the IEEE tables cannot be
checked without the spec, and is recorded as a follow-up.

**`akm`** (`util.cpp:474-512`):

- 7 names map to 6 suites: `wpa-eap` 0050F201, `wpa-psk` 0050F202, `wpa2-eap` 000FAC01,
  `wpa2-psk` and `psk` 000FAC02, `sae` 000FAC08, `dpp` 506F9A02. `000FAC02` maps back to
  `"wpa2-psk"`.
- Name lookup ignores ASCII case, without allocating (`eq_ignore_ascii_case`).
- The Rust API works on bytes. Upstream's text form (uppercase 8 hex digits out, any case in) is a
  serialization detail of its callers, e.g. `akm_suites += util::akm_to_oui(...)` building JSON at
  `em.cpp:2567`. Those callers do the hex conversion at their boundary when they are ported.
- Not found: `None` (upstream: empty string or vector).

**`time`.**

- Output is `YYYY-MM-DDTHH:MM:SSZ` in **UTC**. **Intentional difference:** this fixes the
  local-time defect. Implemented without dependencies (civil date from days since the epoch).
- `Rfc3339Utc::new` returns `None` for instants before `1970-01-01T00:00:00Z` or after
  `9999-12-31T23:59:59Z` (four-digit year). Sub-second parts are truncated.
- Callers that need to own the text call `to_string()` at that boundary (allowed by 0004 R2).

**`hex`.** Byte-identical to upstream for non-empty input, assuming the C locale for `isprint`
(printable is `0x20..=0x7e`):

- each line starts with `"  %04x "` (offset, at least 4 hex digits, so offsets above `0xffff` grow)
- each byte is `" %02x"`, 16 per line
- the last line is padded with `"   "` per missing byte
- then `"  "` and the ASCII column, `.` for non-printable bytes
- each line ends with `\n`

**Intentional difference:** for an empty slice upstream prints
`buffer NULL or BAD LENGTH = 0 :\n` (`util.cpp:226-230`); Rust prints nothing. A Rust slice cannot
be null and an empty dump is not an error. Choosing the log sink is not part of `HexDump`; the
caller passes it to `tracing` (0004 R5).

### D3. Golden data for the channel table

Same approach as `em-types`, following `tools/gen-upstream-enums`.

- `tools/gen-upstream-freq-ranges` reads `src/utils/util.cpp` at the full pinned SHA with
  `git --no-replace-objects show`.
- **Extraction contract:**
  - exactly one `const std::vector<freq_range> frequency_ranges = { ... };`, else error
  - `//` and `/* */` comments are removed outside quoted strings
  - the whole body is consumed in order; every entry must be exactly
    `{int, int, int, int, int, "REGION"}`, and any other text is an error (not skipped)
  - an optional trailing comma is allowed
- **Validation before writing:** `op_class`, `min_chan`, `max_chan` fit `u8`; `base_freq`,
  `spacing` fit `u16`; `min_chan <= max_chan`; `spacing > 0`; region is `""` or two uppercase
  letters; counts are exactly **68 rows: 30 global, 11 US, 11 EU, 9 JP, 7 CN**.
- **Output:** `crates/em-utils/tests/data/upstream_freq_ranges.tsv` with a provenance header and an
  SPDX line, then columns `op_class min_chan max_chan base_freq spacing region` in table order.
  A global row has `-` in the region column.
- `--check` regenerates in memory and compares byte for byte; it never writes.
- **Exit codes:** `0` success; `1` invalid source, failed validation, stale fixture, or missing
  fixture under `--check`; `2` Git or I/O failure. On failure an existing fixture is left untouched.
- Python unit tests cover: a valid extraction, comments containing commas and semicolons, a
  trailing comma, a malformed entry in an initializer that still has 68 entries, a wrong count,
  stale `--check` (no write), and failed validation leaving an existing fixture intact.
- A Rust test asserts `FREQ_RANGES` equals the fixture row for row, in order.

The AKM map (7 entries) and the hex layout are small and are pinned by literal tests quoted from
upstream instead of by a generator.

### D4. Deferred, with a named owner

| Upstream | Owner | Notes |
|---|---|---|
| `ThreadedTimer` | Mechanism: 0004 R4 (actor-owned tokio timers, no timer type). Timing: the `em-easyconnect` note | That note must pin upstream's observable timing: periodic timers sleep **before** the first callback and again **after** each callback finishes, so the gap between starts is the interval plus the callback's run time (fixed delay, not fixed rate: `timer.cpp:24-31`); stop-on-`false` (upstream's `stop()` from the timer thread tries to join itself, so only the intent is kept); the enrollee's immediate first search, 1 s retries cancelled on response, and 10 s authentication deadline (`ec_enrollee.cpp:116, 2098`) |
| Logging | 0004 R5 (`tracing` in libraries) | The binary ports decide stdout for `EM_STDOUT`, the `/tmp` vs `/rdklogs/logs` files, and the readable-marker debug toggle described above |
| Compact MAC text | `em-types`, as a compact `Display` adapter on `MacAddr`, added with the first caller (`em-msg` or `em-easyconnect`) | Colon form and compact parsing already exist |

`em-easyconnect` will depend on `em-utils` for the channel lookups.

### D5. Not ported, with where the behaviour goes

| Upstream | Where it goes |
|---|---|
| `split_by_delim` | Caller ports. Note: `std::getline` drops the empty item after a trailing delimiter (`"a,b,"` → 2 items), unlike `str::split`. The EasyConnect URI parser checks token counts (`ec_util.cpp:768`), so `em-easyconnect` must match this. |
| `EM_ASSERT_*` | Caller ports: an error return (`?`) plus a `tracing` event; the `_FREE` forms become RAII (`Drop`). |
| `print_stacktrace` | Caller and logging ports (stderr backtrace before two missing-manager errors, `em.cpp:67`). |
| `deref_net_uint16_to_host`, `set_net_uint16_from_host`, `SWAP_LITTLE_ENDIAN` | The codecs that need them, using `u16::from_be_bytes` etc. |
| `add_milliseconds`, `monotonic_now`, `monotonic_cond_init` | Runtime code (`std::time::Instant`, tokio time). |
| `remove_whitespace` | Upstream tests only; ported tests do their own cleanup. |
| `delay` | Unused. |

## Consequences

- `em-utils` is small and has no dependencies. `STATUS.md` changes its "Depends on" from
  `em-types` to `—`.
- `em-utils` follows 0004 R1 (no tokio) and R2 (no clones in code or tests, `Clone` only on `Copy`
  types).
- Follow-ups recorded in `STATUS.md`: compact MAC formatting (em-types); the `em-easyconnect` timing
  and `split_by_delim` items in D4/D5; checking the table quirks against the IEEE op-class tables.

## Acceptance criteria

1. `crates/em-utils` is a workspace member with no dependencies. These pass on stable and on
   1.85.0:
   - `cargo build --all-targets`
   - `cargo test --all-targets`
   - `cargo clippy --all-targets -- -D warnings` (includes the 0004 R7 `clippy.toml` clone ban)
   - `cargo fmt --check`
   - `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`
2. `tools/gen-upstream-freq-ranges --check` reproduces the fixture, and its unit tests (D3) pass.
3. Rust tests:
   - `FREQ_RANGES` equals the fixture; a bite test shows changing one row fails it
   - forward: every row's min and max channel via a row-local check; class 83 ch 13 → 2472; class 1
     ch 0 → 5180 with `"US"` and 5170 with `"JP"`; class 136 ch 2 → 5937; unknown class → `None`
   - reverse: 2412 with `""` → (81, 1), with `"US"` → (12, 1), with `"us"` or `"XX"` → (83, 1);
     5170 with `""` → `None`; 2413 → `None`; `u32::MAX` → `None`
4. AKM: every mapping in both directions, mixed-case names, the `"psk"` alias, unknown names and
   suites.
5. `Rfc3339Utc`: the epoch, a leap day (`2024-02-29`), a year end, a fixed recent instant,
   sub-second truncation, 0.5 s before the epoch → `None`, `9999-12-31T23:59:59Z` accepted and one
   second later → `None`.
6. `HexDump`: lengths 1, 15, 16, 17 and 32; bytes `0x1f`, `0x20`, `0x7e`, `0x7f`, `0xff`; and an
   offset above `0xffff` (a 65552-byte input, checking its last line); all against the upstream
   text. Length 0 prints nothing (the intentional difference in D2).
7. Review checks R2 cases a lint cannot see: manual `Clone` impls and `to_owned`/`to_string` use.
8. `STATUS.md`, `crates/em-utils/README.md` and the root `README.md` are updated.

## Open questions

1. **Resolved (2026-11-18, user):** `chan` lives in `em-utils`. It is behaviour (a lookup), not a
   shared vocabulary type.
2. **Resolved (2026-11-18, user):** note 0004 was written first: async on tokio, no `.clone()`
   (including `Arc`), no `unsafe`.

## Errata

- **E1 (2026-11-21, Codex audit of `em-utils`):** two upstream cites at `ba4180b` are corrected here; the text
  above is left as written.
  - D2, `hex`: upstream's empty-buffer branch is `util.cpp:230-234`, and the message is printed at `:231`
    (the note says `:226-230`).
  - D5, `split_by_delim`: the EasyConnect URI parser checks its token counts at `ec_util.cpp:771` (at least
    one part) and `:779` (each part is exactly a key and a value). The note's `:768` only trims the URI.
