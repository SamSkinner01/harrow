#!/usr/bin/env bash
# Rebuild every operator:
#   tools/blender/characters/operators/*.toml -> assets/models/characters/*.glb
# Extra arguments pass through, e.g. --only auger, --preview DIR, --keep-blend DIR,
# --mocap DIR (the CMU BVH staging folder; default $HARROW_MOCAP or
# ~/harrow-assets-staging/mocap)
#
# First time only: --setup installs MPFB 2.0.18 and the MakeHuman CC0 system
# assets into your Blender user configuration (downloads ~330 MB).
set -euo pipefail
cd "$(dirname "$0")/../.."
BLENDER="${HARROW_BLENDER:-blender}"
if [[ "${1:-}" == "--setup" ]]; then
    exec "$BLENDER" --background --factory-startup \
        --python tools/blender/characters/setup_mpfb.py
fi
exec "$BLENDER" --background --factory-startup \
    --python tools/blender/build_characters.py -- --out assets/models/characters "$@"
