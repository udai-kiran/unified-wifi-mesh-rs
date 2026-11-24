import Abstraction
import Reference
import Bridge

/-! # Refinement: the extracted Rust code does what the step spec says

`abs` (in `Abstraction.lean`) maps a state of the extracted `LoopCore` to the `LoopState` of
`StepSpec.lean`. For every operation, the extracted function never diverges and, mapped through
`abs` (`ok` to `ok`, `fail` to `panic`), its result satisfies the matching `*Step` relation. There
is no invariant on the Rust state: the theorems hold for every state of the extracted types. -/

open Aeneas Aeneas.Std Result ControlFlow

namespace EmEventLoop

set_option linter.unusedSectionVars false
set_option linter.unusedVariables false
set_option linter.unusedSimpArgs false

/-! ## Lemmas about `abs` -/

theorem absUsed_congr_phase {T : Type} {l l' : List em_event_loop.SourceState}
    (h : absUsed l' = absUsed l) (ph : em_event_loop.Phase T) :
    absPhase l' ph = absPhase l ph := by
  cases ph <;> simp [absPhase, h]

/-- Setting one source to a new value with the same `used` does not change `absUsed`. -/
theorem absUsed_set_same {l : List em_event_loop.SourceState} {i : Nat}
    {x x' : em_event_loop.SourceState} (hx : l[i]? = some x) (hu : x'.used = x.used) :
    absUsed (l.set i x') = absUsed l := by
  funext k
  unfold absUsed
  by_cases hk : k = i
  · subst hk
    obtain ⟨hlt, hkx⟩ := List.getElem?_eq_some_iff.1 hx
    simp [hlt, hu, hkx]
  · simp [hk]

/-- Changing one list entry by `g`-image `f` is `modifyAt` on the mapped list. -/
theorem map_set_eq_modifyAt {α β : Type _} (g : α → β) (l : List α) (i : Nat) (x x' : α)
    (f : β → β) (hx : l[i]? = some x) (h : g x' = f (g x)) :
    (l.set i x').map g = modifyAt (l.map g) i f := by
  apply List.ext_getElem?
  intro k
  rw [pf_modifyAt_getElem?]
  by_cases hk : k = i
  · subst hk
    obtain ⟨hlt, hkx⟩ := List.getElem?_eq_some_iff.1 hx
    simp [hlt, hkx, h]
  · simp [hk, List.getElem?_map]

theorem abs_with_sources {T : Type} (self : em_event_loop.LoopCore T)
    (v : alloc.vec.Vec em_event_loop.SourceState)
    (hu : absUsed v.val = absUsed self.sources.val) :
    abs { self with sources := v } = { abs self with sources := v.val.map absSrc } := by
  unfold abs
  simp only [absUsed_congr_phase hu]

theorem abs_with_handlers {T : Type} (self : em_event_loop.LoopCore T)
    (v : alloc.vec.Vec (em_event_loop.HandlerState T)) :
    abs { self with handlers := v } = { abs self with handlers := v.val.map absHdl } := rfl

theorem index_mut_usize_oob {α : Type _} (v : alloc.vec.Vec α) (i : Usize)
    (h : v.val.length ≤ i.val) :
    v.index_mut_usize i = Result.fail Error.arrayOutOfBounds := by
  have : v.val[i.val]? = none := List.getElem?_eq_none h
  simp [alloc.vec.Vec.index_mut_usize, alloc.vec.Vec.index_usize, this]

variable {T : Type} [LinearOrder T] (inst : core.cmp.Ord T) (cp : core.marker.Copy T)

theorem spec_exists_ok {α : Type _} {m : Result α} {p : α → Prop} (h : m ⦃ v => p v ⦄) :
    ∃ v, m = Result.ok v ∧ p v := by
  rcases hm : m.match with a | _ | ⟨e, k⟩
  · have := Result.match.isOk.1 hm
    subst this
    exact ⟨a, rfl, (WP.spec_ok _).1 h⟩
  · have := Result.match.isDiv.1 hm
    subst this
    exact ((WP.spec_div).1 h).elim
  · have := Result.match.isVis.1 hm
    subst this
    exact ((WP.spec_vis _ _).1 h).elim

/-! ## The setters -/

theorem set_ready_spec (self : em_event_loop.LoopCore T) (source : Usize) (ready : Bool)
    (h : source.val < self.sources.val.length) :
    em_event_loop.LoopCore.set_ready inst cp self source ready ⦃ r =>
      abs r = { abs self with
        sources := modifyAt (abs self).sources source.val (fun x => { x with ready := ready }) } ⦄ := by
  unfold em_event_loop.LoopCore.set_ready
  step*
  have hx : self.sources.val[source.val]? = some ss := by simp [ss_post, h]
  rw [abs_with_sources]
  · subst index_mut_back_post
    simp only [alloc.vec.Vec.set_val_eq]
    exact congrArg (fun l => { abs self with sources := l })
      (map_set_eq_modifyAt absSrc _ _ ss { ss with ready := ready }
        (fun x => { x with ready := ready }) hx rfl)
  · subst index_mut_back_post
    simp only [alloc.vec.Vec.set_val_eq]
    exact absUsed_set_same hx rfl

theorem set_ready_refines (self : em_event_loop.LoopCore T) (source : Usize) (ready : Bool) :
    ∃ o, OutcomeOf abs (em_event_loop.LoopCore.set_ready inst cp self source ready) o ∧
      SetReadyStep (abs self) source.val ready o := by
  unfold SetReadyStep
  by_cases h : source.val < self.sources.val.length
  · obtain ⟨r, hr, hp⟩ := spec_exists_ok (set_ready_spec inst cp self source ready h)
    refine ⟨_, Or.inl ⟨r, hr, rfl⟩, fun _ => by rw [hp], fun hge => ?_⟩
    simp [abs] at hge
    omega
  · have hfail : em_event_loop.LoopCore.set_ready inst cp self source ready =
        Result.fail Error.arrayOutOfBounds := by
      unfold em_event_loop.LoopCore.set_ready
      rw [alloc.vec.Vec.index_mut_slice_index, index_mut_usize_oob _ _ (by omega)]
      simp
    refine ⟨_, Or.inr ⟨_, hfail, rfl⟩, fun hlt => ?_, fun _ => rfl⟩
    simp [abs] at hlt
    omega

theorem set_paused_spec (self : em_event_loop.LoopCore T) (source : Usize) (paused : Bool)
    (h : source.val < self.sources.val.length) :
    em_event_loop.LoopCore.set_paused inst cp self source paused ⦃ r =>
      abs r = { abs self with
        sources := modifyAt (abs self).sources source.val (fun x => { x with paused := paused }) } ⦄ := by
  unfold em_event_loop.LoopCore.set_paused
  step*
  have hx : self.sources.val[source.val]? = some ss := by simp [ss_post, h]
  rw [abs_with_sources]
  · subst index_mut_back_post
    simp only [alloc.vec.Vec.set_val_eq]
    exact congrArg (fun l => { abs self with sources := l })
      (map_set_eq_modifyAt absSrc _ _ ss { ss with paused := paused }
        (fun x => { x with paused := paused }) hx rfl)
  · subst index_mut_back_post
    simp only [alloc.vec.Vec.set_val_eq]
    exact absUsed_set_same hx rfl

theorem set_paused_refines (self : em_event_loop.LoopCore T) (source : Usize) (paused : Bool) :
    ∃ o, OutcomeOf abs (em_event_loop.LoopCore.set_paused inst cp self source paused) o ∧
      SetPausedStep (abs self) source.val paused o := by
  unfold SetPausedStep
  by_cases h : source.val < self.sources.val.length
  · obtain ⟨r, hr, hp⟩ := spec_exists_ok (set_paused_spec inst cp self source paused h)
    refine ⟨_, Or.inl ⟨r, hr, rfl⟩, fun _ => by rw [hp], fun hge => ?_⟩
    simp [abs] at hge
    omega
  · have hfail : em_event_loop.LoopCore.set_paused inst cp self source paused =
        Result.fail Error.arrayOutOfBounds := by
      unfold em_event_loop.LoopCore.set_paused
      rw [alloc.vec.Vec.index_mut_slice_index, index_mut_usize_oob _ _ (by omega)]
      simp
    refine ⟨_, Or.inr ⟨_, hfail, rfl⟩, fun hlt => ?_, fun _ => rfl⟩
    simp [abs] at hlt
    omega

theorem set_deadline_spec (self : em_event_loop.LoopCore T) (handler : Usize) (deadline : Option T)
    (h : handler.val < self.handlers.val.length) :
    em_event_loop.LoopCore.set_deadline inst cp self handler deadline ⦃ r =>
      abs r = { abs self with
        handlers := modifyAt (abs self).handlers handler.val (fun x => { x with deadline := deadline }) } ⦄ := by
  unfold em_event_loop.LoopCore.set_deadline
  step*
  have hx : self.handlers.val[handler.val]? = some hs := by simp [hs_post, h]
  rw [abs_with_handlers]
  subst index_mut_back_post
  simp only [alloc.vec.Vec.set_val_eq]
  exact congrArg (fun l => { abs self with handlers := l })
    (map_set_eq_modifyAt absHdl _ _ hs { hs with deadline := deadline }
      (fun x => { x with deadline := deadline }) hx rfl)

theorem set_deadline_refines (self : em_event_loop.LoopCore T) (handler : Usize) (deadline : Option T) :
    ∃ o, OutcomeOf abs (em_event_loop.LoopCore.set_deadline inst cp self handler deadline) o ∧
      SetDeadlineStep (abs self) handler.val deadline o := by
  unfold SetDeadlineStep
  by_cases h : handler.val < self.handlers.val.length
  · obtain ⟨r, hr, hp⟩ := spec_exists_ok (set_deadline_spec inst cp self handler deadline h)
    refine ⟨_, Or.inl ⟨r, hr, rfl⟩, fun _ => by rw [hp], fun hge => ?_⟩
    simp [abs] at hge
    omega
  · have hfail : em_event_loop.LoopCore.set_deadline inst cp self handler deadline =
        Result.fail Error.arrayOutOfBounds := by
      unfold em_event_loop.LoopCore.set_deadline
      rw [alloc.vec.Vec.index_mut_slice_index, index_mut_usize_oob _ _ (by omega)]
      simp
    refine ⟨_, Or.inr ⟨_, hfail, rfl⟩, fun hlt => ?_, fun _ => rfl⟩
    simp [abs] at hlt
    omega

theorem set_deadline_paused_spec (self : em_event_loop.LoopCore T) (handler : Usize) (paused : Bool)
    (h : handler.val < self.handlers.val.length) :
    em_event_loop.LoopCore.set_deadline_paused inst cp self handler paused ⦃ r =>
      abs r = { abs self with
        handlers := modifyAt (abs self).handlers handler.val (fun x => { x with paused := paused }) } ⦄ := by
  unfold em_event_loop.LoopCore.set_deadline_paused
  step*
  have hx : self.handlers.val[handler.val]? = some hs := by simp [hs_post, h]
  rw [abs_with_handlers]
  subst index_mut_back_post
  simp only [alloc.vec.Vec.set_val_eq]
  exact congrArg (fun l => { abs self with handlers := l })
    (map_set_eq_modifyAt absHdl _ _ hs { hs with paused := paused }
      (fun x => { x with paused := paused }) hx rfl)

theorem set_deadline_paused_refines (self : em_event_loop.LoopCore T) (handler : Usize) (paused : Bool) :
    ∃ o, OutcomeOf abs (em_event_loop.LoopCore.set_deadline_paused inst cp self handler paused) o ∧
      SetDeadlinePausedStep (abs self) handler.val paused o := by
  unfold SetDeadlinePausedStep
  by_cases h : handler.val < self.handlers.val.length
  · obtain ⟨r, hr, hp⟩ := spec_exists_ok (set_deadline_paused_spec inst cp self handler paused h)
    refine ⟨_, Or.inl ⟨r, hr, rfl⟩, fun _ => by rw [hp], fun hge => ?_⟩
    simp [abs] at hge
    omega
  · have hfail : em_event_loop.LoopCore.set_deadline_paused inst cp self handler paused =
        Result.fail Error.arrayOutOfBounds := by
      unfold em_event_loop.LoopCore.set_deadline_paused
      rw [alloc.vec.Vec.index_mut_slice_index, index_mut_usize_oob _ _ (by omega)]
      simp
    refine ⟨_, Or.inr ⟨_, hfail, rfl⟩, fun hlt => ?_, fun _ => rfl⟩
    simp [abs] at hlt
    omega

/-! ## `begin_pass` -/

/-- A source with its used budget reset. -/
def zeroUsed (s : em_event_loop.SourceState) : em_event_loop.SourceState :=
  { s with used := 0#usize }

theorem begin_pass_loop_spec (self0 : em_event_loop.LoopCore T) :
    em_event_loop.LoopCore.begin_pass_loop self0 0#usize ⦃ r =>
      r.1.val = self0.sources.val.map zeroUsed ∧ r.2 = self0.handlers ⦄ := by
  unfold em_event_loop.LoopCore.begin_pass_loop
  apply loop.spec_decr_nat (measure := fun x => self0.sources.val.length - x.2.val)
    (inv := fun x => x.1.handlers = self0.handlers ∧
      x.1.sources.val.length = self0.sources.val.length ∧
      ∀ k, x.1.sources.val[k]? =
        (self0.sources.val[k]?).map (fun s => if k < x.2.val then zeroUsed s else s))
  · rintro ⟨self', index⟩ ⟨hh, hl, hk⟩
    dsimp only at hh hl hk
    unfold em_event_loop.LoopCore.begin_pass_loop.body
    simp only [alloc.vec.Vec.len]
    split
    · rename_i hlt
      have hlt' : index.val < self0.sources.val.length := by scalar_tac
      have hlen : index.val < self'.sources.val.length := by omega
      step as ⟨ ss, index_mut_back, hss, hback ⟩
      step as ⟨ index1, hi1 ⟩
      subst hback
      refine ⟨hh, by simp [hl], fun k => ?_, by omega⟩
      simp only [alloc.vec.Vec.set_val_eq, hi1]
      by_cases hki : k = index.val
      · subst hki
        have heq : self'.sources.val[index.val] = self0.sources.val[index.val] := by
          have h1 := hk index.val
          rw [List.getElem?_eq_getElem hlen, List.getElem?_eq_getElem hlt'] at h1
          simpa using h1
        simp [hlen, hlt', zeroUsed, hss, heq]
      · simp [hki, hk k, show k ≤ index.val ↔ k < index.val by omega]
    · rename_i hge
      have : index.val ≥ self0.sources.val.length := by scalar_tac
      rw [WP.spec_ok]
      refine ⟨?_, hh⟩
      apply List.ext_getElem?
      intro k
      rw [hk k, List.getElem?_map]
      by_cases hk' : k < self0.sources.val.length
      · simp [hk', show k < index.val by omega]
      · simp [List.getElem?_eq_none (show self0.sources.val.length ≤ k by omega)]
  · refine ⟨rfl, rfl, fun k => ?_⟩
    cases self0.sources.val[k]? <;> simp

theorem begin_pass_refines (self : em_event_loop.LoopCore T) (now : T) :
    ∃ o, OutcomeOf abs (em_event_loop.LoopCore.begin_pass inst cp self now) o ∧
      BeginPassStep (abs self) now o := by
  obtain ⟨⟨v, v1⟩, hr, hv, hv1⟩ := spec_exists_ok (begin_pass_loop_spec self)
  have hbp : em_event_loop.LoopCore.begin_pass inst cp self now =
      Result.ok { sources := v, handlers := v1, phase := em_event_loop.Phase.Serving now 0#usize } := by
    unfold em_event_loop.LoopCore.begin_pass
    rw [hr]
    simp
  refine ⟨_, Or.inl ⟨_, hbp, rfl⟩, ?_⟩
  simp only at hv hv1
  subst hv1
  have hused : absUsed (self.sources.val.map zeroUsed) = fun _ => 0 := by
    funext i
    unfold absUsed
    rw [List.getElem?_map]
    cases self.sources.val[i]? <;> simp [zeroUsed]
  unfold BeginPassStep abs
  congr 1
  simp [hv, absPhase, hused, List.map_map, Function.comp_def, zeroUsed, absSrc]

/-! ## `new` -/

/-- A source as `new` makes it. -/
def freshSource (budget : Usize) : em_event_loop.SourceState :=
  { budget := budget, used := 0#usize, ready := false, paused := false }

/-- A handler as `new` makes it. -/
def freshHandler : em_event_loop.HandlerState T := { deadline := none, paused := false }

theorem new_sources_spec (budgets : Slice Usize) :
    em_event_loop.LoopCore.new_loop0 budgets
      (alloc.vec.Vec.new em_event_loop.SourceState) 0#usize ⦃ ss =>
      ss.val = budgets.val.map freshSource ⦄ := by
  unfold em_event_loop.LoopCore.new_loop0
  apply loop.spec_decr_nat (measure := fun x => budgets.length - x.2.val)
    (inv := fun x => x.2.val ≤ budgets.length ∧
      x.1.val = (budgets.val.take x.2.val).map freshSource)
  · rintro ⟨ss, index⟩ ⟨h1, h2⟩
    dsimp only at h1 h2
    unfold em_event_loop.LoopCore.new_loop0.body
    simp only [Slice.len]
    split
    · rename_i hlt
      have hlt' : index.val < budgets.length := by scalar_tac
      have hlen : ss.length ≤ index.val := by
        have := congrArg List.length h2
        simp at this
        simp [alloc.vec.Vec.length]
        omega
      step as ⟨ b, hb ⟩
      step as ⟨ ss1, hss1 ⟩
      step as ⟨ index1, hi1 ⟩
      refine ⟨by omega, ?_, by omega⟩
      rw [hss1, hi1]
      have hb' : index.val < budgets.val.length := hlt'
      show (↑ss : List em_event_loop.SourceState) ++ _ = _
      rw [h2, List.take_add_one, List.getElem?_eq_getElem hb', List.map_append]
      simp only [Option.toList_some, List.map_cons, List.map_nil, freshSource, hb]
    · rename_i hge
      have : index.val = budgets.length := by
        have := h1; scalar_tac
      rw [WP.spec_ok]
      simp [h2, this]
  · refine ⟨by simp, by simp [alloc.vec.Vec.new]⟩

theorem new_handlers_spec (handler_count : Usize) :
    em_event_loop.LoopCore.new_loop1 (T := T) handler_count
      (alloc.vec.Vec.new (em_event_loop.HandlerState T)) 0#usize ⦃ hs =>
      hs.val = List.replicate handler_count.val freshHandler ⦄ := by
  unfold em_event_loop.LoopCore.new_loop1
  apply loop.spec_decr_nat (measure := fun x => handler_count.val - x.2.val)
    (inv := fun x => x.2.val ≤ handler_count.val ∧
      x.1.val = List.replicate x.2.val freshHandler)
  · rintro ⟨hs, count⟩ ⟨h1, h2⟩
    unfold em_event_loop.LoopCore.new_loop1.body
    dsimp only at h1 h2 ⊢
    split
    · rename_i hlt
      have hlt' : count.val < handler_count.val := by scalar_tac
      have hlen : hs.length ≤ count.val := by
        have := congrArg List.length h2
        simp at this
        simp [alloc.vec.Vec.length]
        omega
      have hmax : hs.val.length < Usize.max := by
        have h3 : hs.val.length ≤ count.val := hlen
        scalar_tac
      step as ⟨ hs1, hhs1 ⟩
      step as ⟨ count1, hc1 ⟩
      refine ⟨by omega, ?_, by omega⟩
      rw [hhs1, hc1]
      rw [h2, List.replicate_succ']
      rfl
    · rename_i hge
      rw [WP.spec_ok]
      have : count.val = handler_count.val := by
        have := h1; scalar_tac
      simp [h2, this]
  · refine ⟨by simp, by simp [alloc.vec.Vec.new]⟩

theorem new_refines (budgets : Slice Usize) (handler_count : Usize) :
    ∃ o, OutcomeOf abs (em_event_loop.LoopCore.new inst cp budgets handler_count) o ∧
      NewStep (T := T) (budgets.val.map (·.val)) handler_count.val o := by
  obtain ⟨sources, hs, hsv⟩ := spec_exists_ok (new_sources_spec budgets)
  obtain ⟨handlers, hh, hhv⟩ := spec_exists_ok (new_handlers_spec (T := T) handler_count)
  have hnew : em_event_loop.LoopCore.new inst cp budgets handler_count =
      Result.ok { sources := sources, handlers := handlers, phase := em_event_loop.Phase.Done } := by
    unfold em_event_loop.LoopCore.new
    rw [hs]
    simp only [bind_ok]
    rw [hh]
    simp
  refine ⟨_, Or.inl ⟨_, hnew, rfl⟩, ?_⟩
  unfold NewStep abs
  congr 1
  simp [hsv, hhv, List.map_map, Function.comp_def, freshSource, absSrc, freshHandler, absHdl,
    absPhase]

/-! ## `has_ready_source` -/

/-- Ready, not paused, budget above 0. -/
def HasWork (y : em_event_loop.SourceState) : Prop :=
  y.ready = true ∧ y.paused = false ∧ 0 < y.budget.val

theorem source_has_work_spec (self : em_event_loop.LoopCore T) (index : Usize)
    (h : index.val < self.sources.val.length) :
    em_event_loop.LoopCore.source_has_work inst cp self index ⦃ b =>
      (b = true ↔ HasWork self.sources.val[index.val]) ⦄ := by
  unfold em_event_loop.LoopCore.source_has_work
  step as ⟨ source, hsource ⟩
  subst hsource
  unfold HasWork
  cases hr : self.sources.val[index.val].ready <;> cases hp : self.sources.val[index.val].paused <;>
    simp [WP.spec_ok, hr, hp]

theorem has_ready_source_loop_spec (self : em_event_loop.LoopCore T) :
    em_event_loop.LoopCore.has_ready_source_loop inst cp self 0#usize ⦃ b =>
      (b = true ↔ ∃ y ∈ self.sources.val, HasWork y) ⦄ := by
  unfold em_event_loop.LoopCore.has_ready_source_loop
  apply loop.spec_decr_nat (measure := fun index => self.sources.val.length - index.val)
    (inv := fun index => ∀ i, i < index.val → ∀ y, self.sources.val[i]? = some y → ¬ HasWork y)
  · intro index hinv
    unfold em_event_loop.LoopCore.has_ready_source_loop.body
    simp only [alloc.vec.Vec.len]
    split
    · rename_i hlt
      have hlt' : index.val < self.sources.val.length := by scalar_tac
      apply WP.spec_bind (source_has_work_spec inst cp self index hlt')
      intro b hb
      split
      · rename_i hbt
        rw [WP.spec_ok]
        subst hbt
        exact ⟨fun _ => ⟨_, List.getElem_mem hlt', hb.1 rfl⟩, fun _ => rfl⟩
      · rename_i hbf
        step as ⟨ index1, hi1 ⟩
        refine ⟨fun i hi y hy => ?_, by omega⟩
        by_cases hii : i = index.val
        · subst hii
          have : y = self.sources.val[index.val] := by
            simp [List.getElem?_eq_getElem hlt'] at hy; exact hy.symm
          subst this
          intro hw
          exact hbf (hb.2 hw)
        · exact hinv i (by omega) y hy
    · rename_i hge
      rw [WP.spec_ok]
      constructor
      · intro hfalse
        exact absurd hfalse (by simp)
      · rintro ⟨y, hy, hw⟩
        obtain ⟨i, hi⟩ := List.mem_iff_getElem?.1 hy
        exact absurd hw (hinv i (by have := (List.getElem?_eq_some_iff.1 hi).1; scalar_tac) y hi)
  · intro i hi; simp at hi

theorem has_ready_source_refines (self : em_event_loop.LoopCore T) :
    ∃ o, OutcomeOf (fun b : Bool => b) (em_event_loop.LoopCore.has_ready_source inst cp self) o ∧
      HasReadySourceStep (abs self) o := by
  obtain ⟨b, hb, hbv⟩ := spec_exists_ok (has_ready_source_loop_spec inst cp self)
  have hr : em_event_loop.LoopCore.has_ready_source inst cp self = Result.ok b := hb
  refine ⟨_, Or.inl ⟨_, hr, rfl⟩, b, rfl, ?_⟩
  rw [hbv]
  unfold ReadySourceExists abs HasWork absSrc
  simp

/-! ## `next_deadline` -/

/-- Appending a handler that cannot fire does not change the earliest deadline. -/
theorem earliest_append_unarmed {l : List (Hdl T)} {o : Option T} (x : Hdl T)
    (hx : x.paused = false → x.deadline = none) (h : EarliestDeadline l o) :
    EarliestDeadline (l ++ [x]) o := by
  rcases h with ⟨ho, hall⟩ | ⟨d, ho, ⟨y, hy, hyp, hyd⟩, hmin⟩
  · left
    refine ⟨ho, fun z hz hzp => ?_⟩
    rcases List.mem_append.1 hz with hzl | hzx
    · exact hall z hzl hzp
    · rw [List.mem_singleton.1 hzx] at hzp ⊢
      exact hx hzp
  · right
    refine ⟨d, ho, ⟨y, List.mem_append_left _ hy, hyp, hyd⟩, fun z hz hzp e hze => ?_⟩
    rcases List.mem_append.1 hz with hzl | hzx
    · exact hmin z hzl hzp e hze
    · rw [List.mem_singleton.1 hzx] at hzp hze
      rw [hx hzp] at hze
      cases hze

/-- The first armed handler becomes the earliest deadline. -/
theorem earliest_append_first {l : List (Hdl T)} (x : Hdl T) (d : T)
    (hxp : x.paused = false) (hxd : x.deadline = some d) (h : EarliestDeadline l none) :
    EarliestDeadline (l ++ [x]) (some d) := by
  rcases h with ⟨-, hall⟩ | ⟨e, ho, -⟩
  · right
    refine ⟨d, rfl, ⟨x, List.mem_append_right _ (List.mem_singleton_self x), hxp, hxd⟩,
      fun z hz hzp e hze => ?_⟩
    rcases List.mem_append.1 hz with hzl | hzx
    · rw [hall z hzl hzp] at hze
      cases hze
    · rw [List.mem_singleton.1 hzx, hxd] at hze
      cases hze
      exact le_refl _
  · cases ho

/-- An armed handler that is not later than the best so far becomes the earliest. -/
theorem earliest_append_earlier {l : List (Hdl T)} (x : Hdl T) (d best : T)
    (hxp : x.paused = false) (hxd : x.deadline = some d) (hle : d ≤ best)
    (h : EarliestDeadline l (some best)) : EarliestDeadline (l ++ [x]) (some d) := by
  rcases h with ⟨ho, -⟩ | ⟨b, ho, -, hmin⟩
  · cases ho
  · cases ho
    right
    refine ⟨d, rfl, ⟨x, List.mem_append_right _ (List.mem_singleton_self x), hxp, hxd⟩,
      fun z hz hzp e hze => ?_⟩
    rcases List.mem_append.1 hz with hzl | hzx
    · exact hle.trans (hmin z hzl hzp e hze)
    · rw [List.mem_singleton.1 hzx, hxd] at hze
      cases hze
      exact le_refl _

/-- An armed handler that is later than the best so far leaves the earliest unchanged. -/
theorem earliest_append_later {l : List (Hdl T)} (x : Hdl T) (d best : T)
    (hxp : x.paused = false) (hxd : x.deadline = some d) (hlt : ¬ d ≤ best)
    (h : EarliestDeadline l (some best)) : EarliestDeadline (l ++ [x]) (some best) := by
  rcases h with ⟨ho, -⟩ | ⟨b, ho, ⟨y, hy, hyp, hyd⟩, hmin⟩
  · cases ho
  · cases ho
    right
    refine ⟨best, rfl, ⟨y, List.mem_append_left _ hy, hyp, hyd⟩, fun z hz hzp e hze => ?_⟩
    rcases List.mem_append.1 hz with hzl | hzx
    · exact hmin z hzl hzp e hze
    · rw [List.mem_singleton.1 hzx, hxd] at hze
      cases hze
      exact (not_le.1 hlt).le

theorem is_earlier_or_unset_spec (hle : ∀ a b : T, inst.partialOrdInst.le a b = Result.ok (decide (a ≤ b)))
    (earliest : Option T) (deadline : T) :
    em_event_loop.is_earlier_or_unset inst cp earliest deadline ⦃ b =>
      b = true ↔ (earliest = none ∨ ∃ best, earliest = some best ∧ deadline ≤ best) ⦄ := by
  unfold em_event_loop.is_earlier_or_unset
  cases earliest with
  | none => simp [WP.spec_ok]
  | some best => simp [hle, WP.spec_ok]

theorem next_deadline_loop_spec
    (hle : ∀ a b : T, inst.partialOrdInst.le a b = Result.ok (decide (a ≤ b)))
    (self : em_event_loop.LoopCore T) :
    em_event_loop.LoopCore.next_deadline_loop inst cp self none 0#usize ⦃ r =>
      EarliestDeadline (abs self).handlers r ⦄ := by
  unfold em_event_loop.LoopCore.next_deadline_loop
  apply loop.spec_decr_nat (measure := fun x => self.handlers.val.length - x.2.val)
    (inv := fun x => x.2.val ≤ self.handlers.val.length ∧
      EarliestDeadline ((abs self).handlers.take x.2.val) x.1)
  · rintro ⟨earliest, index⟩ ⟨hidx, hinv⟩
    dsimp only at hidx hinv
    unfold em_event_loop.LoopCore.next_deadline_loop.body
    simp only [alloc.vec.Vec.len]
    split
    · rename_i hlt
      have hlt' : index.val < self.handlers.val.length := by scalar_tac
      step as ⟨ handler, hhandler ⟩
      have htake : (abs self).handlers.take (index.val + 1) =
          (abs self).handlers.take index.val ++ [absHdl handler] := by
        have habs : (abs self).handlers[index.val]? = some (absHdl handler) := by
          simp [abs, hhandler, hlt']
        rw [List.take_add_one, habs]
        rfl
      by_cases hp : handler.paused = true
      · simp only [hp, ite_true, bind_ok]
        step as ⟨ index1, hi1 ⟩
        refine ⟨by omega, ?_, by omega⟩
        rw [hi1, htake]
        exact earliest_append_unarmed _ (by simp [absHdl, hp]) hinv
      · rw [ite_eq_right_iff.2 (fun h => absurd h hp)]
        rcases hdl : handler.deadline with _ | deadline
        · simp only [hdl, bind_ok]
          step as ⟨ index1, hi1 ⟩
          refine ⟨by omega, ?_, by omega⟩
          rw [hi1, htake]
          exact earliest_append_unarmed _ (by simp [absHdl, hdl]) hinv
        · obtain ⟨b, hbr, hb⟩ := spec_exists_ok (is_earlier_or_unset_spec inst cp hle earliest deadline)
          simp only [hdl, hbr, bind_ok, bind_tc_ok]
          have hnp : handler.paused = false := by simpa using hp
          have harm : absHdl handler = { deadline := some deadline, paused := false } := by
            simp [absHdl, hdl, hnp]
          by_cases hbt : b = true
          · simp only [hbt, ite_true, bind_ok]
            step as ⟨ index1, hi1 ⟩
            refine ⟨by omega, ?_, by omega⟩
            rw [hi1, htake]
            rcases hb.1 hbt with hnone | ⟨best, hbest, hdb⟩
            · subst hnone
              exact earliest_append_first _ deadline hnp (by simp [absHdl, hdl]) hinv
            · subst hbest
              exact earliest_append_earlier _ deadline best hnp (by simp [absHdl, hdl]) hdb hinv
          · have hbf : b = false := by simpa using hbt
            simp only [hbf, Bool.false_eq_true, ↓reduceIte, bind_ok]
            step as ⟨ index1, hi1 ⟩
            refine ⟨by omega, ?_, by omega⟩
            rw [hi1, htake]
            have : ¬ (earliest = none ∨ ∃ best, earliest = some best ∧ deadline ≤ best) :=
              fun h => hbt (hb.2 h)
            push Not at this
            obtain ⟨hne, hbest⟩ := this
            obtain ⟨best, hbe⟩ := Option.ne_none_iff_exists'.1 hne
            subst hbe
            exact earliest_append_later _ deadline best hnp (by simp [absHdl, hdl]) (not_le.2 (hbest best rfl)) hinv
    · rename_i hge
      have : index.val = self.handlers.val.length := by scalar_tac
      rw [WP.spec_ok]
      have hlen : (abs self).handlers.length = self.handlers.val.length := by simp [abs]
      rw [this, ← hlen, List.take_length] at hinv
      exact hinv
  · refine ⟨by simp, ?_⟩
    simp [abs, List.take_zero]
    left
    simp

theorem next_deadline_refines
    (hle : ∀ a b : T, inst.partialOrdInst.le a b = Result.ok (decide (a ≤ b)))
    (self : em_event_loop.LoopCore T) :
    ∃ o, OutcomeOf (fun d : Option T => d) (em_event_loop.LoopCore.next_deadline inst cp self) o ∧
      NextDeadlineStep (abs self) o := by
  obtain ⟨r, hr, hearliest⟩ := spec_exists_ok (next_deadline_loop_spec inst cp hle self)
  exact ⟨_, Or.inl ⟨r, hr, rfl⟩, r, rfl, hearliest⟩

/-! ## `next_turn` -/

/-- The first index from `start` where `P` holds, or `len` if there is none. -/
def FirstFrom (P : Nat → Prop) (len start r : Nat) : Prop :=
  (r < len ∧ start ≤ r ∧ P r ∧ ∀ i, start ≤ i → i < r → ¬ P i) ∨
  (r = len ∧ ∀ i, start ≤ i → i < len → ¬ P i)

theorem eligible_iff (self : em_event_loop.LoopCore T) (i : Nat)
    (h : i < self.sources.val.length) :
    Eligible (abs self).sources (absUsed self.sources.val) i ↔
      (self.sources.val[i].ready = true ∧ self.sources.val[i].paused = false ∧
        self.sources.val[i].used.val < self.sources.val[i].budget.val) := by
  unfold Eligible absUsed abs absSrc
  simp [List.getElem?_eq_getElem h]

theorem due_iff (self : em_event_loop.LoopCore T) (now : T) (i : Nat)
    (h : i < self.handlers.val.length) :
    Due (abs self).handlers now i ↔
      (∃ d, self.handlers.val[i].deadline = some d ∧ self.handlers.val[i].paused = false ∧ d ≤ now) := by
  unfold Due abs absHdl
  simp [List.getElem?_eq_getElem h]

theorem source_is_eligible_spec (self : em_event_loop.LoopCore T) (index : Usize)
    (h : index.val < self.sources.val.length) :
    em_event_loop.LoopCore.source_is_eligible inst cp self index ⦃ b =>
      (b = true ↔ Eligible (abs self).sources (absUsed self.sources.val) index.val) ⦄ := by
  unfold em_event_loop.LoopCore.source_is_eligible
  step as ⟨ source, hsource ⟩
  subst hsource
  rw [eligible_iff self _ h]
  cases hr : self.sources.val[index.val].ready <;> cases hp : self.sources.val[index.val].paused <;>
    simp [WP.spec_ok, hr, hp]

theorem handler_is_due_spec
    (hle : ∀ a b : T, inst.partialOrdInst.le a b = Result.ok (decide (a ≤ b)))
    (self : em_event_loop.LoopCore T) (index : Usize) (now : T)
    (h : index.val < self.handlers.val.length) :
    em_event_loop.LoopCore.handler_is_due inst cp self index now ⦃ b =>
      (b = true ↔ Due (abs self).handlers now index.val) ⦄ := by
  unfold em_event_loop.LoopCore.handler_is_due
  step as ⟨ handler, hhandler ⟩
  subst hhandler
  rw [due_iff self _ _ h]
  cases hp : self.handlers.val[index.val].paused
  · cases hd : self.handlers.val[index.val].deadline with
    | none => simp [em_event_loop.deadline_is_due, WP.spec_ok, hp, hd]
    | some d => simp [em_event_loop.deadline_is_due, WP.spec_ok, hp, hd, hle]
  · simp [WP.spec_ok, hp]

theorem first_eligible_source_loop_spec (self : em_event_loop.LoopCore T) (start : Usize) :
    em_event_loop.LoopCore.first_eligible_source_loop inst cp self start ⦃ r =>
      FirstFrom (Eligible (abs self).sources (absUsed self.sources.val))
        self.sources.val.length start.val r.val ⦄ := by
  unfold em_event_loop.LoopCore.first_eligible_source_loop
  apply loop.spec_decr_nat (measure := fun index => self.sources.val.length - index.val)
    (inv := fun index => start.val ≤ index.val ∧
      ∀ i, start.val ≤ i → i < index.val →
        ¬ Eligible (abs self).sources (absUsed self.sources.val) i)
  · intro index ⟨hstart, hinv⟩
    unfold em_event_loop.LoopCore.first_eligible_source_loop.body
    simp only [alloc.vec.Vec.len]
    split
    · rename_i hlt
      have hlt' : index.val < self.sources.val.length := by scalar_tac
      apply WP.spec_bind (source_is_eligible_spec inst cp self index hlt')
      intro b hb
      by_cases hbt : b = true
      · simp only [hbt, ite_true]
        rw [WP.spec_ok]
        exact Or.inl ⟨hlt', hstart, hb.1 hbt, fun i hi hij => hinv i hi hij⟩
      · have hbf : b = false := by simpa using hbt
        simp only [hbf, Bool.false_eq_true, ↓reduceIte]
        step as ⟨ index1, hi1 ⟩
        refine ⟨by omega, fun i hi hij => ?_, by omega⟩
        by_cases hii : i = index.val
        · subst hii
          exact fun he => hbt (hb.2 he)
        · exact hinv i hi (by omega)
    · rename_i hge
      have : self.sources.val.length ≤ index.val := by scalar_tac
      rw [WP.spec_ok]
      refine Or.inr ⟨rfl, fun i hi hil => hinv i hi (by omega)⟩
  · exact ⟨le_refl _, fun i hi hij => by omega⟩

theorem first_due_handler_loop_spec
    (hle : ∀ a b : T, inst.partialOrdInst.le a b = Result.ok (decide (a ≤ b)))
    (self : em_event_loop.LoopCore T) (now : T) (start : Usize) :
    em_event_loop.LoopCore.first_due_handler_loop inst cp self now start ⦃ r =>
      FirstFrom (Due (abs self).handlers now) self.handlers.val.length start.val r.val ⦄ := by
  unfold em_event_loop.LoopCore.first_due_handler_loop
  apply loop.spec_decr_nat (measure := fun index => self.handlers.val.length - index.val)
    (inv := fun index => start.val ≤ index.val ∧
      ∀ i, start.val ≤ i → i < index.val → ¬ Due (abs self).handlers now i)
  · intro index ⟨hstart, hinv⟩
    unfold em_event_loop.LoopCore.first_due_handler_loop.body
    simp only [alloc.vec.Vec.len]
    split
    · rename_i hlt
      have hlt' : index.val < self.handlers.val.length := by scalar_tac
      apply WP.spec_bind (handler_is_due_spec inst cp hle self index now hlt')
      intro b hb
      by_cases hbt : b = true
      · simp only [hbt, ite_true]
        rw [WP.spec_ok]
        exact Or.inl ⟨hlt', hstart, hb.1 hbt, fun i hi hij => hinv i hi hij⟩
      · have hbf : b = false := by simpa using hbt
        simp only [hbf, Bool.false_eq_true, ↓reduceIte]
        step as ⟨ index1, hi1 ⟩
        refine ⟨by omega, fun i hi hij => ?_, by omega⟩
        by_cases hii : i = index.val
        · subst hii
          exact fun he => hbt (hb.2 he)
        · exact hinv i hi (by omega)
    · rename_i hge
      have : self.handlers.val.length ≤ index.val := by scalar_tac
      rw [WP.spec_ok]
      refine Or.inr ⟨rfl, fun i hi hil => hinv i hi (by omega)⟩
  · exact ⟨le_refl _, fun i hi hij => by omega⟩

/-- The abstraction of the pair that `next_turn` and `turn_from_handler` return. -/
def absStep (r : em_event_loop.Turn × em_event_loop.LoopCore T) : Answer × LoopState T :=
  (absTurn r.1, abs r.2)

theorem turn_from_handler_spec
    (hle : ∀ a b : T, inst.partialOrdInst.le a b = Result.ok (decide (a ≤ b)))
    (self : em_event_loop.LoopCore T) (now : T) (start : Usize) :
    em_event_loop.LoopCore.turn_from_handler inst cp self now start ⦃ r =>
      ScanHandlers (abs self) now start.val (Outcome.ok (absStep r)) ⦄ := by
  unfold em_event_loop.LoopCore.turn_from_handler
  apply WP.spec_bind (first_due_handler_loop_spec inst cp hle self now start)
  intro handler hf
  simp only [alloc.vec.Vec.len]
  split
  · rename_i hlt
    have hlt' : handler.val < self.handlers.val.length := by scalar_tac
    rcases hf with ⟨-, hstart, hdue, hbefore⟩ | ⟨hlen, -⟩
    · step as ⟨ hs, index_mut_back, hhs, hback ⟩
      step as ⟨ i1, hi1 ⟩
      left
      refine ⟨handler.val, hstart, hdue, hbefore, ?_⟩
      have hx : self.handlers.val[handler.val]? = some hs := by simp [hhs, hlt']
      subst hback
      have hm := map_set_eq_modifyAt absHdl self.handlers.val handler.val hs
        { hs with deadline := none } (fun x => { x with deadline := none }) hx rfl
      simp only [absStep, absTurn]
      unfold abs
      simp only [absPhase, alloc.vec.Vec.set_val_eq, hi1]
      rw [hm]
    · omega
  · rename_i hge
    rcases hf with ⟨hlt, -⟩ | ⟨-, hnone⟩
    · exfalso
      exact hge (by scalar_tac)
    · rw [WP.spec_ok]
      right
      refine ⟨fun i hi hdue => ?_, rfl⟩
      have hi' : i < self.handlers.val.length := by
        have := Due_lt hdue
        simpa [abs] using this
      exact hnone i hi hi' hdue

/-- Setting one source: `absUsed` changes only at that source. -/
theorem absUsed_set {l : List em_event_loop.SourceState} {j : Nat} (x' : em_event_loop.SourceState)
    (hj : j < l.length) :
    absUsed (l.set j x') = fun k => if k = j then x'.used.val else absUsed l k := by
  funext k
  unfold absUsed
  by_cases hk : k = j
  · subst hk
    simp [hj]
  · simp [hk]

theorem next_turn_spec
    (hle : ∀ a b : T, inst.partialOrdInst.le a b = Result.ok (decide (a ≤ b)))
    (self : em_event_loop.LoopCore T) :
    em_event_loop.LoopCore.next_turn inst cp self ⦃ r =>
      NextTurnStep (abs self) (Outcome.ok (absStep r)) ⦄ := by
  unfold em_event_loop.LoopCore.next_turn
  rcases hph : self.phase with _ | ⟨now, position⟩ | ⟨now, next⟩
  · simp only [hph]
    rw [WP.spec_ok]
    have : (abs self).phase = Phase.done := by simp [abs, absPhase, hph]
    simp only [NextTurnStep, this]
    rfl
  · simp only [hph]
    have habs : (abs self).phase = Phase.serving now position.val (absUsed self.sources.val) := by
      simp [abs, absPhase, hph]
    simp only [NextTurnStep, habs, ScanSources]
    unfold em_event_loop.LoopCore.first_eligible_source
    apply WP.spec_bind (first_eligible_source_loop_spec inst cp self position)
    intro source hf
    simp only [alloc.vec.Vec.len]
    split
    · rename_i hlt
      have hlt' : source.val < self.sources.val.length := by scalar_tac
      rcases hf with ⟨-, hstart, helig, hbefore⟩ | ⟨hlen, -⟩
      · step as ⟨ ss, back1, hss, hback1 ⟩
        subst hback1
        have hlen2 : source.val < (self.sources.set source { ss with ready := false }).length := by
          simp [hlt']
        step as ⟨ ss1, back2, hss1, hback2 ⟩
        have hss1' : ss1 = { ss with ready := false } := by
          rw [hss1]
          simp
        have hused : ss.used.val < ss.budget.val := by
          have := (eligible_iff self _ hlt').1 helig
          rw [hss]
          exact this.2.2
        step as ⟨ i1, hi1 ⟩
        have hx : self.sources.val[source.val]? = some ss := by simp [hss, hlt']
        left
        refine ⟨source.val, hstart, helig, hbefore, ?_⟩
        subst hback2
        subst hss1'
        simp only [absStep, absTurn, alloc.vec.Vec.set_val_eq, List.set_set]
        unfold abs
        simp only [absPhase, alloc.vec.Vec.set_val_eq, List.set_set]
        rw [absUsed_set _ hlt']
        have hm := map_set_eq_modifyAt absSrc self.sources.val source.val ss
          { ss with ready := false, used := i1 } (fun x => { x with ready := false }) hx rfl
        have hu : absUsed self.sources.val source.val = ss.used.val := by simp [absUsed, hx]
        rw [hm]
        congr 1
        simp only [Prod.mk.injEq, true_and, LoopState.mk.injEq, Phase.serving.injEq, and_true]
        funext k
        simp only [usedBump]
        by_cases hk : k = source.val
        · subst hk
          simp [hu, hi1]
        · simp [hk]
      · omega
    · rename_i hge
      have hnone : ∀ i, position.val ≤ i → i < self.sources.val.length →
          ¬ Eligible (abs self).sources (absUsed self.sources.val) i := by
        rcases hf with ⟨hlt, -⟩ | ⟨-, hnone⟩
        · exfalso
          exact hge (by scalar_tac)
        · exact hnone
      apply WP.spec_mono (turn_from_handler_spec inst cp hle self now 0#usize)
      intro r hr
      right
      refine ⟨fun i hi hel => ?_, hr⟩
      have hi' : i < self.sources.val.length := by
        have := Eligible_lt hel
        simpa [abs] using this
      exact hnone i hi hi' hel
  · simp only [hph]
    have habs : (abs self).phase = Phase.deadlines now next.val := by
      simp [abs, absPhase, hph]
    simp only [NextTurnStep, habs]
    exact WP.spec_mono (turn_from_handler_spec inst cp hle self now next) (fun r hr => hr)

theorem next_turn_refines
    (hle : ∀ a b : T, inst.partialOrdInst.le a b = Result.ok (decide (a ≤ b)))
    (self : em_event_loop.LoopCore T) :
    ∃ o, OutcomeOf absStep (em_event_loop.LoopCore.next_turn inst cp self) o ∧
      NextTurnStep (abs self) o := by
  obtain ⟨r, hr, hstep⟩ := spec_exists_ok (next_turn_spec inst cp hle self)
  exact ⟨_, Or.inl ⟨r, hr, rfl⟩, hstep⟩

/-! ## Numbers that do not exist: the call fails (the Rust panic) -/

theorem set_ready_fails (self : em_event_loop.LoopCore T) (source : Usize) (ready : Bool)
    (h : self.sources.val.length ≤ source.val) :
    em_event_loop.LoopCore.set_ready inst cp self source ready =
      Result.fail Error.arrayOutOfBounds := by
  unfold em_event_loop.LoopCore.set_ready
  rw [alloc.vec.Vec.index_mut_slice_index, index_mut_usize_oob _ _ h]
  simp

theorem set_paused_fails (self : em_event_loop.LoopCore T) (source : Usize) (paused : Bool)
    (h : self.sources.val.length ≤ source.val) :
    em_event_loop.LoopCore.set_paused inst cp self source paused =
      Result.fail Error.arrayOutOfBounds := by
  unfold em_event_loop.LoopCore.set_paused
  rw [alloc.vec.Vec.index_mut_slice_index, index_mut_usize_oob _ _ h]
  simp

theorem set_deadline_fails (self : em_event_loop.LoopCore T) (handler : Usize)
    (deadline : Option T) (h : self.handlers.val.length ≤ handler.val) :
    em_event_loop.LoopCore.set_deadline inst cp self handler deadline =
      Result.fail Error.arrayOutOfBounds := by
  unfold em_event_loop.LoopCore.set_deadline
  rw [alloc.vec.Vec.index_mut_slice_index, index_mut_usize_oob _ _ h]
  simp

theorem set_deadline_paused_fails (self : em_event_loop.LoopCore T) (handler : Usize)
    (paused : Bool) (h : self.handlers.val.length ≤ handler.val) :
    em_event_loop.LoopCore.set_deadline_paused inst cp self handler paused =
      Result.fail Error.arrayOutOfBounds := by
  unfold em_event_loop.LoopCore.set_deadline_paused
  rw [alloc.vec.Vec.index_mut_slice_index, index_mut_usize_oob _ _ h]
  simp

end EmEventLoop
