"""Motion capture retargeting onto MPFB's `game_engine` rig.

Sources (read at build time from the staging folder, never copied into the
repository; see ASSETS.md "Motion capture"):

- CMU Graphics Lab Motion Capture Database, BVH conversion by Bruce Hahne:
  31-joint skeleton, Y up, about 1/16 m units, 120 fps. Frame 1 of every
  file is a T-pose (used as the source rest pose); the clips are raw
  captures (not looped, the actor walks across the room).

The BVH files are parsed here as plain text (no importer, no scripts).

Method (per clip):

1. Forward kinematics of the source on every frame, converted to Z up.
2. Cycle extraction: the segment [a, b] that best repeats (pose and
   velocity match) over a whole number of gait cycles near the length we
   ask for.
3. Heading: the T-pose's facing and the segment's direction of travel are
   both turned to +Y (the rig faces +Y in armature space).
4. In place: the average velocity over the segment is removed from every
   position, so the hips stay over the origin and the stance feet move
   backwards at the walking speed. The vertical bob and the side-to-side
   sway are kept. Everything is scaled by target leg length / source leg
   length (per operator).
5. Rotations: world-space deltas from the source rest. For a source joint
   J, D(t) = Rworld_J(t) @ Rworld_J(rest)^-1 is the rotation that takes the
   T-pose to frame t in world space; the target bone gets D(t) @ its own
   rest rotation. Both rests are upright with straight legs, so this needs
   no per-bone axis tables (MPFB's bone rolls and the CMU joint axes don't
   matter). Used for the pelvis, spine, neck, head, feet and toes.
6. Legs: two-bone IK (anim.Skeleton) to the scaled source ankle positions,
   knee towards the source thigh's forward direction. The captured feet
   are planted in the world, so in place their contact points move back
   at the travel speed; `measure_speed` reads that speed off the feet.
7. Loop: whatever differs between the last frame and the first is spread
   linearly over the cycle (positions lerped, rotations slerped); the loop
   then starts at the left foot's touchdown (the same phase in every clip).
8. Overlays (`normalise`, called by anim.py): the clip's average pelvis,
   chest (spine_02, spine_03) and head rotations are turned towards the
   standing pose's (per clip; the per-frame motion stays), so the lean
   overlay (spine_01/02) and the aim/reload overlays (spine_03 up), which
   are posed standing, still line up. `lower` deepens a crouch (hips down,
   feet unchanged).

The arms are not taken from the mocap: anim.py puts the rifle on the chest
and both hands on it.
"""

import math
import os

from mathutils import Matrix, Quaternion, Vector

# Y-up BVH -> Z-up: (x, y, z) -> (x, -z, y).
C = Matrix(((1, 0, 0), (0, 0, -1), (0, 1, 0)))

# Target bone <- CMU joint (rotation deltas).
CMU_ROT = {
    "pelvis": "Hips",
    "spine_01": "LowerBack",
    "spine_02": "Spine",
    "spine_03": "Spine1",
    "neck_01": "Neck1",
    "head": "Head",
    "thigh_l": "LeftUpLeg",
    "thigh_r": "RightUpLeg",
    "foot_l": "LeftFoot",
    "foot_r": "RightFoot",
    "ball_l": "LeftToeBase",
    "ball_r": "RightToeBase",
}
CMU_LEG = {"l": ("LeftUpLeg", "LeftLeg", "LeftFoot", "LeftToeBase"),
           "r": ("RightUpLeg", "RightLeg", "RightFoot", "RightToeBase")}


def _rot(axis, deg):
    return Matrix.Rotation(math.radians(deg), 3, axis)


