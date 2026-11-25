# em-event-loop

General-purpose turn-by-turn scheduler core in the style of libae. It only answers "what should be done next?"
(`LoopCore::next_turn`): serve a source, run a handler's deadline, or the pass is done.

**Status:** Rust written and proved (refinement to the frozen step spec, note 0017 D6 step 3); the gates decide from Lean's own checks (note 0017 A1); the async driver `run` (over the `Io` and `App` traits, tested with a fake `Io`) is done (note 0016 D8 step 4a), with `Io::yield_now` for a source that stays ready (0016 E11); the tokio `Io` for the transport is in `em-transport` (step 4b-3).

**The proofs are frozen (note 0017 A2).** `verify/` records what was proved for `LoopCore` at commit `a286628`. It is not part of
the standard checks and is not maintained. While the `LoopCore` code is unchanged (re-extraction gives byte-identical `verify/lean/Extracted/` files; checked
after the driver was added, which sits at the end of `src/lib.rs` and is excluded by `verify/extract.sh`) the proofs still hold. If a change to `LoopCore`
breaks them, delete the proof files (everything under `verify/` except `spec-en.md`) in that change instead of repairing them;
`spec-en.md` stays the contract, and a rule change needs the user's approval. The Rust tests are the safety net.

## Owns

- Sources (ready flag, pause flag, per-pass budget) and handlers (one deadline, pause flag), numbered from 0.
- One pass: sources in index order, then every due deadline once, then `PassDone`.
- `next_deadline` and `has_ready_source` queries.
- The async driver (`driver.rs`): `run`, `Io`, `App`. It runs passes, performs a pass's external actions in the order produced, delivers events, and waits only when no source is ready. When a source is still ready after a pass, it calls `Io::yield_now` (default: returns at once) and delivers the events that arrived during it before the next pass, so other tasks on the thread run (driver rule, note 0016 D2, E3, E11, U14). No dependencies, no tokio; the `wait` future is `Send`.

## Must not own

- Any EasyMesh type, any I/O or tokio, logging.
- Queue sizes, drop rules, timer values, the EasyMesh source table (application policy, note 0016 U13, U15).
- A real `Io` (tokio, sockets, clocks): each application or crate supplies its own.

## Rules for the code

Ids that do not exist panic (S15). No `unsafe`, no `.clone()`, no dependencies. The code follows the Aeneas-friendly
style of note 0016 erratum E8 (index `while` loops, small `bool` helpers, guarded counters) because it was extracted to
Lean for the proofs. `LoopCore` keeps this style (its proofs are frozen, not dropped); since 0017 A2 it is not
required for code outside the proofs, such as the driver.

## Verification

- Rust: `cargo test -p em-event-loop --locked` (44 unit tests, 9 of them for the driver), on stable and on 1.85.0.
- Proofs and gates, from the container image `aeneas-spike:latest` (needs Docker and its two volumes):
  `verify/reproduce.sh` runs them in order and exits 0 only if every gate passes: extraction twice with a
  byte comparison, the Rust tests on nightly-2026-09-17, the file checks (`check-modules.py`),
  `lake build --wfail` from an empty build directory (this also runs the axiom audit), the frozen digests,
  the negative fixtures, and the planted bugs in the Rust. Each gate's exit code is its verdict. The run
  works in one `mktemp -d` folder under `/cache/em-event-loop-runs/`; at the end its bulky build folders are
  deleted and the logs and tables are kept. The cargo checks (stable and 1.85.0 tests, clippy, fmt, rustdoc)
  are not part of `reproduce.sh`.
- `SKIP_G3=1 verify/reproduce.sh` skips the planted bugs. `MUTANT_JOBS=n` sets how many run at once
  (default 4). With 4 at a time they took about 3 to 5 minutes (about 6.5 minutes one at a time); a full run
  took 301 seconds in the last check.
- Axiom audit: `lean/AuditAxioms.lean` reads the elaborated environment (no text parsing). It allows only
  `propext`, `Classical.choice` and `Quot.sound`, plus the `native_decide` axiom
  (`<decl>._native.native_decide.ax_*`) for theorems of `ExtractedExamples` and, on the planted-bug path only,
  the generated counter-example module. It fails on `sorryAx` and on any user `axiom`, and it checks the exact
  number of theorems per module. The last count was 384 theorems: 355 in 13 modules and 29 in `PlantedBugs`.
- Frozen values (checked by `reproduce.sh`): the `spec-en.md` sha256, the statement digest
  `41 856189881b75665f04102a46eedbe675ee7db6456c412785797d330f43106358` (`lean/check-statements.py`: the step
  spec and the rule statements), and the refinement digest
  `refinement 29 478acf91f57efc22b0a631e1add9447fc5ca938f7a060a6a2046514b5f59c0eb` (`Abstraction.lean` and
  the 29 refinement statements, printed from the elaborated types). The refinement statements were frozen
  after a blind back-translation that the user accepted on 2026-11-24. Since 0017 A2 the statements are not
  changed: if a `LoopCore` change breaks the proofs, the proof files are deleted (see the frozen-proofs note above).
- Lean commands run through `verify/proofs.sh` (for example `verify/proofs.sh lake env lean CounterExamples.lean`),
  which works on a copy so that no `.lake` folder is written into the repository.

Files under `verify/`:

- `spec-en.md`: the English spec, frozen (sha256 `64085e5b…`).
- `lean/`: the Lean step spec and rules (`StepSpec.lean`, `Rules.lean`, and the checks built on them), the
  refinement to the extracted code (`Abstraction.lean`, `Refinement.lean`, `Transfer.lean`, `ExtractedRun.lean`,
  `ExtractedExamples.lean`), the examples and counter-examples (`Examples.lean`, `CounterExamples.lean`,
  `PlantedBugs.lean`), and `rule-map.md` (each rule to its theorems). `Extracted/` is generated by
  `extract.sh` and is never edited by hand. `lakefile.lean` makes every `.lean` file a root.
- The audit and its checks in `lean/`: `AuditAxioms.lean` (the checker), `AuditMain.lean` and
  `AuditPlanted.lean` (the two drivers; `PlantedBugs` is audited alone because it cannot share an environment
  with `Examples`), `RefinementStatements.lean` (prints the refinement statements for the second digest), and
  `check-statements.py` (the first digest).
- `extract.sh` (Charon and Aeneas), `run.sh` (runs a command in the container), `proofs.sh` (Lean commands on a
  copy), `rust-tests.sh`, `reproduce.sh` (the gates), `check-modules.py` (every Lean file is a root, and every
  project module is audited; no `set_option debug.skipKernelTC` or `@[implemented_by` line), `check-fixtures.sh` with `fixtures/`
  (bad files that the gate must reject), and `refinement-digest.sh`.
- `mutants/`: the planted bugs (`mutate.py`) and their runner (`run-all.sh`, `run-mutant.sh`, `lean-steps.sh`);
  `verdict.py` is the one function that decides whether a bug is caught; `report.py` writes `g3-code-layer.md`;
  `check-negatives.sh` checks that injected failures make a bug fail the gate.

## Links

- English spec (rules S1 to S17): [`verify/spec-en.md`](verify/spec-en.md)
- Lean step spec: [`verify/lean/StepSpec.lean`](verify/lean/StepSpec.lean)
- [`docs/0016-sans-io-cores.md`](../../docs/0016-sans-io-cores.md) and [`docs/0017-verification-workflow.md`](../../docs/0017-verification-workflow.md).
