#!/bin/bash
set -euo pipefail
source "$HOME/.cargo/env"
export CARGO_TARGET_DIR=$HOME/.cache/cudra-target
cd /mnt/c/Users/awtop/Documents/cudraAI
cargo build -q -p cudra-lexer --example dump_tokens
N=${1:-800}
for seed in 1 2 3 4; do
  for mode in rich fuzz; do
    python3 tests/oracle/lexer_oracle_v2.py "$CARGO_TARGET_DIR/debug/examples/dump_tokens" "$N" "$seed" "$mode"
  done
done
