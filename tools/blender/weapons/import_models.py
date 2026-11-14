"""Turn the staged CC0 gun models into HARROW weapon models (realism pass).

Usually run through tools/scripts/import_weapons.sh. Directly:

    blender --background --factory-startup --disable-autoexec \
        --python tools/blender/weapons/import_models.py -- \
        --staging /path/to/harrow-assets-staging [--out assets/models/weapons] \
        [--only tamarisk] [--preview DIR]

The source models are not in the repo (see ASSETS.md for their pages and
licences); this script records exactly what was done to them so the .glb
files can be rebuilt. The output follows the same conventions as the
scripted blockouts (tools/blender/weapons/models.py), so the game treats
both alike:

- metres; the barrel points game -Z, +Y up, +X to the shooter's right;
- the origin is where the web of the firing hand sits: the top-back of the
  pistol grip (the third-person rig's `weapon_r` bone uses the same point);
- `MAGAZINE` (and `SLIDE` on the sidearm) are separate nodes whose origin
  is their pivot; `SIGHT` and `MUZZLE` are empties on the sight line (the
  front sight tip, level with the rear sight) and the barrel tip;
- extra empties for the first-person hands: `RACK` (charging handle or
  slide serrations, where the support hand pulls on an empty reload) and
  `MAG_GRIP` (child of `MAGAZINE`, where the support hand holds it).

Textures stay those of the source (PBR: base colour, metal/roughness,
normal, occlusion), re-encoded as JPEG to keep the files small.

Untrusted input: the source files are only imported (glTF has no scripts),
and Blender runs with --disable-autoexec.
"""

import argparse
import math
import os
import sys

import bmesh
import bpy
from mathutils import Matrix, Vector


def parse_args():
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    parser = argparse.ArgumentParser()
    parser.add_argument("--staging", default=os.environ.get("HARROW_ASSET_STAGING", ""))
    parser.add_argument("--out", default="assets/models/weapons")
    parser.add_argument("--only", nargs="*", default=None)
    parser.add_argument("--preview", default=None)
    return parser.parse_args(argv)


# ---------------------------------------------------------------- helpers


def import_glb(path):
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=path)
    return [o for o in bpy.data.objects if o not in before]


def world_verts(obj):
    mw = obj.matrix_world
    return [mw @ v.co for v in obj.data.vertices]


def bounds(points):
    lo = Vector((min(p.x for p in points), min(p.y for p in points), min(p.z for p in points)))
    hi = Vector((max(p.x for p in points), max(p.y for p in points), max(p.z for p in points)))
    return lo, hi


def loose_parts(obj):
    """Connected vertex groups of a mesh: [(vertex indices, lo, hi)]."""
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    bm.verts.ensure_lookup_table()
    mw = obj.matrix_world
    seen, parts = set(), []
    for v in bm.verts:
        if v.index in seen:
            continue
        stack, comp = [v], []
        seen.add(v.index)
        while stack:
            x = stack.pop()
            comp.append(x.index)
            for e in x.link_edges:
                o = e.other_vert(x)
                if o.index not in seen:
                    seen.add(o.index)
                    stack.append(o)
        lo, hi = bounds([mw @ bm.verts[i].co for i in comp])
        parts.append((comp, lo, hi))
    bm.free()
    return parts


def split_vertices(obj, indices, name):
    """Move the vertices `indices` (and their faces) into a new object."""
    keep = set(indices)
    new = obj.copy()
    new.data = obj.data.copy()
    new.name = new.data.name = name
    for coll in obj.users_collection:
        coll.objects.link(new)
    for target, drop in ((new, lambda i: i not in keep), (obj, lambda i: i in keep)):
        bm = bmesh.new()
        bm.from_mesh(target.data)
        bm.verts.ensure_lookup_table()
        bmesh.ops.delete(bm, geom=[v for v in bm.verts if drop(v.index)], context="VERTS")
        bm.to_mesh(target.data)
        bm.free()
        target.data.update()
    return new


