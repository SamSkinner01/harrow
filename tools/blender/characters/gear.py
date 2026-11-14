"""Scripted tactical gear: mid-detail blockouts fitted to each body.

Every piece is built into one "gear" mesh (one material, vertex colours)
that is skinned to the rig. Pieces that wrap the body (plate carriers,
belts, straps, pads, helmet shells) are *conformed*: rays are cast at the
dressed body and the hits, pushed out by an offset, become the piece's
surface, so the same script fits a slim scout and a heavy anchor. Their
skin weights are copied from the nearest body surface. Hard items (pouches,
radios, the cutter disc) are boxes and cylinders placed on those surfaces,
and are either skinned the same way or tied rigidly to one bone.

Blender space after staging: Z up, the character faces +Y, its left side
is -X. Angles around the body (`phi`) are measured from the front, positive
towards the character's right (+X).

All designs are generic and original: no real products, brands or insignia.
"""

import math

import bmesh
import bpy
from mathutils import Matrix, Vector
from mathutils.bvhtree import BVHTree

from . import body as body_mod

ARMS = ("upperarm", "lowerarm", "clavicle", "hand") + body_mod.HAND_BONES[1:]

# Material slots of the gear mesh, and which palette roles use which.
GEAR_MATERIALS = ("webbing", "hard", "leather", "uniform")
ROLE_MATERIAL = {
    "helmet": "hard", "metal": "hard", "dark": "hard", "lens": "hard", "pads": "hard",
    "sole": "hard", "accent": "hard",
    "gloves": "leather", "boots": "leather",
    "shirt": "uniform", "trousers": "uniform",
}


def _smooth01(t):
    t = max(0.0, min(1.0, t))
    return t * t * (3 - 2 * t)


def _lerp_table(table, x):
    """Piecewise-linear lookup in [(x, y), ...] sorted by x."""
    if x <= table[0][0]:
        return table[0][1]
    for (x0, y0), (x1, y1) in zip(table, table[1:]):
        if x <= x1:
            t = (x - x0) / (x1 - x0)
            return y0 + (y1 - y0) * t
    return table[-1][1]


def _frame(normal, up=Vector((0, 0, 1))):
    """Rotation whose local +Y points along `normal` and +Z is as close to
    `up` as possible (pouches face outwards, stand upright)."""
    y = normal.normalized()
    x = y.cross(up)
    if x.length < 1e-4:
        x = Vector((1, 0, 0))
    x.normalize()
    z = x.cross(y).normalized()
    return Matrix((x, y, z)).transposed().to_4x4()


def _face_ring(axis, sign):
    """The four box corners (sign tuples) of the face on `axis` at `sign`,
    in order around the face."""
    u, v = [a for a in range(3) if a != axis]
    out = []
    for su, sv in ((-1, -1), (1, -1), (1, 1), (-1, 1)):
        c = [0, 0, 0]
        c[axis], c[u], c[v] = sign, su, sv
        out.append(tuple(c))
    return out


class Surfaces:
    """Ray-cast targets over parts of the dressed body."""

    def __init__(self, body):
        mesh = body.data
        names = {g.index: g.name for g in body.vertex_groups}
        self.verts = [v.co.copy() for v in mesh.vertices]
        self.mesh = mesh
        self.names = names
        groups = {"all": [], "torso": [], "head": [], "legs": [], "arms": [],
                  "legs_l": [], "legs_r": [], "arms_l": [], "arms_r": []}
        self.poly_bone = []
        for p in mesh.polygons:
            bone = body_mod._poly_bone(p, mesh, names)
            self.poly_bone.append(bone)
            verts = list(p.vertices)
            groups["all"].append(verts)
            if bone in ("head", "neck_01"):
                groups["head"].append(verts)
            if bone.startswith(ARMS):
                groups["arms"].append(verts)
                groups["arms_" + bone[-1]].append(verts)
            elif bone.startswith(("thigh", "calf", "foot", "ball")):
                groups["legs"].append(verts)
                groups["legs_" + bone[-1]].append(verts)
                if bone.startswith("thigh"):
                    groups["torso"].append(verts)
            elif bone != "head":
                groups["torso"].append(verts)
        self.trees = {k: BVHTree.FromPolygons(self.verts, v) for k, v in groups.items()}
        self.all_polys = groups["all"]
        self.vert_weights = [
            {names[g.group]: g.weight for g in v.groups if g.weight > 1e-4} for v in mesh.vertices
        ]

    def cast(self, part, origin, direction, max_dist=2.0):
        loc, normal, index, dist = self.trees[part].ray_cast(origin, direction.normalized(), max_dist)
        if loc is None:
            return None
        return loc, normal, dist

    def weights_at(self, point):
        """Skin weights at the body surface nearest to `point`."""
        loc, _n, index, _d = self.trees["all"].find_nearest(point)
        if index is None:
            return {"spine_02": 1.0}
        verts = self.all_polys[index]
        total = {}
        wsum = 0.0
        for vi in verts:
            d = (self.verts[vi] - loc).length
            w = 1.0 / (d + 1e-3)
            wsum += w
            for bone, bw in self.vert_weights[vi].items():
                total[bone] = total.get(bone, 0.0) + bw * w
        best = sorted(total.items(), key=lambda kv: -kv[1])[:4]
        s = sum(w for _, w in best) or 1.0
        return {b: w / s for b, w in best}


