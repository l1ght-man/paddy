---
name: release
description: Build the small optimized Linux binary of paddy. Use when asked to release or ship.
---
1. `scripts/release.sh` → portable binary at `target/x86_64-unknown-linux-gnu/release/paddy` (glibc 2.30+, so Kali, Debian 11+, Ubuntu 20.04+, Fedora 32+). It fetches zig (checksum-pinned) and cargo-zigbuild on first use; no sudo.
2. Report size and the glibc floor it prints.
3. Real-distro check (needs Docker Desktop running): `scripts/distro-test.sh` (Kali+XFCE, Debian 11+Openbox, Fedora+i3, Debian 12+Sway). On the target machine: `scripts/doctor.sh ./paddy`.
Binary only, Linux x86_64. No tarball, no GitHub release, no Windows unless asked.
Runtime libs on X11: libxkbcommon-x11-0, libxcb-xkb1, libxcursor1, libxi6, libx11-xcb1, libfontconfig1 (doctor.sh checks and prints the apt line). In WSL use `scripts/wsl-run.sh --release`.
