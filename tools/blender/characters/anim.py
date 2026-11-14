"""Third-person animations, baked per operator.

Locomotion (idle, walk, run, crouch_idle, crouch_walk) is motion capture
from the CMU database, retargeted onto this rig by `retarget.py` (the BVH
files are read from the asset staging folder at build time and end up only
as keyframes in the .glb). The arms never come from the mocap: the rifle is
carried on the chest and both hands hold it. The clips no free source has
(aiming and firing, reloading, prone, leaning) are posed here.

Everything is posed in *armature axes* (Z up, the character faces +Y, its
right is +X), which keeps the authoring independent of how MPFB rolled the
bones:

- Spine, neck, head, pelvis, clavicles, fingers: rotations relative to the
  parent, expressed as if the parent were at rest (`Pose.rot`), or, for the
  retargeted clips, absolute armature-space rotations (`Pose.world`).
- Legs and arms: analytic two-bone IK to an ankle/wrist target with a pole
  direction for the knee/elbow (`Pose.feet`, `Pose.hands`).
- The rifle: a frame `G` attached to the chest. The right hand grips it,
  the left hand supports the handguard (or does the reload). Aiming puts
  the sight line through the right eye, the cheek on the stock.

We evaluate the skeleton ourselves (no constraints, no baking) and key the
resulting local rotations on every frame. Clips are in place (no root
motion). Lengths and, for locomotion, the speed the stance feet move back
at are written to the rig's `harrow_clips` extra; the game plays each clip
at movement speed / that speed, so the feet don't slide.

Clip list (glTF animation names) and what the game lets each one drive
(mask groups, src/characters/library.rs):

  idle, walk, run, crouch_idle,   whole body (walk is a jog: the game's
  crouch_walk, prone_idle,        3.4 m/s is jogging pace)
  prone_crawl
  lean_left, lean_right           spine_01, spine_02
  fire                            arms, spine_03, neck, head: aim (cheek
                                  on the stock) + one recoil kick per loop
  reload                          arms, spine_03, neck, head: magazine
                                  swap, one-shot
"""

import json
import math
import os

import bpy
from mathutils import Matrix, Quaternion, Vector

from . import retarget

FPS = 30

# The rifle (TAMARISK, AK pattern) in the gun frame: origin where the web of
# the right hand sits on the top-back of the pistol grip, +Y along the
# barrel, +Z up, +X right; metres.
BUTT = Vector((0.0, -0.25, -0.01))            # butt plate centre
MAGWELL = Vector((0.0, 0.152, 0.013))          # top of the magazine
SIGHT_Z = 0.077                                # rear notch and front post tip
FRONT_SIGHT = Vector((0.0, 0.589, SIGHT_Z))
MUZZLE = Vector((0.0, 0.629, 0.03))
# The aiming eye: on the sight line, 0.63 m behind the front post.
EYE = Vector((0.0, FRONT_SIGHT.y - 0.63, SIGHT_Z))

# The aim pose (degrees): chest turned right and pitched forward, neck and
# head flexed and tilted right onto the stock, right shoulder forward/up
# (Animator.aim_body). Chosen so the butt plate sits in the right shoulder
# pocket while the sight line passes through the eye.
AIM = {"blade": 30.0, "chest": 14.0, "neck": 18.0, "head": 3.0, "tilt": 15.0,
       "shoulder_fwd": 0.0, "shoulder_up": 10.0}

# Mask groups (the game's split, src/characters/library.rs).
SPINE_GROUP = ("spine_01", "spine_02")           # lean
UPPER_GROUP = ("spine_03", "neck_01", "head")    # aim / reload, with the arms
FINGERS = ("index", "middle", "ring", "pinky")

# Motion capture: CMU clip, search window (s), cycle length range (s), time
# scale (> 1 plays faster), whether it travels, and how far the average
# pelvis / chest are turned back to the standing pose (so the aim and lean
# overlays, posed standing, line up).
CMU_DIR = os.path.join("cmu_mocap_bvh_selection", "source")


class Mocap:
    def __init__(self, clip, window, length, time_scale=1.0, travel=True, pelvis=1.0, chest=1.0, head=1.0,
                 hips=None):
        self.clip, self.window, self.length = clip, window, length
        self.time_scale, self.travel = time_scale, travel
        self.pelvis, self.chest, self.head = pelvis, chest, head
        # Average hip height as a fraction of standing (None: as captured).
        self.hips = hips


# crouch_idle is the crouch walk's double-support stance, held, with breathing.
MOCAP = {
    "idle": Mocap("77_02", (0.2, 4.5), (2.8, 4.0), travel=False),
    "walk": Mocap("35_17", (0.1, 1.38), (0.7, 0.85)),
    "run": Mocap("09_01", (0.05, 1.22), (0.62, 0.8), time_scale=1.35, chest=0.5),
    "crouch_walk": Mocap("136_09", (3.9, 9.3), (2.3, 2.7), time_scale=1.8, pelvis=0.6, hips=0.52),
}


def R(axis, deg):
    if isinstance(axis, str):
        return Matrix.Rotation(math.radians(deg), 3, axis)
    return Matrix.Rotation(math.radians(deg), 3, Vector(axis).normalized())


def frame(y, n):
    """3x3 with columns y, n, y x n (y and n orthonormal)."""
    return Matrix((y, n, y.cross(n))).transposed()


