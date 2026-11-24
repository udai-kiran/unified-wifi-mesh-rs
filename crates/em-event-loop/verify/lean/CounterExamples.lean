import Bridge
import Reference
import Mathlib.Order.Nat

/-! # G2 (note 0017 D4): every counter-example of the English spec is impossible

Each theorem `ce_S<rule>_<short>` takes `hops : ops.Meets` and the bad behaviour, and derives
`False` from the matching rule theorem of `Rules.lean`. The bad behaviour is stated about a
`PassTrace` (a pass followed call by call) or about single calls, in the words of the spec.
`aux_*` lemmas only count or unfold; they carry no rule. -/

namespace EmEventLoop

set_option linter.unusedSectionVars false

variable {T : Type} [LinearOrder T] {ops : Ops T} {now : T}

/-! ## Helpers -/

lemma aux_serveCount_mono (answer : Nat → Answer) (i : Nat) {m n : Nat} (h : m ≤ n) :
    serveCount answer m i ≤ serveCount answer n i := by
  induction n, h using Nat.le_induction with
  | base => exact Nat.le_refl _
  | succ n _ ih => rw [pf_serveCount_succ]; omega

lemma aux_nextHandler_zero_of_no_run (answer : Nat → Answer) :
    ∀ k, (∀ j, j < k → ∀ h, answer j ≠ .runDeadline h) → nextHandler answer k = 0 := by
  intro k
  induction k with
  | zero => intro _; rfl
  | succ k ih =>
    intro hall
    have hk0 := ih (fun j hj => hall j (by omega))
    cases hk : answer k with
    | serve i => simp [nextHandler, hk, hk0]
    | runDeadline h => exact absurd hk (hall k (by omega) h)
    | passDone => simp [nextHandler, hk, hk0]

/-! ## S1. Start -/

