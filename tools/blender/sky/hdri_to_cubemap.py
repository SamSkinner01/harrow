"""Equirectangular HDRI (.hdr / .exr) -> KTX2 cubemap for Bevy's `Skybox`.

Run inside Blender (it decodes the HDR; numpy does the rest):

    blender -b --factory-startup --python tools/blender/sky/hdri_to_cubemap.py -- \
        IN.hdr OUT.ktx2 [--size 1024] [--sun-clamp 200] [--preview OUT.png]

Wrapper: tools/scripts/build_sky.sh.

What it does:
- Finds the sun (the brightest pixel of the upper half) and rotates the sky
  about the vertical so the sun sits due south (world +Z, Bevy: x east,
  y up, z south). At runtime the skybox is turned by the sun's azimuth
  (`lighting.sun_azimuth`), so sky and sun light always agree.
- Normalises brightness so the sky's mean luminance (sun excluded,
  weighted by solid angle) is 1.0; `lighting.sky_brightness` is then
  roughly the sky's luminance in cd/m2.
- Clamps the sun disc to `--sun-clamp` times the sky mean (the real sun is
  ~10^5 times brighter; our DirectionalLight does the lighting, the disc
  only has to bloom).
- Writes six faces (+X -X +Y -Y +Z -Z, WebGPU cube convention; Bevy samples
  cubes with z negated) as E5B9G9R9 (shared-exponent HDR, 4 bytes per
  texel), one mip level, zstd-supercompressed (the `zstd` CLI).
- Prints the horizon, zenith and ground colours (same normalised units) to
  copy into the `lighting` tuning: fog colour = horizon.
"""

import math
import struct
import subprocess
import sys

import numpy as np

try:
    import bpy
except ImportError:  # pragma: no cover
    sys.exit("run this inside Blender: blender -b --python hdri_to_cubemap.py -- IN OUT")


def parse_args():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    if len(argv) < 2:
        sys.exit(__doc__)
    opts = {"size": 1024, "sun_clamp": 200.0, "preview": None}
    rest = argv[2:]
    i = 0
    while i < len(rest):
        key = rest[i].lstrip("-").replace("-", "_")
        opts[key] = rest[i + 1]
        i += 2
    opts["size"] = int(opts["size"])
    opts["sun_clamp"] = float(opts["sun_clamp"])
    return argv[0], argv[1], opts


def load_equirect(path):
    """Linear float RGB, row 0 = top (zenith)."""
    img = bpy.data.images.load(path)
    img.colorspace_settings.name = "Linear Rec.709"
    w, h = img.size
    px = np.empty(w * h * 4, dtype=np.float32)
    img.pixels.foreach_get(px)
    rgb = px.reshape(h, w, 4)[::-1, :, :3].astype(np.float64)
    return np.ascontiguousarray(rgb)


def luminance(rgb):
    return rgb[..., 0] * 0.2126 + rgb[..., 1] * 0.7152 + rgb[..., 2] * 0.0722


# Equirect convention (not mirrored, as seen from inside): longitude 0 looks
# north (-Z), increasing towards east (+X); u = 0.5 + lon / 2pi; row v = 0 is
# the zenith.
def dir_to_lonlat(x, y, z):
    lon = np.arctan2(x, -z)
    lat = np.arcsin(np.clip(y, -1.0, 1.0))
    return lon, lat


def sample_bilinear(rgb, u, v):
    h, w, _ = rgb.shape
    fx = u * w - 0.5
    fy = np.clip(v * h - 0.5, 0.0, h - 1.0)
    x0 = np.floor(fx).astype(np.int64)
    y0 = np.floor(fy).astype(np.int64)
    tx = (fx - x0)[..., None]
    ty = (fy - y0)[..., None]
    x1 = (x0 + 1) % w
    x0 = x0 % w
    y1 = np.minimum(y0 + 1, h - 1)
    top = rgb[y0, x0] * (1 - tx) + rgb[y0, x1] * tx
    bottom = rgb[y1, x0] * (1 - tx) + rgb[y1, x1] * tx
    return top * (1 - ty) + bottom * ty


