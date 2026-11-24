#!/usr/bin/env bash
# G3 code layer: every planted bug in a disposable copy, then the verdict table. Run INSIDE the
# container, after RUN/proofs (a built copy of verify/lean) exists:
#   verify/run.sh bash /crate/verify/mutants/run-all.sh RUN
# RUN is a run folder under /cache/em-event-loop-runs/ (reproduce.sh makes it with mktemp -d).
# MUTANT_JOBS (default 4) mutants run at the same time; one Lean run needs about 3 GB.
# Exit 0 only if verdict.py says every mutant is caught. Writes RUN/verdicts.tsv, RUN/mutants/*/
# and RUN/mutants-wall-seconds.txt.
set -uo pipefail
RUN=${1:-}
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
JOBS=${MUTANT_JOBS:-4}
[[ "$JOBS" =~ ^[1-9][0-9]*$ ]] || { echo "bad MUTANT_JOBS: $JOBS" >&2; exit 2; }
[ -d "$RUN/proofs" ] || { echo "no built proofs copy: $RUN/proofs" >&2; exit 2; }

python3 "$HERE/verdict.py" coverage || exit 1
mkdir -p "$RUN/mutants"
start=$(date +%s)
python3 "$HERE/mutate.py" list | cut -f1 \
  | xargs -P "$JOBS" -I{} bash "$HERE/run-mutant.sh" "$RUN" {}
xargs_rc=$?
echo "$(( $(date +%s) - start )) seconds, $JOBS at a time" > "$RUN/mutants-wall-seconds.txt"
cat "$RUN/mutants-wall-seconds.txt"
[ "$xargs_rc" -eq 0 ] || echo "FAIL: run-mutant.sh exited with an error (xargs status $xargs_rc)"

python3 "$HERE/verdict.py" table "$RUN" > "$RUN/verdicts.tsv"
table_rc=$?
cat "$RUN/verdicts.tsv"
[ "$xargs_rc" -eq 0 ] && [ "$table_rc" -eq 0 ]
