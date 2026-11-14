"""First-person arms (realism pass).

Gloved, sleeved forearms and hands for the weapon view, built with the same
MPFB pipeline as the operators (same skeleton, uniform and glove colours),
cut down to the arms, posed on each weapon and exported to
`assets/models/characters/arms_<team>.glb`. Configs: `arms/*.toml`.

How the game uses them (src/weapons/arms.rs): the arms' scene is a child of
the weapon rig (render layer 1, the weapon camera), placed per weapon so
that the weapon's grip origin is where the hands expect it. Everything the
viewmodel does to the weapon (aim, sway, bob, kick, sprint, reload tilt)
moves the arms with it. Per weapon there are three clips:

  <weapon>_hold          both hands on the weapon (static)
  <weapon>_reload        tactical reload, 0..1 of the reload time: the
                         left hand takes the magazine out, fetches a new
                         one and seats it, following exactly the path
                         src/weapons/viewmodel.rs moves the magazine on
  <weapon>_reload_empty  the same, then racks the charging handle/slide

The game seeks the reload clips to the reload's progress, so the length
of the clip doesn't matter (they are authored over RELOAD_SECONDS).

Frames:
- Blender: Z up, the gun's barrel along +Y, the shooter's right +X (the
  gun frame of anim.py: origin where the web of the right hand grips).
- The armature stays at the world origin; each weapon is placed at its
  gun frame `G` in armature space (bladed stance for the rifle, square
  for the pistol). The inverse of `G` (in glTF axes) is written to the rig
  node's `harrow_arms` extra so the game can put the gun frame on the rig.
"""

import json
import math
import os
import re

import bmesh
import bpy
from mathutils import Matrix, Quaternion, Vector

from . import anim, materials, mpfb
from . import body as body_mod
from . import gear as gear_mod
from .palette import palette_for
from .pipeline import Character, triangulate

ARM_PREFIXES = ("clavicle", "upperarm", "lowerarm", "hand", "thumb", "index", "middle", "ring", "pinky")
FINGERS = ("index", "middle", "ring", "pinky")
# Keep the upper arm from this fraction of its length (shoulder = 0) down.
UPPER_ARM_KEEP = 0.4
# Clip length the reloads are authored over (the game rescales them).
RELOAD_SECONDS = 2.4
FPS = anim.FPS

R = anim.R


def is_arm(name):
    return name.startswith(ARM_PREFIXES)


def ease(t):
    t = max(0.0, min(1.0, t))
    return t * t * (3 - 2 * t)


def ramp(t, a, b):
    return ease((t - a) / (b - a))


def bump(t, a, b):
    x = max(0.0, min(1.0, (t - a) / (b - a)))
    return math.sin(x * math.pi)


def magazine_drop(t):
    """How far the magazine is out (0 seated .. 1 gone), same as
    `pose_viewmodel` in src/weapons/viewmodel.rs."""
    return ramp(t, 0.12, 0.32) if t < 0.55 else 1.0 - ramp(t, 0.55, 0.75)


# The magazine moves straight down by this much (viewmodel.rs).
MAG_TRAVEL = 0.35
# The slide moves back this much when racked (viewmodel.rs).
SLIDE_TRAVEL = 0.022


# ------------------------------------------------------------------- body


def build_body(cfg, scratch):
    """The operator pipeline up to gear, then cut down to the arms."""
    ch = Character(cfg)
    rig, body, eyes, brows, lashes = mpfb.build_human(cfg)
    mpfb.stage_for_game(rig, [body, eyes, brows, lashes], cfg["body"]["height_m"])
    rig.name = rig.data.name = "rig"
    ch.rig, ch.body, ch.eyes = rig, body, eyes
    ch.marks = body_mod.Landmarks(rig, eyes)
    mats = materials.build(cfg, scratch)
    colours = palette_for(cfg)
    body_mod.dress(body, ch.marks, cfg, colours, mats)
    ch.materials, ch.colours = mats, colours
    gear = gear_mod.build(ch, gear_mod.Surfaces(body))
    for obj in (eyes, brows, lashes):
        bpy.data.objects.remove(obj, do_unlink=True)
    ch.eyes = None
    ch.body = keep_arms(body, rig, "arms")
    ch.gear = keep_arms(gear, rig, "arms_gear")
    for obj in ch.meshes:
        triangulate(obj)
    for pb in rig.pose.bones:
        pb.rotation_mode = "QUATERNION"
    return ch


