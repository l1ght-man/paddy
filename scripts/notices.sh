#!/usr/bin/env bash
# Regenerate THIRD-PARTY-NOTICES.md (needs: cargo install --locked cargo-about --features cli).
# cargo-about does not find Slint's license texts (they live in LICENSES/), so the
# royalty-free license paddy uses Slint under is appended from the slint crate itself.
set -euo pipefail
cd "$(dirname "$0")/.."
out=THIRD-PARTY-NOTICES.md
cargo about generate about.hbs -o "$out" 2>/dev/null
ver=$(cargo metadata --format-version 1 | python3 -c "import sys,json; print(next(p['version'] for p in json.load(sys.stdin)['packages'] if p['name']=='slint'))")
src=$(cargo metadata --format-version 1 | python3 -c "import sys,json,os; print(os.path.dirname(next(p['manifest_path'] for p in json.load(sys.stdin)['packages'] if p['name']=='slint')))")
{
    printf '\n## Slint Royalty-free Desktop, Mobile, and Web Applications License 2.0\n\n'
    printf 'Used by: slint %s and its i-slint-* / slint-* crates (Slint is offered under GPL-3.0-only OR this license OR a commercial license; paddy uses this one).\n\n' "$ver"
    printf '```text\n'; cat "$src/LICENSES/LicenseRef-Slint-Royalty-free-2.0.md"; printf '\n```\n'
} >> "$out"
echo "wrote $out"
