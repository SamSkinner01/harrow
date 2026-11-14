"""One operator, end to end: MPFB body -> game space -> clothing -> gear ->
animations. Returns a `Character` ready for export."""

import bmesh
import bpy

from . import anim, materials, mpfb
from . import body as body_mod
from . import gear as gear_mod
from .palette import palette_for

# Target triangle count for the body (MakeHuman's base mesh is ~27k
# triangles once the helpers are gone). The head and hands keep more of
# their detail. A ~10k version would make a good second LOD.
BODY_TRIANGLES = 20000


class Character:
    def __init__(self, cfg):
        self.cfg = cfg
        self.rig = None
        self.body = None
        self.eyes = None
        self.brows = None
        self.gear = None
        self.prop = None
        self.actions = []
        self.marks = None

    @property
    def meshes(self):
        return [o for o in (self.body, self.eyes, self.brows, self.gear, self.prop) if o is not None]


def _protect(bone):
    if bone in ("head", "neck_01"):
        return 1.0
    if body_mod.is_hand(bone):
        return 0.6
    return 0.0


def _join_brows(eyebrows, eyelashes, material):
    """Eyebrows and eyelashes share one atlas (brows left, lashes right):
    remap their UVs and join them into one mesh."""
    for obj, offset in ((eyebrows, 0.0), (eyelashes, 0.5)):
        for layer in obj.data.uv_layers:
            for loop in layer.data:
                loop.uv = (loop.uv[0] * 0.5 + offset, loop.uv[1])
        obj.data.materials.clear()
        obj.data.materials.append(material)
    # Same UV layer name, so the join merges them.
    eyelashes.data.uv_layers[0].name = eyebrows.data.uv_layers[0].name
    with bpy.context.temp_override(active_object=eyebrows, selected_editable_objects=[eyebrows, eyelashes]):
        bpy.ops.object.join()
    eyebrows.name = "brows"
    return eyebrows


def build(cfg, scratch, mocap=None):
    ch = Character(cfg)
    rig, body, eyes, brows, lashes = mpfb.build_human(cfg)
    mpfb.stage_for_game(rig, [body, eyes, brows, lashes], cfg["body"]["height_m"])
    rig.name = rig.data.name = "rig"
    body.name, eyes.name = "body", "eyes"
    ch.rig, ch.body, ch.eyes = rig, body, eyes
    ch.marks = body_mod.Landmarks(rig, eyes)

    mats = materials.build(cfg, scratch)
    triangles = sum(len(p.vertices) - 2 for p in body.data.polygons)
    body_mod.decimate(body, min(1.0, BODY_TRIANGLES / triangles), _protect)
    colours = palette_for(cfg)
    body_mod.dress(body, ch.marks, cfg, colours, mats)
    eyes.data.materials.clear()
    eyes.data.materials.append(mats["eyes"])
    ch.brows = _join_brows(brows, lashes, mats["brows"])
    ch.materials = mats
    ch.colours = colours
    surfaces = gear_mod.Surfaces(body)
    ch.gear = gear_mod.build(ch, surfaces)
    anim.build(ch, gear_mod, surfaces, mocap)
    for obj in ch.meshes:
        triangulate(obj)
    return ch


def triangulate(obj):
    """Triangles only, split the same way on every build (reproducible files)."""
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    bmesh.ops.triangulate(bm, faces=bm.faces[:], quad_method="BEAUTY", ngon_method="BEAUTY")
    bm.to_mesh(obj.data)
    bm.free()
    obj.data.update()