def keep_arms(obj, rig, name):
    """Delete everything but the forearms, hands and the lower part of the
    upper arms. Returns the object (renamed) or None if nothing is left."""
    bones = rig.data.bones
    groups = {g.index: g.name for g in obj.vertex_groups}
    mw = obj.matrix_world
    keep = set()
    for v in obj.data.vertices:
        total = arm = upper = 0.0
        side = None
        for g in v.groups:
            n = groups.get(g.group, "")
            total += g.weight
            if is_arm(n) and not n.startswith("clavicle"):
                arm += g.weight
            if n.startswith("upperarm"):
                upper += g.weight
                side = n[-1]
        if total <= 0.0 or arm / total < 0.6:
            continue
        if side is not None and upper / total > 0.3:
            sh = bones[f"upperarm_{side}"].head_local
            el = bones[f"lowerarm_{side}"].head_local
            axis = el - sh
            if (mw @ v.co - sh).dot(axis) / axis.length_squared < UPPER_ARM_KEEP:
                continue
        keep.add(v.index)
    if not keep:
        bpy.data.objects.remove(obj, do_unlink=True)
        return None
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    bm.verts.ensure_lookup_table()
    bmesh.ops.delete(bm, geom=[v for v in bm.verts if v.index not in keep], context="VERTS")
    bm.to_mesh(obj.data)
    bm.free()
    obj.data.update()
    # Unused vertex groups would only bloat the skin.
    obj.name = obj.data.name = name
    print(f"{name}: kept {len(obj.data.vertices)} vertices")
    return obj


# ---------------------------------------------------------------- weapons


def read_tuning(path):
    """Hip pose and ADS distance per weapon from assets/config/tuning.ron
    (for the preview cameras only)."""
    text = open(path, encoding="utf-8").read()
    out = {}
    for slot, name in (("primary", "tamarisk"), ("sidearm", "acacia")):
        start = text.index(f"\n        {slot}: (")
        hip = re.search(r"hip: \(([-\d.]+), ([-\d.]+), ([-\d.]+)\)", text[start:])
        ads = re.search(r"ads_distance: ([-\d.]+)", text[start:])
        fov = re.search(r"\n        view: \(.*?fov: ([-\d.]+)", text, re.S)
        out[name] = {
            "hip": tuple(float(x) for x in hip.groups()),
            "ads_distance": float(ads.group(1)),
            "fov": float(fov.group(1)),
        }
    return out


def import_weapon(path):
    """Import a weapon .glb (built by tools/scripts/import_weapons.sh) under
    a pivot empty. Returns (pivot, {node name: object})."""
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=path)
    new = [o for o in bpy.data.objects if o not in before]
    pivot = bpy.data.objects.new("weapon_pivot", None)
    bpy.context.scene.collection.objects.link(pivot)
    nodes = {}
    for o in new:
        nodes[o.name.split(".")[0]] = o
        if o.parent is None:
            o.parent = pivot
    return pivot, nodes


def node_pos(nodes, name):
    """A node's position in the gun frame (pivot at identity)."""
    return nodes[name].matrix_world.translation.copy()


# ---------------------------------------------------------------- holding


class Hand:
    """Where a hand goes, in the gun frame: wrist position, the direction
    the fingers point (wrist to knuckles), the palm normal, the elbow pole,
    and finger curls in degrees (index, middle, ring, pinky; thumb joints)."""

    def __init__(self, wrist, fingers, palm, pole, curl=(60, 65, 70, 72), thumb=(20, 25, 20), swing=0.0):
        self.wrist = Vector(wrist)
        self.fingers = Vector(fingers)
        self.palm = Vector(palm)
        self.pole = Vector(pole)
        self.curl = curl
        self.thumb = thumb
        # Thumb base turned across the palm towards the fingers (degrees).
        self.swing = swing

    def moved(self, offset=Vector(), fingers=None, palm=None, curl=None, thumb=None):
        return Hand(self.wrist + offset, fingers or self.fingers, palm or self.palm, self.pole,
                    curl or self.curl, thumb or self.thumb, self.swing)


