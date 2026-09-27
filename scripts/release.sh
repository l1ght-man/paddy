#!/usr/bin/env bash
# Portable release binary: runs on any x86_64 Linux with glibc 2.30+ (Kali, Debian 11+, Ubuntu 20.04+, Fedora 32+).
# Uses zig as the linker (no sudo): downloads it once into ~/.local/share/paddy-tools, checksum-verified.
set -euo pipefail
cd "$(dirname "$0")/.."
ZIG_VER=0.16.0
ZIG_SHA=70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00
ZDIR=$HOME/.local/share/paddy-tools/zig-x86_64-linux-$ZIG_VER
if [ ! -x "$ZDIR/zig" ]; then
    mkdir -p "$(dirname "$ZDIR")"; tmp=$(mktemp)
    curl -fsSL -o "$tmp" "https://ziglang.org/download/$ZIG_VER/zig-x86_64-linux-$ZIG_VER.tar.xz"
    echo "$ZIG_SHA  $tmp" | sha256sum -c - >/dev/null
    tar -C "$(dirname "$ZDIR")" -xf "$tmp"; rm "$tmp"
fi
command -v cargo-zigbuild >/dev/null || cargo install cargo-zigbuild --locked
PATH="$ZDIR:$PATH" cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.31 -p paddy-ui
B=target/x86_64-unknown-linux-gnu/release/paddy
ls -lh "$B"; file "$B"
echo "needs glibc $(objdump -T "$B" | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1 | cut -d_ -f2)+"
