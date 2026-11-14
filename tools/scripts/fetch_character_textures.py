#!/usr/bin/env python3
"""Download the CC0 fabric/leather detail textures for characters from
Poly Haven.

Writes assets/textures/characters/<surface>/{normal,arm}.jpg. Only the
normal and ARM (occlusion, roughness, metal) maps are used: colour comes
from the characters' vertex colours, so the team palettes stay exact.
The real-world width of each texture is printed; it must match
`DETAIL_TEXTURES` in src/characters/setup.rs. Every texture here is listed
in ASSETS.md.

Only needed to change or refresh textures; the results are committed (LFS).
Standard library only:  python3 tools/scripts/fetch_character_textures.py
"""

import json
import os
import sys
import urllib.request

# surface -> (Poly Haven id, resolution)
MANIFEST = {
    "uniform": ("stretch_poplin", "1k"),     # shirts and trousers
    "webbing": ("denim_fabric_04", "1k"),    # twill weave for carriers, pouches, straps
    "leather": ("fabric_leather_02", "1k"),  # boots and gloves
}

MAPS = {"nor_gl": "normal", "arm": "arm"}

USER_AGENT = "harrow-asset-fetch (hobby game project)"
ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..")
OUT = os.path.join(ROOT, "assets", "textures", "characters")


def get(url):
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read()


def main():
    for surface, (asset_id, res) in MANIFEST.items():
        files = json.loads(get(f"https://api.polyhaven.com/files/{asset_id}"))
        info = json.loads(get(f"https://api.polyhaven.com/info/{asset_id}"))
        folder = os.path.join(OUT, surface)
        os.makedirs(folder, exist_ok=True)
        for ph_map, ours in MAPS.items():
            data = get(files[ph_map][res]["jpg"]["url"])
            with open(os.path.join(folder, f"{ours}.jpg"), "wb") as f:
                f.write(data)
        width_m = info["dimensions"][0] / 1000.0
        authors = ", ".join(info.get("authors", {}).keys())
        print(f"{surface:8} {asset_id:20} {res}  {width_m:.3f} m  by {authors}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