class Gear:
    """Accumulates gear pieces into one bmesh."""

    def __init__(self, ch, surfaces):
        self.ch = ch
        self.m = ch.marks
        self.c = ch.colours
        self.s = surfaces
        self.bm = bmesh.new()
        self.col = self.bm.loops.layers.float_color.new("Color")
        self.dl = self.bm.verts.layers.deform.verify()
        self.uv = self.bm.loops.layers.uv.new("UVMap")
        self.bones = [b.name for b in ch.rig.data.bones]
        self.bone_index = {n: i for i, n in enumerate(self.bones)}
        self.anchors = {}
        # Material from the colour's palette role; `force_material` overrides.
        self.role_of = {tuple(v): k for k, v in ch.colours.items()}
        self.force_material = None

    # ------------------------------------------------------------ output

    def to_object(self, name, materials):
        mesh = bpy.data.meshes.new(name)
        self.bm.normal_update()
        self.bm.to_mesh(mesh)
        self.bm.free()
        for m in GEAR_MATERIALS:
            mesh.materials.append(materials[m])
        obj = bpy.data.objects.new(name, mesh)
        bpy.context.scene.collection.objects.link(obj)
        for bone in self.bones:
            obj.vertex_groups.new(name=bone)
        obj.parent = self.ch.rig
        mod = obj.modifiers.new("Armature", "ARMATURE")
        mod.object = self.ch.rig
        # The glTF exporter's "ACTIVE" vertex colour is the *render* one.
        colours = mesh.color_attributes
        colours.active_color = colours["Color"]
        colours.render_color_index = colours.find("Color")
        return obj

    # --------------------------------------------------------- finishing

    def finish(self, faces, colour, weights=None, smooth=None, shade=True):
        """Colour, weight and normal-fix a piece made of `faces`.
        `weights`: None to copy from the body, or {bone: weight}."""
        faces = [f for f in faces if f.is_valid]
        bmesh.ops.recalc_face_normals(self.bm, faces=faces)
        verts = {v for f in faces for v in f.verts}
        for v in verts:
            w = weights if weights is not None else self.s.weights_at(v.co)
            dv = v[self.dl]
            dv.clear()
            for bone, bw in w.items():
                dv[self.bone_index[bone]] = bw
        role = self.role_of.get(tuple(colour), "gear")
        material = GEAR_MATERIALS.index(self.force_material or ROLE_MATERIAL.get(role, "webbing"))
        for f in faces:
            if smooth is not None:
                f.smooth = smooth
            f.normal_update()
            f.material_index = material
            for loop in f.loops:
                loop[self.uv].uv = body_mod.box_uv(loop.vert.co, f.normal)
            k = 1.0
            if shade:
                # Baked hint of occlusion: undersides a little darker.
                k = 0.86 + 0.14 * (0.5 + 0.5 * f.normal.z)
            for loop in f.loops:
                loop[self.col] = (colour[0] * k, colour[1] * k, colour[2] * k, 1.0)
        return faces

    # --------------------------------------------------------- primitives

    def _make(self, points, quads, matrix):
        vs = [self.bm.verts.new(matrix @ Vector(p)) for p in points]
        return [self.bm.faces.new([vs[i] for i in q]) for q in quads]

    def box(self, matrix, size, colour, weights=None, bevel=0.0, taper=1.0):
        """Box centred on `matrix`, size (x, y, z), edges chamfered by
        `bevel`. `taper` scales the +Z end (wedges, rounded-looking pouches).
        Built by hand rather than with bmesh's bevel so the face order is
        the same on every run (reproducible .glb files)."""
        sx, sy, sz = (s / 2 for s in size)
        b = min(bevel, sx * 0.9, sy * 0.9, sz * 0.9)
        signs = [(cx, cy, cz) for cx in (-1, 1) for cy in (-1, 1) for cz in (-1, 1)]

        def corner(c, axis):
            # The corner's vertex on the face perpendicular to `axis`.
            p = [(sx - b) * c[0], (sy - b) * c[1], (sz - b) * c[2]]
            p[axis] = (sx, sy, sz)[axis] * c[axis]
            if p[2] > 0:
                p[0], p[1] = p[0] * taper, p[1] * taper
            return p

        if b <= 0.0:
            pts = [corner(c, 0) for c in signs]  # all three coincide
            idx = {c: i for i, c in enumerate(signs)}
            quads = []
            for axis in range(3):
                for sign in (-1, 1):
                    ring = _face_ring(axis, sign)
                    quads.append(tuple(idx[c] for c in ring))
            faces = self._make(pts, quads, matrix)
            return self.finish(faces, colour, weights, smooth=False)

        pts, idx = [], {}
        for c in signs:
            for axis in range(3):
                idx[(c, axis)] = len(pts)
                pts.append(corner(c, axis))
        polys = []
        for axis in range(3):                      # main faces
            for sign in (-1, 1):
                polys.append(tuple(idx[(c, axis)] for c in _face_ring(axis, sign)))
        for a1, a2 in ((0, 1), (0, 2), (1, 2)):    # edge strips
            free = 3 - a1 - a2
            for s1 in (-1, 1):
                for s2 in (-1, 1):
                    def c(sf, s1=s1, s2=s2):
                        out = [0, 0, 0]
                        out[a1], out[a2], out[free] = s1, s2, sf
                        return tuple(out)
                    polys.append((idx[(c(-1), a1)], idx[(c(1), a1)], idx[(c(1), a2)], idx[(c(-1), a2)]))
        for c in signs:                            # corner triangles
            polys.append((idx[(c, 0)], idx[(c, 1)], idx[(c, 2)]))
        faces = self._make(pts, polys, matrix)
        return self.finish(faces, colour, weights, smooth=False)

    def cylinder(self, matrix, radius, length, colour, weights=None, segments=12, radius2=None, smooth=True):
        """Cylinder along local Z, centred on `matrix`."""
        r2 = radius if radius2 is None else radius2
        pts = []
        for z, r in ((-length / 2, radius), (length / 2, r2)):
            for i in range(segments):
                a = 2 * math.pi * i / segments
                pts.append((r * math.cos(a), r * math.sin(a), z))
        n = segments
        quads = [(i, (i + 1) % n, n + (i + 1) % n, n + i) for i in range(n)]
        quads.append(tuple(reversed(range(n))))
        quads.append(tuple(range(n, 2 * n)))
        faces = self._make(pts, quads, matrix)
        for f in faces[:n]:
            f.smooth = smooth
        return self.finish(faces, colour, weights)

    def shell(self, part, rays, nu, nv, offset, thickness, colour, wrap_u=False, weights=None,
              smooth_iters=2, cap_top=False, smooth=True):
        """Conformed solid sheet. `rays(i, j)` -> (origin, direction) for
        grid point i in 0..nu-1, j in 0..nv-1. Each ray's hit on the body
        part, moved back along the ray by `offset` (inner face) and
        `offset + thickness` (outer face), gives the sheet."""
        depth = [[None] * nv for _ in range(nu)]
        rays_at = [[rays(i, j) for j in range(nv)] for i in range(nu)]
        for i in range(nu):
            for j in range(nv):
                o, d = rays_at[i][j]
                hit = self.s.cast(part, o, d)
                if hit is not None:
                    depth[i][j] = hit[2]
        # Fill misses from neighbours, then smooth the depth field.
        known = [depth[i][j] for i in range(nu) for j in range(nv) if depth[i][j] is not None]
        if not known:
            return []
        fallback = sum(known) / len(known)
        for _ in range(max(nu, nv)):
            missing = False
            for i in range(nu):
                for j in range(nv):
                    if depth[i][j] is None:
                        ns = [depth[a][b] for a, b in self._neighbours(i, j, nu, nv, wrap_u) if depth[a][b] is not None]
                        if ns:
                            depth[i][j] = sum(ns) / len(ns)
                        else:
                            missing = True
            if not missing:
                break
        for i in range(nu):
            for j in range(nv):
                if depth[i][j] is None:
                    depth[i][j] = fallback
        for _ in range(smooth_iters):
            new = [[0.0] * nv for _ in range(nu)]
            for i in range(nu):
                for j in range(nv):
                    ns = [depth[a][b] for a, b in self._neighbours(i, j, nu, nv, wrap_u)]
                    # Never pull a point inside the body: keep the minimum.
                    avg = (depth[i][j] * 2 + sum(ns)) / (2 + len(ns))
                    new[i][j] = min(avg, depth[i][j])
            depth = new

        def point(i, j, extra):
            o, d = rays_at[i][j]
            return o + d.normalized() * (depth[i][j] - extra)

        inner = [[self.bm.verts.new(point(i, j, offset)) for j in range(nv)] for i in range(nu)]
        outer = [[self.bm.verts.new(point(i, j, offset + thickness)) for j in range(nv)] for i in range(nu)]
        faces = []
        iu = nu if wrap_u else nu - 1
        for i in range(iu):
            i2 = (i + 1) % nu
            for j in range(nv - 1):
                faces.append(self.bm.faces.new((outer[i][j], outer[i2][j], outer[i2][j + 1], outer[i][j + 1])))
                faces.append(self.bm.faces.new((inner[i][j + 1], inner[i2][j + 1], inner[i2][j], inner[i][j])))
        # Rims.
        for i in range(iu):
            i2 = (i + 1) % nu
            for j in (0, nv - 1):
                if j == nv - 1 and cap_top:
                    continue
                faces.append(self.bm.faces.new((inner[i][j], inner[i2][j], outer[i2][j], outer[i][j])))
        if not wrap_u:
            for i in (0, nu - 1):
                for j in range(nv - 1):
                    faces.append(self.bm.faces.new((inner[i][j], outer[i][j], outer[i][j + 1], inner[i][j + 1])))
        if cap_top:
            faces.append(self.bm.faces.new([outer[i][nv - 1] for i in range(nu)]))
            faces.append(self.bm.faces.new([inner[i][nv - 1] for i in reversed(range(nu))]))
        self.finish(faces, colour, weights, smooth=smooth)
        return faces

    @staticmethod
    def _neighbours(i, j, nu, nv, wrap_u):
        out = []
        for a, b in ((i - 1, j), (i + 1, j), (i, j - 1), (i, j + 1)):
            if wrap_u:
                a %= nu
            if 0 <= a < nu and 0 <= b < nv:
                out.append((a, b))
        return out

    def surface(self, part, origin, direction, extra):
        """Point `extra` metres off the body along a ray, and the outward
        normal there (towards the ray origin)."""
        hit = self.s.cast(part, origin, direction)
        d = direction.normalized()
        if hit is None:
            return None, -d
        loc, normal, dist = hit
        n = (normal.normalized() - d).normalized()
        return origin + d * (dist - extra), n

    def place(self, part, origin, direction, extra, depth, up=Vector((0, 0, 1)), tilt=0.0):
        """Matrix for an item of thickness `depth` sitting on the body,
        `extra` metres out, its back against the surface."""
        p, n = self.surface(part, origin, direction, extra)
        if p is None:
            p = origin
        n = n.lerp(-direction.normalized(), 0.5).normalized()
        frame = _frame(n, up)
        if tilt:
            frame = frame @ Matrix.Rotation(tilt, 4, "X")
        frame.translation = p + n * (depth / 2)
        return frame

    def limb_band(self, part, p0, axis, t0, t1, offset, thickness, colour, nu=12, nv=2,
                  weights=None, phi0=0.0, phi1=2 * math.pi):
        """Band around a limb (or the neck): rays aimed at the axis from
        all sides, between distances t0..t1 along it from p0."""
        axis = axis.normalized()
        ref = Vector((0, 0, 1)) if abs(axis.z) < 0.9 else Vector((0, 1, 0))
        e1 = axis.cross(ref).normalized()
        e2 = axis.cross(e1).normalized()
        full = abs(phi1 - phi0 - 2 * math.pi) < 1e-6
        count = nu if full else nu + 1

        def rays(i, j):
            a = phi0 + (phi1 - phi0) * i / (nu if full else nu)
            t = t0 + (t1 - t0) * j / (nv - 1)
            d = e1 * math.cos(a) + e2 * math.sin(a)
            centre = p0 + axis * t
            return centre + d * 0.35, -d

        return self.shell(part, rays, count, nv, offset, thickness, colour, wrap_u=full, weights=weights, smooth_iters=1)

    def torso_band(self, z0, z1, offset, thickness, colour, phi0=-math.pi, phi1=math.pi, nu=24, nv=3,
                   part="torso", centre=None, weights=None):
        """Band around the torso between heights z0..z1 (or a part of it,
        from angle phi0 to phi1)."""
        m = self.m
        full = abs(phi1 - phi0 - 2 * math.pi) < 1e-6
        cy = centre if centre is not None else (m.chest.y + m.pelvis.y) / 2

        def rays(i, j):
            a = phi0 + (phi1 - phi0) * i / (nu if full else nu - 1)
            z = z0 + (z1 - z0) * j / (nv - 1)
            d = Vector((math.sin(a), math.cos(a), 0))
            return Vector((0, cy, z)) + d * 0.5, -d

        return self.shell(part, rays, nu, nv, offset, thickness, colour, wrap_u=full, weights=weights)

    def panel(self, side, x0, x1, z0, z1, offset, thickness, colour, nu=7, nv=6, cut=0.0,
              part="torso", weights=None):
        """Front (side=+1) or back (side=-1) panel over x0..x1, z0..z1.
        `cut` trims the top corners (shooter's cut), as a fraction of width."""
        def rays(i, j):
            v = j / (nv - 1)
            z = z0 + (z1 - z0) * v
            k = 1.0 - cut * _smooth01((v - 0.6) / 0.4)
            xm = (x0 + x1) / 2
            x = xm + ((x0 + (x1 - x0) * i / (nu - 1)) - xm) * k
            return Vector((x, 0.6 * side, z)), Vector((0, -side, 0))

        return self.shell(part, rays, nu, nv, offset, thickness, colour, weights=weights)


