import Rules
import Reference
import Mathlib.Order.Nat

/-! # Every example of the English spec, run on the reference implementation

Time is `Nat`. `Act` is one call made by the application; `run` performs the calls in order and
returns the final state and the list of answers of `next_turn`. Each example is checked by
`decide`, so the kernel does the evaluation (no `native_decide`). The counter-examples are for a
later gate. -/

namespace EmEventLoop

abbrev Loop := LoopState Nat

inductive Act where
  | ready (i : Nat) (b : Bool)
  | paused (i : Nat) (b : Bool)
  | deadline (h : Nat) (d : Option Nat)
  | deadlinePaused (h : Nat) (b : Bool)
  | beginPass (now : Nat)
  | next

/-- The examples never use a number that does not exist, so a panic leaves the state alone. -/
def keepOnPanic (s : Loop) : Outcome Loop → Loop
  | .ok s' => s'
  | .panic => s

def exec : Loop → List Act → List Answer → Loop × List Answer
  | s, [], log => (s, log.reverse)
  | s, .ready i b :: rest, log => exec (keepOnPanic s (refOps.setReady s i b)) rest log
  | s, .paused i b :: rest, log => exec (keepOnPanic s (refOps.setPaused s i b)) rest log
  | s, .deadline h d :: rest, log => exec (keepOnPanic s (refOps.setDeadline s h d)) rest log
  | s, .deadlinePaused h b :: rest, log =>
    exec (keepOnPanic s (refOps.setDeadlinePaused s h b)) rest log
  | s, .beginPass now :: rest, log => exec (keepOnPanic s (refOps.beginPass s now)) rest log
  | s, .next :: rest, log =>
    match refOps.nextTurn s with
    | .ok (answer, s') => exec s' rest (answer :: log)
    | .panic => exec s rest log

/-- A new loop with the given budgets and number of handlers. -/
def newLoop (budgets : List Nat) (handlers : Nat) : Loop :=
  keepOnPanic ⟨[], [], .done⟩ (refOps.new budgets handlers)

def answers (s : Loop) (acts : List Act) : List Answer := (exec s acts []).2
def finalState (s : Loop) (acts : List Act) : Loop := (exec s acts []).1

open Answer Act

/-! ## S1. Start -/

example : answers (newLoop [1, 1] 1) [beginPass 0, next] = [passDone] := by decide

-- before the first `begin_pass`, with a ready source: "pass done", and nothing changes
example : answers (newLoop [1] 0) [ready 0 true, next] = [passDone] := by decide
example : (finalState (newLoop [1] 0) [ready 0 true, next]).sources = [⟨true, false, 1⟩] := by decide
example : (match (finalState (newLoop [1] 0) [ready 0 true, next]).phase with
    | .done => true
    | _ => false) = true := by decide

/-! ## S2. Budget -/

example : answers (newLoop [2] 0)
    [ready 0 true, beginPass 0, next, ready 0 true, next, ready 0 true, next] =
    [serve 0, serve 0, passDone] := by decide

example : answers (newLoop [0] 0) [ready 0 true, beginPass 0, next] = [passDone] := by decide

/-! ## S3. Order and shape -/

example : answers (newLoop [1, 1] 1)
    [ready 0 true, ready 1 true, deadline 0 (some 0), beginPass 0, next, next, next, next, next] =
    [serve 0, serve 1, runDeadline 0, passDone, passDone] := by decide

/-! ## S4. Checked at each answer -/

example : answers (newLoop [1, 1] 0)
    [ready 0 true, ready 1 true, beginPass 0, next, paused 1 true, next] =
    [serve 0, passDone] := by decide

/-! ## S5. Paused means never -/

example : answers (newLoop [1] 0)
    [ready 0 true, paused 0 true, beginPass 0, next, paused 0 false, beginPass 0, next, next] =
    [passDone, serve 0, passDone] := by decide

example : answers (newLoop [] 1)
    [deadline 0 (some 3), deadlinePaused 0 true, beginPass 5, next,
     deadlinePaused 0 false, beginPass 5, next] =
    [passDone, runDeadline 0] := by decide

/-! ## S6. Sources are not skipped -/

example : answers (newLoop [1, 1, 1] 0)
    [ready 0 true, ready 1 true, ready 2 true, beginPass 0, next, next, next, next] =
    [serve 0, serve 1, serve 2, passDone] := by decide

example : answers (newLoop [1, 1] 0)
    [ready 0 true, ready 1 true, beginPass 0, next, next, ready 0 true, next,
     beginPass 0, next, next] =
    [serve 0, serve 1, passDone, serve 0, passDone] := by decide

/-! ## S7. Serving clears ready -/

example : answers (newLoop [3] 0) [ready 0 true, beginPass 0, next, next] =
    [serve 0, passDone] := by decide

example : answers (newLoop [3] 0)
    [ready 0 true, beginPass 0, next, ready 0 true, next, ready 0 true, next, ready 0 true, next] =
    [serve 0, serve 0, serve 0, passDone] := by decide

/-! ## S8. Deadlines are not skipped -/

example : answers (newLoop [] 2)
    [deadline 0 (some 0), deadline 1 (some 0), beginPass 0, next, next, next] =
    [runDeadline 0, runDeadline 1, passDone] := by decide

/-- The pass of the S8 example, with the application changing nothing between answers. -/
def twoDueTrace : PassTrace (refOps : Ops Nat) 0 :=
  refTrace
    (finalState (newLoop [] 2) [deadline 0 (some 0), deadline 1 (some 0)]) 0
    (fun _ s => s) (fun _ _ => ⟨rfl, rfl, rfl⟩)

theorem dueAt_of_ref (tr : PassTrace (refOps : Ops Nat) 0) (k h : Nat)
    (hb : refDue (tr.seen k).handlers 0 h = true) : DueAt tr k h :=
  (refDue_iff _ _ _).1 hb

-- the pass ends at call 2 ("pass done")
example : twoDueTrace.answer 0 = runDeadline 0 ∧ twoDueTrace.answer 1 = runDeadline 1 ∧
    twoDueTrace.answer 2 = passDone := by decide

/-- The hypothesis of `S8_due_handler_runs_exactly_once` holds for handler 0 here, although
running clears the deadline: handler 0 is due at call 0, the call that runs it, and the
hypothesis asks nothing about the later calls. -/
example : ∀ k, k ≤ 2 → (∀ j, j < k → twoDueTrace.answer j ≠ runDeadline 0) →
    DueAt twoDueTrace k 0 := by
  intro k hk hprev
  have hcases : k = 0 ∨ k = 1 ∨ k = 2 := by omega
  rcases hcases with rfl | rfl | rfl
  · exact dueAt_of_ref _ _ _ (by decide)
  · exact absurd (by decide : twoDueTrace.answer 0 = runDeadline 0) (hprev 0 (by omega))
  · exact absurd (by decide : twoDueTrace.answer 0 = runDeadline 0) (hprev 0 (by omega))

/-- Same for handler 1, which runs at call 1. -/
example : ∀ k, k ≤ 2 → (∀ j, j < k → twoDueTrace.answer j ≠ runDeadline 1) →
    DueAt twoDueTrace k 1 := by
  intro k hk hprev
  have hcases : k = 0 ∨ k = 1 ∨ k = 2 := by omega
  rcases hcases with rfl | rfl | rfl
  · exact dueAt_of_ref _ _ _ (by decide)
  · exact dueAt_of_ref _ _ _ (by decide)
  · exact absurd (by decide : twoDueTrace.answer 1 = runDeadline 1) (hprev 1 (by omega))

/-- The form "due at every call" would not apply: handler 0 is no longer due at call 1,
because running it cleared its deadline. -/
example : ¬ ∀ k, k ≤ 2 → DueAt twoDueTrace k 0 := by
  intro hall
  have h1 := (refDue_iff _ _ _).2 (hall 1 (by omega))
  revert h1
  decide

/-! ## S9. Running a deadline clears it -/

example : answers (newLoop [] 1) [deadline 0 (some 5), beginPass 5, next, next] =
    [runDeadline 0, passDone] := by decide

example : refNextDeadline
    (finalState (newLoop [] 1) [deadline 0 (some 5), beginPass 5, next, next]).handlers =
    none := by decide

example : answers (newLoop [] 1)
    [deadline 0 (some 5), beginPass 5, next, deadline 0 (some 5), next, beginPass 5, next] =
    [runDeadline 0, passDone, runDeadline 0] := by decide

/-! ## S10. Time is fixed per pass -/

example : answers (newLoop [] 1)
    [deadline 0 (some 6), beginPass 5, next, beginPass 6, next] =
    [passDone, runDeadline 0] := by decide

/-! ## S11. A pass always ends -/

/-- Make both sources ready and the deadline due again. -/
def refill : List Act := [ready 0 true, ready 1 true, deadline 0 (some 0)]

example : answers (newLoop [2, 1] 1)
    (refill ++ [beginPass 0, next] ++ refill ++ [next] ++ refill ++ [next] ++ refill ++ [next]
      ++ refill ++ [next]) =
    [serve 0, serve 0, serve 1, runDeadline 0, passDone] := by decide

/-! ## S12. Next deadline -/

def threeHandlers : Loop :=
  finalState (newLoop [] 3)
    [deadline 0 (some 7), deadline 1 (some 3), deadline 2 (some 1), deadlinePaused 2 true]

example : refOps.nextDeadline threeHandlers = .ok (some 3) := by decide

/-! ## S13. More work -/

example : refOps.hasReadySource (finalState (newLoop [1] 0) [ready 0 true]) = .ok true := by decide

example : refOps.hasReadySource (finalState (newLoop [1] 0) [ready 0 true, paused 0 true]) =
    .ok false := by decide

example : refOps.hasReadySource (newLoop [1] 0) = .ok false := by decide

example : refOps.hasReadySource (finalState (newLoop [0] 0) [ready 0 true]) = .ok false := by decide

/-! ## S16. Starting again -/

example : answers (newLoop [1] 0)
    [ready 0 true, beginPass 0, next, ready 0 true, beginPass 1, next, next] =
    [serve 0, serve 0, passDone] := by decide

/-! ## S17. Setters change only their target -/

example : ((finalState (newLoop [1, 1] 0) [paused 0 true, paused 1 true, ready 1 true]).sources) =
    [⟨false, true, 1⟩, ⟨true, true, 1⟩] := by decide

end EmEventLoop
