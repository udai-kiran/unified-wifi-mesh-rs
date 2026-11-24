import ExtractedRun

/-! # Gate G2 on the extracted code: every example of `spec-en.md`

The examples here run the extracted functions with `native_decide`:
`Aeneas.Std.loop` is a `partial_fixpoint`, which `rfl` and `decide` cannot unfold, so the compiler
evaluates it. Each check is a named theorem (an `example` never enters the environment, so the
axiom audit could not see it). The audit (`AuditAxioms.lean`) allows the `native_decide` axiom of
a theorem in this module and nowhere else. The kernel-checked part of G2 is the refinement
(`Refinement.lean`) together with `Examples.lean`, which runs the same scripts on the reference.

The scripts are those of `Examples.lean`, run by `execX` of `ExtractedRun.lean`. -/

open Aeneas Aeneas.Std Result

namespace EmEventLoop

open XAct Answer

/-! ## S1. Start -/

theorem exX_S1_1 : (answersX [u 1, u 1] (u 1) [beginPass 0, next]).reducesTo [passDone] = true := by
  native_decide

-- before the first `begin_pass`, with a ready source: "pass done", and nothing changes
theorem exX_S1_2 : (answersX [u 1] (u 0) [rdy (u 0) true, next]).reducesTo [passDone] = true := by native_decide

theorem exX_S1_3 : ((stateX [u 1] (u 0) [rdy (u 0) true, next]) >>= fun s => Result.ok s.sources).reducesTo
    [⟨true, false, 1⟩] = true := by native_decide

theorem exX_S1_4 : ((stateX [u 1] (u 0) [rdy (u 0) true, next]) >>= fun s =>
    Result.ok (match s.phase with | .done => true | _ => false)).reducesTo true = true := by
  native_decide

/-! ## S2. Budget -/

theorem exX_S2_1 : (answersX [u 2] (u 0)
    [rdy (u 0) true, beginPass 0, next, rdy (u 0) true, next, rdy (u 0) true, next]).reducesTo
    [serve 0, serve 0, passDone] = true := by native_decide

theorem exX_S2_2 : (answersX [u 0] (u 0) [rdy (u 0) true, beginPass 0, next]).reducesTo [passDone] = true := by
  native_decide

/-! ## S3. Order and shape -/

theorem exX_S3_1 : (answersX [u 1, u 1] (u 1)
    [rdy (u 0) true, rdy (u 1) true, deadline (u 0) (some 0), beginPass 0, next, next, next, next,
     next]).reducesTo
    [serve 0, serve 1, runDeadline 0, passDone, passDone] = true := by native_decide

/-! ## S4. Checked at each answer -/

theorem exX_S4_1 : (answersX [u 1, u 1] (u 0)
    [rdy (u 0) true, rdy (u 1) true, beginPass 0, next, pau (u 1) true, next]).reducesTo
    [serve 0, passDone] = true := by native_decide

/-! ## S5. Paused means never -/

theorem exX_S5_1 : (answersX [u 1] (u 0)
    [rdy (u 0) true, pau (u 0) true, beginPass 0, next, pau (u 0) false, beginPass 0, next,
     next]).reducesTo [passDone, serve 0, passDone] = true := by native_decide

theorem exX_S5_2 : (answersX [] (u 1)
    [deadline (u 0) (some 3), deadlinePaused (u 0) true, beginPass 5, next,
     deadlinePaused (u 0) false, beginPass 5, next]).reducesTo
    [passDone, runDeadline 0] = true := by native_decide

/-! ## S6. Sources are not skipped -/

theorem exX_S6_1 : (answersX [u 1, u 1, u 1] (u 0)
    [rdy (u 0) true, rdy (u 1) true, rdy (u 2) true, beginPass 0, next, next, next, next]).reducesTo
    [serve 0, serve 1, serve 2, passDone] = true := by native_decide

theorem exX_S6_2 : (answersX [u 1, u 1] (u 0)
    [rdy (u 0) true, rdy (u 1) true, beginPass 0, next, next, rdy (u 0) true, next,
     beginPass 0, next, next]).reducesTo
    [serve 0, serve 1, passDone, serve 0, passDone] = true := by native_decide

/-! ## S7. Serving clears ready -/

theorem exX_S7_1 : (answersX [u 3] (u 0) [rdy (u 0) true, beginPass 0, next, next]).reducesTo
    [serve 0, passDone] = true := by native_decide

