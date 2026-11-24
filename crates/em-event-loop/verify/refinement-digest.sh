#!/usr/bin/env bash
# R7: the second digest, over the refinement contract. Host script. Needs a built copy of the
# proofs (PROOFS_DIR, as in proofs.sh; reproduce.sh builds it first). Prints the statements, then
#   refinement N <sha256>
# where N is the number of statement lines and the hash covers Abstraction.lean and the statements
# printed by lean/RefinementStatements.lean. Frozen value (user accepted the blind back-translation,
# 2026-11-24): `refinement 29 478acf91f57efc22b0a631e1add9447fc5ca938f7a060a6a2046514b5f59c0eb`,
# checked by reproduce.sh. A change needs the user's approval and a new back-translation.
# Env: PROOFS_DIR (default /cache/em-event-loop-proofs), STATEMENTS_FILE (where the statements are
# kept; default PROOFS_DIR/refinement-statements.txt).
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROOFS_DIR="${PROOFS_DIR:-/cache/em-event-loop-proofs}"
STATEMENTS_FILE="${STATEMENTS_FILE:-$PROOFS_DIR/refinement-statements.txt}"
"$HERE/run.sh" bash -c '
set -euo pipefail
cd "$1"
EM_STATEMENTS_OUT="$2" lake env lean RefinementStatements.lean > /dev/null
cat "$2"
printf "refinement %s %s\n" "$(wc -l < "$2")" \
  "$(cat Abstraction.lean "$2" | sha256sum | cut -d" " -f1)"
' bash "$PROOFS_DIR" "$STATEMENTS_FILE"
