//! Loads fortress-kit pieces (`assets/models/kit/*.glb`, built by
//! `tools/scripts/build_kit.sh`) and prepares them once spawned:
//!
//! - nodes named `COL_*` become invisible convex colliders,
//! - materials are replaced by the shared library versions (by name),
//! - the piece root is a static rigid body.
//!
//! `FRAG_*` nodes (pre-broken props) are not used until destruction (M4).

use avian3d::prelude::*;
use bevy::{
    gltf::{GltfAssetLabel, GltfMaterialName},
    prelude::*,
    world_serialization::{WorldAssetRoot, WorldInstanceReady},
};

use super::materials::MaterialLibrary;

pub struct KitPlugin;

impl Plugin for KitPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(prepare_piece);
    }
}

/// Root of a spawned kit piece.
#[derive(Component)]
pub struct KitPiece;

/// Components for spawning kit piece `name` (e.g. "stone_wall_2m").
pub fn piece(assets: &AssetServer, name: &str) -> impl Bundle {
    let path = format!("models/kit/{name}.glb");
    (
        Name::new(name.to_string()),
        KitPiece,
        WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))),
        RigidBody::Static,
    )
}

fn prepare_piece(
    ready: On<WorldInstanceReady>,
    pieces: Query<(), With<KitPiece>>,
    children: Query<&Children>,
    names: Query<&Name>,
    material_names: Query<&GltfMaterialName>,
    library: Res<MaterialLibrary>,
    mut commands: Commands,
) {
    if !pieces.contains(ready.entity) {
        return;
    }
    for entity in children.iter_descendants(ready.entity) {
        if let Ok(name) = names.get(entity)
            && name.starts_with("COL_")
        {
            commands.entity(entity).insert((
                ColliderConstructorHierarchy::new(ColliderConstructor::ConvexHullFromMesh),
                Visibility::Hidden,
            ));
            continue;
        }
        if let Ok(material) = material_names.get(entity)
            && let Some(handle) = library.get(&material.0)
        {
            commands.entity(entity).insert(MeshMaterial3d(handle));
        }
    }
}
