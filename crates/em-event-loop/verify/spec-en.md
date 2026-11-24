# `em-event-loop`: English specification

**Status: approved by the user, 2026-11-23 (rev 4, after plan review and G1; note 0017 D2, D6 step 1). Frozen by hash: a change
needs the user's approval.**

`em-event-loop` is a general-purpose scheduler in the style of libae. It knows nothing about EasyMesh, sockets or tokio.
It never does any work itself: it only **answers** "what should be done next?". Limits and drop rules belong to the
application (note 0016 U13, U15, U28, U35).

## Words used

- **Source:** something that can have work (a socket, a queue, a channel). Sources are numbered 0, 1, 2, … in a fixed
  order chosen when the loop is created.
- **Budget:** the number *n* of times a source may be served in one pass ("at most *n*"). Every source has one.
- **Ready:** the source has work. The application sets this flag. Answering "serve" clears it.
- **Paused:** the source must not be served for now. Only the application changes this flag.
- **Eligible:** ready, not paused, and with budget left in this pass.
- **Handler:** something that can have one timer. Handlers are numbered 0, 1, 2, …
- **Deadline:** the time a handler's timer fires. A handler has one deadline or none. Answering "run deadline" clears it.
  A handler's timer can also be **paused**, which keeps the deadline but stops it firing.
- **Due:** the handler has a deadline, is not paused, and the deadline is not later than the pass's time.
- **Pass:** starts with `begin_pass(now)`. The time *now* stays fixed for the whole pass. The application then asks
  `next_turn()` repeatedly. Each answer is "serve source *i*", "run deadline of handler *h*", or "pass done".
- **Current position:** the last source served in this pass (source 0 at the start).
- **Pass progress:** the current position, the budget used so far, and whether the pass is serving, running deadlines or
  done.
- **Between answers** the application may change any ready flag, pause flag or deadline. It can never change the number
  of sources, their budgets, or the number of handlers.

## Rules

**S1. Start.** A new loop has every source not ready and not paused, and every handler with no deadline and not paused.
- Example: new loop with 2 sources, 1 handler; `begin_pass(0)` → pass done.
- Before the first `begin_pass`, `next_turn()` answers "pass done" and changes nothing, even if a source is ready.
- Counter-example: a new loop answers "serve 0".

**S2. Budget.** In one pass, a source with budget "at most *n*" is served at most *n* times, however often it is made
ready again.
- Example: one source, at most 2; the application makes it ready after every answer → serve 0, serve 0, pass done.
- Example: budget "at most 0", ready → pass done.
- Counter-example: serve 0 three times in that pass.

**S3. Order and shape.** A pass is: zero or more "serve" answers, then zero or more "run deadline" answers, then "pass
done". Source numbers never go down; handler numbers strictly go up. After "pass done", every further answer is "pass
done" until the next `begin_pass`.
- Example: sources 0 and 1 ready (at most 1 each), handler 0 due → serve 0, serve 1, run deadline 0, pass done, pass
  done.
- Counter-examples: serve 1 then serve 0; run deadline 0 then serve 1; run deadline 2 then run deadline 1.

**S4. Checked at each answer.** A source is served only if it is eligible at that moment. A deadline runs only if it is
due at that moment. A change made between two answers counts from the very next answer.
- Example: sources 0 and 1 ready; after "serve 0" the application pauses source 1 → pass done.
- Counter-example: the application pauses source 1, and the next answer is "serve 1".

**S5. Paused means never.** A source paused at every answer of a pass is never served in it. A handler paused at every
answer never runs in it. Pausing keeps the ready flag and the deadline.
- Example: source 0 ready and paused → pass done. After resuming, the next pass → serve 0, pass done.
- Example: handler 0 deadline 3, paused, `begin_pass(5)` → pass done. After resuming, the next pass → run deadline 0.
- Counter-example: "serve 0" while source 0 stays paused.

**S6. Sources are not skipped.** When the loop serves source *j*, no source from the current position up to (not
including) *j* is eligible. At its first answer that is not "serve", no source from the current position onward is
eligible. A source behind the current position that becomes ready waits for the next pass.
- Example: sources 0, 1, 2 ready → serve 0, serve 1, serve 2, pass done.
- Example: sources 0 and 1 ready; after "serve 1" the application makes source 0 ready → pass done; the next pass →
  serve 0, pass done.
- Counter-example: sources 0, 1, 2 ready → serve 0, serve 2 (skipped 1).

**S7. Serving clears ready.** Answering "serve source *i*" clears the ready flag of source *i* and nothing else. The
source is served again only if the application makes it ready again.
- Example: one source, at most 3, ready, never made ready again → serve 0, pass done.
- Example: same, made ready after each answer → serve 0, serve 0, serve 0, pass done.
- Counter-example: serve 0 twice when the application did not make it ready again.

