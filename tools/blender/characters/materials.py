"""Character materials and textures.

Materials per operator, matched by name in the game (`src/characters/setup.rs`):

| name      | colour                         | detail maps (added in the game)      |
|-----------|--------------------------------|--------------------------------------|
| skin      | CC0 skin texture x vertex col. | shared skin normal + roughness       |
| uniform   | vertex colour (shirt, trousers)| poplin weave normal + ARM            |
| webbing   | vertex colour (carriers, pouches, straps, belts) | twill weave normal + ARM |
| leather   | vertex colour (boots, gloves)  | leather grain normal + ARM           |
| hard      | vertex colour (helmet shells, plates' hardware, lenses, carbine) | none, smoother |
| eyes      | CC0 iris texture, 128 px       |                                      |
| brows     | eyebrow + eyelash atlas, alpha cut-out |                              |

Colour textures that differ per operator (skin, eyes, brows) are embedded
in the .glb. Detail maps are the same for everyone, so the game loads them
once from `assets/textures/characters/` instead (like the fortress kit's
material library): the fabric maps come from Poly Haven
(`tools/scripts/fetch_character_textures.py`), the skin maps are made here
from the CC0 Mindfront skin's normal and specular maps
(`write_skin_detail`). Previews in Blender add the detail maps after export.
"""

import os

import bpy
import numpy as np

from . import mpfb

SKIN_SIZE = 1024
SMALL_SIZE = 128
DETAIL_SKIN = "mindfront_aksel_skin/mindfront_aksel_skin.mhmat"

# Surface -> (roughness, real-world texture width in metres) for previews.
FABRICS = {"uniform": 0.291, "webbing": 0.273, "leather": 0.5}


def _mhmat(subdir, fragment):
    path = mpfb.services().AssetService.find_asset_absolute_path(fragment, subdir)
    if path is None:
        raise FileNotFoundError(f"MakeHuman asset {subdir}/{fragment} is not installed "
                                "(run build_characters.sh --setup)")
    out = {}
    with open(path, encoding="utf-8") as f:
        for line in f:
            parts = line.split()
            if len(parts) == 2 and parts[0].endswith("Texture"):
                out[parts[0]] = os.path.join(os.path.dirname(path), parts[1])
    return out


def _load(path):
    return bpy.data.images.load(path, check_existing=False)


def _save(image, size, out_path, file_format, quality=90):
    if tuple(image.size) != (size, size):
        image.scale(size, size)
    image.filepath_raw = out_path
    image.file_format = file_format
    image.save(filepath=out_path, quality=quality)
    image.name = os.path.splitext(os.path.basename(out_path))[0]
    return image


def _pixels(image):
    return np.array(image.pixels[:], dtype=np.float32).reshape(image.size[1], image.size[0], 4)


def _brow_atlas(brow_src, lash_src, out_path):
    """Eyebrow texture on the left half, eyelashes on the right (one alpha
    cut-out material for both)."""
    half = SMALL_SIZE
    parts = []
    for src in (brow_src, lash_src):
        img = _load(src)
        img.scale(half, half)
        parts.append(_pixels(img))
        bpy.data.images.remove(img)
    atlas = np.concatenate(parts, axis=1)  # rows, cols: brows | lashes
    img = bpy.data.images.new("brows", width=half * 2, height=half, alpha=True)
    img.pixels[:] = atlas.ravel()
    img.filepath_raw = out_path
    img.file_format = "PNG"
    img.save(filepath=out_path)
    return img


def write_skin_detail(out_dir):
    """Shared skin detail maps (MakeHuman UV layout): the Mindfront skin's
    normal map, and an ARM map whose roughness comes from its specular map."""
    os.makedirs(out_dir, exist_ok=True)
    maps = _mhmat("skins", DETAIL_SKIN)
    normal = _load(maps["normalmapTexture"])
    normal.colorspace_settings.name = "Non-Color"
    _save(normal, SKIN_SIZE, os.path.join(out_dir, "normal.jpg"), "JPEG", quality=92)
    spec = _load(maps["specularTexture"])
    spec.colorspace_settings.name = "Non-Color"
    spec.scale(SKIN_SIZE, SKIN_SIZE)
    s = _pixels(spec)[:, :, 0]
    arm = np.ones((SKIN_SIZE, SKIN_SIZE, 4), dtype=np.float32)
    arm[:, :, 1] = np.clip(0.72 - 0.5 * s, 0.35, 0.8)   # roughness
    arm[:, :, 2] = 0.0                                   # metal
    img = bpy.data.images.new("skin_arm", width=SKIN_SIZE, height=SKIN_SIZE)
    img.colorspace_settings.name = "Non-Color"
    img.pixels[:] = arm.ravel()
    img.filepath_raw = os.path.join(out_dir, "arm.jpg")
    img.file_format = "JPEG"
    img.save(filepath=img.filepath_raw, quality=92)
    for image in (normal, spec, img):
        bpy.data.images.remove(image)


