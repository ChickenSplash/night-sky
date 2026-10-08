#!/usr/bin/env bash
# Project-local CEF SDK and helper tools. No sudo or global Cargo changes.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
export CEF_PATH="$PWD/data/cef-sdk"
mkdir -p data

if [[ ! -x data/cef-tools/bin/export-cef-dir ]]; then
    cargo install export-cef-dir --version '145.6.1+145.0.28' --locked \
        --root data/cef-tools --target-dir target/cef-tools
fi
if [[ ! -f "$CEF_PATH/libcef.so" ]]; then
    data/cef-tools/bin/export-cef-dir "$CEF_PATH"
fi
if [[ ! -x "$CEF_PATH/bin/bevy_cef_render_process" ]]; then
    cargo install bevy_cef_render_process --version 0.11.0 --locked \
        --root "$CEF_PATH" --target-dir target/cef-tools
fi
echo "Native HTML UI runtime ready in $CEF_PATH"
