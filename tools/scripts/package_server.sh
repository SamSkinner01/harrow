#!/usr/bin/env bash
# Builds the dedicated server for Linux x86_64 VPSes and packs it:
#
#   tools/scripts/package_server.sh            # build in a container, pack
#   tools/scripts/package_server.sh --local    # build with this machine's toolchain
#   tools/scripts/package_server.sh --no-build # pack the last build again
#
# Output: dist/harrow-server-<BUILD_ID>-linux-x86_64.tar.gz (BUILD_ID is
# what `harrow --version` prints: version + git commit), containing the
# binary, the assets the server loads (kit models, tuning), the example
# settings, the systemd unit, install.sh and README-VPS.md.
#
# Why a container: a binary built on a new distro (Fedora 44: glibc 2.43)
# refuses to start on older ones. Building inside Ubuntu 22.04 (glibc
# 2.35) makes one that runs on Ubuntu 22.04+, Debian 12+ and anything newer.
# Uses podman (or docker); the first run downloads the ubuntu:22.04 image
# and installs gcc in a cached build image (harrow-server-build). Rust comes
# from your own rustup toolchain (mounted read-only), crates from your cargo
# registry (offline). The build lands in target/vps/ (separate from your
# normal target/).

set -euo pipefail
cd "$(dirname "$0")/../.."
ROOT=$PWD

MODE=container
for arg in "$@"; do
    case "$arg" in
        --local) MODE=local ;;
        --no-build) MODE=none ;;
        -h|--help) sed -n '2,21p' "$0"; exit 0 ;;
        *) echo "unknown option: $arg" >&2; exit 2 ;;
    esac
done

TOOLCHAIN=1.97.1
JOBS=${JOBS:-6}
# The commit the binary reports (and clients are checked against).
COMMIT=$(git rev-parse --short=10 HEAD)
if [[ -n $(git status --porcelain --untracked-files=no) ]]; then
    COMMIT="$COMMIT-dirty"
    echo "warning: uncommitted changes: the build will be $COMMIT, which no player has" >&2
fi

case $MODE in
    container)
        ENGINE=$(command -v podman || command -v docker || true)
        if [[ -z $ENGINE ]]; then
            echo "No podman or docker: use --local (the binary then needs this machine's glibc" >&2
            echo "or newer), or build on the VPS (README \"Dedicated server\")." >&2
            exit 1
        fi
        RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup}
        CARGO_HOME=${CARGO_HOME:-$HOME/.cargo}
        TOOLCHAIN_DIR=$RUSTUP_HOME/toolchains/$TOOLCHAIN-x86_64-unknown-linux-gnu
        if [[ ! -x $TOOLCHAIN_DIR/bin/cargo ]]; then
            echo "Rust $TOOLCHAIN isn't installed: rustup toolchain install $TOOLCHAIN" >&2
            exit 1
        fi
        # Crates for the lockfile, so the container can build offline.
        cargo fetch --locked >/dev/null
        if ! $ENGINE image inspect harrow-server-build >/dev/null 2>&1; then
            echo "== creating the build image (ubuntu:22.04 + gcc), once"
            printf 'FROM docker.io/library/ubuntu:22.04\nRUN apt-get update && apt-get install -y --no-install-recommends gcc libc6-dev && rm -rf /var/lib/apt/lists/*\n' \
                | $ENGINE build -t harrow-server-build -f - .
        fi
        echo "== building the dedicated server in ubuntu:22.04 (build $COMMIT, -j $JOBS)"
        mkdir -p target/vps
        # label=disable: no SELinux relabelling of the (huge) mounted trees.
        $ENGINE run --rm --security-opt label=disable \
            -v "$ROOT:/src" \
            -v "$TOOLCHAIN_DIR:/rust:ro" \
            -v "$CARGO_HOME/registry:/cargo/registry:ro" \
            -e CARGO_HOME=/cargo \
            -e HARROW_BUILD_COMMIT="$COMMIT" \
            -e PATH=/rust/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
            -w /src \
            harrow-server-build \
            cargo build --release --offline --locked -j "$JOBS" \
                --no-default-features --features dedicated --target-dir /src/target/vps
        BIN=target/vps/release/harrow
        ;;
    local)
        echo "== building the dedicated server with the local toolchain (build $COMMIT)"
        HARROW_BUILD_COMMIT=$COMMIT cargo build --release -j "$JOBS" \
            --no-default-features --features dedicated
        BIN=target/release/harrow
        ;;
    none)
        BIN=target/vps/release/harrow
        [[ -x $BIN ]] || BIN=target/release/harrow
        ;;
esac

if [[ ! -x $BIN ]]; then
    echo "no server binary at $BIN" >&2
    exit 1
fi
BUILD_ID=$("$BIN" --version | awk '{print $2}')
if [[ $BUILD_ID != *"+$COMMIT" ]]; then
    echo "warning: $BIN is build $BUILD_ID, not $COMMIT (rebuild it?)" >&2
fi
NAME=harrow-server-$BUILD_ID-linux-x86_64
STAGE=dist/$NAME
rm -rf "$STAGE"
mkdir -p "$STAGE/assets/models" "$STAGE/assets/config"

echo "== packing $NAME"
install -m 0755 "$BIN" "$STAGE/harrow"
strip "$STAGE/harrow" 2>/dev/null || true
# Only what the server loads: kit models (the map's collision) and the
# tuning file. Textures, sounds and character models are presentation: the
# dedicated server doesn't read them (net/dedicated.rs, "lean assets").
cp -r assets/models/kit "$STAGE/assets/models/kit"
cp assets/config/tuning.ron "$STAGE/assets/config/tuning.ron"
cp assets/config/server.ron "$STAGE/assets/config/server.ron"
cp assets/config/server.ron "$STAGE/server.example.ron"
cp deploy/harrow-server.service deploy/README-VPS.md "$STAGE/"
install -m 0755 deploy/install.sh "$STAGE/install.sh"
echo "$BUILD_ID" > "$STAGE/BUILD_ID"
tar -C dist -czf "dist/$NAME.tar.gz" "$NAME"
rm -rf "$STAGE"

GLIBC=$(objdump -T "$BIN" 2>/dev/null | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -n 1)
echo
echo "dist/$NAME.tar.gz ($(du -h "dist/$NAME.tar.gz" | cut -f1); binary needs ${GLIBC:-glibc ?})"
echo "players need build $BUILD_ID (git commit ${COMMIT%-dirty})"
