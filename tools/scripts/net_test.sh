#!/usr/bin/env bash
# Multiplayer test on one machine: a headless dedicated server and two
# windowed clients (they open on your desktop for ~20-30 s and quit by
# themselves after taking screenshots).
#
#   tools/scripts/net_test.sh                     # free play: see each other, movement, lag comp
#   tools/scripts/net_test.sh --round             # round loop with short timers (phase sync)
#   tools/scripts/net_test.sh --latency 80 --jitter 10 --loss 2
#   tools/scripts/net_test.sh --gadgets frag      # gadget scenarios (below)
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
#
# Gadget scenarios (--gadgets NAME, free play; Alpha attacks, Bravo
# defends). The server sets them up with --net-script (equip, teleport,
# reinforce: src/net/test_script.rs); the clients press keys with
# --gadget-test (real input over the network) and log what they see:
#   frag    Alpha throws a frag at Bravo, who stands by a wall: Bravo's
#           health drops on all three; both clients see the grenade fly.
#   smoke   Alpha throws smoke between them: both clients pour the cloud.
#   flash   Alpha throws a flashbang at Bravo's feet: Bravo (only) is
#           blinded; screenshots show the white-out.
#   drone   Alpha throws a drone and drives it towards Bravo: Bravo sees it
#           move, Alpha's screen is the drone camera.
#   wire    Bravo lays razor wire, Alpha walks through it: slowed (0.35)
#           while inside, on the server and predicted on Alpha's client,
#           with no extra rollbacks (net-trace rollback counter).
#   cutter  Bravo reinforces a wall, Alpha clamps a ring cutter on it and
#           cuts through: the hole is the same on all three instances.
#   cctv    Bravo watches the courtyard camera, Alpha shoots it out: Bravo's
#           feed moves to the next camera.
#   all     every scenario above, one after the other.
# Screenshots: $OUT/<scenario>-alpha*.png, $OUT/<scenario>-bravo*.png.

set -euo pipefail
cd "$(dirname "$0")/../.."

OUT=${OUT:-/tmp/harrow-net-test}
PORT=${PORT:-5888}
LATENCY=0
JITTER=0
LOSS=0
ROUND=0
GADGETS=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --latency) LATENCY=$2; shift 2 ;;
        --jitter) JITTER=$2; shift 2 ;;
        --loss) LOSS=$2; shift 2 ;;
        --round) ROUND=1; shift ;;
        --gadgets) GADGETS=$2; shift 2 ;;
        *) echo "unknown option $1" >&2; exit 2 ;;
    esac
done

if [[ $GADGETS == all ]]; then
    for scenario in frag smoke flash drone wire cutter cctv; do
        "$0" --gadgets "$scenario" --latency "$LATENCY" --jitter "$JITTER" --loss "$LOSS"
    done
    exit 0
fi

mkdir -p "$OUT"
cargo build -j 4 --quiet
BIN=$PWD/target/debug/harrow
export BEVY_ASSET_ROOT=$PWD

NET=()
if [[ $LATENCY != 0 || $JITTER != 0 || $LOSS != 0 ]]; then
    NET=(--net-latency $((LATENCY / 2)) --net-jitter "$JITTER" --net-loss "$LOSS")
fi

strip() { sed -e 's/\x1b\[[0-9;]*m//g' "$1"; }

PIDS=()
cleanup() {
    for pid in "${PIDS[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
}
trap cleanup EXIT

# Waits for the clients (they quit by themselves; give up after 90 s), then
# stops the server.
finish() {
    local server=$1
    shift
    for pid in "$@"; do
        for _ in $(seq 90); do
            kill -0 "$pid" 2>/dev/null || break
            sleep 1
        done
    done
    sleep 2
    kill "$server" 2>/dev/null || true
    wait "$server" 2>/dev/null || true
}

