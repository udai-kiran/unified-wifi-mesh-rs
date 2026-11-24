#!/usr/bin/env python3
"""G3 code layer (note 0017): planted bugs in a disposable copy of src/lib.rs.

  mutate.py list                       one line per mutant
  mutate.py apply ID path/to/lib.rs    plant the bug in that copy (never run it on the repo's src/)
  mutate.py counter ID                 print the Lean file with the counter-example script of ID

Each mutant is a list of exact (old, new) text replacements; every `old` must occur exactly once.
The counter-example is a script of calls (Lean `XAct` syntax of ExtractedRun.lean); the check
compares what the application sees after the script, for the mutant's extracted code and for
the reference implementation, which meets the step spec (so its answer is the one the step
relation allows). The check is the named theorem `counter` (verdict.py reads it from the audit).
EXPECTED_OPERATION names the operation whose refinement theorem `<operation>_refines` the bug
must break; verdict.py decides, from the Lean audit, whether a mutant is caught.
"""
import sys

REFILL = "rdy (u 0) true, rdy (u 1) true, deadline (u 0) (some 0)"

# id: (rule, what the bug does, edits, counter-example)
# counter-example: dict(budgets, handlers, script) or dict(custom=<Lean text>)
MUTANTS = {
    "M01": ("S1", "a new loop starts with every source ready",
            [("                used: 0,\n                ready: false,",
              "                used: 0,\n                ready: true,")],
            dict(budgets="[u 1]", handlers="u 0", script="[beginPass 0, next]")),
    "M01b": ("S1", "a new loop starts with every handler's timer paused",
             [("                deadline: None,\n                paused: false,",
               "                deadline: None,\n                paused: true,")],
             dict(budgets="[]", handlers="u 1",
                  script="[deadline (u 0) (some 0), beginPass 0, next]")),
    "M02": ("S2", "a source may be served budget + 1 times (`used <= budget`)",
            [("source.ready && !source.paused && source.used < source.budget",
              "source.ready && !source.paused && source.used <= source.budget")],
            dict(budgets="[u 1]", handlers="u 0",
                 script="[rdy (u 0) true, beginPass 0, next, rdy (u 0) true, next, next]")),
    "M03": ("S3", "after `PassDone` the loop starts looking at sources again",
            [("            self.phase = Phase::Done;\n            Turn::PassDone",
              "            self.phase = Phase::Serving { now, position: 0 };\n            Turn::PassDone")],
            dict(budgets="[u 2]", handlers="u 0",
                 script="[rdy (u 0) true, beginPass 0, next, next, rdy (u 0) true, next]")),
    "M04": ("S4", "the pause flag of a source is read once, at `begin_pass`",
            [("    ready: bool,\n    paused: bool,\n}\n\nstruct HandlerState",
              "    ready: bool,\n    paused: bool,\n    frozen: bool,\n}\n\nstruct HandlerState"),
             ("                ready: false,\n                paused: false,\n            });",
              "                ready: false,\n                paused: false,\n                frozen: false,\n            });"),
             ("            self.sources[index].used = 0;\n",
              "            self.sources[index].used = 0;\n            self.sources[index].frozen = self.sources[index].paused;\n"),
             ("source.ready && !source.paused && source.used < source.budget",
              "source.ready && !source.frozen && source.used < source.budget")],
            dict(budgets="[u 1, u 1]", handlers="u 0",
                 script="[rdy (u 0) true, rdy (u 1) true, beginPass 0, next, pau (u 1) true, next]")),
    "M05": ("S5", "a paused source can be served",
            [("source.ready && !source.paused && source.used < source.budget",
              "source.ready && source.used < source.budget")],
            dict(budgets="[u 1]", handlers="u 0",
                 script="[rdy (u 0) true, pau (u 0) true, beginPass 0, next]")),
    "M05b": ("S5", "a handler with a paused timer runs",
             [("        !handler.paused && deadline_is_due(handler.deadline, now)",
               "        deadline_is_due(handler.deadline, now)")],
             dict(budgets="[]", handlers="u 1",
                  script="[deadline (u 0) (some 3), deadlinePaused (u 0) true, beginPass 5, next]")),
    "M06": ("S6", "the scan for a source skips every other source",
            [("            if self.source_is_eligible(index) {\n                return index;\n            }\n            index += 1;",
              "            if self.source_is_eligible(index) {\n                return index;\n            }\n            index += 2;")],
            dict(budgets="[u 1, u 1, u 1]", handlers="u 0",
                 script="[rdy (u 0) true, rdy (u 1) true, rdy (u 2) true, beginPass 0, next, next, next, next]")),
    "M06b": ("S6", "the scan for a source starts at source 0, not at the position",
             [("let source = self.first_eligible_source(position);",
               "let source = self.first_eligible_source(0);")],
             dict(budgets="[u 2, u 1]", handlers="u 0",
                  script="[rdy (u 0) true, rdy (u 1) true, beginPass 0, next, next, rdy (u 0) true, next]")),
    "M07": ("S7", "serving a source does not clear its ready flag",
            [("                    self.sources[source].ready = false;\n", "")],
            dict(budgets="[u 3]", handlers="u 0",
                 script="[rdy (u 0) true, beginPass 0, next, next]")),
    "M08": ("S8", "the deadline part of a pass starts at handler 1",
            [("self.turn_from_handler(now, 0)", "self.turn_from_handler(now, 1)")],
            dict(budgets="[]", handlers="u 2",
                 script="[deadline (u 0) (some 0), deadline (u 1) (some 0), beginPass 0, next, next, next]")),
    "M09": ("S9", "running a deadline does not clear it",
            [("            self.handlers[handler].deadline = None;\n", "")],
            dict(budgets="[]", handlers="u 1",
                 script="[deadline (u 0) (some 5), beginPass 5, next, next]")),
    "M10": ("S10", "a deadline is due when it is later than `now` (comparison reversed)",
            [("        Some(time) => time <= now,", "        Some(time) => now <= time,")],
            dict(budgets="[]", handlers="u 1",
                 script="[deadline (u 0) (some 6), beginPass 5, next, beginPass 6, next]")),
    "M10b": ("S10", "a deadline equal to `now` is not due (`<` instead of `<=`)",
             [("        Some(time) => time <= now,", "        Some(time) => !(now <= time),")],
             dict(budgets="[]", handlers="u 1",
                  script="[deadline (u 0) (some 5), beginPass 5, next]")),
    "M11": ("S11", "serving a source does not use up budget, so a pass can go on for ever",
            [("                    // Eligible means `used < budget`, so this cannot overflow.\n                    self.sources[source].used += 1;\n", "")],
            dict(budgets="[u 2, u 1]", handlers="u 1",
                 script="[" + REFILL + ", beginPass 0, next, " + REFILL + ", next, " + REFILL
                 + ", next, " + REFILL + ", next, " + REFILL + ", next]")),
    "M12": ("S12", "`next_deadline` counts handlers with a paused timer",
            [("            if !handler.paused {\n                if let Some(deadline)",
              "            if true {\n                if let Some(deadline)")],
            dict(budgets="[]", handlers="u 3",
                 script="[deadline (u 0) (some 7), deadline (u 1) (some 3), deadline (u 2) (some 1), deadlinePaused (u 2) true]")),
    "M12b": ("S12", "`next_deadline` gives the latest deadline, not the earliest",
             [("        Some(best) => deadline <= best,", "        Some(best) => best <= deadline,")],
             dict(budgets="[]", handlers="u 3",
                  script="[deadline (u 0) (some 7), deadline (u 1) (some 3), deadline (u 2) (some 1), deadlinePaused (u 2) true]")),
    "M13": ("S13", "`has_ready_source` counts a paused source",
            [("source.ready && !source.paused && 0 < source.budget", "source.ready && 0 < source.budget")],
            dict(budgets="[u 1]", handlers="u 0",
                 script="[rdy (u 0) true, pau (u 0) true]")),
    "M13b": ("S13", "`has_ready_source` looks at the budget left in this pass",
             [("source.ready && !source.paused && 0 < source.budget",
               "source.ready && !source.paused && source.used < source.budget")],
             dict(budgets="[u 1]", handlers="u 0",
                  script="[rdy (u 0) true, beginPass 0, next, rdy (u 0) true]")),
    "M14": ("S14", "serving source i also clears the ready flag of source i + 1",
            [("                    self.sources[source].ready = false;\n",
              "                    self.sources[source].ready = false;\n                    if source + 1 < self.sources.len() {\n                        self.sources[source + 1].ready = false;\n                    }\n")],
            dict(budgets="[u 1, u 1]", handlers="u 0",
                 script="[rdy (u 0) true, rdy (u 1) true, beginPass 0, next, next]")),
    "M15": ("S15", "`begin_pass` reads one source past the end (panics for every loop with a source)",
            [("        while index < self.sources.len() {\n            self.sources[index].used = 0;",
              "        while index <= self.sources.len() {\n            self.sources[index].used = 0;")],
            dict(budgets="[u 1]", handlers="u 0", script="[beginPass 0]")),
    "M15b": ("S15", "`set_ready` with a source that does not exist does not panic",
             [("        self.sources[source.0].ready = ready;",
               "        if source.0 < self.sources.len() {\n            self.sources[source.0].ready = ready;\n        }")],
             dict(custom="""-- the mutant returns normally for source 5 of a loop with one source
def mutantResult : Result Bool := do
  let c ← newX [u 1] (u 0)
  let _ ← em_event_loop.LoopCore.set_ready natOrd natCopy c (u 5) true
  Result.ok true
#eval mutantResult
-- the step spec says: panic (the reference, which meets it, panics)
def specPanics : Bool :=
  match refOps.setReady (newLoop [1] 0) 5 true with
  | .panic => true
  | .ok _ => false
#eval specPanics
theorem counter : mutantResult.reducesTo true = true := by native_decide
theorem counter_spec : specPanics = true := by decide
""")),
    "M16": ("S16", "`begin_pass` does not give the budget back",
            [("            self.sources[index].used = 0;", "            self.sources[index].used = self.sources[index].used;")],
            dict(budgets="[u 1]", handlers="u 0",
                 script="[rdy (u 0) true, beginPass 0, next, rdy (u 0) true, beginPass 1, next, next]")),
    "M17": ("S17", "`set_ready` also gives the source fresh budget in this pass",
            [("        self.sources[source.0].ready = ready;",
              "        self.sources[source.0].ready = ready;\n        self.sources[source.0].used = 0;")],
            dict(budgets="[u 1]", handlers="u 0",
                 script="[rdy (u 0) true, beginPass 0, next, rdy (u 0) true, next]")),
    "M17b": ("S17", "`set_paused` also clears the ready flag of its source",
             [("        self.sources[source.0].paused = paused;",
               "        self.sources[source.0].paused = paused;\n        self.sources[source.0].ready = false;")],
             dict(budgets="[u 1]", handlers="u 0", script="[rdy (u 0) true, pau (u 0) false]")),
}