def select_only(objects):
    bpy.ops.object.select_all(action="DESELECT")
    for o in objects:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objects[0]


def join(objects, name):
    select_only(objects)
    bpy.ops.object.join()
    obj = bpy.context.view_layer.objects.active
    obj.name = obj.data.name = name
    return obj


def apply_transform(objects, matrix):
    """Bake `matrix` (world space) into the meshes; objects end at identity."""
    for o in objects:
        o.matrix_world = matrix @ o.matrix_world
    select_only(objects)
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)


def set_origin(obj, point):
    """Move `obj`'s origin to world `point` without moving its geometry."""
    offset = point - obj.matrix_world.translation
    obj.data.transform(Matrix.Translation(-offset))  # unrotated, unscaled objects
    obj.matrix_world = Matrix.Translation(offset) @ obj.matrix_world


def empty(name, location, parent=None):
    e = bpy.data.objects.new(name, None)
    e.empty_display_type = "PLAIN_AXES"
    e.empty_display_size = 0.02
    bpy.context.scene.collection.objects.link(e)
    if parent is not None:
        e.parent = parent
        e.matrix_parent_inverse = Matrix.Identity(4)
        e.location = parent.matrix_world.inverted() @ location
    else:
        e.location = location
    return e


def region(points, axis, lo, hi):
    a = "xyz".index(axis)
    return [p for p in points if lo <= p[a] <= hi]


def scrub(images, box, width=48):
    """Blur away stamped markings: a wide horizontal box blur over `box`
    (x0, y0, x1, y1 in pixels from the top-left), which keeps the look of
    horizontally brushed metal. Applied to colour and normal maps alike."""
    import numpy as np

    for image in images:
        w, h = image.size
        px = np.array(image.pixels[:], dtype=np.float32).reshape(h, w, 4)
        x0, y0, x1, y1 = box
        rows = slice(h - y1, h - y0)  # Blender images start at the bottom
        band = px[rows, max(0, x0 - width):min(w, x1 + width)].copy()
        kernel = np.ones(width, dtype=np.float32) / width
        for c in range(3):
            for r in range(band.shape[0]):
                band[r, :, c] = np.convolve(band[r, :, c], kernel, mode="same")
        px[rows, x0:x1, :3] = band[:, (x0 - max(0, x0 - width)):(x0 - max(0, x0 - width)) + (x1 - x0), :3]
        image.pixels[:] = px.ravel()
        image.pack()
        print(f"scrubbed {image.name} {box}")


def material_images(objects):
    out = {}
    for o in objects:
        for slot in o.material_slots:
            if slot.material and slot.material.node_tree:
                for node in slot.material.node_tree.nodes:
                    if node.type == "TEX_IMAGE" and node.image:
                        out[node.image.name] = node.image
    return out


def fmt(v):
    return "(" + ", ".join(f"{c:+.4f}" for c in v) + ")"


# In Blender space (Z up) the converted sources and our output both point
# the barrel along +Y; the glTF export turns that into game -Z.


# ------------------------------------------------------------- TAMARISK


AKM_SCALE = 0.905  # the source is 0.972 m long; a real AKM is 0.88 m


