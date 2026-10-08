#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
mkdir -p data/tmp
export TMPDIR="$PWD/data/tmp"
export CEF_PATH="$PWD/data/cef-sdk"
export LD_LIBRARY_PATH="$CEF_PATH${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
cargo fmt --all --check
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked -p space-client --no-default-features
cargo clippy --locked -p space-client --no-default-features --all-targets -- -D warnings
