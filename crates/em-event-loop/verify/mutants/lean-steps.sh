#!/usr/bin/env bash
# The Lean steps of run-mutant.sh. Run with `lake env` in the proofs copy:
#   lake env bash lean-steps.sh MUTANT_FOLDER
# A step that fails stops the steps that need its output; the exit codes go to summary.txt.
set -u
MD=$1
export LEAN_PATH="$LEAN_PATH:$MD/build"
cd "$MD/lean" || exit 0
mkdir -p "$MD/build/Mutant"
lean_step() {  # name log lean-arguments...
  local name=$1 log=$2
  shift 2
  timeout 900 lean -R . "$@" > "$log" 2>&1
  local rc=$?
  echo "$name=$rc" >> "$MD/summary.txt"
  return "$rc"
}
lean_step types_rc "$MD/lean-types.log" -o "$MD/build/Mutant/Types.olean" Mutant/Types.lean || exit 0
lean_step funs_rc "$MD/lean-funs.log" -o "$MD/build/Mutant/Funs.olean" Mutant/Funs.lean || exit 0
lean_step abs_rc "$MD/lean-abs.log" -o "$MD/build/MutAbstraction.olean" MutAbstraction.lean || exit 0
lean_step refinement_rc "$MD/refinement.log" MutRefinement.lean
lean_step run_rc "$MD/lean-run.log" -o "$MD/build/MutRun.olean" MutRun.lean || exit 0
lean_step counter_rc "$MD/counter.log" MutCounter.lean
exit 0
