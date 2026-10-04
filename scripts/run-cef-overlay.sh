#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
export CEF_PATH="$PWD/data/cef-sdk"
export LD_LIBRARY_PATH="$CEF_PATH${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
if [[ ! -f "$CEF_PATH/libcef.so" || ! -x "$CEF_PATH/bin/bevy_cef_render_process" ]]; then
    echo "Run ./scripts/setup-cef-overlay.sh first." >&2
    exit 1
fi

mkdir -p data
cargo build --locked -p space-server -p space-client --features space-client/cef-ui
target/debug/space-server > data/cef-overlay-server.log 2>&1 &
server_pid=$!
cleanup() { kill "$server_pid" 2>/dev/null || true; }
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
for attempt in {1..50}; do
    if ! kill -0 "$server_pid" 2>/dev/null; then
        cat data/cef-overlay-server.log >&2
        exit 1
    fi
    if curl --fail --silent http://127.0.0.1:4321/health >/dev/null; then
        sleep 0.1
        if ! kill -0 "$server_pid" 2>/dev/null; then
            cat data/cef-overlay-server.log >&2
            exit 1
        fi
        target/debug/space-client --cef-html-ui "$@"
        exit $?
    fi
    sleep 0.1
done
echo "Local API did not start. See data/cef-overlay-server.log." >&2
exit 1
