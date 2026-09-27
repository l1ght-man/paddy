#!/usr/bin/env bash
# paddy compatibility check. Run it on the machine where paddy should run:
#   scripts/doctor.sh [path/to/paddy]
# Read-only: it changes nothing on the system. The launch test uses a throwaway
# data folder, so your real vaults and settings are never touched.
set -u
BIN=${1:-}
if [ -z "$BIN" ]; then
    for c in ./paddy target/x86_64-unknown-linux-gnu/release/paddy target/release/paddy "$(command -v paddy 2>/dev/null)"; do
        [ -n "$c" ] && [ -x "$c" ] && BIN=$c && break
    done
fi

ok=0; warn=0; bad=0
good() { printf '  \033[32mok\033[0m    %s\n' "$*"; ok=$((ok+1)); }
note() { printf '  \033[33mnote\033[0m  %s\n' "$*"; warn=$((warn+1)); }
fail() { printf '  \033[31mFAIL\033[0m  %s\n' "$*"; bad=$((bad+1)); }
hint() { printf '        → %s\n' "$*"; }
have() { command -v "$1" >/dev/null 2>&1; }

echo "paddy doctor"
echo "== system"
. /etc/os-release 2>/dev/null && good "${PRETTY_NAME:-unknown distro} ($(uname -m))"
if grep -qi microsoft /proc/version 2>/dev/null; then
    note "running inside WSL: launch with scripts/wsl-run.sh (it supplies the X11 keyboard libraries without sudo)"
fi
[ "$(uname -m)" = x86_64 ] || fail "paddy is built for x86_64; this machine is $(uname -m)"
glibc=$(ldd --version 2>/dev/null | head -1 | grep -oE '[0-9]+\.[0-9]+$')
if [ -n "$glibc" ] && [ "$(printf '%s\n2.30\n' "$glibc" | sort -V | head -1)" = "2.30" ]; then
    good "glibc $glibc (needs 2.30+)"
else
    fail "glibc ${glibc:-unknown}: paddy needs 2.30 or newer"
fi

echo "== session"
session=${XDG_SESSION_TYPE:-}
[ -z "$session" ] && { [ -n "${WAYLAND_DISPLAY:-}" ] && session=wayland || { [ -n "${DISPLAY:-}" ] && session=x11; }; }
desk=${XDG_CURRENT_DESKTOP:-${DESKTOP_SESSION:-unknown}}
case "$session" in
    x11) good "X11 session, desktop: $desk" ;;
    wayland) good "Wayland session, desktop: $desk (see the hotkey note below)" ;;
    *) fail "no graphical session found (DISPLAY and WAYLAND_DISPLAY are empty)"; hint "run this from a terminal inside your desktop, not over plain ssh" ;;
esac
if [ -n "${DISPLAY:-}" ] && have xprop; then
    wid=$(xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -oE '0x[0-9a-f]+')
    wm=$( [ -n "$wid" ] && xprop -id "$wid" _NET_WM_NAME 2>/dev/null | sed -n 's/.*= "\(.*\)"/\1/p')
    if [ -n "$wm" ]; then good "window manager: $wm"; else note "window manager doesn't identify itself (EWMH); basic windows still work"; fi
    if xprop -root _NET_SUPPORTED 2>/dev/null | grep -q _NET_WM_STATE_ABOVE; then
        good "window manager supports always-on-top (quick list, floating button)"
    else
        note "window manager may ignore always-on-top: the quick list can open behind other windows"
    fi
fi

echo "== libraries"
# ldconfig is in /sbin, which is not on a normal user's PATH on Debian/Kali
LDCONFIG=$(command -v ldconfig || echo /sbin/ldconfig)
missing_pkgs=""
check_lib() { # lib  debian-package  why
    if $LDCONFIG -p 2>/dev/null | grep -q "$1"; then good "$1"; else
        if [ "$4" = required ]; then fail "$1 missing ($3)"; else note "$1 missing ($3)"; fi
        missing_pkgs="$missing_pkgs $2"
    fi
}
check_lib libfontconfig.so.1 libfontconfig1 "text rendering" required
if [ "$session" = x11 ] || [ -n "${DISPLAY:-}" ]; then
    check_lib libX11.so.6 libx11-6 "X11 windows" required
    check_lib libX11-xcb.so.1 libx11-xcb1 "X11 windows" required
    check_lib libXcursor.so.1 libxcursor1 "mouse cursors" required
    check_lib libXi.so.6 libxi6 "X11 input" required
    check_lib libxkbcommon.so.0 libxkbcommon0 "keyboard" required
    check_lib libxkbcommon-x11.so.0 libxkbcommon-x11-0 "keyboard on X11" required
    check_lib libxcb-xkb.so.1 libxcb-xkb1 "keyboard on X11" required
