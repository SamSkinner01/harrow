#!/usr/bin/env bash
# Measures a headless dedicated server's memory: starts it (free play, no
# players), samples its resident memory every 5 s for SECS seconds and
# prints the samples and the peak.
#
#   tools/scripts/server_memory.sh [binary] [port]
#   SECS=60 tools/scripts/server_memory.sh target/release/harrow
#
# Default binary: target/debug/harrow (the client build also runs --server).

set -euo pipefail
cd "$(dirname "$0")/../.."

BIN=${1:-target/debug/harrow}
PORT=${2:-5979}
SECS=${SECS:-30}
LOG=${LOG:-/tmp/harrow-server-memory.log}
export BEVY_ASSET_ROOT=${BEVY_ASSET_ROOT:-$PWD}

"$BIN" --server "$PORT" --no-round > "$LOG" 2>&1 &
PID=$!
trap 'kill $PID 2>/dev/null || true' EXIT
for ((t = 5; t <= SECS; t += 5)); do
    sleep 5
    if ! kill -0 "$PID" 2>/dev/null; then
        echo "server exited early, see $LOG" >&2
        exit 1
    fi
    rss=$(awk '/VmRSS/ {print $2}' "/proc/$PID/status")
    echo "t=${t}s RSS $((rss / 1024)) MB"
done
peak=$(awk '/VmHWM/ {print $2}' "/proc/$PID/status")
echo "peak $((peak / 1024)) MB ($BIN, log $LOG)"
