#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
rustup target add wasm32-unknown-unknown
version=$(python - <<'PY'
import tomllib
lock = tomllib.load(open('Cargo.lock', 'rb'))
print(next(p['version'] for p in lock['package'] if p['name'] == 'wasm-bindgen'))
PY
)
tool="tools/wasm-bindgen-${version}-x86_64-unknown-linux-musl"
if [[ ! -x "$tool/wasm-bindgen" ]]; then
    mkdir -p tools
    curl --fail --location --silent --show-error \
        "https://github.com/wasm-bindgen/wasm-bindgen/releases/download/${version}/wasm-bindgen-${version}-x86_64-unknown-linux-musl.tar.gz" \
        -o tools/wasm-bindgen.tar.gz
    tar -xzf tools/wasm-bindgen.tar.gz -C tools
fi
cargo build --locked -p space-client --target wasm32-unknown-unknown --profile web-release
mkdir -p web/dist
"$tool/wasm-bindgen" --target web --no-typescript --out-dir web/dist --out-name space_client \
    target/wasm32-unknown-unknown/web-release/space-client.wasm
cp web/index.html web/dist/index.html
# Keep an uncompressed copy for clients without gzip support.
gzip -9 -k -f web/dist/space_client_bg.wasm web/dist/space_client.js
echo "Browser client built in web/dist."
