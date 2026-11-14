"""Character materials and textures.

Four materials per operator, matched by name in the game:

- "skin": the MakeHuman CC0 skin texture, downscaled to 1024 px, times the
  vertex colour (tint, buzz cut, stubble).
- "gear": white times the vertex colour. Uniform, gloves, boots and every
  gear piece use it, so they cost one draw call together.
- "eyes": the MakeHuman iris texture at 128 px.
- "eyebrows": the MakeHuman eyebrow texture at 128 px, alpha-masked.

Textures are written to a scratch folder and embedded in the .glb.
"""

import os

import bpy

from . import mpfb

SKIN_SIZE = 1024
SMALL_SIZE = 128


def _mhmat_texture(subdir, fragment):
    """Absolute path of the diffuse texture named in a MakeHuman .mhmat."""
    path = mpfb.services().AssetService.find_asset_absolute_path(fragment, subdir)
    if path is None:
        raise FileNotFoundError(f"MakeHuman asset {subdir}/{fragment} is not installed")
    with open(path, encoding="utf-8") as f:
        for line in f:
            parts = line.split()
            if len(parts) == 2 and parts[0] == "diffuseTexture":
                return os.path.join(os.path.dirname(path), parts[1])
    raise ValueError(f"{path} names no diffuseTexture")


def _resized(src, size, out_path, file_format):
    image = bpy.data.images.load(src, check_existing=False)
    if image.size[0] != size:
        image.scale(size, size)
    image.filepath_raw = out_path
    image.file_format = file_format
    image.save(filepath=out_path, quality=90)
    image.name = os.path.splitext(os.path.basename(out_path))[0]
    return image


def _principled(name):
    mat = bpy.data.materials.new(name)
    nodes = mat.node_tree.nodes
    bsdf = nodes.get("Principled BSDF")
    return mat, nodes, mat.node_tree.links, bsdf


def build(cfg, scratch):
    """Create the four materials for an operator; returns {name: material}."""
    os.makedirs(scratch, exist_ok=True)
    op = cfg["id"]
    skin_src = _mhmat_texture("skins", f"{cfg['skin']['texture']}/{cfg['skin']['texture']}.mhmat")
    eye_src = _mhmat_texture("eyes", f"materials/{cfg['face']['eyes']}.mhmat")
    brow = cfg["face"]["eyebrows"]
    brow_src = _mhmat_texture("eyebrows", f"{brow}/{brow}.mhmat")

    skin_img = _resized(skin_src, SKIN_SIZE, os.path.join(scratch, f"{op}_skin.jpg"), "JPEG")
    eye_img = _resized(eye_src, SMALL_SIZE, os.path.join(scratch, f"{op}_eyes.jpg"), "JPEG")
    brow_img = _resized(brow_src, SMALL_SIZE, os.path.join(scratch, f"{op}_eyebrows.png"), "PNG")

    out = {}

    # Skin: texture x vertex colour.
    mat, nodes, links, bsdf = _principled("skin")
    tex = nodes.new("ShaderNodeTexImage")
    tex.image = skin_img
    col = nodes.new("ShaderNodeVertexColor")
    col.layer_name = "Color"
    mix = nodes.new("ShaderNodeMix")
    mix.data_type = "RGBA"
    mix.blend_type = "MULTIPLY"
    mix.inputs["Factor"].default_value = 1.0
    links.new(tex.outputs["Color"], mix.inputs["A"])
    links.new(col.outputs["Color"], mix.inputs["B"])
    links.new(mix.outputs["Result"], bsdf.inputs["Base Color"])
    bsdf.inputs["Roughness"].default_value = 0.62
    out["skin"] = mat

    # Gear: vertex colour only.
    mat, nodes, links, bsdf = _principled("gear")
    col = nodes.new("ShaderNodeVertexColor")
    col.layer_name = "Color"
    links.new(col.outputs["Color"], bsdf.inputs["Base Color"])
    bsdf.inputs["Roughness"].default_value = 0.85
    out["gear"] = mat

    mat, nodes, links, bsdf = _principled("eyes")
    tex = nodes.new("ShaderNodeTexImage")
    tex.image = eye_img
    links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    bsdf.inputs["Roughness"].default_value = 0.25
    out["eyes"] = mat

    mat, nodes, links, bsdf = _principled("eyebrows")
    tex = nodes.new("ShaderNodeTexImage")
    tex.image = brow_img
    links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    links.new(tex.outputs["Alpha"], bsdf.inputs["Alpha"])
    bsdf.inputs["Roughness"].default_value = 0.8
    mat.surface_render_method = "DITHERED"
    out["eyebrows"] = mat
    return out
