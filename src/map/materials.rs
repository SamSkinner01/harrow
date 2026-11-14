//! Material library: one shared `StandardMaterial` per surface type, built
//! from the CC0 texture sets in `assets/textures/<name>/` (see ASSETS.md).
//!
//! Kit pieces exported from Blender only *name* their material; the kit
//! loader swaps in the library version, so every wall shares one material
//! and textures are loaded once.
//!
//! Textures tile at their real-world size: kit UVs are in metres and each
//! material scales them by the texture's physical width.
//!
//! Bevy doesn't build mipmaps for JPG/PNG, and without them textures shimmer
//! at a distance, so they are generated on the CPU when each texture loads.

use std::collections::{HashMap, HashSet};

use bevy::{
    image::{
        ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler,
        ImageSamplerDescriptor,
    },
    math::Affine2,
    prelude::*,
    render::render_resource::TextureFormat,
};

use crate::config::Tuning;

pub struct MaterialsPlugin;

impl Plugin for MaterialsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MaterialLibrary>().add_systems(
            PreUpdate,
            (
                generate_mipmaps,
                apply_surface_tuning.run_if(resource_changed::<Tuning>),
            ),
        );
    }
}

/// Surface names, as used by the Blender kit. Each has a texture set in
/// `assets/textures/<name>/` and a `surfaces` entry (scale, tint) in the
/// tuning file. Real-world texture widths from Poly Haven are listed by
/// `tools/scripts/fetch_textures.py`.
const SURFACES: &[&str] = &[
    "stone", "plaster", "flagstone", "sand", "wood", "metal", "burlap",
];

#[derive(Resource)]
pub struct MaterialLibrary {
    materials: HashMap<&'static str, Handle<StandardMaterial>>,
    /// Images that still need mipmaps.
    textures: HashSet<AssetId<Image>>,
}

impl MaterialLibrary {
    pub fn get(&self, surface: &str) -> Option<Handle<StandardMaterial>> {
        self.materials.get(surface).cloned()
    }
}

impl FromWorld for MaterialLibrary {
    fn from_world(world: &mut World) -> Self {
        let server = world.resource::<AssetServer>().clone();
        let mut textures = HashSet::new();
        let mut load = |path: String, srgb: bool| {
            let handle: Handle<Image> = server
                .load_builder()
                .with_settings(move |s: &mut ImageLoaderSettings| {
                    s.is_srgb = srgb;
                    s.sampler = ImageSampler::Descriptor(tiling_sampler());
                })
                .load(path);
            textures.insert(handle.id());
            handle
        };

        let tuning = world.resource::<Tuning>().clone();
        let mut materials = HashMap::new();
        let mut assets = world.resource_mut::<Assets<StandardMaterial>>();
        for &name in SURFACES {
            let (tint, uv_transform) = surface_look(&tuning, name);
            let arm = load(format!("textures/{name}/arm.jpg"), false);
            let material = StandardMaterial {
                base_color: tint,
                base_color_texture: Some(load(format!("textures/{name}/diff.jpg"), true)),
                normal_map_texture: Some(load(format!("textures/{name}/normal.jpg"), false)),
                // ARM packing: R = ambient occlusion, G = roughness, B = metal.
                occlusion_texture: Some(arm.clone()),
                metallic_roughness_texture: Some(arm),
                perceptual_roughness: 1.0,
                metallic: 1.0,
                uv_transform,
                ..default()
            };
            materials.insert(name, assets.add(material));
        }
        Self {
            materials,
            textures,
        }
    }
}

/// Tint and UV transform for a surface from the tuning file.
fn surface_look(tuning: &Tuning, name: &str) -> (Color, Affine2) {
    let Some(look) = tuning.surfaces.get(name) else {
        warn!("no `surfaces` entry for {name:?} in the tuning file");
        return (Color::WHITE, Affine2::IDENTITY);
    };
    let (r, g, b) = look.tint;
    (
        Color::linear_rgb(r, g, b),
        Affine2::from_scale(Vec2::splat(1.0 / look.scale.max(0.01))),
    )
}

fn apply_surface_tuning(
    library: Res<MaterialLibrary>,
    tuning: Res<Tuning>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (name, handle) in &library.materials {
        let (tint, uv_transform) = surface_look(&tuning, name);
        if let Some(mut material) = materials.get_mut(handle) {
            material.base_color = tint;
            material.uv_transform = uv_transform;
        }
    }
}

fn tiling_sampler() -> ImageSamplerDescriptor {
    ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..default()
    }
}

fn generate_mipmaps(
    mut events: MessageReader<AssetEvent<Image>>,
    mut library: ResMut<MaterialLibrary>,
    mut images: ResMut<Assets<Image>>,
) {
    for event in events.read() {
        let AssetEvent::LoadedWithDependencies { id } = event else {
            continue;
        };
        if !library.textures.remove(id) {
            continue;
        }
        if let Some(mut image) = images.get_mut(*id) {
            add_mip_chain(&mut image);
        }
    }
}

/// Appends a full box-filtered mip chain to an RGBA8 image. sRGB images are
/// averaged in linear space so distant surfaces don't darken.
fn add_mip_chain(image: &mut Image) {
    let srgb = match image.texture_descriptor.format {
        TextureFormat::Rgba8UnormSrgb => true,
        TextureFormat::Rgba8Unorm => false,
        other => {
            warn!("no mipmaps for texture format {other:?}");
            return;
        }
    };
    if image.texture_descriptor.mip_level_count > 1 {
        return;
    }
    let Some(base) = image.data.take() else {
        return;
    };

    let to_linear: Vec<f32> = (0..256)
        .map(|i| {
            let c = i as f32 / 255.0;
            if srgb { srgb_to_linear(c) } else { c }
        })
        .collect();
    // linear (quantised to 4096 steps) -> 8-bit value, to avoid powf per pixel.
    let from_linear: Vec<u8> = (0..4096)
        .map(|i| {
            let l = i as f32 / 4095.0;
            let c = if srgb { linear_to_srgb(l) } else { l };
            (c * 255.0 + 0.5) as u8
        })
        .collect();

    let size = image.texture_descriptor.size;
    let (mut w, mut h) = (size.width as usize, size.height as usize);
    let mut all = base.clone();
    let mut prev = base;
    let mut levels = 1;
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            for x in 0..nw {
                let (x0, y0) = ((x * 2).min(w - 1), (y * 2).min(h - 1));
                let (x1, y1) = ((x * 2 + 1).min(w - 1), (y * 2 + 1).min(h - 1));
                for c in 0..4 {
                    let px = |xx: usize, yy: usize| prev[(yy * w + xx) * 4 + c] as usize;
                    let out = &mut next[(y * nw + x) * 4 + c];
                    if c == 3 {
                        // Alpha is linear in every format.
                        *out = ((px(x0, y0) + px(x1, y0) + px(x0, y1) + px(x1, y1) + 2) / 4) as u8;
                    } else {
                        let sum = to_linear[px(x0, y0)]
                            + to_linear[px(x1, y0)]
                            + to_linear[px(x0, y1)]
                            + to_linear[px(x1, y1)];
                        *out = from_linear[((sum / 4.0) * 4095.0 + 0.5) as usize];
                    }
                }
            }
        }
        all.extend_from_slice(&next);
        prev = next;
        (w, h) = (nw, nh);
        levels += 1;
    }
    image.data = Some(all);
    image.texture_descriptor.mip_level_count = levels;
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(l: f32) -> f32 {
    if l <= 0.003_130_8 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    }
}
