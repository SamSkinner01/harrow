"""Body post-processing: decimation, clothing regions, vertex colours.

The uniform is not a separate mesh. The MPFB body is split into regions by
its bone weights (skin, scalp, shirt, trousers, gloves, boots); clothed
regions are pushed outwards a little and coloured from the palette, and use
the shared vertex-coloured "gear" material. Only skin keeps the textured
"skin" material. This costs no extra triangles and nothing pokes through.

Coordinates here are Blender space after `mpfb.stage_for_game`: Z up, the
character faces +Y, its left side is -X.
"""

import math
import random

import bmesh
import bpy
from mathutils import Vector

from .palette import hex_to_linear

HAND_BONES = ("hand", "thumb", "index", "middle", "ring", "pinky")
FINGER_TIPS = ("_02_", "_03_")

# Outward push per region, metres.
INFLATE = {
    "skin": 0.0,
    "scalp": 0.0,
    "shirt": 0.007,
    "trousers": 0.010,
    "glove": 0.0035,
    "boot": 0.012,
}

MATERIAL_OF = {
    "skin": "skin",
    "scalp": "skin",
    "shirt": "gear",
    "trousers": "gear",
    "glove": "gear",
    "boot": "gear",
}


def apply_modifier(obj, mod):
    """Apply one modifier through the depsgraph (no operator context needed),
    with any armature modifier switched off so we work in rest pose."""
    armatures = [m for m in obj.modifiers if m.type == "ARMATURE"]
    for m in armatures:
        m.show_viewport = False
    depsgraph = bpy.context.evaluated_depsgraph_get()
    evaluated = obj.evaluated_get(depsgraph)
    mesh = bpy.data.meshes.new_from_object(evaluated, preserve_all_data_layers=True, depsgraph=depsgraph)
    old = obj.data
    obj.modifiers.remove(mod)
    obj.data = mesh
    bpy.data.meshes.remove(old)
    for m in armatures:
        m.show_viewport = True


def decimate(obj, ratio, protect):
    """Collapse-decimate to `ratio`, symmetric in X. Vertices weighted by
    `protect(name_of_dominant_bone)` (0..1) keep more detail."""
    group = obj.vertex_groups.new(name="_protect")
    names = {g.index: g.name for g in obj.vertex_groups}
    for v in obj.data.vertices:
        bone = dominant_bone(v, names)
        w = protect(bone) if bone else 0.0
        if w > 0.0:
            group.add([v.index], w, "REPLACE")
    mod = obj.modifiers.new("Decimate", "DECIMATE")
    mod.decimate_type = "COLLAPSE"
    mod.ratio = ratio
    mod.use_symmetry = True
    mod.symmetry_axis = "X"
    # Weighted vertices collapse *more* readily, so the protection is
    # inverted: unprotected areas get weight 1.
    mod.vertex_group = group.name
    mod.invert_vertex_group = True
    mod.vertex_group_factor = 4.0
    apply_modifier(obj, mod)
    obj.vertex_groups.remove(obj.vertex_groups["_protect"])


def dominant_bone(vertex, names):
    best, best_w = None, 0.0
    for g in vertex.groups:
        if g.weight > best_w and names.get(g.group, "_").find("_protect") < 0:
            best, best_w = names.get(g.group), g.weight
    return best


def is_hand(bone):
    return bone is not None and bone.startswith(HAND_BONES)