# ---- Gadget scenarios ---------------------------------------------------------------
if [[ -n $GADGETS ]]; then
    # Client scripts run on the client's clock (Alpha starts 3 s after the
    # server, Bravo 4 s); server commands naming a player wait for it to join.
    S=$OUT/$GADGETS
    case $GADGETS in
        frag)
            SERVER="6 equip Alpha lark frag; 6 equip Bravo fathom; 6 tp Alpha 9 0 6.5; 6 tp Bravo 5 0 1.2; 18 log; 22 log"
            # Cooked for 2.3 s of the 3 s fuse: it goes off near Bravo.
            ALPHA="8 aimat 5 1.0 1.2; 9.5 hold4 2.3; 12.0 shot $S-alpha-flight.png; 12.1 log; 14.5 log; 15 shot $S-alpha.png; 16 quit"
            BRAVO="7 aimat 9 1.2 6.5; 10.8 log; 10.9 shot $S-bravo-flight.png; 13.5 log; 14 shot $S-bravo.png; 15 quit"
            ;;
        smoke)
            SERVER="6 equip Alpha lark smoke; 6 equip Bravo fathom; 6 tp Alpha 8.5 0 9.5; 6 tp Bravo 8.5 0 2; 20 log"
            ALPHA="8 look 0 -6; 9 tap4; 15 log; 15.5 shot $S-alpha.png; 17 quit"
            BRAVO="7 aimat 8.5 1.4 9.5; 14 log; 14.5 shot $S-bravo.png; 16 quit"
            ;;
        flash)
            SERVER="6 equip Alpha lark flash; 6 equip Bravo fathom; 6 tp Alpha 8.5 0 9; 6 tp Bravo 8.5 0 4; 13 log"
            ALPHA="8 aimat 8.5 0 5.5; 9 tap4; 11.2 log; 11.5 shot $S-alpha.png; 13 quit"
            BRAVO="7 aimat 8.5 1.4 6; 10.2 log; 10.4 shot $S-bravo.png; 13 log; 13.5 shot $S-bravo-later.png; 14 quit"
            ;;
        drone)
            # The drone drives ~3 m towards the keep, stops and turns to Bravo.
            SERVER="6 equip Alpha lark; 6 equip Bravo fathom; 6 tp Alpha 8 0 9; 6 tp Bravo 10.5 0 3; 17 log; 19 log"
            ALPHA="8 look 0 0; 9 g; 11 drive 0 1; 12 drive 0 0; 12.5 dface 10.5 3; 13 log; 14 shot $S-alpha-view.png; 14.5 log; 15 g; 16.5 log; 17 quit"
            BRAVO="7 aimat 8 0.1 1.8; 11 log; 12.5 log; 13.5 shot $S-bravo.png; 14 log; 16 quit"
            ;;
        wire)
            # Bravo lays the wire across x=8.5 at z=8 and steps aside; Alpha
            # walks north through it (server t~21-23).
            SERVER="6 equip Bravo mortise wire; 6 equip Alpha lark; 6 tp Bravo 8.5 0 9; 6 tp Alpha 8.5 0 4; 17 log; 18 tp Bravo 11 0 3.5; 21.6 log; 25 log"
            BRAVO="7 look 0 0; 8 hold4 1.8; 11 log; 15 aimat 8.5 0.5 8; 17.6 shot $S-bravo.png; 20 log; 21 quit"
            ALPHA="7 look 180 0; 16 log; 17 move 0 1; 17.4 log; 17.8 log; 18.2 log; 18.4 shot $S-alpha.png; 18.6 log; 19 log; 19.4 log; 20.5 move 0 0; 21 log; 22 quit"
            ;;
        cutter)
            SERVER="6 equip Bravo mortise; 6 equip Alpha auger; 6 tp Bravo 7 0 -5.6; 6 tp Alpha 7 0 -2.4; 12 reinforce Bravo; 13 log; 21 log; 30 log"
            BRAVO="6 look 180 0; 9.5 log; 19 log; 20 shot $S-bravo-blade.png; 27 log; 27.5 shot $S-bravo.png; 29 quit"
            ALPHA="7 look 0 -12; 11 hold3 2; 13.5 tap3; 18 log; 26.5 log; 27 shot $S-alpha.png; 29 quit"
            ;;
        cctv)
            # Bravo watches the courtyard camera; Alpha shoots it out: the
            # feed is lost on Bravo's client and moves to the next camera.
            SERVER="6 equip Alpha lark; 6 equip Bravo fathom; 6 tp Alpha 11 0 6; 6 tp Bravo 4 0 7; 16 log; 20 log"
            BRAVO="8 g; 9.5 log; 10 shot $S-bravo-view.png; 13 log; 13.5 shot $S-bravo-next.png; 14.5 g; 15 log; 16 quit"
            ALPHA="8 aimat 13.6 2.7 0.45; 9.8 shot $S-alpha.png; 11 fire; 11.4 fire; 11.8 fire; 13 log; 14 shot $S-alpha-after.png; 15 quit"
            ;;
        *) echo "unknown scenario $GADGETS (frag smoke flash drone wire cutter cctv all)" >&2; exit 2 ;;
    esac
    # Positions are on Harrow Keep section A (the first, small map).
    NET+=(--map section_a)
    echo "server: port $PORT, gadget scenario $GADGETS ${NET[*]}"
    "$BIN" --server "$PORT" --no-round --net-trace --net-script "$SERVER" "${NET[@]}" \
        > "$S-server.log" 2>&1 &
    SERVER_PID=$!
    PIDS+=("$SERVER_PID")
    sleep 3
    "$BIN" --connect "127.0.0.1:$PORT" --name Alpha --net-trace --gadget-test "$ALPHA" "${NET[@]}" \
        > "$S-alpha.log" 2>&1 &
    ALPHA_PID=$!
    PIDS+=("$ALPHA_PID")
    sleep 1
    "$BIN" --connect "127.0.0.1:$PORT" --name Bravo --net-trace --gadget-test "$BRAVO" "${NET[@]}" \
        > "$S-bravo.log" 2>&1 &
    BRAVO_PID=$!
    PIDS+=("$BRAVO_PID")
    finish "$SERVER_PID" "$ALPHA_PID" "$BRAVO_PID"

    pattern='gadget: |net script t=|test_script:   |panicked|ERROR'
    for who in server alpha bravo; do
        echo
        echo "== $GADGETS: $who =="
        strip "$S-$who.log" | grep -E "$pattern" | cut -c 28- \
            | sed -E 's/^ *INFO harrow::[a-z_:]+: //' \
            | grep -vE "100% intact, reinforced false|average fps|screenshot ->|cctv: |gadget test t=.*(Shot|Quit)" \
            | head -n 70 || true
        strip "$S-$who.log" | grep "net-trace: InGame" | tail -n 1 \
            | grep -oE "rollbacks [0-9]+ \([0-9]+ ticks\)( \| others' private state [0-9]+)?" \
            | sed 's/^/last trace: /' || true
    done
    if [[ $GADGETS == wire ]]; then
        echo
        echo "== wire: Alpha's rollbacks, once a second (walking through the wire at t=17..20.5) =="
        strip "$S-alpha.log" | grep "net-trace: InGame" | grep -oE "rollbacks [0-9]+ \([0-9]+ ticks\)" | uniq -c
    fi
    echo
    echo "screenshots: $(ls "$S"-*.png 2>/dev/null | tr '\n' ' ')"
    exit 0
