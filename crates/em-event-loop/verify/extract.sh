#!/usr/bin/env bash
# Rust crate -> LLBC (Charon) -> Lean (Aeneas). Run inside the container:
#   verify/run.sh bash /crate/verify/extract.sh
# Copies the sources to a scratch folder in /cache with a generated standalone Cargo.toml (the
# workspace cannot resolve offline), then writes $OUT_DIR/$SUBDIR/{Types,Funs}.lean (never hand-edit).
# The defaults extract the crate into verify/lean/Extracted. The mutation gate (mutants/) sets the
# variables to extract a mutated copy elsewhere.
set -euxo pipefail
SRC_DIR=${SRC_DIR:-/crate/src}
OUT_DIR=${OUT_DIR:-/crate/verify/lean}
SUBDIR=${SUBDIR:-Extracted}
SCRATCH=${SCRATCH:-/cache/em-event-loop-extract}
# SCRATCH is emptied and OUT_DIR/SUBDIR replaced below: keep both inside this crate's own places.
case "$SCRATCH" in
  /cache/em-event-loop-extract|/cache/em-event-loop-runs/*/?*) ;;
  *) echo "SCRATCH must be /cache/em-event-loop-extract or a folder inside a run folder: $SCRATCH" >&2; exit 2 ;;
esac
case "$OUT_DIR" in /crate/verify/lean|/cache/em-event-loop-*) ;; *) echo "OUT_DIR not allowed: $OUT_DIR" >&2; exit 2 ;; esac
[[ "$SUBDIR" =~ ^[A-Za-z]+$ ]] || { echo "SUBDIR not allowed: $SUBDIR" >&2; exit 2; }
case "$SCRATCH$OUT_DIR" in *..*) echo "'..' not allowed in SCRATCH or OUT_DIR" >&2; exit 2 ;; esac

find "$SCRATCH" -mindepth 1 -delete 2>/dev/null || true
mkdir -p "$SCRATCH/src"
cp "$SRC_DIR"/*.rs "$SCRATCH/src/"
cat > "$SCRATCH/Cargo.toml" <<'TOML'
[package]
name = "em-event-loop"
version = "0.1.0"
edition = "2024"

[workspace]
TOML

cd "$SCRATCH"
# The async driver (src/driver.rs, 0016 D8 step 4a) is outside the proofs; Aeneas cannot translate it.
charon cargo --preset=aeneas --exclude crate::driver --dest-file="$SCRATCH/em_event_loop.llbc"
rm -rf "${OUT_DIR:?}/$SUBDIR"
aeneas -backend lean -no-progress-bar -split-files "$SCRATCH/em_event_loop.llbc" \
  -dest "$OUT_DIR" -subdir "/$SUBDIR"
