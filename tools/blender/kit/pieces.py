"""Fortress kit piece definitions.

Conventions (game space, metres; see geometry.py):

- 1 m grid. A storey is 3.2 m: 3.0 m of wall on top of a 0.2 m floor slab.
- Wall pieces run along +X from one grid point (x = 0) to the next
  (x = length), centred on z = 0, standing on y = 0. They stop 0.25 m short of
  each grid point; a 0.5 m stone post fills every junction. This way no two
  pieces ever overlap, so nothing flickers.
- Exterior side of a wall is +Z.
- Floor tiles cover x 0..2, z 0..2 with their top at y = 0.
- Props are centred on their origin, standing on y = 0.
- `COL_*` objects are invisible convex colliders. `FRAG_*` objects are the
  pieces of a pre-broken prop or barricade.
- Each piece's main object carries a `surface` extra: "hard" (indestructible)
  or "soft" (destructible, M4).
"""

import random

from .geometry import MeshBuilder, arch_outline

H = 3.0  # wall height, floor top to slab underside
SLAB = 0.2  # floor slab thickness
T = 0.5  # stone wall thickness
ST = 0.15  # soft (plaster on lath) wall thickness
P = T / 2  # walls stop this far from each grid point


class Piece:
    def __init__(self, name, surface="hard"):
        self.name = name
        self.surface = surface
        self.visual = MeshBuilder([])
        self.colliders = []
        self.fragments = []

    def solid(self, mn, mx, material):
        """Visible box that is also collision."""
        self.visual.box(mn, mx, material)
        self.collider_box(mn, mx)

    def collider_box(self, mn, mx):
        b = MeshBuilder([])
        b.box(mn, mx, "collider")
        self.colliders.append(b)

    def collider_hexa(self, corners):
        """Convex 8-corner collider (e.g. a ramp). Corners in box order:
        bottom face (y0) x0z0, x1z0, x1z1, x0z1, then the same for the top."""
        b = MeshBuilder([])
        b0, b1, b2, b3, t0, t1, t2, t3 = corners
        for quad in (
            [b3, b2, t2, t3],  # +Z side
            [b1, b0, t0, t1],  # -Z side
            [b2, b1, t1, t2],  # +X side
            [b0, b3, t3, t0],  # -X side
            [t3, t2, t1, t0],  # top
            [b0, b1, b2, b3],  # bottom
        ):
            b.face(quad, "collider")
        self.colliders.append(b)

    def fragment(self, mn, mx, material):
        b = MeshBuilder([])
        b.box(mn, mx, material)
        self.fragments.append(b)


# --- Stone (indestructible) --------------------------------------------------


def stone_post():
    p = Piece("stone_post")
    p.solid((-P, 0, -P), (P, H, P), "stone")
    return p


def stone_cap():
    """Fills a junction in the 0.2 m floor-slab layer of an exterior wall."""
    p = Piece("stone_cap")
    p.solid((-P, 0, -P), (P, SLAB, P), "stone")
    return p


def stone_band(length):
    """Outer half of the slab layer on an exterior wall (also used as a
    plinth below ground-floor walls)."""
    p = Piece(f"stone_band_{length}m")
    p.solid((P, 0, 0), (length - P, SLAB, P), "stone")
    return p


def stone_wall(length):
    p = Piece(f"stone_wall_{length}m")
    p.solid((P, 0, -T / 2), (length - P, H, T / 2), "stone")
    return p


def stone_wall_door():
    """2 m wall with a 1.1 x 2.2 m doorway."""
    p = Piece("stone_wall_door_2m")
    z = (-T / 2, T / 2)
    p.solid((P, 0, z[0]), (0.45, H, z[1]), "stone")
    p.solid((1.55, 0, z[0]), (2 - P, H, z[1]), "stone")
    p.solid((0.45, 2.2, z[0]), (1.55, H, z[1]), "stone")
    # Timber lintel showing on both faces, just proud of the stone.
    p.visual.box((0.35, 2.2, z[0] - 0.01), (1.65, 2.38, z[1] + 0.01), "wood")
    return p


