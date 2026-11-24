import StepSpec

/-! # A small pure reference implementation of the step spec

It shows that the step spec can be met (so it is not empty) and, with the uniqueness proofs at
the end, that it gives exactly one result per call. -/

set_option linter.unusedSectionVars false

namespace EmEventLoop

variable {T : Type} [LinearOrder T]

/-! ## Finding the first index from `start` where `p` holds -/

/-- Look at `start, start + 1, …` (at most `fuel` of them) and return the first where `p` holds. -/
def findFrom (p : Nat → Bool) (start : Nat) : Nat → Option Nat
  | 0 => none
  | fuel + 1 => if p start then some start else findFrom p (start + 1) fuel

theorem findFrom_some (p : Nat → Bool) :
    ∀ (fuel start j : Nat), findFrom p start fuel = some j →
      start ≤ j ∧ p j = true ∧ ∀ i, start ≤ i → i < j → p i = false := by
  intro fuel
  induction fuel with
  | zero => intro start j h; simp [findFrom] at h
  | succ fuel ih =>
    intro start j h
    unfold findFrom at h
    by_cases hp : p start = true
    · simp [hp] at h
      cases h
      exact ⟨Nat.le_refl _, hp, fun i h1 h2 => by omega⟩
    · simp [hp] at h
      obtain ⟨h1, h2, h3⟩ := ih (start + 1) j h
      refine ⟨by omega, h2, fun i hi hij => ?_⟩
      by_cases hs : i = start
      · subst hs; simpa using hp
      · exact h3 i (by omega) hij

theorem findFrom_none (p : Nat → Bool) :
    ∀ (fuel start : Nat), findFrom p start fuel = none →
      ∀ i, start ≤ i → i < start + fuel → p i = false := by
  intro fuel
  induction fuel with
  | zero => intro start _ i h1 h2; omega
  | succ fuel ih =>
    intro start h i h1 h2
    unfold findFrom at h
    by_cases hp : p start = true
    · simp [hp] at h
    · simp [hp] at h
      by_cases hs : i = start
      · subst hs; simpa using hp
      · exact ih (start + 1) h i (by omega) (by omega)

/-! ## Boolean versions of `Eligible` and `Due` -/

def refEligible (sources : List Src) (used : Nat → Nat) (i : Nat) : Bool :=
  match sources[i]? with
  | some x => x.ready && !x.paused && decide (used i < x.budget)
  | none => false

def refDue (handlers : List (Hdl T)) (now : T) (h : Nat) : Bool :=
  match handlers[h]? with
  | some x =>
    match x.deadline with
    | some d => !x.paused && decide (d ≤ now)
    | none => false
  | none => false

theorem refEligible_iff (sources : List Src) (used : Nat → Nat) (i : Nat) :
    refEligible sources used i = true ↔ Eligible sources used i := by
  unfold refEligible Eligible
  cases h : sources[i]? with
  | none => simp
  | some x => simp [Bool.and_eq_true]; grind

theorem refDue_iff (handlers : List (Hdl T)) (now : T) (h : Nat) :
    refDue handlers now h = true ↔ Due handlers now h := by
  unfold refDue Due
  cases hx : handlers[h]? with
  | none => simp
  | some x =>
    cases hd : x.deadline with
    | none => simp [hd]
    | some d => simp [hd]

theorem Eligible_lt {sources : List Src} {used : Nat → Nat} {i : Nat}
    (h : Eligible sources used i) : i < sources.length := by
  obtain ⟨x, hx, -⟩ := h
  exact (List.getElem?_eq_some_iff.1 hx).1

theorem Due_lt {handlers : List (Hdl T)} {now : T} {h : Nat}
    (hd : Due handlers now h) : h < handlers.length := by
  obtain ⟨x, d, hx, -⟩ := hd
  exact (List.getElem?_eq_some_iff.1 hx).1

/-! ## The reference operations -/

def refScanHandlers (s : LoopState T) (now : T) (start : Nat) : Answer × LoopState T :=
  match findFrom (refDue s.handlers now) start s.handlers.length with
  | some h =>
    (.runDeadline h,
      { s with handlers := modifyAt s.handlers h fun x => { x with deadline := none }
               phase := .deadlines now (h + 1) })
  | none => (.passDone, { s with phase := .done })

