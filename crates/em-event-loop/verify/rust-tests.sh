#!/usr/bin/env bash
# Run the crate's Rust tests on the nightly of the container, from the standalone copy that
# extract.sh makes (needs extract.sh to have run). Run inside the container:
#   verify/run.sh bash /crate/verify/rust-tests.sh [scratch_dir]
set -euo pipefail
SCRATCH=${1:-/cache/em-event-loop-extract}
cd "$SCRATCH"
CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/cache/target-em-event-loop} cargo +nightly-2026-09-17 test --offline
