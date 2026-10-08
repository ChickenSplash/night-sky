#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
mkdir -p data
features=()
if [[ " $* " == *" --ui=egui "* ]]; then
    features+=(--no-default-features)
else
    export CEF_PATH="$PWD/data/cef-sdk"
    export LD_LIBRARY_PATH="$CEF_PATH${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    if [[ ! -f "$CEF_PATH/libcef.so" || ! -x "$CEF_PATH/bin/bevy_cef_render_process" ]]; then
        echo "Run ./scripts/setup-native-ui.sh first, or use --ui=egui." >&2
        exit 1
    fi
fi
cargo build --locked -p space-server -p space-client "${features[@]}"
target/debug/space-server > data/server.log 2>&1 &
server_pid=$!
cleanup() { kill "$server_pid" 2>/dev/null || true; }
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
for attempt in {1..50}; do
    if ! kill -0 "$server_pid" 2>/dev/null; then
        cat data/server.log >&2
        exit 1
    fi
    if curl --fail --silent http://127.0.0.1:4321/health >/dev/null; then
        # Make sure the response belongs to our still-running server.
        sleep 0.1
        if ! kill -0 "$server_pid" 2>/dev/null; then
            cat data/server.log >&2
            exit 1
        fi
        target/debug/space-client "$@"
        exit $?
    fi
    sleep 0.1
done
echo "Local API did not start. See data/server.log." >&2
exit 1
