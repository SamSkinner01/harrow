"""Build the fortress kit and export each piece to <out>/<name>.glb.

Usually run through tools/scripts/build_kit.sh (Linux) or build_kit.ps1
(Windows). Directly:

    blender --background --factory-startup --python tools/blender/build_kit.py -- \
        [--out assets/models/kit] [--only NAME ...] [--preview sheet.png]

--preview renders every piece side by side into a PNG instead of exporting.
"""

import argparse
import math
import os
import sys

import bpy
from mathutils import Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from kit.geometry import to_blender  # noqa: E402
from kit.pieces import all_pieces  # noqa: E402


def parse_args():
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", default="assets/models/kit")
    parser.add_argument("--only", nargs="*", default=None)
    parser.add_argument("--preview", default=None)
    return parser.parse_args(argv)


def has_faces(builder):
    return len(builder.bm.faces) > 0


def build_objects(piece, collection):
    """Create Blender objects for a piece; returns them."""
    objects = []
    if has_faces(piece.visual):
        main = piece.visual.to_object(piece.name, collection)
        main["surface"] = piece.surface
        objects.append(main)
    for i, col in enumerate(piece.colliders):
        objects.append(col.to_object(f"COL_{piece.name}_{i}", collection))
    for i, frag in enumerate(piece.fragments):
        obj = frag.to_object(f"FRAG_{piece.name}_{i}", collection)
        obj["surface"] = piece.surface
        objects.append(obj)
    return objects


def export_piece(piece, out_dir):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    build_objects(piece, bpy.context.scene.collection)
    path = os.path.join(out_dir, f"{piece.name}.glb")
    bpy.ops.export_scene.gltf(
        filepath=path,
        export_format="GLB",
        export_extras=True,
        export_tangents=True,
        export_yup=True,
        export_apply=True,
        export_animations=False,
        export_skins=False,
        export_morph=False,
        export_cameras=False,
        export_lights=False,
        export_image_format="NONE",
        check_existing=False,
    )
    return path


def render_preview(pieces, path):
    """All pieces in a grid, colliders hidden, rendered with Workbench."""
    bpy.ops.wm.read_factory_settings(use_empty=True)
    scene = bpy.context.scene
    per_row, spacing = 8, 5.0
    for index, piece in enumerate(pieces):
        col, row = index % per_row, index // per_row
        offset = to_blender((col * spacing, 0.0, row * spacing))
        for obj in build_objects(piece, scene.collection):
            obj.location = offset
            if obj.name.startswith("COL_"):
                obj.hide_render = True
    rows = math.ceil(len(pieces) / per_row)
    centre = to_blender(((per_row - 1) * spacing / 2, 0.0, (rows - 1) * spacing / 2))

    cam_data = bpy.data.cameras.new("cam")
    cam_data.lens = 28
    cam = bpy.data.objects.new("cam", cam_data)
    scene.collection.objects.link(cam)
    cam.location = centre + to_blender((0.0, 26.0, 30.0))
    direction = centre - cam.location
    cam.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()
    scene.camera = cam

    scene.render.engine = "BLENDER_WORKBENCH"
    scene.display.shading.light = "STUDIO"
    scene.display.shading.color_type = "MATERIAL"
    scene.display.shading.show_shadows = True
    scene.display.shading.show_cavity = True
    scene.render.resolution_x, scene.render.resolution_y = 1600, 1000
    scene.render.filepath = path
    bpy.ops.render.render(write_still=True)
    print(f"preview written to {path}")


def main():
    args = parse_args()
    pieces = all_pieces()
    if args.only:
        pieces = [p for p in pieces if p.name in args.only]
    if args.preview:
        render_preview(pieces, os.path.abspath(args.preview))
        return
    out_dir = os.path.abspath(args.out)
    os.makedirs(out_dir, exist_ok=True)
    for piece in pieces:
        export_piece(piece, out_dir)
        print(f"exported {piece.name}")
    print(f"{len(pieces)} pieces -> {out_dir}")


main()