def find_sun(rgb):
    h, w, _ = rgb.shape
    lum = luminance(rgb[: h // 2])
    row, col = np.unravel_index(np.argmax(lum), lum.shape)
    lon = ((col + 0.5) / w - 0.5) * 2 * math.pi
    lat = (0.5 - (row + 0.5) / h) * math.pi
    return lon, lat, float(lum[row, col])


def band_mean(rgb, lat_lo, lat_hi, exclude=None):
    """Solid-angle weighted mean colour between two latitudes (degrees)."""
    h, w, _ = rgb.shape
    lat = (0.5 - (np.arange(h) + 0.5) / h) * math.pi
    rows = (lat >= math.radians(lat_lo)) & (lat <= math.radians(lat_hi))
    weight = np.cos(lat[rows])[:, None] * np.ones((1, w))
    block = rgb[rows]
    if exclude is not None:
        weight = weight * exclude[rows]
    total = weight.sum()
    return (block * weight[..., None]).sum(axis=(0, 1)) / max(total, 1e-9)


def face_directions(face, n):
    """World directions (Bevy) for every texel of cube face `face`."""
    c = (np.arange(n) + 0.5) / n * 2.0 - 1.0
    sc, tc = np.meshgrid(c, c)  # sc along u (columns), tc along v (rows)
    one = np.ones_like(sc)
    # WebGPU / Vulkan cube convention: direction in cube space.
    r = {
        0: (one, -tc, -sc),  # +X
        1: (-one, -tc, sc),  # -X
        2: (sc, one, tc),  # +Y
        3: (sc, -one, -tc),  # -Y
        4: (sc, -tc, one),  # +Z
        5: (-sc, -tc, -one),  # -Z
    }[face]
    x, y, z = r
    # Bevy samples cubes with z negated: world = (x, y, -z).
    z = -z
    length = np.sqrt(x * x + y * y + z * z)
    return x / length, y / length, z / length


def encode_rgb9e5(rgb):
    n, b, emax = 9, 15, 31
    max_value = (2**n - 1) / 2**n * 2 ** (emax - b)
    c = np.clip(rgb, 0.0, max_value)
    maxc = c.max(axis=-1)
    with np.errstate(divide="ignore"):
        log = np.floor(np.log2(np.maximum(maxc, 1e-30)))
    exp_p = np.maximum(-b - 1, log) + 1 + b
    max_s = np.floor(maxc / np.exp2(exp_p - b - n) + 0.5)
    exp = np.where(max_s >= 2**n, exp_p + 1, exp_p)
    denom = np.exp2(exp - b - n)[..., None]
    mant = np.floor(c / denom + 0.5).astype(np.uint32)
    mant = np.minimum(mant, 511)
    packed = mant[..., 0] | (mant[..., 1] << 9) | (mant[..., 2] << 18) | (exp.astype(np.uint32) << 27)
    return packed.astype("<u4")


def rgb9e5_dfd():
    samples = b""
    for channel, offset in ((0, 0), (1, 9), (2, 18)):
        samples += struct.pack("<HBB4BII", offset, 8, channel, 0, 0, 0, 0, 0, 8448)
        samples += struct.pack("<HBB4BII", 27, 4, channel | (0x2 << 4), 0, 0, 0, 0, 15, 31)
    block_size = 8 + 16 + len(samples)
    header = struct.pack("<IHH", 0, 2, block_size)
    basic = struct.pack("<4B4B8B", 1, 1, 1, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0)
    block = header + basic + samples
    return struct.pack("<I", 4 + len(block)) + block


def write_ktx2(path, faces, n):
    raw = b"".join(f.tobytes() for f in faces)
    compressed = subprocess.run(
        ["zstd", "-19", "-q", "-c", "-"], input=raw, stdout=subprocess.PIPE, check=True
    ).stdout
    identifier = bytes([0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A])
    header_len = 12 + 9 * 4 + 4 * 4 + 2 * 8
    level_index_len = 3 * 8
    dfd = rgb9e5_dfd()
    dfd_offset = header_len + level_index_len
    data_offset = dfd_offset + len(dfd)
    data_offset += (-data_offset) % 8
    out = bytearray(identifier)
    # vkFormat 123 = E5B9G9R9_UFLOAT_PACK32, typeSize 4, w, h, depth 0,
    # layers 0, faces 6, levels 1, supercompression 2 = zstd.
    out += struct.pack("<9I", 123, 4, n, n, 0, 0, 6, 1, 2)
    out += struct.pack("<4I2Q", dfd_offset, len(dfd), 0, 0, 0, 0)
    out += struct.pack("<3Q", data_offset, len(compressed), len(raw))
    out += dfd
    out += b"\0" * (data_offset - len(out))
    out += compressed
    with open(path, "wb") as f:
        f.write(out)
    return len(out)


def tonemap_preview(rgb, scale):
    x = rgb * scale
    x = x / (1.0 + x)
    return (np.clip(x, 0, 1) ** (1 / 2.2) * 255).astype(np.uint8)


def main():
    src, dst, opts = parse_args()
    n = opts["size"]
    rgb = load_equirect(src)
    h, w, _ = rgb.shape
    sun_lon, sun_lat, sun_peak = find_sun(rgb)
    print(f"source {w}x{h}; sun at azimuth {math.degrees(sun_lon):.1f} (from north), "
          f"elevation {math.degrees(sun_lat):.1f} deg, peak {sun_peak:.0f}")

    # Sky mean, excluding a 6 degree cap around the sun.
    lat = (0.5 - (np.arange(h) + 0.5) / h) * math.pi
    lon = ((np.arange(w) + 0.5) / w - 0.5) * 2 * math.pi
    lon_g, lat_g = np.meshgrid(lon, lat)
    sun_dir = np.array([math.cos(sun_lat) * math.sin(sun_lon), math.sin(sun_lat), -math.cos(sun_lat) * math.cos(sun_lon)])
    dirs = np.stack([np.cos(lat_g) * np.sin(lon_g), np.sin(lat_g), -np.cos(lat_g) * np.cos(lon_g)], axis=-1)
    away = (dirs @ sun_dir) < math.cos(math.radians(6.0))
    sky_mean = luminance(band_mean(rgb, 5.0, 90.0, exclude=away))
    rgb = rgb / sky_mean
    clamp = opts["sun_clamp"]
    lum = luminance(rgb)
    over = lum > clamp
    rgb[over] *= (clamp / lum[over])[:, None]

    horizon = band_mean(rgb, 0.0, 4.0, exclude=away)
    low_sky = band_mean(rgb, 4.0, 20.0, exclude=away)
    zenith = band_mean(rgb, 60.0, 90.0, exclude=away)
    ground = band_mean(rgb, -40.0, -5.0)
    fmt = lambda c: f"({c[0]:.3f}, {c[1]:.3f}, {c[2]:.3f})"
    print(f"normalised by sky mean luminance {sky_mean:.3f}")
    print(f"horizon colour  {fmt(horizon)}  (lighting.fog_colour)")
    print(f"low sky colour  {fmt(low_sky)}")
    print(f"zenith colour   {fmt(zenith)}")
    print(f"ground colour   {fmt(ground)}")

    faces = []
    preview_faces = []
    for face in range(6):
        x, y, z = face_directions(face, n)
        lon_o, lat_o = dir_to_lonlat(x, y, z)
        # Output longitude pi (due south) shows the source sun.
        lon_s = lon_o - math.pi + sun_lon
        u = (0.5 + lon_s / (2 * math.pi)) % 1.0
        v = 0.5 - lat_o / math.pi
        texels = sample_bilinear(rgb, u, v)
        faces.append(encode_rgb9e5(texels))
        preview_faces.append(texels)
    size = write_ktx2(dst, faces, n)
    print(f"wrote {dst}: 6 x {n}x{n} rgb9e5, {size / 1e6:.1f} MB")

    if opts["preview"]:
        # A strip of the six faces (+X -X +Y -Y +Z -Z), quarter size,
        # simply tonemapped.
        strip = np.concatenate([tonemap_preview(f[::4, ::4], 0.6) for f in preview_faces], axis=1)
        ph, pw, _ = strip.shape
        image = bpy.data.images.new("preview", pw, ph, alpha=True)
        rgba = np.concatenate([strip.astype(np.float32) / 255.0, np.ones((ph, pw, 1), np.float32)], axis=-1)
        image.pixels.foreach_set(rgba[::-1].ravel())
        image.filepath_raw = opts["preview"]
        image.file_format = "PNG"
        image.save()
        print(f"preview {opts['preview']}")


main()
