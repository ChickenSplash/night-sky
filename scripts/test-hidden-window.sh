#!/usr/bin/env bash
# Regression check for the NVIDIA/Wayland swap-chain timeout (vendor/README.md):
# hides the client on a hidden Hyprland workspace (42), then brings it back with focus.
# Usage: scripts/test-hidden-window.sh [cycles] [seconds hidden]
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
cycles=${1:-3}
hidden=${2:-60}
command -v hyprctl >/dev/null || { echo "Needs Hyprland (hyprctl)." >&2; exit 2; }
mkdir -p data/logs
cargo build --locked -p space-client
log=data/logs/hidden-window-test.log
target/debug/space-client >"$log" 2>&1 &
pid=$!
trap 'kill "$pid" 2>/dev/null || true' EXIT

window=""
for _ in {1..100}; do
    window=$(hyprctl clients -j | python3 -c "import json, sys
print(next((c['address'] for c in json.load(sys.stdin) if c['pid'] == $pid), ''))")
    [[ -n $window ]] && break
    sleep 0.1
done
[[ -n $window ]] || { echo "Client window did not appear." >&2; exit 1; }
# Returns to each monitor's visible workspace in turn: the original crash came back
# on a different monitor from the one the window was hidden from.
read -ra shown <<<"$(hyprctl monitors -j | python3 -c "import json, sys
print(' '.join(str(m['activeWorkspace']['id']) for m in json.load(sys.stdin)))")"
dispatch() { hyprctl dispatch "$1" >/dev/null; }

for cycle in $(seq "$cycles"); do
    home=${shown[$(((cycle - 1) % ${#shown[@]}))]}
    echo "Cycle $cycle/$cycles: hidden for ${hidden}s, then shown on workspace $home"
    dispatch "hl.dsp.window.move({ workspace = '42', window = 'address:$window' })"
    sleep "$hidden"
    dispatch "hl.dsp.window.move({ workspace = '$home', window = 'address:$window' })"
    dispatch "hl.dsp.focus({ window = 'address:$window' })"
    sleep 5
    if ! kill -0 "$pid" 2>/dev/null; then
        echo "FAILED: the client exited after returning from the hidden workspace. See $log." >&2
        grep -a -m3 "panicked\|ERROR" "$log" >&2 || true
        exit 1
    fi
done
skipped=$(grep -ac "Skipped a frame" "$log" || true)
echo "Passed: survived $cycles hide/show cycles ($skipped skipped-frame warnings logged)."
