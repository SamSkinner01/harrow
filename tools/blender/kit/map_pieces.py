"""Pieces added for the full Harrow Keep map: curtain walls, exterior stairs,
arcades, furniture, room dressing and desert props.

Same conventions as pieces.py (game space, 1 m grid, exterior +Z, props
centred on their origin standing on y = 0). Registered from
pieces.all_pieces().
"""

import math
import random

from .pieces import H, P, SLAB, STEP_RISE, STEP_RUN, STEPS, Piece, stone_wall_arch

# --- Helpers -----------------------------------------------------------------


def _normal(pts):
    """Newell normal of a polygon given as (x, y, z) tuples."""
    nx = ny = nz = 0.0
    for i, a in enumerate(pts):
        b = pts[(i + 1) % len(pts)]
        nx += (a[1] - b[1]) * (a[2] + b[2])
        ny += (a[2] - b[2]) * (a[0] + b[0])
        nz += (a[0] - b[0]) * (a[1] + b[1])
    return (nx, ny, nz)


def _face(builder, pts, material, out):
    """Add a face, flipping its winding so it faces direction `out`."""
    n = _normal(pts)
    if n[0] * out[0] + n[1] * out[1] + n[2] * out[2] < 0:
        pts = list(reversed(pts))
    builder.face(pts, material)


def _centroid(pts):
    k = len(pts)
    return tuple(sum(p[i] for p in pts) / k for i in range(3))


def _hexa_faces(builder, corners, material, top=None):
    """Six faces of a convex 8-corner solid (collider_hexa corner order)."""
    b0, b1, b2, b3, t0, t1, t2, t3 = corners
    c = _centroid(corners)
    quads = [
        ([b3, b2, t2, t3], material),
        ([b1, b0, t0, t1], material),
        ([b2, b1, t1, t2], material),
        ([b0, b3, t3, t0], material),
        ([t3, t2, t1, t0], top or material),
        ([b0, b1, b2, b3], material),
    ]
    for quad, mat in quads:
        f = _centroid(quad)
        _face(builder, quad, mat, (f[0] - c[0], f[1] - c[1], f[2] - c[2]))


def box(p, mn, mx, material, collide=True, top=None):
    """Axis-aligned box; `top` overrides the material of the +Y face."""
    if top is None:
        p.visual.box(mn, mx, material)
    else:
        (x0, y0, z0), (x1, y1, z1) = mn, mx
        corners = [
            (x0, y0, z0), (x1, y0, z0), (x1, y0, z1), (x0, y0, z1),
            (x0, y1, z0), (x1, y1, z0), (x1, y1, z1), (x0, y1, z1),
        ]
        _hexa_faces(p.visual, corners, material, top)
    if collide:
        p.collider_box(mn, mx)


def hexa(p, corners, material, collide=True):
    _hexa_faces(p.visual, corners, material)
    if collide:
        p.collider_hexa(corners)


def ngon(p, cx, cz, r, y0, y1, n, material, collide=False, phase=0.0, caps=True):
    """Vertical n-sided prism (barrels, well, posts)."""
    pts = [
        (cx + r * math.cos(phase + 2 * math.pi * i / n), cz + r * math.sin(phase + 2 * math.pi * i / n))
        for i in range(n)
    ]

    def build(builder, mat):
        for i in range(n):
            (ax, az), (bx, bz) = pts[i], pts[(i + 1) % n]
            quad = [(ax, y0, az), (bx, y0, bz), (bx, y1, bz), (ax, y1, az)]
            mx, mz = (ax + bx) / 2 - cx, (az + bz) / 2 - cz
            _face(builder, quad, mat, (mx, 0.0, mz))
        if caps:
            _face(builder, [(x, y1, z) for x, z in pts], mat, (0, 1, 0))
            _face(builder, [(x, y0, z) for x, z in pts], mat, (0, -1, 0))

    build(p.visual, material)
    if collide:
        from .geometry import MeshBuilder

        col = MeshBuilder([])
        build(col, "collider")
        p.colliders.append(col)


