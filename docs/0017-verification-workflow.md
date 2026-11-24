# 0017: Verification workflow: an English contract, checked by Lean proofs

| | |
|---|---|
| Status | **Accepted** (2026-11-23, user), rev 2, after the trial on the spike's `LoopCore`. User decisions V1 to V9. Amended by A1 and A2 (2026-11-24). **Since A2, proofs only for `LoopCore` and the `Reassembler`; spec and tests for the rest** |
| Date | 2026-11-23 |
| Follows | [0016](0016-sans-io-cores.md) D7, D8, errata E1 to E9; the Aeneas spike (tracking folder `aeneas-spike/report.md`) |
| Amends | 0016 D7 (how verification is done and reviewed), D8 step 3 (starts with an approved English spec) |
| Affects | every crate or module chosen as a **proven core** (D1); the review set for those (D5) |

## Context

The Aeneas spike proved 39 theorems about two pure cores (the `Reassembler` and a `LoopCore` prototype). Proofs check
the code against **statements**, and a statement can be true and still say less than intended. The user does not read
Lean, so a Lean statement cannot be the contract. This note makes an **English spec** the contract and defines the
evidence that the Lean statements say what the English says.

## Decision

### D1. Proven cores

A **proven core** is a pure, synchronous module or crate under the 0016 core rule (no clock, randomness, I/O, tokio)
that the user names for proof. First ones: `em-event-loop`, and in `em-transport` the `Reassembler`, the received queue
and the size cap (0016 U1, U2, U32 to U34). Every other crate keeps today's process.

### D2. The English spec is the contract (V1)

Each proven core has `spec-en.md`, written in plain English:

- **Words used:** the terms, defined once.
- **Rules S1, S2, …:** one sentence each, saying what must always hold.
- **Examples** (must happen) and **counter-examples** (must never happen) under each rule, as concrete runs.
- **Assumptions** the proofs may use (for example "comparing two times always answers").

The user approves the spec before code or proofs are written. It is frozen by hash; a change needs the user's approval
and a new hash.

### D3. The Lean side

- A **step spec**: a short Lean description of what one call does (as `NextSpec` in the spike). The rule theorems are
  proved **from the step spec alone**: over any step function that meets it, not over the extracted code. So they do not
  depend on the code's shape, and a bug planted in the step spec tests the rules directly (G3, rule layer). The spike
  proved its rules through the code; the trial showed why that is not enough.
- A **refinement theorem**: the code, extracted by Charon and Aeneas, does exactly what the step spec says, and never
  fails (as `next_turn_spec`).
- **One theorem per rule** (at least), proved with no `sorry` and only Lean's standard axioms.
- When the code changes but the behaviour does not, only the refinement proof is redone. When the behaviour changes, the
  English spec changes first (D2).
- Rust style for proven code: 0016 E8.

### D4. The evidence the user approves (V2)

