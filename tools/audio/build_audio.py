"""Build HARROW's shipped sound effects (assets/audio/**) from the staged
open-licence sound packs.

Run with Blender's bundled Python (it has the `aud` audio library and numpy;
no ffmpeg/sox needed):

    blender --background --factory-startup --python tools/audio/build_audio.py -- \
        --staging /path/to/harrow-assets-staging/audio --out assets/audio

What it does, per output file: decode the source(s), resample to 48 kHz,
cut one-shots at fixed times (gunshot takes, multi-take foley) or take whole
files, optionally layer two or three sources aligned on their onsets,
high-pass away rumble/DC, trim and fade the tail, match the variants of a
bank in loudness, normalise the bank to a peak level, and encode Ogg Vorbis.
Every choice (files, slice times, layer gains) is fixed in the tables below,
so the output is reproducible. Slice times were picked by transient/spectral
analysis, not by ear: listen and adjust them here if something sounds off.

Licences: only CC0 and CC-BY packs are read. The `sonniss-*` folders are
never touched (their licence forbids redistributing the files). Every pack
used here is listed in ASSETS.md ("Audio"); CC-BY credits are in CREDITS.md.

The staging folder is treated as untrusted data: only audio files are read
from it, nothing there is executed.

At the end a table of every output file is printed (decoded back from the
.ogg): duration, peak dBFS, RMS dBFS, channels, sample rate, and the onset
time (to check that Vorbis priming didn't eat the transient).
"""

import argparse
import glob
import os
import sys

import aud
import numpy as np

RATE = 48000

# Packs this script may read. Anything else (in particular sonniss-*) is
# refused, so a typo can't pull in a file we may not redistribute.
ALLOWED_PACKS = {
    "oga-free-firearm-sound-library": "CC0",
    "oga-zer0sol-handgun-reload": "CC0",
    "oga-lfa-equipment-clicks-2": "CC0",
    "oga-lfa-equipment-clicks-3": "CC0",
    "oga-baradari-weapon-clicks": "CC-BY 3.0",
    "kenney-rpg-audio": "CC0",
    "kenney-impact-sounds": "CC0",
    "oga-fantozzi-footsteps": "CC0",
    "oga-congusbongus-footsteps-surfaces": "CC-BY 3.0",
    "oga-qubodup-impact": "CC0",
    "oga-rubberduck-75-breaking": "CC0",
    "oga-rubberduck-100-metal-wood": "CC0",
    "oga-rubberduck-25-bangs": "CC0",
    "oga-indepnu-wood-crack": "CC0",
    "oga-indepnu-break-crunch": "CC0",
    "oga-baradari-explosions": "CC-BY 3.0",
    "oga-varkalandar-big-boom": "CC-BY 4.0",
    "oga-qubodup-bangs": "CC-BY 3.0",
}

FIREARMS = "oga-free-firearm-sound-library/extracted/Prepared SFX Library"
RUBBER75 = "oga-rubberduck-75-breaking/extracted"
RUBBER100 = "oga-rubberduck-100-metal-wood/extracted"
KENNEY_IMPACT = "kenney-impact-sounds/extracted/Audio"
KENNEY_RPG = "kenney-rpg-audio/extracted/Audio"
CONGUS = "oga-congusbongus-footsteps-surfaces/extracted/footsteps"
FANTOZZI = "oga-fantozzi-footsteps/extracted/Fantozzi-footsteps/flac"
INDEPNU_WOOD = "oga-indepnu-wood-crack/extracted/wood_impact"
INDEPNU_CRUNCH = "oga-indepnu-break-crunch/extracted/impcrunch"
LFA2 = "oga-lfa-equipment-clicks-2/equipmentclicks.wav"
LFA3 = "oga-lfa-equipment-clicks-3/equipment_clicks3.wav"


# --------------------------------------------------------------------------
# Basic signal helpers. Audio is float64 numpy arrays shaped (frames, channels)
# at RATE.
# --------------------------------------------------------------------------


def db(x):
    return 20.0 * np.log10(np.maximum(x, 1e-12))


def gain(g_db):
    return 10.0 ** (g_db / 20.0)


