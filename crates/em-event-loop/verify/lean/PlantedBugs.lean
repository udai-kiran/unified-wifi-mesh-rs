import Bridge
import Reference
import Mathlib.Order.Nat

/-! # G3 (note 0017 D4): the rule layer catches planted bugs

For each bug: `bugN : Ops Nat` is the reference operations with exactly that one defect, and
`bugN_caught : ¬ bugN.Meets` is proved by APPLYING the named rule theorem of `Rules.lean` to a
concrete bad run (a `PassTrace` where the rule is about a pass, a single call where it is not)
and getting `False`. The proof never unfolds `Meets` itself: `hops : bugN.Meets` is only handed
to the rule theorem.

Most defects of `next_turn` are made by changing one field of `Knobs`. With no field changed,
`kNextTurn` is the reference (`kNextTurn_default`), so a bug differs from the reference in
that one place only. -/

namespace EmEventLoop

set_option linter.unusedSectionVars false

abbrev Loop := LoopState Nat

/-! ## `next_turn` with one switchable defect -/

structure Knobs where
  /-- Source with `used` budget spent can be served. -/
  eligible : Src → Nat → Bool := fun x used => x.ready && !x.paused && decide (used < x.budget)
  /-- Handler is due at time `now`. -/
  due : Hdl Nat → Nat → Bool := fun x now =>
    match x.deadline with
    | some d => !x.paused && decide (d ≤ now)
    | none => false
  /-- Where the source scan starts, given the current position. -/
  sourceScanStart : Nat → Nat := fun position => position
  /-- Where the handler scan starts when the sources have nothing left. -/
  handlerScanStart : Nat := 0
  /-- The sources after "serve `j`". -/
  afterServe : Nat → List Src → List Src := fun j sources =>
    modifyAt sources j fun x => { x with ready := false }
  /-- The handlers after "run deadline `h`". -/
  afterRun : Nat → List (Hdl Nat) → List (Hdl Nat) := fun h handlers =>
    modifyAt handlers h fun x => { x with deadline := none }
  /-- The next handler to look at after "run deadline `h`". -/
  nextHandlerAfterRun : Nat → Nat := fun h => h + 1
  /-- The phase after "pass done", given the time of the pass. -/
  phaseAfterPassDone : Nat → Phase Nat := fun _ => .done

def kScanHandlers (k : Knobs) (s : Loop) (now start : Nat) : Answer × Loop :=
  let isDue := fun h =>
    match s.handlers[h]? with
    | some x => k.due x now
    | none => false
  match findFrom isDue start s.handlers.length with
  | some h =>
    (.runDeadline h,
      { s with handlers := k.afterRun h s.handlers
               phase := .deadlines now (k.nextHandlerAfterRun h) })
  | none => (.passDone, { s with phase := k.phaseAfterPassDone now })

def kScanSources (k : Knobs) (s : Loop) (now position : Nat) (used : Nat → Nat) :
    Answer × Loop :=
  let isEligible := fun i =>
    match s.sources[i]? with
    | some x => k.eligible x (used i)
    | none => false
  match findFrom isEligible (k.sourceScanStart position) s.sources.length with
  | some j =>
    (.serve j,
      { s with sources := k.afterServe j s.sources
               phase := .serving now j (usedBump used j) })
  | none => kScanHandlers k s now k.handlerScanStart

def kNextTurn (k : Knobs) (s : Loop) : Answer × Loop :=
  match s.phase with
  | .done => (.passDone, s)
  | .serving now position used => kScanSources k s now position used
  | .deadlines now next => kScanHandlers k s now next

def kOps (k : Knobs) : Ops Nat := { (refOps : Ops Nat) with nextTurn := fun s => .ok (kNextTurn k s) }

