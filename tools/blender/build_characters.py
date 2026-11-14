"""Build every operator from its config and export one .glb each.

Usually run through tools/scripts/build_characters.sh (Linux) or
build_characters.ps1 (Windows). Directly:

    blender --background --factory-startup --python tools/blender/build_characters.py -- \
        [--out assets/models/characters] [--only auger ...] [--preview DIR] [--keep-blend DIR]

--preview DIR renders each operator (front, side, back, and a few animation
poses) into DIR as PNGs, in addition to exporting.
--keep-blend DIR also saves each operator's .blend there, for inspection.

Requires MPFB 2.0.18 and the MakeHuman CC0 system assets; see
`build_characters.sh --setup`.
"""

import argparse
import os
import sys
import tempfile
import time

import bpy

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from characters import config, export, pipeline  # noqa: E402


def parse_args():
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", default="assets/models/characters")
    parser.add_argument("--only", nargs="*", default=None)
    parser.add_argument("--preview", default=None)
    parser.add_argument("--keep-blend", default=None)
    parser.add_argument("--no-export", action="store_true")
    return parser.parse_args(argv)


def main():
    args = parse_args()
    configs = config.load_all(args.only)
    out_dir = os.path.abspath(args.out)
    os.makedirs(out_dir, exist_ok=True)
    scratch = tempfile.mkdtemp(prefix="harrow_characters_")
    for cfg in configs:
        start = time.time()
        bpy.ops.wm.read_factory_settings(use_empty=True)
        character = pipeline.build(cfg, os.path.join(scratch, cfg["id"]))
        stats = export.stats(character)
        print(f"{cfg['id']}: {stats}")
        if args.keep_blend:
            os.makedirs(args.keep_blend, exist_ok=True)
            bpy.ops.wm.save_as_mainfile(filepath=os.path.abspath(os.path.join(args.keep_blend, f"{cfg['id']}.blend")))
        if not args.no_export:
            path = export.export_glb(character, out_dir)
            print(f"exported {path}")
        if args.preview:
            export.render_previews(character, os.path.abspath(args.preview))
        print(f"{cfg['id']} done in {time.time() - start:.1f} s")
    print(f"{len(configs)} operators -> {out_dir}")


main()
