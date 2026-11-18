# Porting status

Upstream base: `rdkcentral/unified-wifi-mesh` @ `ba4180ba36748629f10c20babc4320dd69c9db4f` (`develop`).

Status values: `not started` · `in progress` · `ported` (builds and has tests) · `parity verified`
(tested against upstream behaviour).

## Components

Listed in the planned porting order (bottom-up). The order can change; record why when it does.

| # | Crate | Upstream source | Depends on | Status | Notes |
|---|---|---|---|---|---|
| 1 | `em-types` | shared basics from `inc/em_base.h` (see [0001](docs/0001-em-types-scope.md) D2) | — | ported | wire codes pinned to upstream by the golden test; 31 IEEE 1905.1-only names provisional (0001 A2); rest of `em_base.h` goes to its owning crates; `ieee80211.h` deferred to em-core/em-agent |
| 2 | `em-utils` | `src/utils/` (see [0003](docs/0003-em-utils-scope.md)) | — | in progress | Step 1 (channel-table golden data) committed as work in progress; one test gap open (see follow-ups). Step 2 (crate) not started |
| 3 | `em-msg` | `src/em/em_msg.cpp`, `inc/em_msg.h` | em-types | not started | |
| 4 | `em-crypto` | `src/util_crypto/`, `src/em/crypto/` | — | not started | `util_crypto` is modified-BSD; prefer an existing crate over a port |
| 5 | `em-dm` | `src/dm/` | em-types | not started | |
| 6 | `em-db` | `src/db/` | em-dm | not started | controller only |
| 7 | `em-transport` | `src/em/em.cpp` (socket code), `src/al-sap/` | em-types | not started | raw socket + AL-SAP client |
| 8 | `em-easyconnect` | `src/em/prov/easyconnect/` | em-crypto, em-msg | not started | |
| 9 | `em-cmd` | `src/cmd/` | *provisional* | not started | order, scope and deps of rows 9–11 are provisional; design note 0002 (cmd/core/orch boundary, [0001](docs/0001-em-types-scope.md) D7) is a prerequisite |
| 10 | `em-core` | `src/em/` | em-msg, em-dm, em-transport, em-easyconnect, *provisional* | not started | see row 9 |
| 11 | `em-orch` | `src/orch/` | *provisional* | not started | see row 9 |
| 12 | `em-agent` | `src/agent/` | em-core, em-orch | not started | OneWifi bus integration |
| 13 | `em-ctrl` | `src/ctrl/` | em-core, em-orch, em-db | not started | includes TR-181 |
| 14 | `em-cli` | `src/cli/em_cli.cpp`, `src/cli/em_cmd_cli.cpp` | em-cmd | not started | Go UI out of scope for now |

## Log