class Bvh:
    """A parsed BVH file: joints, offsets, channels and frames."""

    def __init__(self, path):
        with open(path, encoding="ascii") as f:
            tokens = f.read().split()
        self.name = os.path.splitext(os.path.basename(path))[0]
        self.joints = []          # names, parents before children
        self.parent = {}
        self.offset = {}
        self.channels = {}
        self.end = {}             # joint -> end site offset
        i = 0
        stack = []
        current = None
        while tokens[i] != "MOTION":
            tok = tokens[i]
            if tok in ("ROOT", "JOINT"):
                current = tokens[i + 1]
                self.joints.append(current)
                self.parent[current] = stack[-1] if stack else None
                i += 2
            elif tok == "End":
                current = ("end", stack[-1])
                i += 2
            elif tok == "{":
                stack.append(current)
                i += 1
            elif tok == "}":
                stack.pop()
                i += 1
            elif tok == "OFFSET":
                v = Vector([float(x) for x in tokens[i + 1:i + 4]])
                if isinstance(current, tuple):
                    self.end[current[1]] = v
                else:
                    self.offset[current] = v
                i += 4
            elif tok == "CHANNELS":
                n = int(tokens[i + 1])
                self.channels[current] = tokens[i + 2:i + 2 + n]
                i += 2 + n
            else:
                i += 1
        n_frames = int(tokens[i + 2])
        self.frame_time = float(tokens[i + 5])
        values = tokens[i + 6:]
        width = sum(len(c) for c in self.channels.values())
        assert len(values) >= n_frames * width, path
        self.frames = [[float(x) for x in values[f * width:(f + 1) * width]] for f in range(n_frames)]
        self.fps = 1.0 / self.frame_time
        self._cache = {}

    def world(self, f):
        """{joint: (rotation 3x3, position)} at frame f, Z up, source units."""
        if f in self._cache:
            return self._cache[f]
        vals = self.frames[f]
        out = {}
        k = 0
        for j in self.joints:
            pos = Vector(self.offset[j])
            rot = Matrix.Identity(3)
            for ch in self.channels[j]:
                v = vals[k]
                k += 1
                if ch == "Xposition":
                    pos.x += v
                elif ch == "Yposition":
                    pos.y += v
                elif ch == "Zposition":
                    pos.z += v
                else:
                    rot = rot @ _rot(ch[0], v)
            p = self.parent[j]
            if p is None:
                out[j] = (rot, pos)
            else:
                prot, ppos = out[p]
                out[j] = (prot @ rot, ppos + prot @ pos)
        conv = {j: (C @ r, C @ p) for j, (r, p) in out.items()}
        self._cache[f] = conv
        return conv

    def sample(self, t):
        """World transforms at fractional frame t (lerp / slerp)."""
        f0 = int(math.floor(t))
        f0 = max(0, min(len(self.frames) - 2, f0))
        k = t - f0
        a, b = self.world(f0), self.world(f0 + 1)
        if k <= 1e-9:
            return a
        out = {}
        for j in self.joints:
            qa, qb = a[j][0].to_quaternion(), b[j][0].to_quaternion()
            if qa.dot(qb) < 0:
                qb.negate()
            out[j] = (qa.slerp(qb, k).to_matrix(), a[j][1].lerp(b[j][1], k))
        return out

    def leg_length(self):
        o = self.offset
        return (o["LeftLeg"].length + o["LeftFoot"].length + o["RightLeg"].length + o["RightFoot"].length) / 2


_BVH = {}
_CYCLES = {}


def load(path):
    """Parsed BVH, cached for the whole build (every operator uses it)."""
    if path not in _BVH:
        _BVH[path] = Bvh(path)
    return _BVH[path]


def cycle(bvh, window, length):
    """find_cycle, cached."""
    key = (bvh.name, tuple(window), tuple(length))
    if key not in _CYCLES:
        _CYCLES[key] = find_cycle(bvh, window[0], window[1], length[0], length[1])
    return _CYCLES[key]


# ------------------------------------------------------------- analysis


def yaw_to_y(direction):
    """Rotation about Z that turns `direction` (horizontal part) to +Y."""
    return Matrix.Rotation(math.atan2(direction.x, direction.y), 3, "Z")


def facing(world):
    """Horizontal facing of the hips: (left hip - right hip) x up."""
    left = world["LeftUpLeg"][1] - world["RightUpLeg"][1]
    f = left.cross(Vector((0, 0, 1)))
    f.z = 0.0
    return f.normalized()


FEATURE_JOINTS = ("LeftFoot", "RightFoot", "LeftLeg", "RightLeg", "LeftToeBase", "RightToeBase",
                  "Spine1", "Head")


def _features(bvh, f):
    """Pose of frame f relative to the hips, heading removed."""
    w = bvh.world(f)
    h = w["Hips"][1]
    turn = yaw_to_y(facing(w))
    out = [Vector((0.0, 0.0, h.z))]
    out += [turn @ (w[j][1] - h) for j in FEATURE_JOINTS]
    return out


def find_cycle(bvh, lo, hi, min_len, max_len):
    """Frames (a, b), lo <= a < b <= hi (seconds), b - a in [min_len,
    max_len] seconds, where the pose and its velocity at b best match a."""
    fps = bvh.fps
    lo_f, hi_f = max(1, round(lo * fps)), min(len(bvh.frames) - 3, round(hi * fps))
    step = 2
    feats = {f: _features(bvh, f) for f in range(lo_f - step, hi_f + step + 1)}

    def vel(f):
        return [(x - y) for x, y in zip(feats[f + step], feats[f - step])]

    vels = {f: vel(f) for f in range(lo_f, hi_f + 1)}
    best = None
    for a in range(lo_f, hi_f + 1):
        for b in range(a + round(min_len * fps), min(hi_f, a + round(max_len * fps)) + 1):
            d = sum((x - y).length_squared for x, y in zip(feats[a], feats[b]))
            d += 4.0 * sum((x - y).length_squared for x, y in zip(vels[a], vels[b]))
            if best is None or d < best[0]:
                best = (d, a, b)
    return best[1], best[2]


