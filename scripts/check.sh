#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
mkdir -p data/tmp
export TMPDIR="$PWD/data/tmp"
export PYTHONDONTWRITEBYTECODE=1
python3 scripts/test-ns.py
python3 scripts/test-ice-vi-reference.py
python3 scripts/test-hhe-reference.py
python3 scripts/test-envelope-prescription.py
python3 scripts/test-secondary-reference.py
cargo fmt --all --check
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
