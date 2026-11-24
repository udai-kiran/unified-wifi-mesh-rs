#!/usr/bin/env bash
# One planted bug, end to end. Run INSIDE the container:
#   verify/run.sh bash /crate/verify/mutants/run-mutant.sh RUN M02
# RUN is a run folder under /cache/em-event-loop-runs/ whose `proofs` folder is a built copy of
# verify/lean (run-all.sh makes it once before it starts the mutants; a mutant never rebuilds it).
# The mutant works in RUN/mutants/ID, which must not exist yet; the crate is not touched.
# Writes RUN/mutants/ID/summary.txt with one `key=value` per step; verdict.py decides from it and
# from the two audit files (refinement.verdict, counter.verdict). The exit code of this script is
# not the verdict.
#   apply_rc, extract_rc    the bug is planted; charon and aeneas extract the mutant
#   compile_rc              `cargo test --no-run` on the mutant (does it compile?)
#   rust_*                  the crate's Rust tests on the mutant (counts, failing test names)
#   types_rc funs_rc abs_rc the extracted files and the UNCHANGED Abstraction.lean elaborate
#   refinement_rc           Lean on the unchanged Refinement.lean against the mutant (1 = errors)
#   run_rc, counter_gen_rc, counter_rc   the counter-example: runner, generator, Lean
# Test hooks (for check-negatives.sh): MUTANT_TEST_HOOK is a script run with the mutant folder
# after the Lean files are written; PROOFS overrides RUN/proofs.
set -uo pipefail
RUN=${1:-}
ID=${2:-}
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
case "$RUN" in
  /cache/em-event-loop-runs/*/*|/cache/em-event-loop-runs/?*) ;;
  *) echo "bad run folder: '$RUN'" >&2; exit 2 ;;
esac
case "$RUN" in *..*) echo "'..' in the run folder" >&2; exit 2 ;; esac
[ -d "$RUN" ] || { echo "no run folder: $RUN" >&2; exit 2; }
[[ "$ID" =~ ^M[0-9]+[a-z]?$ ]] || { echo "bad mutant id: '$ID'" >&2; exit 2; }
python3 "$HERE/mutate.py" list | cut -f1 | grep -qx "$ID" || { echo "unknown mutant id: $ID" >&2; exit 2; }
LEAN_SRC=/crate/verify/lean
PROOFS=${PROOFS:-$RUN/proofs}
MD=$RUN/mutants/$ID
SUMMARY=$MD/summary.txt
[ ! -e "$MD" ] || { echo "mutant folder exists: $MD" >&2; exit 2; }
mkdir -p "$MD/src" "$MD/lean" "$MD/build"
: > "$SUMMARY"
step() { echo "$1=$2" >> "$SUMMARY"; }

# 1. plant the bug
cp /crate/src/*.rs "$MD/src/"
python3 "$HERE/mutate.py" apply "$ID" "$MD/src/lib.rs"
step apply_rc $?
diff -u /crate/src/lib.rs "$MD/src/lib.rs" > "$MD/mutation.diff"
grep -qx "apply_rc=0" "$SUMMARY" || exit 0

# 2. extraction (the scratch folder is a standalone copy of the crate)
SRC_DIR=$MD/src OUT_DIR=$MD/lean SUBDIR=Mutant SCRATCH=$MD/scratch \
  bash "$HERE/../extract.sh" > "$MD/extract.log" 2>&1
step extract_rc $?
grep -qx "extract_rc=0" "$SUMMARY" && [ -f "$MD/lean/Mutant/Funs.lean" ] || exit 0

# 3. the mutant compiles, and the crate's Rust tests run on it
cd "$MD/scratch" || exit 0
export CARGO_TARGET_DIR=$MD/target
cargo +nightly-2026-09-17 test --offline --no-run > "$MD/cargo-build.log" 2>&1
step compile_rc $?
timeout 600 cargo +nightly-2026-09-17 test --offline > "$MD/cargo.log" 2>&1
step rust_rc $?
result_line=$(grep -h "^test result" "$MD/cargo.log" | head -1)
step rust_passed "$(echo "$result_line" | sed -E 's/.* ([0-9]+) passed; ([0-9]+) failed.*/\1/')"
step rust_failed "$(echo "$result_line" | sed -E 's/.* ([0-9]+) passed; ([0-9]+) failed.*/\2/')"
step rust_failing_tests "$(grep -h '^    [a-z0-9_:]*$' "$MD/cargo.log" | sort -u | tr -d ' ' | tr '\n' ' ')"

# 4. the Lean files: the unchanged Abstraction, Refinement and runner against the mutant, each
# ending with the audit that writes the verdict file
cp "$LEAN_SRC/Abstraction.lean" "$MD/lean/MutAbstraction.lean"
sed -i 's/^import Extracted.Funs/import Mutant.Funs/' "$MD/lean/MutAbstraction.lean"
sed 's/^import Abstraction/import MutAbstraction\nimport AuditAxioms/' "$LEAN_SRC/Refinement.lean" > "$MD/lean/MutRefinement.lean"
echo "#eval EmEventLoopAudit.writeVerdict \"$MD/refinement.verdict\" []" >> "$MD/lean/MutRefinement.lean"
sed 's/^import Abstraction/import MutAbstraction/' "$LEAN_SRC/ExtractedRun.lean" > "$MD/lean/MutRun.lean"
python3 "$HERE/mutate.py" counter "$ID" > "$MD/counter-body.lean"
counter_gen_rc=$?
# an empty generator output is a failure too (code 3)
if [ "$counter_gen_rc" -eq 0 ] && [ -z "$(tr -d '[:space:]' < "$MD/counter-body.lean")" ]; then counter_gen_rc=3; fi
step counter_gen_rc "$counter_gen_rc"
# the counter-example may use native_decide: the audit allows it for this file only
{ echo "import AuditAxioms"; cat "$MD/counter-body.lean"
  echo "#eval EmEventLoopAudit.writeVerdict \"$MD/counter.verdict\" [\`_this]"; } > "$MD/lean/MutCounter.lean"
[ -z "${MUTANT_TEST_HOOK:-}" ] || bash "$MUTANT_TEST_HOOK" "$MD"

# 5. Lean (in the proofs copy, whose environment finds the built project modules)
cd "$PROOFS" || exit 0
lake env bash "$HERE/lean-steps.sh" "$MD"

# 6. only the bulky build folders are removed; the logs and the verdict files stay
rm -rf "$MD/target" "$MD/scratch/target"
exit 0
