"""One operator, end to end: MPFB body -> game space -> clothing -> gear ->
animations. Returns a `Character` ready for export."""

from . import body as body_mod
from . import gear as gear_mod
from . import anim, materials, mpfb
from .palette import palette_for

# Target triangle count for the body before clothing (MakeHuman's base mesh
# is ~27k triangles). The head and hands keep more of their detail.
BODY_TRIANGLES = 10000


class Character:
    def __init__(self, cfg):
        self.cfg = cfg
        self.rig = None
        self.body = None
        self.eyes = None
        self.eyebrows = None
        self.gear = None
        self.prop = None
        self.actions = []
        self.marks = None

    @property
    def meshes(self):
        return [o for o in (self.body, self.eyes, self.eyebrows, self.gear, self.prop) if o is not None]


def _protect(bone):
    if bone in ("head", "neck_01"):
        return 1.0
    if body_mod.is_hand(bone):
        return 0.6
    return 0.0


def build(cfg, scratch):
    ch = Character(cfg)
    rig, body, eyes, brows = mpfb.build_human(cfg)
    mpfb.stage_for_game(rig, [body, eyes, brows], cfg["body"]["height_m"])
    rig.name = rig.data.name = "rig"
    body.name, eyes.name, brows.name = "body", "eyes", "eyebrows"
    ch.rig, ch.body, ch.eyes, ch.eyebrows = rig, body, eyes, brows
    ch.marks = body_mod.Landmarks(rig, eyes)

    mats = materials.build(cfg, scratch)
    triangles = sum(len(p.vertices) - 2 for p in body.data.polygons)
    body_mod.decimate(body, min(1.0, BODY_TRIANGLES / triangles), _protect)
    colours = palette_for(cfg)
    body_mod.dress(body, ch.marks, cfg, colours, mats)
    for obj, name in ((eyes, "eyes"), (brows, "eyebrows")):
        obj.data.materials.clear()
        obj.data.materials.append(mats[name])
    ch.materials = mats
    ch.colours = colours
    surfaces = gear_mod.Surfaces(body)
    ch.gear = gear_mod.build(ch, surfaces)
    anim.build(ch, gear_mod, surfaces)
    return ch