def ngon_x(p, cy, cz, r, x0, x1, n, material):
    """Horizontal n-sided prism along X (cart wheels, axles)."""
    pts = [(cy + r * math.cos(2 * math.pi * i / n), cz + r * math.sin(2 * math.pi * i / n)) for i in range(n)]
    for i in range(n):
        (ay, az), (by, bz) = pts[i], pts[(i + 1) % n]
        quad = [(x0, ay, az), (x0, by, bz), (x1, by, bz), (x1, ay, az)]
        _face(p.visual, quad, material, (0.0, (ay + by) / 2 - cy, (az + bz) / 2 - cz))
    _face(p.visual, [(x1, y, z) for y, z in pts], material, (1, 0, 0))
    _face(p.visual, [(x0, y, z) for y, z in pts], material, (-1, 0, 0))


# --- Curtain walls -------------------------------------------------------------

CURTAIN_HALF = 1.5  # 3 m thick
CURTAIN_TOP = 6.4  # wall-walk level (two storeys)
PARAPET_T = 0.5
CRENEL_Y = CURTAIN_TOP + 1.0  # top of the parapet between merlons
MERLON_Y = CURTAIN_TOP + 1.6


def curtain_wall(length):
    """Solid 3 m thick curtain wall, 6.4 m to the paved wall-walk, with a
    crenellated parapet on the exterior (+Z) side. Stops P short of each
    grid point like other walls; `curtain_post` fills junctions. Sinks
    0.2 m into the ground so its foot never shows a gap."""
    p = Piece(f"curtain_wall_{length}m")
    z0, z1 = -CURTAIN_HALF, CURTAIN_HALF
    box(p, (P, -SLAB, z0), (length - P, CURTAIN_TOP, z1), "stone", top="flagstone")
    box(p, (P, CURTAIN_TOP, z1 - PARAPET_T), (length - P, CRENEL_Y, z1), "stone")
    for c in range(2, length, 2):
        box(p, (c - P, CRENEL_Y, z1 - PARAPET_T), (c + P, MERLON_Y, z1), "stone")
    return p


def curtain_post():
    p = Piece("curtain_post")
    z0, z1 = -CURTAIN_HALF, CURTAIN_HALF
    box(p, (-P, -SLAB, z0), (P, CURTAIN_TOP, z1), "stone", top="flagstone")
    box(p, (-P, CURTAIN_TOP, z1 - PARAPET_T), (P, MERLON_Y, z1), "stone")
    return p


def cap_slab():
    """1 cm paving slab over a junction cap where a roof edge has no merlon
    (wall-walk entries): the cap and the floor tile both end at floor level
    there, and this keeps them from flickering."""
    p = Piece("cap_slab")
    box(p, (-P, 0, -P), (P, 0.01, P), "flagstone", collide=False)
    return p


def arcade_arch():
    """4 m bay with a wide round arch (2.8 m opening): arcades and stables."""
    p = stone_wall_arch(4, opening=2.8, spring=1.4)
    p.name = "arcade_arch_4m"
    return p


def pier():
    """Free-standing square stone pier with base and capital, 3 m tall."""
    p = Piece("pier")
    box(p, (-0.36, 0, -0.36), (0.36, 0.15, 0.36), "stone")
    box(p, (-0.3, 0.15, -0.3), (0.3, 2.8, 0.3), "stone")
    box(p, (-0.36, 2.8, -0.36), (0.36, H, 0.36), "stone")
    return p


def floor_stone_4m():
    p = Piece("floor_stone_4m")
    box(p, (0, -SLAB, 0), (4, 0, 4), "flagstone")
    return p


# --- Exterior stairs -------------------------------------------------------------

STRING_X0, STRING_X1 = 2 - P, 2.0  # side wall of exterior stairs (+x side)
RAIL_H = 0.9


def _ramp(p, y_base=0.0):
    """The stairs' walking ramp, as in pieces.stairs()."""
    x0, x1 = P, 2 - P
    top_y = STEP_RISE * STEPS
    toe_z, head_z = STEP_RUN, -STEP_RUN * (STEPS - 1)
    d = 0.15
    p.collider_hexa(
        [
            (x0, top_y - d, head_z), (x1, top_y - d, head_z), (x1, -d, toe_z), (x0, -d, toe_z),
            (x0, top_y, head_z), (x1, top_y, head_z), (x1, 0, toe_z), (x0, 0, toe_z),
        ]
    )
    p.collider_box((x0, top_y - SLAB, -STEP_RUN * STEPS), (x1, top_y, head_z))