/-- With no switch changed, `next_turn` is the reference one. -/
theorem kNextTurn_default (s : Loop) : kNextTurn {} s = refNextTurn s := by
  have hdue : ∀ now, (fun h => match s.handlers[h]? with
      | some x => ({} : Knobs).due x now
      | none => false) = refDue s.handlers now := by
    intro now
    funext h
    unfold refDue
    cases s.handlers[h]? with
    | none => rfl
    | some x =>
      show Knobs.due _ x now = _
      cases hd : x.deadline with
      | none => simp [hd]
      | some d => simp [hd]
  have helig : ∀ used, (fun i => match s.sources[i]? with
      | some x => ({} : Knobs).eligible x (used i)
      | none => false) = refEligible s.sources used := by
    intro used
    funext i
    unfold refEligible
    cases s.sources[i]? with
    | none => rfl
    | some x => simp
  have hscanH : ∀ now start, kScanHandlers {} s now start = refScanHandlers s now start := by
    intro now start
    unfold kScanHandlers refScanHandlers
    simp only [hdue]
    rfl
  cases hp : s.phase with
  | done => simp [kNextTurn, refNextTurn, hp]
  | serving now position used =>
    simp only [kNextTurn, refNextTurn, hp, kScanSources, refScanSources]
    simp only [helig, hscanH]
    rfl
  | deadlines now next =>
    simp only [kNextTurn, refNextTurn, hp]
    exact hscanH now next

theorem kOps_call (k : Knobs) (s : Loop) (a : Answer) (h : (kNextTurn k s).1 = a) :
    (kOps k).nextTurn s = .ok (a, (kNextTurn k s).2) := by
  subst h
  rfl

/-! ## Passes of the buggy operations -/

/-- A pass of `kOps k`: `begin_pass now` on `start`, and the application changes the state with
`env k` before call `k`. -/
def knobSeen (k : Knobs) (p : Loop) (env : Nat → Loop → Loop) : Nat → Loop
  | 0 => env 0 p
  | n + 1 => env (n + 1) (kNextTurn k (knobSeen k p env n)).2

def knobTrace (k : Knobs) (start : Loop) (now : Nat) (env : Nat → Loop → Loop)
    (henv : ∀ n s, Env s (env n s)) : PassTrace (kOps k) now where
  start := start
  seen := knobSeen k { start with phase := .serving now 0 (fun _ => 0) } env
  after := fun n =>
    (kNextTurn k (knobSeen k { start with phase := .serving now 0 (fun _ => 0) } env n)).2
  answer := fun n =>
    (kNextTurn k (knobSeen k { start with phase := .serving now 0 (fun _ => 0) } env n)).1
  began := ⟨_, rfl, henv 0 _⟩
  call := fun _ => rfl
  change := fun n => henv (n + 1) _

/-- A loop that has just been created or finished a pass, with these sources and handlers. -/
def loopOf (sources : List Src) (handlers : List (Hdl Nat)) : Loop := ⟨sources, handlers, .done⟩

def readySrc (budget : Nat) : Src := ⟨true, false, budget⟩
def idleSrc (budget : Nat) : Src := ⟨false, false, budget⟩

/-! ### What the application does between answers -/

def armReady (i : Nat) (s : Loop) : Loop :=
  { s with sources := modifyAt s.sources i fun x => { x with ready := true } }

theorem armReady_env (i : Nat) (s : Loop) : Env s (armReady i s) :=
  ⟨rfl, pf_modifyAt_map (fun x : Src => x.budget) _ _ _ (fun _ => rfl), rfl⟩

def armDeadline (h : Nat) (s : Loop) : Loop :=
  { s with handlers := modifyAt s.handlers h fun x => { x with deadline := some 0 } }

theorem armDeadline_env (h : Nat) (s : Loop) : Env s (armDeadline h s) :=
  ⟨rfl, rfl, pf_modifyAt_length _ _ _⟩

/-- Apply `change` before call `n` only. -/
def envAt (n : Nat) (change : Loop → Loop) : Nat → Loop → Loop :=
  fun k s => if k = n then change s else s