# =================================================================== pieces


def build(ch, s):
    """Build every gear piece named in the config into one skinned object."""
    g = Gear(ch, s)
    gear = ch.cfg["gear"]
    m = ch.marks
    measure(g)
    helmet(g, gear["helmet"])
    if gear["headset"]:
        headset(g, gear["helmet"])
    eyewear(g, gear["eyewear"], gear["helmet"])
    if gear["neck"] == "gaiter":
        g.limb_band("head", Vector((0, m.neck.y, m.collar_z - 0.015)), Vector((0, 0.05, 1)), 0.0,
                    m.eye.z - 0.10 - m.collar_z, 0.006, 0.006, g.c["gear_alt"], nu=16, nv=3)
    uniform_details(g, gear)
    belt(g)
    carrier(g, gear["carrier"])
    molle(g)
    for name in gear["pouches"]:
        if name not in POUCHES:
            raise ValueError(f"{ch.cfg['path']}: unknown pouch {name!r} (have {sorted(POUCHES)})")
        POUCHES[name](g)
    for name in gear["pads"]:
        {"knees": knee_pads, "elbows": elbow_pads}[name](g)
    if gear["gloves"] != "none":
        gloves(g)
    boots(g, gear["boots"])
    return g.to_object("gear", ch.materials)


def measure(g):
    """Head and torso sizes used by several pieces."""
    m, s = g.m, g.s
    head_pts = [s.verts[vi] for p, b in zip(s.all_polys, s.poly_bone) if b == "head" for vi in p]
    upper = [p for p in head_pts if p.z > m.eye.z]
    ys = [p.y for p in upper]
    g.head_centre = Vector((0.0, (min(ys) + max(ys)) / 2, m.eye.z + 0.015))
    ear = [p for p in head_pts if m.eye.z - 0.06 < p.z < m.eye.z + 0.01 and abs(p.y - g.head_centre.y) < 0.04]
    g.ear_x = max(abs(p.x) for p in ear) if ear else 0.08
    g.head_back = min(ys)
    # Torso half-width and depth at chest height.
    z = m.chest.z + 0.12
    left = s.cast("torso", Vector((-0.6, m.chest.y, z)), Vector((1, 0, 0)))
    front = s.cast("torso", Vector((0, 0.6, z)), Vector((0, -1, 0)))
    back = s.cast("torso", Vector((0, -0.6, z)), Vector((0, 1, 0)))
    g.chest_half = (0.6 - left[2]) if left else 0.17
    g.chest_front = (0.6 - front[2]) if front else 0.12
    g.chest_back = -(0.6 - back[2]) if back else -0.12
    # Carrier extents (heights), shared by pouches.
    g.plate_top = m.neck.z - 0.07
    g.plate_bottom = m.waist_z + 0.07


