import StepSpec

/-! # Rules S1 to S17 of the English spec, stated over any operations that meet the step spec

Every theorem takes `hops : ops.Meets`. Bodies are `sorry` at this step (note 0017 D6 step 2). -/

namespace EmEventLoop

variable {T : Type} [LinearOrder T] {ops : Ops T} {now : T}

/-! ## Words used by the rules, in terms of one pass trace -/

def IsServe (a : Answer) : Prop := ∃ i, a = .serve i

/-- How many of the calls `0 .. n-1` answered "serve source `i`". -/
def serveCount (answer : Nat → Answer) (n i : Nat) : Nat :=
  ((List.range n).filter fun k => answer k == .serve i).length

/-- The current position at call `k`: the last source served before it, or 0. -/
def curPos (answer : Nat → Answer) : Nat → Nat
  | 0 => 0
  | k + 1 => match answer k with
    | .serve i => i
    | _ => curPos answer k

/-- The first handler not yet passed at call `k`: one after the last handler run, or 0. -/
def nextHandler (answer : Nat → Answer) : Nat → Nat
  | 0 => 0
  | k + 1 => match answer k with
    | .runDeadline h => h + 1
    | _ => nextHandler answer k

/-- Source `i` is eligible at call `k`: ready, not paused, and fewer than its budget served
in calls before `k`. -/
def EligibleAt (tr : PassTrace ops now) (k i : Nat) : Prop :=
  ∃ x, (tr.seen k).sources[i]? = some x ∧ x.ready = true ∧ x.paused = false ∧
    serveCount tr.answer k i < x.budget

/-- Handler `h` is due at call `k`. -/
def DueAt (tr : PassTrace ops now) (k h : Nat) : Prop := Due (tr.seen k).handlers now h

/-- Source `i` has ready flag `b` in state `s`. -/
def SourceReady (s : LoopState T) (i : Nat) (b : Bool) : Prop :=
  ∃ x, s.sources[i]? = some x ∧ x.ready = b

/-! ## S1. Start -/

theorem S1_new_loop_is_idle (hops : ops.Meets) (budgets : List Nat) (n : Nat) (s : LoopState T)
    (h : ops.new budgets n = .ok s) :
    s.sources.map (·.budget) = budgets ∧
    (∀ x ∈ s.sources, x.ready = false ∧ x.paused = false) ∧
    s.handlers.length = n ∧
    (∀ x ∈ s.handlers, x.deadline = none ∧ x.paused = false) := by
  have hs := hops.new budgets n
  rw [h] at hs
  unfold NewStep at hs
  simp only [Outcome.ok.injEq] at hs
  subst hs
  refine ⟨?_, ?_, ?_, ?_⟩
  · simp [List.map_map, Function.comp_def]
  · simp
  · simp
  · simp [List.mem_replicate]
lemma pf_nt_exists (hops : ops.Meets) (s : LoopState T) :
    ∃ a s', ops.nextTurn s = .ok (a, s') := by
  have hs := hops.nextTurn s
  unfold NextTurnStep at hs
  have hscan : ∀ now start, ScanHandlers s now start (ops.nextTurn s) →
      ∃ a s', ops.nextTurn s = .ok (a, s') := by
    intro now start h
    unfold ScanHandlers at h
    rcases h with ⟨_, _, _, _, h⟩ | ⟨_, h⟩ <;> exact ⟨_, _, h⟩
  cases hph : s.phase with
  | done => rw [hph] at hs; exact ⟨_, _, hs⟩
  | serving now pos used =>
    rw [hph] at hs
    unfold ScanSources at hs
    rcases hs with ⟨_, _, _, _, h⟩ | ⟨_, h⟩
    · exact ⟨_, _, h⟩
    · exact hscan _ _ h
  | deadlines now next => rw [hph] at hs; exact hscan _ _ hs

lemma pf_lt_of_some {α : Type} {l : List α} {i : Nat} {x : α} (h : l[i]? = some x) :
    i < l.length := by
  obtain ⟨hlt, _⟩ := List.getElem?_eq_some_iff.mp h
  exact hlt