def stairs_ext():
    """Exterior stone stairs: one storey (16 steps), solid underneath, with a
    sloping side wall on the open (+x) side. Steps x 0.25..1.75 (against a
    wall at x = 0), rising towards -Z from z = 0; ramp collider as for
    stairs_3_2m."""
    p = Piece("stairs_ext_3_2m")
    x0, x1 = P, 2 - P
    for i in range(STEPS):
        p.visual.box((x0, 0, -STEP_RUN * (i + 1)), (x1, STEP_RISE * (i + 1), -STEP_RUN * i), "stone")
    _ramp(p)
    length = STEP_RUN * STEPS
    top = STEP_RISE * STEPS
    hexa(
        p,
        [
            (STRING_X0, 0, -length), (STRING_X1, 0, -length), (STRING_X1, 0, 0), (STRING_X0, 0, 0),
            (STRING_X0, top + RAIL_H, -length), (STRING_X1, top + RAIL_H, -length),
            (STRING_X1, RAIL_H, 0), (STRING_X0, RAIL_H, 0),
        ],
        "stone",
    )
    return p


def stair_base():
    """Solid masonry under an upper exterior flight (same footprint, 3.2 m)."""
    p = Piece("stair_base_3_2m")
    box(p, (P, 0, -STEP_RUN * STEPS), (STRING_X1, STEP_RISE * STEPS, 0), "stone")
    return p


def stair_landing(name, rail_side=True, rail_end=False):
    """2 m long masonry landing (x 0.25..2, z -2..0, 3.2 m tall) for
    exterior stairs, optionally with a parapet on +x and across the far end."""
    p = Piece(name)
    top = STEP_RISE * STEPS
    box(p, (P, 0, -2), (STRING_X1, top, 0), "stone")
    if rail_side:
        box(p, (STRING_X0, top, -2), (STRING_X1, top + RAIL_H, 0), "stone")
    if rail_end:
        box(p, (P, top, -2), (STRING_X0, top + RAIL_H, -2 + P), "stone")
    return p


# --- Doors, windows, ceilings ------------------------------------------------------


def door_leaf():
    """Plank door standing open, for stone_wall_door_2m (same origin): hinged
    at the left jamb and swung 90 degrees into the -Z side."""
    p = Piece("door_leaf")
    x0, x1 = 0.46, 0.50
    z0, z1 = -0.25 - 1.06, -0.25
    box(p, (x0, 0.02, z0), (x1, 2.16, z1), "wood")
    for y in (0.3, 1.0, 1.75):
        box(p, (x1, y, z0 + 0.08), (x1 + 0.03, y + 0.12, z1 - 0.06), "wood", collide=False)
    box(p, (x1, 0.4, z0 + 0.12), (x1 + 0.025, 1.8, z0 + 0.22), "wood", collide=False)
    box(p, (x0 - 0.005, 1.0, z0 + 0.06), (x1 + 0.04, 1.06, z0 + 0.14), "metal", collide=False)
    return p


def shutters():
    """A pair of plank shutters folded open flat against the exterior (+Z)
    face beside a stone_wall_window_2m (same origin). Decoration only."""
    p = Piece("shutters_2m")
    z0, z1 = 0.255, 0.29
    for x0, x1 in ((0.0, 0.48), (1.52, 2.0)):
        box(p, (x0, 1.0, z0), (x1, 2.1, z1), "wood", collide=False)
        for y in (1.15, 1.85):
            box(p, (x0 + 0.03, y, z1), (x1 - 0.03, y + 0.08, z1 + 0.02), "wood", collide=False)
    return p


def beam(length):
    """Timber ceiling beam hanging from y = 0 (place at the slab underside),
    along x 0..length. Decoration only."""
    p = Piece(f"beam_{length}m")
    box(p, (0, -0.22, -0.09), (length, 0, 0.09), "wood", collide=False)
    return p


# --- Furniture and room dressing ----------------------------------------------------


def table():
    p = Piece("table_2m")
    box(p, (-1.0, 0.74, -0.45), (1.0, 0.8, 0.45), "wood")
    for x in (-0.9, 0.82):
        for z in (-0.38, 0.3):
            box(p, (x, 0, z), (x + 0.08, 0.74, z + 0.08), "wood")
    box(p, (-0.86, 0.15, -0.03), (0.86, 0.21, 0.03), "wood", collide=False)
    return p


def bench():
    p = Piece("bench_2m")
    box(p, (-1.0, 0.42, -0.17), (1.0, 0.47, 0.17), "wood", collide=False)
    for x in (-0.8, 0.74):
        box(p, (x, 0, -0.15), (x + 0.06, 0.42, 0.15), "wood", collide=False)
    p.collider_box((-1.0, 0, -0.17), (1.0, 0.47, 0.17))
    return p