def blend_hand(a, b, k):
    """Interpolate two hands (positions linearly, directions slerped by
    normalised lerp, curls linearly)."""
    def lerp(x, y):
        return x.lerp(y, k)

    def nlerp(x, y):
        v = x.normalized().lerp(y.normalized(), k)
        return v.normalized() if v.length > 1e-6 else y

    return Hand(lerp(a.wrist, b.wrist), nlerp(a.fingers, b.fingers), nlerp(a.palm, b.palm),
                nlerp(a.pole, b.pole),
                tuple(x + (y - x) * k for x, y in zip(a.curl, b.curl)),
                tuple(x + (y - x) * k for x, y in zip(a.thumb, b.thumb)),
                a.swing + (b.swing - a.swing) * k)


class WeaponHold:
    """Everything about holding one weapon. Positions in the gun frame."""

    def __init__(self, name, shoulder_r, blade, right, left, mag_left, rack_left, rack_pull,
                 stash, empty_rack=True):
        self.name = name
        self.shoulder_r = Vector(shoulder_r)  # right shoulder joint in the gun frame
        self.blade = blade                    # torso turned to the right, degrees
        self.right = right
        self.left = left
        self.mag_left = mag_left              # left hand holding the seated magazine
        self.rack_left = rack_left            # left hand on the charging handle / slide
        self.rack_pull = Vector(rack_pull)    # how far the hand pulls when racking
        self.stash = Vector(stash)            # where the hand goes off screen (pouch)


def holds(nodes_by_weapon):
    """Hand placements per weapon, from the weapon's own nodes."""
    out = {}
    n = nodes_by_weapon.get("tamarisk")
    if n is not None:
        mag, rack = n["MAG_GRIP"], n["RACK"]
        out["tamarisk"] = WeaponHold(
            "tamarisk",
            shoulder_r=(0.05, -0.36, -0.03),
            blade=38.0,
            # Firing hand: palm against the right of the raked grip, the web
            # at the top-back, wrist behind; index finger on the trigger.
            right=Hand((0.025, -0.065, -0.03), (-0.15, 0.94, -0.35), (-1.0, 0.0, 0.0), (0.8, -0.3, -0.6),
                       curl=(25, 85, 90, 92), thumb=(40, 35, 30)),
            # Support hand cradling the lower handguard: palm up, fingers
            # round the right side, thumb along the left.
            left=Hand((-0.05, 0.27, -0.045), (0.85, 0.35, 0.25), (0.3, 0.0, 0.95), (-0.4, -0.2, -1.0),
                      curl=(60, 65, 70, 72), thumb=(10, 15, 10)),
            mag_left=Hand(mag + Vector((-0.035, -0.04, -0.035)), (0.25, 0.7, -0.65), (0.95, 0.1, 0.25),
                          (-0.5, -0.3, -1.0), curl=(55, 60, 64, 68), thumb=(30, 25, 20)),
            # Over the top to the charging handle on the right.
            rack_left=Hand(rack + Vector((-0.03, 0.01, 0.06)), (0.7, 0.1, -0.7), (0.3, 0.0, -0.95),
                           (-0.5, -0.5, -1.0), curl=(70, 75, 80, 82), thumb=(40, 35, 30)),
            rack_pull=(0.0, -0.075, 0.0),
            stash=(-0.12, -0.25, -0.35),
        )
    n = nodes_by_weapon.get("acacia")
    if n is not None:
        mag, rack = n["MAG_GRIP"], n["RACK"]
        out["acacia"] = WeaponHold(
            "acacia",
            shoulder_r=(0.16, -0.5, -0.06),
            blade=8.0,
            right=Hand((0.02, -0.062, -0.03), (-0.15, 0.94, -0.35), (-1.0, 0.0, 0.0), (0.7, -0.3, -0.8),
                       curl=(25, 85, 90, 92), thumb=(35, 30, 25)),
            # Support hand wrapped round the firing hand's fingers, heel of
            # the palm on the left grip panel, thumb forward along the frame.
            left=Hand((-0.045, -0.035, -0.075), (0.55, 0.55, -0.6), (0.9, 0.0, 0.3), (-0.6, -0.3, -1.0),
                      curl=(70, 75, 80, 82), thumb=(5, 10, 5)),
            # Palm under the magazine's base plate, pushing it home.
            mag_left=Hand(mag + Vector((-0.03, -0.05, -0.045)), (0.4, 0.8, 0.3), (0.0, 0.0, 1.0),
                          (-0.6, -0.4, -1.0), curl=(45, 50, 55, 60), thumb=(25, 25, 20)),
            # Overhand on the slide, fingers down its right side.
            rack_left=Hand(rack + Vector((-0.06, -0.01, 0.03)), (0.9, 0.0, -0.4), (0.1, 0.0, -1.0),
                           (-0.6, -0.6, -1.0), curl=(75, 80, 85, 85), thumb=(45, 40, 35)),
            rack_pull=(0.0, -SLIDE_TRAVEL - 0.008, 0.0),
            stash=(-0.15, -0.3, -0.35),
        )
    return out


