#!/usr/bin/env bash
# The container gates of note 0017 D6 step 3, in order. Host script (needs Docker, the image
# aeneas-spike:latest and its two volumes). Exit 0 only if every gate passes. The cargo checks
# (tests on stable and 1.85.0, clippy, fmt, rustdoc) are not run here.
# Every gate is one command whose exit code is the verdict; the logs are only for reading. All
# scratch output goes to one `mktemp -d` run folder in the volume,
# /cache/em-event-loop-runs/run.XXXXXX. At the end its bulky target folders are deleted; the logs,
# summaries and verdict tables stay.
#   SKIP_G3=1       skip the planted bugs (about 4 minutes with 4 at a time)
#   MUTANT_JOBS=n   mutants at the same time (default 4)
#   verify/reproduce.sh
set -uo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUN=$("$HERE/run.sh" bash -c 'mkdir -p /cache/em-event-loop-runs && mktemp -d /cache/em-event-loop-runs/run.XXXXXX') \
  || { echo "cannot make the run folder"; exit 1; }
LOGS=$RUN/logs
"$HERE/run.sh" mkdir "$LOGS" || exit 1
START=$(date +%s)
status=0
gate() { echo; echo "=== $1"; }
fail() { echo "FAILED: $1"; status=1; }
# logged NAME COMMAND...: run in the container, output to $LOGS/NAME.log, exit code of the command
logged() {
  local name=$1
  shift
  "$HERE/run.sh" bash -c 'log=$1; shift; "$@" > "$log" 2>&1' bash "$LOGS/$name.log" "$@"
}
# the lines of a log that matter, for reading (never decides)
excerpt() { "$HERE/run.sh" grep -E "$2" "$LOGS/$1.log" || true; }
echo "run folder: $RUN"

gate "G5: spec hash and statement digest"
[ "$(sha256sum "$HERE/spec-en.md" | cut -d' ' -f1)" = 64085e5bd35d155321b9df7c27d852022ec889170bb41db0c02d3cfe7cc8acc9 ] \
  || fail "spec-en.md hash"
digest=$(python3 "$HERE/lean/check-statements.py")
echo "$digest"
[ "$digest" = "41 856189881b75665f04102a46eedbe675ee7db6456c412785797d330f43106358" ] || fail "statement digest"

gate "R2: every Lean file is a lakefile root and is audited; no skipKernelTC or implemented_by"
python3 "$HERE/check-modules.py" || fail "check-modules"

gate "AC5: extract twice into the run folder; both equal each other and the committed files"
for n in 1 2; do
  logged "extract-$n" env OUT_DIR="$RUN/extract/$n" SCRATCH="$RUN/scratch-$n" bash /crate/verify/extract.sh \
    || fail "extraction $n"
done
for f in Types Funs; do
  "$HERE/run.sh" cmp "$RUN/extract/1/Extracted/$f.lean" "$RUN/extract/2/Extracted/$f.lean" \
    || fail "$f.lean: extraction 1 and 2 differ"
  "$HERE/run.sh" cmp "$RUN/extract/1/Extracted/$f.lean" "/crate/verify/lean/Extracted/$f.lean" \
    || fail "$f.lean differs from the committed file"
done
echo "compared Types.lean and Funs.lean"

gate "AC4/AC5: the crate's Rust tests on nightly-2026-09-17, from the standalone copy"
logged rust-tests env CARGO_TARGET_DIR="$RUN/target-main" bash /crate/verify/rust-tests.sh "$RUN/scratch-1" \
  || fail "rust tests"
excerpt rust-tests '^test result|FAILED|^error'

gate "AC9/R1/R2: lake build --wfail from an empty build directory (compiles every file, runs the axiom audit)"
export PROOFS_DIR=$RUN/proofs BUILD_DIR=$RUN/build
"$HERE/proofs.sh" bash -c 'lake build --wfail > "$0" 2>&1' "$LOGS/lake-build.log" || fail "lake build --wfail"
excerpt lake-build 'AUDIT|^error|^warning|Build completed'
# The build passing is not enough: both audit drivers must have run to their end and reported ok
# (a driver commented out, or dropped from the lakefile, prints nothing).
for want in "AUDIT ok: 13 modules," "AUDIT ok: 1 modules,"; do
  "$HERE/run.sh" grep -qF "$want" "$LOGS/lake-build.log" || fail "the audit did not report '$want'"
done

gate "R7/G5: the digest of the refinement contract (frozen 2026-11-24, user accepted the back-translation)"
digest2=$(STATEMENTS_FILE=$RUN/refinement-statements.txt "$HERE/refinement-digest.sh") \
  || fail "refinement digest"
echo "statements: $RUN/refinement-statements.txt"
echo "$digest2" | tail -n 1
[ "$(echo "$digest2" | tail -n 1)" = "refinement 29 478acf91f57efc22b0a631e1add9447fc5ca938f7a060a6a2046514b5f59c0eb" ] \
  || fail "refinement digest differs from the frozen value"
unset PROOFS_DIR BUILD_DIR

gate "R3: negative fixtures (sorry, admit, axiom, native_decide outside, lost theorem, stray file)"
"$HERE/run.sh" bash /crate/verify/check-fixtures.sh "$RUN" || fail "fixtures"

if [ -n "${SKIP_G3:-}" ]; then
  gate "R4, AC8/R6: SKIPPED (SKIP_G3 is set): the planted bugs were not run"
else
  gate "R4: injected failures make a mutant fail"
  "$HERE/run.sh" bash /crate/verify/mutants/check-negatives.sh "$RUN" || fail "negatives"
  gate "AC8/R6: G3 code layer (planted bugs in the Rust)"
  "$HERE/run.sh" env MUTANT_JOBS="${MUTANT_JOBS:-4}" bash /crate/verify/mutants/run-all.sh "$RUN" || fail "mutants"
  "$HERE/run.sh" python3 /crate/verify/mutants/report.py "$RUN" || fail "report"
fi

# only bulky build folders of this run are removed
"$HERE/run.sh" bash -c 'for d in "$1"/target-main "$1"/scratch-*/target "$1"/mutants/*/target; do [ -d "$d" ] && rm -rf "$d"; done; true' bash "$RUN"

echo
echo "wall time: $(( $(date +%s) - START )) seconds; run folder $RUN"
if [ "$status" -ne 0 ]; then echo "SOME GATE FAILED"
elif [ -n "${SKIP_G3:-}" ]; then echo "ALL GATES RUN PASSED; G3 (planted bugs) SKIPPED"
else echo "ALL GATES PASSED"; fi
exit $status
