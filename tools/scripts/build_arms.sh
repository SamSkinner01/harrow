#!/usr/bin/env bash
# Rebuild the first-person arms: tools/blender/build_arms.py ->
#   assets/models/characters/arms_{attackers,defenders}.glb
# Needs MPFB (build_characters.sh --setup) and the weapon models in
# assets/models/weapons (import_weapons.sh). Extra arguments pass through,
# e.g. --only defenders, or --preview DIR (first-person renders per weapon).
set -euo pipefail
cd "$(dirname "$0")/../.."
BLENDER="${HARROW_BLENDER:-blender}"
exec "$BLENDER" --background --factory-startup \
    --python tools/blender/build_arms.py -- --out assets/models/characters "$@"
