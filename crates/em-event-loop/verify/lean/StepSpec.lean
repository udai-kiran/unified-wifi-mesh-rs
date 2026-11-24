import Mathlib.Order.Basic

/-! # Step spec of `em-event-loop` (note 0017 D3)

A pure model: an abstract state, and for every operation a relation that says what one call
returns. Nothing here is Rust or generated code. Time is any type with a linear order. -/

namespace EmEventLoop

/-- What an operation returns: a value, or a panic (a number that does not exist). -/
inductive Outcome (α : Type) where
  | ok (value : α)
  | panic
  deriving DecidableEq

/-- The answer of `next_turn`. -/
inductive Answer where
  | serve (source : Nat)
  | runDeadline (handler : Nat)
  | passDone
  deriving DecidableEq, Repr

/-- A source: its flags and its budget (the budget is fixed when the loop is created). -/
structure Src where
  ready : Bool
  paused : Bool
  budget : Nat
  deriving DecidableEq, Repr

/-- A handler: one deadline or none, and a pause flag for its timer. -/
structure Hdl (T : Type) where
  deadline : Option T
  paused : Bool

/-- Pass progress. `done` is both "before the first `begin_pass`" and "pass done".
`serving`: the time of the pass, the current position, and the budget used so far per source.
`deadlines`: the time of the pass and the next handler to look at. -/
inductive Phase (T : Type) where
  | done
  | serving (now : T) (position : Nat) (used : Nat → Nat)
  | deadlines (now : T) (next : Nat)

structure LoopState (T : Type) where
  sources : List Src
  handlers : List (Hdl T)
  phase : Phase T

/-- Change the element at index `i` and keep the rest (no change if `i` is out of range). -/
def modifyAt {α : Type} (l : List α) (i : Nat) (f : α → α) : List α :=
  l.mapIdx fun k x => if k = i then f x else x

/-- One more unit of budget used by source `j`. -/
def usedBump (used : Nat → Nat) (j : Nat) : Nat → Nat :=
  fun i => if i = j then used i + 1 else used i

variable {T : Type} [LinearOrder T]

/-- Source `i` exists, is ready, is not paused, and has budget left when `used i` is spent. -/
def Eligible (sources : List Src) (used : Nat → Nat) (i : Nat) : Prop :=
  ∃ x, sources[i]? = some x ∧ x.ready = true ∧ x.paused = false ∧ used i < x.budget

/-- Handler `h` exists, has a deadline, is not paused, and the deadline is not later than `now`. -/
def Due (handlers : List (Hdl T)) (now : T) (h : Nat) : Prop :=
  ∃ x d, handlers[h]? = some x ∧ x.deadline = some d ∧ x.paused = false ∧ d ≤ now

/-! ## Step relations. `XStep args result` holds exactly when `result` is what the call returns. -/

def NewStep (budgets : List Nat) (handlerCount : Nat) (r : Outcome (LoopState T)) : Prop :=
  r = .ok { sources := budgets.map fun b => { ready := false, paused := false, budget := b }
            handlers := List.replicate handlerCount { deadline := none, paused := false }
            phase := .done }

def BeginPassStep (s : LoopState T) (now : T) (r : Outcome (LoopState T)) : Prop :=
  r = .ok { s with phase := .serving now 0 (fun _ => 0) }

/-- The deadline part of a pass: look at handlers from `start`; the first due one runs. -/
def ScanHandlers (s : LoopState T) (now : T) (start : Nat)
    (r : Outcome (Answer × LoopState T)) : Prop :=
  (∃ h, start ≤ h ∧ Due s.handlers now h ∧
      (∀ i, start ≤ i → i < h → ¬ Due s.handlers now i) ∧
      r = .ok (.runDeadline h,
        { s with handlers := modifyAt s.handlers h fun x => { x with deadline := none }
                 phase := .deadlines now (h + 1) })) ∨
  ((∀ i, start ≤ i → ¬ Due s.handlers now i) ∧
      r = .ok (.passDone, { s with phase := .done }))

/-- The source part of a pass: look at sources from `position`; the first eligible one is
served; if none is, go on with the handlers from 0. -/
def ScanSources (s : LoopState T) (now : T) (position : Nat) (used : Nat → Nat)
    (r : Outcome (Answer × LoopState T)) : Prop :=
  (∃ j, position ≤ j ∧ Eligible s.sources used j ∧
      (∀ i, position ≤ i → i < j → ¬ Eligible s.sources used i) ∧
      r = .ok (.serve j,
        { s with sources := modifyAt s.sources j fun x => { x with ready := false }
                 phase := .serving now j (usedBump used j) })) ∨
  ((∀ i, position ≤ i → ¬ Eligible s.sources used i) ∧ ScanHandlers s now 0 r)

def NextTurnStep (s : LoopState T) (r : Outcome (Answer × LoopState T)) : Prop :=
  match s.phase with
  | .done => r = .ok (.passDone, s)
  | .serving now position used => ScanSources s now position used r
  | .deadlines now next => ScanHandlers s now next r

