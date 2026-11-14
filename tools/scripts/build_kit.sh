#!/usr/bin/env bash
# Rebuild the fortress kit: tools/blender/kit/*.py -> assets/models/kit/*.glb
# Extra arguments pass through, e.g. --only stone_wall_2m, or --preview out.png
set -euo pipefail
cd "$(dirname "$0")/../.."
BLENDER="${HARROW_BLENDER:-blender}"
exec "$BLENDER" --background --factory-startup \
    --python tools/blender/build_kit.py -- --out assets/models/kit "$@"
