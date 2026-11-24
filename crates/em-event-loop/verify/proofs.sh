#!/usr/bin/env bash
# Run a command (default: lake build) on a copy of verify/lean inside the container. Lake writes
# a .lake/config folder next to the lakefile, so it runs in a copy under /cache, not in the repo.
#   verify/proofs.sh lake build
#   verify/proofs.sh lake env lean CounterExamples.lean
# Env: PROOFS_DIR (copy, default /cache/em-event-loop-proofs), BUILD_DIR (lake buildDir, default
# the one in lakefile.lean; set it to get a fresh, empty build).
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROOFS_DIR="${PROOFS_DIR:-/cache/em-event-loop-proofs}"
BUILD_DIR="${BUILD_DIR:-}"
[ "$#" -gt 0 ] || set -- lake build
exec "$HERE/run.sh" bash -c '
set -euo pipefail
dest=$1; build=$2; shift 2
mkdir -p "$dest"
# .lake (left on the host by the Lean language server of an editor) is not copied
(cd /crate/verify/lean && tar --exclude=./.lake -cf - .) | tar -xf - -C "$dest"
if [ -n "$build" ]; then
  sed -i "s#buildDir := .*#buildDir := \"$build\"#" "$dest/lakefile.lean"
fi
cd "$dest"
exec "$@"
' bash "$PROOFS_DIR" "$BUILD_DIR" "$@"