def gun_frame(sk, hold):
    """The gun frame in armature space: the torso turned by `blade`, the
    right shoulder at `hold.shoulder_r`."""
    turn = Matrix.Rotation(math.radians(-hold.blade), 4, "Z")
    shoulder = sk.head["upperarm_r"]
    # Armature placed in the gun frame: A = T @ turn, with A @ shoulder = shoulder_r.
    A = Matrix.Translation(hold.shoulder_r - (turn @ shoulder)) @ turn
    return A.inverted()


class Poser:
    def __init__(self, ch):
        self.ch = ch
        self.sk = anim.Skeleton(ch)
        self.animator = anim.Animator(ch)
        sk = self.sk
        # Anatomical rest frames of the hands. The rig's hand bones don't
        # point along the fingers (their tails run across the knuckles), so
        # use the joints: wrist -> middle knuckle, and the palm normal from
        # the knuckle line (palms face the thighs at rest).
        self.rest_hand = {}
        for side, inward in (("l", Vector((1, 0, 0))), ("r", Vector((-1, 0, 0)))):
            wrist = sk.head[f"hand_{side}"]
            along = (sk.head[f"middle_01_{side}"] - wrist).normalized()
            across = (sk.head[f"index_01_{side}"] - sk.head[f"pinky_01_{side}"]).normalized()
            palm = along.cross(across).normalized()
            if palm.dot(inward) < 0:
                palm.negate()
            palm = (palm - along * palm.dot(along)).normalized()
            self.rest_hand[side] = (along, palm)

    def hand_rot(self, side, fingers, palm):
        """Hand bone rotation (armature space) with the fingers (wrist to
        knuckles) along `fingers` and the palm facing `palm`."""
        f = Vector(fingers).normalized()
        p = Vector(palm)
        p = (p - f * p.dot(f)).normalized()
        f0, p0 = self.rest_hand[side]
        return anim.frame(f, p) @ anim.frame(f0, p0).transposed() @ self.sk.rest_rot[f"hand_{side}"]

    def pose(self, G, right, left):
        sk = self.sk
        pose = self.animator.standing(0.0, breathe=False)
        rot = G.to_3x3()
        for side, hand in (("r", right), ("l", left)):
            pose.hands[side] = (G @ hand.wrist, self.hand_rot(side, rot @ hand.fingers, rot @ hand.palm),
                                rot @ hand.pole)
            self.fingers(pose, side, hand)
        pose.rot["clavicle_l"] = R("Z", -8.0)
        pose.rot["clavicle_r"] = R("Z", 4.0)
        return pose

    def fingers(self, pose, side, hand):
        sk = self.sk
        palm = self.rest_hand[side][1]
        for f, c in zip(FINGERS, hand.curl):
            for k, scale in (("01", 0.8), ("02", 1.1), ("03", 0.8)):
                name = f"{f}_{k}_{side}"
                y = (sk.tail[name] - sk.head[name]).normalized()
                hinge = y.cross(palm).normalized()
                pose.rot[name] = R(hinge, c * scale)
        for k, deg in zip(("01", "02", "03"), hand.thumb):
            name = f"thumb_{k}_{side}"
            y = (sk.tail[name] - sk.head[name]).normalized()
            hinge = y.cross(palm).normalized()
            rot = R(hinge, deg)
            if k == "01" and hand.swing:
                # Swing about the palm normal, towards the index finger.
                to_index = sk.head[f"index_01_{side}"] - sk.head[name]
                sign = 1.0 if y.cross(to_index).dot(palm) > 0 else -1.0
                rot = R(palm, sign * hand.swing) @ rot
            pose.rot[name] = rot

    def record(self, name, length, fn):
        """Key the arm bones over `length` seconds; fn(t) with t in 0..1."""
        rig = self.ch.rig
        sk = self.sk
        frames = max(1, round(length * FPS))
        action = bpy.data.actions.new(name)
        rig.animation_data_create()
        rig.animation_data.action = action
        prev = {}
        for f in range(frames + 1):
            B = sk.basis(sk.solve(fn(f / frames)))
            for pb in rig.pose.bones:
                if not is_arm(pb.name):
                    continue
                b = B[pb.name]
                q = b.to_quaternion()
                if pb.name in prev and prev[pb.name].dot(q) < 0:
                    q.negate()
                prev[pb.name] = q
                pb.rotation_quaternion = q
                pb.location = b.translation if not pb.bone.use_connect else Vector()
                pb.keyframe_insert("rotation_quaternion", frame=f, group=pb.name)
                pb.keyframe_insert("location", frame=f, group=pb.name)
        action.use_fake_user = True
        track = rig.animation_data.nla_tracks.new()
        track.name = name
        track.strips.new(name, 0, action)
        track.mute = True
        rig.animation_data.action = None
        return action