class Landmarks:
    """Heights and positions read from the rig (and the eyes), Blender space."""

    def __init__(self, rig, eyes):
        b = rig.data.bones
        self.rig = rig
        self.head = b["head"].head_local.copy()
        self.head_tail = b["head"].tail_local.copy()
        self.neck = b["neck_01"].head_local.copy()
        self.chest = b["spine_03"].head_local.copy()
        self.chest_top = b["spine_03"].tail_local.copy()
        self.spine2 = b["spine_02"].head_local.copy()
        self.spine1 = b["spine_01"].head_local.copy()
        self.pelvis = b["pelvis"].head_local.copy()
        self.waist_z = (self.pelvis.z + self.spine1.z) / 2.0 + 0.03
        self.collar_z = self.neck.z + 0.03
        self.knee = {s: b[f"calf_{s}"].head_local.copy() for s in "lr"}
        self.hip = {s: b[f"thigh_{s}"].head_local.copy() for s in "lr"}
        self.ankle = {s: b[f"foot_{s}"].head_local.copy() for s in "lr"}
        self.ball = {s: b[f"ball_{s}"].head_local.copy() for s in "lr"}
        self.toe = {s: b[f"ball_{s}"].tail_local.copy() for s in "lr"}
        self.shoulder = {s: b[f"upperarm_{s}"].head_local.copy() for s in "lr"}
        self.elbow = {s: b[f"lowerarm_{s}"].head_local.copy() for s in "lr"}
        self.wrist = {s: b[f"hand_{s}"].head_local.copy() for s in "lr"}
        self.clavicle = {s: b[f"clavicle_{s}"].head_local.copy() for s in "lr"}
        self.forearm_dir = {s: (self.wrist[s] - self.elbow[s]).normalized() for s in "lr"}
        verts = [v.co for v in eyes.data.vertices]
        self.eye = sum(verts, Vector()) / len(verts)
        self.eye_half = max(abs(v.x) for v in verts)
        self.top = None  # set from the body mesh


ARM_CHAIN = ("upperarm", "lowerarm", "clavicle", "hand") + HAND_BONES[1:]


def _cut_planes(marks, gear):
    """(point, normal, keep) planes along which garments end. `keep(c)`
    limits each cut to the faces near it."""
    planes = []
    m = marks
    planes.append((Vector((0, 0, m.waist_z)), Vector((0, 0, 1)), lambda c: True))
    planes.append((
        Vector((0, 0, m.collar_z)), Vector((0, 0, 1)),
        lambda c: abs(c.x) < 0.11 and abs(c.z - m.collar_z) < 0.08,
    ))
    boot_top = BOOT_TOP.get(gear["boots"], 0.0)
    if boot_top:
        planes.append((Vector((0, 0, boot_top)), Vector((0, 0, 1)), lambda c: c.z < boot_top + 0.1))
    for s in "lr":
        wrist, d = m.wrist[s], m.forearm_dir[s]
        if gear["gloves"] != "none":
            cuff = wrist - d * 0.035
            planes.append((cuff, d, lambda c, p=cuff: (c - p).length < 0.12))
        if gear["sleeves"] == "rolled":
            sleeve = m.elbow[s] + d * 0.07
            planes.append((sleeve, d, lambda c, p=sleeve: (c - p).length < 0.12))
    return planes


BOOT_TOP = {"high": 0.20, "low": 0.12}


def cut_seams(body, marks, gear):
    """Bisect the mesh where garments end so seams are clean lines."""
    bm = bmesh.new()
    bm.from_mesh(body.data)
    for point, normal, keep in _cut_planes(marks, gear):
        faces = [f for f in bm.faces if keep(f.calc_center_median())]
        # Ordered (not sets) so the result is the same on every run.
        edges = list(dict.fromkeys(e for f in faces for e in f.edges))
        verts = list(dict.fromkeys(v for f in faces for v in f.verts))
        geom = edges + faces + verts
        bmesh.ops.bisect_plane(bm, geom=geom, dist=1e-5, plane_co=point, plane_no=normal)
    bm.to_mesh(body.data)
    bm.free()
    body.data.update()


def _poly_bone(poly, mesh, names):
    totals = {}
    for vi in poly.vertices:
        for g in mesh.vertices[vi].groups:
            totals[g.group] = totals.get(g.group, 0.0) + g.weight
    if not totals:
        return ""
    return names.get(max(totals, key=totals.get), "")


