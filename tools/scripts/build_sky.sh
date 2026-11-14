#!/usr/bin/env bash
# Convert an equirectangular HDRI to the game's sky cubemap (KTX2, rgb9e5).
#
#   tools/scripts/build_sky.sh IN.hdr [OUT.ktx2] [--size 1024] [--sun-clamp 200] [--preview faces.png]
#
# Default output: assets/sky/<name>.ktx2. Needs Blender (decodes the HDR;
# $HARROW_BLENDER or `blender`) and the `zstd` command. Prints the sky's
# horizon colour: copy it to `lighting.fog_colour` in the tuning file, and
# set `lighting.sun_elevation` to the printed sun elevation.
# Script: tools/blender/sky/hdri_to_cubemap.py (conventions in its header).
set -euo pipefail
here=$(cd "$(dirname "$0")/../.." && pwd)
in=${1:?usage: build_sky.sh IN.hdr [OUT.ktx2] [options]}
shift
out="$here/assets/sky/$(basename "${in%.*}" | sed 's/_[0-9]k$//').ktx2"
if [[ $# -gt 0 && $1 != --* ]]; then
  out=$1
  shift
fi
mkdir -p "$(dirname "$out")"
"${HARROW_BLENDER:-blender}" -b --factory-startup \
  --python "$here/tools/blender/sky/hdri_to_cubemap.py" -- "$in" "$out" "$@" 2>&1 \
  | grep -v -E "^(Blender|Read prefs|$)" || true