def left_hand_reload(hold, t, empty):
    """The support hand during a reload (t = 0..1 of the reload)."""
    drop = magazine_drop(t)
    down = Vector((0.0, 0.0, -MAG_TRAVEL * drop))
    on_mag = hold.mag_left.moved(down)
    stash = hold.mag_left.moved(hold.stash, curl=(70, 75, 80, 82))
    if t < 0.12:
        # Reach from the handguard to the magazine.
        return blend_hand(hold.left, hold.mag_left, ease(t / 0.11))
    if t < 0.32:
        return on_mag
    if t < 0.45:
        # Old magazine gone: on to the pouch, off screen.
        return blend_hand(on_mag, stash, ramp(t, 0.32, 0.42))
    if t < 0.55:
        return blend_hand(stash, on_mag, ramp(t, 0.45, 0.55))
    if t < 0.78:
        return on_mag
    if not empty:
        return blend_hand(hold.mag_left, hold.left, ramp(t, 0.78, 0.9))
    rack = hold.rack_left
    pulled = rack.moved(hold.rack_pull)
    if t < 0.84:
        return blend_hand(hold.mag_left, rack, ramp(t, 0.78, 0.83))
    if t < 0.92:
        # Pull and let go, in step with the slide (bump 0.82..0.94).
        return blend_hand(rack, pulled, bump(t, 0.82, 0.94))
    return blend_hand(rack, hold.left, ramp(t, 0.92, 1.0))


def record_clips(poser, holds_by_weapon):
    actions = []
    frames = {}
    for name, hold in holds_by_weapon.items():
        G = gun_frame(poser.sk, hold)
        frames[name] = G
        actions.append(poser.record(f"{name}_hold", 1.0 / FPS, lambda t, G=G, h=hold: poser.pose(G, h.right, h.left)))
        for empty in (False, True):
            clip = f"{name}_reload_empty" if empty else f"{name}_reload"
            actions.append(poser.record(
                clip, RELOAD_SECONDS,
                lambda t, G=G, h=hold, e=empty: poser.pose(G, h.right, left_hand_reload(h, t, e))))
    return actions, frames


# ------------------------------------------------------------------ export


# Blender (x, y, z) -> glTF (x, z, -y).
C = Matrix(((1, 0, 0, 0), (0, 0, 1, 0), (0, -1, 0, 0), (0, 0, 0, 1)))


def gltf_transform(M):
    """A Blender-space rigid transform as glTF translation + rotation."""
    g = C @ M @ C.inverted()
    q = g.to_quaternion()
    return {"translation": [round(v, 6) for v in g.translation],
            "rotation": [round(q.x, 6), round(q.y, 6), round(q.z, 6), round(q.w, 6)]}