def look(barrel, top):
    """Gun rotation: +Y along `barrel`, +Z towards `top`."""
    y = Vector(barrel).normalized()
    x = y.cross(Vector(top)).normalized()
    z = x.cross(y).normalized()
    return Matrix((x, y, z)).transposed()


def ease(t):
    t = max(0.0, min(1.0, t))
    return t * t * (3 - 2 * t)


def keys(points, t):
    """Smoothly interpolate [(time, value), ...] (values: float/Vector)."""
    if t <= points[0][0]:
        return points[0][1]
    for (t0, v0), (t1, v1) in zip(points, points[1:]):
        if t <= t1:
            k = ease((t - t0) / (t1 - t0)) if t1 > t0 else 1.0
            return v0 * (1 - k) + v1 * k if not isinstance(v0, Vector) else v0.lerp(v1, k)
    return points[-1][1]


class Pose:
    def __init__(self):
        self.rot = {}
        self.world = {}
        self.pelvis = Vector()
        self.feet = {}
        self.hands = {}
        self.extra = {}
        self.gun = None


class Skeleton:
    """Rest data and the pose solver."""

    def __init__(self, ch):
        rig = ch.rig
        self.rig = rig
        self.m = ch.marks
        bones = rig.data.bones
        self.rest = {b.name: b.matrix_local.copy() for b in bones}
        self.rest_rot = {n: mm.to_3x3() for n, mm in self.rest.items()}
        self.parent = {b.name: (b.parent.name if b.parent else None) for b in bones}
        self.head = {n: mm.translation.copy() for n, mm in self.rest.items()}
        self.tail = {b.name: b.tail_local.copy() for b in bones}
        self.order = []

        def visit(b):
            self.order.append(b.name)
            for c in b.children:
                visit(c)

        for b in bones:
            if b.parent is None:
                visit(b)
        self.chains = {}
        for s in "lr":
            self.chains[f"thigh_{s}"] = (f"thigh_{s}", f"calf_{s}", f"foot_{s}")
            self.chains[f"upperarm_{s}"] = (f"upperarm_{s}", f"lowerarm_{s}", f"hand_{s}")
        self.chain_rest = {}
        for key, (up, lo, end) in self.chains.items():
            s0, e0, w0 = self.head[up], self.head[lo], self.head[end]
            u0 = (w0 - s0).normalized()
            p0 = ((e0 - s0) - u0 * (e0 - s0).dot(u0)).normalized()
            n0 = u0.cross(p0).normalized()
            self.chain_rest[key] = ((e0 - s0).length, (w0 - e0).length, n0,
                                    (e0 - s0).normalized(), (w0 - e0).normalized())
        # Palms face the thighs in MakeHuman's rest pose.
        self.palm = {}
        for s, inward in (("l", Vector((1, 0, 0))), ("r", Vector((-1, 0, 0)))):
            y = (self.tail[f"hand_{s}"] - self.head[f"hand_{s}"]).normalized()
            self.palm[s] = (inward - y * inward.dot(y)).normalized()
        # Anatomical hand frames: the hand bones' tails run across the
        # knuckles, so use wrist -> middle knuckle and the knuckle line.
        self.rest_hand = {}
        for s, inward in (("l", Vector((1, 0, 0))), ("r", Vector((-1, 0, 0)))):
            wrist = self.head[f"hand_{s}"]
            along = (self.head[f"middle_01_{s}"] - wrist).normalized()
            across = (self.head[f"index_01_{s}"] - self.head[f"pinky_01_{s}"]).normalized()
            palm = along.cross(across).normalized()
            if palm.dot(inward) < 0:
                palm.negate()
            self.rest_hand[s] = (along, (palm - along * palm.dot(along)).normalized())
        self.ik_parts = {lo for (_, lo, _) in self.chains.values()} | {end for (_, _, end) in self.chains.values()}
        # Standing ankle height and leg length.
        self.ankle_z = (self.head["foot_l"].z + self.head["foot_r"].z) / 2
        self.leg = self.chain_rest["thigh_l"][0] + self.chain_rest["thigh_l"][1]

    # ------------------------------------------------------------- solve

    def solve(self, pose):
        """Posed armature-space matrices for every bone."""
        M = {}
        for name in self.order:
            if name in M:
                continue
            p = self.parent[name]
            mp = M[p] if p else Matrix.Identity(4)
            prest = self.rest[p] if p else Matrix.Identity(4)
            base = mp @ prest.inverted() @ self.rest[name]
            if name in self.chains:
                self._solve_chain(name, base.translation, pose, M)
                continue
            if name in pose.extra:
                M[name] = pose.extra[name]
                continue
            if name in pose.world:
                at = base.translation + (pose.pelvis if name == "pelvis" else Vector())
                M[name] = Matrix.Translation(at) @ pose.world[name].to_4x4()
                continue
            rr = self.rest_rot[name]
            b = Matrix.Identity(4)
            if name in pose.rot:
                b = (rr.inverted() @ pose.rot[name] @ rr).to_4x4()
            if name == "pelvis":
                b.translation = rr.inverted() @ pose.pelvis
            M[name] = base @ b
        return M

    def _solve_chain(self, up, s, pose, M):
        _, lo, end = self.chains[up]
        side = up[-1]
        a, b, n0, y0u, y0l = self.chain_rest[up]
        if up.startswith("thigh"):
            target, end_rot, pole = pose.feet[side]
        else:
            target, end_rot, pole = pose.hands[side]
        d_vec = target - s
        d = max(abs(a - b) + 1e-4, min(a + b - 1e-4, d_vec.length))
        u = d_vec.normalized()
        cos_a = (a * a + d * d - b * b) / (2 * a * d)
        sin_a = math.sqrt(max(0.0, 1 - cos_a * cos_a))
        p = pole - u * pole.dot(u)
        p.normalize()
        e = s + u * (a * cos_a) + p * (a * sin_a)
        w = s + u * d
        n = u.cross(p).normalized()
        r_up = frame((e - s).normalized(), n) @ frame(y0u, n0).transposed() @ self.rest_rot[up]
        r_lo = frame((w - e).normalized(), n) @ frame(y0l, n0).transposed() @ self.rest_rot[lo]
        M[up] = Matrix.Translation(s) @ r_up.to_4x4()
        M[lo] = Matrix.Translation(e) @ r_lo.to_4x4()
        M[end] = Matrix.Translation(w) @ end_rot.to_4x4()

    def basis(self, M):
        """Local (pose-bone) matrices from armature-space ones."""
        out = {}
        for name in self.order:
            p = self.parent[name]
            mp = M[p] if p else Matrix.Identity(4)
            prest = self.rest[p] if p else Matrix.Identity(4)
            out[name] = (mp @ prest.inverted() @ self.rest[name]).inverted() @ M[name]
        return out

    # ------------------------------------------------------------ helpers

    def foot_rot(self, side, delta):
        return delta @ self.rest_rot[f"foot_{side}"]

    def hand_rot(self, side, fingers, palm):
        """Absolute hand rotation with the hand *bone* along `fingers` and
        the palm facing `palm` (see `grip_rot` for the anatomical version)."""
        f = Vector(fingers).normalized()
        pl = Vector(palm)
        pl = (pl - f * pl.dot(f)).normalized()
        y0 = (self.tail[f"hand_{side}"] - self.head[f"hand_{side}"]).normalized()
        return frame(f, pl) @ frame(y0, self.palm[side]).transposed() @ self.rest_rot[f"hand_{side}"]

    def grip_rot(self, side, fingers, palm):
        """Absolute hand rotation with the fingers (wrist to middle
        knuckle) along `fingers` and the palm facing `palm`."""
        f = Vector(fingers).normalized()
        pl = Vector(palm)
        pl = (pl - f * pl.dot(f)).normalized()
        f0, p0 = self.rest_hand[side]
        return frame(f, pl) @ frame(f0, p0).transposed() @ self.rest_rot[f"hand_{side}"]

    def chest(self, M):
        """Maps rest-pose points attached to the chest to their posed place."""
        return M["spine_03"] @ self.rest["spine_03"].inverted()