def stone_wall_window():
    """2 m wall with a 1.0 x 1.1 m window, sill at 1.0 m."""
    p = Piece("stone_wall_window_2m")
    z = (-T / 2, T / 2)
    p.solid((P, 0, z[0]), (0.5, H, z[1]), "stone")
    p.solid((1.5, 0, z[0]), (2 - P, H, z[1]), "stone")
    p.solid((0.5, 0, z[0]), (1.5, 0.95, z[1]), "stone")
    p.solid((0.5, 2.1, z[0]), (1.5, H, z[1]), "stone")
    # Timber sill, slightly deeper than the wall.
    p.solid((0.45, 0.95, z[0] - 0.03), (1.55, 1.0, z[1] + 0.03), "wood")
    return p


def stone_wall_arch(length, opening, spring):
    """Wall with a round-arched opening reaching the floor."""
    p = Piece(f"stone_wall_arch_{length}m")
    span = length - 2 * P
    outline = [(x + P, y) for x, y in arch_outline(span, opening, spring, H)]
    p.visual.prism_xy(outline, -T / 2, T / 2, "stone")
    cx, r = length / 2, opening / 2
    z = (-T / 2, T / 2)
    p.collider_box((P, 0, z[0]), (cx - r, H, z[1]))
    p.collider_box((cx + r, 0, z[0]), (length - P, H, z[1]))
    # Approximate the curve: solid from 70% of the way up the arch.
    p.collider_box((cx - r, spring + 0.7 * r, z[0]), (cx + r, H, z[1]))
    return p


def parapet(length):
    p = Piece(f"parapet_{length}m")
    p.solid((P, 0, -T / 2), (length - P, 1.0, T / 2), "stone")
    return p


def merlon():
    """Taller post for parapet junctions: gives the roofline battlements."""
    p = Piece("merlon")
    p.solid((-P, 0, -P), (P, 1.4, P), "stone")
    return p


# --- Soft walls (destructible) -------------------------------------------------


def soft_wall(length):
    p = Piece(f"soft_wall_{length}m", "soft")
    p.solid((P, 0, -ST / 2), (length - P, H, ST / 2), "plaster")
    return p


def soft_wall_door():
    """2 m plaster wall with a 1.0 x 2.1 m doorway."""
    p = Piece("soft_wall_door_2m", "soft")
    z = (-ST / 2, ST / 2)
    p.solid((P, 0, z[0]), (0.5, H, z[1]), "plaster")
    p.solid((1.5, 0, z[0]), (2 - P, H, z[1]), "plaster")
    p.solid((0.5, 2.1, z[0]), (1.5, H, z[1]), "plaster")
    return p


def soft_wall_window():
    """2 m plaster wall with a 1.0 x 1.1 m window, sill at 1.0 m."""
    p = Piece("soft_wall_window_2m", "soft")
    z = (-ST / 2, ST / 2)
    p.solid((P, 0, z[0]), (0.5, H, z[1]), "plaster")
    p.solid((1.5, 0, z[0]), (2 - P, H, z[1]), "plaster")
    p.solid((0.5, 0, z[0]), (1.5, 1.0, z[1]), "plaster")
    p.solid((0.5, 2.1, z[0]), (1.5, H, z[1]), "plaster")
    return p


# --- Floors, hatches, stairs, railings ---------------------------------------


def floor(name, material, surface):
    p = Piece(name, surface)
    p.solid((0, -SLAB, 0), (2, 0, 2), material)
    return p


HATCH_OPEN = (0.55, 1.45)  # hole in the hatch tile
HATCH_FRAME = 0.04


def floor_hatch():
    """Wooden floor tile with a hatch opening and a metal frame."""
    p = Piece("floor_hatch_2m", "soft")
    a, b = HATCH_OPEN
    y = (-SLAB, 0)
    p.solid((0, y[0], 0), (2, y[1], a), "wood")
    p.solid((0, y[0], b), (2, y[1], 2), "wood")
    p.solid((0, y[0], a), (a, y[1], b), "wood")
    p.solid((b, y[0], a), (2, y[1], b), "wood")
    f = HATCH_FRAME
    p.solid((a, y[0], a), (b, y[1], a + f), "metal")
    p.solid((a, y[0], b - f), (b, y[1], b), "metal")
    p.solid((a, y[0], a + f), (a + f, y[1], b - f), "metal")
    p.solid((b - f, y[0], a + f), (b, y[1], b - f), "metal")
    return p