**S8. Deadlines are not skipped.** In the deadline part of a pass, the loop looks at handlers in order and never jumps over
one that is due and not yet passed. At the first "pass done", no handler after the last one run (no handler at all, if none
has run) is due. In a finished pass, a handler that stays due until its turn runs exactly once.
- Example: handlers 0 and 1 due → run deadline 0, run deadline 1, pass done.
- Counter-examples: no deadline has run yet and handler 0 is due, and the answer is "run deadline 1"; "run deadline 0"
  twice in one pass.

**S9. Running a deadline clears it.** Answering "run deadline of handler *h*" clears the deadline of *h* and nothing
else. A deadline set again for a handler already passed waits for the next pass, even if it is already due.
- Example: handler 0 deadline 5, `begin_pass(5)` → run deadline 0, pass done; `next_deadline()` is then "none".
- Example: after "run deadline 0" the application sets its deadline to 5 again → pass done; the next `begin_pass(5)` →
  run deadline 0.
- Counter-example: after "run deadline 0", `next_deadline()` still gives the old deadline.

**S10. Time is fixed per pass.** A deadline is due only if it is not later than the *now* given to `begin_pass`.
- Example: handler 0 deadline 6, `begin_pass(5)` → pass done. `begin_pass(6)` → run deadline 0.
- Counter-example: a deadline of 6 runs in a pass that began at 5.

**S11. A pass always ends.** Whatever the application does, at most (sum of all budgets) + (number of handlers) answers come before "pass done". So sources that stay busy can
never hold back timers.
- Example: budgets [at most 2, at most 1], 1 handler, everything made ready and due again after each answer → serve 0,
  serve 0, serve 1, run deadline 0, pass done.
- Counter-example: 5 answers without "pass done" in that setup.

**S12. Next deadline.** `next_deadline()` gives the earliest deadline among handlers that have one and are not paused, or
"none" if there are none. It changes nothing.
- Example: handler 0 at 7, handler 1 at 3, handler 2 at 1 but paused → 3.
- Counter-examples: 1 (a paused handler counted); 7 (not the earliest); "none" while handler 1 has a deadline.

**S13. More work.** `has_ready_source()` answers yes exactly when some source is ready, not paused, and has a budget
above 0 (budget used in the current pass does not matter). It changes nothing. The driver uses it to decide whether to run another pass at once.
- Example: source 0 ready → yes. Source 0 ready and paused → no. No source ready → no. Source 0 ready with budget 0 → no.
- Counter-example: yes when the only ready source is paused.

**S14. Nothing else changes.** Asking for an answer changes only the pass progress, the ready flag of the
source served (S7) and the deadline of the handler run (S9). Pause flags, budgets, other sources and other handlers stay
as they were.
- Counter-example: answering "serve 0" clears the ready flag of source 1.

**S15. Never fails.** For source and handler numbers that exist, every operation returns normally: no panic, no overflow,
however many passes are run, provided the memory for the loop could be allocated when it was created. Using a number that
does not exist is a programming bug and panics.

**S16. Starting again.** `begin_pass` during a pass abandons that pass and starts a new one: the position returns to
source 0, every source has its full budget again, and the time is the new *now*. Ready flags, pause flags and deadlines
are kept.
- Example: one source, at most 1, ready; `begin_pass(0)` → serve 0; the application makes it ready; `begin_pass(1)` →
  serve 0, pass done.
- Counter-example: after the second `begin_pass`, the source gets no turn because the old pass used its budget.

**S17. Setters change only their target.** `set_ready`, `set_paused`, `set_deadline` and `set_deadline_paused` each change
exactly the one flag or deadline they name. They do not change any other source or handler, the budgets, or the pass
progress. `set_deadline` takes a time or "none"; "none" clears the handler's deadline.
- Example: sources 0 and 1 paused; `set_ready(1, true)` → source 0 is still paused.
- Counter-example: `set_ready(1, true)` clears the pause of source 0, or gives source 1 fresh budget in this pass.

## Assumptions the proofs may use

- Comparing two times always gives an answer, and the comparison is a total order (consistent, like comparing numbers).
- The application uses only source and handler numbers that exist (except for the panic sentence of S15, which is proved
  for numbers that do not exist).
- The memory for the loop can be allocated when it is created.

## Not covered by this spec

- That the application does the work it is told to do, or routes its actions in order (driver and application tests).
- The driver, tokio and all I/O.
- Queue sizes, drop rules, timer values and the EasyMesh source table (application policy, note 0016 D3b).
- Fairness across several passes.