def classify(body, marks, gear):
    """Region name per polygon (after `cut_seams`)."""
    names = {g.index: g.name for g in body.vertex_groups}
    boot_top = BOOT_TOP.get(gear["boots"], 0.0)
    m = marks
    mesh = body.data
    out = []
    for poly in mesh.polygons:
        c = poly.center
        bone = _poly_bone(poly, mesh, names)
        side = "l" if c.x < 0 else "r"
        past_cuff = (c - (m.wrist[side] - m.forearm_dir[side] * 0.035)).dot(m.forearm_dir[side]) > 0
        past_sleeve = (c - (m.elbow[side] + m.forearm_dir[side] * 0.07)).dot(m.forearm_dir[side]) > 0
        if bone == "head" or (bone == "neck_01" and c.z > m.collar_z):
            region = "skin"
        elif bone.startswith(ARM_CHAIN) and (is_hand(bone) or bone.startswith("lowerarm")) and past_cuff:
            fingertip = any(t in bone + "_" for t in FINGER_TIPS)
            if gear["gloves"] == "full" or (gear["gloves"] == "fingerless" and not fingertip):
                region = "glove"
            else:
                region = "skin"
        elif bone.startswith(ARM_CHAIN):
            rolled = gear["sleeves"] == "rolled" and bone.startswith(("lowerarm", "hand")) and past_sleeve
            region = "skin" if rolled else "shirt"
        elif boot_top and c.z < boot_top and bone.startswith(("foot", "ball", "calf", "thigh")):
            region = "boot"
        elif c.z < m.waist_z:
            region = "trousers"
        else:
            region = "shirt"
        out.append(region)
    return out


def smooth_region(body, verts, iterations, factor=0.5):
    """Taubin smoothing (shrink, then inflate) restricted to `verts`
    (indices), so the shape loses detail but not volume. Vertices on the
    edge of the set stay put."""
    bm = bmesh.new()
    bm.from_mesh(body.data)
    bm.verts.ensure_lookup_table()
    inside = set(verts)
    movable = [i for i in inside if all(e.other_vert(bm.verts[i]).index in inside for e in bm.verts[i].link_edges)]
    for _ in range(iterations):
        for step in (factor, -factor * 1.06):
            new = {}
            for i in movable:
                v = bm.verts[i]
                avg = Vector()
                for e in v.link_edges:
                    avg += e.other_vert(v).co
                avg /= max(1, len(v.link_edges))
                new[i] = v.co + (avg - v.co) * step
            for i, co in new.items():
                bm.verts[i].co = co
    bm.to_mesh(body.data)
    bm.free()


def inflate(body, offsets):
    """Push each vertex out along its normal by offsets[i]."""
    body.data.update()
    normals = [v.normal.copy() for v in body.data.vertices]
    for v, n, d in zip(body.data.vertices, normals, offsets):
        if d:
            v.co += n * d
    body.data.update()


def dress(body, marks, cfg, colours, materials):
    """Assign regions, materials, vertex colours and inflation."""
    gear = cfg["gear"]
    skin = cfg["skin"]
    cut_seams(body, marks, gear)
    mesh = body.data
    regions = classify(body, marks, gear)
    marks.top = max(v.co.z for v in mesh.vertices)

    # Boots: lose the toes before inflating.
    boot_verts = {vi for p, r in zip(mesh.polygons, regions) if r == "boot" for vi in p.vertices}
    if boot_verts:
        smooth_region(body, boot_verts, 12, 0.6)

    # Inflation per vertex: the largest offset of any polygon using it.
    offsets = [0.0] * len(mesh.vertices)
    for poly, region in zip(mesh.polygons, regions):
        for vi in poly.vertices:
            offsets[vi] = max(offsets[vi], INFLATE[region])
    inflate(body, offsets)

    # Materials.
    mesh.materials.clear()
    for name in ("skin", "gear"):
        mesh.materials.append(materials[name])
    for poly, region in zip(mesh.polygons, regions):
        poly.material_index = 0 if MATERIAL_OF[region] == "skin" else 1

    # Vertex colours (per corner, linear).
    attr = mesh.color_attributes.new("Color", "FLOAT_COLOR", "CORNER")
    tint = tuple(skin["tint"]) + (1.0,)
    hair = hex_to_linear(skin["hair_colour"])
    noise = _value_noise(random.Random(cfg["id"]))
    role = {"shirt": "shirt", "trousers": "trousers", "glove": "gloves", "boot": "boots"}
    for poly, region in zip(mesh.polygons, regions):
        for li in poly.loop_indices:
            co = mesh.vertices[mesh.loops[li].vertex_index].co
            if region in ("skin", "scalp"):
                c = _skin_colour(co, marks, tint, hair, skin)
            else:
                c = _fabric(colours[role[region]], co, noise, region)
            attr.data[li].color = c
    mesh.color_attributes.active_color = attr
    mesh.color_attributes.render_color_index = 0
    mesh.update()
    return regions