class Sources:
    """Decodes staged files (once each), resampled to RATE."""

    def __init__(self, staging):
        self.staging = os.path.abspath(staging)
        self.cache = {}

    def get(self, rel):
        pack = rel.split("/", 1)[0]
        if pack not in ALLOWED_PACKS:
            sys.exit(f"refusing to read {rel}: pack {pack!r} is not in ALLOWED_PACKS")
        if rel not in self.cache:
            path = os.path.join(self.staging, rel)
            if not os.path.isfile(path):
                sys.exit(f"missing source file: {path}")
            sound = aud.Sound(path)
            rate = int(sound.specs[0])
            if rate != RATE:
                sound = sound.resample(RATE, 3)
            data = np.array(sound.data(), dtype=np.float64)
            if data.ndim == 1:
                data = data.reshape(-1, 1)
            # Remove any DC offset.
            data -= data.mean(axis=0, keepdims=True)
            self.cache[rel] = data
        return self.cache[rel]


def mono(x, channel=None):
    """Mono (n, 1). `channel` picks one channel of a spaced stereo pair
    (summing uncorrelated mics comb-filters transients); None averages."""
    if x.shape[1] == 1:
        return x.copy()
    if channel is None:
        return x.mean(axis=1, keepdims=True)
    return x[:, channel:channel + 1].copy()


def louder_channel(x):
    if x.shape[1] == 1:
        return 0
    return int(np.argmax((x ** 2).sum(axis=0)))


def seconds(x):
    return len(x) / RATE


def at(t):
    return int(round(t * RATE))


def aud_filter(x, kind, freq, q=0.707):
    """Second-order filter from aud, channel by channel."""
    out = np.empty_like(x)
    for c in range(x.shape[1]):
        sound = aud.Sound.buffer(np.ascontiguousarray(x[:, c:c + 1], dtype=np.float32), RATE)
        sound = sound.highpass(freq, q) if kind == "hp" else sound.lowpass(freq, q)
        y = np.array(sound.data(), dtype=np.float64).reshape(-1)
        n = min(len(y), len(x))
        out[:, c] = 0.0
        out[:n, c] = y[:n]
    return out