theorem envAt_env (n : Nat) (change : Loop → Loop) (hchange : ∀ s, Env s (change s)) :
    ∀ k s, Env s (envAt n change k s) := by
  intro k s
  unfold envAt
  split
  · exact hchange s
  · exact Env_refl s

/-! ## S1. Start -/

def bug1a : Ops Nat := { (refOps : Ops Nat) with
  new := fun budgets n =>
    match (refOps : Ops Nat).new budgets n with
    | .ok s => .ok { s with sources := modifyAt s.sources 0 fun x => { x with ready := true } }
    | .panic => .panic }

/-- Caught by `S1_new_loop_is_idle`: source 0 of a new loop with budget 1 is ready. -/
theorem bug1a_caught : ¬ bug1a.Meets := fun hops => by
  have hnew : bug1a.new [1] 0 = .ok (loopOf [readySrc 1] []) := rfl
  have hidle := (S1_new_loop_is_idle hops [1] 0 _ hnew).2.1 (readySrc 1) (by simp [loopOf])
  simp [readySrc] at hidle

def bug1b : Ops Nat := { (refOps : Ops Nat) with
  nextTurn := fun s =>
    match s.phase with
    | .done => .ok (.serve 0, s)
    | _ => (refOps : Ops Nat).nextTurn s }

/-- Caught by `S1_next_turn_before_begin_pass`: `next_turn` on a new loop answers serve 0. -/
theorem bug1b_caught : ¬ bug1b.Meets := fun hops => by
  have hnew : bug1b.new [1] 0 = .ok (loopOf [idleSrc 1] []) := rfl
  have hidle := S1_next_turn_before_begin_pass hops [1] 0 _ _ hnew (Env_refl _)
  have hbug : bug1b.nextTurn (loopOf [idleSrc 1] []) = .ok (.serve 0, loopOf [idleSrc 1] []) :=
    rfl
  rw [hbug] at hidle
  simp at hidle

/-! ## S2. Budget -/

def bug2 : Ops Nat := kOps { eligible := fun x used => x.ready && !x.paused && decide (used ≤ x.budget) }

/-- Budget 1, made ready again after every answer: the source is served twice. -/
def trace2 : PassTrace bug2 0 :=
  knobTrace _ (loopOf [readySrc 1] []) 0 (fun _ s => armReady 0 s) (fun _ s => armReady_env 0 s)

/-- Caught by `S2_served_at_most_budget`. -/
theorem bug2_caught : ¬ bug2.Meets := fun hops => by
  have hle := S2_served_at_most_budget hops trace2 0 (readySrc 1) (by decide) 2
  have hcount : serveCount trace2.answer 2 0 = 2 := by decide
  rw [hcount] at hle
  exact absurd hle (by decide)

/-! ## S3. Order and shape -/

def bug3a : Ops Nat := kOps { phaseAfterPassDone := fun now => .serving now 0 (fun _ => 0) }

/-- Nothing is ready: "pass done" at call 0; the application then makes source 0 ready, and
the restarted scan serves it at call 1. -/
def trace3a : PassTrace bug3a 0 :=
  knobTrace _ (loopOf [idleSrc 1] []) 0 (envAt 1 (armReady 0)) (envAt_env 1 _ (armReady_env 0))

/-- Caught by `S3_pass_done_stays_done`. -/
theorem bug3a_caught : ¬ bug3a.Meets := fun hops => by
  have hdone : trace3a.answer 0 = .passDone := by decide
  have hstays := S3_pass_done_stays_done hops trace3a 0 1 (by omega) hdone
  have hserve : trace3a.answer 1 = .serve 0 := by decide
  rw [hserve] at hstays
  cases hstays

def bug3b : Ops Nat := kOps { nextHandlerAfterRun := fun _ => 0 }

/-- Two handlers due: run 0, run 1; the scan goes back to 0, where the application has set a
deadline again: run 0. -/
def trace3b : PassTrace bug3b 0 :=
  knobTrace _
    (loopOf [] [⟨some 0, false⟩, ⟨some 0, false⟩]) 0
    (envAt 2 (armDeadline 0)) (envAt_env 2 _ (armDeadline_env 0))

