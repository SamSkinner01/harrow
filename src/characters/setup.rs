//! Wires up a character once its glTF scene has spawned: finds the
//! animation player and the bones, finishes the materials, and shows or
//! hides the prop weapon.
//!
//! Materials come from the .glb by name (see
//! `tools/blender/characters/materials.py`). Colour textures (skin, eyes,
//! brows) are embedded per operator; the detail maps every operator shares
//! (skin pores, cloth weave, leather grain: normal + occlusion/roughness)
//! are loaded once here from `assets/textures/characters/` and plugged in,
//! like the fortress kit's material library.

use std::collections::{HashMap, HashSet};

use bevy::{
    gltf::GltfMaterialName,
    image::{
        ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler,
        ImageSamplerDescriptor,
    },
    math::Affine2,
    prelude::*,
    render::render_resource::TextureFormat,
    world_serialization::WorldInstanceReady,
};

use super::Character;

pub(super) struct SetupPlugin;

impl Plugin for SetupPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PreparedAssets>()
            .init_resource::<DetailTextures>()
            .add_observer(prepare_character)
            .add_systems(Update, (sync_prop_weapon, mip_detail_textures));
    }
}

/// Shared detail maps: (material name, folder, real-world width of one
/// texture repeat in metres; `None` = MakeHuman UVs, no tiling). Widths
/// are printed by `tools/scripts/fetch_character_textures.py`.
const DETAIL_TEXTURES: [(&str, &str, Option<f32>); 4] = [
    ("skin", "skin", None),
    ("uniform", "uniform", Some(0.291)),
    ("webbing", "webbing", Some(0.273)),
    ("leather", "leather", Some(0.5)),
];

/// A spawned character's animation player and bones.
#[derive(Component, Debug)]
pub struct CharacterParts {
    /// Entity with the `AnimationPlayer` (the glTF "rig" node).
    pub player: Entity,
    /// Every node of the model by name: bones (`head`, `neck_01`,
    /// `spine_01..03`, `pelvis`, `thigh_l`, `calf_l`, `foot_l`, ...,
    /// `weapon_r`) and meshes (`body`, `gear`, `weapon`).
    pub bones: HashMap<String, Entity>,
}

impl CharacterParts {
    pub fn bone(&self, name: &str) -> Option<Entity> {
        self.bones.get(name).copied()
    }
}

/// Shared assets that have already been fixed up (materials and textures
/// are shared by every instance of an operator).
#[derive(Resource, Default)]
struct PreparedAssets {
    materials: HashSet<AssetId<StandardMaterial>>,
    images: HashSet<AssetId<Image>>,
}

struct Detail {
    normal: Handle<Image>,
    arm: Handle<Image>,
    width: Option<f32>,
}

#[derive(Resource)]
struct DetailTextures {
    by_material: HashMap<&'static str, Detail>,
    /// Images that still need mipmaps.
    pending: HashSet<AssetId<Image>>,
}

