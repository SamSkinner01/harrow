"""Build every operator from its config and export one .glb each.

Usually run through tools/scripts/build_characters.sh (Linux) or
build_characters.ps1 (Windows). Directly:

    blender --background --factory-startup --python tools/blender/build_characters.py -- \
        [--out assets/models/characters] [--only auger ...] [--preview DIR] [--keep-blend DIR] \
        [--mocap DIR]

--preview DIR renders each operator (front, side, back, and a few animation
poses) into DIR as PNGs, in addition to exporting.
--keep-blend DIR also saves each operator's .blend there, for inspection.
--mocap DIR is the motion capture staging folder (the one holding
cmu_mocap_bvh_selection/source/*.bvh; default $HARROW_MOCAP or
~/harrow-assets-staging/mocap). The BVH files are only read at build time;
they are not part of the repository (see ASSETS.md).
--textures DIR is where the shared skin detail maps are written
(default assets/textures/characters; the fabric maps there come from
tools/scripts/fetch_character_textures.py).

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

from characters import config, export, materials, mpfb, pipeline  # noqa: E402


def parse_args():
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", default="assets/models/characters")
    parser.add_argument("--textures", default="assets/textures/characters")
    parser.add_argument("--only", nargs="*", default=None)
    parser.add_argument("--preview", default=None)
    parser.add_argument("--keep-blend", default=None)
    parser.add_argument("--no-export", action="store_true")
    parser.add_argument("--mocap", default=os.environ.get(
        "HARROW_MOCAP", os.path.expanduser("~/harrow-assets-staging/mocap")))
    return parser.parse_args(argv)


def main():
    args = parse_args()
    configs = config.load_all(args.only)
    out_dir = os.path.abspath(args.out)
    os.makedirs(out_dir, exist_ok=True)
    scratch = tempfile.mkdtemp(prefix="harrow_characters_")
    textures = os.path.abspath(args.textures)
    mocap = os.path.abspath(args.mocap)
    if not os.path.isdir(os.path.join(mocap, "cmu_mocap_bvh_selection", "source")):
        sys.exit(f"motion capture not found in {mocap} (expected cmu_mocap_bvh_selection/source/*.bvh); "
                 "pass --mocap DIR or set HARROW_MOCAP")
    bpy.ops.wm.read_factory_settings(use_empty=True)
    mpfb.services()
    materials.write_skin_detail(os.path.join(textures, "skin"))
    for cfg in configs:
        start = time.time()
        bpy.ops.wm.read_factory_settings(use_empty=True)
        character = pipeline.build(cfg, os.path.join(scratch, cfg["id"]), mocap)
        stats = export.stats(character)
        print(f"{cfg['id']}: {stats}")
        if args.keep_blend:
            os.makedirs(args.keep_blend, exist_ok=True)
            bpy.ops.wm.save_as_mainfile(filepath=os.path.abspath(os.path.join(args.keep_blend, f"{cfg['id']}.blend")))
        if not args.no_export:
            path = export.export_glb(character, out_dir)
            print(f"exported {path}")
        if args.preview:
            materials.add_preview_detail(character.materials, textures)
            export.render_previews(character, os.path.abspath(args.preview))
        print(f"{cfg['id']} done in {time.time() - start:.1f} s")
    print(f"{len(configs)} operators -> {out_dir}")


main()