/-- Caught by `S3_handler_numbers_strictly_go_up`. -/
theorem bug3b_caught : ¬ bug3b.Meets := fun hops => by
  have h1 : trace3b.answer 1 = .runDeadline 1 := by decide
  have h2 : trace3b.answer 2 = .runDeadline 0 := by decide
  have := S3_handler_numbers_strictly_go_up hops trace3b 1 2 1 0 (by omega) h1 h2
  omega

/-! ## S4. Checked at each answer -/

def bug4 : Ops Nat := kOps { eligible := fun x used => !x.paused && decide (used < x.budget) }

/-- Source 0 is not ready, and is served. -/
def trace4 : PassTrace bug4 0 :=
  knobTrace _ (loopOf [idleSrc 1] []) 0 (fun _ s => s) (fun _ s => Env_refl s)

/-- Caught by `S4_served_source_was_eligible`. -/
theorem bug4_caught : ¬ bug4.Meets := fun hops => by
  have hserve : trace4.answer 0 = .serve 0 := by decide
  obtain ⟨x, hx, hready, -⟩ := S4_served_source_was_eligible hops trace4 0 0 hserve
  have hseen : (trace4.seen 0).sources[0]? = some (idleSrc 1) := by decide
  rw [hseen] at hx
  obtain rfl := Option.some.inj hx
  simp [idleSrc] at hready

/-! ## S5. Paused means never -/

def bug5a : Ops Nat := kOps { eligible := fun x used => x.ready && decide (used < x.budget) }

def pausedReadySrc : Src := ⟨true, true, 1⟩

/-- Source 0 is ready and paused, and is served. -/
def trace5a : PassTrace bug5a 0 :=
  knobTrace _ (loopOf [pausedReadySrc] []) 0 (fun _ s => s) (fun _ s => Env_refl s)

/-- Caught by `S5_paused_source_never_served`. -/
theorem bug5a_caught : ¬ bug5a.Meets := fun hops => by
  have hserve : trace5a.answer 0 = .serve 0 := by decide
  refine S5_paused_source_never_served hops trace5a 0 1 ?_ 0 (by omega) hserve
  intro n hn
  have hn0 : n = 0 := by omega
  subst hn0
  exact ⟨pausedReadySrc, by decide, rfl⟩

def bug5b : Ops Nat := kOps { due := fun x now =>
  match x.deadline with
  | some d => decide (d ≤ now)
  | none => false }

/-- Handler 0 has a deadline and is paused, and runs. -/
def trace5b : PassTrace bug5b 0 :=
  knobTrace _ (loopOf [] [⟨some 0, true⟩]) 0 (fun _ s => s) (fun _ s => Env_refl s)

/-- Caught by `S5_paused_handler_never_runs`. -/
theorem bug5b_caught : ¬ bug5b.Meets := fun hops => by
  have hrun : trace5b.answer 0 = .runDeadline 0 := by decide
  refine S5_paused_handler_never_runs hops trace5b 0 1 ?_ 0 (by omega) hrun
  intro n hn
  have hn0 : n = 0 := by omega
  subst hn0
  exact ⟨⟨some 0, true⟩, rfl, rfl⟩

/-! ## S6. Sources are not skipped -/

def bug6 : Ops Nat := kOps { sourceScanStart := fun position => position + 1 }

/-- Three ready sources: the first answer is "serve 1", and source 0 was skipped. -/
def trace6 : PassTrace bug6 0 :=
  knobTrace _ (loopOf [readySrc 1, readySrc 1, readySrc 1] []) 0 (fun _ s => s)
    (fun _ s => Env_refl s)

/-- Caught by `S6_no_eligible_source_skipped`. -/
theorem bug6_caught : ¬ bug6.Meets := fun hops => by
  have hserve : trace6.answer 0 = .serve 1 := by decide
  have hskip := S6_no_eligible_source_skipped hops trace6 0 1 hserve 0 (by simp [curPos])
    (by omega)
  exact hskip ⟨readySrc 1, by decide, rfl, rfl, by decide⟩

