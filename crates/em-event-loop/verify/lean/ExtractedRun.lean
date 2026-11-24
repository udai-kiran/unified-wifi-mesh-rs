import Abstraction
import Examples

/-! # Running the extracted code on a script

Definitions, and the theorem `natOrd_le` (by `rfl`); no `native_decide`, so this file is in the
axiom check. Time is `Nat`. `XAct` is one call made by the
application; `execX` performs the calls in order on the extracted `LoopCore` and returns the answers
of `next_turn` as step-spec `Answer`s (via `absTurn`). `obsX` and `obsRef` show everything the
application can see after a script, for the extracted code and for the reference; the mutation gate
(G3, `mutants/`) compares them. -/

open Aeneas Aeneas.Std Result

namespace EmEventLoop

/-! ## An `Ord Nat` for the extracted code, and hypothesis (a) of the refinement for it -/

def natEq : core.cmp.PartialEq Nat Nat := { eq := fun a b => Result.ok (decide (a = b)) }

def natPartialOrd : core.cmp.PartialOrd Nat Nat :=
  { partialEqInst := natEq
    partial_cmp := fun a b => Result.ok (some (compare a b))
    le := fun a b => Result.ok (decide (a ≤ b)) }

def natOrd : core.cmp.Ord Nat :=
  { eqInst := { partialEqInst := natEq }
    partialOrdInst := natPartialOrd
    cmp := fun a b => Result.ok (compare a b) }

def natCopy : core.marker.Copy Nat := BuiltinCopy Nat

/-- Hypothesis (a) of the refinement holds for this `Ord Nat`: so `Refinement.lean` and
`Transfer.lean` apply to the code run below. -/
theorem natOrd_le (a b : Nat) : natOrd.partialOrdInst.le a b = Result.ok (decide (a ≤ b)) := rfl

/-! ## The scripts -/

inductive XAct where
  | ready (i : Usize) (b : Bool)
  | paused (i : Usize) (b : Bool)
  | deadline (h : Usize) (d : Option Nat)
  | deadlinePaused (h : Usize) (b : Bool)
  | beginPass (now : Nat)
  | next

/-- Short names for the scripts. -/
abbrev rdy := XAct.ready
abbrev pau := XAct.paused

/-- A small number as a `Usize` (the examples use numbers below 16). -/
def u (n : Nat) (h : n ≤ 16 := by decide) : Usize := Usize.ofNat n (by scalar_tac)

def execX : em_event_loop.LoopCore Nat → List XAct → List Answer →
    Result (em_event_loop.LoopCore Nat × List Answer)
  | c, [], log => Result.ok (c, log)
  | c, .ready i b :: rest, log => do
    let c ← em_event_loop.LoopCore.set_ready natOrd natCopy c i b
    execX c rest log
  | c, .paused i b :: rest, log => do
    let c ← em_event_loop.LoopCore.set_paused natOrd natCopy c i b
    execX c rest log
  | c, .deadline h d :: rest, log => do
    let c ← em_event_loop.LoopCore.set_deadline natOrd natCopy c h d
    execX c rest log
  | c, .deadlinePaused h b :: rest, log => do
    let c ← em_event_loop.LoopCore.set_deadline_paused natOrd natCopy c h b
    execX c rest log
  | c, .beginPass now :: rest, log => do
    let c ← em_event_loop.LoopCore.begin_pass natOrd natCopy c now
    execX c rest log
  | c, .next :: rest, log => do
    let (t, c) ← em_event_loop.LoopCore.next_turn natOrd natCopy c
    execX c rest (log ++ [absTurn t])

/-- A new loop with the given budgets and number of handlers. -/
def newX (budgets : List Usize) (handlers : Usize) (hlen : budgets.length ≤ 16 := by decide) :
    Result (em_event_loop.LoopCore Nat) :=
  em_event_loop.LoopCore.new natOrd natCopy (Slice.from budgets (by scalar_tac)) handlers

def finalX (budgets : List Usize) (handlers : Usize) (acts : List XAct)
    (hlen : budgets.length ≤ 16 := by decide) :
    Result (em_event_loop.LoopCore Nat × List Answer) := do
  let c ← newX budgets handlers hlen
  execX c acts []

def answersX (budgets : List Usize) (handlers : Usize) (acts : List XAct)
    (hlen : budgets.length ≤ 16 := by decide) :
    Result (List Answer) := do
  let r ← finalX budgets handlers acts hlen
  Result.ok r.2

/-- The abstract state after the script. -/
def stateX (budgets : List Usize) (handlers : Usize) (acts : List XAct)
    (hlen : budgets.length ≤ 16 := by decide) :
    Result (LoopState Nat) := do
  let r ← finalX budgets handlers acts hlen
  Result.ok (abs r.1)


/-! ## What the application can see after a script -/

/-- Answers, then per source (ready, paused, budget), per handler (deadline, paused), then
`next_deadline()` and `has_ready_source()`. -/
abbrev Obs := List Answer × List (Bool × Bool × Nat) × List (Option Nat × Bool) × Option Nat × Bool

def obsX (budgets : List Usize) (handlers : Usize) (acts : List XAct)
    (hlen : budgets.length ≤ 16 := by decide) : Result Obs := do
  let r ← finalX budgets handlers acts hlen
  let d ← em_event_loop.LoopCore.next_deadline natOrd natCopy r.1
  let b ← em_event_loop.LoopCore.has_ready_source natOrd natCopy r.1
  let s := abs r.1
  Result.ok (r.2, s.sources.map (fun x => (x.ready, x.paused, x.budget)),
    s.handlers.map (fun x => (x.deadline, x.paused)), d, b)

def toAct : XAct → Act
  | .ready i b => .ready i.val b
  | .paused i b => .paused i.val b
  | .deadline h d => .deadline h.val d
  | .deadlinePaused h b => .deadlinePaused h.val b
  | .beginPass now => .beginPass now
  | .next => .next

/-- The same observation of the reference implementation, which meets the step spec. -/
def obsRef (budgets : List Usize) (handlers : Usize) (acts : List XAct) : Obs :=
  let start := newLoop (budgets.map (fun b => b.val)) handlers.val
  let s := finalState start (acts.map toAct)
  (answers start (acts.map toAct), s.sources.map (fun x => (x.ready, x.paused, x.budget)),
    s.handlers.map (fun x => (x.deadline, x.paused)),
    match refOps.nextDeadline s with | .ok d => d | .panic => none,
    match refOps.hasReadySource s with | .ok b => b | .panic => false)

/-- The extracted run succeeded and gave exactly `o`. -/
def agrees (r : Result Obs) (o : Obs) : Bool :=
  match r.match with
  | .ok x => decide (x = o)
  | _ => false

end EmEventLoop