def _skin_colour(co, marks, tint, hair, skin):
    """Skin tint, plus a buzz cut on the scalp and optional stubble, as a
    multiplier on the skin texture. Measured from the eyes."""
    e = marks.eye
    dz = co.z - e.z            # up from eye level
    dy = co.y - marks.head.y   # forward from the middle of the skull
    face_front = e.y - marks.head.y  # eyes are this far forward
    c = list(tint[:3])
    mode = skin["hair"]
    if mode in ("buzz", "short"):
        # Hairline: ~7 cm above the eyes at the front, at eye level at the
        # temples, down to the nape at the back.
        t = _smoothstep(face_front * 0.9, -0.02, dy)  # 0 at the forehead .. 1 behind the ears
        hairline = 0.072 - 0.15 * t
        w = _smoothstep(hairline - 0.012, hairline + 0.012, dz)
        # Ears and sideburn gap stay bare.
        if abs(co.x) > e.x + marks.eye_half * 0.0 + 0.055 and abs(dy) < 0.035 and dz < 0.03:
            w *= 0.0
        strength = 0.8 if mode == "buzz" else 0.95
        c = [c[i] * (1.0 - strength * w) + hair[i] * strength * w * 2.0 for i in range(3)]
    stubble = float(skin["stubble"])
    if stubble > 0.0 and dy > face_front * 0.15:
        jaw = _smoothstep(-0.03, -0.05, dz) * _smoothstep(-0.16, -0.12, dz)
        lips = _smoothstep(0.035, 0.02, abs(co.x)) * _smoothstep(0.022, 0.008, abs(dz + 0.083))
        w = stubble * jaw * (1.0 - lips) * 0.55
        c = [c[i] * (1.0 - w) + hair[i] * w * 2.0 for i in range(3)]
    return (c[0], c[1], c[2], 1.0)


def _fabric(base, co, noise, region):
    """Base colour with low-frequency mottling and dust towards the ground."""
    n = noise(co * 6.0)
    shade = 0.92 + 0.16 * n
    dust = _smoothstep(0.5, 0.0, co.z) * (0.25 if region in ("trousers", "boot") else 0.1)
    dust_colour = (0.52, 0.42, 0.30)
    return tuple(
        (base[i] * shade) * (1.0 - dust) + dust_colour[i] * dust for i in range(3)
    ) + (1.0,)


def _smoothstep(a, b, x):
    if a == b:
        return 1.0 if x >= b else 0.0
    t = max(0.0, min(1.0, (x - a) / (b - a)))
    return t * t * (3.0 - 2.0 * t)


def _value_noise(rng):
    """Smooth 3D value noise in 0..1."""
    table = [rng.random() for _ in range(256)]
    perm = list(range(256))
    rng.shuffle(perm)

    def lattice(x, y, z):
        return table[perm[(perm[(perm[x & 255] + y) & 255] + z) & 255]]

    def noise(p):
        x0, y0, z0 = math.floor(p.x), math.floor(p.y), math.floor(p.z)
        fx, fy, fz = p.x - x0, p.y - y0, p.z - z0
        sx, sy, sz = (_smoothstep(0, 1, f) for f in (fx, fy, fz))
        total = 0.0
        for dx in (0, 1):
            for dy in (0, 1):
                for dz in (0, 1):
                    w = (sx if dx else 1 - sx) * (sy if dy else 1 - sy) * (sz if dz else 1 - sz)
                    total += w * lattice(int(x0) + dx, int(y0) + dy, int(z0) + dz)
        return total

    return noise