/-! ## S7. Serving clears ready -/

def kBug7 : Knobs := { afterServe := fun _ sources => sources }
def bug7 : Ops Nat := kOps kBug7

def state7 : Loop := ⟨[readySrc 2], [], .serving 0 0 (fun _ => 0)⟩

/-- Caught by `S7_serving_clears_ready_and_nothing_else`: source 0 is served and is still ready. -/
theorem bug7_caught : ¬ bug7.Meets := fun hops => by
  have hrun := kOps_call kBug7 state7 (.serve 0) (by decide)
  have hrule := (S7_serving_clears_ready_and_nothing_else hops state7 _ 0 hrun (readySrc 2)
    (by decide)).1
  have hafter : (kNextTurn kBug7 state7).2.sources[0]? = some (readySrc 2) := by decide
  rw [hafter] at hrule
  simp [readySrc] at hrule

/-! ## S8. Deadlines are not skipped -/

def bug8 : Ops Nat := kOps { handlerScanStart := 1 }

/-- Handler 0 is due and nothing else is: "pass done" at call 0. -/
def trace8 : PassTrace bug8 0 :=
  knobTrace _ (loopOf [] [⟨some 0, false⟩]) 0 (fun _ s => s) (fun _ s => Env_refl s)

/-- Caught by `S8_first_pass_done_leaves_no_due_handler`. -/
theorem bug8_caught : ¬ bug8.Meets := fun hops => by
  have hdone : trace8.answer 0 = .passDone := by decide
  have hnone := S8_first_pass_done_leaves_no_due_handler hops trace8 0 hdone
    (fun j hj => absurd hj (Nat.not_lt_zero j)) 0 (by simp [nextHandler])
  exact hnone ⟨⟨some 0, false⟩, 0, rfl, rfl, rfl, by decide⟩

/-! ## S9. Running a deadline clears it -/

def kBug9a : Knobs := { afterRun := fun _ handlers => handlers }
def bug9a : Ops Nat := kOps kBug9a

def state9a : Loop := ⟨[], [⟨some 0, false⟩], .serving 0 0 (fun _ => 0)⟩

/-- Caught by `S9_running_clears_deadline_and_nothing_else`: handler 0 runs and keeps its
deadline. -/
theorem bug9a_caught : ¬ bug9a.Meets := fun hops => by
  have hrun := kOps_call kBug9a state9a (.runDeadline 0) (by decide)
  have hrule := (S9_running_clears_deadline_and_nothing_else hops state9a _ 0 hrun
    ⟨some 0, false⟩ rfl).1
  have hafter : (kNextTurn kBug9a state9a).2.handlers[0]? = some ⟨some 0, false⟩ := rfl
  rw [hafter] at hrule
  simp at hrule

def bug9b : Ops Nat := kOps { nextHandlerAfterRun := fun h => h }

/-- Handler 0 runs; the scan stays at 0, where the application has set a deadline again:
handler 0 runs twice in one pass. -/
def trace9b : PassTrace bug9b 0 :=
  knobTrace _ (loopOf [] [⟨some 0, false⟩]) 0
    (envAt 1 (armDeadline 0)) (envAt_env 1 _ (armDeadline_env 0))

/-- Caught by `S9_handler_runs_at_most_once_per_pass`. -/
theorem bug9b_caught : ¬ bug9b.Meets := fun hops => by
  have h0 : trace9b.answer 0 = .runDeadline 0 := by decide
  have h1 : trace9b.answer 1 = .runDeadline 0 := by decide
  exact S9_handler_runs_at_most_once_per_pass hops trace9b 0 0 1 (by omega) h0 h1

/-! ## S10. Time is fixed per pass -/

