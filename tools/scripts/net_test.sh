#!/usr/bin/env bash
# Multiplayer test on one machine: a headless dedicated server and two
# windowed clients (they open on your desktop for ~20 s and quit by
# themselves after taking a screenshot).
#
#   tools/scripts/net_test.sh                     # free play: see each other, movement, lag comp
#   tools/scripts/net_test.sh --round             # round loop with short timers (phase sync)
#   tools/scripts/net_test.sh --latency 80 --jitter 10 --loss 2
#   OUT=/some/dir PORT=5999 tools/scripts/net_test.sh
#
# Latency/jitter/loss: simulated on every instance's incoming packets; the
# round trip gets the latency added twice, so `--latency 80` gives each
# instance 40 ms (about 80 ms RTT on top of the real one).
#
# What it checks (and prints a summary of):
# - both clients connect and pass the handshake (server log "joined");
# - each client sees the other player (net-trace "others [...]") and the
#   positions agree with the server's;
# - prediction rollbacks on the moving client (few = smooth);
# - lag-compensated test shots on the server (--net-shoot-test);
# - round phases match on all three instances (with --round);
# - Bravo quits first: the server reports it left and Alpha stops seeing it.
# Screenshots: $OUT/alpha.png and $OUT/bravo.png. Logs: $OUT/*.log.

set -euo pipefail
cd "$(dirname "$0")/../.."

OUT=${OUT:-/tmp/harrow-net-test}
PORT=${PORT:-5888}
LATENCY=0
JITTER=0
LOSS=0
ROUND=0
while [[ $# -gt 0 ]]; do
    case "$1" in
        --latency) LATENCY=$2; shift 2 ;;
        --jitter) JITTER=$2; shift 2 ;;
        --loss) LOSS=$2; shift 2 ;;
        --round) ROUND=1; shift ;;
        *) echo "unknown option $1" >&2; exit 2 ;;
    esac
done

mkdir -p "$OUT"
cargo build -j 4 --quiet
BIN=$PWD/target/debug/harrow
export BEVY_ASSET_ROOT=$PWD

NET=()
if [[ $LATENCY != 0 || $JITTER != 0 || $LOSS != 0 ]]; then
    NET=(--net-latency $((LATENCY / 2)) --net-jitter "$JITTER" --net-loss "$LOSS")
fi
if [[ $ROUND == 1 ]]; then
    SERVER_MODE=(--round-test)
else
    SERVER_MODE=(--no-round)
fi

PIDS=()
cleanup() {
    for pid in "${PIDS[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
}
trap cleanup EXIT

echo "server: port $PORT ${SERVER_MODE[*]} ${NET[*]}"
"$BIN" --server "$PORT" "${SERVER_MODE[@]}" --net-trace --net-shoot-test "${NET[@]}" \
    > "$OUT/server.log" 2>&1 &
SERVER=$!
PIDS+=("$SERVER")
sleep 3

# Alpha faces west (towards Bravo) and walks slowly towards it; Bravo
# faces east and watches. Both take a screenshot ~10 s in, then Bravo
# leaves first (the disconnect test) and Alpha a few seconds later.
"$BIN" --connect "127.0.0.1:$PORT" --name Alpha --net-trace --look 90 --auto-move 0,0.05 \
    --screenshot "$OUT/alpha.png" --screenshot-frames 1500 --exit-after 20 "${NET[@]}" \
    > "$OUT/alpha.log" 2>&1 &
ALPHA=$!
PIDS+=("$ALPHA")
sleep 1
"$BIN" --connect "127.0.0.1:$PORT" --name Bravo --net-trace --look 270 \
    --screenshot "$OUT/bravo.png" --screenshot-frames 1500 --exit-after 15 "${NET[@]}" \
    > "$OUT/bravo.log" 2>&1 &
BRAVO=$!
PIDS+=("$BRAVO")

# Clients quit by themselves (--exit-after); give up after 60 s.
for pid in "$BRAVO" "$ALPHA"; do
    for _ in $(seq 60); do
        kill -0 "$pid" 2>/dev/null || break
        sleep 1
    done
done
sleep 2
kill "$SERVER" 2>/dev/null || true
wait "$SERVER" 2>/dev/null || true

strip() { sed -e 's/\x1b\[[0-9;]*m//g' "$1"; }
echo
echo "== connections (server) =="
strip "$OUT/server.log" | grep -E "net: .*(joined|left|refus)" || echo "(none!)"
echo
echo "== lag compensation (server) =="
strip "$OUT/server.log" | grep "lag-comp test: .* -> " | tail -n 4 || echo "(no test shots)"
for who in alpha bravo; do
    echo
    echo "== $who: last trace lines =="
    strip "$OUT/$who.log" | grep "net-trace: InGame" | tail -n 3 | cut -c 60- || echo "(never in game)"
    strip "$OUT/$who.log" | grep -E "panicked|ERROR" | grep -v extract_resource | head -n 5 || true
done
echo
echo "== server: last trace lines =="
strip "$OUT/server.log" | grep "net-trace" | tail -n 3 | cut -c 60-
if [[ $ROUND == 1 ]]; then
    echo
    echo "== round phases (server / alpha / bravo) =="
    for who in server alpha bravo; do
        echo "-- $who"
        strip "$OUT/$who.log" | grep -E "round [0-9]+: (phase|.*WIN)" | cut -c 60- | head -n 12
    done
fi
echo
echo "screenshots: $OUT/alpha.png $OUT/bravo.png"