class Grip:
    """A hand on the rifle, in the gun frame: wrist position, the direction
    the fingers point (wrist to middle knuckle), the palm normal, the elbow
    pole, and finger curls in degrees (index, middle, ring, pinky; thumb)."""

    def __init__(self, wrist, fingers, palm, pole, curl=(60, 65, 70, 72), thumb=(20, 25, 20)):
        self.wrist = Vector(wrist)
        self.fingers = Vector(fingers)
        self.palm = Vector(palm)
        self.pole = Vector(pole)
        self.curl = curl
        self.thumb = thumb

    def blend(self, other, k):
        def nlerp(a, b):
            v = a.normalized().lerp(b.normalized(), k)
            return v.normalized() if v.length > 1e-6 else b

        return Grip(self.wrist.lerp(other.wrist, k), nlerp(self.fingers, other.fingers),
                    nlerp(self.palm, other.palm), nlerp(self.pole, other.pole),
                    tuple(a + (b - a) * k for a, b in zip(self.curl, other.curl)),
                    tuple(a + (b - a) * k for a, b in zip(self.thumb, other.thumb)))


# Same holds as the first-person arms (arms.py, TAMARISK). Firing hand: palm
# against the right of the raked grip, the web at the top-back, index finger
# along the trigger. Support hand: cradling the lower handguard, palm up,
# fingers round the right side, thumb along the left.
GRIP_R = Grip((0.025, -0.065, -0.03), (-0.15, 0.94, -0.35), (-1.0, 0.0, 0.0), (0.8, -0.3, -0.6),
              curl=(25, 85, 90, 92), thumb=(40, 35, 30))
GRIP_L = Grip((-0.05, 0.27, -0.045), (0.85, 0.35, 0.25), (0.3, 0.0, 0.95), (-0.4, -0.2, -1.0),
              curl=(60, 65, 70, 72), thumb=(10, 15, 10))
# Left hand round the magazine (thumb on the release side, fingers over the
# front), and flat open (reaching).
MAG_L = Grip(MAGWELL + Vector((-0.04, 0.0, -0.085)), (0.3, 0.55, -0.75), (0.95, 0.1, 0.2), (-0.5, -0.3, -1.0),
             curl=(55, 60, 64, 68), thumb=(30, 25, 20))
OPEN_L = Grip(MAGWELL + Vector((-0.06, -0.02, -0.12)), (0.3, 0.6, -0.7), (0.9, 0.1, 0.3), (-0.5, -0.3, -1.0),
              curl=(20, 22, 25, 28), thumb=(10, 10, 5))