# ------------------------------------------------------------ retarget


class Frame:
    """One retargeted frame in the target's armature space."""

    def __init__(self):
        self.rot = {}       # bone -> world rotation (3x3)
        self.mid = Vector()     # midpoint of the hip joints
        self.pelvis = Vector()  # pelvis head offset from its rest place
        self.ankle = {}
        self.pole = {}


class Motion:
    """A looped, in-place clip retargeted onto one skeleton (anim.Skeleton).

    `frames[k]` for k in 0..n-1 (frame n would equal frame 0); `length` in
    seconds; `speed` is the backward speed of the stance feet, m/s."""

    def __init__(self, bvh, sk, a, b, fps, time_scale=1.0, travel=True, label=""):
        self.label = label or bvh.name
        self.source = (bvh.name, a / bvh.fps, b / bvh.fps)
        rest = bvh.world(0)
        h_rest = yaw_to_y(facing(rest))
        s = sk.leg / bvh.leg_length()
        self.scale = s
        n = max(2, round((b - a) / bvh.fps / time_scale * fps))
        self.n = n
        self.length = n / fps
        pa, pb = bvh.world(a)["Hips"][1], bvh.world(b)["Hips"][1]
        if travel:
            heading = yaw_to_y(pb - pa)
        else:
            mean = Vector()
            for f in range(a, b + 1, 4):
                mean += facing(bvh.world(f))
            heading = yaw_to_y(mean)
        drift = pb - pa
        rest_inv = {j: r.inverted() for j, (r, _) in rest.items()}
        self.sk = sk
        raw = []
        for k in range(n + 1):
            t = a + (b - a) * k / n
            w = bvh.sample(t)
            fr = Frame()
            off = pa + drift * (k / n)

            def P(j):
                return heading @ (w[j][1] - off) * s

            for bone, j in CMU_ROT.items():
                d = heading @ w[j][0] @ rest_inv[j] @ h_rest.inverted()
                fr.rot[bone] = d @ sk.rest_rot[bone]
            fr.mid = (P("LeftUpLeg") + P("RightUpLeg")) / 2
            for side, (up, _knee, ankle, _toe) in CMU_LEG.items():
                fr.ankle[side] = P(ankle)
                d_up = heading @ w[up][0] @ rest_inv[up] @ h_rest.inverted()
                fr.pole[side] = d_up @ Vector((0.0, 1.0, 0.0))
            raw.append(fr)
        # Loop: spread the last frame's mismatch with the first over the cycle.
        first, last = raw[0], raw[n]
        for k, fr in enumerate(raw):
            u = k / n
            fr.mid = fr.mid + (first.mid - last.mid) * u
            for side in "lr":
                fr.ankle[side] = fr.ankle[side] + (first.ankle[side] - last.ankle[side]) * u
                fr.pole[side] = fr.pole[side] + (first.pole[side] - last.pole[side]) * u
            for bone in fr.rot:
                q0 = first.rot[bone].to_quaternion()
                q1 = last.rot[bone].to_quaternion()
                fix = q0 @ q1.inverted()
                if fix.w < 0:
                    fix.negate()
                fr.rot[bone] = (Quaternion().slerp(fix, u) @ fr.rot[bone].to_quaternion()).to_matrix()
        frames = raw[:n]
        # Ground: the feet's lowest stretch puts the ankles at rest height.
        lows = []
        for side in "lr":
            zs = sorted(fr.ankle[side].z for fr in frames)
            lows.append(sum(zs[: max(1, n // 5)]) / max(1, n // 5))
        dz = sk.ankle_z - sum(lows) / 2
        # Centre the hips over the origin (horizontally).
        rest_mid = (sk.head["thigh_l"] + sk.head["thigh_r"]) / 2
        cx = sum(fr.mid.x for fr in frames) / n
        cy = sum(fr.mid.y for fr in frames) / n
        shift = Vector((rest_mid.x - cx, rest_mid.y - cy, dz))
        for fr in frames:
            fr.mid = fr.mid + shift
            for side in "lr":
                fr.ankle[side] = fr.ankle[side] + shift
        self.frames = frames
        self.place_pelvis()
        if travel:
            self.start_at_left_contact()
        self.speed = self.measure_speed(fps) if travel else 0.0
        self.travel_speed = (drift.x ** 2 + drift.y ** 2) ** 0.5 * s / self.length

    def place_pelvis(self):
        """Pelvis head from the hip joints' midpoint and the pelvis rotation."""
        sk = self.sk
        rest_mid = (sk.head["thigh_l"] + sk.head["thigh_r"]) / 2
        to_mid = rest_mid - sk.head["pelvis"]
        inv = sk.rest_rot["pelvis"].inverted()
        for fr in self.frames:
            delta = fr.rot["pelvis"] @ inv
            fr.pelvis = fr.mid - delta @ to_mid - sk.head["pelvis"]

    def start_at_left_contact(self):
        """Rotate the loop so frame 0 is the left foot touching down (the
        same phase in every locomotion clip, so blends start in step)."""
        down = [min(self.foot_points(k, "l")[1]) <= 0.015 for k in range(self.n)]
        for k in range(self.n):
            if down[k] and not down[k - 1]:
                self.frames = self.frames[k:] + self.frames[:k]
                return

    def lower(self, height):
        """Move the hips (on average) to `height` above the ground, feet
        unchanged: the IK legs bend more (a deeper crouch)."""
        mean = sum(fr.mid.z for fr in self.frames) / self.n
        for fr in self.frames:
            fr.mid.z += height - mean
        self.place_pelvis()

    @staticmethod
    def _mean(rots):
        qs = [r.to_quaternion() for r in rots]
        acc = [0.0, 0.0, 0.0, 0.0]
        for q in qs:
            if q.dot(qs[0]) < 0:
                q = -q
            for i in range(4):
                acc[i] += q[i]
        return Quaternion(acc).normalized()

    def normalise(self, ref, pelvis=1.0, chest=1.0, head=1.0):
        """Turn the clip's average pelvis, chest and head towards the world
        rotations in `ref` (the standing pose), by these fractions; the
        per-frame motion is kept. The chest fix (measured on spine_02) is
        shared by spine_01 (half) and spine_02, then spine_03 gets its own;
        the neck takes half of the chest and head fixes."""
        ident = Quaternion()
        steps = (
            (pelvis, "pelvis", (("pelvis", 1.0),)),
            (chest, "spine_02", (("spine_01", 0.5), ("spine_02", 1.0), ("spine_03", 1.0), ("neck_01", 0.5))),
            (chest, "spine_03", (("spine_03", 1.0), ("neck_01", 0.5))),
            (head, "head", (("neck_01", 0.5), ("head", 1.0))),
        )
        for weight, key, bones in steps:
            if weight <= 0.0:
                continue
            mean = self._mean([fr.rot[key] for fr in self.frames])
            fix = ref[key].to_quaternion() @ mean.inverted()
            if fix.w < 0:
                fix.negate()
            fix = ident.slerp(fix, weight)
            for fr in self.frames:
                for bone, w in bones:
                    fr.rot[bone] = (ident.slerp(fix, w) @ fr.rot[bone].to_quaternion()).to_matrix()
        self.place_pelvis()

    def tilt(self, bone):
        """Average forward pitch of a bone's world rotation relative to its
        rest, degrees (positive = leaning forward), for the build log."""
        mean = self._mean([fr.rot[bone] for fr in self.frames]).to_matrix()
        up = (mean @ self.sk.rest_rot[bone].inverted()) @ Vector((0, 0, 1))
        return math.degrees(math.atan2(up.y, up.z))

    def foot_points(self, k, side):
        """Heel (ankle) and ball positions on frame k, and their heights
        above their rest heights."""
        sk = self.sk
        fr = self.frames[k % self.n]
        ankle = fr.ankle[side]
        turn = fr.rot[f"foot_{side}"] @ sk.rest_rot[f"foot_{side}"].inverted()
        ball = ankle + turn @ (sk.head[f"ball_{side}"] - sk.head[f"foot_{side}"])
        return (ankle, ball), (ankle.z - sk.head[f"foot_{side}"].z, ball.z - sk.head[f"ball_{side}"].z)

    def measure_speed(self, fps):
        """Backward speed of the planted feet, m/s: on every frame where a
        foot's heel or ball is within 1.5 cm of the ground, the backward
        velocity of that (lowest) point; the median of those."""
        speeds = []
        for side in "lr":
            for k in range(self.n):
                pts, hs = self.foot_points(k, side)
                i = 0 if hs[0] <= hs[1] else 1
                if hs[i] > 0.015:
                    continue
                before = self.foot_points(k - 1, side)[0][i]
                after = self.foot_points(k + 1, side)[0][i]
                speeds.append(-(after.y - before.y) * fps / 2)
        speeds.sort()
        return speeds[len(speeds) // 2] if speeds else 0.0

    def frame(self, t, fps):
        return self.frames[round(t * fps) % self.n]
