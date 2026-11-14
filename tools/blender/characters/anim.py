"""Scripted placeholder animations, baked per operator.

No CC0 animation set we could fetch without a login matches this rig, so
the clips are procedural. Each frame is posed in *armature axes* (Z up, the
character faces +Y, its right is +X), which keeps the authoring independent
of how MPFB rolled the bones:

- Spine, neck, head, pelvis, clavicles, fingers: rotations relative to the
  parent, expressed as if the parent were at rest (`Pose.rot`).
- Legs and arms: analytic two-bone IK to an ankle/wrist target with a pole
  direction for the knee/elbow (`Pose.feet`, `Pose.hands`).
- The rifle: a frame `G` attached to the chest. The right hand grips it at
  a fixed offset, the left hand supports the handguard (or does the reload).

We evaluate the skeleton ourselves (no constraints, no baking) and key the
resulting local rotations on every frame. Clips are in place (no root
motion). Lengths and the speed each locomotion clip was authored for are
written to the rig's `harrow_clips` extra so the game can match playback
speed to movement speed.

Clip list (names are the glTF animation names):

  idle, walk, run, crouch_idle, crouch_walk, prone_idle, prone_crawl  full body
  lean_left, lean_right    spine/neck/head pose (game masks it to those bones)
  fire                     arms only: aim + one recoil kick (loops per shot)
  reload                   arms only: magazine swap, one-shot
"""

import json
import math

import bpy
from mathutils import Matrix, Quaternion, Vector

FPS = 30

# Rifle prop layout, in the gun frame: origin at the top of the pistol grip
# (where the web of the right hand sits), +Y along the barrel, +Z up.
BUTT = Vector((0.0, -0.26, 0.03))
MAGWELL = Vector((0.0, 0.075, -0.035))        # top of the magazine

SPINE_GROUP = ("spine_01", "spine_02", "spine_03", "neck_01", "head")
FINGERS = ("index", "middle", "ring", "pinky")


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
        """Absolute hand rotation with the hand's long axis along `fingers`
        and the palm facing `palm`."""
        f = Vector(fingers).normalized()
        pl = Vector(palm)
        pl = (pl - f * pl.dot(f)).normalized()
        y0 = (self.tail[f"hand_{side}"] - self.head[f"hand_{side}"]).normalized()
        return frame(f, pl) @ frame(y0, self.palm[side]).transposed() @ self.rest_rot[f"hand_{side}"]

    def chest(self, M):
        """Maps rest-pose points attached to the chest to their posed place."""
        return M["spine_03"] @ self.rest["spine_03"].inverted()


# Right hand on the grip and left hand on the handguard, in the gun frame:
# (wrist position, finger direction, palm normal).
GRIP_R = (Vector((0.018, -0.07, -0.045)), Vector((-0.2, 0.9, -0.35)), Vector((-1.0, 0.0, 0.1)))
GRIP_L = (Vector((-0.05, 0.13, -0.055)), Vector((0.75, 0.55, 0.25)), Vector((0.35, -0.1, 0.95)))