class Animator:
    def __init__(self, ch, mocap=None):
        self.ch = ch
        self.sk = Skeleton(ch)
        sk, m = self.sk, ch.marks
        self.m = m
        self.mocap = mocap
        self.feet0 = {s: sk.head[f"foot_{s}"].copy() for s in "lr"}
        # Standing stance: feet a little narrower than MakeHuman's.
        for s, sign in (("l", -1), ("r", 1)):
            self.feet0[s].x = sign * 0.12
        self.shoulder_r = sk.head["upperarm_r"].copy()
        self.p0 = Vector((0.0, m.chest.y, sk.head["upperarm_r"].z))
        # Front of the right eye (the aiming eye).
        self.eye_r = Vector((m.eye_half - 0.012, m.eye.y + 0.012, m.eye.z))
        self.clips = {}
        self.motions = {}
        self._mag_in_hand = None
        self._crouch_frame = None
        # World rotations of the standing pose (the overlays are posed on it).
        M = sk.solve(self._body_only(self.standing(0.0, breathe=False)))
        self.stand_world = {n: M[n].to_3x3() for n in ("pelvis", "spine_01", "spine_02", "spine_03",
                                                        "neck_01", "head")}

    # ----------------------------------------------------------- gun poses

    def gun_rest(self, kind):
        """Gun frame in rest-pose armature space (attached to the chest)."""
        sh = self.shoulder_r
        if kind == "ready":
            # Low ready: butt in the shoulder, muzzle down about 30 degrees.
            rot = look((-0.12, 0.86, -0.5), (0.0, 0.5, 0.86))
            butt = sh + Vector((-0.08, 0.09, -0.06))
        elif kind == "sprint":
            # Across the chest, muzzle down to the left.
            rot = look((-0.55, 0.55, -0.62), (0.15, 0.6, 0.78))
            butt = sh + Vector((0.0, 0.13, -0.1))
        elif kind == "reload":
            # Stock tucked under the right arm, muzzle forward-left and a
            # little up, rolled left: the magazine well in front of the chest,
            # under the eyes.
            rot = look((-0.25, 0.95, 0.15), (-0.55, 0.05, 0.84))
            butt = sh + Vector((-0.04, -0.05, -0.2))
        else:
            raise ValueError(kind)
        g = rot.to_4x4()
        g.translation = butt - rot @ BUTT
        return g

    def hands_on_gun(self, pose, G, left=None, right=None):
        """Right hand on the grip, left on the handguard (or the given
        Grips), both in the gun frame `G`."""
        rot = G.to_3x3()
        for side, grip in (("r", right or GRIP_R), ("l", left or GRIP_L)):
            pose.hands[side] = (G @ grip.wrist, self.sk.grip_rot(side, rot @ grip.fingers, rot @ grip.palm),
                                rot @ grip.pole)
            self.fingers(pose, side, grip)
        # Shoulders forward a little, towards the gun.
        pose.rot.setdefault("clavicle_l", R("Z", -10.0))
        pose.rot.setdefault("clavicle_r", R("Z", 6.0))
        pose.gun = G

    def fingers(self, pose, side, grip):
        sk = self.sk
        palm = sk.rest_hand[side][1]
        for f, c in zip(FINGERS, grip.curl):
            for k, scale in (("01", 0.8), ("02", 1.1), ("03", 0.8)):
                name = f"{f}_{k}_{side}"
                y = (sk.tail[name] - sk.head[name]).normalized()
                pose.rot[name] = R(y.cross(palm).normalized(), c * scale)
        for k, deg in zip(("01", "02", "03"), grip.thumb):
            name = f"thumb_{k}_{side}"
            y = (sk.tail[name] - sk.head[name]).normalized()
            pose.rot[name] = R(y.cross(palm).normalized(), deg)

    # -------------------------------------------------------------- legs

    def plant(self, pose, side, pos, pitch=0.0, yaw=0.0, pole=None):
        pole = pole if pole is not None else Vector((0.12 if side == "r" else -0.12, 1, 0))
        pose.feet[side] = (pos, self.sk.foot_rot(side, R("Z", yaw) @ R("X", pitch)), pole)

    # ------------------------------------------------------------- poses

    def standing(self, t, breathe=True):
        pose = Pose()
        b = math.sin(2 * math.pi * t / 3.0) if breathe else 0.0
        pose.pelvis = Vector((0.006 * math.sin(2 * math.pi * t / 3.0 + 1.0), 0.0, -0.025))
        pose.rot["spine_03"] = R("X", 1.0 * b)
        pose.rot["spine_02"] = R("X", 0.4 * b)
        pose.rot["head"] = R("Z", 4.0 * math.sin(2 * math.pi * t / 3.0 + 0.5)) @ R("X", -2.0)
        for s in "lr":
            self.plant(pose, s, self.feet0[s] + Vector((0, 0.03 if s == "l" else -0.03, 0)),
                       yaw=(8 if s == "l" else -8))
        return pose

    def finish(self, pose, gun="ready", bob=Vector(), left=None, gun_extra=None):
        """Place the gun on the chest and the hands on the gun."""
        M = self.sk.solve(self._body_only(pose))
        G = self.sk.chest(M) @ Matrix.Translation(bob) @ self.gun_rest(gun)
        if gun_extra is not None:
            G = G @ gun_extra
        self.hands_on_gun(pose, G, left)
        return pose

    def _body_only(self, pose):
        """A copy of `pose` whose arms hang at rest (to find the chest)."""
        p = Pose()
        p.rot, p.world, p.pelvis, p.feet = pose.rot, pose.world, pose.pelvis, pose.feet
        for s in "lr":
            p.hands[s] = (self.sk.head[f"hand_{s}"], self.sk.rest_rot[f"hand_{s}"],
                          Vector((0, -1, 0)))
        return p

    # ---------------------------------------------------------- mocap

    def motion(self, name):
        """The retargeted, looped clip for `name` (see MOCAP)."""
        if name not in self.motions:
            if self.mocap is None:
                raise RuntimeError("no motion capture folder given (build_characters.py --mocap DIR)")
            spec = MOCAP[name]
            bvh = retarget.load(os.path.join(self.mocap, CMU_DIR, spec.clip + ".bvh"))
            a, b = retarget.cycle(bvh, spec.window, spec.length)
            mo = retarget.Motion(bvh, self.sk, a, b, FPS, time_scale=spec.time_scale, travel=spec.travel)
            before = (mo.tilt("pelvis"), mo.tilt("spine_02"))
            mo.normalise(self.stand_world, pelvis=spec.pelvis, chest=spec.chest, head=spec.head)
            if spec.hips is not None:
                mo.lower(spec.hips * (self.sk.head["thigh_l"].z + self.sk.head["thigh_r"].z) / 2)
            print(f"  {name}: CMU {spec.clip} {a / bvh.fps:.2f}-{b / bvh.fps:.2f} s, "
                  f"{mo.n} frames, scale {mo.scale:.4f}, feet {mo.speed:.2f} m/s "
                  f"(travel {mo.travel_speed:.2f}), pelvis/chest tilt {before[0]:.0f}/{before[1]:.0f} deg "
                  f"-> {mo.tilt('pelvis'):.0f}/{mo.tilt('spine_02'):.0f}")
            self.motions[name] = mo
        return self.motions[name]

    def mocap_pose(self, name, t):
        return self.frame_pose(self.motion(name).frame(t, FPS))

    def frame_pose(self, fr):
        pose = Pose()
        for bone in ("pelvis", "spine_01", "spine_02", "spine_03", "neck_01", "head", "ball_l", "ball_r"):
            pose.world[bone] = fr.rot[bone]
        pose.pelvis = fr.pelvis
        for s in "lr":
            pose.feet[s] = (fr.ankle[s], fr.rot[f"foot_{s}"], fr.pole[s])
        return pose

    # ------------------------------------------------------------- clips

    def idle(self, t):
        return self.finish(self.mocap_pose("idle", t))

    def walk(self, t):
        return self.finish(self.mocap_pose("walk", t))

    def run(self, t):
        return self.finish(self.mocap_pose("run", t), gun="sprint")

    def crouch_stance(self):
        """Frame of the crouch walk to hold for crouch_idle: both feet down
        (heel or ball within 2 cm), left foot ahead, the widest stance."""
        mo = self.motion("crouch_walk")
        best = None
        for k in range(mo.n):
            down = max(min(mo.foot_points(k, s)[1]) for s in "lr")
            ahead = mo.frames[k].ankle["l"].y - mo.frames[k].ankle["r"].y
            score = (down > 0.02, -ahead)
            if best is None or score < best[0]:
                best = (score, k)
        return best[1]

    def crouch_idle(self, t):
        """The crouch walk's stance (left foot ahead), held: the front foot
        flat, the back one on its ball, breathing and a slow sway."""
        mo = self.motion("crouch_walk")
        if self._crouch_frame is None:
            self._crouch_frame = self.crouch_stance()
        k = self._crouch_frame
        pose = self.frame_pose(mo.frames[k])
        sk = self.sk
        for s in "lr":
            heel_h, ball_h = mo.foot_points(k, s)[1]
            if heel_h > ball_h + 0.01:
                continue  # on the ball of the foot: keep
            ankle, rot, pole = pose.feet[s]
            fwd = (rot @ sk.rest_rot[f"foot_{s}"].inverted()) @ Vector((0, 1, 0))
            yaw = R("Z", math.degrees(math.atan2(-fwd.x, fwd.y)))
            pose.feet[s] = (Vector((ankle.x, ankle.y, sk.head[f"foot_{s}"].z)),
                            yaw @ sk.rest_rot[f"foot_{s}"], pole)
            pose.world[f"ball_{s}"] = yaw @ sk.rest_rot[f"ball_{s}"]
        w = 2 * math.pi * t / 3.0
        pose.pelvis = pose.pelvis + Vector((0.008 * math.sin(w + 1.0), 0.0, 0.004 * math.sin(w)))
        pose.world["spine_03"] = R("X", 1.0 * math.sin(w)) @ pose.world["spine_03"]
        pose.world["head"] = R("Z", 4.0 * math.sin(w + 0.5)) @ pose.world["head"]
        return self.finish(pose)

    def crouch_walk(self, t):
        return self.finish(self.mocap_pose("crouch_walk", t))

    def prone_body(self, t, crawl=None):
        """Lying on the belly, head up, rifle forward. `crawl` = phase 0..1."""
        pose = Pose()
        b = math.sin(2 * math.pi * t / 3.0)
        pelvis_head = self.sk.head["pelvis"]
        target = Vector((0.0, -0.32, 0.15))
        sway = 0.0 if crawl is None else math.sin(2 * math.pi * crawl)
        yaw = 0.0 if crawl is None else 6.0 * sway
        pose.pelvis = target - pelvis_head + Vector((0.03 * sway, 0, 0.004 * b))
        pose.rot["pelvis"] = R("Z", yaw) @ R("X", -88.0)
        pose.rot["spine_01"] = R("X", 4.0) @ R("Z", -yaw * 0.6)
        pose.rot["spine_02"] = R("X", 8.0) @ R("Z", -yaw * 0.4)
        pose.rot["spine_03"] = R("X", 10.0 + 0.6 * b)
        if crawl is None:
            # Aiming: chin down to the stock, cheek on the right.
            pose.rot["neck_01"] = R("X", 30.0) @ R("Y", 8.0)
            pose.rot["head"] = R("X", 12.0) @ R("Y", 10.0)
        else:
            pose.rot["neck_01"] = R("X", 32.0)
            pose.rot["head"] = R("X", 26.0)
        hip_y = target.y
        for side, sign in (("l", -1), ("r", 1)):
            pos = Vector((sign * 0.17, hip_y - self.sk.leg * 0.97, 0.09))
            pole = Vector((sign * 0.3, 0, -1))
            if crawl is not None:
                ph = (crawl + (0.0 if side == "l" else 0.5)) % 1.0
                draw = math.sin(math.pi * min(1.0, ph / 0.5)) if ph < 0.5 else 0.0
                pos = pos + Vector((sign * 0.16 * draw, 0.36 * draw, 0.02 * draw))
                pole = Vector((sign * (0.3 + 1.2 * draw), 0, -1))
            self.plant(pose, side, pos, pitch=-105.0, pole=pole)
        return pose

    def prone_idle(self, t):
        """Prone, aiming: sight line through the eye, elbows down."""
        pose = self.prone_body(t)
        pose.rot["clavicle_r"] = R("Z", 14.0) @ R("Y", -8.0)
        pose.rot["clavicle_l"] = R("Z", -18.0)
        G = self.eye_gun(pose, pitch=-2.0)
        self.hands_on_gun(pose, G)
        for s in "lr":
            wrist, hrot, _ = pose.hands[s]
            pose.hands[s] = (wrist, hrot, Vector((0.5 if s == "r" else -0.5, 0, -1)))
        return pose

    def prone_crawl(self, t, cycle):
        """Low crawl: the rifle held across the forearms in front of the
        face, elbows and knees pulling in turn."""
        ph = (t / cycle) % 1.0
        pose = self.prone_body(0.0, crawl=ph)
        M = self.sk.solve(self._body_only(pose))
        sh = M["upperarm_r"].translation
        reach = 0.05 * math.sin(2 * math.pi * ph)
        rot = look((-0.12, 1, -0.05), (0.15, 0, 1))
        G = rot.to_4x4()
        G.translation = sh + Vector((-0.12, 0.2 + reach, -0.06)) - rot @ BUTT
        self.hands_on_gun(pose, G)
        for s in "lr":
            wrist, hrot, _ = pose.hands[s]
            pose.hands[s] = (wrist, hrot, Vector((0.6 if s == "r" else -0.6, 0, -1)))
        return pose

    def lean(self, side):
        """Spine_01/02 tilted to the side (the game masks the clip to them;
        the chest, head and arms come from whatever else is playing)."""
        pose = self.standing(0.0, breathe=False)
        k = -1 if side == "left" else 1
        for bone, deg in (("spine_01", 12), ("spine_02", 14)):
            pose.rot[bone] = R("Y", k * deg) @ pose.rot.get(bone, Matrix.Identity(3))
        return self.finish(pose)

    # ------------------------------------------------------------ aiming

    def aim_body(self, kick=0.0, aim=None):
        """Standing, shouldering the rifle: chest bladed to the right and a
        little forward (the right shoulder pocket comes back to the butt),
        right shoulder up, the head turned back to the front, chin down and
        tilted onto the stock (see AIM)."""
        a = aim or AIM
        pose = self.standing(0.0, breathe=False)
        pose.rot["spine_03"] = R("Z", -a["blade"]) @ R("X", -a["chest"] + 2.0 * kick)
        turn = a["blade"] / 2
        pose.rot["neck_01"] = R("Z", turn) @ R("X", -a["neck"]) @ R("Y", a["tilt"])
        pose.rot["head"] = R("Z", turn) @ R("X", -a["head"]) @ R("Y", a["tilt"])
        pose.rot["clavicle_r"] = R("Z", a["shoulder_fwd"]) @ R("Y", -a["shoulder_up"])
        pose.rot["clavicle_l"] = R("Z", -18.0) @ R("Y", 4.0)
        return pose

    def eye(self, M):
        return M["head"] @ self.sk.rest["head"].inverted() @ self.eye_r

    def eye_gun(self, pose, pitch=0.0):
        """Gun frame with the sight line through the right eye, the barrel
        straight ahead (+Y), pitched by `pitch` degrees."""
        M = self.sk.solve(self._body_only(pose))
        rot = R("X", pitch) @ look((0, 1, 0), (0, 0, 1))
        G = rot.to_4x4()
        G.translation = self.eye(M) - rot @ EYE
        return G

    def fire(self, t, cycle):
        """Aimed, one recoil kick per cycle (the game scales the speed to
        the weapon's rate of fire)."""
        ph = (t / cycle) % 1.0
        kick = (ph / 0.15) if ph < 0.15 else math.exp(-(ph - 0.15) * 7.0)
        if t >= cycle - 1e-6:
            kick = 0.0
        pose = self.aim_body(kick)
        recoil = Matrix.Translation((0, -0.02 * kick, 0.003 * kick)) @ R("X", 2.5 * kick).to_4x4()
        G = self.eye_gun(pose) @ recoil
        self.hands_on_gun(pose, G)
        return pose

    def reload(self, t):
        """Tactical reload, 2.4 s: tilt the rifle, look at the magazine
        well, strip the magazine, stow it, seat a new one, back to ready."""
        look_k = keys([(0.0, 0.0), (0.3, 1.0), (1.7, 1.0), (2.1, 0.0)], t)
        pose = self.standing(0.0, breathe=False)
        pose.rot["spine_03"] = R("X", -6.0 * look_k)
        pose.rot["neck_01"] = R("X", -14.0 * look_k) @ R("Z", -4.0 * look_k)
        pose.rot["head"] = R("X", (-2.0 - 16.0 * look_k)) @ R("Z", -8.0 * look_k)
        tilt = keys([(0.0, 0.0), (0.3, 1.0), (1.55, 1.0), (2.05, 0.0)], t)
        M = self.sk.solve(self._body_only(pose))
        chest = self.sk.chest(M)
        ga, gb = self.gun_rest("ready"), self.gun_rest("reload")
        rot = Quaternion(ga.to_quaternion()).slerp(gb.to_quaternion(), tilt).to_matrix()
        G = rot.to_4x4()
        G.translation = ga.translation.lerp(gb.translation, tilt)
        G = chest @ G
        # Left hand path, gun frame (the pouch is fixed on the chest).
        pouch = G.inverted() @ (chest @ (self.p0 + Vector((-0.08, 0.2, -0.33))))
        on_mag = MAG_L.wrist
        # An AK magazine rocks out: front lip forward, then down.
        out = on_mag + Vector((0.0, 0.06, -0.1))
        path = [
            (0.0, GRIP_L.wrist),
            (0.32, on_mag),
            (0.52, out),
            (0.78, pouch),
            (0.98, pouch),
            (1.18, out + Vector((0.0, 0.0, -0.04))),
            (1.32, out),
            (1.5, on_mag),
            (1.6, on_mag + Vector((0, 0, 0.01))),
            (2.1, GRIP_L.wrist),
        ]
        wrist = keys(path, t)
        hold = keys([(0.0, 0.0), (0.22, 0.0), (0.32, 1.0), (1.55, 1.0), (1.75, 0.0)], t)
        to_grip = keys([(0.0, 1.0), (0.2, 0.0), (1.85, 0.0), (2.1, 1.0)], t)
        grip = OPEN_L.blend(MAG_L, hold).blend(GRIP_L, to_grip)
        grip.wrist = wrist
        self.hands_on_gun(pose, G, left=grip)
        # The magazine leaves the well while the left hand holds it.
        if 0.32 <= t <= 1.5:
            Mh = self.sk.solve(pose)
            hand = Mh["hand_l"]
            if self._mag_in_hand is None:
                well = Mh["weapon_r"] @ self.sk.rest["weapon_r"].inverted() @ self.sk.rest["weapon_mag"]
                self._mag_in_hand = hand.inverted() @ well
            pose.extra["weapon_mag"] = hand @ self._mag_in_hand
        return pose

    # --------------------------------------------------------- recording

    def record(self, name, length, fn, loop=True, speed=None):
        rig = self.ch.rig
        sk = self.sk
        frames = max(2, round(length * FPS))
        action = bpy.data.actions.new(name)
        rig.animation_data_create()
        rig.animation_data.action = action
        prev = {}
        self._mag_in_hand = None
        for f in range(frames + 1):
            t = f / FPS
            pose = fn(t)
            M = sk.solve(pose)
            B = sk.basis(M)
            for pb in rig.pose.bones:
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
        info = {"length": frames / FPS, "loop": loop}
        if speed:
            info["speed"] = round(speed, 3)
        if name in self.motions:
            clip, a, b = self.motions[name].source
            info["source"] = f"CMU {clip} {a:.2f}-{b:.2f} s"
        elif name == "crouch_idle":
            clip, a, b = self.motions["crouch_walk"].source
            info["source"] = f"CMU {clip} (crouch_walk frame {self._crouch_frame}, held)"
        self.clips[name] = info
        return action

    def record_mocap(self, name, fn):
        mo = self.motion(name)
        return self.record(name, mo.length, fn, speed=mo.speed if mo.speed > 0 else None)


