"""Weapon blockout definitions (M3). Original designs.

Conventions (game space, metres; see kit/geometry.py):

- The weapon points down -Z, +Y up, +X to the shooter's right.
- The origin is where the firing hand grips: the top-back of the pistol grip.
- Objects:
  - `<name>` is the body (receiver, barrel, furniture, sights).
  - `MAGAZINE` is separate so the game can animate reloads.
  - `SLIDE` (sidearm only) is separate so it can kick back when firing.
- Empty nodes the game reads (positions in weapon space):
  - `SIGHT`: a point on the sight line (rear sight / optic centre). In ADS
    the game puts this line on the camera axis.
  - `MUZZLE`: the barrel tip, where the flash goes.
- Material names are looked up in the game's weapon material set
  (`src/weapons/materials.rs`); the colours below are only for previews.

Designs:
- TAMARISK: 5.56-class assault rifle. Flat-top steel receiver, octagonal
  sand-coloured handguard with vent slots, skeletal fixed stock, curved
  30-round magazine, compact open reflex sight.
- ACACIA: 9 mm-class sidearm. Two-tone (sand frame, steel slide), squared
  slide with rear grip grooves, three-dot-style iron sights, 15 rounds.
"""

import math

from .shapes import Part, mirror_x

# name: (base colour RGB, metallic, roughness, alpha). Previews and fallback.
MATERIALS = {
    "weapon_steel": ((0.06, 0.062, 0.066), 0.85, 0.42, 1.0),
    "weapon_dark": ((0.03, 0.03, 0.032), 0.6, 0.55, 1.0),
    "weapon_polymer_sand": ((0.42, 0.35, 0.25), 0.0, 0.7, 1.0),
    "weapon_polymer_black": ((0.05, 0.05, 0.05), 0.0, 0.65, 1.0),
    "weapon_rubber": ((0.03, 0.03, 0.03), 0.0, 0.9, 1.0),
    "weapon_glass": ((0.55, 0.75, 0.7), 0.0, 0.05, 0.2),
    "weapon_dot": ((1.0, 0.05, 0.02), 0.0, 0.5, 1.0),
    "weapon_sight_white": ((0.9, 0.9, 0.85), 0.0, 0.5, 1.0),
}


class Weapon:
    def __init__(self, name):
        self.name = name
        self.parts = []
        # Empties: name -> (x, y, z)
        self.points = {}

    def part(self, name):
        p = Part(name)
        self.parts.append(p)
        return p


# --- TAMARISK assault rifle ------------------------------------------------

RIFLE_BORE = 0.055  # bore axis height above the grip
RIFLE_SIGHT = 0.1185  # optic window centre


def tamarisk():
    w = Weapon("tamarisk")
    b = w.part("tamarisk")
    steel, dark = "weapon_steel", "weapon_dark"
    sand, black = "weapon_polymer_sand", "weapon_polymer_black"

    # Receiver: lower, upper, flat-top rail with cross ribs.
    b.box((-0.016, 0.016), (-0.005, 0.042), (-0.175, 0.105), steel)
    b.box((-0.018, 0.018), (0.042, 0.078), (-0.205, 0.125), steel)
    b.box((-0.011, 0.011), (0.078, 0.086), (-0.2, 0.12), steel)
    for i in range(15):
        z0 = -0.195 + i * 0.021
        b.box((-0.0115, 0.0115), (0.086, 0.091), (z0, z0 + 0.011), steel)
    # Ejection port, charging handle, selector.
    b.box((0.018, 0.0195), (0.05, 0.068), (-0.06, 0.0), dark)
    b.box((0.018, 0.034), (0.056, 0.068), (-0.165, -0.14), dark)
    for x in ((0.016, 0.02), (-0.02, -0.016)):
        b.box(x, (0.02, 0.03), (0.03, 0.05), dark)
    # Magazine well (flared) and trigger guard.
    b.box((-0.019, 0.019), (-0.045, -0.005), (-0.137, -0.07), steel)
    b.box((-0.006, 0.006), (-0.048, -0.042), (-0.07, 0.005), steel)
    b.box((-0.003, 0.003), (-0.035, -0.005), (-0.036, -0.029), dark)
    # Pistol grip, raked back, with a finger bump.
    b.side_profile(
        [(0.0, -0.005), (0.048, -0.005), (0.08, -0.118), (0.04, -0.125), (0.022, -0.06)],
        -0.015, 0.015, black,
    )

    # Octagonal handguard with vent slots; gas block, barrel, flash hider.
    b.tube_z((0.0, RIFLE_BORE), 0.03, -0.47, -0.205, sand, segments=8, phase=math.pi / 8)
    flat = 0.03 * math.cos(math.pi / 8)
    for i in range(5):
        z0 = -0.44 + i * 0.045
        for x in ((flat - 0.0005, flat + 0.001), mirror_x((flat - 0.0005, flat + 0.001))):
            b.box(x, (0.048, 0.062), (z0, z0 + 0.028), dark)
    b.box((-0.012, 0.012), (0.04, 0.07), (-0.5, -0.47), steel)
    b.tube_z((0.0, RIFLE_BORE), 0.0095, -0.6, -0.5, steel, segments=12)
    b.tube_z((0.0, RIFLE_BORE), 0.0125, -0.655, -0.6, dark, segments=6)

    # Skeletal fixed stock with a rubber butt pad.
    b.box((-0.017, 0.017), (0.005, 0.07), (0.125, 0.15), black)
    b.side_profile([(0.15, 0.072), (0.36, 0.068), (0.36, 0.05), (0.15, 0.052)], -0.012, 0.012, sand)
    b.side_profile([(0.15, 0.022), (0.345, -0.03), (0.345, -0.05), (0.15, 0.002)], -0.011, 0.011, sand)
    b.box((-0.02, 0.02), (-0.075, 0.08), (0.345, 0.375), "weapon_rubber")

    # Open reflex sight: base, two side walls and a hood, glass, red dot.
    b.box((-0.016, 0.016), (0.091, 0.1), (-0.04, 0.035), dark)
    b.box((-0.019, -0.014), (0.1, 0.142), (-0.03, 0.02), black)
    b.box((0.014, 0.019), (0.1, 0.142), (-0.03, 0.02), black)
    b.box((-0.019, 0.019), (0.137, 0.144), (-0.03, 0.02), black)
    b.box((-0.014, 0.014), (0.1, 0.137), (-0.021, -0.0195), "weapon_glass")
    d = 0.0011
    b.box((-d, d), (RIFLE_SIGHT - d, RIFLE_SIGHT + d), (-0.0195, -0.019), "weapon_dot")

    # Curved 30-round magazine with a wider base plate.
    m = w.part("MAGAZINE")
    m.side_profile(
        [(-0.13, -0.005), (-0.077, -0.005), (-0.088, -0.1), (-0.115, -0.2),
         (-0.172, -0.192), (-0.143, -0.098)],
        -0.0125, 0.0125, black,
    )
    m.side_profile(
        [(-0.112, -0.197), (-0.175, -0.189), (-0.177, -0.203), (-0.112, -0.212)],
        -0.014, 0.014, black,
    )

    w.points["SIGHT"] = (0.0, RIFLE_SIGHT, 0.0)
    w.points["MUZZLE"] = (0.0, RIFLE_BORE, -0.66)
    return w