def tamarisk(staging):
    """AKM by LonesomeDucky (OpenGameArt, CC0): weathered steel, wood and a
    bakelite magazine. One mesh, so the magazine is split off here."""
    src = os.path.join(staging, "firearms/oga_akm_lonesomeducky/oga_akm_lonesomeducky.glb")
    objs = [o for o in import_glb(src) if o.type == "MESH"]
    body = join(objs, "tamarisk") if len(objs) > 1 else objs[0]
    body.name = body.data.name = "tamarisk"
    apply_transform([body], Matrix.Scale(AKM_SCALE, 4))
    pts = world_verts(body)

    # The magazine: loose parts below the receiver and ahead of the trigger
    # guard (the source's magazine sits at y -0.07..0.09, z 0..0.2).
    s = AKM_SCALE
    mag_parts = []
    for comp, lo, hi in loose_parts(body):
        if lo.y > -0.08 * s and hi.y < 0.095 * s and hi.z < 0.205 * s and lo.z < 0.17 * s:
            mag_parts.append(comp)
            print(f"tamarisk: magazine part {len(comp)} verts {fmt(lo)}..{fmt(hi)}")
    mag = split_vertices(body, [i for c in mag_parts for i in c], "MAGAZINE")
    mag_pts = world_verts(mag)
    mlo, mhi = bounds(mag_pts)
    top = [p for p in mag_pts if p.z > mhi.z - 0.012]
    tlo, thi = bounds(top)
    mag_top = Vector(((tlo.x + thi.x) / 2, (tlo.y + thi.y) / 2, thi.z))
    set_origin(mag, mag_top)

    pts = world_verts(body)
    centre_x = sum(p.x for p in region(pts, "y", 0.44 * s, 0.49 * s)) / max(1, len(region(pts, "y", 0.44 * s, 0.49 * s)))
    # Pistol grip: the loose parts behind the magazine, below the receiver.
    grip = [p for comp, lo, hi in loose_parts(body)
            if hi.y < -0.15 * s and lo.y > -0.24 * s and lo.z < 0.1 * s
            for p in (body.matrix_world @ body.data.vertices[i].co for i in comp)]
    glo, ghi = bounds(grip)
    near_top = [p for p in grip if p.z > ghi.z - 0.012]
    grip_point = Vector((centre_x, min(p.y for p in near_top) + 0.006, ghi.z - 0.004))

    # Sight line: front sight post tip and the rear sight leaf.
    front = region(pts, "y", 0.425 * s, 0.445 * s)
    front_tip = max(front, key=lambda p: p.z)
    rear = region(pts, "y", 0.015 * s, 0.045 * s)
    rear_top = max(p.z for p in rear if abs(p.x - centre_x) < 0.012)
    muzzle_pts = region(pts, "y", 0.475 * s, 0.49 * s)
    mlo2, mhi2 = bounds(muzzle_pts)
    muzzle = Vector((centre_x, max(p.y for p in pts), (mlo2.z + mhi2.z) / 2))
    sight = Vector((centre_x, front_tip.y, min(front_tip.z, rear_top)))
    # Charging handle: the knob sticking out on the right of the receiver.
    right = [p for p in pts if p.x > centre_x + 0.02 and p.z > 0.2 * s and p.y > -0.05 * s and p.y < 0.1 * s]
    rlo, rhi = bounds(right) if right else (Vector(), Vector())
    rack = Vector((rhi.x, (rlo.y + rhi.y) / 2, (rlo.z + rhi.z) / 2))
    print(f"tamarisk: centre x {centre_x:+.4f}, grip {fmt(grip_point)}, front post {fmt(front_tip)}, "
          f"rear sight top z {rear_top:+.4f}, muzzle {fmt(muzzle)}, rack {fmt(rack)}, mag {fmt(mlo)}..{fmt(mhi)}")

    # Origin at the grip.
    shift = Matrix.Translation(-grip_point)
    for o in (body, mag):
        o.matrix_world = shift @ o.matrix_world
    set_origin(body, Vector())
    mag.parent = body
    mag.matrix_parent_inverse = body.matrix_world.inverted()
    empty("SIGHT", shift @ sight)
    empty("MUZZLE", shift @ muzzle)
    empty("RACK", shift @ rack)
    # Support hand on the magazine: on its front face, a third of the way down.
    grip_mag = Vector((centre_x, mag_top.y + 0.035, mag_top.z - 0.07))
    empty("MAG_GRIP", shift @ grip_mag, parent=mag)
    return body


# ---------------------------------------------------------------- ACACIA