| Gate | What | How |
|---|---|---|
| **G1 back-translation** | The Lean statements say what the English says | Codex reads only the Lean (comments and docstrings removed, main theorems renamed to neutral labels) and writes in plain English what each theorem guarantees, its assumptions and its weak spots. The coordinator puts it next to the English rule; differences are fixed or explained |
| **G2 examples** | The rules mean what the examples show | Every example runs on the extracted code and gives exactly the stated answers (kernel-checked where possible; otherwise `native_decide`, which trusts Lean's compiled evaluator, allowed for examples only); every counter-example is shown impossible by the matching theorem, kernel-checked with standard axioms only. A counter-example the theorems do not rule out is a finding |
| **G3 planted bugs** | The proofs track the code, and the rules are strong enough | Two layers. **Code layer:** at least one bug per rule planted in the Rust; each must make the refinement proof fail. **Rule layer:** the same bugs planted in the step spec; each must make at least one rule theorem fail. A step-spec bug that no rule catches means the English allows that behaviour (the user decides) or a rule is missing. The table also shows whether the Rust tests catch each bug |
| **G4 build** | The proofs are real | Re-extraction byte-identical; `lake build` from an empty build directory exits 0; `#print axioms` per theorem shows only `propext`, `Classical.choice`, `Quot.sound`; no `sorry` |
| **G5 frozen** | Nobody weakened the contract | Hashes of `spec-en.md` and of the Lean statement files match the approved ones |

The user approves G1 to G3 as tables in English; G4 and G5 are pass/fail.

### D5. Review set for proven cores (V3)

- The **Codex Lean verifier** is the mandatory Codex review, briefed for proven cores: it runs or checks G1, G4 and G5,
  and reviews what proofs cannot see: the I/O shell around the core, project rules (no clone, MSRV 1.85, docs,
  `STATUS.md`), upstream parity cites and `intentional-difference` tests.
- **Correctness and adversarial reviewers** are added only for unproven code in a sensitive area (0016's shell, the
  driver, sockets).
- The "test fails without the fix" experiment is replaced by G3 for proven cores.
- The contrarian is unchanged: it is used only when a plan fails twice.
- Unproven crates keep today's review set.

### D6. Order for a proven core

1. English spec drafted and approved (G5 hash recorded).
2. Lean step spec and rule statements written; G1 run; mismatches fixed until the user accepts the G1 table.
3. Rust written; extracted; refinement and rule proofs; G2, G3, G4.
4. Codex Lean verifier (D5); the user approves G1 to G3.
5. Later changes: code changes re-run G2 to G5 and the refinement proof; spec changes restart at step 1 for that rule.

## Alternatives considered

| Alternative | Why not chosen |
|---|---|
| Lean statements as the contract | The user does not read Lean |
| Keep the full review set on proven cores | Proofs cover the core's logic for every input; the reviewers' time is better spent on the shell and the statements |
| Proofs on the code only, no step spec | Every code change would redo every property proof |
| Back-translation by the same agent that wrote the Lean | Not independent; it would repeat its own reading |

## Acceptance criteria (for the first proven core, `em-event-loop`)

1. `spec-en.md` approved by the user and hashed.
2. G1 table with no unexplained difference; G2 all examples pass and all counter-examples ruled out; G3 every planted
   bug caught; G4 and G5 pass.
3. The Codex Lean verifier reports no confirmed problem.
4. The standard Rust checks pass on stable and 1.85.0, and on the Aeneas nightly for the extracted crate.

## User decisions

| # | Date | Decision |
|---|---|---|
| V1 | 2026-11-23 | The contract is an English spec with examples; the user does not read Lean |
| V2 | 2026-11-23 | The evidence is back-translation, examples, planted bugs (plus build and freeze checks) |
| V3 | 2026-11-23 | Proven cores: the Codex Lean verifier, plus correctness and adversarial reviewers only for unproven sensitive code |
| V4 | 2026-11-23 | The workflow is recorded in this note |
| V5 | 2026-11-23 | Trial run on the spike's `LoopCore` first |
| V6 | 2026-11-23 | C1: proof files live inside each proven crate, `crates/<name>/verify/` (spec, Lean, extract script) |
| V7 | 2026-11-23 | C2: the Lean checks run locally in the Docker toolchain until 0016 C11 is decided |
| V8 | 2026-11-23 | C3: a Claude or Codex implementer writes the proofs, chosen per brief |
| V9 | 2026-11-24 | A2: the full proof workflow is too much for a hobby project. Proofs only for the very core: `LoopCore` (frozen) and the `Reassembler`; spec and tests for the rest; lighter review set |

## Choices (all decided: V6 to V8)

| # | Choice | Options | Recommendation |
|---|---|---|---|
| C1 | Where the proof files live in the repository | (a) inside each proven crate, `crates/<name>/verify/` (spec, Lean, extract script); (b) one top-level `verify/` | **(a)**: the spec and proofs sit next to the code they cover (the project's topology rule) |
| C2 | Where the Lean checks run | (a) locally in the Docker toolchain until 0016 C11 is decided; (b) CI now | **(a)** |
| C3 | Who writes the proofs | (a) a Claude implementer; (b) the Codex implementer; (c) either, per brief | **(c)**; the spike used Claude |

## Amendments

### A1 (2026-11-24, `em-event-loop` gate redesign, step 3b): how the gates decide

This amends D4 (G4, G5) and D5. The text above stays as it was; where it differs from this amendment, this amendment applies.

- **(a) The `native_decide` axiom.** On the container's toolchain (Lean v4.36.0), a theorem proved with `native_decide`
  depends on the axiom `<decl>._native.native_decide.ax_*`, not on `Lean.ofReduceBool`. The G4 check allows that axiom
  only for theorems of the named examples module (`ExtractedExamples`) and, on the planted-bug path only, of the
  generated counter-example module. It is not allowed anywhere else.
- **(b) Lean decides G4 and G5.** The checks read the elaborated Lean environment, not the source text.
  `AuditAxioms.lean` collects the axioms of every theorem (`Lean.collectAxioms`), fails on `sorryAx` and on any user
  `axiom`, and checks the exact number of theorems in each module, so a lost theorem fails. `check-modules.py` requires
  every Lean file to be a lakefile root and every project module to be audited, and rejects only the lines that start with
  `set_option debug.skipKernelTC` or `@[implemented_by`. Negative fixtures (a file with `sorry`, `admit`, a user
  `axiom`, a `native_decide` outside the examples module, a lost theorem, a stray file) must each make the gate fail,
  for their own reason.
- **(c) A second frozen digest (G5).** G5 also freezes the refinement contract: `Abstraction.lean`, the nine `*_refines`
  theorems, the four `*_fails` theorems, `RustChange`, `RustPassTrace`, `RustPassTrace_transfer` and every
  `extracted_S*` theorem of `Transfer.lean`. Their statements are printed from the elaborated types and hashed:
  `refinement 29 478acf91f57efc22b0a631e1add9447fc5ca938f7a060a6a2046514b5f59c0eb`. The value is recorded only after a
  blind back-translation (G1 style) that the user accepts; the user accepted it on 2026-11-24. A change needs the user's
  approval and a new back-translation.
- **(d) Review set (confirms D5).** For this step, the Codex Lean verifier is the only Codex review, covering the proven
  core and its gate tooling (scripts and the audit). Correctness and adversarial reviewers stay for unproven sensitive
  code (the driver and transport). User decision, 2026-11-24.

### A2 (2026-11-24, after the `em-event-loop` retro): proofs only for the very core; spec and tests for the rest

This amends D1, D5, D6 and the acceptance criteria. The text above stays as it was; where it differs from this
amendment, this amendment applies. User decision V9.

**Why.** The full workflow cost about 6,000 lines of Lean, scripts and fixtures for 283 lines of Rust, plus two review
rounds spent on the gate scripts. The proofs found no defect in the Rust, and the Rust tests also caught all 25 planted
bugs. The lasting gain was the precise English spec: writing it settled several rules before any code (0016 U37, U38;
S8, S11, S15). For a hobby project the proofs are worth that cost only for the few cores where a hidden bug is both likely to be
missed by tests and costly; the spec is worth it everywhere.

- **(a) Proofs only for the very core (amends D1).** Aeneas proofs are kept for small, stable code that the project
  writes itself, that sits on the security or reliability path, and is hard to test exhaustively: code that handles
  bytes from the network, or code that every message waits on (the scheduler). The list, by user decision:
  `em-event-loop`'s `LoopCore` (done, frozen, (c)) and the `em-transport`
  `Reassembler`, with its size cap (0016 P0 to P3 and P5, proved in the spike; brought into the repository at 0016 D8
  step 4 with the D6 steps, using a copy of the `em-event-loop` gate tooling in its own `verify/`). The received queue (P4) is no longer a proof
  target; 0016 D7's model-based tests and AC3 cover it. `em-crypto` is not on the list: the primitives (AES, SHA,
  HMAC, the curves) are in the RustCrypto crates, which Aeneas cannot see into, and the project's own constructions on
  top of them (the 802.11 and WPS KDF block loops, about 80 lines) are checked with test vectors generated from the pinned upstream code
  (`tools/upstream-crypto-vectors/`).
  Any other core is added only by a user decision.
- **(b) The default for every other pure core.** A short `spec-en.md` in plain English (words used, numbered rules) approved by
  the user, with an example and a counter-example for each rule whose meaning is subtle; then Rust with unit tests that
  cover every example and counter-example, and a seeded model test against a simple reference model. No Lean, no
  extraction, no gates. The Aeneas-friendly code style (0016 E8, U36) is needed only in the cores of (a); the 0016 core
  rule (no clock, randomness or I/O in cores) stays everywhere, for testability.
- **(c) `em-event-loop`'s proofs are frozen.** `crates/em-event-loop/verify/` stays in the repository as it is, as the
  record of what was proved for `LoopCore` at commit `a286628`. It is not part of the standard checks, and its low gate
  items (`STATUS.md`) will not be fixed. While `src/lib.rs` is unchanged the proofs still hold. If a later change to
  `LoopCore` breaks them, the proof files (everything under `verify/` except `spec-en.md`) are deleted in that change,
  not repaired; `spec-en.md` stays the contract and is updated, with the user's approval, when a rule changes. The Rust
  tests are the safety net either way.
- **(d) Review set (amends D5 and A1 (d), and 0016 D8's "full review set").** One Codex review of every change, plus the
  user's own reading. A second reviewer (correctness) is added only for security-sensitive code: keys, secure
  onboarding, encryption and authentication (secure-onboarding Stages 5 to 8). This also applies to the `em-event-loop`
  driver, `TransportCore` and the `em-agent`/`em-ctrl` moves (0016 D8 steps 4 and 5), which were to get correctness and
  adversarial reviewers. A plan review is run when the user asks for one. The contrarian rule (a plan that fails twice)
  is unchanged. The proven cores of (a) keep A1 (d): the Codex Lean verifier.
- **(e) Acceptance (amends the acceptance criteria).** The criteria above applied to `em-event-loop` and were met,
  with the low Codex findings on the gate scripts left open (`STATUS.md`, 2026-11-24). They apply again to the `Reassembler`. Every other core uses the standard checks only: `cargo test`, clippy (pedantic) on stable and
  1.85.0, `fmt`, `doc`.

## Revision history

- A2 (2026-11-24): proofs only for `LoopCore` (frozen) and the `Reassembler`; spec and tests for every other core;
  lighter review set (V9); see the amendment above.
- A1 (2026-11-24): the gate redesign of step 3b; see the amendment above.
- accepted (2026-11-23): V6 to V8.
- rev 2 (2026-11-23): after the trial on the spike's `LoopCore` (tracking folder `aeneas-spike/loopcore/trial-results.md`):
  G1 pass with four English wording fixes; G2 pass (12 examples, 13 counter-examples); G3 code layer pass (8 of 8
  bugs), rule layer not testable in the spike. Changes: rules proved from the step spec alone (D3); G3 has a code layer
  and a rule layer; `native_decide` allowed for examples only (G2).
- rev 1 (2026-11-23): first draft, during the trial run (V5).
