"""MPFB (MakeHuman for Blender) access.

MPFB 2.0.18 is installed per user as a Blender extension (see
`tools/scripts/build_characters.sh --setup`). We drive it through its own
Python service API, headless:

- `HumanService.deserialize_from_dict` builds the base mesh from macro
  sliders and targets (MPFB's preset format), fits the `game_engine` skeleton
  and loads its weights, and adds eyes and eyebrows (fitted and weighted).
- `TargetService.bake_targets` freezes the shape keys.

After that the human is plain Blender data: we strip helper geometry, turn
it to face the game's forward direction and scale it to the configured
height (see `stage_for_game`).
"""

import importlib
import math

import addon_utils
import bmesh
import bpy
from mathutils import Matrix

# Where Blender installs extensions from the user repository. Other repos
# (e.g. a manual install into "blender_org") are found by suffix.
PREFERRED_MODULE = "bl_ext.user_default.mpfb"

RIG = "game_engine"
EYES = "low-poly"
EYELASHES = "eyelashes01"


class MpfbMissing(Exception):
    pass


def _find_module():
    names = [m.__name__ for m in addon_utils.modules()]
    if PREFERRED_MODULE in names:
        return PREFERRED_MODULE
    for name in names:
        if name.endswith(".mpfb"):
            return name
    raise MpfbMissing(
        "MPFB is not installed for this Blender. Run tools/scripts/build_characters.sh --setup"
    )


_SERVICES = None


def services():
    """Enable MPFB if needed and return its service classes."""
    global _SERVICES
    module = _find_module()
    if not addon_utils.check(module)[1]:
        # default_set registers it in the preferences, where MPFB reads its
        # settings. With --factory-startup nothing is saved to disk.
        addon_utils.enable(module, default_set=True)
    if _SERVICES is not None:
        return _SERVICES
    svc = lambda name: importlib.import_module(f"{module}.services.{name.lower()}")  # noqa: E731

    class S:
        HumanService = svc("HumanService").HumanService
        TargetService = svc("TargetService").TargetService
        AssetService = svc("AssetService").AssetService

    if not S.AssetService.system_assets_pack_is_installed():
        raise MpfbMissing(
            "MakeHuman system assets (CC0) are not installed. Run "
            "tools/scripts/build_characters.sh --setup"
        )
    _SERVICES = S
    return S


def check_target(name):
    """Raise if an MPFB target name doesn't resolve to a file."""
    path = services().TargetService.target_full_path(name)
    base = (path or "").replace("\\", "/").split("/")[-1]
    if base not in (f"{name}.target.gz", f"{name}.target"):
        raise ValueError(f"unknown MPFB target {name!r}")


def build_human(cfg):
    """Build the rigged human for an operator.
    Returns (rig, body, eyes, eyebrows, eyelashes)."""
    S = services()
    body_cfg, face = cfg["body"], cfg["face"]
    for name in face["targets"]:
        check_target(name)
    phenotype = {k: float(body_cfg[k]) for k in (
        "gender", "age", "muscle", "weight", "proportions", "height", "cupsize", "firmness")}
    race = {k: float(v) for k, v in body_cfg["race"].items()}
    total = sum(race.values()) or 1.0
    phenotype["race"] = {k: v / total for k, v in race.items()}

    # MPFB's human preset format (what "save human" writes), built in code.
    info = {
        "name": cfg["id"],
        "phenotype": phenotype,
        "rig": RIG,
        "eyes": f"{EYES}/{EYES}.mhclo",
        "eyebrows": f"{face['eyebrows']}/{face['eyebrows']}.mhclo",
        "eyelashes": f"{EYELASHES}/{EYELASHES}.mhclo",
        "tongue": "",
        "teeth": "",
        "hair": "",
        "proxy": "",
        "targets": [{"target": k, "value": float(v)} for k, v in face["targets"].items()],
        "clothes": [],
        "skin_mhmat": "",
        "skin_material_type": "NONE",
        "eyes_material_type": "NONE",
        "skin_material_settings": {},
        "eyes_material_settings": {},
        "expressions": [],
        "alternative_materials": {},
    }
    settings = S.HumanService.get_default_deserialization_settings()
    settings["subdiv_levels"] = 0
    body = S.HumanService.deserialize_from_dict(info, settings)
    rig = body.parent
    if rig is None or rig.type != "ARMATURE":
        raise RuntimeError("MPFB did not create a rig")
    S.TargetService.bake_targets(body)

    parts = {}
    for obj in rig.children:
        if obj is body or obj.type != "MESH":
            continue
        for key, suffix in (("eyes", EYES), ("eyebrows", face["eyebrows"]), ("eyelashes", EYELASHES)):
            if obj.name.endswith(suffix):
                parts[key] = obj
    if len(parts) != 3:
        raise RuntimeError(f"MPFB eyes/eyebrows/eyelashes missing: {[o.name for o in rig.children]}")
    _remove_helpers(body)
    for obj in (body, *parts.values()):
        for mod in list(obj.modifiers):
            if mod.type != "ARMATURE":
                obj.modifiers.remove(mod)
    return rig, body, parts["eyes"], parts["eyebrows"], parts["eyelashes"]


def _remove_helpers(body):
    """Delete MakeHuman helper geometry (vertices outside the "body" group)
    and every vertex group that isn't a bone."""
    group = body.vertex_groups["body"].index
    bm = bmesh.new()
    bm.from_mesh(body.data)
    deform = bm.verts.layers.deform.verify()
    doomed = [v for v in bm.verts if v[deform].get(group, 0.0) < 0.5]
    bmesh.ops.delete(bm, geom=doomed, context="VERTS")
    bm.to_mesh(body.data)
    bm.free()
    bones = set(body.parent.data.bones.keys())
    for vg in list(body.vertex_groups):
        if vg.name not in bones:
            body.vertex_groups.remove(vg)


def stage_for_game(rig, meshes, height_m):
    """Turn the character to face Blender +Y (game -Z after glTF export) and
    scale it so the top of the head is at `height_m`. Bakes the transform
    into bone rest poses and mesh data, leaving every object at identity."""
    body = meshes[0]
    top = max(v.co.z for v in body.data.vertices)
    scale = height_m / top
    transform = Matrix.Rotation(math.pi, 4, "Z") @ Matrix.Scale(scale, 4)

    bpy.context.view_layer.objects.active = rig
    bpy.ops.object.mode_set(mode="EDIT")
    for bone in rig.data.edit_bones:
        bone.transform(transform, scale=True, roll=True)
    bpy.ops.object.mode_set(mode="OBJECT")
    for obj in meshes:
        obj.data.transform(transform)
        obj.data.update()
    for obj in [rig, *meshes]:
        obj.matrix_world = Matrix.Identity(4)
        if obj is not rig:
            obj.matrix_parent_inverse = Matrix.Identity(4)
    return scale
