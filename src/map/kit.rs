//! Loads fortress-kit pieces (`assets/models/kit/*.glb`, built by
//! `tools/scripts/build_kit.sh`) and prepares them once spawned:
//!
//! - nodes named `COL_*` become invisible convex colliders,
//! - materials are replaced by the shared library versions (by name),
//! - the piece root is a static rigid body with a [`SurfaceKind`] taken from
//!   the piece's `surface` extra (hard unless the kit says soft).
//!
//! The root keeps its `Name` (the piece name, e.g. "soft_wall_2m") and
//! `Transform`, so other systems (destruction) can find and replace pieces.
//!
//! `FRAG_*` nodes (pre-broken props) are not used until destruction (M4).

use avian3d::prelude::*;
use bevy::{
    gltf::{GltfAssetLabel, GltfExtras, GltfMaterialName},
    prelude::*,
    world_serialization::{WorldAssetRoot, WorldInstanceReady},
};

use super::materials::MaterialLibrary;
use crate::combat::SurfaceKind;

pub struct KitPlugin;

impl Plugin for KitPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(prepare_piece)
            .add_systems(Update, detect_loaded.run_if(not(resource_exists::<KitLoaded>)));
    }
}

/// Inserted once every kit piece of the map has spawned and been prepared
/// (and transforms have settled). Colliders exist from about then on.
#[derive(Resource)]
pub struct KitLoaded;

fn detect_loaded(
    pieces: Query<(), With<KitPiece>>,
    waiting: Query<(), (With<KitPiece>, Without<SurfaceKind>)>,
    mut stable_frames: Local<u32>,
    mut commands: Commands,
) {
    if pieces.is_empty() || !waiting.is_empty() {
        *stable_frames = 0;
        return;
    }
    *stable_frames += 1;
    if *stable_frames >= 3 {
        info!("map loaded: {} kit pieces", pieces.iter().count());
        commands.insert_resource(KitLoaded);
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
    extras: Query<&GltfExtras>,
    library: Res<MaterialLibrary>,
    mut commands: Commands,
) {
    if !pieces.contains(ready.entity) {
        return;
    }
    let mut surface = SurfaceKind::Hard;
    for entity in children.iter_descendants(ready.entity) {
        if let Ok(extra) = extras.get(entity) {
            let compact: String = extra.value.split_whitespace().collect();
            if compact.contains("\"surface\":\"soft\"") {
                surface = SurfaceKind::Soft;
            }
        }
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
    if let Ok(name) = names.get(ready.entity) {
        debug!("kit piece {name} is {surface:?}");
    }
    commands.entity(ready.entity).insert(surface);
}