# ----------------------------------------------------------------- head

HELMET_CUTS = {
    # Rim height above eye level (m) by angle from the front (degrees).
    "highcut": [(0, 0.042), (50, 0.04), (72, 0.062), (110, 0.052), (140, 0.0), (180, -0.035)],
    "full": [(0, 0.036), (45, 0.024), (75, -0.03), (120, -0.05), (180, -0.06)],
    "cap": [(0, 0.052), (60, 0.04), (95, 0.032), (180, -0.012)],
}


def _head_shell(g, kind, offset, thickness, colour, rim_drop=0.0, nu=24, nv=7):
    c = g.head_centre
    table = HELMET_CUTS[kind]
    radius = 0.11

    def rays(i, j):
        phi = -math.pi + 2 * math.pi * i / nu
        rim = _lerp_table(table, abs(math.degrees(phi))) - rim_drop
        rim_z = g.m.eye.z + rim
        lo = math.asin(max(-0.95, min(0.95, (rim_z - c.z) / radius)))
        hi = math.radians(78)
        t = j / (nv - 1)
        theta = lo + (hi - lo) * (1 - (1 - t) ** 1.4)
        d = Vector((math.sin(phi) * math.cos(theta), math.cos(phi) * math.cos(theta), math.sin(theta)))
        return c + d * 0.4, -d

    return g.shell("head", rays, nu, nv, offset, thickness, colour, wrap_u=True, cap_top=True,
                   weights={"head": 1.0}, smooth_iters=3)


def _on_shell(g, phi_deg, rise, extra):
    """Point and outward normal on the head surface, `extra` m off it, at
    angle phi (degrees from the front) and `rise` m above eye level."""
    phi = math.radians(phi_deg)
    c = g.head_centre
    target = Vector((math.sin(phi) * 0.1, math.cos(phi) * 0.1, g.m.eye.z + rise - c.z)) + c
    d = (target - c).normalized()
    return g.surface("head", c + d * 0.4, -d, extra)


def helmet(g, kind):
    col = g.c
    if kind == "none":
        return
    if kind not in HELMET_CUTS:
        raise ValueError(f"unknown helmet {kind!r}")
    if kind == "cap":
        _head_shell(g, "cap", 0.004, 0.006, col["gear_alt"])
        # Peak: one curved plate following the rim, sloping down a little.
        rim = _lerp_table(HELMET_CUTS["cap"], 0)
        top, bottom = [], []
        n_phi, n_out = 9, 3
        for i in range(n_phi):
            phi = -62 + 124 * i / (n_phi - 1)
            q, qn = _on_shell(g, phi, rim + 0.002, 0.008)
            if q is None:
                continue
            out = Vector((qn.x, qn.y, 0)).normalized()
            reach = 0.065 * math.cos(math.radians(phi) * 0.8)
            row_t, row_b = [], []
            for j in range(n_out):
                t = j / (n_out - 1)
                pt = q + out * (reach * t) + Vector((0, 0, -0.022 * t * t))
                row_t.append(g.bm.verts.new(pt))
                row_b.append(g.bm.verts.new(pt + Vector((0, 0, -0.006))))
            top.append(row_t)
            bottom.append(row_b)
        faces = []
        for i in range(len(top) - 1):
            for j in range(n_out - 1):
                faces.append(g.bm.faces.new((top[i][j], top[i + 1][j], top[i + 1][j + 1], top[i][j + 1])))
                faces.append(g.bm.faces.new((bottom[i][j + 1], bottom[i + 1][j + 1], bottom[i + 1][j], bottom[i][j])))
            for j in (0, n_out - 1):
                faces.append(g.bm.faces.new((top[i][j], bottom[i][j], bottom[i + 1][j], top[i + 1][j])))
        for i in (0, len(top) - 1):
            for j in range(n_out - 1):
                faces.append(g.bm.faces.new((top[i][j], top[i][j + 1], bottom[i][j + 1], bottom[i][j])))
        g.finish(faces, col["gear_alt"], weights={"head": 1.0}, smooth=True)
        return
    shell_off, thick = (0.02, 0.012) if kind == "highcut" else (0.024, 0.014)
    _head_shell(g, kind, shell_off, thick, col["helmet"])
    out = shell_off + thick
    # Front mount (shroud) above the brow.
    p, n = _on_shell(g, 0, 0.075, out)
    if p is not None:
        f = _frame(n)
        f.translation = p + n * 0.008
        g.box(f, (0.045, 0.016, 0.03), col["metal"], weights={"head": 1.0}, bevel=0.003)
    # Rear counterweight / battery pouch.
    p, n = _on_shell(g, 180, 0.04, out)
    if p is not None:
        f = _frame(n)
        f.translation = p + n * 0.017
        g.box(f, (0.07, 0.034, 0.045), col["gear"], weights={"head": 1.0}, bevel=0.005)
    # Side rails.
    for side in (-1, 1):
        for k in range(3):
            phi = side * (72 + 18 * k)
            rise = _lerp_table(HELMET_CUTS[kind], abs(phi)) + 0.022
            p, n = _on_shell(g, phi, rise, out)
            if p is None:
                continue
            f = _frame(n)
            f.translation = p + n * 0.005
            g.box(f, (0.034, 0.01, 0.016), col["metal"], weights={"head": 1.0})
    # Chin strap: thin strips from the rim at the ears down under the jaw.
    for side in (-1, 1):
        top, n_top = _on_shell(g, side * 95, 0.0, 0.004)
        chin, n_chin = _on_shell(g, side * 40, -0.115, 0.004)
        if top is None or chin is None:
            continue
        mid = (top + chin) / 2
        length = (top - chin).length
        axis = (top - chin).normalized()
        n = (n_top + n_chin).normalized()
        x = axis.cross(n).normalized()
        frame = Matrix((x, n, axis)).transposed().to_4x4()
        frame.translation = mid
        g.box(frame, (0.016, 0.004, length), col["straps"], weights={"head": 1.0})
    if kind == "full":
        # Mandible guard: a curved plate around the jaw, ear to ear.
        c = g.head_centre
        lo, hi = g.m.eye.z - 0.15, g.m.eye.z - 0.085

        def rays(i, j):
            phi = math.radians(-72 + 144 * i / 10)
            z = lo + (hi - lo) * j / 2
            d = Vector((math.sin(phi), math.cos(phi), 0))
            o = Vector((0, c.y - 0.01, z))
            return o + d * 0.4, -d

        g.shell("head", rays, 11, 3, 0.02, 0.01, col["helmet"], weights={"head": 1.0}, smooth_iters=3)
        # Visor, raised: smoked lens across the brow.
        def vrays(i, j):
            phi = math.radians(-62 + 124 * i / 8)
            rise = 0.05 + 0.04 * j / 1
            d = Vector((math.sin(phi), math.cos(phi), (g.m.eye.z + rise - c.z) / 0.1)).normalized()
            return c + d * 0.4, -d

        g.shell("head", vrays, 9, 2, out + 0.004, 0.006, col["dark"], weights={"head": 1.0})