def _principled(name, roughness):
    mat = bpy.data.materials.new(name)
    nodes = mat.node_tree.nodes
    bsdf = nodes.get("Principled BSDF")
    bsdf.inputs["Roughness"].default_value = roughness
    return mat, nodes, mat.node_tree.links, bsdf


def _vertex_colour(nodes, links, target):
    col = nodes.new("ShaderNodeVertexColor")
    col.layer_name = "Color"
    links.new(col.outputs["Color"], target)
    return col


def build(cfg, scratch):
    """Create the operator's materials; returns {name: material}."""
    os.makedirs(scratch, exist_ok=True)
    op = cfg["id"]
    skin_maps = _mhmat("skins", f"{cfg['skin']['texture']}/{cfg['skin']['texture']}.mhmat")
    eye_src = _mhmat("eyes", f"materials/{cfg['face']['eyes']}.mhmat")["diffuseTexture"]
    brow = cfg["face"]["eyebrows"]
    brow_src = _mhmat("eyebrows", f"{brow}/{brow}.mhmat")["diffuseTexture"]
    lash = mpfb.EYELASHES
    lash_src = _mhmat("eyelashes", f"{lash}/{lash}.mhmat")["diffuseTexture"]

    skin_img = _save(_load(skin_maps["diffuseTexture"]), SKIN_SIZE, os.path.join(scratch, f"{op}_skin.jpg"), "JPEG")
    eye_img = _save(_load(eye_src), SMALL_SIZE, os.path.join(scratch, f"{op}_eyes.jpg"), "JPEG")
    brow_img = _brow_atlas(brow_src, lash_src, os.path.join(scratch, f"{op}_brows.png"))

    out = {}
    mat, nodes, links, bsdf = _principled("skin", 0.55)
    tex = nodes.new("ShaderNodeTexImage")
    tex.image = skin_img
    mix = nodes.new("ShaderNodeMix")
    mix.data_type = "RGBA"
    mix.blend_type = "MULTIPLY"
    mix.inputs["Factor"].default_value = 1.0
    links.new(tex.outputs["Color"], mix.inputs["A"])
    _vertex_colour(nodes, links, mix.inputs["B"])
    links.new(mix.outputs["Result"], bsdf.inputs["Base Color"])
    out["skin"] = mat

    for name, rough in (("uniform", 0.9), ("webbing", 0.85), ("leather", 0.7), ("hard", 0.45)):
        mat, nodes, links, bsdf = _principled(name, rough)
        _vertex_colour(nodes, links, bsdf.inputs["Base Color"])
        out[name] = mat

    mat, nodes, links, bsdf = _principled("eyes", 0.2)
    tex = nodes.new("ShaderNodeTexImage")
    tex.image = eye_img
    links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    out["eyes"] = mat

    mat, nodes, links, bsdf = _principled("brows", 0.8)
    tex = nodes.new("ShaderNodeTexImage")
    tex.image = brow_img
    links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    links.new(tex.outputs["Alpha"], bsdf.inputs["Alpha"])
    mat.surface_render_method = "DITHERED"
    out["brows"] = mat
    return out


def add_preview_detail(materials, detail_root):
    """After export: plug the shared detail maps into the materials so
    Blender previews look like the game."""
    for name, width in FABRICS.items():
        _add_detail(materials[name], os.path.join(detail_root, name), width)
    _add_detail(materials["skin"], os.path.join(detail_root, "skin"), None)


def _add_detail(mat, folder, width):
    nodes, links = mat.node_tree.nodes, mat.node_tree.links
    bsdf = nodes.get("Principled BSDF")
    coord = None
    if width:
        uv = nodes.new("ShaderNodeTexCoord")
        mapping = nodes.new("ShaderNodeMapping")
        mapping.inputs["Scale"].default_value = (1 / width, 1 / width, 1)
        links.new(uv.outputs["UV"], mapping.inputs["Vector"])
        coord = mapping.outputs["Vector"]
    normal_tex = nodes.new("ShaderNodeTexImage")
    normal_tex.image = _load(os.path.join(folder, "normal.jpg"))
    normal_tex.image.colorspace_settings.name = "Non-Color"
    arm_tex = nodes.new("ShaderNodeTexImage")
    arm_tex.image = _load(os.path.join(folder, "arm.jpg"))
    arm_tex.image.colorspace_settings.name = "Non-Color"
    if coord is not None:
        links.new(coord, normal_tex.inputs["Vector"])
        links.new(coord, arm_tex.inputs["Vector"])
    nmap = nodes.new("ShaderNodeNormalMap")
    links.new(normal_tex.outputs["Color"], nmap.inputs["Color"])
    links.new(nmap.outputs["Normal"], bsdf.inputs["Normal"])
    sep = nodes.new("ShaderNodeSeparateColor")
    links.new(arm_tex.outputs["Color"], sep.inputs["Color"])
    links.new(sep.outputs["Green"], bsdf.inputs["Roughness"])
