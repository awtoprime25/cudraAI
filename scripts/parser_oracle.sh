#!/bin/bash
set -euo pipefail
source "$HOME/.cargo/env"
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$HOME/.cache/cudra-target}
cd "$(dirname "$0")/.."
cargo build -q -p cudra-parser --example dump_ast
N=${1:-1000}
BIN="$CARGO_TARGET_DIR/debug/examples/dump_ast"
for seed in 1 2 3 4; do
  for mode in valid wild fuzz; do
    python3 tests/oracle/parser_oracle.py "$BIN" "$N" "$seed" "$mode"
  done
done
python3 tests/oracle/parser_oracle.py "$BIN" 300 1 deep