def hatch_cover():
    """Boards closing a hatch; same origin as the hatch tile."""
    p = Piece("hatch_cover", "soft")
    a, b = HATCH_OPEN[0] + HATCH_FRAME, HATCH_OPEN[1] - HATCH_FRAME
    p.solid((a, -0.06, a), (b, 0, b), "wood")
    return p


STEP_RISE, STEP_RUN, STEPS = 0.2, 0.3, 16  # 3.2 m up over 4.8 m


def stairs():
    """One storey of stone stairs, 1.5 m wide (fits a 2 m bay against a wall
    at x = 0). Rises towards -Z from z = 0. Walks on an invisible ramp
    through the step nosings; the ramp starts 0.3 m in front of the first step.
    The stairwell above must leave 1.2 m of headroom: see DESIGN.md."""
    p = Piece("stairs_3_2m")
    x0, x1 = P, 2 - P
    for i in range(STEPS):
        top = STEP_RISE * (i + 1)
        p.visual.box((x0, 0, -STEP_RUN * (i + 1)), (x1, top, -STEP_RUN * i), "stone")
    top_y = STEP_RISE * STEPS
    toe_z, head_z = STEP_RUN, -STEP_RUN * (STEPS - 1)
    d = 0.15  # ramp thickness
    # Corners: min z is the top of the stairs (head), max z the toe.
    p.collider_hexa(
        [
            # Bottom face: the walking surface lowered by d.
            (x0, top_y - d, head_z), (x1, top_y - d, head_z), (x1, -d, toe_z), (x0, -d, toe_z),
            # Top face: the walking surface.
            (x0, top_y, head_z), (x1, top_y, head_z), (x1, 0, toe_z), (x0, 0, toe_z),
        ]
    )
    # Last step is flat and level with the floor above.
    p.collider_box((x0, top_y - SLAB, -STEP_RUN * STEPS), (x1, top_y, head_z))
    return p


def railing():
    """2 m wooden railing along x 0..2 at z = 0, 1 m high."""
    p = Piece("railing_2m")
    for x in (0.0, 0.97, 1.94):
        p.visual.box((x, 0, -0.03), (x + 0.06, 0.95, 0.03), "wood")
    p.visual.box((0, 0.95, -0.04), (2, 1.02, 0.04), "wood")
    p.visual.box((0.06, 0.48, -0.025), (0.97, 0.53, 0.025), "wood")
    p.visual.box((1.03, 0.48, -0.025), (1.94, 0.53, 0.025), "wood")
    p.collider_box((0, 0, -0.05), (2, 1.02, 0.05))
    return p


# --- Props ---------------------------------------------------------------------


def crate(name, size, broken=False):
    """Wooden crate with corner battens. The broken version is a pile of
    plank fragments with the same footprint."""
    sx, sy, sz = size
    hx, hz = sx / 2, sz / 2
    p = Piece(name + ("_broken" if broken else ""), "soft")
    rng = random.Random(name)
    if broken:
        _plank_fragments(p, rng, size)
        return p
    w = 0.07  # batten width
    p.visual.box((-hx + 0.01, 0, -hz + 0.01), (hx - 0.01, sy, hz - 0.01), "wood")
    for cx in (-1, 1):
        for cz in (-1, 1):
            x0 = hx - w if cx > 0 else -hx
            z0 = hz - w if cz > 0 else -hz
            p.visual.box((x0, 0, z0), (x0 + w, sy, z0 + w), "wood")
    for y0 in (0, sy - w):
        for cz in (-1, 1):
            z0 = hz - w if cz > 0 else -hz
            p.visual.box((-hx + w, y0, z0), (hx - w, y0 + w, z0 + w), "wood")
        for cx in (-1, 1):
            x0 = hx - w if cx > 0 else -hx
            p.visual.box((x0, y0, -hz + w), (x0 + w, y0 + w, hz - w), "wood")
    p.collider_box((-hx, 0, -hz), (hx, sy, hz))
    return p