| Date | Step | Result |
|---|---|---|
| 2026-11-18 | Repository and directory scaffolding | created |
| 2026-11-18 | Design note [0001: em-types scope](docs/0001-em-types-scope.md) | accepted (spec-name consts; no per-value profile tagging yet) |
| 2026-11-18 | Codex review of 0001 → revision 2 + [em_base.h coverage table](docs/em_base-coverage.md) | 15 findings, all confirmed; revision 2 proposed, em-cmd moved before em-core |
| 2026-11-18 | Codex review of revision 2 → revision 3 + [bus-event inventory](docs/bus-event-inventory.md) | 13 of 15 resolved; #2, #6 re-opened plus 6 new, all confirmed and addressed; revision 3 proposed |
| 2026-11-18 | Codex review of revision 3 → revision 4 | 5 findings, all confirmed; cmd/core/orch boundary deferred to note 0002 under binding constraints; classification file added to Step 1; revision 4 proposed |
| 2026-11-18 | Codex review of revision 4; 4 doc fixes | verdict "ready to start Step 1"; **0001 revision 4 accepted** |
| 2026-11-18 | em-types Step 1: golden data (`tools/gen-upstream-enums`, `upstream_enums.tsv`, `classification.tsv`) | 220 enumerators (210 spec, 3 vendor, 1 internal, 6 sentinel); fixture matches an independent computation; `--check` and 7 mutation tests verified. 31 IEEE 1905.1-only rows provisional (0001 A2) |
| 2026-11-18 | Codex review of Step 1 | 9 findings: 8 confirmed and fixed (generator completeness/ordering/I-O hardening + unit tests; sentinel-on-wire amendment A1; reason fixes); naming finding mostly dismissed (spec's own names) |
| 2026-11-18 | Codex re-review of Step 1 | verdict "ready to commit as provisional golden data under A2"; 6 earlier findings resolved; wording fixes to A1/A2/D3 applied; tool follow-ups recorded below |
| 2026-11-18 | em-types Step 2: crate (`9e213e1`) + no-clone follow-up | 59 tests on stable and 1.85; clippy pedantic, fmt and rustdoc clean; golden test pins all 220 enumerators; R7 bite test: clippy rejects all 9 banned clone forms; Codex: no blockers |
| 2026-11-18 | Design note [0004](docs/0004-runtime-and-ownership.md) accepted | tokio, actors, no `.clone()` (incl. `Arc`, `Rc`, senders), no `unsafe`; enforced by `clippy.toml` |
| 2026-11-18 | Design note [0003: em-utils scope](docs/0003-em-utils-scope.md), two review rounds | round 1: 9 findings, all confirmed (incl. table is 68 rows, not 66); round 2: 3 doc corrections applied; **accepted** |
| 2026-11-18 | em-utils Step 1: `tools/gen-upstream-freq-ranges`, unit tests, `upstream_freq_ranges.tsv` (work in progress) | 68 rows match an independent extraction; `--check` exit 0; 36 tests pass; bad upstream path exits 2. Bite tests: 3 of 4 caught; dropping the global `-` marker is **not** caught (follow-up). Not yet reviewed |

## Open follow-ups

| Area | Item | Origin |
|---|---|---|
| `tools/gen-upstream-enums` | Two I/O paths miss the exit-2 contract: an over-long `--out` filename is reported as "not found" (exit 1), and a scratch-directory `PermissionError` in `compile_and_run()` escapes as a traceback | Step 1 re-review #5 |
| `tools/gen-upstream-enums` | Docstring promises preprocessor lines always fail; a `#if` inside an initializer (after `=`) is not rejected. Narrow the promise or reject directive lines | Step 1 re-review #8 |
| `tools/test_gen_upstream_enums.py` | No `main()`-level tests (validate-before-write, `--no-classification`, exit-2 I/O paths; verified by hand only); some tests assert only the exit code; add a known-enum extra-row case | Step 1 re-review #10 |
| `classification.tsv` | 31 IEEE 1905.1-only rows provisional (names, references) until checked against 1905.1 | 0001 A2 |
| `em-msg` | Upstream rejects received Multi-AP Profile ≥ 4; spec Table 69 says assume own profile | 0001 A1 |
| `em-types` | Remaining `ADDITIONS` candidates absent upstream: TLV 0xDE and messages 0x8038–0x8042, 0x8048 (vBSS, spec Appendix B; not yet reviewed); media type 0x0000 (IEEE 1905.1, unverified). TLVs 0xE3 and 0xE5 were added in Step 2 | Step 1 |
| `tools/test_gen_upstream_freq_ranges.py` | **Next step.** Add a test that pins literal fixture text (e.g. a global row ends in `\t-\n`); today tests compare only against `build_fixture()` itself, so dropping the `-` marker passes. Re-run that bite test, then review Step 1 | em-utils Step 1 verification |
| `tools/gen-upstream-freq-ranges` | Low: non-atomic write (same as enums tool); region regex `re.match(...$)` accepts a trailing newline (counts backstop it); unused import in tests | em-utils Step 1 |
| `em-utils` table | Check preserved quirks (class 83 ch 1-13 vs comment "1-9", class 136 → 5937 MHz, class 1 min 34 in JP) against the IEEE op-class tables | 0003 D2 |
| `classification.tsv` | Cross-check the 31 provisional 1905.1 names against `rdkcentral/ieee1905-rs` (`ieee1905-core/src/cmdu_codec.rs`: `CMDUType`, `IEEE1905TLVType`) as a secondary source until 1905.1 itself is available | Step 2 discussion |