def bug10 : Ops Nat := kOps { due := fun x now =>
  match x.deadline with
  | some d => !x.paused && decide (d ≤ now + 1)
  | none => false }

/-- Handler 0 has deadline 1, the pass began at 0, and the handler runs. -/
def trace10 : PassTrace bug10 0 :=
  knobTrace _ (loopOf [] [⟨some 1, false⟩]) 0 (fun _ s => s) (fun _ s => Env_refl s)

/-- Caught by `S10_run_deadline_not_later_than_now`. -/
theorem bug10_caught : ¬ bug10.Meets := fun hops => by
  have hrun : trace10.answer 0 = .runDeadline 0 := by decide
  have hlate := S10_run_deadline_not_later_than_now hops trace10 0 0 hrun ⟨some 1, false⟩ 1
    rfl rfl
  exact absurd hlate (by decide)

/-! ## S11. A pass always ends -/

def bug11 : Ops Nat := kOps { eligible := fun x _ => x.ready && !x.paused && decide (0 < x.budget) }

/-- Budget 1, made ready again after every answer: serve 0 at every call. -/
def trace11 : PassTrace bug11 0 :=
  knobTrace _ (loopOf [readySrc 1] []) 0 (fun _ s => armReady 0 s) (fun _ s => armReady_env 0 s)

/-- Caught by `S11_pass_ends_within_bound`: the bound is 1, and calls 0 and 1 both serve. -/
theorem bug11_caught : ¬ bug11.Meets := fun hops => by
  obtain ⟨m, hm, hdone⟩ := S11_pass_ends_within_bound hops trace11
  have hm1 : m ≤ 1 := hm.trans (by decide)
  have hcases : m = 0 ∨ m = 1 := by omega
  have h0 : trace11.answer 0 = .serve 0 := by decide
  have h1 : trace11.answer 1 = .serve 0 := by decide
  rcases hcases with rfl | rfl
  · rw [h0] at hdone
    cases hdone
  · rw [h1] at hdone
    cases hdone

/-! ## S12. Next deadline -/

def bug12 : Ops Nat := { (refOps : Ops Nat) with
  nextDeadline := fun s =>
    .ok (s.handlers.foldr (fun x acc => combineDeadlines x.deadline acc) none) }

def state12 : Loop := loopOf [] [⟨some 7, false⟩, ⟨some 3, false⟩, ⟨some 1, true⟩]

/-- Caught by `S12_some_is_the_earliest_armed_deadline`: the paused handler at 1 is counted. -/
theorem bug12_caught : ¬ bug12.Meets := fun hops => by
  have hnext : bug12.nextDeadline state12 = .ok (some 1) := by decide
  obtain ⟨⟨x, hx, hnotpaused, hdeadline⟩, -⟩ :=
    S12_some_is_the_earliest_armed_deadline hops state12 1 hnext
  simp [state12, loopOf] at hx
  rcases hx with rfl | rfl | rfl <;> simp at hnotpaused hdeadline

/-! ## S13. More work -/

def bug13 : Ops Nat := { (refOps : Ops Nat) with
  hasReadySource := fun s => .ok (s.sources.any fun x => x.ready && !x.paused) }

/-- Caught by `S13_has_ready_source_exactly_when`: a ready source with budget 0 counts. -/
theorem bug13_caught : ¬ bug13.Meets := fun hops => by
  have hyes : bug13.hasReadySource (loopOf [readySrc 0] []) = .ok true := by decide
  obtain ⟨x, hx, -, -, hbudget⟩ :=
    (S13_has_ready_source_exactly_when hops _ true hyes).1 rfl
  simp [loopOf, readySrc] at hx
  subst hx
  simp at hbudget

/-! ## S14. Nothing else changes -/

def clearReady (j : Nat) (sources : List Src) : List Src :=
  modifyAt sources j fun x => { x with ready := false }

/-- Serving `j` also clears the ready flag of source `j + 1`. -/
def kBug14 : Knobs := { afterServe := fun j sources => clearReady (j + 1) (clearReady j sources) }
def bug14 : Ops Nat := kOps kBug14

