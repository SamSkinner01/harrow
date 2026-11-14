"""Build the weapon blockouts and export each to <out>/<name>.glb.

Usually run through tools/scripts/build_weapons.sh (Linux) or
build_weapons.ps1 (Windows). Directly:

    blender --background --factory-startup \
        --python tools/blender/weapons/build_weapons.py -- \
        [--out assets/models/weapons] [--only NAME ...] [--preview sheet.png]

--preview renders every weapon (side and three-quarter views) into a PNG
instead of exporting.
"""

import argparse
import os
import sys

import bpy

# tools/blender, so both `kit` and `weapons` import as packages.
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from kit.geometry import to_blender  # noqa: E402
from weapons.models import MATERIALS, all_weapons  # noqa: E402


def parse_args():
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", default="assets/models/weapons")
    parser.add_argument("--only", nargs="*", default=None)
    parser.add_argument("--preview", default=None)
    return parser.parse_args(argv)


def create_materials():
    """Pre-create the weapon materials so kit.geometry.material() finds them
    (with preview colours) instead of making magenta placeholders."""
    for name, (rgb, metallic, roughness, alpha) in MATERIALS.items():
        mat = bpy.data.materials.new(name)
        bsdf = mat.node_tree.nodes.get("Principled BSDF")
        if bsdf is not None:
            bsdf.inputs["Base Color"].default_value = (*rgb, 1.0)
            bsdf.inputs["Metallic"].default_value = metallic
            bsdf.inputs["Roughness"].default_value = roughness
            bsdf.inputs["Alpha"].default_value = alpha
        mat.diffuse_color = (*rgb, alpha)
        mat.metallic = metallic
        mat.roughness = roughness


def build_objects(weapon, collection):
    objects = []
    for part in weapon.parts:
        objects.append(part.to_object(collection))
    for name, point in weapon.points.items():
        empty = bpy.data.objects.new(name, None)
        empty.empty_display_type = "PLAIN_AXES"
        empty.empty_display_size = 0.02
        empty.location = to_blender(point)
        collection.objects.link(empty)
        objects.append(empty)
    return objects


def export_weapon(weapon, out_dir):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    create_materials()
    build_objects(weapon, bpy.context.scene.collection)
    path = os.path.join(out_dir, f"{weapon.name}.glb")
    bpy.ops.export_scene.gltf(
        filepath=path,
        export_format="GLB",
        export_extras=True,
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


def render_preview(names, path):
    """Each weapon twice, side by side: from its right side, and turned to
    show the left side from slightly above."""
    bpy.ops.wm.read_factory_settings(use_empty=True)
    create_materials()
    scene = bpy.context.scene
    for view in range(2):
        # Meshes are consumed when turned into objects, so build afresh.
        weapons = [w for w in all_weapons() if w.name in names]
        for index, weapon in enumerate(weapons):
            # Screen right is game -Z (camera on +X), screen up is game +Y.
            offset = to_blender((0.0, -index * 0.45, -view * 1.25))
            pivot = bpy.data.objects.new(f"pivot_{weapon.name}_{view}", None)
            scene.collection.objects.link(pivot)
            pivot.location = offset
            for obj in build_objects(weapon, scene.collection):
                obj.parent = pivot
            if view == 1:
                pivot.rotation_euler = (0.35, 0.0, 3.0)

    cam_data = bpy.data.cameras.new("cam")
    cam_data.type = "ORTHO"
    cam_data.ortho_scale = 2.6
    cam = bpy.data.objects.new("cam", cam_data)
    scene.collection.objects.link(cam)
    centre = to_blender((0.0, -0.225 * (len(names) - 1), -0.62))
    cam.location = centre + to_blender((3.0, 0.0, 0.0))
    direction = centre - cam.location
    cam.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()
    scene.camera = cam

    scene.render.engine = "BLENDER_WORKBENCH"
    scene.display.shading.light = "STUDIO"
    scene.display.shading.color_type = "MATERIAL"
    scene.display.shading.show_shadows = False
    scene.display.shading.show_cavity = True
    scene.render.resolution_x, scene.render.resolution_y = 1600, 900
    scene.render.filepath = path
    bpy.ops.render.render(write_still=True)
    print(f"preview written to {path}")


def main():
    args = parse_args()
    weapons = all_weapons()
    if args.only:
        weapons = [w for w in weapons if w.name in args.only]
    if args.preview:
        render_preview([w.name for w in weapons], os.path.abspath(args.preview))
        return
    out_dir = os.path.abspath(args.out)
    os.makedirs(out_dir, exist_ok=True)
    for weapon in weapons:
        export_weapon(weapon, out_dir)
        print(f"exported {weapon.name}")
    print(f"{len(weapons)} weapons -> {out_dir}")


main()