def headset(g, helmet_kind):
    """Ear cups over the ears and a boom mic on the left."""
    m, col = g.m, g.c
    z = m.eye.z - 0.025
    y = g.head_centre.y - 0.005
    for side in (-1, 1):
        f = Matrix.Rotation(math.pi / 2, 4, "Y")
        f.translation = Vector((side * (g.ear_x + 0.012), y, z))
        g.cylinder(f, 0.042, 0.03, col["dark"], weights={"head": 1.0}, segments=14)
        f2 = Matrix.Rotation(math.pi / 2, 4, "Y")
        f2.translation = Vector((side * (g.ear_x + 0.03), y, z))
        g.cylinder(f2, 0.032, 0.01, col["gear"], weights={"head": 1.0}, segments=12)
    # Boom mic from the left cup towards the mouth.
    start = Vector((-(g.ear_x + 0.02), y + 0.02, z - 0.015))
    end = Vector((-0.035, m.eye.y + 0.015, m.eye.z - 0.085))
    axis = (end - start)
    f = axis.to_track_quat("Z", "Y").to_matrix().to_4x4()
    f.translation = (start + end) / 2
    g.cylinder(f, 0.0035, axis.length, col["dark"], weights={"head": 1.0}, segments=6)
    if helmet_kind in ("none", "cap"):
        # Headband over the top.
        c = g.head_centre

        def rays(i, j):
            a = math.radians(-80 + 160 * i / 10)
            yy = y + (j - 0.5) * 0.03
            d = Vector((math.sin(a), 0, math.cos(a)))
            return Vector((0, yy, c.z)) + d * 0.4, -d

        g.shell("head", rays, 11, 2, 0.012 if helmet_kind == "cap" else 0.006, 0.006, col["dark"],
                weights={"head": 1.0})


def eyewear(g, kind, helmet_kind):
    col, m = g.c, g.m
    if kind == "none":
        return
    c = g.head_centre
    if kind == "goggles_up":
        # Goggles pushed up onto the helmet brow: frame, two lenses, strap.
        off = 0.036 if helmet_kind in ("highcut", "full") else 0.012
        rim = _lerp_table(HELMET_CUTS.get(helmet_kind, HELMET_CUTS["cap"]), 0)

        def band(phi0, phi1, r0, r1, n):
            def rays(i, j):
                phi = math.radians(phi0 + (phi1 - phi0) * i / (n - 1))
                rise = rim + r0 + (r1 - r0) * j
                d = Vector((math.sin(phi), math.cos(phi), (m.eye.z + rise - c.z) / 0.1)).normalized()
                return c + d * 0.4, -d
            return rays

        g.shell("head", band(-55, 55, 0.012, 0.042, 9), 9, 2, off, 0.01, col["dark"], weights={"head": 1.0})
        for side in (-1, 1):
            g.shell("head", band(side * 6, side * 46, 0.017, 0.037, 5), 5, 2, off + 0.008, 0.004,
                    col["lens"], weights={"head": 1.0})
        g.shell("head", band(55, 305, 0.02, 0.036, 13), 13, 2, off - 0.006, 0.004, col["straps"],
                weights={"head": 1.0})
    elif kind == "glasses":
        # Wrap-around lens with a thin top bar, arms back to the ears.
        def band(phi0, phi1, z0, z1):
            def rays(i, j):
                phi = math.radians(phi0 + (phi1 - phi0) * i / 10)
                z = m.eye.z + z0 + (z1 - z0) * j
                d = Vector((math.sin(phi), math.cos(phi), 0))
                return Vector((0, c.y, z)) + d * 0.4, -d
            return rays

        g.shell("head", band(-56, 56, -0.013, 0.011), 11, 2, 0.015, 0.003, col["lens"],
                weights={"head": 1.0}, smooth_iters=3)
        g.shell("head", band(-60, 60, 0.009, 0.015), 11, 2, 0.014, 0.005, col["dark"],
                weights={"head": 1.0}, smooth_iters=3)
        for side in (-1, 1):
            front, _n = _on_shell(g, side * 60, 0.012, 0.016)
            back = Vector((side * (g.ear_x + 0.004), g.head_centre.y - 0.01, m.eye.z + 0.01))
            if front is None:
                continue
            axis = back - front
            f = axis.to_track_quat("Z", "X").to_matrix().to_4x4()
            f.translation = (front + back) / 2
            g.box(f, (0.004, 0.006, axis.length), col["dark"], weights={"head": 1.0})
    else:
        raise ValueError(f"unknown eyewear {kind!r}")


# ----------------------------------------------------------------- torso


def uniform_details(g, gear):
    """Combat-shirt sleeve pockets (blank hook-and-loop panels, no
    insignia) and trouser cargo pockets."""
    m, col = g.m, g.c
    for s in "lr":
        sign = -1 if s == "l" else 1
        sh, el = m.shoulder[s], m.elbow[s]
        d = (el - sh).normalized()
        out = Vector((sign, 0.15, 0))
        out = (out - d * out.dot(d)).normalized()
        side = d.cross(out).normalized()

        def rays(i, j, sh=sh, d=d, out=out, side=side):
            p = sh + d * (0.07 + 0.1 * j / 3) + side * (i / 3 - 0.5) * 0.075
            return p + out * 0.3, -out

        g.shell("arms_" + s, rays, 4, 4, 0.004, 0.007, col["shirt"], weights={f"upperarm_{s}": 1.0})
        p, n = g.surface("arms_" + s, sh + d * 0.1 + out * 0.3, -out, 0.012)
        if p is not None:
            f = Matrix((side, n, d)).transposed().to_4x4()
            f.translation = p + n * 0.001
            g.box(f, (0.05, 0.002, 0.035), col["gear_alt"], weights={f"upperarm_{s}": 1.0})
    for s in "lr":
        if s == "r" and "thigh_holster" in gear["pouches"]:
            continue
        sign = -1 if s == "l" else 1
        hip, knee = m.hip[s], m.knee[s]
        d = (knee - hip).normalized()
        out = Vector((sign, 0.2, 0))
        out = (out - d * out.dot(d)).normalized()
        side = d.cross(out).normalized()

        def rays(i, j, hip=hip, d=d, out=out, side=side, length=(knee - hip).length):
            p = hip + d * (length * (0.38 + 0.3 * j / 3)) + side * (i / 3 - 0.5) * 0.13
            return p + out * 0.3, -out

        w = {f"thigh_{s}": 1.0}
        g.shell("legs_" + s, rays, 4, 4, 0.004, 0.012, col["trousers"], weights=w)
        # Flap over the top of the pocket.
        def flap(i, j, hip=hip, d=d, out=out, side=side, length=(knee - hip).length):
            p = hip + d * (length * (0.36 + 0.06 * j)) + side * (i / 3 - 0.5) * 0.14
            return p + out * 0.3, -out

        g.shell("legs_" + s, flap, 4, 2, 0.016, 0.004, col["trousers"], weights=w)


def molle(g):
    """Rows of webbing across the front and back of plate carriers."""
    col = g.c
    for attr, side in (("carrier_front", 1), ("carrier_back", -1)):
        mount = getattr(g, attr, None)
        if mount is None:
            continue
        z0, z1, w, extra = mount
        rows = 4 if side < 0 else 3
        for k in range(rows):
            zc = z0 + 0.03 + k * 0.038
            g.panel(side, -w * 0.9, w * 0.9, zc, zc + 0.022, extra, 0.003, col["straps"], nu=7, nv=2)