def acacia(staging):
    """Service Pistol by Mateusz Sadek (Poly Haven, CC0): a worn P38-style
    pistol. We keep the wood-grip variant (`_a`) and the loaded magazine,
    and drop the tape-grip variant, the empty magazine and the loose round."""
    src = os.path.join(staging, "firearms/service_pistol/service_pistol.glb")
    objs = {o.name: o for o in import_glb(src)}
    for name in list(objs):
        if name.endswith("_b") or name.endswith("magazine_empty") or name.endswith("bullet"):
            bpy.data.objects.remove(objs.pop(name), do_unlink=True)
    meshes = {n: o for n, o in objs.items() if o.type == "MESH"}
    for n, o in objs.items():
        if o.type != "MESH":
            bpy.data.objects.remove(o, do_unlink=True)
    # Muzzle points +X in the source; turn it to +Y.
    turn = Matrix.Rotation(math.radians(90.0), 4, "Z")
    apply_transform(list(meshes.values()), turn)
    frame_obj = meshes["service_pistol_pistol_a"]
    slide = meshes["service_pistol_slide_a"]
    mag = meshes["service_pistol_magazine_loaded"]
    body = join([frame_obj, meshes["service_pistol_hammer_a"], meshes["service_pistol_trigger_a"]], "acacia")
    slide.name = slide.data.name = "SLIDE"
    mag.name = mag.data.name = "MAGAZINE"

    fpts = world_verts(body)
    spts = world_verts(slide)
    flo, fhi = bounds(fpts)
    slo, shi = bounds(spts)
    centre_x = (slo.x + shi.x) / 2
    # The grip: frame vertices behind the trigger guard and below the slide.
    grip = [p for p in fpts if p.z < slo.z - 0.005 and p.y < flo.y + 0.06]
    glo, ghi = bounds(grip)
    # Grip footprint at the bottom, and the magazine's.
    gbot = [p for p in grip if p.z < glo.z + 0.006]
    mpts = world_verts(mag)
    mlo, mhi = bounds(mpts)
    mbot = [p for p in mpts if p.z < mlo.z + 0.006]
    gb_lo, gb_hi = bounds(gbot)
    mb_lo, mb_hi = bounds(mbot)
    # Seat the magazine: its base plate flush with the bottom of the grip,
    # centred in the grip's footprint (front-to-back) and on the centre line.
    dy = (gb_lo.y + gb_hi.y) / 2 - (mb_lo.y + mb_hi.y) / 2
    dz = glo.z - mlo.z
    dx = centre_x - (mlo.x + mhi.x) / 2
    mag.matrix_world = Matrix.Translation((dx, dy, dz)) @ mag.matrix_world
    apply_transform([mag], Matrix.Identity(4))
    mpts = world_verts(mag)
    mlo, mhi = bounds(mpts)
    print(f"acacia: frame {fmt(flo)}..{fmt(fhi)}, slide {fmt(slo)}..{fmt(shi)}, grip {fmt(glo)}..{fmt(ghi)}, "
          f"magazine moved by {fmt((dx, dy, dz))} -> {fmt(mlo)}..{fmt(mhi)}")

    # Origin: top-back of the grip, under the hammer (web of the hand).
    back = [p for p in grip if p.z > ghi.z - 0.01]
    grip_point = Vector((centre_x, min(p.y for p in back) + 0.004, slo.z - 0.002))
    # Sights, on the centre line: the rear notch at the back of the slide
    # and the front blade near the muzzle (the barrel sticks out of this
    # slide, P38-style, and carries the front sight).
    allp = fpts + spts
    on_line = [p for p in allp if abs(p.x - centre_x) < 0.0025]
    rear = [p for p in on_line if p.y < slo.y + 0.03]
    front = [p for p in on_line if p.y > fhi.y - 0.03]
    rear_top = max(p.z for p in rear)
    front_tip = max(front, key=lambda p: p.z)
    sight = Vector((centre_x, front_tip.y, front_tip.z))
    # Muzzle: centre of the barrel's front face.
    nose = [p for p in fpts if p.y > fhi.y - 0.003]
    nlo, nhi = bounds(nose)
    muzzle = Vector(((nlo.x + nhi.x) / 2, fhi.y, (nlo.z + nhi.z) / 2))
    rack = Vector((centre_x, slo.y + 0.025, shi.z - 0.006))
    print(f"acacia: grip {fmt(grip_point)}, rear notch z {rear_top:+.4f}, front blade {fmt(front_tip)}, muzzle {fmt(muzzle)}")

    # The frame's serial number line carries a small proof-style stamp;
    # blur it out (colour and normal map).
    images = material_images([body])
    scrub([i for n, i in images.items() if "diff" in n or "nor" in n], (1300, 1996, 1482, 2036))
    set_origin(mag, Vector(((mlo.x + mhi.x) / 2, (mlo.y + mhi.y) / 2, mhi.z)))
    set_origin(slide, Vector((centre_x, (slo.y + shi.y) / 2, (slo.z + shi.z) / 2)))
    shift = Matrix.Translation(-grip_point)
    for o in (body, slide, mag):
        o.matrix_world = shift @ o.matrix_world
    set_origin(body, Vector())
    for o in (slide, mag):
        o.parent = body
        o.matrix_parent_inverse = body.matrix_world.inverted()
    empty("SIGHT", shift @ sight)
    empty("MUZZLE", shift @ muzzle)
    empty("RACK", shift @ rack)
    grip_mag = Vector((centre_x, mlo.y + 0.01, mlo.z + 0.01))
    empty("MAG_GRIP", shift @ grip_mag, parent=mag)
    return body