theorem exX_S7_2 : (answersX [u 3] (u 0)
    [rdy (u 0) true, beginPass 0, next, rdy (u 0) true, next, rdy (u 0) true, next, rdy (u 0) true,
     next]).reducesTo [serve 0, serve 0, serve 0, passDone] = true := by native_decide

/-! ## S8. Deadlines are not skipped -/

theorem exX_S8_1 : (answersX [] (u 2)
    [deadline (u 0) (some 0), deadline (u 1) (some 0), beginPass 0, next, next, next]).reducesTo
    [runDeadline 0, runDeadline 1, passDone] = true := by native_decide

/-! ## S9. Running a deadline clears it -/

theorem exX_S9_1 : (answersX [] (u 1) [deadline (u 0) (some 5), beginPass 5, next, next]).reducesTo
    [runDeadline 0, passDone] = true := by native_decide

-- `next_deadline()` is then "none"
theorem exX_S9_2 : (do
    let r ← finalX [] (u 1) [deadline (u 0) (some 5), beginPass 5, next, next]
    em_event_loop.LoopCore.next_deadline natOrd natCopy r.1 : Result (Option Nat)).reducesTo
    none = true := by native_decide

theorem exX_S9_3 : (answersX [] (u 1)
    [deadline (u 0) (some 5), beginPass 5, next, deadline (u 0) (some 5), next, beginPass 5,
     next]).reducesTo [runDeadline 0, passDone, runDeadline 0] = true := by native_decide

/-! ## S10. Time is fixed per pass -/

theorem exX_S10_1 : (answersX [] (u 1)
    [deadline (u 0) (some 6), beginPass 5, next, beginPass 6, next]).reducesTo
    [passDone, runDeadline 0] = true := by native_decide

/-! ## S11. A pass always ends -/

/-- Make both sources ready and the deadline due again. -/
def refillX : List XAct := [rdy (u 0) true, rdy (u 1) true, deadline (u 0) (some 0)]

theorem exX_S11_1 : (answersX [u 2, u 1] (u 1)
    (refillX ++ [beginPass 0, next] ++ refillX ++ [next] ++ refillX ++ [next] ++ refillX ++
      [next] ++ refillX ++ [next])).reducesTo
    [serve 0, serve 0, serve 1, runDeadline 0, passDone] = true := by native_decide

/-! ## S12. Next deadline -/

theorem exX_S12_1 : (do
    let r ← finalX [] (u 3)
      [deadline (u 0) (some 7), deadline (u 1) (some 3), deadline (u 2) (some 1),
       deadlinePaused (u 2) true]
    em_event_loop.LoopCore.next_deadline natOrd natCopy r.1 : Result (Option Nat)).reducesTo
    (some 3) = true := by native_decide

/-! ## S13. More work -/

def hasReadyX (budgets : List Usize) (acts : List XAct)
    (hlen : budgets.length ≤ 16 := by decide) : Result Bool := do
  let r ← finalX budgets (u 0) acts hlen
  em_event_loop.LoopCore.has_ready_source natOrd natCopy r.1

theorem exX_S13_1 : (hasReadyX [u 1] [rdy (u 0) true]).reducesTo true = true := by native_decide

theorem exX_S13_2 : (hasReadyX [u 1] [rdy (u 0) true, pau (u 0) true]).reducesTo false = true := by
  native_decide

theorem exX_S13_3 : (hasReadyX [u 1] []).reducesTo false = true := by native_decide

theorem exX_S13_4 : (hasReadyX [u 0] [rdy (u 0) true]).reducesTo false = true := by native_decide

/-! ## S16. Starting again -/

theorem exX_S16_1 : (answersX [u 1] (u 0)
    [rdy (u 0) true, beginPass 0, next, rdy (u 0) true, beginPass 1, next, next]).reducesTo
    [serve 0, serve 0, passDone] = true := by native_decide

/-! ## S17. Setters change only their target -/

theorem exX_S17_1 : ((stateX [u 1, u 1] (u 0) [pau (u 0) true, pau (u 1) true, rdy (u 1) true]) >>= fun s =>
    Result.ok s.sources).reducesTo [⟨false, true, 1⟩, ⟨true, true, 1⟩] = true := by
  native_decide

end EmEventLoop