def add_weapon_bones(ch):
    """`weapon_r` (child of hand_r) is where the rifle sits: +Y along the
    barrel, +Z up, origin at the top of the grip. `weapon_mag` (child of
    weapon_r) holds the magazine so reloads can move it."""
    sk = Skeleton(ch)
    # Hand frame relative to the gun: build the hand from GRIP_R with the gun
    # at identity and invert it.
    hand = Matrix.Translation(GRIP_R.wrist) @ sk.grip_rot("r", GRIP_R.fingers, GRIP_R.palm).to_4x4()
    hand_rest = sk.rest["hand_r"]
    weapon_rest = hand_rest @ hand.inverted()
    rig = ch.rig
    bpy.context.view_layer.objects.active = rig
    bpy.ops.object.mode_set(mode="EDIT")
    ebs = rig.data.edit_bones
    for name, parent, mat in (("weapon_r", "hand_r", weapon_rest),
                              ("weapon_mag", "weapon_r", weapon_rest @ Matrix.Translation(MAGWELL))):
        eb = ebs.new(name)
        eb.head = (0, 0, 0)
        eb.tail = (0, 0.08, 0)
        eb.matrix = mat
        eb.parent = ebs[parent]
        eb.use_connect = False
    bpy.ops.object.mode_set(mode="OBJECT")
    for pb in rig.pose.bones:
        pb.rotation_mode = "QUATERNION"


