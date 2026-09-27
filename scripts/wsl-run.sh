#!/usr/bin/env bash
# Run paddy under WSLg without sudo. winit needs libxkbcommon-x11 + libxcb-xkb1
# at runtime; if they are not installed system-wide, fetch the .debs and unpack
# them into ~/.local/lib/paddy-dev (no root needed). Same binary as a normal run.
# usage: scripts/wsl-run.sh [--release] [-- args]
set -euo pipefail
cd "$(dirname "$0")/.."

root="$HOME/.local/lib/paddy-dev/root"
libdir="$root/usr/lib/x86_64-linux-gnu"

LDCONFIG=$(command -v ldconfig || echo /sbin/ldconfig)  # /sbin is not on a normal user's PATH
if ! $LDCONFIG -p | grep -q libxkbcommon-x11.so.0 || ! $LDCONFIG -p | grep -q libxcb-xkb.so.1; then
    if [ ! -e "$libdir/libxkbcommon-x11.so.0" ] || [ ! -e "$libdir/libxcb-xkb.so.1" ]; then
        tmp=$(mktemp -d)
        (cd "$tmp" && apt download libxkbcommon-x11-0 libxcb-xkb1 >/dev/null)
        mkdir -p "$root"
        for d in "$tmp"/*.deb; do dpkg -x "$d" "$root"; done
        rm -rf "$tmp"
    fi
    export LD_LIBRARY_PATH="$libdir${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi

exec cargo run "$@"