class Animator:
    def __init__(self, ch):
        self.ch = ch
        self.sk = Skeleton(ch)
        sk, m = self.sk, ch.marks
        self.m = m
        self.feet0 = {s: sk.head[f"foot_{s}"].copy() for s in "lr"}
        # Standing stance: feet a little narrower than MakeHuman's.
        for s, sign in (("l", -1), ("r", 1)):
            self.feet0[s].x = sign * 0.12
        self.shoulder_r = sk.head["upperarm_r"].copy()
        self.p0 = Vector((0.0, m.chest.y, sk.head["upperarm_r"].z))
        self.hip_z = sk.head["thigh_l"].z
        self.clips = {}

    # ----------------------------------------------------------- gun poses

    def gun_rest(self, kind):
        """Gun frame in rest-pose armature space (attached to the chest)."""
        sh = self.shoulder_r
        if kind == "aim":
            rot = look((-0.03, 1, 0), (0, 0, 1))
            butt = sh + Vector((-0.06, 0.075, -0.02))
        elif kind == "ready":
            rot = look((-0.1, 0.86, -0.5), (0.0, 0.5, 0.86))
            butt = sh + Vector((-0.07, 0.09, -0.08))
        elif kind == "sprint":
            rot = look((-0.55, 0.55, -0.62), (0.15, 0.6, 0.78))
            butt = sh + Vector((0.02, 0.12, -0.12))
        elif kind == "reload":
            rot = look((-0.12, 0.92, 0.2), (-0.5, 0.0, 0.86))
            butt = sh + Vector((-0.09, 0.07, -0.08))
        else:
            raise ValueError(kind)
        g = rot.to_4x4()
        g.translation = butt - rot @ BUTT
        return g

    def hands_on_gun(self, pose, G, left=None):
        """Right hand on the grip, left on the handguard (or `left` =
        (wrist, fingers, palm) in the gun frame)."""
        rot = G.to_3x3()
        for side, grip in (("r", GRIP_R), ("l", left or GRIP_L)):
            wrist, fingers, palm = grip
            sign = -1 if side == "l" else 1
            pole = rot @ Vector((sign * 0.6, -0.4, -1.0))
            pose.hands[side] = (G @ wrist, self.sk.hand_rot(side, rot @ fingers, rot @ palm), pole)
        # Shoulders forward a little, towards the gun.
        pose.rot["clavicle_l"] = R("Z", -10.0)
        pose.rot["clavicle_r"] = R("Z", 6.0)
        pose.gun = G

    def grip_fingers(self, pose, curl=62.0, side_curl=None):
        sk = self.sk
        for s in "lr":
            c = curl if side_curl is None else side_curl.get(s, curl)
            for f in FINGERS:
                for k, scale in (("01", 0.8), ("02", 1.1), ("03", 0.8)):
                    name = f"{f}_{k}_{s}"
                    y = (sk.tail[name] - sk.head[name]).normalized()
                    hinge = y.cross(sk.palm[s]).normalized()
                    pose.rot[name] = R(hinge, c * scale)
            for k, deg in (("01", 15), ("02", 20), ("03", 15)):
                name = f"thumb_{k}_{s}"
                y = (sk.tail[name] - sk.head[name]).normalized()
                hinge = y.cross(sk.palm[s]).normalized()
                pose.rot[name] = R(hinge, deg)

    # -------------------------------------------------------------- legs

    def plant(self, pose, side, pos, pitch=0.0, yaw=0.0, pole=None):
        pole = pole if pole is not None else Vector((0.12 if side == "r" else -0.12, 1, 0))
        pose.feet[side] = (pos, self.sk.foot_rot(side, R("Z", yaw) @ R("X", pitch)), pole)

    def gait(self, pose, phase, stride, lift, stance, width, pelvis_rot, back=0.0, toe_out=6.0):
        """Feet for one locomotion cycle. phase 0..1; the left foot lands at
        phase 0 and the right at 0.5. Stance feet move back at constant
        speed (`stride` metres during `stance` of the cycle)."""
        for side, offset in (("l", 0.0), ("r", 0.5)):
            ph = (phase + offset) % 1.0
            sign = -1 if side == "l" else 1
            base = self.feet0[side].copy()
            base.x = sign * width
            if ph < stance:
                t = ph / stance
                y = stride / 2 - stride * t
                z = 0.0
                pitch = keys([(0.0, 10.0), (0.15, 0.0), (0.7, 0.0), (1.0, -22.0)], t)
            else:
                t = (ph - stance) / (1 - stance)
                y = -stride / 2 + stride * ease(t)
                z = lift * math.sin(math.pi * t) ** 1.3
                pitch = keys([(0.0, -22.0), (0.35, 0.0), (0.85, 12.0), (1.0, 10.0)], t)
            heel = max(0.0, -pitch) / 22.0 * 0.05
            pos = base + Vector((0, y - back, z + heel))
            yaw = -sign * toe_out
            pole = pelvis_rot @ Vector((sign * 0.15, 1, 0))
            self.plant(pose, side, pos, pitch=pitch, yaw=yaw, pole=pole)

    # ------------------------------------------------------------- clips

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
        self.grip_fingers(pose)
        return pose

    def _body_only(self, pose):
        """A copy of `pose` whose arms hang at rest (to find the chest)."""
        p = Pose()
        p.rot, p.pelvis, p.feet = pose.rot, pose.pelvis, pose.feet
        for s in "lr":
            p.hands[s] = (self.sk.head[f"hand_{s}"], self.sk.rest_rot[f"hand_{s}"],
                          Vector((0, -1, 0)))
        return p

    def idle(self, t):
        return self.finish(self.standing(t))

    def walk(self, t, cycle):
        ph = (t / cycle) % 1.0
        pose = Pose()
        bob = math.cos(4 * math.pi * ph)
        sway = math.sin(2 * math.pi * ph)
        pose.pelvis = Vector((0.015 * sway, 0.0, -0.085 + 0.018 * bob))
        twist = 5.0 * math.cos(2 * math.pi * ph)
        pose.rot["pelvis"] = R("Z", twist) @ R("X", -4.0)
        pose.rot["spine_01"] = R("Z", -twist * 0.5) @ R("X", 1.5)
        pose.rot["spine_02"] = R("Z", -twist * 0.3)
        pose.rot["spine_03"] = R("Z", -twist * 0.2) @ R("X", -1.0)
        pose.rot["head"] = R("X", -3.0)
        self.gait(pose, ph, stride=0.72, lift=0.13, stance=0.45, width=0.1,
                  pelvis_rot=R("Z", twist))
        return self.finish(pose, bob=Vector((0, 0, 0.008 * bob)))

    def run(self, t, cycle):
        ph = (t / cycle) % 1.0
        pose = Pose()
        bob = math.cos(4 * math.pi * ph)
        twist = 8.0 * math.cos(2 * math.pi * ph)
        pose.pelvis = Vector((0.0, 0.04, -0.085 + 0.03 * bob))
        pose.rot["pelvis"] = R("Z", twist) @ R("X", -10.0)
        pose.rot["spine_01"] = R("Z", -twist * 0.4) @ R("X", -3.0)
        pose.rot["spine_02"] = R("Z", -twist * 0.4) @ R("X", -2.0)
        pose.rot["spine_03"] = R("Z", -twist * 0.3)
        pose.rot["neck_01"] = R("X", 7.0)
        pose.rot["head"] = R("X", 8.0)
        self.gait(pose, ph, stride=1.0, lift=0.24, stance=0.34, width=0.09, pelvis_rot=R("Z", twist),
                  back=0.08)
        return self.finish(pose, gun="sprint", bob=Vector((0.012 * math.sin(2 * math.pi * ph), 0, 0.01 * bob)))

    def crouched(self, t):
        pose = Pose()
        b = math.sin(2 * math.pi * t / 3.0)
        drop = self.hip_z - 0.47
        pose.pelvis = Vector((0.0, -0.06, -drop))
        # Pelvis nearly upright and the spine close to its standing pose,
        # so the arm (aim) and spine (lean) overrides, which were authored
        # standing, still line up when crouched.
        pose.rot["pelvis"] = R("X", -6.0)
        pose.rot["spine_01"] = R("X", 2.0)
        pose.rot["spine_02"] = R("X", 2.0)
        pose.rot["spine_03"] = R("X", 1.0 + 0.8 * b)
        pose.rot["head"] = R("X", 3.0) @ R("Z", 3.0 * math.sin(2 * math.pi * t / 3.0 + 0.5))
        return pose

    def crouch_idle(self, t):
        pose = self.crouched(t)
        fl = self.feet0["l"] + Vector((-0.04, 0.16, 0))
        fr = self.feet0["r"] + Vector((0.03, -0.24, 0.035))
        self.plant(pose, "l", fl, yaw=10, pole=Vector((-0.3, 1, 0.1)))
        self.plant(pose, "r", fr, pitch=-28, yaw=-12, pole=Vector((0.35, 1, -0.2)))
        return self.finish(pose)

    def crouch_walk(self, t, cycle):
        ph = (t / cycle) % 1.0
        pose = self.crouched(0.0)
        bob = math.cos(4 * math.pi * ph)
        pose.pelvis += Vector((0.012 * math.sin(2 * math.pi * ph), 0.0, 0.012 * bob))
        twist = 4.0 * math.cos(2 * math.pi * ph)
        pose.rot["pelvis"] = R("Z", twist) @ R("X", -6.0)
        self.gait(pose, ph, stride=0.9, lift=0.11, stance=0.6, width=0.14, pelvis_rot=R("Z", twist),
                  back=0.06, toe_out=10.0)
        for s in "lr":
            pos, rot, pole = pose.feet[s]
            pose.feet[s] = (pos, rot, (pole + Vector((0.4 if s == "r" else -0.4, 0, 0))).normalized())
        return self.finish(pose, bob=Vector((0, 0, 0.006 * bob)))

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
        pose.rot["neck_01"] = R("X", 32.0)
        pose.rot["head"] = R("X", 30.0)
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

    def prone_finish(self, pose, M_extra=0.0):
        """Rifle held level in front of the face (world frame, not the chest)."""
        M = self.sk.solve(self._body_only(pose))
        sh = M["upperarm_r"].translation
        rot = look((-0.02, 1, -0.02), (0, 0, 1))
        G = rot.to_4x4()
        G.translation = sh + Vector((-0.05, 0.08 + M_extra, 0.0)) - rot @ BUTT
        self.hands_on_gun(pose, G)
        for s in "lr":
            wrist, hrot, _ = pose.hands[s]
            pose.hands[s] = (wrist, hrot, Vector((0.6 if s == "r" else -0.6, 0, -1)))
        self.grip_fingers(pose)
        return pose

    def prone_idle(self, t):
        return self.prone_finish(self.prone_body(t))

    def prone_crawl(self, t, cycle):
        ph = (t / cycle) % 1.0
        pose = self.prone_body(0.0, crawl=ph)
        return self.prone_finish(pose, M_extra=0.05 * math.sin(2 * math.pi * ph))

    def lean(self, side):
        pose = self.standing(0.0, breathe=False)
        k = -1 if side == "left" else 1
        for bone, deg in (("spine_01", 8), ("spine_02", 9), ("spine_03", 10), ("neck_01", -6), ("head", -7)):
            pose.rot[bone] = R("Y", k * deg) @ pose.rot.get(bone, Matrix.Identity(3))
        return self.finish(pose)

    def fire(self, t, cycle):
        """Aimed, one recoil kick per cycle (the game scales the speed to
        the weapon's rate of fire)."""
        pose = self.standing(0.0, breathe=False)
        ph = (t / cycle) % 1.0
        kick = (ph / 0.15) if ph < 0.15 else math.exp(-(ph - 0.15) * 7.0)
        if t >= cycle - 1e-6:
            kick = 0.0
        pose.rot["head"] = R("X", -6.0) @ R("Y", 6.0)
        pose.rot["neck_01"] = R("X", -3.0)
        recoil = Matrix.Translation((0, -0.028 * kick, 0.004 * kick)) @ R("X", 3.5 * kick).to_4x4()
        return self.finish(pose, gun="aim", gun_extra=recoil)

    def reload(self, t):
        pose = self.standing(0.0, breathe=False)
        tilt = keys([(0.0, 0.0), (0.3, 1.0), (1.55, 1.0), (2.05, 0.0)], t)
        # Gun: blend the ready pose towards the reload pose.
        M = self.sk.solve(self._body_only(pose))
        chest = self.sk.chest(M)
        ga, gb = self.gun_rest("ready"), self.gun_rest("reload")
        rot = Quaternion(ga.to_quaternion()).slerp(gb.to_quaternion(), tilt).to_matrix()
        G = rot.to_4x4()
        G.translation = ga.translation.lerp(gb.translation, tilt)
        G = chest @ G
        rot = G.to_3x3()
        # Left hand path, gun frame (pouch is fixed on the chest).
        pouch_world = chest @ (self.p0 + Vector((-0.06, 0.2, -0.33)))
        pouch = G.inverted() @ pouch_world
        grab = MAGWELL + Vector((-0.045, 0.0, -0.11))
        path = [
            (0.0, GRIP_L[0]),
            (0.32, grab),
            (0.5, grab + Vector((-0.02, -0.02, -0.18))),
            (0.75, pouch),
            (0.95, pouch),
            (1.15, grab + Vector((-0.02, 0.0, -0.14))),
            (1.32, grab),
            (1.45, grab + Vector((0, 0, 0.012))),
            (1.65, Vector((-0.09, -0.02, -0.01))),
            (1.75, Vector((-0.07, -0.01, -0.01))),
            (2.1, GRIP_L[0]),
        ]
        wrist = keys(path, t)
        grip_w = keys([(0.0, 1.0), (0.2, 0.0), (1.9, 0.0), (2.1, 1.0)], t)
        fingers = GRIP_L[1].lerp(Vector((0.55, 0.15, 0.82)), 1 - grip_w)
        palm = GRIP_L[2].lerp(Vector((0.95, 0.0, -0.1)), 1 - grip_w)
        self.hands_on_gun(pose, G, left=(wrist, fingers, palm))
        self.grip_fingers(pose, side_curl={"l": keys([(0.0, 62.0), (0.25, 20.0), (0.32, 55.0), (1.35, 55.0),
                                                       (1.5, 15.0), (1.95, 15.0), (2.1, 62.0)], t)})
        # The magazine leaves the well while the left hand holds it.
        if 0.32 <= t <= 1.35:
            Mh = self.sk.solve(pose)
            hand = Mh["hand_l"]
            if not hasattr(self, "_mag_in_hand"):
                self._mag_in_hand = None
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
        self.clips[name] = {"length": frames / FPS, "loop": loop, **({"speed": speed} if speed else {})}
        return action