def shelf():
    """2 m wide, 2 m tall shelving, back at z = 0, depth to z = -0.45."""
    p = Piece("shelf_2m")
    rng = random.Random("shelf")
    d = -0.45
    box(p, (-1.0, 0, d), (-0.96, 2.0, 0), "wood", collide=False)
    box(p, (0.96, 0, d), (1.0, 2.0, 0), "wood", collide=False)
    box(p, (-0.96, 0, -0.03), (0.96, 2.0, 0), "wood", collide=False)
    for y in (0.05, 0.5, 0.95, 1.4, 1.96):
        box(p, (-0.96, y, d), (0.96, y + 0.04, -0.03), "wood", collide=False)
    for y in (0.09, 0.54, 0.99, 1.44):
        x = -0.92
        while x < 0.85:
            w = rng.uniform(0.15, 0.35)
            if rng.random() < 0.75:
                h = rng.uniform(0.15, 0.35)
                mat = rng.choice(["burlap", "wood", "metal"])
                box(p, (x, y, d + 0.05), (min(x + w, 0.92), y + h, -0.06), mat, collide=False)
            x += w + 0.04
    p.collider_box((-1.0, 0, d), (1.0, 2.0, 0))
    return p


def bunk_bed():
    """Two-tier timber bunk, 0.9 x 2.0 m, centred."""
    p = Piece("bunk_bed")
    hx, hz = 0.45, 1.0
    for x in (-hx, hx - 0.07):
        for z in (-hz, hz - 0.07):
            box(p, (x, 0, z), (x + 0.07, 1.75, z + 0.07), "wood", collide=False)
    for y in (0.35, 1.3):
        box(p, (-hx, y, -hz), (hx, y + 0.08, hz), "wood", collide=False)
        box(p, (-hx + 0.04, y + 0.08, -hz + 0.04), (hx - 0.04, y + 0.2, hz - 0.04), "burlap", collide=False)
    box(p, (-hx, 1.55, -hz), (-hx + 0.07, 1.62, hz), "wood", collide=False)
    p.collider_box((-hx, 0, -hz), (hx, 1.75, hz))
    return p


def barrel():
    p = Piece("barrel")
    ngon(p, 0, 0, 0.29, 0, 0.9, 10, "wood", collide=True)
    for y in (0.12, 0.74):
        ngon(p, 0, 0, 0.30, y, y + 0.04, 10, "metal", caps=False)
    return p


def weapon_rack():
    """Wall rack with long arms standing in it; back at z = 0."""
    p = Piece("weapon_rack")
    d = -0.35
    box(p, (-1.0, 0, d), (1.0, 0.12, 0), "wood", collide=False)
    box(p, (-1.0, 1.1, d), (1.0, 1.16, 0), "wood", collide=False)
    box(p, (-1.0, 0, -0.04), (-0.94, 1.5, 0), "wood", collide=False)
    box(p, (0.94, 0, -0.04), (1.0, 1.5, 0), "wood", collide=False)
    for i in range(7):
        x = -0.78 + i * 0.26
        box(p, (x - 0.025, 0.12, d + 0.12), (x + 0.025, 1.25, d + 0.2), "metal", collide=False)
        box(p, (x - 0.035, 0.12, d + 0.08), (x + 0.035, 0.5, d + 0.24), "wood", collide=False)
    p.collider_box((-1.0, 0, d), (1.0, 1.5, 0))
    return p


def hearth():
    """Kitchen fireplace against a wall (back at z = 0), 2.4 m wide, with a
    hood up to the ceiling."""
    p = Piece("hearth")
    d = -1.0
    box(p, (-1.2, 0, d), (-0.8, 1.2, 0), "stone", collide=False)
    box(p, (0.8, 0, d), (1.2, 1.2, 0), "stone", collide=False)
    box(p, (-0.8, 0, -0.3), (0.8, 1.2, 0), "stone", collide=False)
    box(p, (-0.8, 0, d), (0.8, 0.25, -0.3), "stone", collide=False)
    box(p, (-1.25, 1.2, d - 0.05), (1.25, 1.35, 0), "wood", collide=False)
    hexa(
        p,
        [
            (-1.2, 1.35, d), (1.2, 1.35, d), (1.2, 1.35, 0), (-1.2, 1.35, 0),
            (-0.7, H, -0.55), (0.7, H, -0.55), (0.7, H, 0), (-0.7, H, 0),
        ],
        "stone",
        collide=False,
    )
    for i, x in enumerate((-0.5, -0.15, 0.2, 0.45)):
        box(p, (x, 0.25, -0.75 + 0.05 * i), (x + 0.25, 0.33, -0.65 + 0.05 * i), "wood", collide=False)
    p.collider_box((-1.2, 0, d), (1.2, 1.35, 0))
    p.collider_box((-0.7, 1.35, -0.55), (0.7, H, 0))
    return p