def belt(g):
    m, col = g.m, g.c
    g.torso_band(m.waist_z - 0.035, m.waist_z + 0.022, 0.006, 0.012, col["straps"], nu=28, nv=2)
    f = Matrix.Identity(4)
    p, n = g.surface("torso", Vector((0, 0.6, m.waist_z - 0.007)), Vector((0, -1, 0)), 0.018)
    if p is not None:
        f = _frame(n)
        f.translation = p + n * 0.005
        g.box(f, (0.06, 0.012, 0.045), col["metal"], bevel=0.003)


def carrier(g, kind):
    m, col = g.m, g.c
    if kind == "none":
        return
    w = g.chest_half * 0.78
    top, bot = g.plate_top, g.plate_bottom
    if kind == "chest_rig":
        # No plates: a pouch panel on the belly, shoulder straps crossing
        # into an X on the back.
        z0, z1 = m.waist_z + 0.07, m.waist_z + 0.22
        g.panel(1, -w * 0.95, w * 0.95, z0, z1, 0.01, 0.022, col["gear"], nu=7, nv=4)
        g.rig_front = (z0, z1, w * 0.95)
        for side in (-1, 1):
            _shoulder_strap(g, side * 0.095, 0.04, col["straps"], front_z=z1, back_z=top)
            # Back diagonal: from this shoulder to the opposite hip.
            def rays(i, j, side=side):
                t = j / 5
                x = side * 0.09 * (1 - t) - side * 0.11 * t + (i - 0.5) * 0.04
                z = top + 0.02 + (z0 - top - 0.02) * t
                return Vector((x, -0.6, z)), Vector((0, 1, 0))
            g.shell("torso", rays, 2, 6, 0.012, 0.008, col["straps"])
        return
    heavy = kind == "heavy"
    if kind not in ("plate", "heavy"):
        raise ValueError(f"unknown carrier {kind!r}")
    thick = 0.05 if heavy else 0.04
    g.panel(1, -w, w, bot, top, 0.012, thick, col["gear"], cut=0.28)
    g.panel(-1, -w, w, bot, top + 0.02, 0.012, thick - 0.008, col["gear"], cut=0.2)
    g.carrier_front = (bot, top, w, 0.012 + thick)
    g.carrier_back = (bot, top + 0.02, w, 0.012 + thick - 0.008)
    # Cummerbund round the sides.
    cb_top = bot + (0.24 if heavy else 0.16)
    for side in (-1, 1):
        g.torso_band(bot, cb_top, 0.012, 0.03 if heavy else 0.02, col["gear"],
                     phi0=side * math.radians(40), phi1=side * math.radians(140), nu=7, nv=3)
        # Side-release buckle where it meets the front plate.
        a = math.radians(side * 46)
        d = Vector((math.sin(a), math.cos(a), 0))
        f = g.place("torso", Vector((0, 0, bot + 0.05)) + d * 0.6, -d, 0.012 + (0.03 if heavy else 0.02), 0.012)
        g.box(f, (0.035, 0.012, 0.03), col["dark"], bevel=0.003)
    for side in (-1, 1):
        _shoulder_strap(g, side * 0.1, 0.05 if heavy else 0.045, col["gear"], front_z=top, back_z=top + 0.02,
                        thickness=0.02 if heavy else 0.014)
    if heavy:
        # Collar guard round the neck, lower at the throat.
        c = m.neck

        def rays(i, j):
            a = -math.pi + 2 * math.pi * i / 20
            front = _smooth01(1 - abs(a) / 1.2)
            z = m.collar_z - 0.03 + (0.07 - 0.04 * front) * j / 2
            d = Vector((math.sin(a), math.cos(a), 0))
            return Vector((0, c.y - 0.01, z)) + d * 0.4, -d

        g.shell("torso", rays, 20, 3, 0.03, 0.022, col["gear"], wrap_u=True)
        # Shoulder guards over the deltoids.
        for s in "lr":
            _pauldron(g, s)
        # Groin flap.
        g.panel(1, -0.075, 0.075, m.pelvis.z - 0.14, m.waist_z - 0.03, 0.02, 0.018, col["gear"], nu=4, nv=3,
                weights={"pelvis": 1.0})


def _shoulder_strap(g, x, width, colour, front_z, back_z, thickness=0.012):
    m = g.m
    centre = Vector((x, m.chest.y, m.shoulder["r"].z - 0.12))

    def rays(i, j):
        a = math.radians(14 + 152 * j / 8)
        d = Vector((0, math.cos(a), math.sin(a)))
        o = centre + Vector(((i - 0.5) * width, 0, 0))
        return o + d * 0.45, -d

    g.shell("torso", rays, 2, 9, 0.012, thickness, colour, smooth_iters=2)


def _pauldron(g, s):
    """Shoulder guard: a shell over the deltoid and the top of the shoulder."""
    m, col = g.m, g.c
    sign = -1 if s == "l" else 1
    sh = m.shoulder[s]
    arm_dir = (m.elbow[s] - sh).normalized()
    centre = sh + arm_dir * 0.04 + Vector((0, 0, -0.02))
    side = Vector((sign, 0, 0))
    up = Vector((0, 0, 1))
    fwd = Vector((0, 1, 0))

    def rays(i, j):
        a = math.radians(-75 + 150 * i / 6)   # front to back around the shoulder
        e = math.radians(5 + 70 * j / 4)      # from the side of the arm up to the top
        d = (side * math.cos(e) + up * math.sin(e)) * math.cos(a) + fwd * math.sin(a)
        return centre + d.normalized() * 0.4, -d.normalized()

    g.shell("all", rays, 7, 5, 0.016, 0.013, col["gear"], weights={f"upperarm_{s}": 0.8, f"clavicle_{s}": 0.2},
            smooth_iters=3)


# ---------------------------------------------------------------- pouches


def _front_mount(g):
    """Surface used for front pouches: (z_bottom, z_top, half width, extra)."""
    if hasattr(g, "carrier_front"):
        return g.carrier_front
    if hasattr(g, "rig_front"):
        z0, z1, w = g.rig_front
        return (z0, z1, w, 0.032)
    return None


def mags_triple(g):
    col = g.c
    mount = _front_mount(g)
    if mount is None:
        return
    z0, z1, w, extra = mount
    count = 4 if hasattr(g, "rig_front") else 3
    zc = z0 + (0.075 if hasattr(g, "rig_front") else 0.08)
    for k in range(count):
        x = (k - (count - 1) / 2) * 0.074
        f = g.place("torso", Vector((x, 0.6, zc)), Vector((0, -1, 0)), extra, 0.036)
        g.box(f, (0.066, 0.036, 0.13), col["gear_alt"], bevel=0.006)
        # Magazine top showing above the pouch, and the pouch's flap.
        mag = f @ Matrix.Translation((0, -0.004, 0.072))
        g.box(mag, (0.05, 0.022, 0.03), col["dark"])
        flap = f @ Matrix.Translation((0, 0.02, 0.045))
        g.box(flap, (0.06, 0.006, 0.04), col["straps"])


