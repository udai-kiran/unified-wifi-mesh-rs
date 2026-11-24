import Refinement

/-! # Transfer: the 41 rules of `Rules.lean` hold for the extracted code

A pass of the extracted code, with the application changing the state between calls through the
extracted setters, is mapped by `abs` to a `PassTrace` of the reference operations `refOps`.
`refOps` meets the step spec (`refOps_meets`), and the step relations have exactly one result
(`*_exactly_one`), so each extracted call is the reference call on the abstract state. The rule
theorems of `Rules.lean` (stated for any operations that meet the step spec) then apply. -/

open Aeneas Aeneas.Std Result ControlFlow

namespace EmEventLoop

set_option linter.unusedSectionVars false
set_option linter.unusedVariables false

variable {T : Type} [LinearOrder T] (inst : core.cmp.Ord T) (cp : core.marker.Copy T)

/-! ## An `ok` result has an `ok` outcome -/

theorem OutcomeOf_ok {α β : Type _} {f : α → β} {r : Result α} {o : Outcome β} {v : α}
    (h : OutcomeOf f r o) (hr : r = Result.ok v) : o = Outcome.ok (f v) := by
  rcases h with ⟨w, hw, ho⟩ | ⟨e, he, -⟩
  · rw [hr] at hw
    rw [ho, Result.ok_injective hw]
  · rw [hr] at he
    exact absurd he ok_not_fail

/-- An `ok` or `fail` result has an outcome, and that outcome is `panic` exactly for `fail`. -/
theorem OutcomeOf_fail {α β : Type _} {f : α → β} {r : Result α} {o : Outcome β} {e : Error}
    (h : OutcomeOf f r o) (hr : r = Result.fail e) : o = Outcome.panic := by
  rcases h with ⟨w, hw, -⟩ | ⟨e', -, ho⟩
  · rw [hr] at hw
    exact absurd hw.symm ok_not_fail
  · exact ho

theorem ok_of_outcome {α β : Type _} {f : α → β} {r : Result α} {o : Outcome β}
    (h : OutcomeOf f r o) (hok : ∃ x, o = Outcome.ok x) : ∃ v, r = Result.ok v := by
  rcases h with ⟨v, hv, -⟩ | ⟨e, -, ho⟩
  · exact ⟨v, hv⟩
  · obtain ⟨x, hx⟩ := hok
    rw [ho] at hx
    cases hx

/-! ## An extracted call, seen through `abs`, is the reference call -/

section Images

variable (hle : ∀ a b : T, inst.partialOrdInst.le a b = Result.ok (decide (a ≤ b)))
include hle