lemma pf_set_ready (hops : ops.Meets) {s s' : LoopState T} {i : Nat} {b : Bool}
    (h : ops.setReady s i b = .ok s') :
    i < s.sources.length ∧
      s' = { s with sources := modifyAt s.sources i fun x => { x with ready := b } } := by
  have hs := hops.setReady s i b
  rw [h] at hs
  have hlt : i < s.sources.length := by
    by_contra hge
    have := hs.2 (Nat.le_of_not_lt hge)
    cases this
  have := hs.1 hlt
  simp only [Outcome.ok.injEq] at this
  exact ⟨hlt, this⟩

lemma pf_set_paused (hops : ops.Meets) {s s' : LoopState T} {i : Nat} {b : Bool}
    (h : ops.setPaused s i b = .ok s') :
    i < s.sources.length ∧
      s' = { s with sources := modifyAt s.sources i fun x => { x with paused := b } } := by
  have hs := hops.setPaused s i b
  rw [h] at hs
  have hlt : i < s.sources.length := by
    by_contra hge
    have := hs.2 (Nat.le_of_not_lt hge)
    cases this
  have := hs.1 hlt
  simp only [Outcome.ok.injEq] at this
  exact ⟨hlt, this⟩

lemma pf_set_deadline (hops : ops.Meets) {s s' : LoopState T} {h : Nat} {d : Option T}
    (hs' : ops.setDeadline s h d = .ok s') :
    h < s.handlers.length ∧
      s' = { s with handlers := modifyAt s.handlers h fun x => { x with deadline := d } } := by
  have hs := hops.setDeadline s h d
  rw [hs'] at hs
  have hlt : h < s.handlers.length := by
    by_contra hge
    have := hs.2 (Nat.le_of_not_lt hge)
    cases this
  have := hs.1 hlt
  simp only [Outcome.ok.injEq] at this
  exact ⟨hlt, this⟩

lemma pf_set_deadline_paused (hops : ops.Meets) {s s' : LoopState T} {h : Nat} {b : Bool}
    (hs' : ops.setDeadlinePaused s h b = .ok s') :
    h < s.handlers.length ∧
      s' = { s with handlers := modifyAt s.handlers h fun x => { x with paused := b } } := by
  have hs := hops.setDeadlinePaused s h b
  rw [hs'] at hs
  have hlt : h < s.handlers.length := by
    by_contra hge
    have := hs.2 (Nat.le_of_not_lt hge)
    cases this
  have := hs.1 hlt
  simp only [Outcome.ok.injEq] at this
  exact ⟨hlt, this⟩


set_option linter.unusedSectionVars false

/-! ## Helper lemmas for the proofs below

They sit here, after the first theorem and not in a new imported file, because
`check-statements.py` hashes everything above the first theorem, so `Rules.lean` cannot take a
new `import` line. They are `lemma`s so that the digest does not count them as rules. -/

lemma pf_modifyAt_getElem? {α : Type} (l : List α) (i : Nat) (f : α → α) (k : Nat) :
    (modifyAt l i f)[k]? = if k = i then l[k]?.map f else l[k]? := by
  unfold modifyAt
  rw [List.getElem?_mapIdx]
  by_cases hk : k = i <;> cases l[k]? <;> simp [hk]

lemma pf_modifyAt_length {α : Type} (l : List α) (i : Nat) (f : α → α) :
    (modifyAt l i f).length = l.length := by
  simp [modifyAt]

lemma pf_modifyAt_self {α : Type} (l : List α) (i : Nat) (f : α → α) (x : α)
    (hx : l[i]? = some x) : (modifyAt l i f)[i]? = some (f x) := by
  simp [pf_modifyAt_getElem?, hx]

lemma pf_modifyAt_other {α : Type} (l : List α) (i : Nat) (f : α → α) (k : Nat) (hk : k ≠ i) :
    (modifyAt l i f)[k]? = l[k]? := by
  simp [pf_modifyAt_getElem?, hk]

lemma pf_modifyAt_map {α β : Type} (g : α → β) (l : List α) (i : Nat) (f : α → α)
    (hf : ∀ x, g (f x) = g x) : (modifyAt l i f).map g = l.map g := by
  apply List.ext_getElem?
  intro k
  rw [List.getElem?_map, List.getElem?_map, pf_modifyAt_getElem?]
  by_cases hk : k = i <;> cases l[k]? <;> simp [hk, hf]

/-- What one call of `next_turn` can do, case by case, read off the step spec. -/
inductive NtCase (s : LoopState T) (a : Answer) (s' : LoopState T) : Prop where
  | idle (hph : s.phase = .done) (ha : a = .passDone) (hs : s' = s)
  | serve (now : T) (pos : Nat) (used : Nat → Nat) (j : Nat)
      (hph : s.phase = .serving now pos used) (hpos : pos ≤ j)
      (hel : Eligible s.sources used j)
      (hbefore : ∀ i, pos ≤ i → i < j → ¬ Eligible s.sources used i)
      (ha : a = .serve j)
      (hs : s' = { s with sources := modifyAt s.sources j fun x => { x with ready := false }
                          phase := .serving now j (usedBump used j) })
  | runAfterSources (now : T) (pos : Nat) (used : Nat → Nat) (h : Nat)
      (hph : s.phase = .serving now pos used)
      (hnone : ∀ i, pos ≤ i → ¬ Eligible s.sources used i)
      (hdue : Due s.handlers now h)
      (hbefore : ∀ i, i < h → ¬ Due s.handlers now i)
      (ha : a = .runDeadline h)
      (hs : s' = { s with
                   handlers := modifyAt s.handlers h fun x => { x with deadline := none }
                   phase := .deadlines now (h + 1) })
  | doneAfterSources (now : T) (pos : Nat) (used : Nat → Nat)
      (hph : s.phase = .serving now pos used)
      (hnone : ∀ i, pos ≤ i → ¬ Eligible s.sources used i)
      (hnodue : ∀ i, ¬ Due s.handlers now i)
      (ha : a = .passDone) (hs : s' = { s with phase := .done })
  | runAfterHandlers (now : T) (next : Nat) (h : Nat)
      (hph : s.phase = .deadlines now next) (hle : next ≤ h)
      (hdue : Due s.handlers now h)
      (hbefore : ∀ i, next ≤ i → i < h → ¬ Due s.handlers now i)
      (ha : a = .runDeadline h)
      (hs : s' = { s with
                   handlers := modifyAt s.handlers h fun x => { x with deadline := none }
                   phase := .deadlines now (h + 1) })
  | doneAfterHandlers (now : T) (next : Nat)
      (hph : s.phase = .deadlines now next)
      (hnodue : ∀ i, next ≤ i → ¬ Due s.handlers now i)
      (ha : a = .passDone) (hs : s' = { s with phase := .done })

lemma pf_scan_handlers_cases {s s' : LoopState T} {a : Answer} {now : T} {start : Nat}
    (h : ScanHandlers s now start (.ok (a, s'))) :
    (∃ hh, start ≤ hh ∧ Due s.handlers now hh ∧
      (∀ i, start ≤ i → i < hh → ¬ Due s.handlers now i) ∧ a = .runDeadline hh ∧
      s' = { s with handlers := modifyAt s.handlers hh fun x => { x with deadline := none }
                    phase := .deadlines now (hh + 1) }) ∨
    ((∀ i, start ≤ i → ¬ Due s.handlers now i) ∧ a = .passDone ∧
      s' = { s with phase := .done }) := by
  unfold ScanHandlers at h
  rcases h with ⟨hh, h1, h2, h3, h4⟩ | ⟨h1, h2⟩
  · simp only [Outcome.ok.injEq, Prod.mk.injEq] at h4
    exact .inl ⟨hh, h1, h2, h3, h4.1, h4.2⟩
  · simp only [Outcome.ok.injEq, Prod.mk.injEq] at h2
    exact .inr ⟨h1, h2.1, h2.2⟩

lemma pf_nt_cases (hops : ops.Meets) {s s' : LoopState T} {a : Answer}
    (h : ops.nextTurn s = .ok (a, s')) : NtCase s a s' := by
  have hs := hops.nextTurn s
  rw [h] at hs
  unfold NextTurnStep at hs
  cases hph : s.phase with
  | done =>
    rw [hph] at hs
    simp only [Outcome.ok.injEq, Prod.mk.injEq] at hs
    exact .idle hph hs.1 hs.2
  | serving now pos used =>
    rw [hph] at hs
    unfold ScanSources at hs
    rcases hs with ⟨j, h1, h2, h3, h4⟩ | ⟨h1, h2⟩
    · simp only [Outcome.ok.injEq, Prod.mk.injEq] at h4
      exact .serve now pos used j hph h1 h2 h3 h4.1 h4.2
    · rcases pf_scan_handlers_cases h2 with ⟨hh, _, h3, h4, h5, h6⟩ | ⟨h3, h4, h5⟩
      · exact .runAfterSources now pos used hh hph h1 h3 (fun i hi => h4 i (Nat.zero_le i) hi)
          h5 h6
      · exact .doneAfterSources now pos used hph h1 (fun i => h3 i (Nat.zero_le i)) h4 h5
  | deadlines now next =>
    rw [hph] at hs
    rcases pf_scan_handlers_cases hs with ⟨hh, h1, h2, h3, h4, h5⟩ | ⟨h1, h2, h3⟩
    · exact .runAfterHandlers now next hh hph h1 h2 h3 h4 h5
    · exact .doneAfterHandlers now next hph h1 h2 h3

theorem S1_next_turn_before_begin_pass (hops : ops.Meets) (budgets : List Nat) (n : Nat)
    (s0 s : LoopState T) (h : ops.new budgets n = .ok s0) (henv : Env s0 s) :
    ops.nextTurn s = .ok (.passDone, s) := by
  have hs := hops.new budgets n
  rw [h] at hs
  unfold NewStep at hs
  simp only [Outcome.ok.injEq] at hs
  subst hs
  have hph : s.phase = .done := henv.1
  have hn := hops.nextTurn s
  unfold NextTurnStep at hn
  rw [hph] at hn
  exact hn

theorem S1_first_answer_after_new_is_done (hops : ops.Meets) (budgets : List Nat) (n : Nat)
    (s0 p : LoopState T) (h : ops.new budgets n = .ok s0)
    (hb : ops.beginPass s0 now = .ok p) :
    ∃ s', ops.nextTurn p = .ok (.passDone, s') := by
  have hidle := S1_new_loop_is_idle hops budgets n s0 h
  have hp := hops.beginPass s0 now
  rw [hb] at hp
  unfold BeginPassStep at hp
  simp only [Outcome.ok.injEq] at hp
  obtain ⟨a, s', hn⟩ := pf_nt_exists hops p
  have hc := pf_nt_cases hops hn
  have hsrc : p.sources = s0.sources := by rw [hp]
  have hhdl : p.handlers = s0.handlers := by rw [hp]
  cases hc with
  | idle _ ha _ => subst ha; exact ⟨s', hn⟩
  | serve now pos used j _ _ hel _ _ _ =>
    exfalso
    obtain ⟨x, hx, hready, _⟩ := hel
    rw [hsrc] at hx
    have := (hidle.2.1 x (List.mem_of_getElem? hx)).1
    rw [this] at hready
    cases hready
  | runAfterSources now pos used hh _ _ hdue _ _ _ =>
    exfalso
    obtain ⟨x, d, hx, hd, _⟩ := hdue
    rw [hhdl] at hx
    have := (hidle.2.2.2 x (List.mem_of_getElem? hx)).1
    rw [this] at hd
    cases hd
  | doneAfterSources _ _ _ _ _ _ ha _ => subst ha; exact ⟨s', hn⟩
  | runAfterHandlers now next hh _ _ hdue _ _ _ =>
    exfalso
    obtain ⟨x, d, hx, hd, _⟩ := hdue
    rw [hhdl] at hx
    have := (hidle.2.2.2 x (List.mem_of_getElem? hx)).1
    rw [this] at hd
    cases hd
  | doneAfterHandlers _ _ _ _ ha _ => subst ha; exact ⟨s', hn⟩

lemma pf_nt_keeps (hops : ops.Meets) {s s' : LoopState T} {a : Answer}
    (h : ops.nextTurn s = .ok (a, s')) :
    s'.sources.map (·.paused) = s.sources.map (·.paused) ∧
    s'.sources.map (·.budget) = s.sources.map (·.budget) ∧
    s'.handlers.map (·.paused) = s.handlers.map (·.paused) := by
  have hc := pf_nt_cases hops h
  cases hc with
  | idle _ _ hs => subst hs; exact ⟨rfl, rfl, rfl⟩
  | serve now pos used j _ _ _ _ _ hs =>
    subst hs
    exact ⟨pf_modifyAt_map (fun x : Src => x.paused) _ _ _ (fun x => rfl),
      pf_modifyAt_map (fun x : Src => x.budget) _ _ _ (fun x => rfl), rfl⟩
  | runAfterSources now pos used hh _ _ _ _ _ hs =>
    subst hs
    exact ⟨rfl, rfl, pf_modifyAt_map (fun x : Hdl T => x.paused) _ _ _ (fun x => rfl)⟩
  | doneAfterSources _ _ _ _ _ _ _ hs => subst hs; exact ⟨rfl, rfl, rfl⟩
  | runAfterHandlers now next hh _ _ _ _ _ hs =>
    subst hs
    exact ⟨rfl, rfl, pf_modifyAt_map (fun x : Hdl T => x.paused) _ _ _ (fun x => rfl)⟩
  | doneAfterHandlers _ _ _ _ _ hs => subst hs; exact ⟨rfl, rfl, rfl⟩

lemma pf_mono_of_succ (f : Nat → Nat) (hf : ∀ k, f k ≤ f (k + 1)) {j k : Nat} (hjk : j ≤ k) :
    f j ≤ f k := by
  induction k with
  | zero =>
    have : j = 0 := by omega
    subst this
    exact Nat.le_refl _
  | succ k ih =>
    rcases Nat.eq_or_lt_of_le hjk with h | h
    · rw [h]; exact Nat.le_refl _
    · exact Nat.le_trans (ih (by omega)) (hf k)

/-! ### Facts about one pass trace

`TraceInv tr k` says: before call `k`, the pass progress in `seen k` matches the answer history.
The history has the shape serve*, run*, done*, and the phase says which part we are in. -/

def IsRun (a : Answer) : Prop := ∃ h, a = .runDeadline h

def TraceInv (tr : PassTrace ops now) (k : Nat) : Prop :=
  ((∀ j, j < k → IsServe (tr.answer j)) ∧
    (tr.seen k).phase = .serving now (curPos tr.answer k) (serveCount tr.answer k)) ∨
  (∃ a, a < k ∧ (∀ j, j < a → IsServe (tr.answer j)) ∧
    (∀ j, a ≤ j → j < k → IsRun (tr.answer j)) ∧
    (tr.seen k).phase = .deadlines now (nextHandler tr.answer k)) ∨
  (∃ a b, a ≤ b ∧ b < k ∧ (∀ j, j < a → IsServe (tr.answer j)) ∧
    (∀ j, a ≤ j → j < b → IsRun (tr.answer j)) ∧
    (∀ j, b ≤ j → j < k → tr.answer j = .passDone) ∧ (tr.seen k).phase = .done)

lemma pf_serveCount_succ (answer : Nat → Answer) (n i : Nat) :
    serveCount answer (n + 1) i = serveCount answer n i + if answer n = .serve i then 1 else 0 := by
  unfold serveCount
  rw [List.range_succ, List.filter_append]
  by_cases h : answer n = .serve i <;> simp [h]

lemma pf_serving_eq {now now' : T} {p p' : Nat} {u u' : Nat → Nat}
    (h : Phase.serving now p u = Phase.serving now' p' u') : now = now' ∧ p = p' ∧ u = u' := by
  simpa using h

lemma pf_deadlines_eq {now now' : T} {p p' : Nat}
    (h : Phase.deadlines now p = Phase.deadlines now' p') : now = now' ∧ p = p' := by
  simpa using h

lemma pf_begin_phase (hops : ops.Meets) (tr : PassTrace ops now) :
    (tr.seen 0).phase = .serving now 0 (fun _ => 0) := by
  obtain ⟨p, hp, henv⟩ := tr.began
  have hb := hops.beginPass tr.start now
  rw [hp] at hb
  unfold BeginPassStep at hb
  simp only [Outcome.ok.injEq] at hb
  subst hb
  exact henv.1

lemma pf_inv_zero (hops : ops.Meets) (tr : PassTrace ops now) : TraceInv tr 0 := by
  refine Or.inl ⟨fun j hj => absurd hj (Nat.not_lt_zero _), ?_⟩
  rw [pf_begin_phase hops tr]
  have hsc : serveCount tr.answer 0 = fun _ => 0 := by
    funext i
    simp [serveCount]
  rw [hsc]
  rfl

lemma pf_inv_succ (hops : ops.Meets) (tr : PassTrace ops now) (k : Nat)
    (h : TraceInv tr k) : TraceInv tr (k + 1) := by
  have hc := pf_nt_cases hops (tr.call k)
  have hph : (tr.seen (k + 1)).phase = (tr.after k).phase := (tr.change k).1
  rcases h with ⟨hall, h0⟩ | ⟨a, hak, hserve, hrun, h0⟩ | ⟨a, b, hab, hbk, hserve, hrun, hdone, h0⟩
  · cases hc with
    | idle hphN _ _ => rw [h0] at hphN; cases hphN
    | serve now' pos used j hphN _ _ _ ha hs =>
      rw [h0] at hphN
      obtain ⟨rfl, rfl, rfl⟩ := pf_serving_eq hphN
      refine Or.inl ⟨?_, ?_⟩
      · intro j' hj'
        by_cases hjk : j' = k
        · subst hjk
          exact ⟨j, ha⟩
        · exact hall j' (by omega)
      · rw [hph, hs]
        have hcur : curPos tr.answer (k + 1) = j := by simp [curPos, ha]
        have hused : serveCount tr.answer (k + 1) = usedBump (serveCount tr.answer k) j := by
          funext i
          rw [pf_serveCount_succ, ha]
          by_cases hi : i = j
          · subst hi
            simp [usedBump]
          · have hj : ¬ j = i := fun e => hi e.symm
            simp [usedBump, hi, hj]
        rw [hcur, hused]
    | runAfterSources now' pos used hh hphN _ _ _ ha hs =>
      rw [h0] at hphN
      obtain ⟨rfl, rfl, rfl⟩ := pf_serving_eq hphN
      refine Or.inr (Or.inl ⟨k, Nat.lt_succ_self k, hall, ?_, ?_⟩)
      · intro j' hj1 hj2
        have : j' = k := by omega
        subst this
        exact ⟨hh, ha⟩
      · rw [hph, hs]
        have : nextHandler tr.answer (k + 1) = hh + 1 := by simp [nextHandler, ha]
        rw [this]
    | doneAfterSources now' pos used hphN _ _ ha hs =>
      refine Or.inr (Or.inr ⟨k, k, Nat.le_refl k, Nat.lt_succ_self k, hall, ?_, ?_, ?_⟩)
      · intro j' hj1 hj2
        omega
      · intro j' hj1 hj2
        have : j' = k := by omega
        subst this
        exact ha
      · rw [hph, hs]
    | runAfterHandlers _ _ _ hphN _ _ _ _ _ => rw [h0] at hphN; cases hphN
    | doneAfterHandlers _ _ hphN _ _ _ => rw [h0] at hphN; cases hphN
  · cases hc with
    | idle hphN _ _ => rw [h0] at hphN; cases hphN
    | serve _ _ _ _ hphN _ _ _ _ _ => rw [h0] at hphN; cases hphN
    | runAfterSources _ _ _ _ hphN _ _ _ _ _ => rw [h0] at hphN; cases hphN
    | doneAfterSources _ _ _ hphN _ _ _ _ => rw [h0] at hphN; cases hphN
    | runAfterHandlers now' next hh hphN _ _ _ ha hs =>
      rw [h0] at hphN
      obtain ⟨rfl, rfl⟩ := pf_deadlines_eq hphN
      refine Or.inr (Or.inl ⟨a, by omega, hserve, ?_, ?_⟩)
      · intro j' hj1 hj2
        by_cases hjk : j' = k
        · subst hjk
          exact ⟨hh, ha⟩
        · exact hrun j' hj1 (by omega)
      · rw [hph, hs]
        have : nextHandler tr.answer (k + 1) = hh + 1 := by simp [nextHandler, ha]
        rw [this]
    | doneAfterHandlers now' next hphN _ ha hs =>
      refine Or.inr (Or.inr ⟨a, k, by omega, Nat.lt_succ_self k, hserve, hrun, ?_, ?_⟩)
      · intro j' hj1 hj2
        have : j' = k := by omega
        subst this
        exact ha
      · rw [hph, hs]
  · cases hc with
    | idle hphN ha hs =>
      refine Or.inr (Or.inr ⟨a, b, hab, by omega, hserve, hrun, ?_, ?_⟩)
      · intro j' hj1 hj2
        by_cases hjk : j' = k
        · subst hjk
          exact ha
        · exact hdone j' hj1 (by omega)
      · rw [hph, hs]
        exact h0
    | serve _ _ _ _ hphN _ _ _ _ _ => rw [h0] at hphN; cases hphN
    | runAfterSources _ _ _ _ hphN _ _ _ _ _ => rw [h0] at hphN; cases hphN
    | doneAfterSources _ _ _ hphN _ _ _ _ => rw [h0] at hphN; cases hphN
    | runAfterHandlers _ _ _ hphN _ _ _ _ _ => rw [h0] at hphN; cases hphN
    | doneAfterHandlers _ _ hphN _ _ _ => rw [h0] at hphN; cases hphN

lemma pf_inv_all (hops : ops.Meets) (tr : PassTrace ops now) (k : Nat) : TraceInv tr k := by
  induction k with
  | zero => exact pf_inv_zero hops tr
  | succ k ih => exact pf_inv_succ hops tr k ih

lemma pf_inv_serving {tr : PassTrace ops now} {k : Nat} {p : Nat} {u : Nat → Nat}
    {n' : T} (h : TraceInv tr k) (hph : (tr.seen k).phase = .serving n' p u) :
    n' = now ∧ p = curPos tr.answer k ∧ u = serveCount tr.answer k ∧
      ∀ j, j < k → IsServe (tr.answer j) := by
  rcases h with ⟨hall, h0⟩ | ⟨a, _, _, _, h0⟩ | ⟨a, b, _, _, _, _, _, h0⟩
  · rw [h0] at hph
    obtain ⟨h1, h2, h3⟩ := pf_serving_eq hph
    exact ⟨h1.symm, h2.symm, h3.symm, hall⟩
  · rw [h0] at hph; cases hph
  · rw [h0] at hph; cases hph

lemma pf_inv_deadlines {tr : PassTrace ops now} {k : Nat} {m : Nat} {n' : T}
    (h : TraceInv tr k) (hph : (tr.seen k).phase = .deadlines n' m) :
    n' = now ∧ m = nextHandler tr.answer k ∧ ∃ j, j < k ∧ IsRun (tr.answer j) := by
  rcases h with ⟨hall, h0⟩ | ⟨a, hak, _, hrun, h0⟩ | ⟨a, b, _, _, _, _, _, h0⟩
  · rw [h0] at hph; cases hph
  · rw [h0] at hph
    obtain ⟨h1, h2⟩ := pf_deadlines_eq hph
    exact ⟨h1.symm, h2.symm, a, hak, hrun a (Nat.le_refl a) hak⟩
  · rw [h0] at hph; cases hph

lemma pf_inv_done {tr : PassTrace ops now} {k : Nat}
    (h : TraceInv tr k) (hph : (tr.seen k).phase = .done) :
    ∃ j, j < k ∧ tr.answer j = .passDone := by
  rcases h with ⟨hall, h0⟩ | ⟨a, hak, _, hrun, h0⟩ | ⟨a, b, _, hbk, _, _, hdone, h0⟩
  · rw [h0] at hph; cases hph
  · rw [h0] at hph; cases hph
  · exact ⟨b, hbk, hdone b (Nat.le_refl b) hbk⟩

/-- The shape of the first `n` answers: `a` serves, then runs up to `b`, then passes done. -/
lemma pf_shape {tr : PassTrace ops now} {n : Nat} (h : TraceInv tr n) :
    ∃ a b, a ≤ b ∧ b ≤ n ∧ ∀ j, j < n →
      (j < a → IsServe (tr.answer j)) ∧ (a ≤ j → j < b → IsRun (tr.answer j)) ∧
      (b ≤ j → tr.answer j = .passDone) := by
  rcases h with ⟨hall, _⟩ | ⟨a, hak, hserve, hrun, _⟩ | ⟨a, b, hab, hbk, hserve, hrun, hdone, _⟩
  · refine ⟨n, n, Nat.le_refl n, Nat.le_refl n, fun j hj => ⟨fun _ => hall j hj, ?_, ?_⟩⟩
    · intro h1 h2; omega
    · intro h1; omega
  · refine ⟨a, n, by omega, Nat.le_refl n, fun j hj => ⟨hserve j, fun h1 _ => hrun j h1 hj, ?_⟩⟩
    intro h1; omega
  · exact ⟨a, b, hab, by omega, fun j hj =>
      ⟨hserve j, fun h1 h2 => hrun j h1 h2, fun h1 => hdone j h1 hj⟩⟩

lemma pf_nextHandler_zero (answer : Nat → Answer) :
    ∀ k, (∀ j, j < k → IsServe (answer j)) → nextHandler answer k = 0 := by
  intro k
  induction k with
  | zero => intro _; rfl
  | succ k ih =>
    intro hall
    obtain ⟨i, hi⟩ := hall k (Nat.lt_succ_self k)
    have := ih (fun j hj => hall j (by omega))
    simp [nextHandler, hi, this]

/-! What each kind of answer tells us at call `k`. -/

lemma pf_call_serve (hops : ops.Meets) (tr : PassTrace ops now) {k j : Nat}
    (hk : tr.answer k = .serve j) :
    curPos tr.answer k ≤ j ∧ EligibleAt tr k j ∧
      ∀ i, curPos tr.answer k ≤ i → i < j → ¬ EligibleAt tr k i := by
  have hc := pf_nt_cases hops (tr.call k)
  have hinv := pf_inv_all hops tr k
  cases hc with
  | idle _ ha _ => rw [hk] at ha; cases ha
  | serve now' pos used j' hphN hpos hel hbf ha _ =>
    rw [hk] at ha
    simp only [Answer.serve.injEq] at ha
    subst ha
    obtain ⟨rfl, rfl, rfl, _⟩ := pf_inv_serving hinv hphN
    exact ⟨hpos, hel, hbf⟩
  | runAfterSources _ _ _ _ _ _ _ _ ha _ => rw [hk] at ha; cases ha
  | doneAfterSources _ _ _ _ _ _ ha _ => rw [hk] at ha; cases ha
  | runAfterHandlers _ _ _ _ _ _ _ ha _ => rw [hk] at ha; cases ha
  | doneAfterHandlers _ _ _ _ ha _ => rw [hk] at ha; cases ha

lemma pf_call_run (hops : ops.Meets) (tr : PassTrace ops now) {k h : Nat}
    (hk : tr.answer k = .runDeadline h) :
    nextHandler tr.answer k ≤ h ∧ DueAt tr k h ∧
      ∀ i, nextHandler tr.answer k ≤ i → i < h → ¬ DueAt tr k i := by
  have hc := pf_nt_cases hops (tr.call k)
  have hinv := pf_inv_all hops tr k
  cases hc with
  | idle _ ha _ => rw [hk] at ha; cases ha
  | serve _ _ _ _ _ _ _ _ ha _ => rw [hk] at ha; cases ha
  | runAfterSources now' pos used h' hphN _ hdue hbf ha _ =>
    rw [hk] at ha
    simp only [Answer.runDeadline.injEq] at ha
    subst ha
    obtain ⟨rfl, _, _, hall⟩ := pf_inv_serving hinv hphN
    rw [pf_nextHandler_zero tr.answer k hall]
    exact ⟨Nat.zero_le _, hdue, fun i _ hi => hbf i hi⟩
  | doneAfterSources _ _ _ _ _ _ ha _ => rw [hk] at ha; cases ha
  | runAfterHandlers now' next h' hphN hle hdue hbf ha _ =>
    rw [hk] at ha
    simp only [Answer.runDeadline.injEq] at ha
    subst ha
    obtain ⟨rfl, rfl, _⟩ := pf_inv_deadlines hinv hphN
    exact ⟨hle, hdue, hbf⟩
  | doneAfterHandlers _ _ _ _ ha _ => rw [hk] at ha; cases ha

lemma pf_call_done (hops : ops.Meets) (tr : PassTrace ops now) {k : Nat}
    (hk : tr.answer k = .passDone) (hfirst : ∀ j, j < k → tr.answer j ≠ .passDone) :
    ∀ i, nextHandler tr.answer k ≤ i → ¬ DueAt tr k i := by
  have hc := pf_nt_cases hops (tr.call k)
  have hinv := pf_inv_all hops tr k
  cases hc with
  | idle hphN _ _ =>
    obtain ⟨j, hj, hjd⟩ := pf_inv_done hinv hphN
    exact absurd hjd (hfirst j hj)
  | serve _ _ _ _ _ _ _ _ ha _ => rw [hk] at ha; cases ha
  | runAfterSources _ _ _ _ _ _ _ _ ha _ => rw [hk] at ha; cases ha
  | doneAfterSources now' pos used hphN _ hnodue _ _ =>
    obtain ⟨rfl, _⟩ := pf_inv_serving hinv hphN
    intro i _
    exact hnodue i
  | runAfterHandlers _ _ _ _ _ _ _ ha _ => rw [hk] at ha; cases ha
  | doneAfterHandlers now' next hphN hnodue _ _ =>
    obtain ⟨rfl, rfl, _⟩ := pf_inv_deadlines hinv hphN
    exact hnodue

lemma pf_call_first_non_serve (hops : ops.Meets) (tr : PassTrace ops now) {k : Nat}
    (hbefore : ∀ j, j < k → IsServe (tr.answer j)) (hk : ¬ IsServe (tr.answer k)) :
    ∀ i, curPos tr.answer k ≤ i → ¬ EligibleAt tr k i := by
  have hc := pf_nt_cases hops (tr.call k)
  have hinv := pf_inv_all hops tr k
  cases hc with
  | idle hphN _ _ =>
    obtain ⟨j, hj, hjd⟩ := pf_inv_done hinv hphN
    obtain ⟨i, hi⟩ := hbefore j hj
    rw [hjd] at hi
    cases hi
  | serve _ _ _ j _ _ _ _ ha _ => exact absurd ⟨j, ha⟩ hk
  | runAfterSources now' pos used h' hphN hnone _ _ _ _ =>
    obtain ⟨_, rfl, rfl, _⟩ := pf_inv_serving hinv hphN
    exact hnone
  | doneAfterSources now' pos used hphN hnone _ _ _ =>
    obtain ⟨_, rfl, rfl, _⟩ := pf_inv_serving hinv hphN
    exact hnone
  | runAfterHandlers _ _ _ hphN _ _ _ _ _ =>
    obtain ⟨_, _, j, hj, i, hi⟩ := pf_inv_deadlines hinv hphN
    obtain ⟨i', hi'⟩ := hbefore j hj
    rw [hi'] at hi
    cases hi
  | doneAfterHandlers _ _ hphN _ _ _ =>
    obtain ⟨_, _, j, hj, i, hi⟩ := pf_inv_deadlines hinv hphN
    obtain ⟨i', hi'⟩ := hbefore j hj
    rw [hi'] at hi
    cases hi

/-! Things that stay fixed along a trace. -/

lemma pf_nt_hlen (hops : ops.Meets) {s s' : LoopState T} {a : Answer}
    (h : ops.nextTurn s = .ok (a, s')) : s'.handlers.length = s.handlers.length := by
  have := (pf_nt_keeps hops h).2.2
  simpa using congrArg List.length this

lemma pf_budgets (hops : ops.Meets) (tr : PassTrace ops now) (k : Nat) :
    (tr.seen k).sources.map (·.budget) = (tr.seen 0).sources.map (·.budget) := by
  induction k with
  | zero => rfl
  | succ k ih =>
    rw [(tr.change k).2.1,
      (pf_nt_keeps hops (tr.call k)).2.1, ih]

lemma pf_hlen (hops : ops.Meets) (tr : PassTrace ops now) (k : Nat) :
    (tr.seen k).handlers.length = (tr.seen 0).handlers.length := by
  induction k with
  | zero => rfl
  | succ k ih => rw [(tr.change k).2.2, pf_nt_hlen hops (tr.call k), ih]

lemma pf_slen (hops : ops.Meets) (tr : PassTrace ops now) (k : Nat) :
    (tr.seen k).sources.length = (tr.seen 0).sources.length := by
  simpa using congrArg List.length (pf_budgets hops tr k)

lemma pf_curPos_le_succ (hops : ops.Meets) (tr : PassTrace ops now) (k : Nat) :
    curPos tr.answer k ≤ curPos tr.answer (k + 1) := by
  cases hk : tr.answer k with
  | serve i =>
    have : curPos tr.answer (k + 1) = i := by simp [curPos, hk]
    rw [this]
    exact (pf_call_serve hops tr hk).1
  | runDeadline h => simp [curPos, hk]
  | passDone => simp [curPos, hk]

lemma pf_curPos_mono (hops : ops.Meets) (tr : PassTrace ops now) {j k : Nat} (h : j ≤ k) :
    curPos tr.answer j ≤ curPos tr.answer k := by
  exact pf_mono_of_succ _ (pf_curPos_le_succ hops tr) h

lemma pf_nextHandler_le_succ (hops : ops.Meets) (tr : PassTrace ops now) (k : Nat) :
    nextHandler tr.answer k ≤ nextHandler tr.answer (k + 1) := by
  cases hk : tr.answer k with
  | serve i => simp [nextHandler, hk]
  | runDeadline h =>
    have : nextHandler tr.answer (k + 1) = h + 1 := by simp [nextHandler, hk]
    rw [this]
    exact Nat.le_succ_of_le (pf_call_run hops tr hk).1
  | passDone => simp [nextHandler, hk]

lemma pf_nextHandler_mono (hops : ops.Meets) (tr : PassTrace ops now) {j k : Nat} (h : j ≤ k) :
    nextHandler tr.answer j ≤ nextHandler tr.answer k := by
  exact pf_mono_of_succ _ (pf_nextHandler_le_succ hops tr) h

lemma pf_nextHandler_le_count (hops : ops.Meets) (tr : PassTrace ops now) (k : Nat) :
    nextHandler tr.answer k ≤ (tr.seen 0).handlers.length := by
  induction k with
  | zero => exact Nat.zero_le _
  | succ k ih =>
    cases hk : tr.answer k with
    | serve i => simp [nextHandler, hk]; exact ih
    | runDeadline h =>
      have : nextHandler tr.answer (k + 1) = h + 1 := by simp [nextHandler, hk]
      rw [this]
      obtain ⟨x, d, hx, _⟩ := (pf_call_run hops tr hk).2.1
      have := pf_lt_of_some hx
      rw [pf_hlen hops tr k] at this
      exact this
    | passDone => simp [nextHandler, hk]; exact ih

/-! ## S2. Budget -/

theorem S2_served_at_most_budget (hops : ops.Meets) (tr : PassTrace ops now) (i : Nat) (x : Src)
    (hx : (tr.seen 0).sources[i]? = some x) (n : Nat) :
    serveCount tr.answer n i ≤ x.budget := by
  induction n with
  | zero => simp [serveCount]
  | succ n ih =>
    rw [pf_serveCount_succ]
    by_cases hs : tr.answer n = .serve i
    · simp only [hs, ↓reduceIte]
      obtain ⟨_, ⟨y, hy, _, _, hlt⟩, _⟩ := pf_call_serve hops tr hs
      have hb := congrArg (fun l => l[i]?) (pf_budgets hops tr n)
      simp only [List.getElem?_map, hy, hx, Option.map_some] at hb
      have hb' := Option.some.inj hb
      omega
    · simp only [hs, ↓reduceIte]
      omega

def ServedBelow (answer : Nat → Answer) : Nat → Nat → Nat
  | 0, _ => 0
  | n + 1, m => ServedBelow answer n m +
      (match answer n with
        | .serve i => if i < m then 1 else 0
        | _ => 0)

lemma pf_sb_zero (answer : Nat → Answer) (n : Nat) : ServedBelow answer n 0 = 0 := by
  induction n with
  | zero => rfl
  | succ n ih =>
    unfold ServedBelow
    rw [ih]
    cases answer n <;> simp

lemma pf_sb_succ (answer : Nat → Answer) (n m : Nat) :
    ServedBelow answer n (m + 1) = ServedBelow answer n m + serveCount answer n m := by
  induction n with
  | zero => simp [ServedBelow, serveCount]
  | succ n ih =>
    rw [ServedBelow, ServedBelow, pf_serveCount_succ, ih]
    cases hk : answer n with
    | serve i =>
      by_cases him : i = m
      · subst him
        simp; omega
      · have : ¬ m = i := fun e => him e.symm
        by_cases hlt : i < m
        · simp [hlt, him, Nat.lt_succ_of_lt hlt]; omega
        · have : ¬ i < m + 1 := by omega
          simp [hlt, him, this]
    | runDeadline h => simp
    | passDone => simp

lemma pf_sb_le (hops : ops.Meets) (tr : PassTrace ops now) (n : Nat) :
    ∀ m, m ≤ (tr.seen 0).sources.length →
      ServedBelow tr.answer n m ≤ (((tr.seen 0).sources.map (·.budget)).take m).sum := by
  intro m
  induction m with
  | zero => intro _; simp [pf_sb_zero]
  | succ m ih =>
    intro hm
    have ih' := ih (by omega)
    obtain ⟨x, hx⟩ : ∃ x, (tr.seen 0).sources[m]? = some x :=
      ⟨_, List.getElem?_eq_getElem (by omega)⟩
    have hs2 := S2_served_at_most_budget hops tr m x hx n
    rw [pf_sb_succ, List.take_add_one, List.sum_append]
    have : ((tr.seen 0).sources.map (·.budget))[m]? = some x.budget := by simp [hx]
    rw [this]
    simp only [Option.toList_some, List.sum_cons, List.sum_nil]
    omega

lemma pf_sb_total (hops : ops.Meets) (tr : PassTrace ops now) (n : Nat) :
    ServedBelow tr.answer n (tr.seen 0).sources.length ≤
      ((tr.seen 0).sources.map (·.budget)).sum := by
  have := pf_sb_le hops tr n (tr.seen 0).sources.length (Nat.le_refl _)
  rw [List.take_of_length_le (by simp)] at this
  exact this

/-- Without a "pass done" before call `n`, each call so far was a serve or a run, and the
serves and runs are bounded by the budgets and the handler count (used for S11). -/
lemma pf_count_bound (hops : ops.Meets) (tr : PassTrace ops now) (n : Nat) :
    (∀ j, j < n → tr.answer j ≠ .passDone) →
    n ≤ ServedBelow tr.answer n (tr.seen 0).sources.length + nextHandler tr.answer n := by
  induction n with
  | zero => intro _; simp
  | succ n ih =>
    intro hno
    have ih' := ih (fun j hj => hno j (by omega))
    cases hk : tr.answer n with
    | serve i =>
      have hlt : i < (tr.seen 0).sources.length := by
        obtain ⟨_, ⟨y, hy, _⟩, _⟩ := pf_call_serve hops tr hk
        have := pf_lt_of_some hy
        rw [pf_slen hops tr n] at this
        exact this
      have e : nextHandler tr.answer (n + 1) = nextHandler tr.answer n := by
        simp [nextHandler, hk]
      rw [e]
      simp only [ServedBelow, hk, hlt, ↓reduceIte]
      omega
    | runDeadline h =>
      have e : nextHandler tr.answer (n + 1) = h + 1 := by simp [nextHandler, hk]
      have := (pf_call_run hops tr hk).1
      rw [e]
      simp only [ServedBelow, hk]
      omega
    | passDone => exact absurd hk (hno n (Nat.lt_succ_self n))


/-! ## S3. Order and shape -/

theorem S3_source_numbers_never_go_down (hops : ops.Meets) (tr : PassTrace ops now)
    (j k a b : Nat) (hjk : j < k) (hj : tr.answer j = .serve a) (hk : tr.answer k = .serve b) :
    a ≤ b := by
  have h1 : curPos tr.answer (j + 1) = a := by simp [curPos, hj]
  have h2 := pf_curPos_mono hops tr (show j + 1 ≤ k by omega)
  have h3 := (pf_call_serve hops tr hk).1
  omega

theorem S3_no_serve_after_a_non_serve (hops : ops.Meets) (tr : PassTrace ops now)
    (j k : Nat) (hjk : j < k) (hj : ¬ IsServe (tr.answer j)) : ¬ IsServe (tr.answer k) := by
  obtain ⟨a, b, hab, hbn, hall⟩ := pf_shape (pf_inv_all hops tr (k + 1))
  have hj' := hall j (by omega)
  have hk' := hall k (by omega)
  have haj : a ≤ j := by
    by_contra hlt
    exact hj (hj'.1 (by omega))
  rintro ⟨i, hi⟩
  rcases Nat.lt_or_ge k b with hkb | hkb
  · obtain ⟨h, hh⟩ := hk'.2.1 (by omega) hkb
    rw [hi] at hh
    cases hh
  · have := hk'.2.2 hkb
    rw [hi] at this
    cases this

theorem S3_handler_numbers_strictly_go_up (hops : ops.Meets) (tr : PassTrace ops now)
    (j k a b : Nat) (hjk : j < k) (hj : tr.answer j = .runDeadline a)
    (hk : tr.answer k = .runDeadline b) : a < b := by
  have h1 : nextHandler tr.answer (j + 1) = a + 1 := by simp [nextHandler, hj]
  have h2 := pf_nextHandler_mono hops tr (show j + 1 ≤ k by omega)
  have h3 := (pf_call_run hops tr hk).1
  omega

theorem S3_pass_done_stays_done (hops : ops.Meets) (tr : PassTrace ops now)
    (j k : Nat) (hjk : j ≤ k) (hj : tr.answer j = .passDone) : tr.answer k = .passDone := by
  obtain ⟨a, b, hab, hbn, hall⟩ := pf_shape (pf_inv_all hops tr (k + 1))
  have hj' := hall j (by omega)
  have hbj : b ≤ j := by
    by_contra hlt
    by_cases haj : j < a
    · obtain ⟨i, hi⟩ := hj'.1 haj
      rw [hj] at hi
      cases hi
    · obtain ⟨h, hh⟩ := hj'.2.1 (by omega) (by omega)
      rw [hj] at hh
      cases hh
  exact (hall k (by omega)).2.2 (by omega)

/-! ## S4. Checked at each answer -/

theorem S4_served_source_was_eligible (hops : ops.Meets) (tr : PassTrace ops now) (k i : Nat)
    (hk : tr.answer k = .serve i) : EligibleAt tr k i := by
  exact (pf_call_serve hops tr hk).2.1

theorem S4_run_handler_was_due (hops : ops.Meets) (tr : PassTrace ops now) (k h : Nat)
    (hk : tr.answer k = .runDeadline h) : DueAt tr k h := by
  exact (pf_call_run hops tr hk).2.1

/-! ## S5. Paused means never -/

theorem S5_paused_source_never_served (hops : ops.Meets) (tr : PassTrace ops now) (i n : Nat)
    (hpaused : ∀ k, k < n → ∃ x, (tr.seen k).sources[i]? = some x ∧ x.paused = true) :
    ∀ k, k < n → tr.answer k ≠ .serve i := by
  intro k hk heq
  obtain ⟨_, ⟨x, hx, _, hp, _⟩, _⟩ := pf_call_serve hops tr heq
  obtain ⟨y, hy, hpy⟩ := hpaused k hk
  have hxy := Option.some.inj (hx.symm.trans hy)
  subst hxy
  rw [hp] at hpy
  cases hpy

theorem S5_paused_handler_never_runs (hops : ops.Meets) (tr : PassTrace ops now) (h n : Nat)
    (hpaused : ∀ k, k < n → ∃ x, (tr.seen k).handlers[h]? = some x ∧ x.paused = true) :
    ∀ k, k < n → tr.answer k ≠ .runDeadline h := by
  intro k hk heq
  obtain ⟨_, ⟨x, d, hx, _, hp, _⟩, _⟩ := pf_call_run hops tr heq
  obtain ⟨y, hy, hpy⟩ := hpaused k hk
  have hxy := Option.some.inj (hx.symm.trans hy)
  subst hxy
  rw [hp] at hpy
  cases hpy

theorem S5_pausing_a_source_keeps_ready (hops : ops.Meets) (s s' : LoopState T) (i : Nat)
    (b : Bool) (h : ops.setPaused s i b = .ok s') (r : Bool) (hr : SourceReady s i r) :
    SourceReady s' i r := by
  obtain ⟨hlt, hs⟩ := pf_set_paused hops h
  obtain ⟨x, hx, hrx⟩ := hr
  subst hs
  refine ⟨{ x with paused := b }, ?_, hrx⟩
  exact pf_modifyAt_self _ _ _ _ hx

theorem S5_pausing_a_handler_keeps_deadline (hops : ops.Meets) (s s' : LoopState T) (h : Nat)
    (b : Bool) (hs : ops.setDeadlinePaused s h b = .ok s') (x : Hdl T)
    (hx : s.handlers[h]? = some x) :
    ∃ x', s'.handlers[h]? = some x' ∧ x'.deadline = x.deadline := by
  obtain ⟨hlt, hs'⟩ := pf_set_deadline_paused hops hs
  subst hs'
  exact ⟨{ x with paused := b }, pf_modifyAt_self _ _ _ _ hx, rfl⟩

/-! ## S6. Sources are not skipped -/

theorem S6_no_eligible_source_skipped (hops : ops.Meets) (tr : PassTrace ops now) (k j : Nat)
    (hk : tr.answer k = .serve j) :
    ∀ i, curPos tr.answer k ≤ i → i < j → ¬ EligibleAt tr k i := by
  exact (pf_call_serve hops tr hk).2.2

theorem S6_first_non_serve_leaves_no_eligible_source (hops : ops.Meets) (tr : PassTrace ops now)
    (k : Nat) (hbefore : ∀ j, j < k → IsServe (tr.answer j)) (hk : ¬ IsServe (tr.answer k)) :
    ∀ i, curPos tr.answer k ≤ i → ¬ EligibleAt tr k i := by
  exact pf_call_first_non_serve hops tr hbefore hk

theorem S6_no_source_behind_the_position_is_served (hops : ops.Meets) (tr : PassTrace ops now)
    (k i : Nat) (hk : tr.answer k = .serve i) : curPos tr.answer k ≤ i := by
  exact (pf_call_serve hops tr hk).1

lemma pf_run_once (hops : ops.Meets) (tr : PassTrace ops now) {h k m : Nat} (hkm : k < m)
    (hk : tr.answer k = .runDeadline h) : tr.answer m ≠ .runDeadline h := by
  intro hm
  have h1 : nextHandler tr.answer (k + 1) = h + 1 := by simp [nextHandler, hk]
  have h2 := pf_nextHandler_mono hops tr (show k + 1 ≤ m by omega)
  have h3 := (pf_call_run hops tr hm).1
  omega

lemma pf_env_slen {s s' : LoopState T} (h : Env s s') : s'.sources.length = s.sources.length := by
  simpa using congrArg List.length h.2.1

lemma pf_ready_false_stays (hops : ops.Meets) {s s' : LoopState T} {a : Answer}
    (h : ops.nextTurn s = .ok (a, s')) {i : Nat} (hr : SourceReady s i false) :
    SourceReady s' i false := by
  have hc := pf_nt_cases hops h
  obtain ⟨x, hx, hrx⟩ := hr
  cases hc with
  | idle _ _ hs => subst hs; exact ⟨x, hx, hrx⟩
  | serve now pos used j _ _ _ _ _ hs =>
    subst hs
    by_cases hij : i = j
    · subst hij
      exact ⟨{ x with ready := false }, pf_modifyAt_self _ _ _ _ hx, rfl⟩
    · exact ⟨x, (pf_modifyAt_other _ _ _ _ hij).trans hx, hrx⟩
  | runAfterSources _ _ _ _ _ _ _ _ _ hs => subst hs; exact ⟨x, hx, hrx⟩
  | doneAfterSources _ _ _ _ _ _ _ hs => subst hs; exact ⟨x, hx, hrx⟩
  | runAfterHandlers _ _ _ _ _ _ _ _ hs => subst hs; exact ⟨x, hx, hrx⟩
  | doneAfterHandlers _ _ _ _ _ hs => subst hs; exact ⟨x, hx, hrx⟩

/-- Once source `i` has ready flag false after call `k`, it can only be ready again at a later
call through a change by the application between two calls. -/
lemma pf_ready_trace (hops : ops.Meets) (tr : PassTrace ops now) (i k : Nat)
    (h0 : SourceReady (tr.after k) i false) :
    ∀ n, k ≤ n → SourceReady (tr.after n) i false ∨
      ∃ c, k ≤ c ∧ c < n ∧ SourceReady (tr.after c) i false ∧
        SourceReady (tr.seen (c + 1)) i true := by
  intro n
  induction n with
  | zero =>
    intro hn
    have : k = 0 := by omega
    subst this
    exact Or.inl h0
  | succ n ih =>
    intro hn
    rcases Nat.eq_or_lt_of_le hn with heq | hlt
    · subst heq
      exact Or.inl h0
    · rcases ih (by omega) with hf | ⟨c, hc1, hc2, hc3, hc4⟩
      · obtain ⟨x, hx, hrx⟩ := hf
        have hl : i < (tr.seen (n + 1)).sources.length := by
          rw [pf_env_slen (tr.change n)]
          exact pf_lt_of_some hx
        obtain ⟨y, hy⟩ : ∃ y, (tr.seen (n + 1)).sources[i]? = some y :=
          ⟨_, List.getElem?_eq_getElem hl⟩
        cases hry : y.ready with
        | true => exact Or.inr ⟨n, by omega, Nat.lt_succ_self n, ⟨x, hx, hrx⟩, ⟨y, hy, hry⟩⟩
        | false => exact Or.inl (pf_ready_false_stays hops (tr.call (n + 1)) ⟨y, hy, hry⟩)
      · exact Or.inr ⟨c, hc1, by omega, hc3, hc4⟩


/-! ## S7. Serving clears ready -/

theorem S7_serving_clears_ready_and_nothing_else (hops : ops.Meets) (s s' : LoopState T)
    (i : Nat) (h : ops.nextTurn s = .ok (.serve i, s')) (x : Src)
    (hx : s.sources[i]? = some x) :
    s'.sources[i]? = some { x with ready := false } ∧
    ∀ k, k ≠ i → s'.sources[k]? = s.sources[k]? := by
  have hc := pf_nt_cases hops h
  cases hc with
  | idle _ ha _ => cases ha
  | serve now pos used j _ _ _ _ ha hs =>
    simp only [Answer.serve.injEq] at ha
    subst ha
    subst hs
    exact ⟨pf_modifyAt_self _ _ _ _ hx, fun k hk => pf_modifyAt_other _ _ _ _ hk⟩
  | runAfterSources _ _ _ _ _ _ _ _ ha _ => cases ha
  | doneAfterSources _ _ _ _ _ _ ha _ => cases ha
  | runAfterHandlers _ _ _ _ _ _ _ ha _ => cases ha
  | doneAfterHandlers _ _ _ _ ha _ => cases ha

theorem S7_served_again_only_if_made_ready_again (hops : ops.Meets) (tr : PassTrace ops now)
    (i k m : Nat) (hkm : k < m) (hk : tr.answer k = .serve i) (hm : tr.answer m = .serve i) :
    ∃ c, k ≤ c ∧ c < m ∧ SourceReady (tr.after c) i false ∧ SourceReady (tr.seen (c + 1)) i true := by
  have hcall : ops.nextTurn (tr.seen k) = .ok (.serve i, tr.after k) := by
    rw [← hk]
    exact tr.call k
  obtain ⟨_, ⟨x, hx, _⟩, _⟩ := pf_call_serve hops tr hk
  have h0 : SourceReady (tr.after k) i false :=
    ⟨{ x with ready := false },
      (S7_serving_clears_ready_and_nothing_else hops _ _ i hcall x hx).1, rfl⟩
  obtain ⟨_, ⟨y, hy, hry, _⟩, _⟩ := pf_call_serve hops tr hm
  obtain ⟨m', rfl⟩ : ∃ m', m = m' + 1 := ⟨m - 1, by omega⟩
  have hready : SourceReady (tr.seen (m' + 1)) i true := ⟨y, hy, hry⟩
  rcases pf_ready_trace hops tr i k h0 m' (by omega) with hf | ⟨c, hc1, hc2, hc3, hc4⟩
  · exact ⟨m', by omega, by omega, hf, hready⟩
  · exact ⟨c, hc1, by omega, hc3, hc4⟩

/-! ## S8. Deadlines are not skipped -/

theorem S8_no_due_handler_jumped_over (hops : ops.Meets) (tr : PassTrace ops now) (k h : Nat)
    (hk : tr.answer k = .runDeadline h) :
    ∀ i, nextHandler tr.answer k ≤ i → i < h → ¬ DueAt tr k i := by
  exact (pf_call_run hops tr hk).2.2

theorem S8_first_pass_done_leaves_no_due_handler (hops : ops.Meets) (tr : PassTrace ops now)
    (k : Nat) (hk : tr.answer k = .passDone) (hfirst : ∀ j, j < k → tr.answer j ≠ .passDone) :
    ∀ i, nextHandler tr.answer k ≤ i → ¬ DueAt tr k i := by
  exact pf_call_done hops tr hk hfirst

/-- The pass has ended at its first "pass done", call `m`. Handler `h` is due at every call up to
and including the call that runs it (if it never runs: up to `m`). Then it runs exactly once. -/
theorem S8_due_handler_runs_exactly_once (hops : ops.Meets) (tr : PassTrace ops now) (h m : Nat)
    (hm : tr.answer m = .passDone) (hfirst : ∀ j, j < m → tr.answer j ≠ .passDone)
    (hdue : ∀ k, k ≤ m → (∀ j, j < k → tr.answer j ≠ .runDeadline h) → DueAt tr k h) :
    ∃ c, c < m ∧ tr.answer c = .runDeadline h ∧
      ∀ c', tr.answer c' = .runDeadline h → c' = c := by
  have key : ∀ n, n ≤ m → (∀ j, j < n → tr.answer j ≠ .runDeadline h) →
      nextHandler tr.answer n ≤ h := by
    intro n
    induction n with
    | zero => intro _ _; simp [nextHandler]
    | succ n ih =>
      intro hnm hno
      have ih' := ih (by omega) (fun j hj => hno j (by omega))
      cases hk : tr.answer n with
      | serve i => simp [nextHandler, hk]; exact ih'
      | passDone => simp [nextHandler, hk]; exact ih'
      | runDeadline h' =>
        have e : nextHandler tr.answer (n + 1) = h' + 1 := by simp [nextHandler, hk]
        rw [e]
        have hne : h' ≠ h := fun heq => hno n (Nat.lt_succ_self n) (by rw [hk, heq])
        by_contra hgt
        obtain ⟨_, _, hbf⟩ := pf_call_run hops tr hk
        exact hbf h ih' (by omega) (hdue n (by omega) (fun j hj => hno j (by omega)))
  have hex : ∃ c, c < m ∧ tr.answer c = .runDeadline h := by
    by_contra hne
    have hno : ∀ c, c < m → tr.answer c ≠ .runDeadline h := fun c hc hrc => hne ⟨c, hc, hrc⟩
    have h1 := key m (Nat.le_refl m) hno
    exact pf_call_done hops tr hm hfirst h h1 (hdue m (Nat.le_refl m) hno)
  obtain ⟨c, hc, hrc⟩ := hex
  refine ⟨c, hc, hrc, ?_⟩
  intro c' hc'
  rcases Nat.lt_trichotomy c' c with hlt | heq | hgt
  · exact absurd hrc (pf_run_once hops tr hlt hc')
  · exact heq
  · exact absurd hc' (pf_run_once hops tr hgt hrc)

/-! ## S9. Running a deadline clears it -/

theorem S9_running_clears_deadline_and_nothing_else (hops : ops.Meets) (s s' : LoopState T)
    (h : Nat) (hrun : ops.nextTurn s = .ok (.runDeadline h, s')) (x : Hdl T)
    (hx : s.handlers[h]? = some x) :
    s'.handlers[h]? = some { x with deadline := none } ∧
    ∀ k, k ≠ h → s'.handlers[k]? = s.handlers[k]? := by
  have hc := pf_nt_cases hops hrun
  cases hc with
  | idle _ ha _ => cases ha
  | serve _ _ _ _ _ _ _ _ ha _ => cases ha
  | runAfterSources now pos used hh _ _ _ _ ha hs' =>
    simp only [Answer.runDeadline.injEq] at ha
    subst ha
    subst hs'
    exact ⟨pf_modifyAt_self _ _ _ _ hx, fun k hk => pf_modifyAt_other _ _ _ _ hk⟩
  | doneAfterSources _ _ _ _ _ _ ha _ => cases ha
  | runAfterHandlers now next hh _ _ _ _ ha hs' =>
    simp only [Answer.runDeadline.injEq] at ha
    subst ha
    subst hs'
    exact ⟨pf_modifyAt_self _ _ _ _ hx, fun k hk => pf_modifyAt_other _ _ _ _ hk⟩
  | doneAfterHandlers _ _ _ _ ha _ => cases ha

theorem S9_handler_runs_at_most_once_per_pass (hops : ops.Meets) (tr : PassTrace ops now)
    (h k m : Nat) (hkm : k < m) (hk : tr.answer k = .runDeadline h) :
    tr.answer m ≠ .runDeadline h := by
  exact pf_run_once hops tr hkm hk

/-! ## S10. Time is fixed per pass -/

theorem S10_run_deadline_not_later_than_now (hops : ops.Meets) (tr : PassTrace ops now)
    (k h : Nat) (hk : tr.answer k = .runDeadline h) (x : Hdl T) (d : T)
    (hx : (tr.seen k).handlers[h]? = some x) (hd : x.deadline = some d) : d ≤ now := by
  obtain ⟨y, d', hy, hd', _, hle⟩ := (pf_call_run hops tr hk).2.1
  rw [hx] at hy
  have hxy := Option.some.inj hy
  subst hxy
  rw [hd] at hd'
  have hdd := Option.some.inj hd'
  subst hdd
  exact hle

/-! ## S11. A pass always ends -/

theorem S11_pass_ends_within_bound (hops : ops.Meets) (tr : PassTrace ops now) :
    ∃ m, m ≤ ((tr.seen 0).sources.map (·.budget)).sum + (tr.seen 0).handlers.length ∧
      tr.answer m = .passDone := by
  by_contra hne'
  have hne : ∀ m, m ≤ ((tr.seen 0).sources.map (·.budget)).sum + (tr.seen 0).handlers.length →
      tr.answer m ≠ .passDone := fun m hm hd => hne' ⟨m, hm, hd⟩
  have h1 := pf_count_bound hops tr
    (((tr.seen 0).sources.map (·.budget)).sum + (tr.seen 0).handlers.length + 1)
    (fun j hj => hne j (by omega))
  have h2 := pf_sb_total hops tr
    (((tr.seen 0).sources.map (·.budget)).sum + (tr.seen 0).handlers.length + 1)
  have h3 := pf_nextHandler_le_count hops tr
    (((tr.seen 0).sources.map (·.budget)).sum + (tr.seen 0).handlers.length + 1)
  omega

/-! ## S12. Next deadline -/

theorem S12_none_means_no_armed_handler (hops : ops.Meets) (s : LoopState T)
    (h : ops.nextDeadline s = .ok none) :
    ∀ x ∈ s.handlers, x.paused = false → x.deadline = none := by
  have hs := hops.nextDeadline s
  rw [h] at hs
  obtain ⟨o, ho, he⟩ := hs
  simp only [Outcome.ok.injEq] at ho
  subst ho
  rcases he with ⟨_, h1⟩ | ⟨d, hd, _⟩
  · exact h1
  · cases hd

theorem S12_some_is_the_earliest_armed_deadline (hops : ops.Meets) (s : LoopState T) (d : T)
    (h : ops.nextDeadline s = .ok (some d)) :
    (∃ x ∈ s.handlers, x.paused = false ∧ x.deadline = some d) ∧
    ∀ x ∈ s.handlers, x.paused = false → ∀ e, x.deadline = some e → d ≤ e := by
  have hs := hops.nextDeadline s
  rw [h] at hs
  obtain ⟨o, ho, he⟩ := hs
  simp only [Outcome.ok.injEq] at ho
  subst ho
  rcases he with ⟨hd, _⟩ | ⟨d', hd', hex, hall⟩
  · cases hd
  · simp only [Option.some.injEq] at hd'
    subst hd'
    exact ⟨hex, hall⟩

/-! ## S13. More work -/

theorem S13_has_ready_source_exactly_when (hops : ops.Meets) (s : LoopState T) (b : Bool)
    (h : ops.hasReadySource s = .ok b) : b = true ↔ ReadySourceExists s.sources := by
  obtain ⟨b', hb, hiff⟩ := hops.hasReadySource s
  rw [h] at hb
  simp only [Outcome.ok.injEq] at hb
  subst hb
  exact hiff

theorem S13_ignores_budget_used_in_the_pass (hops : ops.Meets) (s s' : LoopState T)
    (hsources : s.sources = s'.sources) : ops.hasReadySource s = ops.hasReadySource s' := by
  obtain ⟨b1, e1, i1⟩ := hops.hasReadySource s
  obtain ⟨b2, e2, i2⟩ := hops.hasReadySource s'
  rw [hsources] at i1
  have hb : b1 = b2 := Bool.eq_iff_iff.2 (i1.trans i2.symm)
  rw [e1, e2, hb]

/-! ## S14. Nothing else changes -/

theorem S14_next_turn_keeps_pause_flags_and_budgets (hops : ops.Meets) (s s' : LoopState T)
    (a : Answer) (h : ops.nextTurn s = .ok (a, s')) :
    s'.sources.map (·.paused) = s.sources.map (·.paused) ∧
    s'.sources.map (·.budget) = s.sources.map (·.budget) ∧
    s'.handlers.map (·.paused) = s.handlers.map (·.paused) := by
  exact pf_nt_keeps hops h

theorem S14_next_turn_keeps_other_sources (hops : ops.Meets) (s s' : LoopState T) (a : Answer)
    (h : ops.nextTurn s = .ok (a, s')) :
    ∀ k, a ≠ .serve k → s'.sources[k]? = s.sources[k]? := by
  have hc := pf_nt_cases hops h
  cases hc with
  | idle _ _ hs => subst hs; intro k _; rfl
  | serve now pos used j _ _ _ _ ha hs =>
    subst hs
    intro k hk
    subst ha
    exact pf_modifyAt_other _ _ _ _ (fun e => hk (by rw [e]))
  | runAfterSources _ _ _ _ _ _ _ _ _ hs => subst hs; intro k _; rfl
  | doneAfterSources _ _ _ _ _ _ _ hs => subst hs; intro k _; rfl
  | runAfterHandlers _ _ _ _ _ _ _ _ hs => subst hs; intro k _; rfl
  | doneAfterHandlers _ _ _ _ _ hs => subst hs; intro k _; rfl

theorem S14_next_turn_keeps_other_handlers (hops : ops.Meets) (s s' : LoopState T) (a : Answer)
    (h : ops.nextTurn s = .ok (a, s')) :
    ∀ k, a ≠ .runDeadline k → s'.handlers[k]? = s.handlers[k]? := by
  have hc := pf_nt_cases hops h
  cases hc with
  | idle _ _ hs => subst hs; intro k _; rfl
  | serve _ _ _ _ _ _ _ _ _ hs => subst hs; intro k _; rfl
  | runAfterSources now pos used hh _ _ _ _ ha hs =>
    subst hs
    intro k hk
    subst ha
    exact pf_modifyAt_other _ _ _ _ (fun e => hk (by rw [e]))
  | doneAfterSources _ _ _ _ _ _ _ hs => subst hs; intro k _; rfl
  | runAfterHandlers now next hh _ _ _ _ ha hs =>
    subst hs
    intro k hk
    subst ha
    exact pf_modifyAt_other _ _ _ _ (fun e => hk (by rw [e]))
  | doneAfterHandlers _ _ _ _ _ hs => subst hs; intro k _; rfl

/-! ## S15. Never fails -/

theorem S15_never_fails_for_existing_numbers (hops : ops.Meets) :
    (∀ budgets n, ∃ s : LoopState T, ops.new budgets n = .ok s) ∧
    (∀ (s : LoopState T) now, ∃ p, ops.beginPass s now = .ok p) ∧
    (∀ s : LoopState T, ∃ a s', ops.nextTurn s = .ok (a, s')) ∧
    (∀ (s : LoopState T) i b, i < s.sources.length → ∃ s', ops.setReady s i b = .ok s') ∧
    (∀ (s : LoopState T) i b, i < s.sources.length → ∃ s', ops.setPaused s i b = .ok s') ∧
    (∀ (s : LoopState T) h d, h < s.handlers.length → ∃ s', ops.setDeadline s h d = .ok s') ∧
    (∀ (s : LoopState T) h b, h < s.handlers.length →
      ∃ s', ops.setDeadlinePaused s h b = .ok s') ∧
    (∀ s : LoopState T, ∃ o, ops.nextDeadline s = .ok o) ∧
    (∀ s : LoopState T, ∃ b, ops.hasReadySource s = .ok b) := by
  refine ⟨?_, ?_, ?_, ?_, ?_, ?_, ?_, ?_, ?_⟩
  · intro budgets n
    have hs := hops.new budgets n
    unfold NewStep at hs
    exact ⟨_, hs⟩
  · intro s now
    have hs := hops.beginPass s now
    unfold BeginPassStep at hs
    exact ⟨_, hs⟩
  · exact pf_nt_exists hops
  · intro s i b hi
    exact ⟨_, (hops.setReady s i b).1 hi⟩
  · intro s i b hi
    exact ⟨_, (hops.setPaused s i b).1 hi⟩
  · intro s h d hh
    exact ⟨_, (hops.setDeadline s h d).1 hh⟩
  · intro s h b hh
    exact ⟨_, (hops.setDeadlinePaused s h b).1 hh⟩
  · intro s
    obtain ⟨o, ho, _⟩ := hops.nextDeadline s
    exact ⟨o, ho⟩
  · intro s
    obtain ⟨b, hb, _⟩ := hops.hasReadySource s
    exact ⟨b, hb⟩

theorem S15_nonexistent_number_panics (hops : ops.Meets) :
    (∀ (s : LoopState T) i b, s.sources.length ≤ i → ops.setReady s i b = .panic) ∧
    (∀ (s : LoopState T) i b, s.sources.length ≤ i → ops.setPaused s i b = .panic) ∧
    (∀ (s : LoopState T) h d, s.handlers.length ≤ h → ops.setDeadline s h d = .panic) ∧
    (∀ (s : LoopState T) h b, s.handlers.length ≤ h → ops.setDeadlinePaused s h b = .panic) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · intro s i b hi
    exact (hops.setReady s i b).2 hi
  · intro s i b hi
    exact (hops.setPaused s i b).2 hi
  · intro s h d hh
    exact (hops.setDeadline s h d).2 hh
  · intro s h b hh
    exact (hops.setDeadlinePaused s h b).2 hh

/-! ## S16. Starting again -/

theorem S16_begin_pass_restarts (hops : ops.Meets) (s p : LoopState T)
    (h : ops.beginPass s now = .ok p) :
    p.phase = .serving now 0 (fun _ => 0) ∧ p.sources = s.sources ∧ p.handlers = s.handlers := by
  have hs := hops.beginPass s now
  rw [h] at hs
  unfold BeginPassStep at hs
  simp only [Outcome.ok.injEq] at hs
  subst hs
  exact ⟨rfl, rfl, rfl⟩

theorem S16_begin_pass_forgets_the_old_pass (hops : ops.Meets) (s s' : LoopState T)
    (hsources : s.sources = s'.sources) (hhandlers : s.handlers = s'.handlers) :
    ops.beginPass s now = ops.beginPass s' now := by
  have e1 := hops.beginPass s now
  have e2 := hops.beginPass s' now
  unfold BeginPassStep at e1 e2
  rw [e1, e2]
  obtain ⟨a, b, c⟩ := s
  obtain ⟨a', b', c'⟩ := s'
  simp only at hsources hhandlers
  subst hsources hhandlers
  rfl

/-! ## S17. Setters change only their target -/

theorem S17_set_ready_changes_only_its_target (hops : ops.Meets) (s s' : LoopState T) (i : Nat)
    (b : Bool) (h : ops.setReady s i b = .ok s') (x : Src) (hx : s.sources[i]? = some x) :
    s'.sources[i]? = some { x with ready := b } ∧
    (∀ k, k ≠ i → s'.sources[k]? = s.sources[k]?) ∧
    s'.handlers = s.handlers ∧ s'.phase = s.phase := by
  obtain ⟨hlt, hs⟩ := pf_set_ready hops h
  subst hs
  exact ⟨pf_modifyAt_self _ _ _ _ hx, fun k hk => pf_modifyAt_other _ _ _ _ hk, rfl, rfl⟩

theorem S17_set_paused_changes_only_its_target (hops : ops.Meets) (s s' : LoopState T) (i : Nat)
    (b : Bool) (h : ops.setPaused s i b = .ok s') (x : Src) (hx : s.sources[i]? = some x) :
    s'.sources[i]? = some { x with paused := b } ∧
    (∀ k, k ≠ i → s'.sources[k]? = s.sources[k]?) ∧
    s'.handlers = s.handlers ∧ s'.phase = s.phase := by
  obtain ⟨hlt, hs'⟩ := pf_set_paused hops h
  subst hs'
  exact ⟨pf_modifyAt_self _ _ _ _ hx, fun k hk => pf_modifyAt_other _ _ _ _ hk, rfl, rfl⟩

theorem S17_set_deadline_changes_only_its_target (hops : ops.Meets) (s s' : LoopState T)
    (h : Nat) (d : Option T) (hs : ops.setDeadline s h d = .ok s') (x : Hdl T)
    (hx : s.handlers[h]? = some x) :
    s'.handlers[h]? = some { x with deadline := d } ∧
    (∀ k, k ≠ h → s'.handlers[k]? = s.handlers[k]?) ∧
    s'.sources = s.sources ∧ s'.phase = s.phase := by
  obtain ⟨hlt, hs'⟩ := pf_set_deadline hops hs
  subst hs'
  exact ⟨pf_modifyAt_self _ _ _ _ hx, fun k hk => pf_modifyAt_other _ _ _ _ hk, rfl, rfl⟩

theorem S17_set_deadline_paused_changes_only_its_target (hops : ops.Meets) (s s' : LoopState T)
    (h : Nat) (b : Bool) (hs : ops.setDeadlinePaused s h b = .ok s') (x : Hdl T)
    (hx : s.handlers[h]? = some x) :
    s'.handlers[h]? = some { x with paused := b } ∧
    (∀ k, k ≠ h → s'.handlers[k]? = s.handlers[k]?) ∧
    s'.sources = s.sources ∧ s'.phase = s.phase := by
  obtain ⟨hlt, hs'⟩ := pf_set_deadline_paused hops hs
  subst hs'
  exact ⟨pf_modifyAt_self _ _ _ _ hx, fun k hk => pf_modifyAt_other _ _ _ _ hk, rfl, rfl⟩

end EmEventLoop