def state14 : Loop := ⟨[readySrc 1, readySrc 1], [], .serving 0 0 (fun _ => 0)⟩

/-- Caught by `S14_next_turn_keeps_other_sources`: serving 0 clears the ready flag of source 1. -/
theorem bug14_caught : ¬ bug14.Meets := fun hops => by
  have hrun := kOps_call kBug14 state14 (.serve 0) (by decide)
  have hsame := S14_next_turn_keeps_other_sources hops state14 _ _ hrun 1 (by simp)
  have hafter : (kNextTurn kBug14 state14).2.sources[1]? = some (idleSrc 1) := by decide
  have hbefore : state14.sources[1]? = some (readySrc 1) := by decide
  rw [hafter, hbefore] at hsame
  simp [idleSrc, readySrc] at hsame

/-! ## S15. Never fails -/

def bug15 : Ops Nat := { (refOps : Ops Nat) with
  setReady := fun s i b =>
    .ok { s with sources := modifyAt s.sources i fun x => { x with ready := b } } }

/-- Caught by `S15_nonexistent_number_panics`: `set_ready` of source 0 of a loop with no source. -/
theorem bug15_caught : ¬ bug15.Meets := fun hops => by
  have hpanic := (S15_nonexistent_number_panics hops).1 (loopOf [] []) 0 true (by simp [loopOf])
  have hbug : bug15.setReady (loopOf [] []) 0 true = .ok (loopOf [] []) := rfl
  rw [hbug] at hpanic
  cases hpanic

/-! ## S16. Starting again -/

def bug16 : Ops Nat := { (refOps : Ops Nat) with
  beginPass := fun s now =>
    match s.phase with
    | .serving _ _ used => .ok { s with phase := .serving now 0 used }
    | _ => .ok { s with phase := .serving now 0 (fun _ => 0) } }

/-- A pass in which source 0 has used 1 unit of its budget. -/
def state16 : Loop := ⟨[readySrc 1], [], .serving 0 0 (fun _ => 1)⟩

/-- Caught by `S16_begin_pass_restarts`: after `begin_pass 1` the used budget is still 1. -/
theorem bug16_caught : ¬ bug16.Meets := fun hops => by
  have hbegin : bug16.beginPass state16 1 =
      .ok { state16 with phase := .serving 1 0 (fun _ => 1) } := rfl
  have hrestart := (S16_begin_pass_restarts hops state16 _ hbegin).1
  change Phase.serving 1 0 (fun _ => 1) = Phase.serving 1 0 (fun _ => 0) at hrestart
  injection hrestart with _ _ hused
  have := congrFun hused 0
  simp at this

/-! ## S17. Setters change only their target -/

def bug17 : Ops Nat := { (refOps : Ops Nat) with
  setReady := fun s i b =>
    if i < s.sources.length then
      .ok { s with
        sources := modifyAt (modifyAt s.sources i fun x => { x with ready := b }) (i + 1)
          fun x => { x with paused := false } }
    else .panic }

def state17 : Loop := loopOf [idleSrc 1, ⟨false, true, 1⟩] []

/-- Caught by `S17_set_ready_changes_only_its_target`: `set_ready(0, true)` also clears the pause
of source 1. -/
theorem bug17_caught : ¬ bug17.Meets := fun hops => by
  have hset : bug17.setReady state17 0 true =
      .ok (loopOf [readySrc 1, idleSrc 1] []) := rfl
  have hother := (S17_set_ready_changes_only_its_target hops state17 _ 0 true hset (idleSrc 1)
    (by decide)).2.1 1 (by omega)
  have hafter : (loopOf [readySrc 1, idleSrc 1] []).sources[1]? = some (idleSrc 1) := by decide
  have hbefore : state17.sources[1]? = some ⟨false, true, 1⟩ := by decide
  rw [hafter, hbefore] at hother
  simp [idleSrc] at hother

end EmEventLoop
