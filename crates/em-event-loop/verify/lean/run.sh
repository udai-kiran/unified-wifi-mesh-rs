#!/usr/bin/env bash
# Run a command in the aeneas-spike container with this folder mounted at /proofs.
#   ./run.sh lake build
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec docker run --rm -u "$(id -u):$(id -g)" \
  -v "${HERE}:/proofs" \
  -v aeneas-spike-lake:/opt/aeneas/backends/lean/.lake \
  -v aeneas-spike-cache:/cache -e CARGO_HOME=/cache/cargo \
  -w /proofs \
  aeneas-spike:latest "$@"