def well():
    """Stone well with a timber frame and pulley, centred."""
    p = Piece("well")
    ngon(p, 0, 0, 0.95, 0, 0.85, 12, "stone", collide=True, caps=False)
    ngon(p, 0, 0, 0.72, 0.85, 0.86, 12, "stone", caps=False)
    # Rim cap: a ring of quads between the outer and inner radius.
    n = 12
    outer = [(0.95 * math.cos(2 * math.pi * i / n), 0.95 * math.sin(2 * math.pi * i / n)) for i in range(n)]
    inner = [(0.72 * math.cos(2 * math.pi * i / n), 0.72 * math.sin(2 * math.pi * i / n)) for i in range(n)]
    for i in range(n):
        j = (i + 1) % n
        quad = [(outer[i][0], 0.85, outer[i][1]), (outer[j][0], 0.85, outer[j][1]),
                (inner[j][0], 0.85, inner[j][1]), (inner[i][0], 0.85, inner[i][1])]
        _face(p.visual, quad, "stone", (0, 1, 0))
        quad = [(inner[i][0], 0.85, inner[i][1]), (inner[j][0], 0.85, inner[j][1]),
                (inner[j][0], 0.1, inner[j][1]), (inner[i][0], 0.1, inner[i][1])]
        _face(p.visual, quad, "stone", (-(inner[i][0] + inner[j][0]), 0, -(inner[i][1] + inner[j][1])))
    box(p, (-0.72, 0.08, -0.72), (0.72, 0.1, 0.72), "metal", collide=False)
    for x in (-0.95, 0.85):
        box(p, (x, 0, -0.06), (x + 0.1, 2.2, 0.06), "wood")
    box(p, (-1.0, 2.0, -0.07), (1.0, 2.12, 0.07), "wood", collide=False)
    ngon_x(p, 1.85, 0.0, 0.12, -0.2, 0.2, 8, "wood")
    box(p, (-0.01, 0.9, -0.01), (0.01, 1.75, 0.01), "burlap", collide=False)
    box(p, (-0.12, 0.65, -0.12), (0.12, 0.9, 0.12), "wood", collide=False)
    return p


def market_stall():
    """Timber stall, 3 x 2 m, counter at the front (+Z), canvas roof."""
    p = Piece("market_stall")
    hx, hz = 1.5, 1.0
    for x in (-hx, hx - 0.1):
        box(p, (x, 0, hz - 0.1), (x + 0.1, 2.4, hz), "wood")
        box(p, (x, 0, -hz), (x + 0.1, 2.1, -hz + 0.1), "wood")
    box(p, (-hx + 0.1, 0, hz - 0.6), (hx - 0.1, 0.9, hz - 0.1), "wood")
    box(p, (-hx, 0.9, hz - 0.65), (hx, 0.95, hz - 0.05), "wood", collide=False)
    hexa(
        p,
        [
            (-hx - 0.1, 2.08, -hz - 0.15), (hx + 0.1, 2.08, -hz - 0.15),
            (hx + 0.1, 2.38, hz + 0.25), (-hx - 0.1, 2.38, hz + 0.25),
            (-hx - 0.1, 2.12, -hz - 0.15), (hx + 0.1, 2.12, -hz - 0.15),
            (hx + 0.1, 2.42, hz + 0.25), (-hx - 0.1, 2.42, hz + 0.25),
        ],
        "fabric",
        collide=False,
    )
    rng = random.Random("stall")
    for i in range(5):
        x = -1.2 + i * 0.55
        box(p, (x, 0.95, hz - 0.55), (x + 0.4, 0.95 + rng.uniform(0.12, 0.3), hz - 0.2),
            rng.choice(["burlap", "wood"]), collide=False)
    return p


