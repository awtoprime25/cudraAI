#!/bin/bash
set -euo pipefail
source "$HOME/.cargo/env"
export CARGO_TARGET_DIR=$HOME/.cache/cudra-target
cd /mnt/c/Users/awtop/Documents/cudraAI
cargo build -q -p cudra-lexer --example dump_tokens
N=${1:-1000}
for seed in 1 2 3; do
  python3 tests/oracle/lexer_oracle.py "$CARGO_TARGET_DIR/debug/examples/dump_tokens" "$N" "$seed" /tmp/lexoracle_$seed
done