def write_extras(ch, frames):
    """Per weapon, where the arms' scene goes relative to the weapon model's
    origin (glTF/Bevy axes): the inverse of the gun frame."""
    ch.rig["harrow_arms"] = json.dumps(
        {name: gltf_transform(G.inverted()) for name, G in frames.items()}, sort_keys=True)


# ----------------------------------------------------------------- previews


def preview(ch, poser, hold, nodes_pivot, tuning, out_dir, team):
    """First-person renders from the game's camera positions: hip, aimed,
    and a few reload moments."""
    from . import export as export_mod

    scene = bpy.context.scene
    export_mod._setup_render_scene(scene, (1280, 720))
    G = gun_frame(poser.sk, hold)
    nodes_pivot.matrix_world = G
    t = tuning[hold.name]
    hip = Vector(t["hip"])
    sight = poser.weapon_sight[hold.name]
    # Game camera space (x right, y up, -z forward) -> gun frame (Blender).
    cams = {
        "hip": Vector((-hip.x, -hip.z, -hip.y)),
        "ads": Vector((sight.x, sight.y - t["ads_distance"], sight.z)),
    }
    cam_data = bpy.data.cameras.new("fp_cam")
    cam_data.sensor_fit = "VERTICAL"
    cam_data.angle_y = math.radians(t["fov"])
    cam_data.clip_start = 0.01
    cam = bpy.data.objects.new("fp_cam", cam_data)
    scene.collection.objects.link(cam)
    scene.camera = cam
    look = Matrix(((1, 0, 0), (0, 0, -1), (0, 1, 0))).to_4x4()  # camera -Z along gun +Y, +Y up
    rig = ch.rig
    shots = [("hold", f"{hold.name}_hold", 0.0, "hip"), ("ads", f"{hold.name}_hold", 0.0, "ads")]
    for frac in (0.08, 0.2, 0.5, 0.66, 0.88):
        shots.append((f"reload{int(frac * 100):02d}", f"{hold.name}_reload_empty", frac, "hip"))
    mag = poser.weapon_mag[hold.name]
    for label, action_name, frac, view in shots:
        rig.animation_data.action = bpy.data.actions[action_name]
        frame = round(frac * RELOAD_SECONDS * FPS) if "reload" in action_name else 0
        scene.frame_set(frame)
        drop = magazine_drop(frac) if "reload" in action_name else 0.0
        mag.location = poser.weapon_mag_home[hold.name] + Vector((0, 0, -MAG_TRAVEL * drop))
        mag.hide_render = drop > 0.95
        cam.matrix_world = G @ Matrix.Translation(cams[view]) @ look
        scene.render.filepath = os.path.join(out_dir, f"arms_{team}_{hold.name}_{label}.png")
        bpy.ops.render.render(write_still=True)
    mag.location = poser.weapon_mag_home[hold.name]
    mag.hide_render = False
    # Outside views of the hold pose, to check hand placement.
    rig.animation_data.action = bpy.data.actions[f"{hold.name}_hold"]
    scene.frame_set(0)
    cam_data.type = "ORTHO"
    left_at = hold.left.wrist + Vector((0.03, 0.04, 0.0))
    views = (
        ("side", Vector((-1.5, 0.05, 0.0)), Vector((0.0, -0.1, -0.05)), 1.1),
        ("front", Vector((0.4, 1.5, 0.2)), Vector((0.0, -0.1, -0.05)), 1.1),
        ("grip_right", Vector((1.0, -0.02, -0.02)), Vector((0.0, -0.02, -0.04)), 0.28),
        ("grip_left", Vector((-1.0, -0.02, -0.02)), Vector((0.0, -0.02, -0.04)), 0.28),
        ("support_left", left_at + Vector((-0.8, -0.1, -0.5)), left_at, 0.3),
        ("support_below", left_at + Vector((0.1, 0.0, -1.0)), left_at, 0.3),
    )
    for label, where, at, scale in views:
        cam_data.ortho_scale = scale
        target = G @ at
        loc = G @ where
        cam.matrix_world = Matrix.Translation(loc) @ (target - loc).to_track_quat("-Z", "Y").to_matrix().to_4x4()
        scene.render.filepath = os.path.join(out_dir, f"arms_{team}_{hold.name}_{label}.png")
        bpy.ops.render.render(write_still=True)
    rig.animation_data.action = None
    bpy.data.objects.remove(cam, do_unlink=True)
