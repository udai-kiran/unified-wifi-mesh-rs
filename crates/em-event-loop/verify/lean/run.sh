#!/usr/bin/env bash
# Kept for old instructions: runs the command through ../proofs.sh, which works on a copy of this
# folder under /cache, so lake never writes a .lake folder into the repository.
#   ./run.sh lake build
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "$HERE/../proofs.sh" "$@"