def SetReadyStep (s : LoopState T) (i : Nat) (b : Bool) (r : Outcome (LoopState T)) : Prop :=
  (i < s.sources.length →
      r = .ok { s with sources := modifyAt s.sources i fun x => { x with ready := b } }) ∧
  (s.sources.length ≤ i → r = .panic)

def SetPausedStep (s : LoopState T) (i : Nat) (b : Bool) (r : Outcome (LoopState T)) : Prop :=
  (i < s.sources.length →
      r = .ok { s with sources := modifyAt s.sources i fun x => { x with paused := b } }) ∧
  (s.sources.length ≤ i → r = .panic)

def SetDeadlineStep (s : LoopState T) (h : Nat) (d : Option T) (r : Outcome (LoopState T)) : Prop :=
  (h < s.handlers.length →
      r = .ok { s with handlers := modifyAt s.handlers h fun x => { x with deadline := d } }) ∧
  (s.handlers.length ≤ h → r = .panic)

def SetDeadlinePausedStep (s : LoopState T) (h : Nat) (b : Bool)
    (r : Outcome (LoopState T)) : Prop :=
  (h < s.handlers.length →
      r = .ok { s with handlers := modifyAt s.handlers h fun x => { x with paused := b } }) ∧
  (s.handlers.length ≤ h → r = .panic)

/-- `o` is the earliest deadline among handlers that have one and are not paused, or `none`. -/
def EarliestDeadline (handlers : List (Hdl T)) (o : Option T) : Prop :=
  (o = none ∧ ∀ x ∈ handlers, x.paused = false → x.deadline = none) ∨
  (∃ d, o = some d ∧ (∃ x ∈ handlers, x.paused = false ∧ x.deadline = some d) ∧
      ∀ x ∈ handlers, x.paused = false → ∀ e, x.deadline = some e → d ≤ e)

def NextDeadlineStep (s : LoopState T) (r : Outcome (Option T)) : Prop :=
  ∃ o, r = .ok o ∧ EarliestDeadline s.handlers o

/-- Some source is ready, not paused, and has a budget above 0. -/
def ReadySourceExists (sources : List Src) : Prop :=
  ∃ x ∈ sources, x.ready = true ∧ x.paused = false ∧ 0 < x.budget

def HasReadySourceStep (s : LoopState T) (r : Outcome Bool) : Prop :=
  ∃ b, r = .ok b ∧ (b = true ↔ ReadySourceExists s.sources)

/-! ## Operations that meet the step spec -/

structure Ops (T : Type) where
  new : List Nat → Nat → Outcome (LoopState T)
  beginPass : LoopState T → T → Outcome (LoopState T)
  nextTurn : LoopState T → Outcome (Answer × LoopState T)
  setReady : LoopState T → Nat → Bool → Outcome (LoopState T)
  setPaused : LoopState T → Nat → Bool → Outcome (LoopState T)
  setDeadline : LoopState T → Nat → Option T → Outcome (LoopState T)
  setDeadlinePaused : LoopState T → Nat → Bool → Outcome (LoopState T)
  nextDeadline : LoopState T → Outcome (Option T)
  hasReadySource : LoopState T → Outcome Bool

structure Ops.Meets (ops : Ops T) : Prop where
  new : ∀ budgets n, NewStep budgets n (ops.new budgets n)
  beginPass : ∀ s now, BeginPassStep s now (ops.beginPass s now)
  nextTurn : ∀ s, NextTurnStep s (ops.nextTurn s)
  setReady : ∀ s i b, SetReadyStep s i b (ops.setReady s i b)
  setPaused : ∀ s i b, SetPausedStep s i b (ops.setPaused s i b)
  setDeadline : ∀ s h d, SetDeadlineStep s h d (ops.setDeadline s h d)
  setDeadlinePaused : ∀ s h b, SetDeadlinePausedStep s h b (ops.setDeadlinePaused s h b)
  nextDeadline : ∀ s, NextDeadlineStep s (ops.nextDeadline s)
  hasReadySource : ∀ s, HasReadySourceStep s (ops.hasReadySource s)

/-! ## The application between answers, and one pass as seen by the application -/

/-- Between answers the application may change any ready flag, pause flag or deadline. It cannot
change the number of sources, their budgets, the number of handlers, or the pass progress. -/
def Env (s s' : LoopState T) : Prop :=
  s'.phase = s.phase ∧ s'.sources.map (·.budget) = s.sources.map (·.budget) ∧
    s'.handlers.length = s.handlers.length

/-- One pass, followed for as many calls as we like. Call `k` of `next_turn` sees `seen k` and
returns `answer k` and `after k`; the application then changes `after k` into `seen (k + 1)`.
The first call sees the state left by `begin_pass now` after one application change. -/
structure PassTrace (ops : Ops T) (now : T) where
  start : LoopState T
  seen : Nat → LoopState T
  after : Nat → LoopState T
  answer : Nat → Answer
  began : ∃ p, ops.beginPass start now = .ok p ∧ Env p (seen 0)
  call : ∀ k, ops.nextTurn (seen k) = .ok (answer k, after k)
  change : ∀ k, Env (after k) (seen (k + 1))

end EmEventLoop