fi
if [ "$session" = wayland ]; then
    check_lib libwayland-client.so.0 libwayland-client0 "Wayland windows" required
    check_lib libwayland-cursor.so.0 libwayland-cursor0 "Wayland cursors" optional
    check_lib libxkbcommon.so.0 libxkbcommon0 "keyboard" required
fi
if [ -n "$missing_pkgs" ]; then
    if have apt; then hint "sudo apt install -y$missing_pkgs"; else hint "install the packages that provide:$missing_pkgs"; fi
fi
if have fc-list && fc-list 2>/dev/null | grep -qi "DejaVu Sans Mono"; then
    good "default font DejaVu Sans Mono"
else
    note "DejaVu Sans Mono not installed; another monospace font is used"
    hint "sudo apt install -y fonts-dejavu-core   (or download a font in paddy: packs tab)"
fi

echo "== tray icon"
if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ] && [ ! -S "${XDG_RUNTIME_DIR:-/nonexistent}/bus" ]; then
    note "no D-Bus session bus: no tray icon; paddy shows its floating button instead"
elif have gdbus && [ "$(gdbus call --session --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus --method org.freedesktop.DBus.NameHasOwner org.kde.StatusNotifierWatcher 2>/dev/null)" = "(true,)" ]; then
    good "a tray (StatusNotifier) host is running: the paddy icon will appear"
else
    note "no StatusNotifier tray host found: paddy shows its floating button instead"
    case "$desk" in
        *XFCE*) hint "XFCE: right-click the panel → Panel → Add New Items → \"Status Tray Plugin\"" ;;
        *GNOME*) hint "GNOME: install the \"AppIndicator and KStatusNotifierItem Support\" extension" ;;
        *i3*|*sway*) hint "i3: use a bar with SNI support, or run snixembed; sway: waybar with the tray module" ;;
        *) hint "add a system-tray / status-notifier applet to your panel" ;;
    esac
fi

echo "== global hotkey (quick list from anywhere)"
if [ "$session" = wayland ]; then
    note "Wayland doesn't let apps grab global hotkeys: use the tray icon or the floating button"
    hint "or log in to an X11 session (e.g. \"Xfce Session\" on Kali) for the hotkey"
elif [ "$session" = x11 ]; then
    good "X11: the hotkey can work (paddy reports in its status line if the combo is already taken)"
fi

echo "== clipboard"
if [ "$session" = x11 ]; then
    if pgrep -x -u "$(id -u)" 'xfce4-clipman|clipit|parcellite|copyq|diodon|greenclip|gpaste-daemon|klipper' >/dev/null 2>&1; then
        good "a clipboard manager is running: copied text survives paddy quitting"
    else
        note "no clipboard manager: on X11, text copied from paddy is gone once paddy quits (normal X11 behavior)"
        hint "keep paddy running, or: sudo apt install -y xfce4-clipman"
    fi
fi

echo "== launch test"
if [ -z "$BIN" ]; then
    note "paddy binary not found; pass its path: scripts/doctor.sh /path/to/paddy"
elif [ "$session" != x11 ] && [ "$session" != wayland ]; then
    note "skipped (no graphical session)"
else
    tmp=$(mktemp -d)
    PADDY_HOME="$tmp" timeout 5 "$BIN" >"$tmp/out.log" 2>&1
    code=$?
    if [ $code -eq 124 ]; then
        good "paddy started and stayed up (window should have flashed for 5 s)"
    else
        fail "paddy exited early (code $code):"
        sed 's/^/        /' "$tmp/out.log" | head -8
    fi
    rm -rf "$tmp"
fi

echo
echo "result: $ok ok, $warn notes, $bad problems"
[ $bad -eq 0 ]
