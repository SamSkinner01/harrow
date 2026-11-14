#!/usr/bin/env bash
# Build the realistic weapon models from the staged CC0 sources:
#   tools/blender/weapons/import_models.py -> assets/models/weapons/{tamarisk,acacia}.glb
# The sources are not in the repo (see ASSETS.md); point HARROW_ASSET_STAGING
# (or --staging) at the asset staging folder. Extra arguments pass through,
# e.g. --only acacia, or --preview DIR (renders instead of exporting).
set -euo pipefail
cd "$(dirname "$0")/../.."
BLENDER="${HARROW_BLENDER:-blender}"
STAGING="${HARROW_ASSET_STAGING:-$HOME/harrow-assets-staging}"
exec "$BLENDER" --background --factory-startup --disable-autoexec \
    --python tools/blender/weapons/import_models.py -- \
    --staging "$STAGING" --out assets/models/weapons "$@"
