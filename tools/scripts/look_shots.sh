#!/usr/bin/env bash
# Screenshots of six reference views of Harrow Keep (look-and-feel checks),
# plus a labelled contact sheet and the average fps of each view.
#
#   tools/scripts/look_shots.sh OUT_DIR [extra game flags...]
#   BIN=target/release/harrow tools/scripts/look_shots.sh /tmp/look --window 1920x1080
#
# Views: outside the gate, the main yard toward the keep, the great hall,
# the cistern (basement), the map room (upper floor), the NE tower roof.
# Each run opens a game window for about SECS seconds (default 14: the map
# loads, eye adaptation settles), then takes the shot and quits.
set -euo pipefail
out=${1:?usage: look_shots.sh OUT_DIR [game flags...]}
shift
bin=${BIN:-target/debug/harrow}
secs=${SECS:-14}
# Run from the repository, whichever binary.
export BEVY_ASSET_ROOT=${BEVY_ASSET_ROOT:-$PWD}
mkdir -p "$out"
views=(
  "gate|0,0,44,0,4"
  "yard|2,0,14,0,10"
  "great_hall|6.5,0,-15.2,95,-4"
  "basement|14.5,-3.2,-21,40,-6"
  "upper_room|-3.2,3.2,-23,60,-6"
  "tower_roof|37.5,9.6,-27.5,140,-8"
)
: > "$out/fps.txt"
for view in "${views[@]}"; do
  name=${view%%|*}
  spawn=${view#*|}
  "$bin" --no-round --spawn "$spawn" --screenshot "$out/$name.png" \
    --screenshot-secs "$secs" "$@" > "$out/$name.log" 2>&1 || true
  fps=$(grep -o "average fps [0-9.]*" "$out/$name.log" | tail -1 | awk '{print $3}')
  echo "$name ${fps:-?}" | tee -a "$out/fps.txt"
done
python3 - "$out" <<'EOF'
import sys, os
from PIL import Image, ImageDraw
out = sys.argv[1]
names = ["gate", "yard", "great_hall", "basement", "upper_room", "tower_roof"]
fps = dict(line.split() for line in open(os.path.join(out, "fps.txt")) if len(line.split()) == 2)
shots = [Image.open(os.path.join(out, f"{n}.png")).convert("RGB") for n in names if os.path.exists(os.path.join(out, f"{n}.png"))]
if shots:
    w, h = 640, int(640 * shots[0].height / shots[0].width)
    sheet = Image.new("RGB", (w * 3, h * 2), "black")
    for i, (n, img) in enumerate(zip(names, shots)):
        tile = img.resize((w, h), Image.LANCZOS)
        d = ImageDraw.Draw(tile)
        label = f"{n}  {fps.get(n, '?')} fps"
        d.rectangle([0, 0, 8 + 7 * len(label), 18], fill="black")
        d.text((4, 3), label, fill="white")
        sheet.paste(tile, ((i % 3) * w, (i // 3) * h))
    sheet.save(os.path.join(out, "sheet.png"))
    print("sheet:", os.path.join(out, "sheet.png"))
EOF
