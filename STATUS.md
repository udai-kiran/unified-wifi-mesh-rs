# Porting status

Upstream base: `rdkcentral/unified-wifi-mesh` @ `ba4180ba36748629f10c20babc4320dd69c9db4f` (`develop`).

Status values: `not started` · `in progress` · `ported` (builds and has tests) · `parity verified`
(tested against upstream behaviour).

## Components

Listed in the planned porting order (bottom-up). The order can change; record why when it does.

| # | Crate | Upstream source | Depends on | Status | Notes |
|---|---|---|---|---|---|
| 1 | `em-types` | shared basics from `inc/em_base.h` (see [0001](docs/0001-em-types-scope.md) D2) | — | ported | wire codes pinned to upstream by the golden test; 31 IEEE 1905.1-only names provisional (0001 A2); rest of `em_base.h` goes to its owning crates; `ieee80211.h` deferred to em-core/em-agent |
| 2 | `em-utils` | `src/utils/` (see [0003](docs/0003-em-utils-scope.md)) | — | ported | modules `chan`, `akm`, `time`, `hex`; channel table pinned to upstream by the golden test; hex dump byte-identical to upstream's loop; timestamps are UTC (intentional fix); timer and logging follow 0004 R4/R5 |
| 3 | `em-msg` | `src/em/em_msg.cpp`, `inc/em_msg.h` (see [0005](docs/0005-em-msg-scope.md)) | em-types | in progress | Steps 1 and 2 done: CMDU header, TLV framing, builder, validation against the 70 upstream tables, WSC M1/M2 selector; builder bytes match upstream's own code. TLV bodies and field readers come later, per consumer |
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
| 2026-11-18 | Review of 0001 → revision 2 + [em_base.h coverage table](docs/em_base-coverage.md) | 15 findings, all confirmed; revision 2 proposed, em-cmd moved before em-core |
| 2026-11-18 | Review of revision 2 → revision 3 + [bus-event inventory](docs/bus-event-inventory.md) | 13 of 15 resolved; #2, #6 re-opened plus 6 new, all confirmed and addressed; revision 3 proposed |
| 2026-11-18 | Review of revision 3 → revision 4 | 5 findings, all confirmed; cmd/core/orch boundary deferred to note 0002 under binding constraints; classification file added to Step 1; revision 4 proposed |
| 2026-11-18 | Review of revision 4; 4 doc fixes | verdict "ready to start Step 1"; **0001 revision 4 accepted** |
| 2026-11-18 | em-types Step 1: golden data (`tools/gen-upstream-enums`, `upstream_enums.tsv`, `classification.tsv`) | 220 enumerators (210 spec, 3 vendor, 1 internal, 6 sentinel); fixture matches an independent computation; `--check` and 7 mutation tests verified. 31 IEEE 1905.1-only rows provisional (0001 A2) |
| 2026-11-18 | Review of Step 1 | 9 findings: 8 confirmed and fixed (generator completeness/ordering/I-O hardening + unit tests; sentinel-on-wire amendment A1; reason fixes); naming finding mostly dismissed (spec's own names) |
| 2026-11-18 | Re-review of Step 1 | verdict "ready to commit as provisional golden data under A2"; 6 earlier findings resolved; wording fixes to A1/A2/D3 applied; tool follow-ups recorded below |
| 2026-11-18 | em-types Step 2: crate (`9e213e1`) + no-clone follow-up | 59 tests on stable and 1.85; clippy pedantic, fmt and rustdoc clean; golden test pins all 220 enumerators; R7 bite test: clippy rejects all 9 banned clone forms; review: no blockers |
| 2026-11-18 | Design note [0004](docs/0004-runtime-and-ownership.md) accepted | tokio, actors, no `.clone()` (incl. `Arc`, `Rc`, senders), no `unsafe`; enforced by `clippy.toml` |
| 2026-11-18 | Design note [0003: em-utils scope](docs/0003-em-utils-scope.md), two review rounds | round 1: 9 findings, all confirmed (incl. table is 68 rows, not 66); round 2: 3 doc corrections applied; **accepted** |
| 2026-11-18 | em-utils Step 1: `tools/gen-upstream-freq-ranges`, unit tests, `upstream_freq_ranges.tsv` (work in progress) | 68 rows match an independent extraction; `--check` exit 0; 36 tests pass; bad upstream path exits 2. Bite tests: 3 of 4 caught; dropping the global `-` marker is **not** caught (follow-up). Not yet reviewed |
| 2026-11-18 | em-utils Step 1 completed: literal fixture-text tests; review round 1 (6 findings) | `-` marker, header and fixture-row bite tests now caught. Fixed: block comment merged tokens (`8/**/1` read as `81`); added the D3 malformed-entry test at 68 rows. #1, #3, #5, #6 left as low (follow-ups) |
| 2026-11-18 | em-utils Step 1 review round 2, plus one fix after it | both fixes confirmed. The comment fix broke an exact-text declaration match (`= /* c */{`); the declaration is now matched with any whitespace between tokens. That last fix was verified independently, but not reviewed (two-round limit) |
| 2026-11-18 | em-utils Step 2: crate `em-utils` (`chan`, `akm`, `time`, `hex`) | 24 tests on stable and 1.85; clippy pedantic, fmt and rustdoc clean; no clones, no `unsafe`, no dependencies. `HexDump` byte-identical to upstream's loop compiled as C, for 12 inputs incl. all 256 byte values and 1000 random bytes. 13 mutation tests caught. Review round 1: 6 findings; fixed docs, a row-local forward test, and 3 forward cases. Round 2: ready to commit. #4-#6 left as low (follow-ups) |
| 2026-11-18 | Design note [0005: em-msg scope](docs/0005-em-msg-scope.md), two review rounds | round 1: 10 findings, all confirmed (strict length checks would reject upstream's own messages; WSC M1/M2 selector needed in Step 2; generator grammar; profile callers); round 2: wording and test-list fixes applied; **accepted** with the user's 3 decisions |
| 2026-11-18 | em-msg Step 1: `tools/gen-upstream-msg-tables`, tests, `upstream_msg_tables.tsv` | 70 tables, 69 message types, 179 rows, 6 empty; all 185 fixture lines match an independent extraction. Review round 1: case grammar and WSC guard fixes; round 2: WSC check still foolable by unusual code, so the WSC case is now pinned to its exact upstream text (verified independently, not reviewed: two-round limit). #3, #4 left as low |
| 2026-11-19 | em-msg Step 2: crate `em-msg` | 96 tests on stable and 1.85; clippy pedantic, fmt, rustdoc clean; only dependency em-types; no clones, no `unsafe`. Generated tables equal the fixture (independent script). Builder bytes equal 76 frames from upstream's own `add_1905_header`/`add_tlv` compiled as C++ (`tools/upstream-cmdu-vectors`). 14 of 15 mutations caught at first; a later test catches the 15th (`>=` budget check), confirmed by a rerun. Review round 1: no parity defects; 5 test gaps closed, docs updated. Round 2: two tests made order-sensitive (WSC TLV order, violation row order); each new test catches its mutation (verified independently; the last two edits not reviewed: two-round limit) |
| 2026-11-19 | Low follow-ups fixed before the em-msg commit | enums tool: exit 2 for over-long `--out` and scratch-dir errors, directives rejected inside initializers, `main()` tests (27 tests). freq-ranges tool: atomic write, `fullmatch` region, declaration found only outside literals, stricter guard and success-path tests (52). msg-tables tool: scanner skips string/char literals, `EMEX_MIN_EXT_DEV_INFO_SZ` rejected under a conditional (the include guard excepted) or `#undef` (93). em-utils: 2000/2100 leap-year and unknown-OUI tests, `HexDump` length limit documented (27). Each new test fails with its fix reverted |
| 2026-11-19 | Generator hardening: two review rounds, then design A+ (user decision) | round 1: 8 findings: 5 on unusual C syntax (fixed by joining continued lines first and rejecting raw strings, prefixed literals, digit separators, digraphs and trigraphs), 2 on temp-file handling, 1 test gap. Round 2: 4 medium edge cases that need hand-made input (two were already in the committed tools). The tools only read a fixed pinned commit, so hardening stops: enum declarations are now found only outside comments and strings, trigraphs anywhere in the header are rejected, and line joins outside `#define` lines are rejected. A+ round 2 found three more silent cases on hand-made headers (recorded below); the committed fixture matches all 220 values compiled directly from the pinned header. The rest are recorded as known limits |
| 2026-11-19 | Notes 0006 and 0007 revision 3 | round-2 findings that need no decision fixed (0007: wrong `ctrl_id.media` defect removed, call sites corrected); user decision recorded: the port fixes the upstream defects the notes list. Ownership, type classification and AP MLD questions pending the user |

## Open follow-ups

| Area | Item | Origin |
|---|---|---|
| `tools/gen-upstream-*` | Known limits (A+ decision, not C preprocessors): vertical tab or form feed inside a directive and code before or after the include guard are not checked (msg-tables); some valid but unusual C++ is rejected loudly (`<::`, a char literal right after an identifier); a failed temp-file cleanup is not reported (freq-ranges); table and switch brace masking is covered only by helper tests (msg-tables). **On any change to the pinned commit, re-extract each fixture independently and compare before trusting `--check`** | generator review round 2 (R2-02, R2-03, R2-05 to R2-07) |
| `em-easyconnect`, `em-core` | Fix, do not copy, upstream's authentication defects (user decision): compare tags and MICs over their full expected length in constant time (upstream compares 1 byte at `ec_ctrl_configurator.cpp:965`); reject a WSC M2 whose authenticator does not match (upstream's `return` is commented out, `em_configuration.cpp:4820-4823`) | note 0006 (proposed) |
| `tools/gen-upstream-enums` | Fix in progress: an enum copy disabled by `#if 0` (or on a `#define` line) before the real one is picked silently. Plausible on a pin bump | A+ review round 2, APLUS-07 |
| `tools/gen-upstream-enums` | Known limits (silent only on hand-made headers, absent from the pin): a bare CR ending a `//` comment inside an initializer; `__LINE__` in an initializer. Low: a continued `#define` after a block comment on the same line is rejected; two tests do not distinguish the raw trigraph scan and the CR splice boundary. The committed fixture matches all 220 values compiled directly from the pinned header | A+ review round 2, APLUS-06, -08, -02, -09 |
| `classification.tsv` | 31 IEEE 1905.1-only rows provisional (names, references) until checked against 1905.1 | 0001 A2 |
| `em-core` | Upstream rejects received Multi-AP Profile ≥ 4; spec Table 69 says assume own profile. Receive policy, moves to `em-core` with the Profile TLV reader | 0001 A1, 0005 D5 |
| `em-msg` | Stricter TLV length checks (every instance, optional TLVs) not added: upstream's own senders would fail them. Revisit with the spec | 0005 D3 |
| `em-types` | Remaining `ADDITIONS` candidates absent upstream: TLV 0xDE and messages 0x8038–0x8042, 0x8048 (vBSS, spec Appendix B; not yet reviewed); media type 0x0000 (IEEE 1905.1, unverified). TLVs 0xE3 and 0xE5 were added in Step 2 | Step 1 |
| `em-utils` table | Check preserved quirks (class 83 ch 1-13 vs comment "1-9", class 136 → 5937 MHz, class 1 min 34 in JP) against the IEEE op-class tables | 0003 D2 |
| `classification.tsv` | Cross-check the 31 provisional 1905.1 names against `rdkcentral/ieee1905-rs` (`ieee1905-core/src/cmdu_codec.rs`: `CMDUType`, `IEEE1905TLVType`) as a secondary source until 1905.1 itself is available | Step 2 discussion |
