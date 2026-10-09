#!/usr/bin/env bash
# Browser client with web/index.html and web/star-worker.js linked live, so
# edits to them need only a page refresh.
# Rebuild (rerun this script) only after Rust changes.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
./scripts/build-web.sh
rm -rf web/dist/index.html web/dist/ui web/dist/star-worker.js
ln -s ../index.html web/dist/index.html
ln -s ../star-worker.js web/dist/star-worker.js
cargo build --locked -p space-server
echo "Open http://127.0.0.1:4321/"
exec target/debug/space-server