def admin(g):
    col = g.c
    mount = _front_mount(g)
    if mount is None or not hasattr(g, "carrier_front"):
        return
    z0, z1, w, extra = mount
    f = g.place("torso", Vector((0, 0.6, z1 - 0.075)), Vector((0, -1, 0)), extra, 0.024)
    g.box(f, (0.17, 0.024, 0.09), col["gear"], bevel=0.005)
    # A blank ID panel (team marker colour).
    g.box(f @ Matrix.Translation((0.04, 0.013, 0.01)), (0.06, 0.003, 0.035), col["accent"])


def radio_left(g):
    """Radio on the back left, antenna up over the shoulder."""
    col, m = g.c, g.m
    x = -0.085
    z = (g.plate_top if hasattr(g, "carrier_back") else m.chest.z + 0.16) - 0.08
    extra = g.carrier_back[3] if hasattr(g, "carrier_back") else 0.02
    f = g.place("torso", Vector((x, -0.6, z)), Vector((0, 1, 0)), extra, 0.045)
    g.box(f, (0.065, 0.045, 0.15), col["gear"], bevel=0.006)
    g.box(f @ Matrix.Translation((0, 0.005, 0.085)), (0.03, 0.03, 0.02), col["dark"])
    ant = f @ Matrix.Translation((0.008, 0.0, 0.25)) @ Matrix.Rotation(math.radians(-14), 4, "Y")
    g.cylinder(ant, 0.004, 0.32, col["dark"], segments=5, radius2=0.002)


def dump_left(g):
    """Rolled dump pouch on the left hip."""
    col, m = g.c, g.m
    a = math.radians(-115)
    d = Vector((math.sin(a), math.cos(a), 0))
    f = g.place("torso", Vector((0, 0, m.waist_z - 0.06)) + d * 0.6, -d, 0.02, 0.06)
    g.box(f, (0.11, 0.06, 0.13), col["gear_alt"], bevel=0.012, taper=0.9)


def grenades(g):
    col, m = g.c, g.m
    for k, deg in enumerate((32, 50)):
        a = math.radians(deg)
        d = Vector((math.sin(a), math.cos(a), 0))
        f = g.place("torso", Vector((0, 0, m.waist_z - 0.03)) + d * 0.6, -d, 0.02, 0.06)
        g.cylinder(f, 0.028, 0.08, col["gear"], segments=10)
        g.cylinder(f @ Matrix.Translation((0, 0, 0.05)), 0.012, 0.02, col["metal"], segments=6)


def med_pouch(g):
    col, m = g.c, g.m
    f = g.place("torso", Vector((0, -0.6, m.waist_z - 0.02)), Vector((0, 1, 0)), 0.02, 0.05)
    g.box(f, (0.15, 0.05, 0.08), col["gear_alt"], bevel=0.008)


def _back_mount(g):
    if hasattr(g, "carrier_back"):
        z0, z1, w, extra = g.carrier_back
        return z0, z1, extra
    return g.m.waist_z + 0.08, g.plate_top, 0.03


def breach_pack(g):
    """Auger: pack with the Ring Cutter disc strapped to its back."""
    col = g.c
    z0, z1, extra = _back_mount(g)
    zc = (z0 + z1) / 2 + 0.02
    f = g.place("torso", Vector((0, -0.6, zc)), Vector((0, 1, 0)), extra, 0.13, up=Vector((0, 0.15, 1)))
    g.box(f, (0.29, 0.13, 0.36), col["gear_alt"], bevel=0.02)
    g.box(f @ Matrix.Translation((0, 0.02, 0.19)), (0.27, 0.11, 0.03), col["gear"], bevel=0.008)
    # The cutter: a thick disc with a hub and three clamp arms.
    disc = f @ Matrix.Translation((0, 0.08, 0.0)) @ Matrix.Rotation(-math.pi / 2, 4, "X")
    g.cylinder(disc, 0.135, 0.03, col["metal"], segments=24)
    g.cylinder(disc @ Matrix.Translation((0, 0, 0.017)), 0.128, 0.006, col["accent"], segments=24)
    g.cylinder(disc @ Matrix.Translation((0, 0, 0.03)), 0.04, 0.035, col["dark"], segments=12)
    for k in range(3):
        a = 2 * math.pi * k / 3 + 0.3
        arm = disc @ Matrix.Rotation(a, 4, "Z") @ Matrix.Translation((0.09, 0, 0.022))
        g.box(arm, (0.1, 0.02, 0.012), col["dark"])


def hydration(g):
    col = g.c
    z0, z1, extra = _back_mount(g)
    f = g.place("torso", Vector((0, -0.6, (z0 + z1) / 2)), Vector((0, 1, 0)), extra, 0.05)
    g.box(f, (0.21, 0.05, 0.34), col["gear_alt"], bevel=0.015, taper=0.92)


def stake_quiver(g):
    """Fathom: tube bag of seismic stakes across the back, tops showing."""
    col, m = g.c, g.m
    z0, z1, extra = _back_mount(g)
    f = g.place("torso", Vector((0.06, -0.6, (z0 + z1) / 2)), Vector((0, 1, 0)), extra, 0.1)
    f = f @ Matrix.Rotation(math.radians(-22), 4, "Y")
    g.cylinder(f, 0.05, 0.42, col["gear_alt"], segments=12)
    g.cylinder(f @ Matrix.Translation((0, 0, 0.215)), 0.052, 0.012, col["straps"], segments=12)
    for k in range(3):
        a = 2 * math.pi * k / 3
        t = f @ Matrix.Translation((0.024 * math.cos(a), 0.024 * math.sin(a), 0.27))
        g.cylinder(t, 0.009, 0.1, col["metal"], segments=6, radius2=0.006)
        g.cylinder(t @ Matrix.Translation((0, 0, 0.055)), 0.014, 0.012, col["accent"], segments=6)


def charge_cases(g):
    """Mortise: three flat charge cases stacked on the back plate."""
    col = g.c
    z0, z1, extra = _back_mount(g)
    for k in range(3):
        z = z1 - 0.07 - k * 0.1
        f = g.place("torso", Vector((0, -0.6, z)), Vector((0, 1, 0)), extra, 0.04)
        g.box(f, (0.2, 0.04, 0.09), col["gear_alt"], bevel=0.008)
        g.box(f @ Matrix.Translation((0.07, 0.021, 0)), (0.03, 0.004, 0.06), col["accent"])


def dart_launcher(g):
    """Lark: wrist launcher on the left forearm, tube along the arm."""
    col, m = g.c, g.m
    s = "l"
    d = m.forearm_dir[s]
    p = m.elbow[s] + (m.wrist[s] - m.elbow[s]) * 0.62
    out = Vector((-1, 0.25, 0)).normalized()
    out = (out - d * out.dot(d)).normalized()
    surf, n = g.surface("arms_l", p + out * 0.3, -out, 0.01)
    if surf is None:
        surf, n = p + out * 0.04, out
    y = n
    z = d
    x = y.cross(z).normalized()
    f = Matrix((x, y, z)).transposed().to_4x4()
    f.translation = surf + n * 0.018
    w = {"lowerarm_l": 1.0}
    g.box(f, (0.05, 0.036, 0.11), col["gear"], weights=w, bevel=0.005)
    tube = f @ Matrix.Translation((0, 0.026, 0.01))
    g.cylinder(tube, 0.012, 0.13, col["metal"], weights=w, segments=8)
    g.box(f @ Matrix.Translation((0, 0.02, -0.03)), (0.03, 0.006, 0.02), col["accent"], weights=w)


