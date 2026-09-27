#!/bin/sh
# paddy installer / uninstaller.
#
#   curl -fsSL https://raw.githubusercontent.com/OWNER/paddy/main/install.sh | sh
#   curl -fsSL https://raw.githubusercontent.com/OWNER/paddy/main/install.sh | sh -s -- --uninstall
#
# Options:  --uninstall    remove everything this script installed (your vaults are kept)
#           --from-source  build with cargo instead of downloading a release
#           --system       install to /usr/local (asks for sudo) instead of ~/.local
#           --no-deps      never offer to apt-install missing libraries
#
# What it does: downloads the latest release for x86_64 Linux, checks its SHA-256,
# installs the binary, a menu entry and icons, and puts the bin folder on your PATH.
# It never touches your vaults or settings.
set -eu

REPO="${PADDY_REPO:-OWNER/paddy}"
BASE_URL="${PADDY_BASE_URL:-https://github.com/$REPO/releases/latest/download}"
ASSET="paddy-x86_64-linux.tar.gz"
MARK="# added by the paddy installer"

say() { printf '\033[32m::\033[0m %s\n' "$*"; }
warn() { printf '\033[33m!!\033[0m %s\n' "$*" >&2; }
die() { printf '\033[31mxx\033[0m %s\n' "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

UNINSTALL=0 SOURCE=0 SYSTEM=0 DEPS=1
for a in "$@"; do
    case $a in
        --uninstall) UNINSTALL=1 ;;
        --from-source) SOURCE=1 ;;
        --system) SYSTEM=1 ;;
        --no-deps) DEPS=0 ;;
        -h|--help) sed -n '2,17p' "$0" 2>/dev/null || echo "see the top of install.sh"; exit 0 ;;
        *) die "unknown option: $a" ;;
    esac
done

if [ "$SYSTEM" = 1 ]; then
    PREFIX=/usr/local SUDO=sudo
    [ "$(id -u)" = 0 ] && SUDO=
else
    PREFIX="$HOME/.local" SUDO=
fi
BIN="$PREFIX/bin"
APPS="$PREFIX/share/applications"
ICONS="$PREFIX/share/icons/hicolor"

refresh_caches() {
    have gtk-update-icon-cache && $SUDO gtk-update-icon-cache -q -t -f "$ICONS" 2>/dev/null || true
    have update-desktop-database && $SUDO update-desktop-database -q "$APPS" 2>/dev/null || true
}

shell_rcs() { for f in "$HOME/.profile" "$HOME/.bashrc" "$HOME/.zshrc"; do [ -f "$f" ] && echo "$f"; done; }

# ---------------------------------------------------------------- uninstall
if [ "$UNINSTALL" = 1 ]; then
    $SUDO rm -f "$BIN/paddy" "$BIN/paddy-doctor" "$APPS/paddy.desktop" "$ICONS/scalable/apps/paddy.svg"
    for s in 16 24 32 48 64 128 256 512; do $SUDO rm -f "$ICONS/${s}x${s}/apps/paddy.png"; done
    for f in $(shell_rcs); do
        if grep -q "$MARK" "$f"; then
            tmp=$(mktemp); grep -v "$MARK" "$f" > "$tmp" && cat "$tmp" > "$f"; rm -f "$tmp"
        fi
    done
    refresh_caches
    say "paddy removed. Your vaults and settings are still in ~/.local/share/paddy and ~/.config/paddy"
    say "(delete those two folders too if you want everything gone)"
    exit 0
fi

# ---------------------------------------------------------------- checks
[ "$(uname -s)" = Linux ] || die "paddy currently supports Linux only"
[ "$(uname -m)" = x86_64 ] || [ "$SOURCE" = 1 ] || die "prebuilt binaries are x86_64 only; try --from-source"
have curl || have wget || die "need curl or wget"

fetch() { # url dest
    case $1 in
        file://*) curl -fsSL "$1" -o "$2" ;;  # local test releases only (PADDY_BASE_URL=file://...)
        *) if have curl; then curl -fsSL --proto '=https' --tlsv1.2 "$1" -o "$2"; else wget -q --https-only -O "$2" "$1"; fi ;;
    esac
}
case $BASE_URL in https://*|file://*) ;; *) die "refusing a non-https download URL: $BASE_URL" ;; esac

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT INT TERM

# ---------------------------------------------------------------- get the files
if [ "$SOURCE" = 0 ]; then
    say "downloading $ASSET"
    if fetch "$BASE_URL/$ASSET" "$WORK/$ASSET" && fetch "$BASE_URL/SHA256SUMS" "$WORK/SHA256SUMS"; then
        expected=$(grep " $ASSET\$" "$WORK/SHA256SUMS" | cut -d' ' -f1)
        actual=$(sha256sum "$WORK/$ASSET" | cut -d' ' -f1)
        [ -n "$expected" ] && [ "$expected" = "$actual" ] || die "checksum mismatch for $ASSET: not installing"
        say "checksum ok"
        tar -xzf "$WORK/$ASSET" -C "$WORK"
        PKG="$WORK/paddy-x86_64-linux"
    else
        warn "no release download available; building from source instead"
        SOURCE=1
    fi
