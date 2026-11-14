#!/usr/bin/env bash
# Rebuild the weapon blockouts: tools/blender/weapons/*.py -> assets/models/weapons/*.glb
# Extra arguments pass through, e.g. --only tamarisk, or --preview out.png
set -euo pipefail
cd "$(dirname "$0")/../.."
BLENDER="${HARROW_BLENDER:-blender}"
exec "$BLENDER" --background --factory-startup \
    --python tools/blender/weapons/build_weapons.py -- --out assets/models/weapons "$@"