def thigh_holster(g):
    """Drop-leg platform with a holstered sidearm on the right thigh."""
    col, m = g.c, g.m
    s = "r"
    hip, knee = m.hip[s], m.knee[s]
    d = (knee - hip).normalized()
    p = hip + (knee - hip) * 0.38
    out = Vector((1, 0.1, 0)).normalized()
    out = (out - d * out.dot(d)).normalized()
    surf, n = g.surface("legs_r", p + out * 0.3, -out, 0.012)
    if surf is None:
        return
    x = n.cross(-d).normalized()
    f = Matrix((x, n, -d)).transposed().to_4x4()
    f.translation = surf + n * 0.02
    w = {f"thigh_{s}": 1.0}
    g.box(f, (0.08, 0.04, 0.17), col["gear"], weights=w, bevel=0.006)
    g.box(f @ Matrix.Translation((0, 0.008, 0.1)) @ Matrix.Rotation(math.radians(-15), 4, "X"),
          (0.03, 0.03, 0.08), col["dark"], weights=w)
    for t in (0.3, 0.55):
        g.limb_band("legs_r", hip, d, (knee - hip).length * t, (knee - hip).length * t + 0.035, 0.012, 0.006,
                    col["straps"], nu=12, weights=w)


POUCHES = {
    "mags_triple": mags_triple,
    "admin": admin,
    "radio_left": radio_left,
    "dump_left": dump_left,
    "grenades": grenades,
    "med_pouch": med_pouch,
    "breach_pack": breach_pack,
    "hydration": hydration,
    "stake_quiver": stake_quiver,
    "charge_cases": charge_cases,
    "dart_launcher": dart_launcher,
    "thigh_holster": thigh_holster,
}


# ------------------------------------------------------------------ limbs


def knee_pads(g):
    col, m = g.c, g.m
    for s in "lr":
        knee, ankle, hip = m.knee[s], m.ankle[s], m.hip[s]

        def rays(i, j, knee=knee):
            x = knee.x + (i / 4 - 0.5) * 0.11
            z = knee.z - 0.07 + 0.15 * j / 4
            return Vector((x, knee.y + 0.5, z)), Vector((0, -1, 0))

        w = {f"calf_{s}": 0.7, f"thigh_{s}": 0.3}
        g.shell("legs_" + s, rays, 5, 5, 0.008, 0.022, col["pads"], weights=w, smooth_iters=3)
        shin = (ankle - knee).normalized()
        g.limb_band("legs_" + s, knee, shin, 0.06, 0.085, 0.01, 0.006, col["straps"], nu=12, weights={f"calf_{s}": 1.0})


def elbow_pads(g):
    col, m = g.c, g.m
    for s in "lr":
        sign = -1 if s == "l" else 1
        elbow = m.elbow[s]
        back = Vector((sign * 0.4, -1, 0)).normalized()

        def rays(i, j, elbow=elbow, back=back):
            side = back.cross(Vector((0, 0, 1))).normalized()
            o = elbow + side * (i / 3 - 0.5) * 0.08 + Vector((0, 0, (j / 3 - 0.5) * 0.1))
            return o + back * 0.4, -back

        g.shell("arms_" + s, rays, 4, 4, 0.006, 0.018, col["pads"], weights={f"lowerarm_{s}": 0.5, f"upperarm_{s}": 0.5})


def gloves(g):
    col, m = g.c, g.m
    for s in "lr":
        d = m.forearm_dir[s]
        g.limb_band("arms_" + s, m.wrist[s], d, -0.045, -0.012, 0.004, 0.006, col["gloves"], nu=12,
                    weights={f"hand_{s}": 0.5, f"lowerarm_{s}": 0.5})
        # Knuckle guard on the back of the hand (outside, in the A-pose).
        sign = -1 if s == "l" else 1
        hand_dir = (g.ch.rig.data.bones[f"hand_{s}"].tail_local - m.wrist[s]).normalized()
        out = Vector((sign, 0, 0))
        out = (out - hand_dir * out.dot(hand_dir)).normalized()
        p = m.wrist[s] + hand_dir * 0.075
        surf, n = g.surface("arms_" + s, p + out * 0.3, -out, 0.001)
        if surf is None:
            continue
        x = n.cross(hand_dir).normalized()
        f = Matrix((x, n, hand_dir)).transposed().to_4x4()
        f.translation = surf + n * 0.004
        g.box(f, (0.05, 0.008, 0.026), col["dark"], weights={f"hand_{s}": 1.0}, bevel=0.002)


def boots(g, kind):
    """Sole slab under each (already coloured) boot and a collar at the top."""
    col, m, s = g.c, g.m, g.s
    top = body_mod.BOOT_TOP[kind]
    for side in "lr":
        sign = -1 if side == "l" else 1
        pts = [s.verts[vi] for p, b in zip(s.all_polys, s.poly_bone)
               if b.startswith(("foot_" + side, "ball_" + side, "calf_" + side)) for vi in p]
        low = [p for p in pts if p.z < 0.06 and p.x * sign > 0]
        if len(low) < 4:
            continue
        outline = _hull_2d([(p.x, p.y) for p in low])
        cx = sum(x for x, _ in outline) / len(outline)
        cy = sum(y for _, y in outline) / len(outline)
        grown = []
        for x, y in outline:
            v = Vector((x - cx, y - cy))
            grow = 0.012 if v.length > 1e-4 else 0
            v2 = v + v.normalized() * grow if v.length > 1e-4 else v
            grown.append((cx + v2.x, cy + v2.y))
        faces = _prism(g, grown, -0.002, 0.03)
        g.finish(faces, col["sole"])
        # Collar at the top of the shaft.
        calf = m.knee[side]
        shin = (m.ankle[side] - calf).normalized()
        dist = (calf.z - top) / max(1e-3, -shin.z)
        g.limb_band("legs_" + side, calf, shin, dist - 0.02, dist + 0.006, 0.012, 0.008, col["boots"], nu=12)


def _prism(g, outline, z0, z1):
    bm = g.bm
    lo = [bm.verts.new((x, y, z0)) for x, y in outline]
    hi = [bm.verts.new((x, y, z1)) for x, y in outline]
    n = len(outline)
    faces = [bm.faces.new(list(reversed(lo))), bm.faces.new(hi)]
    for i in range(n):
        j = (i + 1) % n
        faces.append(bm.faces.new((lo[i], lo[j], hi[j], hi[i])))
    return faces


def _hull_2d(points):
    pts = sorted(set((round(x, 5), round(y, 5)) for x, y in points))
    if len(pts) < 3:
        return pts

    def cross(o, a, b):
        return (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])

    lower, upper = [], []
    for p in pts:
        while len(lower) >= 2 and cross(lower[-2], lower[-1], p) <= 0:
            lower.pop()
        lower.append(p)
    for p in reversed(pts):
        while len(upper) >= 2 and cross(upper[-2], upper[-1], p) <= 0:
            upper.pop()
        upper.append(p)
    return lower[:-1] + upper[:-1]
