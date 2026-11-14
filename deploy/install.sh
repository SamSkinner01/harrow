#!/usr/bin/env bash
# Installs (or updates) the Harrow dedicated server on this Linux machine.
# Run it as root from the unpacked server bundle:
#
#   tar xzf harrow-server-<build>-linux-x86_64.tar.gz
#   cd harrow-server-<build>-linux-x86_64
#   sudo ./install.sh              # install/update, then (re)start the service
#   sudo ./install.sh --no-start   # install/update only
#
# What it does:
# - creates the system user `harrow` (no login, no home) if it's missing;
# - copies the server to /opt/harrow-server (binary, assets, docs), replacing
#   the previous version;
# - puts the example settings in /etc/harrow-server/server.ron, but only if
#   there is no server.ron yet (your settings are kept on updates);
# - installs/updates the systemd unit harrow-server.service, enables it at
#   boot and (re)starts it.
# Safe to run again for every update.

set -euo pipefail

PREFIX=/opt/harrow-server
CONF_DIR=/etc/harrow-server
UNIT=/etc/systemd/system/harrow-server.service
SERVICE_USER=harrow
START=1

for arg in "$@"; do
    case "$arg" in
        --no-start) START=0 ;;
        -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
        *) echo "unknown option: $arg" >&2; exit 2 ;;
    esac
done

if [[ $EUID -ne 0 ]]; then
    echo "Run me as root: sudo $0 $*" >&2
    exit 1
fi
HERE=$(cd "$(dirname "$0")" && pwd)
for f in harrow assets harrow-server.service server.example.ron; do
    if [[ ! -e "$HERE/$f" ]]; then
        echo "$HERE/$f is missing: run me from the unpacked server bundle." >&2
        exit 1
    fi
done
if [[ $(uname -m) != x86_64 ]]; then
    echo "This bundle is for x86_64 Linux; this machine is $(uname -m)." >&2
    exit 1
fi

# The binary needs glibc 2.35 or newer (Ubuntu 22.04+, Debian 12+).
if ! "$HERE/harrow" --version >/dev/null 2>&1; then
    echo "The server binary doesn't run here:" >&2
    "$HERE/harrow" --version || true
    echo "(it needs x86_64 Linux with glibc 2.35+; see README-VPS.md)" >&2
    exit 1
fi
echo "Installing $("$HERE/harrow" --version)"

# --- User ------------------------------------------------------------------------
if ! id -u "$SERVICE_USER" >/dev/null 2>&1; then
    NOLOGIN=$(command -v nologin || echo /usr/sbin/nologin)
    useradd --system --no-create-home --home-dir /nonexistent --shell "$NOLOGIN" \
        --user-group "$SERVICE_USER"
    echo "Created system user $SERVICE_USER"
fi

# --- Files: a fresh copy next to the old one, then swapped in ---------------------
install -d -m 0755 "$PREFIX"
STAGE="$PREFIX/.new"
rm -rf "$STAGE"
install -d -m 0755 "$STAGE"
install -m 0755 "$HERE/harrow" "$STAGE/harrow"
cp -r "$HERE/assets" "$STAGE/assets"
for f in README-VPS.md server.example.ron BUILD_ID; do
    [[ -e "$HERE/$f" ]] && install -m 0644 "$HERE/$f" "$STAGE/$f"
done
chmod -R u=rwX,go=rX "$STAGE"
for f in harrow assets README-VPS.md server.example.ron BUILD_ID; do
    rm -rf "${PREFIX:?}/$f"
    [[ -e "$STAGE/$f" ]] && mv "$STAGE/$f" "$PREFIX/$f"
done
rmdir "$STAGE"
chown -R root:root "$PREFIX"
echo "Server files in $PREFIX"

# --- Settings: never overwrite the admin's --------------------------------------
install -d -m 0750 -o root -g "$SERVICE_USER" "$CONF_DIR"
if [[ -e "$CONF_DIR/server.ron" ]]; then
    echo "Keeping your settings: $CONF_DIR/server.ron (new example: $PREFIX/server.example.ron)"
else
    install -m 0640 -o root -g "$SERVICE_USER" "$HERE/server.example.ron" "$CONF_DIR/server.ron"
    echo "Settings: $CONF_DIR/server.ron (edit it, then: sudo systemctl restart harrow-server)"
fi

# --- Service ----------------------------------------------------------------------
if [[ ! -d /run/systemd/system ]]; then
    echo "systemd isn't running here: not installing the service."
    echo "Start the server by hand:"
    echo "  sudo -u $SERVICE_USER env BEVY_ASSET_ROOT=$PREFIX $PREFIX/harrow --server --config $CONF_DIR/server.ron"
    exit 0
fi
install -m 0644 "$HERE/harrow-server.service" "$UNIT"
systemctl daemon-reload
systemctl enable harrow-server.service >/dev/null 2>&1
echo "Service harrow-server enabled (starts at boot)"
if [[ $START == 1 ]]; then
    systemctl restart harrow-server.service
    sleep 2
    if systemctl is-active --quiet harrow-server.service; then
        echo "Service harrow-server is running:"
        journalctl -u harrow-server.service -n 8 --no-pager -o cat || true
    else
        echo "Service harrow-server failed to start:" >&2
        journalctl -u harrow-server.service -n 30 --no-pager -o cat >&2 || true
        exit 1
    fi
fi
