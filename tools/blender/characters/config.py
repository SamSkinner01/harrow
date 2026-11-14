"""Operator configs: `tools/blender/characters/operators/<id>.toml`.

Each file describes one operator: body (MPFB macro sliders), face (MPFB
targets), skin, gear selection and optional palette overrides. Unknown keys
are an error, so a typo doesn't silently fall back to a default. See
`operators/auger.toml` for a commented example.
"""

import os
import tomllib

OPERATOR_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "operators")

TEAMS = ("attackers", "defenders")

BODY_DEFAULTS = {
    "gender": 1.0,
    "age": 0.5,
    "muscle": 0.5,
    "weight": 0.5,
    "proportions": 0.5,
    "height": 0.5,
    "cupsize": 0.5,
    "firmness": 0.5,
    "height_m": 1.77,
    "race": {"african": 0.33, "asian": 0.33, "caucasian": 0.33},
}

FACE_DEFAULTS = {
    "eyebrows": "eyebrow001",
    "eyes": "brown",
    "targets": {},
}

SKIN_DEFAULTS = {
    "texture": "young_caucasian_male",
    "tint": [1.0, 1.0, 1.0],
    "hair": "buzz",
    "hair_colour": "#2a2420",
    "stubble": 0.0,
}

GEAR_DEFAULTS = {
    "helmet": "none",
    "headset": False,
    "eyewear": "none",
    "carrier": "none",
    "pouches": [],
    "gloves": "none",
    "boots": "high",
    "pads": [],
    "neck": "none",
    "sleeves": "long",
}


class ConfigError(Exception):
    pass


def _merge(section, defaults, data, path):
    out = dict(defaults)
    for key, value in data.items():
        if key not in defaults:
            raise ConfigError(f"{path}: unknown key [{section}] {key!r}")
        out[key] = value
    return out


def load(path):
    with open(path, "rb") as f:
        data = tomllib.load(f)
    known = {"id", "name", "team", "role", "armour", "speed", "body", "face", "skin", "gear", "palette"}
    for key in data:
        if key not in known:
            raise ConfigError(f"{path}: unknown top-level key {key!r}")
    for key in ("id", "name", "team"):
        if key not in data:
            raise ConfigError(f"{path}: missing {key!r}")
    if data["team"] not in TEAMS:
        raise ConfigError(f"{path}: team must be one of {TEAMS}")
    cfg = {
        "id": data["id"],
        "name": data["name"],
        "team": data["team"],
        "role": data.get("role", ""),
        "armour": int(data.get("armour", 2)),
        "speed": int(data.get("speed", 2)),
        "body": _merge("body", BODY_DEFAULTS, data.get("body", {}), path),
        "face": _merge("face", FACE_DEFAULTS, data.get("face", {}), path),
        "skin": _merge("skin", SKIN_DEFAULTS, data.get("skin", {}), path),
        "gear": _merge("gear", GEAR_DEFAULTS, data.get("gear", {}), path),
        "palette": dict(data.get("palette", {})),
        "path": path,
    }
    expected = os.path.splitext(os.path.basename(path))[0]
    if cfg["id"] != expected:
        raise ConfigError(f"{path}: id {cfg['id']!r} must match the file name {expected!r}")
    return cfg


def load_all(only=None):
    names = sorted(f for f in os.listdir(OPERATOR_DIR) if f.endswith(".toml"))
    configs = [load(os.path.join(OPERATOR_DIR, n)) for n in names]
    if only:
        missing = set(only) - {c["id"] for c in configs}
        if missing:
            raise ConfigError(f"no operator config for {sorted(missing)}")
        configs = [c for c in configs if c["id"] in only]
    return configs
