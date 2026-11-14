#!/usr/bin/env bash
# Deploys the dedicated server to your Linux VPS over SSH:
#
#   tools/scripts/deploy_server.sh user@host               # UDP 5888
#   tools/scripts/deploy_server.sh user@host --port 6000   # another port
#   tools/scripts/deploy_server.sh user@host --ssh-port 2222
#
# 1. packs the server (tools/scripts/package_server.sh) unless dist/ already
#    has the bundle for the current commit;
# 2. copies it to the VPS (scp, into a temporary folder);
# 3. runs its install.sh with sudo there (user `harrow`, /opt/harrow-server,
#    settings /etc/harrow-server/server.ron kept on updates, systemd service
#    harrow-server enabled and restarted). --port sets the port in
#    server.ron;
# 4. shows the service's log for a few seconds, and prints the firewall
#    commands for the port.
#
# Needs: your SSH access to the VPS (a key or password), and sudo there
# (you'll be asked for the sudo password if it needs one). The VPS needs
# x86_64 Linux with glibc 2.35+ (Ubuntu 22.04+, Debian 12+) and systemd.
# Extra ssh/scp options: SSH_OPTS="-i ~/.ssh/my_vps_key" tools/scripts/...

set -euo pipefail
cd "$(dirname "$0")/../.."

TARGET=""
PORT=""
SSH_PORT=22
REPACK=0
while [[ $# -gt 0 ]]; do
    case "$1" in
        --port) PORT=$2; shift 2 ;;
        --ssh-port) SSH_PORT=$2; shift 2 ;;
        --repack) REPACK=1; shift ;;
        -h|--help) sed -n '2,21p' "$0"; exit 0 ;;
        -*) echo "unknown option $1" >&2; exit 2 ;;
        *) TARGET=$1; shift ;;
    esac
done
if [[ -z $TARGET ]]; then
    echo "usage: $0 user@host [--port N] [--ssh-port N] [--repack]" >&2
    exit 2
fi
if [[ -n $PORT && ! $PORT =~ ^[0-9]+$ ]]; then
    echo "--port needs a number" >&2
    exit 2
fi

# --- 1. The bundle for this commit ---------------------------------------------------
COMMIT=$(git rev-parse --short=10 HEAD)
[[ -n $(git status --porcelain --untracked-files=no) ]] && COMMIT="$COMMIT-dirty"
BUNDLE=$(ls -t dist/harrow-server-*+"$COMMIT"-linux-x86_64.tar.gz 2>/dev/null | head -n 1 || true)
if [[ -z $BUNDLE || $REPACK == 1 ]]; then
    tools/scripts/package_server.sh
    BUNDLE=$(ls -t dist/harrow-server-*+"$COMMIT"-linux-x86_64.tar.gz | head -n 1)
fi
NAME=$(basename "$BUNDLE" .tar.gz)
echo "== deploying $NAME to $TARGET"

read -r -a EXTRA <<< "${SSH_OPTS:-}"
SSH=(ssh -p "$SSH_PORT" -o ConnectTimeout=15 "${EXTRA[@]}")
# A terminal for sudo's password prompt.
SSH_TTY=(ssh -t -p "$SSH_PORT" -o ConnectTimeout=15 "${EXTRA[@]}")

# --- 2. Copy ------------------------------------------------------------------------
REMOTE_DIR=$("${SSH[@]}" "$TARGET" 'mktemp -d /tmp/harrow-deploy.XXXXXX')
scp -P "$SSH_PORT" -q "${EXTRA[@]}" "$BUNDLE" "$TARGET:$REMOTE_DIR/"
echo "copied to $TARGET:$REMOTE_DIR"

# --- 3. Install (and set the port), restart ------------------------------------------
SET_PORT=""
if [[ -n $PORT ]]; then
    # Rewrite `port: N` in the settings (created from the example on the first
    # install); install.sh restarts the service afterwards.
    SET_PORT="sudo ./install.sh --no-start && sudo sed -i -E 's/^([[:space:]]*)port:[[:space:]]*[0-9]+/\\1port: $PORT/' /etc/harrow-server/server.ron && "
fi
"${SSH_TTY[@]}" "$TARGET" "set -e; cd '$REMOTE_DIR' && tar xzf '$NAME.tar.gz' && cd '$NAME' && ${SET_PORT}sudo ./install.sh; cd / && rm -rf '$REMOTE_DIR'"

# --- 4. Log and firewall ---------------------------------------------------------------
echo
echo "== server log (Ctrl+C stops watching; the server keeps running)"
"${SSH_TTY[@]}" "$TARGET" "sudo timeout 8 journalctl -u harrow-server -n 20 -f --no-pager -o short-iso" || true

# Without --port: the default (or whatever server.ron on the VPS says).
SHOW_PORT=${PORT:-5888}
HOST=${TARGET#*@}
cat <<EOF

== Open UDP $SHOW_PORT on the VPS (once). Whichever firewall it uses:
  ufw (Ubuntu/Debian):    sudo ufw allow $SHOW_PORT/udp
  firewalld (Fedora/RHEL): sudo firewall-cmd --permanent --add-port=$SHOW_PORT/udp && sudo firewall-cmd --reload
  iptables:               sudo iptables -I INPUT -p udp --dport $SHOW_PORT -j ACCEPT   (make it persistent)
And in your provider's web console (cloud firewall / security group):
  inbound rule: UDP, port $SHOW_PORT, source 0.0.0.0/0 (and ::/0 for IPv6).

== Players join with build $(echo "$NAME" | sed -E 's/^harrow-server-(.*)-linux-x86_64$/\1/')
  menu: Join Game -> $HOST$( [[ $SHOW_PORT != 5888 ]] && echo ":$SHOW_PORT")
  or:   harrow --connect $HOST:$SHOW_PORT --name YourName
Settings: ssh $TARGET, then sudo nano /etc/harrow-server/server.ron && sudo systemctl restart harrow-server
EOF