fi

# ---- Movement, lag compensation, rounds -----------------------------------------------
if [[ $ROUND == 1 ]]; then
    SERVER_MODE=(--round-test)
else
    SERVER_MODE=(--no-round)
fi

echo "server: port $PORT ${SERVER_MODE[*]} ${NET[*]}"
"$BIN" --server "$PORT" "${SERVER_MODE[@]}" --net-trace --net-shoot-test "${NET[@]}" \
    > "$OUT/server.log" 2>&1 &
SERVER=$!
PIDS+=("$SERVER")
sleep 3

# Alpha faces west (towards Bravo) and walks slowly towards it; Bravo
# faces east and watches. Both take a screenshot ~10 s in, then Bravo
# leaves first (the disconnect test) and Alpha a few seconds later. In
# between, Alpha opens fire on Bravo (server-side, lag-compensated hits).
"$BIN" --connect "127.0.0.1:$PORT" --name Alpha --net-trace --look 90 --auto-move 0,0.05 \
    --screenshot "$OUT/alpha.png" --screenshot-secs 11 --auto-fire 11.5 --exit-after 20 "${NET[@]}" \
    > "$OUT/alpha.log" 2>&1 &
ALPHA=$!
PIDS+=("$ALPHA")
sleep 1
"$BIN" --connect "127.0.0.1:$PORT" --name Bravo --net-trace --look 270 \
    --screenshot "$OUT/bravo.png" --screenshot-secs 10 --exit-after 15 "${NET[@]}" \
    > "$OUT/bravo.log" 2>&1 &
BRAVO=$!
PIDS+=("$BRAVO")

finish "$SERVER" "$BRAVO" "$ALPHA"

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
