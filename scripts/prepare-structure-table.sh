#!/usr/bin/env bash
# Rebuild the embedded structure table from the reference solvers. Needs the
# local H/He density resource (scripts/prepare-hhe-density.py). Rerun whenever
# structure physics or a table grid changes, then commit the resource.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
cargo test --release --locked -p space-simulation prepare_structure_table -- --ignored --nocapture
cargo test --release --locked -p space-simulation structure_table_accuracy -- --ignored --nocapture
