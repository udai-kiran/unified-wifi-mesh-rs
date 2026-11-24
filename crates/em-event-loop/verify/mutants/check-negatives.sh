#!/usr/bin/env bash
# R4: each injected failure must make the mutant gate FAIL, for the right reason. Run INSIDE the
# container, after RUN/proofs (a built copy of verify/lean) exists:
#   verify/run.sh bash /crate/verify/mutants/check-negatives.sh RUN
# Every case runs the real scripts (run-mutant.sh, run-all.sh, verdict.py) on mutant M05; the
# failure is injected through the MUTANT_TEST_HOOK of run-mutant.sh (Lean files) or into a copy of
# the mutants folder (Python), never into the real files. Exit 0 only if every case fails the
# gate with the expected reason. The cases run in parallel.
set -uo pipefail
RUN=${1:-}
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
case "$RUN" in /cache/em-event-loop-runs/?*) ;; *) echo "bad run folder: '$RUN'" >&2; exit 2 ;; esac
case "$RUN" in *..*) echo "'..' in the run folder" >&2; exit 2 ;; esac
[ -d "$RUN/proofs" ] || { echo "no built proofs copy: $RUN/proofs" >&2; exit 2; }
NEG=$RUN/negatives
ID=M05
mkdir -p "$NEG"

# A copy of the mutants folder (and extract.sh, which run-mutant.sh finds one level up) to patch.
patched_copy() {
  mkdir -p "$NEG/$1/v"
  cp -r "$HERE" "$NEG/$1/v/mutants"
  cp "$HERE/../extract.sh" "$NEG/$1/v/extract.sh"
}

write_hook() {  # case, one line of shell run with the mutant folder in $1
  printf '%s\n' "$2" > "$NEG/$1/hook.sh"
}

run_case() {  # case, verify folder to use, hook (or empty)
  local case_name=$1 verify_mutants=$2 hook=$3
  mkdir -p "$NEG/$case_name/run"
  MUTANT_TEST_HOOK=$hook PROOFS=$RUN/proofs \
    bash "$verify_mutants/run-mutant.sh" "$NEG/$case_name/run" $ID > "$NEG/$case_name/run.log" 2>&1
  python3 "$verify_mutants/verdict.py" one "$NEG/$case_name/run" $ID > "$NEG/$case_name/verdict.txt" 2>&1
  echo $? > "$NEG/$case_name/verdict.rc"
}

mkdir -p "$NEG/import-error" "$NEG/abstraction" "$NEG/empty-counter" "$NEG/generator-exit1" "$NEG/empty-list"
write_hook import-error 'sed -i "1i import NoSuchModule" "$1/lean/MutRefinement.lean"'
write_hook abstraction 'sed -i "s/s\.used\.val/s.usedX.val/" "$1/lean/MutAbstraction.lean"'
patched_copy empty-counter
sed -i 's/^def counter_file(mutant_id):/&\n    return ""/' "$NEG/empty-counter/v/mutants/mutate.py"
patched_copy generator-exit1
sed -i 's/^def counter_file(mutant_id):/&\n    sys.exit(1)/' "$NEG/generator-exit1/v/mutants/mutate.py"
patched_copy empty-list
sed -i 's/^def counter_file(mutant_id):/MUTANTS.clear()\n\n\n&/' "$NEG/empty-list/v/mutants/mutate.py"

run_case import-error "$HERE" "$NEG/import-error/hook.sh" &
run_case abstraction "$HERE" "$NEG/abstraction/hook.sh" &
run_case empty-counter "$NEG/empty-counter/v/mutants" "" &
run_case generator-exit1 "$NEG/generator-exit1/v/mutants" "" &
wait
# the empty list is stopped before any mutant runs (run-all.sh needs RUN/proofs)
mkdir -p "$NEG/empty-list/run"
ln -sfn "$RUN/proofs" "$NEG/empty-list/run/proofs"
bash "$NEG/empty-list/v/mutants/run-all.sh" "$NEG/empty-list/run" > "$NEG/empty-list/verdict.txt" 2>&1
echo $? > "$NEG/empty-list/verdict.rc"

status=0
check() {  # case, regex
  local rc
  rc=$(cat "$NEG/$1/verdict.rc")
  if [ "$rc" -ne 0 ] && grep -Eq "$2" "$NEG/$1/verdict.txt"; then
    echo "ok   $1: gate failed (exit $rc): $(grep -Eo "$2.{0,120}" "$NEG/$1/verdict.txt" | head -1)"
  else
    echo "FAIL $1: exit $rc; wanted a failure matching $2"; sed 's/^/     /' "$NEG/$1/verdict.txt" | head -5; status=1
  fi
}
check import-error 'no verdict from MutRefinement'
check abstraction 'abs_rc=1 \(the mutant does not elaborate against the unchanged Abstraction'
check empty-counter 'counter_gen_rc=3'
check generator-exit1 'counter_gen_rc=1'
check empty-list 'COVERAGE FAIL: the mutant list is empty'
[ "$status" -eq 0 ] && echo "ALL NEGATIVES FAIL THE GATE" || echo "SOME NEGATIVE DID NOT FAIL THE GATE"
exit $status