/-- "A new loop answers serve 0", before the first `begin_pass`. -/
theorem ce_S1_new_loop_serves_0 (hops : ops.Meets) (budgets : List Nat) (n : Nat)
    (s0 s s' : LoopState T) (h : ops.new budgets n = .ok s0) (henv : Env s0 s)
    (hserve : ops.nextTurn s = .ok (.serve 0, s')) : False := by
  have hidle := S1_next_turn_before_begin_pass hops budgets n s0 s h henv
  rw [hserve] at hidle
  simp at hidle

/-- The same, for the first answer after `begin_pass`. -/
theorem ce_S1_new_loop_serves_0_after_begin_pass (hops : ops.Meets) (budgets : List Nat)
    (n : Nat) (s0 p s' : LoopState T) (h : ops.new budgets n = .ok s0)
    (hb : ops.beginPass s0 now = .ok p) (hserve : ops.nextTurn p = .ok (.serve 0, s')) :
    False := by
  obtain ⟨s'', hdone⟩ := S1_first_answer_after_new_is_done hops budgets n s0 p h hb
  rw [hserve] at hdone
  simp at hdone

/-! ## S2. Budget -/

/-- A source with budget at most 2 is served at three different calls of one pass. -/
theorem ce_S2_served_three_times (hops : ops.Meets) (tr : PassTrace ops now) (x : Src)
    (hx : (tr.seen 0).sources[0]? = some x) (hb : x.budget = 2)
    (k0 k1 k2 : Nat) (h01 : k0 < k1) (h12 : k1 < k2)
    (a0 : tr.answer k0 = .serve 0) (a1 : tr.answer k1 = .serve 0)
    (a2 : tr.answer k2 = .serve 0) : False := by
  have hle := S2_served_at_most_budget hops tr 0 x hx (k2 + 1)
  have c0 := pf_serveCount_succ tr.answer k0 0
  have c1 := pf_serveCount_succ tr.answer k1 0
  have c2 := pf_serveCount_succ tr.answer k2 0
  have m0 := aux_serveCount_mono tr.answer 0 (show k0 + 1 ≤ k1 by omega)
  have m1 := aux_serveCount_mono tr.answer 0 (show k1 + 1 ≤ k2 by omega)
  simp [a0, a1, a2] at c0 c1 c2
  omega

/-! ## S3. Order and shape -/

theorem ce_S3_serve_1_then_serve_0 (hops : ops.Meets) (tr : PassTrace ops now) (j k : Nat)
    (hjk : j < k) (hj : tr.answer j = .serve 1) (hk : tr.answer k = .serve 0) : False := by
  have := S3_source_numbers_never_go_down hops tr j k 1 0 hjk hj hk
  omega

theorem ce_S3_run_deadline_then_serve (hops : ops.Meets) (tr : PassTrace ops now) (j k : Nat)
    (hjk : j < k) (hj : tr.answer j = .runDeadline 0) (hk : tr.answer k = .serve 1) : False := by
  refine S3_no_serve_after_a_non_serve hops tr j k hjk ?_ ⟨1, hk⟩
  rintro ⟨i, hi⟩
  rw [hj] at hi
  cases hi

theorem ce_S3_run_deadline_2_then_1 (hops : ops.Meets) (tr : PassTrace ops now) (j k : Nat)
    (hjk : j < k) (hj : tr.answer j = .runDeadline 2) (hk : tr.answer k = .runDeadline 1) :
    False := by
  have := S3_handler_numbers_strictly_go_up hops tr j k 2 1 hjk hj hk
  omega

/-! ## S4. Checked at each answer -/

/-- The application pauses source 1 (a real `set_paused` call), and the next answer is
"serve 1". -/
theorem ce_S4_paused_between_answers_then_served (hops : ops.Meets) (tr : PassTrace ops now)
    (c : Nat) (hset : ops.setPaused (tr.after c) 1 true = .ok (tr.seen (c + 1)))
    (hk : tr.answer (c + 1) = .serve 1) : False := by
  obtain ⟨hlt, _⟩ := pf_set_paused hops hset
  obtain ⟨y, hy⟩ : ∃ y, (tr.after c).sources[1]? = some y := ⟨_, List.getElem?_eq_getElem hlt⟩
  have hnew := (S17_set_paused_changes_only_its_target hops _ _ 1 true hset y hy).1
  obtain ⟨x, hx, -, hp, -⟩ := S4_served_source_was_eligible hops tr (c + 1) 1 hk
  rw [hnew] at hx
  obtain rfl := Option.some.inj hx
  simp at hp

/-! ## S5. Paused means never -/

theorem ce_S5_served_while_paused (hops : ops.Meets) (tr : PassTrace ops now) (n k : Nat)
    (hk : k < n)
    (hpaused : ∀ k, k < n → ∃ x, (tr.seen k).sources[0]? = some x ∧ x.paused = true)
    (hserve : tr.answer k = .serve 0) : False :=
  S5_paused_source_never_served hops tr 0 n hpaused k hk hserve

theorem ce_S5_handler_ran_while_paused (hops : ops.Meets) (tr : PassTrace ops now) (n k : Nat)
    (hk : k < n)
    (hpaused : ∀ k, k < n → ∃ x, (tr.seen k).handlers[0]? = some x ∧ x.paused = true)
    (hrun : tr.answer k = .runDeadline 0) : False :=
  S5_paused_handler_never_runs hops tr 0 n hpaused k hk hrun

/-! ## S6. Sources are not skipped -/

/-- Serve 0, then serve 2, while source 1 is ready, not paused and has budget at call 1. -/
theorem ce_S6_skipped_source_1 (hops : ops.Meets) (tr : PassTrace ops now)
    (a0 : tr.answer 0 = .serve 0) (a1 : tr.answer 1 = .serve 2) (x : Src)
    (hx : (tr.seen 1).sources[1]? = some x) (hr : x.ready = true) (hp : x.paused = false)
    (hb : 0 < x.budget) : False := by
  have hpos : curPos tr.answer 1 ≤ 1 := by simp [curPos, a0]
  have hskip := S6_no_eligible_source_skipped hops tr 1 2 a1 1 hpos (by omega)
  have hcount : serveCount tr.answer 1 1 = 0 := by simp [serveCount, a0]
  exact hskip ⟨x, hx, hr, hp, by omega⟩

/-! ## S7. Serving clears ready -/

/-- Serve 0 twice, and between the two answers the application never sets the ready flag of
source 0 from false to true. -/
theorem ce_S7_served_twice_without_ready_again (hops : ops.Meets) (tr : PassTrace ops now)
    (i k m : Nat) (hkm : k < m) (hk : tr.answer k = .serve i) (hm : tr.answer m = .serve i)
    (hkeep : ∀ c, k ≤ c → c < m → ∀ x y, (tr.after c).sources[i]? = some x →
      (tr.seen (c + 1)).sources[i]? = some y → y.ready = x.ready) : False := by
  obtain ⟨c, hc1, hc2, ⟨x, hx, hxr⟩, ⟨y, hy, hyr⟩⟩ :=
    S7_served_again_only_if_made_ready_again hops tr i k m hkm hk hm
  have := hkeep c hc1 hc2 x y hx hy
  rw [hxr, hyr] at this
  cases this

/-! ## S8. Deadlines are not skipped -/

/-- No deadline has run yet, handler 0 is due, and the answer is "run deadline 1". -/
theorem ce_S8_skipped_handler_0 (hops : ops.Meets) (tr : PassTrace ops now) (k : Nat)
    (hnone : ∀ j, j < k → ∀ h, tr.answer j ≠ .runDeadline h) (hdue : DueAt tr k 0)
    (hk : tr.answer k = .runDeadline 1) : False := by
  have hnext := aux_nextHandler_zero_of_no_run tr.answer k hnone
  exact S8_no_due_handler_jumped_over hops tr k 1 hk 0 (by omega) (by omega) hdue

/-- "Run deadline 0" twice in one pass (no assumption about the pass being finished: the
rule theorem used is S9's "at most once per pass", the same sentence as S8's "exactly once"). -/
theorem ce_S8_run_deadline_0_twice (hops : ops.Meets) (tr : PassTrace ops now) (k m : Nat)
    (hkm : k < m) (hk : tr.answer k = .runDeadline 0) (hm : tr.answer m = .runDeadline 0) :
    False :=
  S9_handler_runs_at_most_once_per_pass hops tr 0 k m hkm hk hm

/-- The same, in a finished pass in which handler 0 stays due until its turn, using the S8
theorem "runs exactly once". -/
theorem ce_S8_run_deadline_0_twice_in_finished_pass (hops : ops.Meets) (tr : PassTrace ops now)
    (m : Nat) (hm : tr.answer m = .passDone) (hfirst : ∀ j, j < m → tr.answer j ≠ .passDone)
    (hdue : ∀ k, k ≤ m → (∀ j, j < k → tr.answer j ≠ .runDeadline 0) → DueAt tr k 0)
    (k l : Nat) (hkl : k < l) (hk : tr.answer k = .runDeadline 0)
    (hl : tr.answer l = .runDeadline 0) : False := by
  obtain ⟨c, -, -, huniq⟩ := S8_due_handler_runs_exactly_once hops tr 0 m hm hfirst hdue
  have h1 := huniq k hk
  have h2 := huniq l hl
  omega

/-! ## S9. Running a deadline clears it -/

/-- Handler 0 is the only handler, with deadline `d`. It runs, and `next_deadline()` on the
state it leaves still gives `d`. -/
theorem ce_S9_old_deadline_still_given (hops : ops.Meets) (s s' : LoopState T) (x : Hdl T)
    (d : T) (hsingle : s.handlers = [x]) (_hd : x.deadline = some d)
    (hrun : ops.nextTurn s = .ok (.runDeadline 0, s'))
    (hnext : ops.nextDeadline s' = .ok (some d)) : False := by
  obtain ⟨h0, hother⟩ :=
    S9_running_clears_deadline_and_nothing_else hops s s' 0 hrun x (by simp [hsingle])
  obtain ⟨⟨y, hy, -, hyd⟩, -⟩ := S12_some_is_the_earliest_armed_deadline hops s' d hnext
  obtain ⟨k, hk⟩ := List.mem_iff_getElem?.1 hy
  cases k with
  | zero =>
    rw [h0] at hk
    obtain rfl := Option.some.inj hk
    simp at hyd
  | succ k =>
    rw [hother (k + 1) (by omega), hsingle] at hk
    simp at hk

/-! ## S10. Time is fixed per pass -/

/-- A deadline later than the time of the pass runs (the spec: 6 in a pass that began at 5). -/
theorem ce_S10_later_deadline_runs (hops : ops.Meets) (tr : PassTrace ops now) (k h : Nat)
    (hk : tr.answer k = .runDeadline h) (x : Hdl T) (d : T)
    (hx : (tr.seen k).handlers[h]? = some x) (hd : x.deadline = some d) (hlater : now < d) :
    False :=
  not_le.2 hlater (S10_run_deadline_not_later_than_now hops tr k h hk x d hx hd)

/-! ## S11. A pass always ends -/

/-- Budgets 2 and 1, one handler, and five answers without "pass done". -/
theorem ce_S11_five_answers_without_pass_done (hops : ops.Meets) (tr : PassTrace ops now)
    (hbudgets : (tr.seen 0).sources.map (·.budget) = [2, 1])
    (hhandlers : (tr.seen 0).handlers.length = 1)
    (hnot : ∀ k, k < 5 → tr.answer k ≠ .passDone) : False := by
  obtain ⟨m, hm, hdone⟩ := S11_pass_ends_within_bound hops tr
  rw [hbudgets, hhandlers] at hm
  simp at hm
  exact hnot m (by omega) hdone

/-! ## S12. Next deadline -/

/-- Handlers at 7, 3 and 1 (the last one paused): `next_deadline()` gives 1. -/
theorem ce_S12_paused_handler_counted {ops : Ops Nat} (hops : ops.Meets) (s : LoopState Nat)
    (hhandlers : s.handlers = [⟨some 7, false⟩, ⟨some 3, false⟩, ⟨some 1, true⟩])
    (h : ops.nextDeadline s = .ok (some 1)) : False := by
  obtain ⟨⟨x, hx, hp, hd⟩, -⟩ := S12_some_is_the_earliest_armed_deadline hops s 1 h
  rw [hhandlers] at hx
  simp at hx
  rcases hx with rfl | rfl | rfl <;> simp at hp hd

/-- The same handlers: `next_deadline()` gives 7, which is not the earliest. -/
theorem ce_S12_not_the_earliest {ops : Ops Nat} (hops : ops.Meets) (s : LoopState Nat)
    (hhandlers : s.handlers = [⟨some 7, false⟩, ⟨some 3, false⟩, ⟨some 1, true⟩])
    (h : ops.nextDeadline s = .ok (some 7)) : False := by
  obtain ⟨-, hmin⟩ := S12_some_is_the_earliest_armed_deadline hops s 7 h
  have := hmin ⟨some 3, false⟩ (by rw [hhandlers]; simp) rfl 3 rfl
  omega

/-- "None" while handler 1 has a deadline and is not paused. -/
theorem ce_S12_none_while_armed (hops : ops.Meets) (s : LoopState T) (x : Hdl T) (d : T)
    (hx : s.handlers[1]? = some x) (hp : x.paused = false) (hd : x.deadline = some d)
    (h : ops.nextDeadline s = .ok none) : False := by
  have := S12_none_means_no_armed_handler hops s h x (List.mem_of_getElem? hx) hp
  rw [hd] at this
  cases this

/-! ## S13. More work -/

/-- "Yes" when the only ready source is paused. -/
theorem ce_S13_paused_source_counted (hops : ops.Meets) (s : LoopState T)
    (honly : ∀ x ∈ s.sources, x.ready = true → x.paused = true)
    (h : ops.hasReadySource s = .ok true) : False := by
  obtain ⟨x, hx, hr, hp, -⟩ := (S13_has_ready_source_exactly_when hops s true h).1 rfl
  rw [honly x hx hr] at hp
  cases hp

/-! ## S14. Nothing else changes -/

/-- Answering "serve 0" clears the ready flag of source 1. -/
theorem ce_S14_serve_0_clears_source_1 (hops : ops.Meets) (s s' : LoopState T)
    (h : ops.nextTurn s = .ok (.serve 0, s')) (x y : Src) (hx : s.sources[1]? = some x)
    (hy : s'.sources[1]? = some y) (hchanged : y.ready ≠ x.ready) : False := by
  have hsame := S14_next_turn_keeps_other_sources hops s s' _ h 1 (by simp)
  rw [hx, hy] at hsame
  exact hchanged (congrArg Src.ready (Option.some.inj hsame))

/-! ## S16. Starting again -/

/-- After the second `begin_pass`, the budget used in the old pass is still counted. -/
theorem ce_S16_old_budget_kept (hops : ops.Meets) (s p : LoopState T)
    (h : ops.beginPass s now = .ok p) (position : Nat) (used : Nat → Nat)
    (hphase : p.phase = .serving now position used) (hused : used 0 ≠ 0) : False := by
  have hrestart := (S16_begin_pass_restarts hops s p h).1
  rw [hphase] at hrestart
  injection hrestart with _ _ hu
  exact hused (congrFun hu 0)

/-- After the second `begin_pass`, a ready source with budget gets no turn (the first answer
is not "serve"). The old pass is whatever state `tr.start` is in. -/
theorem ce_S16_source_gets_no_turn (hops : ops.Meets) (tr : PassTrace ops now) (x : Src)
    (hx : (tr.seen 0).sources[0]? = some x) (hr : x.ready = true) (hp : x.paused = false)
    (hb : 0 < x.budget) (hnot_serve : ¬ IsServe (tr.answer 0)) : False := by
  have hnone := S6_first_non_serve_leaves_no_eligible_source hops tr 0
    (fun j hj => absurd hj (Nat.not_lt_zero j)) hnot_serve 0 (by simp [curPos])
  exact hnone ⟨x, hx, hr, hp, by simp [serveCount, hb]⟩

/-! ## S17. Setters change only their target -/

/-- `set_ready(1, true)` clears the pause of source 0. -/
theorem ce_S17_set_ready_clears_other_pause (hops : ops.Meets) (s s' : LoopState T)
    (h : ops.setReady s 1 true = .ok s') (x y : Src) (hx : s.sources[0]? = some x)
    (hy : s'.sources[0]? = some y) (hpaused : x.paused = true) (hcleared : y.paused = false) :
    False := by
  obtain ⟨hlt, -⟩ := pf_set_ready hops h
  obtain ⟨x1, hx1⟩ : ∃ x1, s.sources[1]? = some x1 := ⟨_, List.getElem?_eq_getElem hlt⟩
  have hsame := (S17_set_ready_changes_only_its_target hops s s' 1 true h x1 hx1).2.1 0
    (by omega)
  rw [hx, hy] at hsame
  have hyx := Option.some.inj hsame
  rw [hyx, hpaused] at hcleared
  cases hcleared

/-- `set_ready(1, true)` gives source 1 a different budget. -/
theorem ce_S17_set_ready_changes_budget (hops : ops.Meets) (s s' : LoopState T)
    (h : ops.setReady s 1 true = .ok s') (x y : Src) (hx : s.sources[1]? = some x)
    (hy : s'.sources[1]? = some y) (hchanged : y.budget ≠ x.budget) : False := by
  have hnew := (S17_set_ready_changes_only_its_target hops s s' 1 true h x hx).1
  rw [hy] at hnew
  have hyx := Option.some.inj hnew
  rw [hyx] at hchanged
  exact hchanged rfl

/-- `set_ready(1, true)` changes the pass progress (a fresh budget in this pass is part of it). -/
theorem ce_S17_set_ready_changes_pass_progress (hops : ops.Meets) (s s' : LoopState T)
    (h : ops.setReady s 1 true = .ok s') (x : Src) (hx : s.sources[1]? = some x)
    (hchanged : s'.phase ≠ s.phase) : False :=
  hchanged (S17_set_ready_changes_only_its_target hops s s' 1 true h x hx).2.2.2

end EmEventLoop
