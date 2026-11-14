"""Export to .glb, triangle stats and preview renders."""

import math
import os

import bpy
from mathutils import Vector


def triangles(obj):
    return sum(len(p.vertices) - 2 for p in obj.data.polygons)


def stats(ch):
    per = {o.name: triangles(o) for o in ch.meshes}
    total = sum(per.values())
    mats = sorted({s.material.name for o in ch.meshes for s in o.material_slots if s.material})
    return (
        f"{total} triangles {per}, {len(ch.rig.data.bones)} bones, "
        f"materials {mats}, clips {[a.name for a in ch.actions]}"
    )


def _select_only(objs):
    for obj in bpy.context.view_layer.objects:
        obj.select_set(False)
    for obj in objs:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = objs[0]


def export_glb(ch, out_dir):
    path = os.path.join(out_dir, f"{ch.cfg['id']}.glb")
    if ch.rig.animation_data is None:
        ch.rig.animation_data_create()
    ch.rig.animation_data.action = None
    _select_only([ch.rig, *ch.meshes])
    bpy.ops.export_scene.gltf(
        filepath=path,
        check_existing=False,
        export_format="GLB",
        use_selection=True,
        export_yup=True,
        export_apply=False,
        export_extras=True,
        export_texcoords=True,
        export_normals=True,
        export_tangents=True,
        export_materials="EXPORT",
        export_image_format="AUTO",
        export_vertex_color="ACTIVE",
        export_all_vertex_colors=False,
        export_skins=True,
        export_influence_nb=4,
        export_def_bones=False,
        export_leaf_bone=False,
        export_morph=False,
        export_cameras=False,
        export_lights=False,
        export_animations=bool(ch.actions),
        export_animation_mode="ACTIONS",
        export_force_sampling=True,
        export_frame_range=False,
        export_anim_slide_to_zero=True,
        export_optimize_animation_size=False,
        export_anim_single_armature=True,
        export_reset_pose_bones=True,
        export_rest_position_armature=True,
    )
    return path


# ---------------------------------------------------------------- previews


def _setup_render_scene(scene, resolution):
    scene.render.engine = "BLENDER_EEVEE"
    scene.render.resolution_x, scene.render.resolution_y = resolution
    scene.render.film_transparent = False
    scene.view_settings.view_transform = "AgX"
    world = bpy.data.worlds.new("preview") if scene.world is None else scene.world
    scene.world = world
    world.use_nodes = True
    bg = world.node_tree.nodes.get("Background")
    bg.inputs["Color"].default_value = (0.55, 0.50, 0.42, 1.0)
    bg.inputs["Strength"].default_value = 0.9
    if "preview_sun" not in bpy.data.objects:
        sun_data = bpy.data.lights.new("preview_sun", "SUN")
        sun_data.energy = 4.0
        sun = bpy.data.objects.new("preview_sun", sun_data)
        scene.collection.objects.link(sun)
        sun.rotation_euler = (math.radians(50), 0.0, math.radians(30))
        ground = bpy.data.meshes.new("preview_ground")
        ground.from_pydata([(-5, -5, 0), (5, -5, 0), (5, 5, 0), (-5, 5, 0)], [], [(0, 1, 2, 3)])
        gmat = bpy.data.materials.new("preview_ground")
        gmat.diffuse_color = (0.6, 0.5, 0.38, 1.0)
        gmat.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.45, 0.36, 0.25, 1.0)
        ground.materials.append(gmat)
        gobj = bpy.data.objects.new("preview_ground", ground)
        scene.collection.objects.link(gobj)


def _camera(scene, location, target, lens=50):
    if "preview_cam" in bpy.data.objects:
        cam = bpy.data.objects["preview_cam"]
    else:
        cam = bpy.data.objects.new("preview_cam", bpy.data.cameras.new("preview_cam"))
        scene.collection.objects.link(cam)
    cam.data.lens = lens
    cam.location = location
    cam.rotation_euler = (Vector(target) - Vector(location)).to_track_quat("-Z", "Y").to_euler()
    scene.camera = cam


def render(path, views=None, frame=None, action=None, ch=None, resolution=(700, 1000)):
    scene = bpy.context.scene
    _setup_render_scene(scene, resolution)
    if ch is not None:
        ch.rig.animation_data_create()
        ch.rig.animation_data.action = action
        if action is not None:
            scene.frame_set(int(frame if frame is not None else action.frame_range[0]))
        else:
            for pb in ch.rig.pose.bones:
                pb.matrix_basis.identity()
            scene.frame_set(0)
    for name, (loc, target, lens) in (views or {}).items():
        _camera(scene, loc, target, lens)
        scene.render.filepath = path.replace("{view}", name)
        bpy.ops.render.render(write_still=True)
        print(f"preview written to {scene.render.filepath}")


def render_previews(ch, out_dir):
    os.makedirs(out_dir, exist_ok=True)
    op = ch.cfg["id"]
    h = 1.0
    views = {
        "front": ((0.0, 4.2, h), (0.0, 0.0, 0.92), 50),
        "side": ((4.2, 0.6, h), (0.0, 0.0, 0.92), 50),
        "back": ((-1.6, -3.9, h + 0.3), (0.0, 0.0, 0.92), 50),
        "face": ((0.35, 1.1, 1.62), (0.0, 0.0, 1.62), 70),
    }
    render(os.path.join(out_dir, f"{op}_{{view}}.png"), views, ch=ch)
    for action in ch.actions:
        if action.name in ("idle", "walk", "crouch_walk", "prone_crawl", "reload", "lean_left", "fire"):
            mid = (action.frame_range[0] + action.frame_range[1]) / 2
            render(
                os.path.join(out_dir, f"{op}_{action.name}_{{view}}.png"),
                {"q": ((2.6, 3.0, 1.4), (0.0, 0.0, 0.7), 40)},
                frame=mid,
                action=action,
                ch=ch,
                resolution=(600, 700),
            )
