"""Shape helpers for weapon blockouts, on top of kit.geometry.MeshBuilder.

Game space as in the kit (X right, Y up, Z towards the viewer, metres).
Weapons point down -Z, so a side profile is drawn in the (z, y) plane.
"""

import math

from kit.geometry import MeshBuilder


class Part:
    """One exported object: a named mesh (and optional custom props)."""

    def __init__(self, name):
        self.name = name
        self.mesh = MeshBuilder([])

    # -- primitives -------------------------------------------------------

    def box(self, x, y, z, material):
        """Axis-aligned box; x, y, z are (min, max) pairs."""
        self.mesh.box((x[0], y[0], z[0]), (x[1], y[1], z[1]), material)

    def side_profile(self, outline, x0, x1, material):
        """Extrude a side profile across X.

        `outline` is a list of (z, y) points (any winding); the shape spans
        x0..x1. Use for grips, magazines, stocks and trigger guards.
        """
        # Work in (u, v, w) = (-z, y, x), which is right-handed, so the kit's
        # "counter-clockwise seen from outside" rule carries over.
        pts = [(-z, y) for z, y in outline]
        area = sum(a[0] * b[1] - b[0] * a[1] for a, b in zip(pts, pts[1:] + pts[:1]))
        if area < 0:
            pts.reverse()

        def p(uv, w):
            return (w, uv[1], -uv[0])

        self.mesh.face([p(q, x1) for q in pts], material)
        self.mesh.face([p(q, x0) for q in reversed(pts)], material)
        n = len(pts)
        for i in range(n):
            a, b = pts[i], pts[(i + 1) % n]
            self.mesh.face([p(a, x1), p(a, x0), p(b, x0), p(b, x1)], material)

    def tube_z(self, centre, radius, z0, z1, material, segments=12, phase=0.0, caps=True):
        """Prism along Z (a cylinder with `segments` sides) from z0 to z1
        (z0 < z1). `phase` rotates the cross-section (radians)."""
        cx, cy = centre
        ring = [
            (cx + radius * math.cos(phase + 2 * math.pi * i / segments),
             cy + radius * math.sin(phase + 2 * math.pi * i / segments))
            for i in range(segments)
        ]
        for i in range(segments):
            (ax, ay), (bx, by) = ring[i], ring[(i + 1) % segments]
            # Counter-clockwise seen from outside: around the tube.
            self.mesh.face([(ax, ay, z1), (ax, ay, z0), (bx, by, z0), (bx, by, z1)], material)
        if caps:
            self.mesh.face([(x, y, z1) for x, y in ring], material)
            self.mesh.face([(x, y, z0) for x, y in reversed(ring)], material)

    def to_object(self, collection):
        return self.mesh.to_object(self.name, collection)


def mirror_x(x):
    """(min, max) pair mirrored to the other side of X = 0."""
    return (-x[1], -x[0])
