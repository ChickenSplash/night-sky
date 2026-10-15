#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
mkdir -p data/tmp
export TMPDIR="$PWD/data/tmp"
python3 scripts/test-ns.py
cargo fmt --all --check
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
# Craft physics must hold on both targets; these run under Node with the
# wasm-bindgen tools that build-web.sh downloads.
bindgen=$(sed -n '/^name = "wasm-bindgen"$/{n;s/^version = "\(.*\)"$/\1/p}' Cargo.lock)
runner="$PWD/tools/wasm-bindgen-${bindgen}-x86_64-unknown-linux-musl/wasm-bindgen-test-runner"
if [[ -x "$runner" ]]; then
  CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER="$runner" \
    cargo test --locked -p space-simulation --lib --target wasm32-unknown-unknown -- craft::
else
  echo "warning: WebAssembly craft tests skipped; run ./scripts/build-web.sh once" >&2
fi
# The browser's code paths only build for WebAssembly.
cargo clippy --locked -p space-client -p sim-worker --target wasm32-unknown-unknown -- -D warnings
