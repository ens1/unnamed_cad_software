#!/usr/bin/env sh
set -eu
TCAD_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$TCAD_ROOT"
cargo build --workspace --locked
exec "$TCAD_ROOT/target/debug/tcad-desktop" "$@"