# The operation whose `<operation>_refines` theorem the bug must break.
EXPECTED_OPERATION = {
    "M01": "new", "M01b": "new", "M02": "next_turn", "M03": "next_turn", "M04": "next_turn",
    "M05": "next_turn", "M05b": "next_turn", "M06": "next_turn", "M06b": "next_turn",
    "M07": "next_turn", "M08": "next_turn", "M09": "next_turn", "M10": "next_turn",
    "M10b": "next_turn", "M11": "next_turn", "M12": "next_deadline", "M12b": "next_deadline",
    "M13": "has_ready_source", "M13b": "has_ready_source", "M14": "next_turn",
    "M15": "begin_pass", "M15b": "set_ready", "M16": "begin_pass", "M17": "set_ready",
    "M17b": "set_paused",
}

# The named theorems of the counter-example file (default: `counter` only).
COUNTER_THEOREMS = {"M15b": ["counter", "counter_spec"]}


def counter_file(mutant_id):
    rule, what, edits, ce = MUTANTS[mutant_id]
    out = [
        "import MutRun",
        "",
        f"-- Counter-example for {mutant_id} ({rule}): {what}.",
        "open Aeneas Aeneas.Std Result",
        "namespace EmEventLoop",
        "open XAct Answer",
        "",
    ]
    if "custom" in ce:
        out.append(ce["custom"])
    else:
        out += [
            f"def budgets : List Usize := {ce['budgets']}",
            f"def handlers : Usize := {ce['handlers']}",
            f"def script : List XAct := {ce['script']}",
            "-- what the application sees: the extracted mutant, then the reference (the step spec)",
            "#eval (obsX budgets handlers script)",
            "#eval (obsRef budgets handlers script)",
            "-- the mutant does not give the reference's observation",
            "theorem counter : agrees (obsX budgets handlers script) (obsRef budgets handlers script) = false := by",
            "  native_decide",
        ]
    out += ["", "end EmEventLoop", ""]
    return "\n".join(out)


def main():
    cmd = sys.argv[1]
    if cmd == "list":
        for mutant_id, (rule, what, _, _) in MUTANTS.items():
            print(f"{mutant_id}\t{rule}\t{what}")
    elif cmd == "apply":
        mutant_id, path = sys.argv[2], sys.argv[3]
        src = open(path).read()
        for old, new in MUTANTS[mutant_id][2]:
            if src.count(old) != 1:
                sys.exit(f"{mutant_id}: expected exactly one match of {old!r}, found {src.count(old)}")
            src = src.replace(old, new)
        open(path, "w").write(src)
    elif cmd == "counter":
        print(counter_file(sys.argv[2]))
    else:
        sys.exit("unknown command")


if __name__ == "__main__":
    main()