def refScanSources (s : LoopState T) (now : T) (position : Nat) (used : Nat → Nat) :
    Answer × LoopState T :=
  match findFrom (refEligible s.sources used) position s.sources.length with
  | some j =>
    (.serve j,
      { s with sources := modifyAt s.sources j fun x => { x with ready := false }
               phase := .serving now j (usedBump used j) })
  | none => refScanHandlers s now 0

def refNextTurn (s : LoopState T) : Answer × LoopState T :=
  match s.phase with
  | .done => (.passDone, s)
  | .serving now position used => refScanSources s now position used
  | .deadlines now next => refScanHandlers s now next

/-- Smaller of two optional deadlines; `none` means no deadline. -/
def combineDeadlines : Option T → Option T → Option T
  | none, o => o
  | some a, none => some a
  | some a, some b => some (min a b)

/-- The deadline of a handler that can fire: not paused and having a deadline. -/
def armedDeadline (x : Hdl T) : Option T := if x.paused then none else x.deadline

def refNextDeadline (handlers : List (Hdl T)) : Option T :=
  handlers.foldr (fun x acc => combineDeadlines (armedDeadline x) acc) none

def refOps : Ops T where
  new := fun budgets n =>
    .ok { sources := budgets.map fun b => { ready := false, paused := false, budget := b }
          handlers := List.replicate n { deadline := none, paused := false }
          phase := .done }
  beginPass := fun s now => .ok { s with phase := .serving now 0 (fun _ => 0) }
  nextTurn := fun s => .ok (refNextTurn s)
  setReady := fun s i b =>
    if i < s.sources.length then
      .ok { s with sources := modifyAt s.sources i fun x => { x with ready := b } }
    else .panic
  setPaused := fun s i b =>
    if i < s.sources.length then
      .ok { s with sources := modifyAt s.sources i fun x => { x with paused := b } }
    else .panic
  setDeadline := fun s h d =>
    if h < s.handlers.length then
      .ok { s with handlers := modifyAt s.handlers h fun x => { x with deadline := d } }
    else .panic
  setDeadlinePaused := fun s h b =>
    if h < s.handlers.length then
      .ok { s with handlers := modifyAt s.handlers h fun x => { x with paused := b } }
    else .panic
  nextDeadline := fun s => .ok (refNextDeadline s.handlers)
  hasReadySource := fun s =>
    .ok (s.sources.any fun x => x.ready && !x.paused && decide (0 < x.budget))

/-! ## The reference meets the step spec -/

theorem refScanHandlers_meets (s : LoopState T) (now : T) (start : Nat) :
    ScanHandlers s now start (.ok (refScanHandlers s now start)) := by
  unfold ScanHandlers refScanHandlers
  cases hf : findFrom (refDue s.handlers now) start s.handlers.length with
  | some h =>
    obtain ⟨h1, h2, h3⟩ := findFrom_some _ _ _ _ hf
    left
    refine ⟨h, h1, (refDue_iff _ _ _).1 h2, fun i hi hij hd => ?_, rfl⟩
    have := h3 i hi hij
    rw [(refDue_iff _ _ _).2 hd] at this
    cases this
  | none =>
    right
    refine ⟨fun i hi hd => ?_, rfl⟩
    have hlt := Due_lt hd
    have := findFrom_none _ _ _ hf i hi (by omega)
    rw [(refDue_iff _ _ _).2 hd] at this
    cases this

theorem refScanSources_meets (s : LoopState T) (now : T) (position : Nat) (used : Nat → Nat) :
    ScanSources s now position used (.ok (refScanSources s now position used)) := by
  unfold ScanSources refScanSources
  cases hf : findFrom (refEligible s.sources used) position s.sources.length with
  | some j =>
    obtain ⟨h1, h2, h3⟩ := findFrom_some _ _ _ _ hf
    left
    refine ⟨j, h1, (refEligible_iff _ _ _).1 h2, fun i hi hij he => ?_, rfl⟩
    have := h3 i hi hij
    rw [(refEligible_iff _ _ _).2 he] at this
    cases this
  | none =>
    right
    refine ⟨fun i hi he => ?_, refScanHandlers_meets s now 0⟩
    have hlt := Eligible_lt he
    have := findFrom_none _ _ _ hf i hi (by omega)
    rw [(refEligible_iff _ _ _).2 he] at this
    cases this