def highpass(x, freq, order=2):
    for _ in range(order // 2):
        x = aud_filter(x, "hp", freq)
    return x


def lowpass(x, freq, order=2):
    for _ in range(order // 2):
        x = aud_filter(x, "lp", freq)
    return x


def onset(x, thresh_db=-24.0, start=0.0, end=None):
    """First sample (index) above `thresh_db` relative to the loudest peak in
    [start, end) seconds."""
    m = np.abs(x).max(axis=1)
    a = at(start)
    b = len(m) if end is None else min(len(m), at(end))
    region = m[a:b]
    peak = region.max()
    above = np.nonzero(region > peak * gain(thresh_db))[0]
    return a + int(above[0])


def cut(x, start, end):
    a, b = max(0, at(start)), min(len(x), at(end))
    return x[a:b].copy()


def fade(x, fade_in=0.002, fade_out=0.03):
    """Raised-cosine fades at both ends."""
    x = x.copy()
    n = len(x)
    fi = min(n // 2, at(fade_in))
    fo = min(n // 2, at(fade_out))
    if fi > 0:
        x[:fi] *= (0.5 - 0.5 * np.cos(np.linspace(0, np.pi, fi)))[:, None]
    if fo > 0:
        x[n - fo:] *= (0.5 + 0.5 * np.cos(np.linspace(0, np.pi, fo)))[:, None]
    return x


def rise_onset(x, start, end):
    """Index where the steepest 3 ms rise in level starts, within [start, end)
    seconds. Robust when the previous shot's tail is still ringing."""
    m = np.abs(x).max(axis=1)
    hop = at(0.0005)
    a, b = at(start), min(len(m), at(end))
    n = (b - a) // hop
    env = db(m[a:a + n * hop].reshape(n, hop).max(axis=1))
    rise = env[6:] - env[:-6]
    return a + int(np.argmax(rise)) * hop


def one_shot(x, approx, length, pre=0.008, fade_out=None, search=0.03):
    """Cut a one-shot out of a long take: find the transient near `approx`
    seconds, start `pre` seconds before it (lead-in for Vorbis priming), keep
    `length` seconds and fade out the last part of the natural tail."""
    i = rise_onset(x, max(0.0, approx - search), approx + search)
    start = i / RATE - pre
    seg = cut(x, start, start + length)
    return fade(seg, 0.003, fade_out if fade_out is not None else length * 0.45)


def whole(x, pre=0.006, thresh_db=-30.0, max_len=None, fade_out=0.03, floor_db=-55.0):
    """A whole file as a one-shot: lead-in trimmed to `pre` before the onset,
    tail trimmed where it falls `floor_db` below the peak, then faded."""
    i = onset(x, thresh_db)
    lead = at(pre)
    if i < lead:
        # The file starts right on the transient: pad silence in front so the
        # Vorbis priming can't smear it.
        x = np.concatenate([np.zeros((lead - i, x.shape[1])), x])
        i = lead
    seg = x[i - lead:]
    seg = trim_tail(seg, floor_db)
    if max_len is not None:
        seg = seg[:at(max_len)]
    return fade(seg, 0.002, fade_out)


def trim_tail(x, floor_db=-55.0):
    """Drop the end once a 10 ms RMS envelope stays `floor_db` below its peak."""
    m = mono(x)[:, 0]
    hop = at(0.01)
    n = len(m) // hop
    if n < 2:
        return x
    env = np.sqrt((m[: n * hop].reshape(n, hop) ** 2).mean(axis=1))
    peak = env.max()
    loud = np.nonzero(env > peak * gain(floor_db))[0]
    end = min(len(x), (loud[-1] + 2) * hop)
    return x[:end]


def mix(layers, channels=1):
    """Layers: (signal, offset_seconds, gain_db). Each layer is aligned on its
    onset, then shifted by its offset."""
    placed = []
    for sig, offset, g in layers:
        sig = sig if sig.shape[1] == channels else (mono(sig) if channels == 1 else np.repeat(mono(sig), channels, axis=1))
        o = onset(sig, -20.0)
        placed.append((sig, at(offset) - o, gain(g)))
    first = min(p[1] for p in placed)
    lead = at(0.006)
    length = max(len(s) + (start - first) for s, start, _ in placed) + lead
    out = np.zeros((length, channels))
    for sig, start, g in placed:
        a = start - first + lead
        if a < 0:
            sig = sig[-a:]
            a = 0
        out[a:a + len(sig)] += sig * g
    return out


def splice(parts, crossfade=0.004):
    """Join segments with short crossfades."""
    out = parts[0]
    n = at(crossfade)
    for p in parts[1:]:
        ramp = np.linspace(0, 1, n)[:, None]
        joined = out[-n:] * (1 - ramp) + p[:n] * ramp
        out = np.concatenate([out[:-n], joined, p[n:]])
    return out


def balance_stereo(x):
    """Equal RMS in both channels (keeps the first-person shot centred)."""
    if x.shape[1] != 2:
        return x
    rms = np.sqrt((x ** 2).mean(axis=0)) + 1e-12
    target = rms.mean()
    return x * (target / rms)[None, :]


def a_weighted(x):
    """A-weighting applied in the frequency domain (for loudness matching)."""
    m = mono(x)[:, 0]
    spec = np.fft.rfft(m)
    f = np.fft.rfftfreq(len(m), 1 / RATE)
    f2 = np.maximum(f, 1e-3) ** 2
    ra = (12194 ** 2 * f2 ** 2) / (
        (f2 + 20.6 ** 2) * np.sqrt((f2 + 107.7 ** 2) * (f2 + 737.9 ** 2)) * (f2 + 12194 ** 2)
    )
    return np.fft.irfft(spec * ra * gain(2.0), len(m))


def loudness(x):
    """Short-term (50 ms) maximum of the A-weighted RMS, dB."""
    w = a_weighted(x)
    hop = at(0.05)
    n = max(1, len(w) // hop)
    frames = w[: n * hop].reshape(n, hop) if len(w) >= hop else w.reshape(1, -1)
    return db(np.sqrt((frames ** 2).mean(axis=1)).max())


def normalise_bank(variants, peak_db, loudness_db=None):
    """Match the variants' loudness (to the bank median). Then either scale
    the bank so its loudest peak sits at `peak_db` dBFS, or, with a
    `loudness_db` target, bring its median loudness there with `peak_db` as
    a ceiling (so related banks, e.g. footsteps on different surfaces, sound
    equally loud)."""
    levels = [loudness(v) for v in variants]
    target = float(np.median(levels))
    variants = [v * gain(target - l) for v, l in zip(variants, levels)]
    scale = gain(peak_db) / max(np.abs(v).max() for v in variants)
    if loudness_db is not None:
        scale = min(scale, gain(loudness_db - target))
    return [v * scale for v in variants]


def soft_limit(x, drive_db):
    """tanh saturation: raises the body of a sound relative to its peaks."""
    peak = np.abs(x).max()
    return np.tanh(x / peak * gain(drive_db))


# --------------------------------------------------------------------------
# The banks. Each returns a list of arrays (the variants).
# --------------------------------------------------------------------------


def firearm(src, name):
    return src.get(f"{FIREARMS}/{name}")


def rifle_shot(src, stereo):
    """TAMARISK (AK-pattern 7.62x39): AK-47 near take, four single shots."""
    take = firearm(src, "AK-47/C_28P.wav")
    out = []
    for k, t in enumerate((0.610, 3.255, 6.020, 9.155)):
        shot = one_shot(take, t, 1.15, fade_out=0.55)
        if stereo:
            out.append(highpass(balance_stereo(shot), 35.0))
        else:
            # One mic of the spaced pair (alternating) for the mono version.
            out.append(highpass(mono(shot, k % 2), 35.0))
    return out


def rifle_far(src):
    """Mid-distance AK-47 takes, low-passed: the distant layer for other
    shooters. C_31P single shots, and the second shot of C_34P pairs."""
    single = firearm(src, "AK-47/C_31P.wav")
    pairs = firearm(src, "AK-47/C_34P.wav")
    out = []
    for take, t in ((single, 0.335), (single, 4.400), (pairs, 0.820), (pairs, 9.195)):
        shot = one_shot(take, t, 1.9, fade_out=0.9)
        out.append(lowpass(highpass(mono(shot, louder_channel(shot)), 40.0), 5000.0))
    return out


def pistol_shot(src, stereo):
    """ACACIA (9 mm): Walther PPQ near take (three shots) plus one 1911 shot."""
    ppq = firearm(src, "Walther PPQ/X_39P.wav")
    m1911 = firearm(src, "1911/A_42P.wav")
    out = []
    for k, (take, t) in enumerate(((ppq, 1.405), (ppq, 6.440), (ppq, 10.660), (m1911, 5.000))):
        shot = one_shot(take, t, 0.95, fade_out=0.5)
        if stereo:
            out.append(highpass(balance_stereo(shot), 40.0))
        else:
            out.append(highpass(mono(shot, k % 2), 40.0))
    return out


def pistol_far(src):
    take = firearm(src, "Walther PPQ/X_31P.wav")
    out = []
    for t in (1.085, 5.240):
        shot = one_shot(take, t, 1.5, fade_out=0.7)
        out.append(lowpass(highpass(mono(shot, louder_channel(shot)), 50.0), 5000.0))
    return out


def dry_fire(src):
    x = src.get("oga-baradari-weapon-clicks/extracted/clicks/outofammo.wav")
    return [highpass(whole(mono(x), fade_out=0.02), 80.0)]


def pistol_reload(src, part):
    """zer0_sol's handgun reload, sliced: mag out | mag in | slide rack."""
    x = mono(src.get("oga-zer0sol-handgun-reload/reload.wav"))
    start, end = {"mag_out": (0.090, 0.45), "mag_in": (0.555, 0.98), "slide": (1.005, 1.59)}[part]
    seg = cut(x, start, end)
    return [highpass(fade(trim_tail(seg, -50.0), 0.003, 0.05), 80.0)]


def rifle_mag_out(src):
    """LFA equipment clicks III: release click with a scrape (lever press and
    the magazine rocking out)."""
    x = src.get(LFA3)
    return [highpass(fade(cut(x, 2.172, 2.50), 0.003, 0.08), 70.0)]


def rifle_mag_in(src):
    """LFA equipment clicks II: low thump then a heavy clack (rock in, latch)."""
    x = src.get(LFA2)
    return [highpass(fade(cut(x, 2.268, 2.72), 0.003, 0.1), 50.0)]


def rifle_charge(src):
    """Charging handle: pull back and slam forward. Variant 1 is one
    continuous LFA III action; variant 2 splices LFA III's slow pull (scrape)
    and its release with the pause between them removed."""
    x = src.get(LFA3)
    a = fade(cut(x, 14.622, 15.02), 0.003, 0.1)
    pull = cut(x, 2.172, 2.335)
    release = cut(x, 2.745, 3.12)
    b = fade(splice([pull, np.zeros((at(0.05), 1)), release]), 0.003, 0.1)
    return [highpass(a, 60.0), highpass(b, 60.0)]


def draw(src):
    """Weapon draw: cloth/gear rustle with a light metal touch."""
    cloth1 = src.get(f"{KENNEY_RPG}/cloth1.ogg")
    cloth2 = src.get(f"{KENNEY_RPG}/cloth2.ogg")
    click = src.get(f"{KENNEY_RPG}/metalClick.ogg")
    latch = src.get(f"{KENNEY_RPG}/metalLatch.ogg")
    a = mix([(whole(cloth1), 0.0, 0.0), (whole(click), 0.16, -4.0)])
    b = mix([(whole(cloth2), 0.0, 0.0), (whole(latch), 0.12, -12.0)])
    return [highpass(fade(a, 0.002, 0.05), 80.0), highpass(fade(b, 0.002, 0.05), 80.0)]


def holster(src):
    cloth3 = src.get(f"{KENNEY_RPG}/cloth3.ogg")
    cloth4 = src.get(f"{KENNEY_RPG}/cloth4.ogg")
    leather = src.get(f"{KENNEY_RPG}/handleSmallLeather.ogg")
    leather2 = src.get(f"{KENNEY_RPG}/handleSmallLeather2.ogg")
    a = mix([(whole(cloth3), 0.0, 0.0), (whole(leather), 0.08, -2.0)])
    b = mix([(whole(cloth4), 0.0, 0.0), (whole(leather2), 0.06, -2.0)])
    return [highpass(fade(a, 0.002, 0.05), 80.0), highpass(fade(b, 0.002, 0.05), 80.0)]


# ---- Footsteps -------------------------------------------------------------


def steps_stone(src):
    """Fantozzi's stone steps (real, gritty heel/sole detail) with the body
    thump of congusbongus' boots on concrete underneath."""
    out = []
    names = ["StoneL1", "StoneR1", "StoneL2", "StoneR2", "StoneL3", "StoneR3"]
    for k, n in enumerate(names):
        detail = whole(src.get(f"{FANTOZZI}/Fantozzi-{n}.flac"), floor_db=-50.0)
        body = highpass(whole(src.get(f"{CONGUS}/boots/{k}.ogg")), 70.0, order=4)
        out.append(highpass(fade(mix([(detail, 0.0, 0.0), (body, 0.0, -9.0)]), 0.002, 0.04), 50.0))
    return out


def steps_wood(src):
    """Hollow wooden-floor thumps (congusbongus wood) with a soft sole contact
    (Kenney RPG footsteps) on top."""
    out = []
    for k in range(8):
        # The thump is mostly below 150 Hz (much of it infrasonic): cut
        # under 70 Hz so the boom doesn't eat all the headroom.
        body = highpass(whole(src.get(f"{CONGUS}/wood/{k}.ogg")), 70.0, order=4)
        contact = highpass(whole(src.get(f"{KENNEY_RPG}/footstep0{k}.ogg")), 60.0)
        # A little saturation tames the thump's peak so the step can be as
        # loud as the others under the same peak ceiling.
        out.append(fade(soft_limit(mix([(body, 0.0, 0.0), (contact, 0.0, 0.0)]), 4.0), 0.002, 0.04))
    return out


def steps_sand(src):
    names = ["SandL1", "SandR1", "SandL2", "SandR2", "SandL3", "SandR3"]
    return [
        highpass(whole(src.get(f"{FANTOZZI}/Fantozzi-{n}.flac"), floor_db=-50.0, fade_out=0.05), 60.0)
        for n in names
    ]


def steps_metal(src):
    """Boots on an aluminium ladder (congusbongus metal): hatch frames,
    plates."""
    return [highpass(whole(src.get(f"{CONGUS}/metal/{k}.ogg")), 50.0) for k in range(8)]


# ---- Bullet impacts --------------------------------------------------------


def impacts_stone(src):
    """Sharp crack (small hard hits) plus grit (rocks falling), trimmed."""
    hits = ["bfh1_rock_hit_01", "bfh1_hit_05", "bfh1_hit_09", "bfh1_hit_11", "bfh1_hit_04"]
    grit = ["bfh1_rock_falling_04", "bfh1_rock_falling_05", "bfh1_rock_falling_06",
            "bfh1_rock_falling_08", "bfh1_rock_falling_07"]
    stone = whole(src.get("oga-qubodup-impact/extracted/qubodupImpact/qubodupImpactStone.flac"))
    out = []
    for k, (h, g) in enumerate(zip(hits, grit)):
        layers = [
            (whole(src.get(f"{RUBBER75}/{h}.ogg")), 0.0, 0.0),
            (whole(src.get(f"{RUBBER75}/{g}.ogg")), 0.012, -7.0),
        ]
        if k % 2 == 0:
            layers.append((stone, 0.0, -9.0))
        out.append(highpass(fade(mix(layers)[: at(0.45)], 0.002, 0.12), 120.0))
    return out


def impacts_plaster(src):
    """Dull broad hit plus crumble."""
    pairs = [
        ("bfh1_hit_08", "bfh1_rock_falling_02"),
        ("bfh1_hit_06", "bfh1_breaking_02"),
        ("bfh1_hit_01", "bfh1_breaking_03"),
        ("bfh1_hit_13", "bfh1_rock_falling_03"),
    ]
    out = []
    for h, c in pairs:
        layers = [
            (whole(src.get(f"{RUBBER75}/{h}.ogg")), 0.0, 0.0),
            (whole(src.get(f"{RUBBER75}/{c}.ogg")), 0.015, -6.0),
        ]
        out.append(highpass(fade(mix(layers)[: at(0.5)], 0.002, 0.15), 90.0))
    return out


def impacts_wood(src):
    """Wooden thunk plus a splinter crack."""
    pairs = [
        ("wood_hit_07", "wood_cracking_01"),
        ("wood_hit_05", "wood_cracking_02"),
        ("wood_hit_03", "wood_cracking_03"),
        ("wood_hit_08", "wood_cracking_04"),
        ("wood_hit_06", "wood_cracking_02"),
    ]
    out = []
    for h, c in pairs:
        layers = [
            (whole(src.get(f"{RUBBER100}/{h}.ogg")), 0.0, 0.0),
            (whole(src.get(f"{RUBBER100}/{c}.ogg")), 0.004, -6.0),
        ]
        out.append(highpass(fade(mix(layers)[: at(0.4)], 0.002, 0.1), 80.0))
    return out


def impacts_metal(src):
    files = [
        f"{RUBBER100}/metal_hit_01.ogg",
        f"{RUBBER100}/metal_hit_04.ogg",
        f"{RUBBER75}/bfh1_metal_hit_06.ogg",
        f"{RUBBER75}/bfh1_metal_hit_02.ogg",
        "oga-qubodup-impact/extracted/qubodupImpact/qubodupImpactMetal.flac",
    ]
    return [highpass(whole(src.get(f), max_len=0.45, fade_out=0.15), 120.0) for f in files]


def impacts_body(src):
    """Wet meat impact with a punch thump underneath."""
    meat = [whole(src.get(f"oga-qubodup-impact/extracted/qubodupImpact/qubodupImpactMeat0{k}.flac"))
            for k in (1, 2)]
    punches = [whole(src.get(f"{KENNEY_IMPACT}/impactPunch_medium_00{k}.ogg")) for k in range(4)]
    out = []
    for k in range(4):
        layers = [(meat[k % 2], 0.0, 0.0), (punches[k], 0.0, -4.0 if k < 2 else -1.0)]
        out.append(highpass(fade(mix(layers)[: at(0.35)], 0.002, 0.12), 70.0))
    return out


def impacts_sand(src):
    """Bullet into sand: soft thud plus a short spray (sand footstep grit)."""
    names = ["SandL2", "SandR2", "SandL3", "SandR3"]
    out = []
    for k, n in enumerate(names):
        spray = whole(src.get(f"{FANTOZZI}/Fantozzi-{n}.flac"), max_len=0.22, fade_out=0.1)
        thud = highpass(whole(src.get(f"{KENNEY_IMPACT}/impactSoft_heavy_00{k}.ogg")), 60.0)
        out.append(fade(mix([(spray, 0.0, 0.0), (thud, 0.0, -3.0)])[: at(0.3)], 0.002, 0.1))
    return out


# ---- Destruction -----------------------------------------------------------


def break_wood(src):
    """Planks splitting: Independent.nu cracks, plus a rubberduck break."""
    out = []
    for name, length in (("crack05", 1.3), ("crack01", 1.3), ("crack04", 1.2)):
        x = whole(src.get(f"{INDEPNU_WOOD}/{name}.mp3.flac"), max_len=length, fade_out=0.4)
        out.append(highpass(x, 60.0))
    layered = mix([
        (whole(src.get(f"{RUBBER100}/wood_breaking_01.ogg")), 0.0, 0.0),
        (whole(src.get(f"{RUBBER100}/wood_falling_01.ogg")), 0.12, -5.0),
    ])
    out.append(highpass(fade(layered, 0.002, 0.15), 60.0))
    return out


def break_plaster(src):
    """Plaster and lath crunching, with crumbs falling after."""
    pairs = [("impactcrunch01", "bfh1_falling_04"), ("impactcrunch03", "bfh1_rock_falling_01"),
             ("impactcrunch04", "bfh1_falling_01"), ("impactcrunch05", "bfh1_rock_falling_03")]
    out = []
    for c, f in pairs:
        layers = [
            (whole(src.get(f"{INDEPNU_CRUNCH}/{c}.mp3.flac")), 0.0, 0.0),
            (whole(src.get(f"{RUBBER75}/{f}.ogg")), 0.15, -6.0),
        ]
        out.append(highpass(fade(mix(layers)[: at(1.5)], 0.002, 0.3), 60.0))
    return out


def break_metal(src):
    files = ["metal_falling_01", "metal_sheet_03", "metal_sheet_06"]
    return [highpass(whole(src.get(f"{RUBBER100}/{f}.ogg"), max_len=1.3, fade_out=0.3), 80.0) for f in files]


# ---- Explosions ------------------------------------------------------------


def explosion_near(src):
    """Breach charge / grenade: a real bang (rubberduck's cannon and firework
    recordings) for the crack, Baradari's explosion for the body, Varkalandar's
    sub boom underneath, rocks falling after."""
    boom = highpass(src.get("oga-varkalandar-big-boom/hjm-big_explosion_3.wav"), 28.0, order=4)
    boom = fade(boom[: at(3.0)], 0.002, 1.5)
    body = {
        "big": highpass(src.get("oga-baradari-explosions/extracted/explosions/explode.wav"), 30.0, order=4),
        "mini": highpass(src.get("oga-baradari-explosions/extracted/explosions/explodemini.wav"), 30.0, order=4),
    }
    recipes = [
        ("cannon_01", "big", "bfh1_falling_04"),
        ("bang_06", "big", "bfh1_rock_falling_01"),
        ("cannon_04", "mini", "bfh1_falling_01"),
    ]
    out = []
    for crack, b, debris in recipes:
        layers = [
            (whole(src.get(f"oga-rubberduck-25-bangs/extracted/{crack}.ogg")), 0.0, 0.0),
            (whole(body[b], floor_db=-60.0), 0.0, -2.0),
            (boom, 0.0, -6.0),
            (whole(src.get(f"{RUBBER75}/{debris}.ogg")), 0.35, -14.0),
        ]
        # Gentle saturation: denser and louder for the same peak level.
        out.append(fade(soft_limit(mix(layers)[: at(3.0)], 6.0), 0.002, 1.0))
    return out


def explosion_far(src):
    """Distant layer: qubodup's background bangs, low-passed."""
    out = []
    for n in ("BangLong", "BangMid", "BangShort"):
        x = src.get(f"oga-qubodup-bangs/extracted/qubodup-Bangs/flac/qubodup-{n}.ogg")
        out.append(fade(lowpass(highpass(whole(mono(x), floor_db=-60.0), 30.0), 1800.0), 0.002, 0.6))
    return out


# Output banks: (relative path prefix, builder, channels, peak dBFS,
# loudness target or None). Files are written as <out>/<prefix>_<n>.ogg,
# n = 1, 2, ... With a loudness target (dB, A-weighted 50 ms maximum, see
# `loudness`) the peak is a ceiling; otherwise the bank peaks there.
STEP_LOUDNESS = -20.0
IMPACT_LOUDNESS = -19.0
BANKS = [
    ("weapons/rifle_shot", lambda s: rifle_shot(s, True), 2, -1.0, None),
    ("weapons/rifle_shot_mono", lambda s: rifle_shot(s, False), 1, -1.0, None),
    ("weapons/rifle_far", rifle_far, 1, -1.0, None),
    ("weapons/pistol_shot", lambda s: pistol_shot(s, True), 2, -1.0, None),
    ("weapons/pistol_shot_mono", lambda s: pistol_shot(s, False), 1, -1.0, None),
    ("weapons/pistol_far", pistol_far, 1, -1.0, None),
    ("weapons/dry_fire", dry_fire, 1, -3.0, None),
    ("weapons/rifle_mag_out", rifle_mag_out, 1, -3.0, None),
    ("weapons/rifle_mag_in", rifle_mag_in, 1, -3.0, None),
    ("weapons/rifle_charge", rifle_charge, 1, -3.0, None),
    ("weapons/pistol_mag_out", lambda s: pistol_reload(s, "mag_out"), 1, -3.0, None),
    ("weapons/pistol_mag_in", lambda s: pistol_reload(s, "mag_in"), 1, -3.0, None),
    ("weapons/pistol_slide", lambda s: pistol_reload(s, "slide"), 1, -3.0, None),
    ("weapons/draw", draw, 1, -3.0, None),
    ("weapons/holster", holster, 1, -3.0, None),
    ("footsteps/stone", steps_stone, 1, -1.0, STEP_LOUDNESS),
    ("footsteps/wood", steps_wood, 1, -1.0, STEP_LOUDNESS),
    ("footsteps/sand", steps_sand, 1, -1.0, STEP_LOUDNESS - 1.5),
    ("footsteps/metal", steps_metal, 1, -1.0, STEP_LOUDNESS),
    ("impacts/stone", impacts_stone, 1, -1.0, IMPACT_LOUDNESS),
    ("impacts/plaster", impacts_plaster, 1, -1.0, IMPACT_LOUDNESS),
    ("impacts/wood", impacts_wood, 1, -1.0, IMPACT_LOUDNESS),
    ("impacts/metal", impacts_metal, 1, -1.0, IMPACT_LOUDNESS),
    ("impacts/body", impacts_body, 1, -1.0, IMPACT_LOUDNESS),
    ("impacts/sand", impacts_sand, 1, -1.0, IMPACT_LOUDNESS - 1.5),
    ("destruction/wood", break_wood, 1, -2.0, None),
    ("destruction/plaster", break_plaster, 1, -2.0, None),
    ("destruction/metal", break_metal, 1, -2.0, None),
    ("explosions/near", explosion_near, 1, -1.0, None),
    ("explosions/far", explosion_far, 1, -1.0, None),
]

MANAGED_DIRS = sorted({prefix.split("/")[0] for prefix, *_ in BANKS})


def write_ogg(x, path):
    channels = x.shape[1]
    sound = aud.Sound.buffer(np.ascontiguousarray(x, dtype=np.float32), RATE)
    sound.write(
        path,
        rate=RATE,
        channels=aud.CHANNELS_STEREO if channels == 2 else aud.CHANNELS_MONO,
        format=aud.FORMAT_FLOAT32,
        container=aud.CONTAINER_OGG,
        codec=aud.CODEC_VORBIS,
        bitrate=160000 if channels == 2 else 112000,
    )


def describe(path):
    sound = aud.Sound(path)
    rate, channels = sound.specs
    data = np.array(sound.data(), dtype=np.float64)
    m = np.abs(data).max(axis=1)
    peak = m.max()
    first = np.nonzero(m > peak * gain(-20.0))[0]
    return {
        "duration": len(data) / rate,
        "peak": db(peak),
        "rms": db(np.sqrt((data ** 2).mean())),
        "channels": int(channels),
        "rate": int(rate),
        "onset_ms": 1000.0 * first[0] / rate if len(first) else float("nan"),
        "bytes": os.path.getsize(path),
    }


def main():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    parser = argparse.ArgumentParser()
    parser.add_argument("--staging", required=True, help="the harrow-assets-staging/audio folder")
    parser.add_argument("--out", default="assets/audio")
    args = parser.parse_args(argv)

    src = Sources(args.staging)
    out_dir = os.path.abspath(args.out)
    # Start clean so renamed or dropped variants don't linger.
    for d in MANAGED_DIRS:
        for old in glob.glob(os.path.join(out_dir, d, "*.ogg")):
            os.remove(old)

    rows = []
    for prefix, build, channels, peak_db, loudness_db in BANKS:
        variants = [mono(v) if channels == 1 else v for v in build(src)]
        for v in variants:
            if v.shape[1] != channels:
                sys.exit(f"{prefix}: expected {channels} channel(s), got {v.shape[1]}")
        variants = normalise_bank(variants, peak_db, loudness_db)
        os.makedirs(os.path.dirname(os.path.join(out_dir, prefix)), exist_ok=True)
        for n, v in enumerate(variants, start=1):
            path = os.path.join(out_dir, f"{prefix}_{n}.ogg")
            write_ogg(v, path)
            rows.append((os.path.relpath(path, out_dir), describe(path)))
        print(f"built {prefix}: {len(variants)} variant(s)")

    print("\n| File | Duration (s) | Peak (dBFS) | RMS (dBFS) | Ch | Rate | Onset (ms) |")
    print("|---|---|---|---|---|---|---|")
    total = 0
    for rel, d in rows:
        total += d["bytes"]
        print(f"| {rel} | {d['duration']:.3f} | {d['peak']:.1f} | {d['rms']:.1f} | {d['channels']} | {d['rate']} | {d['onset_ms']:.1f} |")
    print(f"\n{len(rows)} files, {total / 1e6:.2f} MB")
    print("\nBank sizes (keep src/audio/mod.rs in step):")
    counts = {}
    for rel, _ in rows:
        bank = rel.rsplit("_", 1)[0]
        counts[bank] = counts.get(bank, 0) + 1
    for bank, n in counts.items():
        print(f"  {bank}: {n}")


main()
