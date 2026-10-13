#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
mkdir -p data/tmp
export TMPDIR="$PWD/data/tmp"
python3 scripts/test-ns.py
cargo fmt --all --check
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
# The browser's code paths only build for WebAssembly.
cargo clippy --locked -p space-client -p sim-worker --target wasm32-unknown-unknown -- -D warnings