def cart():
    """Two-wheeled timber hand cart, bed 2.2 x 1.2 m, shafts towards -X."""
    p = Piece("cart")
    box(p, (-1.0, 0.55, -0.6), (1.2, 0.65, 0.6), "wood", collide=False)
    for z0, z1 in ((-0.6, -0.55), (0.55, 0.6)):
        box(p, (-1.0, 0.65, z0), (1.2, 0.95, z1), "wood", collide=False)
    box(p, (1.15, 0.65, -0.55), (1.2, 0.95, 0.55), "wood", collide=False)
    for z in (-0.45, 0.39):
        box(p, (-2.4, 0.5, z), (-1.0, 0.56, z + 0.06), "wood", collide=False)
    for z0 in (-0.78, 0.68):
        n = 12
        pts = [(0.2 + 0.5 * math.cos(2 * math.pi * i / n), 0.5 + 0.5 * math.sin(2 * math.pi * i / n)) for i in range(n)]
        for i in range(n):
            (ax, ay), (bx, by) = pts[i], pts[(i + 1) % n]
            quad = [(ax, ay, z0), (bx, by, z0), (bx, by, z0 + 0.1), (ax, ay, z0 + 0.1)]
            _face(p.visual, quad, "wood", ((ax + bx) / 2 - 0.2, (ay + by) / 2 - 0.5, 0))
        _face(p.visual, [(x, y, z0 + 0.1) for x, y in pts], "wood", (0, 0, 1))
        _face(p.visual, [(x, y, z0) for x, y in pts], "wood", (0, 0, -1))
    box(p, (0.17, 0.47, -0.7), (0.23, 0.53, 0.7), "metal", collide=False)
    p.collider_box((-1.0, 0, -0.78), (1.2, 0.95, 0.78))
    return p


def rubble(name, size, count, seed):
    """A heap of fallen stone blocks within a size x size footprint."""
    p = Piece(name)
    rng = random.Random(seed)
    for i in range(count):
        w, d, h = rng.uniform(0.25, 0.6), rng.uniform(0.2, 0.5), rng.uniform(0.15, 0.4)
        r = size / 2 * math.sqrt(rng.random())
        a = rng.uniform(0, 2 * math.pi)
        x, z = r * math.cos(a), r * math.sin(a)
        y = max(0.0, (1 - r / (size / 2)) * 0.6 * rng.random())
        box(p, (x - w / 2, y, z - d / 2), (x + w / 2, y + h, z + d / 2), "stone", collide=y + h > 0.35)
    return p


def rock(name, radius, height, seed):
    """Irregular boulder: a noisy subdivided octahedron, sunk 0.15 m."""
    p = Piece(name)
    rng = random.Random(seed)
    # Octahedron subdivided twice.
    verts = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)]
    faces = [(0, 2, 4), (4, 2, 1), (1, 2, 5), (5, 2, 0), (0, 4, 3), (4, 1, 3), (1, 5, 3), (5, 0, 3)]
    for _ in range(2):
        cache = {}
        new_faces = []

        def mid(a, b):
            key = (min(a, b), max(a, b))
            if key not in cache:
                va, vb = verts[a], verts[b]
                m = [(va[i] + vb[i]) / 2 for i in range(3)]
                length = math.sqrt(sum(c * c for c in m))
                verts.append(tuple(c / length for c in m))
                cache[key] = len(verts) - 1
            return cache[key]

        for a, b, c in faces:
            ab, bc, ca = mid(a, b), mid(b, c), mid(c, a)
            new_faces += [(a, ab, ca), (b, bc, ab), (c, ca, bc), (ab, bc, ca)]
        faces = new_faces
    noise = [rng.uniform(0.82, 1.12) for _ in verts]
    pts = []
    for (x, y, z), k in zip(verts, noise):
        yy = y * height * k
        if yy < 0:
            yy *= 0.3
        pts.append((x * radius * k, yy + height * 0.12 - 0.15, z * radius * k * rng.uniform(0.85, 1.0)))
    from .geometry import MeshBuilder

    col = MeshBuilder([])
    for a, b, c in faces:
        tri = [pts[a], pts[b], pts[c]]
        cen = _centroid(tri)
        _face(p.visual, tri, "rock", cen)
        _face(col, tri, "collider", cen)
    p.colliders.append(col)
    return p


