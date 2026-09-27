#!/usr/bin/env bash
# Background RAM/CPU check of the release binary.
# usage: scripts/perf.sh [entries=2000] [seconds=30]
# Seeds a scratch vault, starts paddy against it, samples RSS and CPU% every 2s, prints a summary.
# Needs a display (WSLg is fine). Uses scripts/wsl-run.sh's lib dir if present.
set -euo pipefail
cd "$(dirname "$0")/.."
N=${1:-2000}; SECS=${2:-30}
home=$(mktemp -d); trap 'kill $pid 2>/dev/null || true; rm -rf "$home"' EXIT
export PADDY_HOME="$home"

cargo build --release -q
cargo run -q -p paddy-core --example seed -- "$home/data/paddy/vaults/default.db" "$N"

libdir="$HOME/.local/lib/paddy-dev/root/usr/lib/x86_64-linux-gnu"
[ -d "$libdir" ] && export LD_LIBRARY_PATH="$libdir${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

target/release/paddy & pid=$!
tck=$(getconf CLK_TCK)
ticks() { awk '{print $14+$15}' /proc/$pid/stat; }
rss() { awk '/VmRSS/ {print $2}' /proc/$pid/status; }

# Startup: wait until CPU use settles (<=1 tick/s for 3s running), max 60s.
calm=0; last=$(ticks); waited=0
while [ $calm -lt 3 ] && [ $waited -lt 60 ]; do
    sleep 1; waited=$((waited+1))
    kill -0 $pid 2>/dev/null || { echo "paddy exited during startup"; exit 1; }
    now=$(ticks); if [ $((now-last)) -le 1 ]; then calm=$((calm+1)); else calm=0; fi; last=$now
done
startup_ticks=$(ticks)

# Idle: sample RSS every 2s and total CPU over the window.
t0=$(ticks); s0=$(date +%s.%N); peak=0; sum=0; n=0; end=$((SECONDS + SECS))
while [ $SECONDS -lt $end ]; do
    sleep 2
    r=$(rss); n=$((n+1)); sum=$((sum+r)); [ "$r" -gt "$peak" ] && peak=$r
done
t1=$(ticks); s1=$(date +%s.%N)
cpu=$(awk -v a=$t0 -v b=$t1 -v s=$s0 -v e=$s1 -v t=$tck 'BEGIN{printf "%.2f", (b-a)/t/(e-s)*100}')
echo "paddy with $N entries (pid $pid, ${SECS}s idle window)"
echo "  startup   $(awk -v t=$startup_ticks -v c=$tck 'BEGIN{printf "%.1f", t/c}') s of CPU, settled after ~${waited}s"
echo "  idle cpu  ${cpu} % of one core"
echo "  memory    avg $((sum/n/1024)) MB   peak $((peak/1024)) MB"
echo "  threads   $(awk '/Threads/ {print $2}' /proc/$pid/status)"