def add_weapon_bones(ch):
    """`weapon_r` (child of hand_r) is where the rifle sits: +Y along the
    barrel, +Z up, origin at the top of the grip. `weapon_mag` (child of
    weapon_r) holds the magazine so reloads can move it."""
    sk = Skeleton(ch)
    # Hand frame relative to the gun: build the hand from GRIP_R with the gun
    # at identity and invert it.
    wrist, fingers, palm = GRIP_R
    hand = Matrix.Translation(wrist) @ sk.hand_rot("r", fingers, palm).to_4x4()
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
    """A generic carbine blockout skinned to weapon_r / weapon_mag."""
    g = gear_mod.Gear(ch, surfaces)
    g.force_material = "hard"
    rest = {b.name: b.matrix_local.copy() for b in ch.rig.data.bones}
    W = rest["weapon_r"]
    c = ch.colours
    w = {"weapon_r": 1.0}
    T = Matrix.Translation

    def at(x, y, z, rx=0.0):
        return W @ T((x, y, z)) @ Matrix.Rotation(math.radians(rx), 4, "X")

    g.box(at(0, 0.03, 0.045), (0.05, 0.24, 0.065), c["metal"], weights=w, bevel=0.004)      # receiver
    g.box(at(0, 0.27, 0.045), (0.054, 0.24, 0.058), c["gear"], weights=w, bevel=0.006)     # handguard
    g.cylinder(at(0, 0.46, 0.05, -90), 0.011, 0.18, c["metal"], weights=w, segments=8)       # barrel
    g.cylinder(at(0, 0.565, 0.05, -90), 0.016, 0.05, c["dark"], weights=w, segments=8)       # muzzle
    g.box(at(0, -0.19, 0.03), (0.044, 0.22, 0.075), c["gear"], weights=w, bevel=0.006, taper=0.85)  # stock
    g.box(at(0, -0.3, 0.02), (0.048, 0.02, 0.1), c["dark"], weights=w)                     # butt pad
    g.box(at(0, -0.01, -0.035, 18), (0.03, 0.04, 0.095), c["dark"], weights=w, bevel=0.004)  # grip
    g.box(at(0, 0.045, 0.1), (0.032, 0.11, 0.035), c["dark"], weights=w, bevel=0.004)      # optic
    g.box(at(0, 0.21, 0.08), (0.012, 0.15, 0.008), c["metal"], weights=w)                  # rail
    M = rest["weapon_mag"]
    g.box(M @ T((0, 0.0, -0.065)) @ Matrix.Rotation(math.radians(12), 4, "X"), (0.026, 0.06, 0.13),
          c["dark"], weights={"weapon_mag": 1.0}, bevel=0.003)
    return g.to_object("weapon", ch.materials)


def build(ch, gear_mod, surfaces):
    """Add weapon bones and the prop, then record every clip."""
    add_weapon_bones(ch)
    ch.prop = build_prop(ch, gear_mod, surfaces)
    a = Animator(ch)
    scene = bpy.context.scene
    scene.render.fps = FPS
    walk_cycle, run_cycle, crouch_cycle, crawl_cycle = 0.66, 0.62, 0.9, 1.6
    a.record("idle", 3.0, a.idle)
    a.record("walk", walk_cycle, lambda t: a.walk(t, walk_cycle), speed=3.4)
    a.record("run", run_cycle, lambda t: a.run(t, run_cycle), speed=5.6)
    a.record("crouch_idle", 3.0, a.crouch_idle)
    a.record("crouch_walk", crouch_cycle, lambda t: a.crouch_walk(t, crouch_cycle), speed=1.9)
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
        "arms": sorted(n for n in a.sk.order if n.startswith(
            ("clavicle", "upperarm", "lowerarm", "hand", "thumb", "weapon") + FINGERS)),
    })
    return a