theorem armedDeadline_eq_some (x : Hdl T) (a : T) :
    armedDeadline x = some a ↔ x.paused = false ∧ x.deadline = some a := by
  unfold armedDeadline
  cases x.paused <;> simp

theorem armedDeadline_eq_none (x : Hdl T) :
    armedDeadline x = none ↔ (x.paused = false → x.deadline = none) := by
  unfold armedDeadline
  cases x.paused <;> simp

theorem refNextDeadline_earliest (handlers : List (Hdl T)) :
    EarliestDeadline handlers (refNextDeadline handlers) := by
  induction handlers with
  | nil => left; simp [refNextDeadline]
  | cons x rest ih =>
    show EarliestDeadline (x :: rest) (combineDeadlines (armedDeadline x) (refNextDeadline rest))
    rcases ih with ⟨ho, hrest⟩ | ⟨b, ho, ⟨y, hy, hyp, hyd⟩, hmin⟩
    · rw [ho]
      cases ha : armedDeadline x with
      | none =>
        left
        refine ⟨rfl, fun z hz hzp => ?_⟩
        rcases List.mem_cons.1 hz with hzx | hzr
        · rw [hzx] at hzp ⊢
          exact (armedDeadline_eq_none x).1 ha hzp
        · exact hrest z hzr hzp
      | some a =>
        obtain ⟨hp, hd⟩ := (armedDeadline_eq_some x a).1 ha
        right
        refine ⟨a, rfl, ⟨x, List.mem_cons_self, hp, hd⟩, fun z hz hzp e hze => ?_⟩
        rcases List.mem_cons.1 hz with hzx | hzr
        · rw [hzx, hd] at hze
          cases hze
          exact le_refl _
        · rw [hrest z hzr hzp] at hze
          cases hze
    · rw [ho]
      cases ha : armedDeadline x with
      | none =>
        right
        refine ⟨b, rfl, ⟨y, List.mem_cons_of_mem _ hy, hyp, hyd⟩, fun z hz hzp e hze => ?_⟩
        rcases List.mem_cons.1 hz with hzx | hzr
        · rw [hzx] at hzp hze
          rw [(armedDeadline_eq_none x).1 ha hzp] at hze
          cases hze
        · exact hmin z hzr hzp e hze
      | some a =>
        obtain ⟨hp, hd⟩ := (armedDeadline_eq_some x a).1 ha
        right
        refine ⟨min a b, rfl, ?_, fun z hz hzp e hze => ?_⟩
        · by_cases hab : a ≤ b
          · rw [min_eq_left hab]
            exact ⟨x, List.mem_cons_self, hp, hd⟩
          · rw [min_eq_right (not_le.1 hab).le]
            exact ⟨y, List.mem_cons_of_mem _ hy, hyp, hyd⟩
        · rcases List.mem_cons.1 hz with hzx | hzr
          · rw [hzx, hd] at hze
            cases hze
            exact min_le_left _ _
          · exact (min_le_right a b).trans (hmin z hzr hzp e hze)

theorem refOps_meets : (refOps : Ops T).Meets where
  new := fun _ _ => rfl
  beginPass := fun _ _ => rfl
  nextTurn := fun s => by
    unfold NextTurnStep refOps
    cases hp : s.phase with
    | done => simp [refNextTurn, hp]
    | serving now position used => simp only [refNextTurn, hp]; exact refScanSources_meets _ _ _ _
    | deadlines now next => simp only [refNextTurn, hp]; exact refScanHandlers_meets _ _ _
  setReady := fun s i b => by
    unfold SetReadyStep refOps
    constructor
    · intro h; simp [h]
    · intro h; simp [Nat.not_lt.2 h]
  setPaused := fun s i b => by
    unfold SetPausedStep refOps
    constructor
    · intro h; simp [h]
    · intro h; simp [Nat.not_lt.2 h]
  setDeadline := fun s i b => by
    unfold SetDeadlineStep refOps
    constructor
    · intro h; simp [h]
    · intro h; simp [Nat.not_lt.2 h]
  setDeadlinePaused := fun s i b => by
    unfold SetDeadlinePausedStep refOps
    constructor
    · intro h; simp [h]
    · intro h; simp [Nat.not_lt.2 h]
  nextDeadline := fun s => ⟨_, rfl, refNextDeadline_earliest _⟩
  hasReadySource := fun s => by
    refine ⟨_, rfl, ?_⟩
    simp [ReadySourceExists, Bool.and_eq_true, and_assoc]

