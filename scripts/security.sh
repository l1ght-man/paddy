#!/usr/bin/env bash
# Security checks in one go. Exit non-zero on any failure.
#   scripts/security.sh          offline checks
#   scripts/security.sh --live   also runs the real-internet download tests
set -euo pipefail
cd "$(dirname "$0")/.."

echo "== 1. no unsafe code in hand-written crates =="
if grep -rn "unsafe" paddy-core/src paddy-net/src paddy-ui/src paddy-core/tests paddy-net/tests paddy-ui/tests 2>/dev/null \
    | grep -v 'forbid(unsafe_code)\|deny(unsafe_code)'; then
    echo "FOUND unsafe"; exit 1
fi
echo "none"

echo "== 2. clippy, warnings are errors =="
cargo clippy --workspace --all-targets -- -D warnings

echo "== 3. tests (includes parser fuzzing, tamper and SSRF tests) =="
cargo test --workspace

echo "== 4. long fuzz soak (300k inputs per parser) =="
FUZZ_ITER=300000 cargo test -p paddy-core --test fuzz --release

echo "== 5. dependency vulnerabilities (RustSec) =="
if command -v cargo-audit >/dev/null; then cargo audit; else echo "cargo-audit missing: cargo install cargo-audit --locked"; exit 1; fi

if [ "${1:-}" = "--live" ]; then
    echo "== 6. live downloads: every font vs its pinned checksum, wrong pin rejected =="
    cargo test -p paddy-net --test live -- --ignored
    cargo test -p paddy-ui --test fonts_live -- --ignored
fi
echo "== security checks passed =="
