#!/usr/bin/env python3
"""Download the CC0 textures used by the game from Poly Haven.

Writes assets/textures/<material>/{diff,normal,arm}.jpg and prints each
texture's real-world size (used by src/map/materials.rs to tile at true
scale). Every texture here must also be listed in ASSETS.md.

Only needed to change or refresh textures; the results are committed (LFS).
Standard library only:  python3 tools/scripts/fetch_textures.py
"""

import json
import os
import sys
import urllib.request

# material name -> (Poly Haven id, resolution)
MANIFEST = {
    "stone": ("rustic_stone_wall", "2k"),
    "plaster": ("patterned_clay_plaster", "2k"),
    "flagstone": ("mixed_stone_tiles", "2k"),
    "sand": ("coast_sand_01", "2k"),
    "wood": ("weathered_plank_siding", "1k"),
    "metal": ("rusty_metal_02", "1k"),
    "burlap": ("hessian_230", "1k"),
}

# Poly Haven map name -> our file name
MAPS = {"Diffuse": "diff", "nor_gl": "normal", "arm": "arm"}

USER_AGENT = "harrow-asset-fetch (hobby game project)"
ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..")
OUT = os.path.join(ROOT, "assets", "textures")


def get(url):
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read()


def main():
    for material, (asset_id, res) in MANIFEST.items():
        files = json.loads(get(f"https://api.polyhaven.com/files/{asset_id}"))
        info = json.loads(get(f"https://api.polyhaven.com/info/{asset_id}"))
        folder = os.path.join(OUT, material)
        os.makedirs(folder, exist_ok=True)
        for ph_map, ours in MAPS.items():
            url = files[ph_map][res]["jpg"]["url"]
            data = get(url)
            with open(os.path.join(folder, f"{ours}.jpg"), "wb") as f:
                f.write(data)
        width_m = info["dimensions"][0] / 1000.0
        authors = ", ".join(info.get("authors", {}).keys())
        print(f"{material:10} {asset_id:24} {res}  {width_m:.2f} m  by {authors}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