def build_prop(ch, gear_mod, surfaces):
    """A blockout of the TAMARISK (AK pattern) skinned to weapon_r /
    weapon_mag. The game hides it once the real model is attached."""
    g = gear_mod.Gear(ch, surfaces)
    g.force_material = "hard"
    rest = {b.name: b.matrix_local.copy() for b in ch.rig.data.bones}
    W = rest["weapon_r"]
    c = ch.colours
    w = {"weapon_r": 1.0}
    T = Matrix.Translation

    def at(x, y, z, rx=0.0):
        return W @ T((x, y, z)) @ Matrix.Rotation(math.radians(rx), 4, "X")

    g.box(at(0, 0.075, 0.022), (0.036, 0.27, 0.07), c["metal"], weights=w, bevel=0.004)          # receiver
    g.box(at(0, 0.305, 0.017), (0.04, 0.15, 0.042), c["gear"], weights=w, bevel=0.008)          # lower handguard
    g.box(at(0, 0.32, 0.06), (0.03, 0.18, 0.026), c["gear"], weights=w, bevel=0.006)            # gas tube cover
    g.cylinder(at(0, 0.5, 0.03, -90), 0.009, 0.24, c["metal"], weights=w, segments=8)            # barrel
    g.cylinder(at(0, 0.612, 0.03, -90), 0.013, 0.035, c["dark"], weights=w, segments=8)          # muzzle brake
    g.box(at(0, 0.572, 0.054), (0.012, 0.03, 0.048), c["metal"], weights=w)                      # front sight
    g.box(at(0, 0.205, 0.067), (0.02, 0.04, 0.02), c["metal"], weights=w)                        # rear sight
    g.box(at(0, -0.16, 0.0, -8), (0.034, 0.2, 0.05), c["gear"], weights=w, bevel=0.006)         # stock
    g.box(at(0, -0.24, -0.015), (0.036, 0.03, 0.1), c["gear"], weights=w, bevel=0.006)          # butt
    g.box(at(0, -0.015, -0.05, -17), (0.028, 0.036, 0.1), c["dark"], weights=w, bevel=0.004)    # grip
    g.box(at(0, 0.045, -0.025), (0.012, 0.07, 0.02), c["metal"], weights=w)                      # trigger guard
    M = rest["weapon_mag"]
    m = {"weapon_mag": 1.0}
    g.box(M @ T((0, 0.035, -0.045)) @ Matrix.Rotation(math.radians(15), 4, "X"), (0.026, 0.07, 0.1),
          c["dark"], weights=m, bevel=0.003)
    g.box(M @ T((0, 0.075, -0.13)) @ Matrix.Rotation(math.radians(32), 4, "X"), (0.026, 0.066, 0.09),
          c["dark"], weights=m, bevel=0.003)
    return g.to_object("weapon", ch.materials)


