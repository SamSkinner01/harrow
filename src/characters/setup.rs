//! Wires up a character once its glTF scene has spawned: finds the
//! animation player and the bones, fixes up the shared materials (eyebrow
//! alpha, mipmaps), and shows or hides the prop weapon.

use std::collections::{HashMap, HashSet};

use bevy::{
    gltf::GltfMaterialName, prelude::*, render::render_resource::TextureFormat,
    world_serialization::WorldInstanceReady,
};

use super::Character;

pub(super) struct SetupPlugin;

impl Plugin for SetupPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PreparedAssets>()
            .add_observer(prepare_character)
            .add_systems(Update, sync_prop_weapon);
    }
}

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
) {
    if !prepared.materials.insert(handle.id()) {
        return;
    }
    let Some(mut material) = materials.get_mut(handle) else {
        return;
    };
    match name {
        // Eyebrow cards: cut out rather than blended (no sorting issues).
        "eyebrows" => material.alpha_mode = AlphaMode::Mask(0.5),
        "skin" => material.perceptual_roughness = 0.62,
        "gear" => material.perceptual_roughness = 0.85,
        _ => {}
    }
    // glTF textures arrive without mipmaps and shimmer at a distance.
    if let Some(texture) = material.base_color_texture.clone()
        && prepared.images.insert(texture.id())
        && let Some(mut image) = images.get_mut(&texture)
    {
        add_mip_chain(&mut image);
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