theorem begin_pass_image (c c' : em_event_loop.LoopCore T) (now : T)
    (h : em_event_loop.LoopCore.begin_pass inst cp c now = Result.ok c') :
    (refOps : Ops T).beginPass (abs c) now = Outcome.ok (abs c') := by
  obtain ⟨o, ho, hs⟩ := begin_pass_refines inst cp c now
  rw [OutcomeOf_ok ho h] at hs
  exact (beginPass_exactly_one (abs c) now).unique (refOps_meets.beginPass _ _) hs

theorem next_turn_image (c c' : em_event_loop.LoopCore T) (t : em_event_loop.Turn)
    (h : em_event_loop.LoopCore.next_turn inst cp c = Result.ok (t, c')) :
    (refOps : Ops T).nextTurn (abs c) = Outcome.ok (absTurn t, abs c') := by
  obtain ⟨o, ho, hs⟩ := next_turn_refines inst cp hle c
  rw [OutcomeOf_ok ho h] at hs
  exact (nextTurn_exactly_one (abs c)).unique (refOps_meets.nextTurn _) hs

theorem set_ready_image (c c' : em_event_loop.LoopCore T) (source : Usize) (b : Bool)
    (h : em_event_loop.LoopCore.set_ready inst cp c source b = Result.ok c') :
    (refOps : Ops T).setReady (abs c) source.val b = Outcome.ok (abs c') := by
  obtain ⟨o, ho, hs⟩ := set_ready_refines inst cp c source b
  rw [OutcomeOf_ok ho h] at hs
  exact (setReady_exactly_one (abs c) source.val b).unique (refOps_meets.setReady _ _ _) hs

theorem set_paused_image (c c' : em_event_loop.LoopCore T) (source : Usize) (b : Bool)
    (h : em_event_loop.LoopCore.set_paused inst cp c source b = Result.ok c') :
    (refOps : Ops T).setPaused (abs c) source.val b = Outcome.ok (abs c') := by
  obtain ⟨o, ho, hs⟩ := set_paused_refines inst cp c source b
  rw [OutcomeOf_ok ho h] at hs
  exact (setPaused_exactly_one (abs c) source.val b).unique (refOps_meets.setPaused _ _ _) hs

theorem set_deadline_image (c c' : em_event_loop.LoopCore T) (handler : Usize) (d : Option T)
    (h : em_event_loop.LoopCore.set_deadline inst cp c handler d = Result.ok c') :
    (refOps : Ops T).setDeadline (abs c) handler.val d = Outcome.ok (abs c') := by
  obtain ⟨o, ho, hs⟩ := set_deadline_refines inst cp c handler d
  rw [OutcomeOf_ok ho h] at hs
  exact (setDeadline_exactly_one (abs c) handler.val d).unique (refOps_meets.setDeadline _ _ _) hs

theorem set_deadline_paused_image (c c' : em_event_loop.LoopCore T) (handler : Usize) (b : Bool)
    (h : em_event_loop.LoopCore.set_deadline_paused inst cp c handler b = Result.ok c') :
    (refOps : Ops T).setDeadlinePaused (abs c) handler.val b = Outcome.ok (abs c') := by
  obtain ⟨o, ho, hs⟩ := set_deadline_paused_refines inst cp c handler b
  rw [OutcomeOf_ok ho h] at hs
  exact (setDeadlinePaused_exactly_one (abs c) handler.val b).unique
    (refOps_meets.setDeadlinePaused _ _ _) hs

end Images

/-! ## What the application may do between two calls of `next_turn`

`RustChange a b`: the application goes from state `a` to state `b` by a finite list of calls to
the extracted setters, `next_deadline` or `has_ready_source`, each returning `ok`. -/

inductive RustChange : em_event_loop.LoopCore T → em_event_loop.LoopCore T → Prop where
  | done (c : em_event_loop.LoopCore T) : RustChange c c
  | setReady (c c' c'' : em_event_loop.LoopCore T) (source : Usize) (b : Bool) :
      em_event_loop.LoopCore.set_ready inst cp c source b = Result.ok c' →
      RustChange c' c'' → RustChange c c''
  | setPaused (c c' c'' : em_event_loop.LoopCore T) (source : Usize) (b : Bool) :
      em_event_loop.LoopCore.set_paused inst cp c source b = Result.ok c' →
      RustChange c' c'' → RustChange c c''
  | setDeadline (c c' c'' : em_event_loop.LoopCore T) (handler : Usize) (d : Option T) :
      em_event_loop.LoopCore.set_deadline inst cp c handler d = Result.ok c' →
      RustChange c' c'' → RustChange c c''
  | setDeadlinePaused (c c' c'' : em_event_loop.LoopCore T) (handler : Usize) (b : Bool) :
      em_event_loop.LoopCore.set_deadline_paused inst cp c handler b = Result.ok c' →
      RustChange c' c'' → RustChange c c''
  | nextDeadline (c c'' : em_event_loop.LoopCore T) (d : Option T) :
      em_event_loop.LoopCore.next_deadline inst cp c = Result.ok d →
      RustChange c c'' → RustChange c c''
  | hasReadySource (c c'' : em_event_loop.LoopCore T) (b : Bool) :
      em_event_loop.LoopCore.has_ready_source inst cp c = Result.ok b →
      RustChange c c'' → RustChange c c''

theorem RustChange_env
    (hle : ∀ a b : T, inst.partialOrdInst.le a b = Result.ok (decide (a ≤ b)))
    {c c' : em_event_loop.LoopCore T} (h : RustChange inst cp c c') : Env (abs c) (abs c') := by
  induction h with
  | done c => exact Env_refl _
  | setReady c c' c'' source b hcall _ ih =>
    exact Env_trans (Env_of_setReady refOps_meets (set_ready_image inst cp hle c c' source b hcall)) ih
  | setPaused c c' c'' source b hcall _ ih =>
    exact Env_trans (Env_of_setPaused refOps_meets (set_paused_image inst cp hle c c' source b hcall)) ih
  | setDeadline c c' c'' handler d hcall _ ih =>
    exact Env_trans
      (Env_of_setDeadline refOps_meets (set_deadline_image inst cp hle c c' handler d hcall)) ih
  | setDeadlinePaused c c' c'' handler b hcall _ ih =>
    exact Env_trans (Env_of_setDeadlinePaused refOps_meets
      (set_deadline_paused_image inst cp hle c c' handler b hcall)) ih
  | nextDeadline c c'' d _ _ ih => exact ih
  | hasReadySource c c'' b _ _ ih => exact ih

/-! ## A pass of the extracted code -/

/-- The shape of `PassTrace`, over states of the extracted code. Call `k` of `next_turn` sees
`seen k` and returns `answer k` and `after k`; the application then changes `after k` into
`seen (k + 1)` with extracted calls. -/
structure RustPassTrace (now : T) where
  start : em_event_loop.LoopCore T
  seen : Nat → em_event_loop.LoopCore T
  after : Nat → em_event_loop.LoopCore T
  answer : Nat → em_event_loop.Turn
  began : ∃ p, em_event_loop.LoopCore.begin_pass inst cp start now = Result.ok p ∧
    RustChange inst cp p (seen 0)
  call : ∀ k, em_event_loop.LoopCore.next_turn inst cp (seen k) = Result.ok (answer k, after k)
  change : ∀ k, RustChange inst cp (after k) (seen (k + 1))

/-- The image of a Rust pass under `abs`: a pass trace of the reference operations. -/
def RustPassTrace.image
    (hle : ∀ a b : T, inst.partialOrdInst.le a b = Result.ok (decide (a ≤ b))) {now : T}
    (tr : RustPassTrace inst cp now) : PassTrace (refOps : Ops T) now where
  start := abs tr.start
  seen := fun k => abs (tr.seen k)
  after := fun k => abs (tr.after k)
  answer := fun k => absTurn (tr.answer k)
  began := by
    obtain ⟨p, hp, hch⟩ := tr.began
    exact ⟨abs p, begin_pass_image inst cp hle tr.start p now hp, RustChange_env inst cp hle hch⟩
  call := fun k => next_turn_image inst cp hle _ _ _ (tr.call k)
  change := fun k => RustChange_env inst cp hle (tr.change k)

/-- Transfer: the image of every Rust pass is a pass trace of `refOps`, with the abstract states
`abs (seen k)` and the answers `absTurn (answer k)`. -/
theorem RustPassTrace_transfer
    (hle : ∀ a b : T, inst.partialOrdInst.le a b = Result.ok (decide (a ≤ b))) {now : T}
    (tr : RustPassTrace inst cp now) :
    ∃ pt : PassTrace (refOps : Ops T) now,
      pt.start = abs tr.start ∧ (∀ k, pt.seen k = abs (tr.seen k)) ∧
      (∀ k, pt.after k = abs (tr.after k)) ∧ (∀ k, pt.answer k = absTurn (tr.answer k)) :=
  ⟨tr.image inst cp hle, rfl, fun _ => rfl, fun _ => rfl, fun _ => rfl⟩

/-! ## Rules applied to the extracted code -/

theorem absTurn_eq_passDone (t : em_event_loop.Turn) :
    absTurn t = Answer.passDone ↔ t = em_event_loop.Turn.PassDone := by
  cases t <;> simp [absTurn]

theorem absTurn_eq_runDeadline (t : em_event_loop.Turn) (h : Usize) :
    absTurn t = Answer.runDeadline h.val ↔ t = em_event_loop.Turn.Deadline h := by
  cases t with
  | Serve s => simp [absTurn]
  | Deadline h' => simp [absTurn, UScalar.eq_equiv]
  | PassDone => simp [absTurn]


section Rules

variable (hle : ∀ a b : T, inst.partialOrdInst.le a b = Result.ok (decide (a ≤ b)))
  {now : T} (tr : RustPassTrace inst cp now)
include hle

/-- S2 for the extracted code: a source is served at most its budget in one pass, however the
application changes the state in between. -/
theorem extracted_S2_served_at_most_budget (i : Nat) (x : em_event_loop.SourceState)
    (hx : (tr.seen 0).sources.val[i]? = some x) (n : Nat) :
    serveCount (fun k => absTurn (tr.answer k)) n i ≤ x.budget.val := by
  refine S2_served_at_most_budget refOps_meets (tr.image inst cp hle) i (absSrc x) ?_ n
  simp [RustPassTrace.image, abs, hx]

/-- S8 for the extracted code: a handler that stays due until its turn runs exactly once in a
finished pass. -/
theorem extracted_S8_due_handler_runs_exactly_once (h : Usize) (m : Nat)
    (hm : tr.answer m = em_event_loop.Turn.PassDone)
    (hfirst : ∀ j, j < m → tr.answer j ≠ em_event_loop.Turn.PassDone)
    (hdue : ∀ k, k ≤ m → (∀ j, j < k → tr.answer j ≠ em_event_loop.Turn.Deadline h) →
      Due (abs (tr.seen k)).handlers now h.val) :
    ∃ c, c < m ∧ tr.answer c = em_event_loop.Turn.Deadline h ∧
      ∀ c', tr.answer c' = em_event_loop.Turn.Deadline h → c' = c := by
  have key := S8_due_handler_runs_exactly_once refOps_meets (tr.image inst cp hle) h.val m
    ((absTurn_eq_passDone _).2 hm)
    (fun j hj heq => hfirst j hj ((absTurn_eq_passDone _).1 heq))
    (fun k hk hno => hdue k hk (fun j hj heq => hno j hj ((absTurn_eq_runDeadline _ _).2 heq)))
  obtain ⟨c, hc, hans, huniq⟩ := key
  exact ⟨c, hc, (absTurn_eq_runDeadline _ _).1 hans,
    fun c' hc' => huniq c' ((absTurn_eq_runDeadline _ _).2 hc')⟩

/-- S11 for the extracted code: a pass ends within (sum of budgets) + (number of handlers)
answers. -/
theorem extracted_S11_pass_ends_within_bound :
    ∃ m, m ≤ ((tr.seen 0).sources.val.map (fun s => s.budget.val)).sum +
        (tr.seen 0).handlers.val.length ∧
      tr.answer m = em_event_loop.Turn.PassDone := by
  obtain ⟨m, hm, hdone⟩ := S11_pass_ends_within_bound refOps_meets (tr.image inst cp hle)
  refine ⟨m, ?_, (absTurn_eq_passDone _).1 hdone⟩
  simpa [RustPassTrace.image, abs, List.map_map, Function.comp_def, absSrc] using hm

end Rules

/-! ## S15 for the extracted functions

No call fails for a source or handler number that exists, and there is no condition on the
state. So the statement holds for every state, which is the state left by any earlier calls: any
number of passes. A number that does not exist fails (the Rust panic). -/

theorem extracted_S15_never_fails_for_existing_numbers
    (hle : ∀ a b : T, inst.partialOrdInst.le a b = Result.ok (decide (a ≤ b))) :
    (∀ (budgets : Slice Usize) (n : Usize), ∃ c : em_event_loop.LoopCore T,
      em_event_loop.LoopCore.new inst cp budgets n = Result.ok c) ∧
    (∀ (c : em_event_loop.LoopCore T) now, ∃ p,
      em_event_loop.LoopCore.begin_pass inst cp c now = Result.ok p) ∧
    (∀ c : em_event_loop.LoopCore T, ∃ t c',
      em_event_loop.LoopCore.next_turn inst cp c = Result.ok (t, c')) ∧
    (∀ (c : em_event_loop.LoopCore T) (i : Usize) b, i.val < c.sources.val.length →
      ∃ c', em_event_loop.LoopCore.set_ready inst cp c i b = Result.ok c') ∧
    (∀ (c : em_event_loop.LoopCore T) (i : Usize) b, i.val < c.sources.val.length →
      ∃ c', em_event_loop.LoopCore.set_paused inst cp c i b = Result.ok c') ∧
    (∀ (c : em_event_loop.LoopCore T) (h : Usize) d, h.val < c.handlers.val.length →
      ∃ c', em_event_loop.LoopCore.set_deadline inst cp c h d = Result.ok c') ∧
    (∀ (c : em_event_loop.LoopCore T) (h : Usize) b, h.val < c.handlers.val.length →
      ∃ c', em_event_loop.LoopCore.set_deadline_paused inst cp c h b = Result.ok c') ∧
    (∀ c : em_event_loop.LoopCore T, ∃ d,
      em_event_loop.LoopCore.next_deadline inst cp c = Result.ok d) ∧
    (∀ c : em_event_loop.LoopCore T, ∃ b,
      em_event_loop.LoopCore.has_ready_source inst cp c = Result.ok b) := by
  refine ⟨?_, ?_, ?_, ?_, ?_, ?_, ?_, ?_, ?_⟩
  · intro budgets n
    obtain ⟨o, ho, hs⟩ := new_refines inst cp budgets n
    have hok : ∃ x, o = Outcome.ok x := ⟨_, hs⟩
    exact ok_of_outcome ho hok
  · intro c now
    obtain ⟨o, ho, hs⟩ := begin_pass_refines inst cp c now
    have hok : ∃ x, o = Outcome.ok x := ⟨_, hs⟩
    exact ok_of_outcome ho hok
  · intro c
    obtain ⟨r, hr, -⟩ := spec_exists_ok (next_turn_spec inst cp hle c)
    exact ⟨r.1, r.2, hr⟩
  · intro c i b hi
    obtain ⟨r, hr, -⟩ := spec_exists_ok (set_ready_spec inst cp c i b hi)
    exact ⟨r, hr⟩
  · intro c i b hi
    obtain ⟨r, hr, -⟩ := spec_exists_ok (set_paused_spec inst cp c i b hi)
    exact ⟨r, hr⟩
  · intro c h d hh
    obtain ⟨r, hr, -⟩ := spec_exists_ok (set_deadline_spec inst cp c h d hh)
    exact ⟨r, hr⟩
  · intro c h b hh
    obtain ⟨r, hr, -⟩ := spec_exists_ok (set_deadline_paused_spec inst cp c h b hh)
    exact ⟨r, hr⟩
  · intro c
    obtain ⟨d, hd, -⟩ := spec_exists_ok (next_deadline_loop_spec inst cp hle c)
    exact ⟨d, hd⟩
  · intro c
    obtain ⟨b, hb, -⟩ := spec_exists_ok (has_ready_source_loop_spec inst cp c)
    exact ⟨b, hb⟩

/-- A number that does not exist fails: the Rust panic. -/
theorem extracted_S15_nonexistent_number_panics :
    (∀ (c : em_event_loop.LoopCore T) (i : Usize) b, c.sources.val.length ≤ i.val →
      ∃ e, em_event_loop.LoopCore.set_ready inst cp c i b = Result.fail e) ∧
    (∀ (c : em_event_loop.LoopCore T) (i : Usize) b, c.sources.val.length ≤ i.val →
      ∃ e, em_event_loop.LoopCore.set_paused inst cp c i b = Result.fail e) ∧
    (∀ (c : em_event_loop.LoopCore T) (h : Usize) d, c.handlers.val.length ≤ h.val →
      ∃ e, em_event_loop.LoopCore.set_deadline inst cp c h d = Result.fail e) ∧
    (∀ (c : em_event_loop.LoopCore T) (h : Usize) b, c.handlers.val.length ≤ h.val →
      ∃ e, em_event_loop.LoopCore.set_deadline_paused inst cp c h b = Result.fail e) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · intro c i b hi
    exact ⟨_, set_ready_fails inst cp c i b hi⟩
  · intro c i b hi
    exact ⟨_, set_paused_fails inst cp c i b hi⟩
  · intro c h d hh
    exact ⟨_, set_deadline_fails inst cp c h d hh⟩
  · intro c h b hh
    exact ⟨_, set_deadline_paused_fails inst cp c h b hh⟩

end EmEventLoop