fi
if [ "$SOURCE" = 1 ]; then
    have cargo || die "building from source needs Rust: https://rustup.rs  (then run this again)"
    have git || die "building from source needs git"
    say "building from source (a few minutes)"
    git clone --depth 1 "https://github.com/$REPO.git" "$WORK/src" >/dev/null 2>&1 || die "could not clone https://github.com/$REPO"
    (cd "$WORK/src" && cargo build --release -p paddy-ui) || die "build failed (Debian/Kali: sudo apt install -y pkg-config libfontconfig1-dev libxkbcommon-dev)"
    PKG="$WORK/pkg"; mkdir -p "$PKG/icons"
    cp "$WORK/src/target/release/paddy" "$PKG/"
    cp "$WORK/src/packaging/paddy.desktop" "$WORK/src/scripts/doctor.sh" "$PKG/"
    cp "$WORK/src/assets/icons/"* "$PKG/icons/"
fi

# ---------------------------------------------------------------- install
say "installing to $PREFIX"
$SUDO mkdir -p "$BIN" "$APPS" "$ICONS/scalable/apps"
$SUDO install -m 755 "$PKG/paddy" "$BIN/paddy"
$SUDO install -m 755 "$PKG/doctor.sh" "$BIN/paddy-doctor"
$SUDO install -m 644 "$PKG/paddy.desktop" "$APPS/paddy.desktop"
$SUDO install -m 644 "$PKG/icons/paddy.svg" "$ICONS/scalable/apps/paddy.svg"
for s in 16 24 32 48 64 128 256 512; do
    [ -f "$PKG/icons/paddy-$s.png" ] || continue
    $SUDO mkdir -p "$ICONS/${s}x${s}/apps"
    $SUDO install -m 644 "$PKG/icons/paddy-$s.png" "$ICONS/${s}x${s}/apps/paddy.png"
done
refresh_caches

# ---------------------------------------------------------------- PATH
case ":$PATH:" in
    *":$BIN:"*) ;;
    *)
        if [ "$SYSTEM" = 0 ]; then
            line="export PATH=\"\$HOME/.local/bin:\$PATH\" $MARK"
            rcs=$(shell_rcs); [ -z "$rcs" ] && rcs="$HOME/.profile" && touch "$rcs"
            for f in $rcs; do grep -q "$MARK" "$f" || printf '\n%s\n' "$line" >> "$f"; done
            say "added ~/.local/bin to PATH (open a new terminal, or run: export PATH=\"\$HOME/.local/bin:\$PATH\")"
        fi ;;
esac

# ---------------------------------------------------------------- runtime libraries
# ldconfig is in /sbin, which is not on a normal user's PATH on Debian/Kali
LDCONFIG=$(command -v ldconfig || echo /sbin/ldconfig)
missing=""
libs="libfontconfig.so.1:libfontconfig1 libxkbcommon.so.0:libxkbcommon0"
if [ "${XDG_SESSION_TYPE:-}" != wayland ] || [ -n "${DISPLAY:-}" ]; then
    libs="$libs libX11.so.6:libx11-6 libX11-xcb.so.1:libx11-xcb1 libXcursor.so.1:libxcursor1 libXi.so.6:libxi6"
    libs="$libs libxkbcommon-x11.so.0:libxkbcommon-x11-0 libxcb-xkb.so.1:libxcb-xkb1"
fi
[ -n "${WAYLAND_DISPLAY:-}" ] && libs="$libs libwayland-client.so.0:libwayland-client0"
for pair in $libs; do
    $LDCONFIG -p 2>/dev/null | grep -q "${pair%%:*}" || missing="$missing ${pair#*:}"
done
if [ -n "$missing" ]; then
    warn "missing libraries:$missing"
    if [ "$DEPS" = 1 ] && have apt-get && [ -r /dev/tty ]; then
        printf 'Install them now with "sudo apt-get install -y%s"? [Y/n] ' "$missing" > /dev/tty
        read -r ans < /dev/tty || ans=n
        case $ans in [nN]*) warn "skipped; paddy won't start until they are installed" ;;
            *) sudo apt-get install -y $missing ;; esac
    else
        warn "install them with: sudo apt-get install -y$missing"
    fi
fi

say "done. Start it from your applications menu, or run: paddy"
say "check your desktop (tray, hotkey, libraries) any time with: paddy-doctor"
