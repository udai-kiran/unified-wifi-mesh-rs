#!/usr/bin/env bash
# Run a command in the aeneas-spike container (Aeneas 2e3d448, Charon 9e193f0, nightly-2026-09-17,
# Lean v4.36.0 with Mathlib). The crate is mounted at /crate: src/ read-only, verify/ read-write.
# Build outputs go to the cache volume; no network is needed.
#   ./run.sh bash /crate/verify/extract.sh
#   ./run.sh bash -c 'cd /crate/verify/lean && lake build'
set -euo pipefail
CRATE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec docker run --rm --network none -u "$(id -u):$(id -g)" \
  -v "${CRATE}/src:/crate/src:ro" \
  -v "${CRATE}/verify:/crate/verify" \
  -v aeneas-spike-lake:/opt/aeneas/backends/lean/.lake \
  -v aeneas-spike-cache:/cache -e CARGO_HOME=/cache/cargo -e PYTHONDONTWRITEBYTECODE=1 \
  -w /crate/verify/lean \
  aeneas-spike:latest "$@"