def dune(name, half_x, half_z, height):
    """Smooth convex sand mound (a paraboloid cap), edges 0.3 m underground."""
    p = Piece(name)
    from .geometry import MeshBuilder

    nx, nz = 12, 8
    sink = 0.3

    def h(u, v):
        return height * (1 - u * u - v * v) - sink

    grid = {}
    for i in range(nx + 1):
        for j in range(nz + 1):
            u, v = -1 + 2 * i / nx, -1 + 2 * j / nz
            # Squash the grid to the ellipse so the rim is round.
            s = max(abs(u), abs(v))
            r = math.hypot(u, v)
            k = s / r if r > 0 else 0
            uu, vv = u * k, v * k
            grid[(i, j)] = (uu * half_x, h(uu, vv), vv * half_z)
    col = MeshBuilder([])
    for i in range(nx):
        for j in range(nz):
            quad = [grid[(i, j)], grid[(i + 1, j)], grid[(i + 1, j + 1)], grid[(i, j + 1)]]
            _face(p.visual, quad, "sand", (0, 1, 0))
            _face(col, quad, "collider", (0, 1, 0))
    p.colliders.append(col)
    return p


def rug():
    """Woven rug, 2 x 3 m, 12 mm thick. Decoration only."""
    p = Piece("rug_2x3")
    box(p, (-1.0, 0, -1.5), (1.0, 0.012, 1.5), "fabric", collide=False)
    return p


def tomb():
    """Stone sarcophagus with an overhanging lid, 2.2 x 0.9 m."""
    p = Piece("tomb")
    box(p, (-1.05, 0, -0.42), (1.05, 0.75, 0.42), "stone")
    box(p, (-1.12, 0.75, -0.48), (1.12, 0.92, 0.48), "stone")
    return p


def generator():
    """Field generator on skids, 1.6 x 0.9 m."""
    p = Piece("generator")
    box(p, (-0.8, 0, -0.45), (0.8, 0.1, 0.45), "metal", collide=False)
    box(p, (-0.75, 0.1, -0.4), (0.75, 0.95, 0.4), "metal", collide=False)
    box(p, (-0.6, 0.95, -0.25), (0.1, 1.05, 0.25), "metal", collide=False)
    box(p, (0.35, 0.95, -0.06), (0.45, 1.4, 0.06), "metal", collide=False)
    p.collider_box((-0.8, 0, -0.45), (0.8, 1.05, 0.45))
    return p


def radio_set():
    """Stack of field radio cases (sits on a table top), 0.9 m wide."""
    p = Piece("radio_set")
    box(p, (-0.45, 0, -0.2), (0.0, 0.28, 0.2), "metal", collide=False)
    box(p, (0.02, 0, -0.18), (0.42, 0.22, 0.18), "metal", collide=False)
    box(p, (-0.4, 0.28, -0.16), (-0.05, 0.45, 0.16), "metal", collide=False)
    box(p, (-0.3, 0.45, -0.01), (-0.28, 1.1, 0.01), "metal", collide=False)
    return p


def track(length):
    """Strip of gravel track, x 0..length, z -2..2, a single top face."""
    p = Piece(f"track_{length}m")
    _face(p.visual, [(0, 0, -2), (length, 0, -2), (length, 0, 2), (0, 0, 2)], "gravel", (0, 1, 0))
    return p


# --- Registry -----------------------------------------------------------------------


def all_map_pieces():
    pieces = [curtain_post(), cap_slab()]
    for length in (1, 2, 4):
        pieces.append(curtain_wall(length))
    pieces += [
        arcade_arch(),
        pier(),
        floor_stone_4m(),
        stairs_ext(),
        stair_base(),
        stair_landing("stair_pier_2m", rail_side=False),
        stair_landing("stair_landing_2m"),
        stair_landing("stair_landing_end_2m", rail_end=True),
        door_leaf(),
        shutters(),
        beam(4),
        table(),
        bench(),
        shelf(),
        bunk_bed(),
        barrel(),
        weapon_rack(),
        hearth(),
        well(),
        market_stall(),
        cart(),
        rubble("rubble_pile", 2.0, 18, "rubble_pile"),
        rubble("rubble_small", 1.2, 8, "rubble_small"),
        rock("rock_large", 1.4, 1.5, "rock_large"),
        rock("rock_medium", 0.8, 0.9, "rock_medium"),
        rock("rock_small", 0.45, 0.5, "rock_small"),
        dune("dune_large", 16.0, 7.0, 2.6),
        dune("dune_small", 8.0, 4.0, 1.3),
        rug(),
        tomb(),
        generator(),
        radio_set(),
        track(8),
    ]
    return pieces
