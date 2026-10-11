#!/bin/zsh
set -eu
TCAD_ROOT=${0:A:h}
exec "$TCAD_ROOT/scripts/start.sh" "$@"