/-! ## The step spec gives exactly one result per call -/

theorem first_unique {P : Nat → Prop} {a j j' : Nat}
    (hj : a ≤ j) (hPj : P j) (hmin : ∀ i, a ≤ i → i < j → ¬ P i)
    (hj' : a ≤ j') (hPj' : P j') (hmin' : ∀ i, a ≤ i → i < j' → ¬ P i) : j = j' := by
  by_contra hne
  rcases Nat.lt_or_gt_of_ne hne with hlt | hlt
  · exact hmin' j hj hlt hPj
  · exact hmin j' hj' hlt hPj'

theorem scanHandlers_unique {s : LoopState T} {now : T} {start : Nat}
    {r r' : Outcome (Answer × LoopState T)}
    (h : ScanHandlers s now start r) (h' : ScanHandlers s now start r') : r = r' := by
  rcases h with ⟨j, h1, h2, h3, hr⟩ | ⟨hn, hr⟩ <;>
    rcases h' with ⟨j', h1', h2', h3', hr'⟩ | ⟨hn', hr'⟩
  · obtain rfl := first_unique h1 h2 h3 h1' h2' h3'
    rw [hr, hr']
  · exact absurd h2 (hn' j h1)
  · exact absurd h2' (hn j' h1')
  · rw [hr, hr']

theorem scanSources_unique {s : LoopState T} {now : T} {position : Nat} {used : Nat → Nat}
    {r r' : Outcome (Answer × LoopState T)}
    (h : ScanSources s now position used r) (h' : ScanSources s now position used r') :
    r = r' := by
  rcases h with ⟨j, h1, h2, h3, hr⟩ | ⟨hn, hr⟩ <;>
    rcases h' with ⟨j', h1', h2', h3', hr'⟩ | ⟨hn', hr'⟩
  · obtain rfl := first_unique h1 h2 h3 h1' h2' h3'
    rw [hr, hr']
  · exact absurd h2 (hn' j h1)
  · exact absurd h2' (hn j' h1')
  · exact scanHandlers_unique hr hr'

theorem nextTurn_unique {s : LoopState T} {r r' : Outcome (Answer × LoopState T)}
    (h : NextTurnStep s r) (h' : NextTurnStep s r') : r = r' := by
  unfold NextTurnStep at h h'
  cases hp : s.phase with
  | done => simp only [hp] at h h'; rw [h, h']
  | serving now position used => simp only [hp] at h h'; exact scanSources_unique h h'
  | deadlines now next => simp only [hp] at h h'; exact scanHandlers_unique h h'

theorem guarded_unique {α : Type} {i len : Nat} {v r r' : Outcome α}
    (h : (i < len → r = v) ∧ (len ≤ i → r = .panic))
    (h' : (i < len → r' = v) ∧ (len ≤ i → r' = .panic)) : r = r' := by
  by_cases hi : i < len
  · rw [h.1 hi, h'.1 hi]
  · rw [h.2 (Nat.not_lt.1 hi), h'.2 (Nat.not_lt.1 hi)]

theorem earliestDeadline_unique {handlers : List (Hdl T)} {o o' : Option T}
    (h : EarliestDeadline handlers o) (h' : EarliestDeadline handlers o') : o = o' := by
  rcases h with ⟨ho, hn⟩ | ⟨d, ho, ⟨x, hx, hxp, hxd⟩, hmin⟩ <;>
    rcases h' with ⟨ho', hn'⟩ | ⟨d', ho', ⟨x', hx', hxp', hxd'⟩, hmin'⟩
  · rw [ho, ho']
  · rw [hn x' hx' hxp'] at hxd'
    cases hxd'
  · rw [hn' x hx hxp] at hxd
    cases hxd
  · have h1 := hmin x' hx' hxp' d' hxd'
    have h2 := hmin' x hx hxp d hxd
    rw [ho, ho', le_antisymm h1 h2]

theorem nextDeadline_unique {s : LoopState T} {r r' : Outcome (Option T)}
    (h : NextDeadlineStep s r) (h' : NextDeadlineStep s r') : r = r' := by
  obtain ⟨o, hr, ho⟩ := h
  obtain ⟨o', hr', ho'⟩ := h'
  rw [hr, hr', earliestDeadline_unique ho ho']

theorem hasReadySource_unique {s : LoopState T} {r r' : Outcome Bool}
    (h : HasReadySourceStep s r) (h' : HasReadySourceStep s r') : r = r' := by
  obtain ⟨b, hr, hb⟩ := h
  obtain ⟨b', hr', hb'⟩ := h'
  have : b = b' := by
    cases b <;> cases b' <;> simp_all
  rw [hr, hr', this]

theorem new_exactly_one (budgets : List Nat) (n : Nat) :
    ∃! r, NewStep (T := T) budgets n r :=
  ⟨_, refOps_meets.new budgets n, fun _ h => h⟩

theorem beginPass_exactly_one (s : LoopState T) (now : T) : ∃! r, BeginPassStep s now r :=
  ⟨_, refOps_meets.beginPass s now, fun _ h => h⟩

theorem nextTurn_exactly_one (s : LoopState T) : ∃! r, NextTurnStep s r :=
  ⟨_, refOps_meets.nextTurn s, fun _ h => nextTurn_unique h (refOps_meets.nextTurn s)⟩

theorem setReady_exactly_one (s : LoopState T) (i : Nat) (b : Bool) :
    ∃! r, SetReadyStep s i b r :=
  ⟨_, refOps_meets.setReady s i b, fun _ h => guarded_unique h (refOps_meets.setReady s i b)⟩

theorem setPaused_exactly_one (s : LoopState T) (i : Nat) (b : Bool) :
    ∃! r, SetPausedStep s i b r :=
  ⟨_, refOps_meets.setPaused s i b, fun _ h => guarded_unique h (refOps_meets.setPaused s i b)⟩

theorem setDeadline_exactly_one (s : LoopState T) (h : Nat) (d : Option T) :
    ∃! r, SetDeadlineStep s h d r :=
  ⟨_, refOps_meets.setDeadline s h d, fun _ hr =>
    guarded_unique hr (refOps_meets.setDeadline s h d)⟩

theorem setDeadlinePaused_exactly_one (s : LoopState T) (h : Nat) (b : Bool) :
    ∃! r, SetDeadlinePausedStep s h b r :=
  ⟨_, refOps_meets.setDeadlinePaused s h b, fun _ hr =>
    guarded_unique hr (refOps_meets.setDeadlinePaused s h b)⟩

theorem nextDeadline_exactly_one (s : LoopState T) : ∃! r, NextDeadlineStep s r :=
  ⟨_, refOps_meets.nextDeadline s, fun _ h => nextDeadline_unique h (refOps_meets.nextDeadline s)⟩

theorem hasReadySource_exactly_one (s : LoopState T) : ∃! r, HasReadySourceStep s r :=
  ⟨_, refOps_meets.hasReadySource s, fun _ h =>
    hasReadySource_unique h (refOps_meets.hasReadySource s)⟩

/-! ## A pass trace of the reference, for the examples -/

/-- The state seen by call `k` when the application changes the state with `env k` before it. -/
def refSeen (start : LoopState T) (now : T) (env : Nat → LoopState T → LoopState T) :
    Nat → LoopState T
  | 0 => env 0 { start with phase := .serving now 0 (fun _ => 0) }
  | k + 1 => env (k + 1) (refNextTurn (refSeen start now env k)).2

def refTrace (start : LoopState T) (now : T) (env : Nat → LoopState T → LoopState T)
    (henv : ∀ k s, Env s (env k s)) : PassTrace (refOps : Ops T) now where
  start := start
  seen := refSeen start now env
  after := fun k => (refNextTurn (refSeen start now env k)).2
  answer := fun k => (refNextTurn (refSeen start now env k)).1
  began := ⟨_, rfl, henv 0 _⟩
  call := fun _ => rfl
  change := fun k => henv (k + 1) _

end EmEventLoop