def _plank_fragments(p, rng, size):
    """Scatter plank pieces roughly within a crate's footprint."""
    sx, _, sz = size
    for i in range(10):
        length = rng.uniform(0.25, max(0.3, sx * 0.8))
        x = rng.uniform(-sx / 2, sx / 2 - length)
        z = rng.uniform(-sz / 2, sz / 2 - 0.12)
        y = 0.03 * (i % 4)
        p.fragment((x, y, z), (x + length, y + 0.03, z + 0.12), "wood")


def sandbags():
    """2 m long, 0.9 m high sandbag wall, two bags deep, centred on origin."""
    p = Piece("sandbags_2m")
    rng = random.Random("sandbags")
    bag_x, bag_y, bag_z = 0.5, 0.15, 0.3
    for layer in range(6):
        y0 = layer * bag_y
        offset = 0.25 if layer % 2 else 0.0
        for row, z0 in enumerate((-bag_z, 0.0)):
            n = 4 if offset == 0 else 3
            for i in range(n):
                x0 = -1.0 + offset + i * bag_x
                j = lambda: rng.uniform(-0.015, 0.015)  # noqa: E731
                p.visual.box(
                    (x0 + 0.01 + j(), y0, z0 + 0.01 + j()),
                    (x0 + bag_x - 0.01 + j(), y0 + bag_y - 0.005, z0 + bag_z - 0.01 + j()),
                    "burlap",
                )
    p.collider_box((-1.0, 0, -bag_z), (1.0, 6 * bag_y, bag_z))
    return p


def barricade(name, width, bottom, height, broken=False):
    """Horizontal planks with gaps across an opening, centred on x = 0, z = 0.
    `bottom`/`height` match the opening it covers."""
    p = Piece(name + ("_broken" if broken else ""), "soft")
    rng = random.Random(name)
    half = width / 2 + 0.05  # planks overlap the jambs a little
    plank, gap = 0.18, 0.08
    y = bottom + 0.05
    while y + plank <= bottom + height:
        if broken:
            # Each plank snaps into 2-3 pieces.
            cuts = sorted(rng.uniform(-half + 0.2, half - 0.2) for _ in range(rng.randint(1, 2)))
            edges = [-half] + cuts + [half]
            for a, b in zip(edges, edges[1:]):
                p.fragment((a + 0.01, y, -0.025), (b - 0.01, y + plank, 0.025), "wood")
        else:
            p.visual.box((-half, y, -0.025), (half, y + plank, 0.025), "wood")
        y += plank + gap
    if not broken:
        # Two upright battens on the inside holding the planks.
        for x in (-width / 4, width / 4):
            p.visual.box((x - 0.05, bottom, -0.065), (x + 0.05, bottom + height, -0.025), "wood")
        p.collider_box((-width / 2, bottom, -0.065), (width / 2, bottom + height, 0.025))
    return p


# --- Registry ------------------------------------------------------------------


def all_pieces():
    pieces = [stone_post(), stone_cap(), merlon()]
    for length in (1, 2, 4):
        pieces += [stone_wall(length), stone_band(length), parapet(length)]
    pieces += [
        stone_wall_door(),
        stone_wall_window(),
        stone_wall_arch(2, opening=1.1, spring=2.0),
        stone_wall_arch(4, opening=2.2, spring=1.7),
    ]
    for length in (1, 2):
        pieces.append(soft_wall(length))
    pieces += [soft_wall_door(), soft_wall_window()]
    pieces += [
        floor("floor_stone_2m", "flagstone", "hard"),
        floor("floor_wood_2m", "wood", "soft"),
        floor_hatch(),
        hatch_cover(),
        stairs(),
        railing(),
    ]
    for broken in (False, True):
        pieces += [
            crate("crate_small", (0.8, 0.8, 0.8), broken),
            crate("crate_large", (1.2, 1.0, 1.2), broken),
            barricade("barricade_door", 1.1, 0.0, 2.2, broken),
            barricade("barricade_window", 1.0, 1.0, 1.1, broken),
        ]
    pieces.append(sandbags())
    # Map pieces live in their own module (imported here to avoid a cycle).
    from .map_pieces import all_map_pieces

    pieces += all_map_pieces()
    return pieces
