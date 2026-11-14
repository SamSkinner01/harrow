//! Scanned props (CC0, Poly Haven; see ASSETS.md) in `assets/models/props/`.
//!
//! Unlike kit pieces they keep their own embedded materials and have no
//! collider nodes, so each model gets a box collider from [`PROPS`] (its
//! bounding box; trees and stools get a smaller one). Bevy builds no
//! mipmaps for glTF textures, so they are generated when a prop first
//! appears, as for the material library.

use avian3d::prelude::*;
use bevy::{
    prelude::*,
    world_serialization::{WorldAssetRoot, WorldInstanceReady},
};

use super::materials::add_mip_chain;

pub struct PropsPlugin;

impl Plugin for PropsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(add_prop_mipmaps);
    }
}

/// Root of a scanned prop.
#[derive(Component)]
pub struct ScannedProp;

/// Prop file (without `.glb`) and collider box (x, height, z in metres,
/// before scaling); `None` for small clutter you can walk through.
pub const PROPS: &[(&str, Option<(f32, f32, f32)>)] = &[
    ("wine_barrel_01", Some((0.72, 0.87, 0.72))),
    ("brass_pot_01", None),
    ("ceramic_pot", Some((0.5, 0.37, 0.5))),
    ("planter_pot_clay", None),
    ("old_military_crate", Some((1.82, 0.30, 0.98))),
    ("wooden_military_crate", Some((1.24, 0.46, 0.52))),
    ("ammo_box", None),
    ("metal_jerrycan_green", Some((0.37, 0.5, 0.17))),
    ("vintage_radio_transceiver", Some((0.62, 0.43, 0.42))),
    ("WoodenTable_03", Some((1.33, 0.83, 0.58))),
    ("small_wooden_table_01", Some((0.92, 0.53, 0.44))),
    ("folding_wooden_stool", Some((0.45, 0.44, 0.45))),
    ("wooden_bookshelf_worn", Some((1.37, 2.06, 0.58))),
    ("worn_metal_rack", Some((0.92, 1.90, 0.60))),
    ("medical_box", None),
    ("wooden_bucket_01", Some((0.34, 0.55, 0.34))),
    ("stone_fire_pit", Some((1.45, 0.39, 1.43))),
    ("namaqualand_boulder_02_lowpoly", Some((2.3, 0.85, 1.1))),
    ("namaqualand_boulder_04_lowpoly", Some((2.3, 1.85, 2.3))),
    ("quiver_tree_02_lowpoly", Some((0.3, 1.47, 0.3))),
    ("dead_quiver_trunk", Some((0.25, 1.99, 0.25))),
];

/// Components for scanned prop `name` (a [`PROPS`] entry) at `transform`
/// (which may scale it uniformly).
pub fn prop(assets: &AssetServer, name: &str, transform: Transform) -> impl Bundle {
    let collider = PROPS
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("unknown scanned prop {name:?}"))
        .1;
    let path = format!("models/props/{name}.glb");
    let root = (
        Name::new(name.to_string()),
        ScannedProp,
        WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))),
        RigidBody::Static,
        transform,
    );
    // The collider sits on a child so it can be centred on the box (prop
    // origins are at the base) and scale with the prop.
    let child = collider.map(|(x, y, z)| (Collider::cuboid(x, y, z), Transform::from_xyz(0.0, y / 2.0, 0.0)));
    (root, Children::spawn(SpawnIter(child.into_iter())))
}

fn add_prop_mipmaps(
    ready: On<WorldInstanceReady>,
    props: Query<(), With<ScannedProp>>,
    children: Query<&Children>,
    surfaces: Query<&MeshMaterial3d<StandardMaterial>>,
    materials: Res<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    if !props.contains(ready.entity) {
        return;
    }
    for entity in children.iter_descendants(ready.entity) {
        let Ok(surface) = surfaces.get(entity) else {
            continue;
        };
        let Some(material) = materials.get(&surface.0) else {
            continue;
        };
        for texture in [
            &material.base_color_texture,
            &material.normal_map_texture,
            &material.metallic_roughness_texture,
            &material.occlusion_texture,
            &material.emissive_texture,
        ]
        .into_iter()
        .flatten()
        {
            // Only once per image: add_mip_chain skips images with mips.
            let needs = images
                .get(texture)
                .is_some_and(|image| image.texture_descriptor.mip_level_count == 1);
            if needs && let Some(mut image) = images.get_mut(texture) {
                add_mip_chain(&mut image);
            }
        }
    }
}