def build(ch, gear_mod, surfaces, mocap=None):
    """Add weapon bones and the prop, then record every clip. `mocap` is
    the staging folder holding cmu_mocap_bvh_selection/."""
    add_weapon_bones(ch)
    ch.prop = build_prop(ch, gear_mod, surfaces)
    a = Animator(ch, mocap)
    scene = bpy.context.scene
    scene.render.fps = FPS
    crawl_cycle = 1.6
    a.record_mocap("idle", a.idle)
    a.record_mocap("walk", a.walk)
    a.record_mocap("run", a.run)
    a.record("crouch_idle", 3.0, a.crouch_idle)
    a.record_mocap("crouch_walk", a.crouch_walk)
    a.record("prone_idle", 3.0, a.prone_idle)
    a.record("prone_crawl", crawl_cycle, lambda t: a.prone_crawl(t, crawl_cycle), speed=0.8)
    a.record("lean_left", 0.5, lambda t: a.lean("left"))
    a.record("lean_right", 0.5, lambda t: a.lean("right"))
    a.record("fire", 0.2, lambda t: a.fire(t, 0.2))
    a.record("reload", 2.4, a.reload, loop=False)
    ch.actions = [t.strips[0].action for t in ch.rig.animation_data.nla_tracks]
    ch.rig["harrow_clips"] = json.dumps(a.clips, sort_keys=True)
    ch.rig["harrow_masks"] = json.dumps({
        "spine": list(SPINE_GROUP),
        "upper": list(UPPER_GROUP),
        "arms": sorted(n for n in a.sk.order if n.startswith(
            ("clavicle", "upperarm", "lowerarm", "hand", "thumb", "weapon") + FINGERS)),
    })
    return a