MODELS = {"tamarisk": tamarisk, "acacia": acacia}


# ------------------------------------------------------------------ main


def export(name, out_dir):
    path = os.path.join(out_dir, f"{name}.glb")
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
        export_image_format="JPEG",
        export_image_quality=90,
        check_existing=False,
    )
    return path


def preview(name, out_dir):
    """Side and three-quarter renders (Workbench, textured)."""
    scene = bpy.context.scene
    cam_data = bpy.data.cameras.new("cam")
    cam_data.type = "ORTHO"
    objs = [o for o in scene.objects if o.type == "MESH"]
    pts = [p for o in objs for p in world_verts(o)]
    lo, hi = bounds(pts)
    size = max(hi - lo)
    cam_data.ortho_scale = size * 1.15
    cam = bpy.data.objects.new("cam", cam_data)
    scene.collection.objects.link(cam)
    scene.camera = cam
    centre = (lo + hi) / 2
    scene.render.engine = "BLENDER_WORKBENCH"
    scene.display.shading.light = "STUDIO"
    scene.display.shading.color_type = "TEXTURE"
    scene.display.shading.show_cavity = True
    scene.render.resolution_x, scene.render.resolution_y = 1600, 900
    for view, offset in (("right", Vector((1, 0, 0))), ("left_top", Vector((-0.8, 0.25, 0.55)))):
        cam.location = centre + offset.normalized() * 2.0
        cam.rotation_euler = (centre - cam.location).to_track_quat("-Z", "Y").to_euler()
        scene.render.filepath = os.path.join(out_dir, f"{name}_{view}.png")
        bpy.ops.render.render(write_still=True)
    print(f"{name}: previews in {out_dir}")


def main():
    args = parse_args()
    if not args.staging or not os.path.isdir(args.staging):
        sys.exit("--staging must point at the asset staging folder (or set HARROW_ASSET_STAGING)")
    names = args.only or list(MODELS)
    out_dir = os.path.abspath(args.out)
    os.makedirs(out_dir, exist_ok=True)
    for name in names:
        bpy.ops.wm.read_factory_settings(use_empty=True)
        MODELS[name](args.staging)
        if args.preview:
            os.makedirs(args.preview, exist_ok=True)
            preview(name, os.path.abspath(args.preview))
        else:
            print(f"exported {export(name, out_dir)}")


main()
