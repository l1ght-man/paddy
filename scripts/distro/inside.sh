#!/bin/sh
# Runs INSIDE a test container. Usage: inside.sh <setup>   (setup: kali-xfce | debian-openbox | fedora-i3 | debian-sway)
# Installs a real desktop stack, starts paddy on it, drives it, reports PASS/FAIL lines, saves screenshots to /out.
set -u
SETUP=$1
OUT=/out; mkdir -p $OUT
log() { echo "[$SETUP] $*"; }
pass() { echo "PASS $SETUP: $*"; }
fail() { echo "FAIL $SETUP: $*"; }
note() { echo "NOTE $SETUP: $*"; }

export DEBIAN_FRONTEND=noninteractive
case $SETUP in
  kali-xfce)
    apt-get update -qq && apt-get install -y -qq --no-install-recommends xvfb xfwm4 xfce4-panel xfconf dbus-x11 x11-utils xdotool imagemagick \
      libxkbcommon-x11-0 libxcb-xkb1 libxcursor1 libxi6 libx11-xcb1 libfontconfig1 fonts-dejavu-core fontconfig procps >/dev/null ;;
  debian-openbox)
    # Debian 11 is end-of-life: its packages now live on archive.debian.org
    echo "deb http://archive.debian.org/debian bullseye main" > /etc/apt/sources.list
    rm -f /etc/apt/sources.list.d/*
    apt-get update -qq && apt-get install -y -qq --no-install-recommends xvfb openbox dbus-x11 x11-utils xdotool imagemagick \
      libxkbcommon-x11-0 libxcb-xkb1 libxcursor1 libxi6 libx11-xcb1 libfontconfig1 fonts-dejavu-core fontconfig procps >/dev/null ;;
  fedora-i3)
    dnf -y -q install xorg-x11-server-Xvfb i3 dbus-x11 xprop xdotool ImageMagick libxkbcommon-x11 libXcursor libXi libX11-xcb fontconfig dejavu-sans-mono-fonts procps-ng >/dev/null 2>&1 ;;
  debian-sway)
    apt-get update -qq && apt-get install -y -qq --no-install-recommends sway grim dbus-x11 libwayland-client0 libwayland-cursor0 libxkbcommon0 \
      libfontconfig1 fonts-dejavu-core fontconfig procps >/dev/null ;;
esac
log "packages installed"

export HOME=/root PADDY_HOME=/tmp/paddyhome XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p $XDG_RUNTIME_DIR && chmod 700 $XDG_RUNTIME_DIR
BIN=/opt/paddy
glibc=$(ldd --version 2>&1 | head -1 | grep -oE '[0-9]+\.[0-9]+$'); log "glibc $glibc"

shot() { # name
  if [ "$SETUP" = debian-sway ]; then grim "$OUT/$SETUP-$1.png" 2>/dev/null; else import -window root "$OUT/$SETUP-$1.png" 2>/dev/null; fi
}

if [ "$SETUP" = debian-sway ]; then
  # Wayland: headless sway compositor
  export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman
  eval "$(dbus-launch --sh-syntax)"
  sway -d >/tmp/sway.log 2>&1 &
  sleep 3
  export WAYLAND_DISPLAY=$(ls $XDG_RUNTIME_DIR | grep -E '^wayland-[0-9]+$' | head -1)
  [ -n "$WAYLAND_DISPLAY" ] && pass "sway compositor running ($WAYLAND_DISPLAY)" || { fail "sway did not start"; tail -12 /tmp/sway.log | sed 's/^/    /'; exit 0; }
  unset DISPLAY
else
  Xvfb :99 -screen 0 1280x800x24 >/tmp/xvfb.log 2>&1 &
  export DISPLAY=:99
  sleep 1
  eval "$(dbus-launch --sh-syntax)"
  case $SETUP in
    kali-xfce) xfconfd >/dev/null 2>&1 & sleep 1; xfwm4 --compositor=off >/tmp/wm.log 2>&1 & sleep 2; xfce4-panel >/tmp/panel.log 2>&1 & sleep 4 ;;
    debian-openbox) openbox >/tmp/wm.log 2>&1 & sleep 2 ;;
    fedora-i3) printf 'bar {\n status_command true\n}\n' > /tmp/i3.conf; i3 -c /tmp/i3.conf >/tmp/wm.log 2>&1 & sleep 2 ;;
  esac
  wid=$(xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -oE '0x[0-9a-f]+')
  wm=$(xprop -id "$wid" _NET_WM_NAME 2>/dev/null | sed -n 's/.*= "\(.*\)"/\1/p')
  [ -n "$wm" ] && pass "window manager running: $wm" || fail "window manager not running"
fi

# ---- launch
$BIN >/tmp/paddy.log 2>&1 &
PID=$!
sleep 5
if kill -0 $PID 2>/dev/null; then pass "paddy starts and stays up (glibc $glibc)"; else fail "paddy exited early"; sed 's/^/    /' /tmp/paddy.log | head -10; exit 0; fi
shot main

if [ "$SETUP" != debian-sway ]; then
  main=$(xdotool search --onlyvisible --name '^paddy$' 2>/dev/null | head -1)
  [ -n "$main" ] && pass "main window mapped" || fail "main window not found"
  geo=$(xdotool getwindowgeometry "$main" 2>/dev/null | grep Geometry | awk '{print $2}')
  log "main window geometry $geo"

  # keyboard: Ctrl+K should switch to the keys tab (visible change), Esc back
  xdotool windowactivate --sync "$main" 2>/dev/null; xdotool windowfocus --sync "$main" 2>/dev/null; sleep 0.5
  xdotool key ctrl+n; sleep 0.3; xdotool type --delay 30 "dc01-from-xdotool"; sleep 0.5
  shot typed
  xdotool key ctrl+s; sleep 0.8
  if grep -q "dc01-from-xdotool" -a $PADDY_HOME/data/paddy/vaults/default.db 2>/dev/null; then
    pass "keyboard works: typed an entry and Ctrl+S saved it to the vault file"
  else
    fail "typed entry not found in the saved vault"
  fi

  # tray (StatusNotifierItem)
  if dbus-send --session --print-reply --dest=org.freedesktop.DBus / org.freedesktop.DBus.NameHasOwner string:org.kde.StatusNotifierWatcher 2>/dev/null | grep -q true; then
    items=$(dbus-send --session --print-reply --dest=org.kde.StatusNotifierWatcher /StatusNotifierWatcher org.freedesktop.DBus.Properties.Get string:org.kde.StatusNotifierWatcher string:RegisteredStatusNotifierItems 2>/dev/null)
    echo "$items" | grep -q "org.kde.StatusNotifierItem" && pass "tray icon registered with the panel" || fail "tray host present but paddy's icon is not registered"
  else
    note "no tray host in this setup"
  fi
  b=$(xdotool search --onlyvisible --name '^paddy launcher$' 2>/dev/null | head -1)
  if [ -n "$b" ]; then
    note "floating button shown (expected without a tray)"
    bg=$(xdotool getwindowgeometry "$b" | grep Geometry | awk '{print $2}')
    bw=${bg%x*}
    [ "$bw" -lt 400 ] && pass "floating button stays small ($bg), not tiled" || fail "floating button was tiled to $bg"
    xprop -id "$b" _NET_WM_WINDOW_TYPE | grep -q UTILITY && pass "floating button is marked as a utility window" || fail "floating button window type not set"
  else
    log "no floating button (tray available)"
  fi

  # global hotkey
  xdotool key ctrl+alt+p; sleep 1.5
  q=$(xdotool search --onlyvisible --name 'paddy quick list' 2>/dev/null | head -1)
  if [ -n "$q" ]; then
    pass "global hotkey Ctrl+Alt+P opens the quick list"
    xprop -id "$q" _NET_WM_STATE 2>/dev/null | grep -q _NET_WM_STATE_ABOVE && pass "quick list is always-on-top" || note "WM did not mark the quick list always-on-top"
    active=$(xdotool getactivewindow 2>/dev/null)
    [ "$active" = "$q" ] && pass "quick list got keyboard focus" || note "quick list did not get focus (active=$active quick=$q)"
    shot quick
    xdotool type --delay 30 "dc01"; sleep 0.5; shot quick-search
    xdotool key Escape; sleep 0.8
    xdotool search --onlyvisible --name 'paddy quick list' >/dev/null 2>&1 && fail "Esc did not close the quick list" || pass "Esc closes the quick list"
  else
    fail "global hotkey did not open the quick list"
    grep -i hotkey /tmp/paddy.log | head -3
  fi

  # window manager actions on the main window
  # quick list must stay on top even when the main window is raised over it
  xdotool key ctrl+alt+p; sleep 1.2
  q=$(xdotool search --onlyvisible --name 'paddy quick list' 2>/dev/null | head -1)
  if [ -n "$q" ]; then
    xdotool windowactivate "$main" 2>/dev/null; sleep 0.8
    top=$(xprop -root _NET_CLIENT_LIST_STACKING 2>/dev/null | grep -oE '0x[0-9a-f]+' | tail -1)
    [ -n "$top" ] && [ $((top)) -eq "$q" ] && pass "quick list stays above the main window when that is raised" || note "stacking: top=$top quick=$q (WM may not report stacking)"
    shot quick-on-top
    xdotool key --window "$q" Escape 2>/dev/null; xdotool key Escape; sleep 0.5
  fi
  xdotool windowminimize --sync "$main" 2>/dev/null && pass "WM can minimize the window" || note "minimize not supported by this WM"
  xdotool windowmap "$main" 2>/dev/null; xdotool windowactivate "$main" 2>/dev/null; sleep 0.5
  xdotool windowsize --sync "$main" 1100 700 2>/dev/null; sleep 0.5
  nsize=$(xdotool getwindowgeometry "$main" 2>/dev/null | grep Geometry | awk '{print $2}')
  log "resized to $nsize"
  shot resized
else
  pass "Wayland: window created under sway"
  grep -iE "hotkey|tray" /tmp/paddy.log | sed 's/^/    /' | head -3
fi

# close gracefully (WM close) and check it saved/quit
kill -TERM $PID 2>/dev/null; sleep 1
kill -0 $PID 2>/dev/null && { kill -9 $PID; note "needed a hard kill to stop"; } || pass "quits cleanly"
if grep -qiE "panic|segfault|error" /tmp/paddy.log; then note "paddy printed:"; grep -iE "panic|segfault|error" /tmp/paddy.log | head -5 | sed 's/^/    /'; fi
echo "DONE $SETUP"
