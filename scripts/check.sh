#!/usr/bin/env bash
# One-shot health check: formatting-free lint + all tests. Exit non-zero on any problem.
set -euo pipefail
cd "$(dirname "$0")/.."
echo "== clippy (warnings are errors) =="
cargo clippy --workspace --all-targets -- -D warnings
echo "== tests =="
cargo test --workspace
echo "== ok =="
