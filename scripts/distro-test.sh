#!/usr/bin/env bash
# Run paddy on real Linux distros / window managers in containers (needs Docker Desktop running).
#   scripts/distro-test.sh [setup...]     default: all four
# Screenshots land in target/distro-shots/.
set -uo pipefail
cd "$(dirname "$0")/.."
DOCKER=docker
if ! docker version >/dev/null 2>&1; then
    DOCKER="/mnt/c/Program Files/Docker/Docker/resources/bin/docker.exe"
    "$DOCKER" version >/dev/null 2>&1 || { echo "Docker is not running: start Docker Desktop"; exit 2; }
fi
BIN=target/x86_64-unknown-linux-gnu/release/paddy
[ -x "$BIN" ] || { echo "build the portable binary first: scripts/release.sh"; exit 2; }
declare -A IMAGE=([kali-xfce]=kalilinux/kali-rolling [debian-openbox]=debian:bullseye [fedora-i3]=fedora:latest [debian-sway]=debian:bookworm)
SETUPS=("${@:-kali-xfce debian-openbox fedora-i3 debian-sway}")
[ $# -eq 0 ] && SETUPS=(kali-xfce debian-openbox fedora-i3 debian-sway)
mkdir -p target/distro-shots
# Whatever happens (finish, error, Ctrl+C, timeout): remove every test container and image.
cleanup() {
    for s in "${SETUPS[@]}"; do
        "$DOCKER" rm -f "paddy-test-$s" >/dev/null 2>&1
        "$DOCKER" rmi -f "${IMAGE[$s]}" >/dev/null 2>&1
    done
}
trap cleanup EXIT INT TERM
for s in "${SETUPS[@]}"; do
    name="paddy-test-$s"
    "$DOCKER" rm -f "$name" >/dev/null 2>&1
    "$DOCKER" run -d --name "$name" "${IMAGE[$s]}" sleep 3600 >/dev/null || { echo "FAIL $s: could not start container"; continue; }
    # copy files in via tar on stdin (works for both docker and docker.exe)
    tar -C "$(dirname "$BIN")" -cf - paddy | "$DOCKER" exec -i "$name" sh -c 'mkdir -p /opt && tar -C /opt -xf -'
    tar -C scripts/distro -cf - inside.sh | "$DOCKER" exec -i "$name" sh -c 'tar -C /opt -xf -'
    "$DOCKER" exec "$name" sh /opt/inside.sh "$s" 2>&1 | grep -E "^(PASS|FAIL|NOTE|DONE|\[)|^    "
    "$DOCKER" exec "$name" sh -c 'cd /out && tar -cf - . 2>/dev/null' | tar -C target/distro-shots -xf - 2>/dev/null
    "$DOCKER" rm -f "$name" >/dev/null 2>&1
    # delete the image right away: nothing is left behind
    "$DOCKER" rmi -f "${IMAGE[$s]}" >/dev/null 2>&1 && echo "[$s] container and image deleted"
done
echo "screenshots: target/distro-shots/"