# --- ACACIA sidearm --------------------------------------------------------

PISTOL_BORE = 0.032
PISTOL_SIGHT = 0.0545  # top of the front post and rear sight


def acacia():
    w = Weapon("acacia")
    b = w.part("acacia")
    steel, dark = "weapon_steel", "weapon_dark"
    sand = "weapon_polymer_sand"

    # Frame: dust cover, beavertail, grip, trigger guard, trigger.
    b.box((-0.0105, 0.0105), (0.0, 0.016), (-0.165, 0.0), sand)
    b.side_profile([(0.0, 0.016), (0.025, 0.012), (0.03, 0.0), (0.0, -0.005)], -0.0105, 0.0105, sand)
    b.side_profile(
        [(-0.03, 0.0), (0.02, 0.0), (0.04, -0.104), (-0.008, -0.104), (-0.016, -0.06)],
        -0.0145, 0.0145, sand,
    )
    b.box((-0.005, 0.005), (-0.035, 0.0), (-0.075, -0.066), sand)
    b.box((-0.005, 0.005), (-0.038, -0.031), (-0.075, -0.025), sand)
    b.box((-0.0025, 0.0025), (-0.028, 0.0), (-0.045, -0.038), dark)
    # Barrel crown.
    b.tube_z((0.0, PISTOL_BORE), 0.0062, -0.175, -0.168, steel, segments=12)
    b.tube_z((0.0, PISTOL_BORE), 0.004, -0.1755, -0.17, dark, segments=8)

    # Slide: grip grooves, ejection port, sights. Separate for recoil.
    s = w.part("SLIDE")
    s.box((-0.0125, 0.0125), (0.016, 0.046), (-0.172, 0.005), steel)
    for i in range(6):
        z0 = -0.03 + i * 0.0055
        for x in ((0.0125, 0.0132), (-0.0132, -0.0125)):
            s.box(x, (0.02, 0.042), (z0, z0 + 0.0025), dark)
    s.box((0.0, 0.0127), (0.04, 0.0463), (-0.085, -0.045), dark)
    for x in ((-0.0115, -0.0035), (0.0035, 0.0115)):
        s.box(x, (0.046, PISTOL_SIGHT), (-0.008, 0.002), dark)
    s.box((-0.0115, 0.0115), (0.046, 0.0485), (-0.008, 0.002), dark)
    s.box((-0.0022, 0.0022), (0.046, PISTOL_SIGHT), (-0.165, -0.158), dark)
    s.box((-0.0012, 0.0012), (0.0505, 0.0529), (-0.158, -0.1576), "weapon_sight_white")
    for x in ((-0.0095, -0.0071), (0.0071, 0.0095)):
        s.box(x, (0.0505, 0.0529), (0.002, 0.0024), "weapon_sight_white")

    # Magazine: body inside the grip and a base plate below it.
    m = w.part("MAGAZINE")
    m.side_profile(
        [(-0.022, -0.002), (0.012, -0.002), (0.032, -0.1), (-0.002, -0.1)],
        -0.011, 0.011, steel,
    )
    m.side_profile(
        [(-0.009, -0.104), (0.04, -0.104), (0.041, -0.115), (-0.01, -0.115)],
        -0.0145, 0.0145, "weapon_polymer_black",
    )

    w.points["SIGHT"] = (0.0, PISTOL_SIGHT, 0.0)
    w.points["MUZZLE"] = (0.0, PISTOL_BORE, -0.176)
    return w


def all_weapons():
    return [tamarisk(), acacia()]
