#!/bin/bash
set -euo pipefail
source "$HOME/.cargo/env"
export CARGO_TARGET_DIR=$HOME/.cache/cudra-target
cd /mnt/c/Users/awtop/Documents/cudraAI
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace "$@"
