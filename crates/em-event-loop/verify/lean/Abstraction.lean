import Extracted.Funs
import StepSpec

/-! # The abstraction function from the extracted code to the step spec

`abs` maps a state of the extracted `LoopCore` to the `LoopState` of `StepSpec.lean`. Definitions
only: the proofs are in `Refinement.lean`. The file is separate so that the mutation gate (G3)
can compile the same definitions against a mutated extraction. -/

open Aeneas Aeneas.Std Result ControlFlow

namespace EmEventLoop

def absSrc (s : em_event_loop.SourceState) : Src :=
  { ready := s.ready, paused := s.paused, budget := s.budget.val }

def absHdl {T : Type} (h : em_event_loop.HandlerState T) : Hdl T :=
  { deadline := h.deadline, paused := h.paused }

/-- Budget used by source `i` in the current pass; 0 for a source that does not exist. -/
def absUsed (sources : List em_event_loop.SourceState) : Nat → Nat :=
  fun i => ((sources[i]?).map (fun s => s.used.val)).getD 0

def absPhase {T : Type} (sources : List em_event_loop.SourceState) :
    em_event_loop.Phase T → Phase T
  | .Done => .done
  | .Serving now position => .serving now position.val (absUsed sources)
  | .Deadlines now next => .deadlines now next.val

def abs {T : Type} (c : em_event_loop.LoopCore T) : LoopState T :=
  { sources := c.sources.val.map absSrc
    handlers := c.handlers.val.map absHdl
    phase := absPhase c.sources.val c.phase }

def absTurn : em_event_loop.Turn → Answer
  | .Serve source => .serve source.val
  | .Deadline handler => .runDeadline handler.val
  | .PassDone => .passDone

/-- `o` is the `Outcome` of the extracted result `r`: `ok v` gives `ok (f v)`, `fail` gives `panic`.
A divergence has no outcome, so a theorem that gives one also says the call does not diverge. -/
def OutcomeOf {α β : Type _} (f : α → β) (r : Result α) (o : Outcome β) : Prop :=
  (∃ v, r = Result.ok v ∧ o = Outcome.ok (f v)) ∨ (∃ e, r = Result.fail e ∧ o = Outcome.panic)

end EmEventLoop