impl FromWorld for DetailTextures {
    fn from_world(world: &mut World) -> Self {
        let server = world.resource::<AssetServer>();
        let mut pending = HashSet::new();
        let mut load = |path: String| {
            let handle: Handle<Image> = server
                .load_builder()
                .with_settings(|s: &mut ImageLoaderSettings| {
                    s.is_srgb = false;
                    s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                        address_mode_u: ImageAddressMode::Repeat,
                        address_mode_v: ImageAddressMode::Repeat,
                        mag_filter: ImageFilterMode::Linear,
                        min_filter: ImageFilterMode::Linear,
                        mipmap_filter: ImageFilterMode::Linear,
                        anisotropy_clamp: 8,
                        ..default()
                    });
                })
                .load(path);
            pending.insert(handle.id());
            handle
        };
        let by_material = DETAIL_TEXTURES
            .iter()
            .map(|&(material, folder, width)| {
                let detail = Detail {
                    normal: load(format!("textures/characters/{folder}/normal.jpg")),
                    arm: load(format!("textures/characters/{folder}/arm.jpg")),
                    width,
                };
                (material, detail)
            })
            .collect();
        Self {
            by_material,
            pending,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn prepare_character(
    ready: On<WorldInstanceReady>,
    characters: Query<&Character>,
    children: Query<&Children>,
    names: Query<&Name>,
    players: Query<(), With<AnimationPlayer>>,
    mesh_materials: Query<(&GltfMaterialName, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut prepared: ResMut<PreparedAssets>,
    details: Res<DetailTextures>,
    mut commands: Commands,
) {
    let Ok(character) = characters.get(ready.entity) else {
        return;
    };
    let mut player = None;
    let mut bones = HashMap::new();
    for entity in children.iter_descendants(ready.entity) {
        if players.contains(entity) {
            player = Some(entity);
        }
        if let Ok(name) = names.get(entity) {
            bones.entry(name.as_str().to_string()).or_insert(entity);
        }
        if let Ok((material_name, material)) = mesh_materials.get(entity) {
            prepare_material(
                &material_name.0,
                &material.0,
                &mut materials,
                &mut images,
                &mut prepared,
                &details,
            );
        }
    }
    let Some(player) = player else {
        error!("{:?}: no animation player in the model", character.operator);
        return;
    };
    if let Some(&weapon) = bones.get("weapon") {
        commands.entity(weapon).insert(prop_visibility(character));
    }
    commands
        .entity(ready.entity)
        .insert(CharacterParts { player, bones });
}

fn prop_visibility(character: &Character) -> Visibility {
    if character.prop_weapon {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    }
}

fn sync_prop_weapon(
    characters: Query<(&Character, &CharacterParts), Changed<Character>>,
    mut visibility: Query<&mut Visibility>,
) {
    for (character, parts) in &characters {
        if let Some(weapon) = parts.bone("weapon")
            && let Ok(mut v) = visibility.get_mut(weapon)
        {
            *v = prop_visibility(character);
        }
    }
}

fn prepare_material(
    name: &str,
    handle: &Handle<StandardMaterial>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    prepared: &mut PreparedAssets,
    details: &DetailTextures,
) {
    if !prepared.materials.insert(handle.id()) {
        return;
    }
    let Some(mut material) = materials.get_mut(handle) else {
        return;
    };
    material.metallic = 0.0;
    match name {
        // Eyebrow and eyelash cards: cut out rather than blended.
        "brows" => material.alpha_mode = AlphaMode::Mask(0.5),
        "eyes" => material.perceptual_roughness = 0.2,
        // Helmet shells, hardware, lenses, the prop carbine.
        "hard" => material.perceptual_roughness = 0.45,
        _ => {}
    }
    if let Some(detail) = details.by_material.get(name) {
        material.normal_map_texture = Some(detail.normal.clone());
        // ARM: occlusion in R, roughness in G (metal in B is 0).
        material.occlusion_texture = Some(detail.arm.clone());
        material.metallic_roughness_texture = Some(detail.arm.clone());
        material.perceptual_roughness = 1.0;
        if let Some(width) = detail.width {
            material.uv_transform = Affine2::from_scale(Vec2::splat(1.0 / width));
        }
    }
    // glTF textures arrive without mipmaps and shimmer at a distance.
    if let Some(texture) = material.base_color_texture.clone()
        && prepared.images.insert(texture.id())
        && let Some(mut image) = images.get_mut(&texture)
    {
        add_mip_chain(&mut image);
    }
}

fn mip_detail_textures(
    mut events: MessageReader<AssetEvent<Image>>,
    mut details: ResMut<DetailTextures>,
    mut images: ResMut<Assets<Image>>,
) {
    for event in events.read() {
        let AssetEvent::LoadedWithDependencies { id } = event else {
            continue;
        };
        if details.pending.remove(id)
            && let Some(mut image) = images.get_mut(*id)
        {
            add_mip_chain(&mut image);
        }
    }
}

/// Appends a box-filtered mip chain to an RGBA8 image (sRGB averaged in
/// linear space). Same approach as the fortress material library.
fn add_mip_chain(image: &mut Image) {
    let srgb = match image.texture_descriptor.format {
        TextureFormat::Rgba8UnormSrgb => true,
        TextureFormat::Rgba8Unorm => false,
        other => {
            warn!("no mipmaps for character texture format {other:?}");
            return;
        }
    };
    if image.texture_descriptor.mip_level_count > 1 {
        return;
    }
    let Some(base) = image.data.take() else {
        return;
    };
    let decode = |c: u8| {
        let c = c as f32 / 255.0;
        if !srgb {
            c
        } else if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let to_linear: Vec<f32> = (0..=255).map(decode).collect();
    let encode = |l: f32| {
        let c = if !srgb {
            l
        } else if l <= 0.003_130_8 {
            l * 12.92
        } else {
            1.055 * l.powf(1.0 / 2.4) - 0.055
        };
        (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
    };
    let from_linear: Vec<u8> = (0..4096).map(|i| encode(i as f32 / 4095.0)).collect();

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
                    let px = |xx: usize, yy: usize| prev[(yy * w + xx) * 4 + c];
                    let quad = [px(x0, y0), px(x1, y0), px(x0, y1), px(x1, y1)];
                    next[(y * nw + x) * 4 + c] = if c == 3 {
                        ((quad.iter().map(|&v| v as u32).sum::<u32>() + 2) / 4) as u8
                    } else {
                        let sum: f32 = quad.iter().map(|&v| to_linear[v as usize]).sum();
                        from_linear[((sum / 4.0) * 4095.0 + 0.5) as usize]
                    };
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
