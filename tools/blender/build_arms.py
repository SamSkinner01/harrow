"""Build the first-person arms (one .glb per team) with their weapon clips.

Usually run through tools/scripts/build_arms.sh (Linux) or build_arms.ps1
(Windows). Directly:

    blender --background --factory-startup --python tools/blender/build_arms.py -- \
        [--out assets/models/characters] [--weapons assets/models/weapons] \
        [--tuning assets/config/tuning.ron] [--only attackers] [--preview DIR]

Needs the weapon models (tools/scripts/import_weapons.sh) and MPFB (see
build_characters.sh --setup). --preview DIR also renders first-person views
(hip, aimed, reload moments) per team and weapon into DIR.
See tools/blender/characters/arms.py.
"""

import argparse
import os
import sys
import tempfile
import time

import bpy

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from characters import arms, config, export, mpfb  # noqa: E402

ARMS_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "characters", "arms")
WEAPONS = ("tamarisk", "acacia")


def parse_args():
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", default="assets/models/characters")
    parser.add_argument("--weapons", default="assets/models/weapons")
    parser.add_argument("--tuning", default="assets/config/tuning.ron")
    parser.add_argument("--only", nargs="*", default=None)
    parser.add_argument("--preview", default=None)
    parser.add_argument("--no-export", action="store_true")
    return parser.parse_args(argv)


def remove_tree(obj):
    for child in list(obj.children_recursive):
        bpy.data.objects.remove(child, do_unlink=True)
    bpy.data.objects.remove(obj, do_unlink=True)


def main():
    args = parse_args()
    teams = args.only or ["attackers", "defenders"]
    out_dir = os.path.abspath(args.out)
    os.makedirs(out_dir, exist_ok=True)
    scratch = tempfile.mkdtemp(prefix="harrow_arms_")
    tuning = arms.read_tuning(args.tuning)
    bpy.ops.wm.read_factory_settings(use_empty=True)
    mpfb.services()
    for team in teams:
        start = time.time()
        cfg = config.load(os.path.join(ARMS_DIR, f"arms_{team}.toml"))
        bpy.ops.wm.read_factory_settings(use_empty=True)
        ch = arms.build_body(cfg, os.path.join(scratch, cfg["id"]))
        poser = arms.Poser(ch)
        poser.weapon_sight, poser.weapon_mag, poser.weapon_mag_home = {}, {}, {}
        nodes_by_weapon, pivots = {}, {}
        for weapon in WEAPONS:
            pivot, nodes = arms.import_weapon(os.path.abspath(os.path.join(args.weapons, f"{weapon}.glb")))
            nodes_by_weapon[weapon] = {k: arms.node_pos(nodes, k) for k in ("SIGHT", "MUZZLE", "RACK", "MAG_GRIP")}
            poser.weapon_sight[weapon] = nodes_by_weapon[weapon]["SIGHT"]
            poser.weapon_mag[weapon] = nodes["MAGAZINE"]
            poser.weapon_mag_home[weapon] = nodes["MAGAZINE"].location.copy()
            pivots[weapon] = pivot
        holds = arms.holds(nodes_by_weapon)
        ch.actions, frames = arms.record_clips(poser, holds)
        arms.write_extras(ch, frames)
        if args.preview:
            os.makedirs(args.preview, exist_ok=True)
            for weapon, hold in holds.items():
                for other, pivot in pivots.items():
                    for obj in pivot.children_recursive:
                        obj.hide_render = other != weapon
                arms.preview(ch, poser, hold, pivots[weapon], tuning, os.path.abspath(args.preview), team)
        for pivot in pivots.values():
            remove_tree(pivot)
        print(f"{cfg['id']}: {export.stats(ch)}")
        if not args.no_export:
            print(f"exported {export.export_glb(ch, out_dir)}")
        print(f"{cfg['id']} done in {time.time() - start:.1f} s")


main()
