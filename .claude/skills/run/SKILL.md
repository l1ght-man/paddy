---
name: run
description: Build and launch paddy (Rust + Slint desktop app) on Linux. Use when asked to run, start, or test the app.
---
1. In WSL use `scripts/wsl-run.sh` (window shows up via WSLg). It fetches libxkbcommon-x11 + libxcb-xkb1 into ~/.local/lib/paddy-dev without sudo. Elsewhere: `cargo run` from repo root. Run in the background (the GUI blocks the terminal).
2. Missing libs on a normal box → tell user: `sudo apt install -y pkg-config libfontconfig1-dev libxkbcommon-dev libxkbcommon-x11-0 libxcb-xkb1`
3. Slint forces X11 under WSL (XWayland) when both winit backends are on; that is expected.
4. `PADDY_HOME=<dir>` points config + vaults at a scratch dir (use it when testing so the real vault is untouched).
5. Headless check without a display: `PADDY_SHOTS=<dir> cargo test -p paddy-ui` renders PNG screenshots of the real UI.
6. Report ok, or only the first real error.
Linux first, Windows later. No --release here.
7. Fake data: `cargo run -p paddy-core --example seed -- <vault.db> 500`. Checks: `scripts/check.sh` (clippy + tests), `scripts/perf.sh [entries] [secs]` (RAM/CPU). ASCII logo preview: `cargo run -p paddy-ui --example logo_preview`.
8. Security: `scripts/security.sh [--live]` (no-unsafe scan, clippy, tests + fuzz, RustSec audit, optional real-internet download checks). Only `paddy-net` touches the network; `paddy-core` is offline.
