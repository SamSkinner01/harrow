"""Geometry helpers for the fortress kit.

All coordinates here are in *game* (Bevy) space: X right, Y up, Z towards the
viewer, metres. They are converted to Blender space (Z up) when written, and
the glTF exporter converts back, so what you write here is exactly what the
game sees.

UVs are world-scale (1 UV unit = 1 metre), projected per face. The game's
material library sets how many metres each texture covers, so textures line
up across neighbouring pieces placed on the 1 m grid.
"""

import math

import bmesh
import bpy
from mathutils import Vector


def to_blender(p):
    """Game (x, y, z) -> Blender (x, -z, y)."""
    return Vector((p[0], -p[2], p[1]))


def newell_normal(points):
    """Normal of a planar polygon (game space), robust for concave outlines."""
    n = Vector((0.0, 0.0, 0.0))
    for i, a in enumerate(points):
        b = points[(i + 1) % len(points)]
        n.x += (a.y - b.y) * (a.z + b.z)
        n.y += (a.z - b.z) * (a.x + b.x)
        n.z += (a.x - b.x) * (a.y + b.y)
    return n.normalized()


def face_uv(p, normal):
    """World-scale UV for point `p` on a face with `normal` (game space)."""
    if abs(normal.y) > 0.7:
        # Floors and ceilings.
        return (p.x, p.z)
    # Walls: u runs to the viewer's right, v downwards (image convention).
    right = Vector((0.0, 1.0, 0.0)).cross(normal)
    if right.length < 1e-6:
        right = Vector((1.0, 0.0, 0.0))
    right.normalize()
    return (p.dot(right), -p.y)


class MeshBuilder:
    """Accumulates faces (with a material slot each) into one bmesh."""

    def __init__(self, materials):
        self.bm = bmesh.new()
        self.uv = self.bm.loops.layers.uv.new("UVMap")
        self.materials = list(materials)

    def slot(self, material):
        if material not in self.materials:
            self.materials.append(material)
        return self.materials.index(material)

    def face(self, points, material):
        """Add one polygon. Points are game-space, counter-clockwise when
        seen from outside."""
        pts = [Vector(p) for p in points]
        normal = newell_normal(pts)
        verts = [self.bm.verts.new(to_blender(p)) for p in pts]
        f = self.bm.faces.new(verts)
        f.material_index = self.slot(material)
        for loop, p in zip(f.loops, pts):
            loop[self.uv].uv = face_uv(p, normal)
        return f

    def box(self, mn, mx, material):
        """Axis-aligned box from min to max corner."""
        (x0, y0, z0), (x1, y1, z1) = mn, mx
        v = lambda x, y, z: (x, y, z)  # noqa: E731
        self.face([v(x0, y0, z1), v(x1, y0, z1), v(x1, y1, z1), v(x0, y1, z1)], material)  # +Z
        self.face([v(x1, y0, z0), v(x0, y0, z0), v(x0, y1, z0), v(x1, y1, z0)], material)  # -Z
        self.face([v(x1, y0, z1), v(x1, y0, z0), v(x1, y1, z0), v(x1, y1, z1)], material)  # +X
        self.face([v(x0, y0, z0), v(x0, y0, z1), v(x0, y1, z1), v(x0, y1, z0)], material)  # -X
        self.face([v(x0, y1, z1), v(x1, y1, z1), v(x1, y1, z0), v(x0, y1, z0)], material)  # +Y
        self.face([v(x0, y0, z0), v(x1, y0, z0), v(x1, y0, z1), v(x0, y0, z1)], material)  # -Y

    def prism_xy(self, outline, z0, z1, material):
        """Extrude a polygon drawn in the XY plane (counter-clockwise seen
        from +Z) between z0 and z1. Used for walls with arched openings."""
        front = [(x, y, z1) for x, y in outline]
        back = [(x, y, z0) for x, y in reversed(outline)]
        self.face(front, material)
        self.face(back, material)
        n = len(outline)
        for i in range(n):
            (ax, ay), (bx, by) = outline[i], outline[(i + 1) % n]
            self.face([(ax, ay, z1), (ax, ay, z0), (bx, by, z0), (bx, by, z1)], material)

    def to_object(self, name, collection):
        mesh = bpy.data.meshes.new(name)
        self.bm.normal_update()
        self.bm.to_mesh(mesh)
        self.bm.free()
        for m in self.materials:
            mesh.materials.append(material(m))
        obj = bpy.data.objects.new(name, mesh)
        collection.objects.link(obj)
        return obj


# Approximate colours, only used if a material is missing from the game's
# library (and for previews in Blender).
MATERIAL_COLOURS = {
    "stone": (0.72, 0.62, 0.48),
    "plaster": (0.86, 0.80, 0.70),
    "flagstone": (0.66, 0.58, 0.47),
    "wood": (0.45, 0.33, 0.22),
    "metal": (0.35, 0.35, 0.36),
    "burlap": (0.62, 0.52, 0.36),
}


def material(name):
    mat = bpy.data.materials.get(name)
    if mat is None:
        mat = bpy.data.materials.new(name)
        r, g, b = MATERIAL_COLOURS.get(name, (1.0, 0.0, 1.0))
        bsdf = mat.node_tree.nodes.get("Principled BSDF")
        if bsdf is not None:
            bsdf.inputs["Base Color"].default_value = (r, g, b, 1.0)
            bsdf.inputs["Roughness"].default_value = 0.9
        mat.diffuse_color = (r, g, b, 1.0)
    return mat


def arch_outline(length, opening, spring, height, segments=12):
    """Outline (XY, CCW) of a wall from x=0..length, y=0..height with a
    round-arched opening centred on the wall, reaching the floor."""
    cx = length / 2.0
    r = opening / 2.0
    pts = [(0.0, 0.0), (cx - r, 0.0), (cx - r, spring)]
    for i in range(1, segments):
        a = math.pi - math.pi * i / segments
        pts.append((cx + r * math.cos(a), spring + r * math.sin(a)))
    pts += [(cx + r, spring), (cx + r, 0.0), (length, 0.0), (length, height), (0.0, height)]
    # Counter-clockwise: the wall is always on the left of the direction of
    # travel (along the floor, up the left jamb, over the arch, down the right).
    return pts
