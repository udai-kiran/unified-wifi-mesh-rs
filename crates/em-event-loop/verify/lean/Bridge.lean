import Rules

/-! # Bridge for the application between answers (Task Contract, "Carried into step 3")

`Env` is an equivalence-like step (reflexive, transitive), and every successful setter call is an
`Env` change. So the pass theorems, which allow any `Env` change between two calls of
`next_turn`, also cover an application that calls the setters in between. -/

namespace EmEventLoop

set_option linter.unusedSectionVars false

variable {T : Type} [LinearOrder T] {ops : Ops T}

theorem Env_refl (s : LoopState T) : Env s s := ⟨rfl, rfl, rfl⟩

theorem Env_trans {s s' s'' : LoopState T} (h1 : Env s s') (h2 : Env s' s'') : Env s s'' :=
  ⟨h2.1.trans h1.1, h2.2.1.trans h1.2.1, h2.2.2.trans h1.2.2⟩

theorem Env_of_setReady (hops : ops.Meets) {s s' : LoopState T} {i : Nat} {b : Bool}
    (h : ops.setReady s i b = .ok s') : Env s s' := by
  obtain ⟨_, hs⟩ := pf_set_ready hops h
  subst hs
  exact ⟨rfl, pf_modifyAt_map (fun x : Src => x.budget) _ _ _ (fun x => rfl), rfl⟩

theorem Env_of_setPaused (hops : ops.Meets) {s s' : LoopState T} {i : Nat} {b : Bool}
    (h : ops.setPaused s i b = .ok s') : Env s s' := by
  obtain ⟨_, hs⟩ := pf_set_paused hops h
  subst hs
  exact ⟨rfl, pf_modifyAt_map (fun x : Src => x.budget) _ _ _ (fun x => rfl), rfl⟩

theorem Env_of_setDeadline (hops : ops.Meets) {s s' : LoopState T} {h : Nat} {d : Option T}
    (hs : ops.setDeadline s h d = .ok s') : Env s s' := by
  obtain ⟨_, hs'⟩ := pf_set_deadline hops hs
  subst hs'
  exact ⟨rfl, rfl, pf_modifyAt_length _ _ _⟩

theorem Env_of_setDeadlinePaused (hops : ops.Meets) {s s' : LoopState T} {h : Nat} {b : Bool}
    (hs : ops.setDeadlinePaused s h b = .ok s') : Env s s' := by
  obtain ⟨_, hs'⟩ := pf_set_deadline_paused hops hs
  subst hs'
  exact ⟨rfl, rfl, pf_modifyAt_length _ _ _⟩

end EmEventLoop
