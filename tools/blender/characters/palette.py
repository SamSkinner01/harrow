"""Team palettes. Colours are sRGB hex (as you'd pick them in an image
editor) and are converted to linear for vertex colours.

Attackers wear warm olive and coyote; defenders wear cool slate and
charcoal with near-black gear, so the two teams read apart against the
sand-and-ochre fortress at any distance. Operators can override single
roles in their config's [palette] table.
"""

TEAM_PALETTES = {
    "attackers": {
        "shirt": "#6c6a4c",      # olive drab combat shirt
        "trousers": "#86714f",   # coyote trousers
        "gear": "#545a42",       # ranger green carrier and pouches
        "gear_alt": "#8d7856",   # coyote pouches / panels
        "helmet": "#8f7d5c",     # tan helmet shell
        "straps": "#3d3f31",     # webbing, belts
        "gloves": "#7a6a52",
        "boots": "#9a8565",      # tan suede
        "sole": "#2f2a24",
        "metal": "#2b2c2d",      # rails, buckles, weapon
        "dark": "#1e1f1c",       # cups, rubber, frames
        "lens": "#2e3b45",       # smoked lenses and visors
        "pads": "#4f523e",
        "accent": "#b08a3c",     # team marker (ochre)
    },
    "defenders": {
        "shirt": "#4a5562",      # slate blue-grey shirt
        "trousers": "#3a3e44",   # charcoal trousers
        "gear": "#2a2d33",       # near-black carrier
        "gear_alt": "#3d4650",   # steel-blue panels
        "helmet": "#3c434b",
        "straps": "#1f2125",
        "gloves": "#1d1e20",
        "boots": "#222325",
        "sole": "#141414",
        "metal": "#2b2c2e",
        "dark": "#121314",
        "lens": "#27323b",
        "pads": "#2c3036",
        "accent": "#5d7f9c",     # team marker (steel blue)
    },
}


def srgb_to_linear(c):
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def hex_to_linear(value):
    value = value.lstrip("#")
    r, g, b = (int(value[i : i + 2], 16) / 255.0 for i in (0, 2, 4))
    return (srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b), 1.0)


def palette_for(cfg):
    """Linear RGBA colour per role for an operator."""
    roles = dict(TEAM_PALETTES[cfg["team"]])
    for role, value in cfg["palette"].items():
        if role not in roles:
            raise ValueError(f"{cfg['path']}: unknown palette role {role!r}")
        roles[role] = value
    return {role: hex_to_linear(value) for role, value in roles.items()}
