#!/usr/bin/env python3
"""The one place that decides whether a planted bug is caught (note 0017 step 3b, R4).

  verdict.py coverage          the mutant table is not empty and every rule S1 to S17 has a mutant
  verdict.py one RUN ID        print the verdict of one mutant; exit 1 if it is not caught
  verdict.py table RUN         print one tab-separated line per mutant; exit 1 if any is not caught

RUN is the run folder of run-all.sh. A mutant is caught only if every step of run-mutant.sh
succeeded, Lean's refinement check exited 1 (an error, not a timeout), and the audit that
`AuditAxioms.lean` wrote at the end of the file shows that the expected `*_refines` theorem depends
on `sorryAx` (it no longer checks). The counter-example is a named theorem without `sorryAx`.
run-all.sh and report.py both call decide().
"""
import os
import sys
from dataclasses import dataclass, field

import mutate

OPERATIONS = ["set_ready", "set_paused", "set_deadline", "set_deadline_paused", "begin_pass", "new",
              "has_ready_source", "next_deadline", "next_turn"]
REFINES = ["EmEventLoop." + operation + "_refines" for operation in OPERATIONS]
RULES = ["S" + str(n) for n in range(1, 18)]
# run-mutant.sh steps that must exit 0, in order
STEPS = ["apply_rc", "extract_rc", "compile_rc", "types_rc", "funs_rc", "abs_rc", "run_rc",
         "counter_gen_rc", "counter_rc"]


@dataclass
class Verdict:
    mutant_id: str
    reasons: list = field(default_factory=list)
    failing_refines: list = field(default_factory=list)
    rust_passed: str = "?"
    rust_failed: str = "?"
    rust_failing_tests: str = ""
    rust_rc: str = "?"

    @property
    def caught(self):
        return not self.reasons


def coverage_problems():
    problems = []
    if not mutate.MUTANTS:
        problems.append("the mutant list is empty")
    covered = {rule for rule, _, _, _ in mutate.MUTANTS.values()}
    for rule in RULES:
        if rule not in covered:
            problems.append(f"rule {rule} has no mutant")
    if set(mutate.EXPECTED_OPERATION) != set(mutate.MUTANTS):
        problems.append("EXPECTED_OPERATION and MUTANTS have different ids")
    for mutant_id, operation in mutate.EXPECTED_OPERATION.items():
        if operation not in OPERATIONS:
            problems.append(f"{mutant_id}: unknown operation {operation}")
    return problems


def read_summary(folder):
    result = {}
    path = os.path.join(folder, "summary.txt")
    if not os.path.exists(path):
        return result
    for line in open(path).read().split("\n"):
        if "=" in line:
            key, value = line.split("=", 1)
            result[key] = value
    return result


def read_marker(path):
    """The audit written by `EmEventLoopAudit.writeVerdict`: (theorems, violations), or None if
    the file is missing or incomplete (the Lean file did not compile to its end)."""
    if not os.path.exists(path):
        return None
    lines = open(path).read().split("\n")
    if "done" not in lines:
        return None
    theorems, violations = {}, []
    for line in lines:
        parts = line.split(" ", 3)
        if parts[0] == "thm":
            theorems[parts[1]] = set(parts[2].split(","))
        elif parts[0] == "viol":
            violations.append(line)
    return theorems, violations


def decide(run, mutant_id):
    folder = os.path.join(run, "mutants", mutant_id)
    verdict = Verdict(mutant_id)
    summary = read_summary(folder)
    if not summary:
        verdict.reasons.append("no summary: run-mutant.sh did not run")
        return verdict
    verdict.rust_passed = summary.get("rust_passed", "?")
    verdict.rust_failed = summary.get("rust_failed", "?")
    verdict.rust_failing_tests = summary.get("rust_failing_tests", "")
    verdict.rust_rc = summary.get("rust_rc", "?")
    for step in STEPS:
        value = summary.get(step)
        if value is None:
            verdict.reasons.append(f"{step}: step not reached")
            break
        if value != "0":
            meaning = " (the mutant does not elaborate against the unchanged Abstraction: invalid mutant)" \
                if step == "abs_rc" else ""
            verdict.reasons.append(f"{step}={value}{meaning}")
            break
    refinement_rc = summary.get("refinement_rc")
    if refinement_rc is None:
        verdict.reasons.append("refinement_rc: step not reached")
    elif refinement_rc == "0":
        verdict.reasons.append("the refinement still holds (Lean exit 0)")
    elif refinement_rc != "1":
        verdict.reasons.append(f"the refinement check did not finish (Lean exit {refinement_rc})")

    refinement = read_marker(os.path.join(folder, "refinement.verdict"))
    if refinement is None:
        verdict.reasons.append("no verdict from MutRefinement (it did not compile to its end, or an import failed)")
    else:
        theorems, violations = refinement
        missing = [name for name in REFINES if name not in theorems]
        if missing:
            verdict.reasons.append("missing refinement theorems: " + " ".join(missing))
        verdict.failing_refines = sorted(
            name.removeprefix("EmEventLoop.") for name in REFINES if "sorryAx" in theorems.get(name, set()))
        if not verdict.failing_refines:
            verdict.reasons.append("no *_refines theorem depends on sorryAx")
        expected = mutate.EXPECTED_OPERATION[mutant_id] + "_refines"
        if expected not in verdict.failing_refines:
            verdict.reasons.append(f"the expected theorem {expected} still checks")
        for line in violations:
            if line.split(" ")[1] != "sorryAx":
                verdict.reasons.append("audit: " + line)

    counter = read_marker(os.path.join(folder, "counter.verdict"))
    if counter is None:
        verdict.reasons.append("no verdict from the counter-example (it did not compile to its end)")
    else:
        theorems, violations = counter
        for name in mutate.COUNTER_THEOREMS.get(mutant_id, ["counter"]):
            axioms = theorems.get("EmEventLoop." + name)
            if axioms is None:
                verdict.reasons.append(f"counter-example theorem {name} does not exist")
            elif "sorryAx" in axioms:
                verdict.reasons.append(f"counter-example theorem {name} depends on sorryAx")
        for line in violations:
            verdict.reasons.append("counter audit: " + line)
    return verdict


def table_line(mutant_id, verdict):
    rule, what, _, _ = mutate.MUTANTS[mutant_id]
    rust = f"{verdict.rust_failed} of {verdict.rust_passed}+{verdict.rust_failed}"
    return "\t".join([mutant_id, rule, mutate.EXPECTED_OPERATION[mutant_id],
                      "CAUGHT" if verdict.caught else "NOT-CAUGHT",
                      ",".join(verdict.failing_refines), "rust-fail " + rust,
                      "; ".join(verdict.reasons)])


def main():
    command = sys.argv[1]
    if command == "coverage":
        problems = coverage_problems()
        for problem in problems:
            print("COVERAGE FAIL:", problem)
        if not problems:
            print(f"coverage ok: {len(mutate.MUTANTS)} mutants, rules {RULES[0]} to {RULES[-1]}")
        sys.exit(1 if problems else 0)
    run = sys.argv[2]
    if command == "one":
        verdict = decide(run, sys.argv[3])
        print(table_line(sys.argv[3], verdict))
        sys.exit(0 if verdict.caught else 1)
    if command == "table":
        all_caught = True
        for mutant_id in mutate.MUTANTS:
            verdict = decide(run, mutant_id)
            print(table_line(mutant_id, verdict))
            all_caught = all_caught and verdict.caught
        sys.exit(0 if all_caught else 1)
    sys.exit("unknown command")


if __name__ == "__main__":
    main()
