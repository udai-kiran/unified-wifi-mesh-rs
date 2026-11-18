# Porting status

Upstream base: `rdkcentral/unified-wifi-mesh` @ `ba4180ba36748629f10c20babc4320dd69c9db4f` (`develop`).

Status values: `not started` · `in progress` · `ported` (builds and has tests) · `parity verified`
(tested against upstream behaviour).

## Components

Listed in the planned porting order (bottom-up). The order can change; record why when it does.

| # | Crate | Upstream source | Depends on | Status | Notes |
|---|---|---|---|---|---|
| 1 | `em-types` | shared basics from `inc/em_base.h` (see [0001](docs/0001-em-types-scope.md) D2) | — | not started | rest of `em_base.h` goes to its owning crates; `ieee80211.h` deferred to em-core/em-agent |
| 2 | `em-utils` | `src/utils/` | em-types | not started | |
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
