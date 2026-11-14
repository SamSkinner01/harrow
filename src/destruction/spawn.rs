//! Turning soft kit pieces into [`SoftSurface`]s, and keeping their meshes
//! and colliders in step with the cells.
//!
//! The kit loader marks every piece root with a [`SurfaceKind`] once its
//! glTF content has spawned and its materials are swapped for the shared
//! library ones. When that happens to a soft piece we know (by its `Name`),
//! the glTF children are replaced by:
//!
//! - one collider child (compound of boxes), rebuilt on the fixed tick when
//!   the cells change, so movement and bullets see holes right away;
//! - up to three mesh children (plaster / wood / metal), rebuilt in
//!   `PostUpdate` when the cells change.
//!
//! Library materials are picked up from the kit pieces as they load (the
//! material library itself is private to the map module).

use std::collections::HashMap;

use avian3d::prelude::*;
use bevy::{gltf::GltfMaterialName, prelude::*, world_serialization::WorldAssetRoot};

use super::{
    DestructionSystems,
    build::{build_meshes, collider_shapes},
    surface::{NotReinforceable, SoftSurface, SurfaceMaterial, layout_for, placement_seed},
};
use crate::{combat::SurfaceKind, config::Tuning};

pub struct SurfaceSpawnPlugin;

impl Plugin for SurfaceSpawnPlugin {
    fn build(&self, app: &mut App) {
        // A headless server (no render app) never draws the surfaces: no
        // meshes (on the full map they were most of a server's memory).
        let draws = app.get_sub_app(bevy::render::RenderApp).is_some();
        app.insert_resource(SurfaceMeshes(draws))
            .init_resource::<SurfaceMaterials>()
            .init_resource::<RebuildStats>()
            .add_observer(convert_piece)
            .add_systems(
                FixedUpdate,
                rebuild_colliders.in_set(DestructionSystems::Rebuild),
            )
            .add_systems(
                PostUpdate,
                (assign_materials, rebuild_meshes.run_if(|draws: Res<SurfaceMeshes>| draws.0)),
            );
    }
}

/// Whether soft surfaces get render meshes (false on a headless server:
/// they keep empty ones, and only their colliders follow the cells).
#[derive(Resource)]
struct SurfaceMeshes(bool);

/// Library materials by surface name, collected from loaded kit pieces.
#[derive(Resource, Default)]
pub struct SurfaceMaterials {
    by_name: HashMap<String, Handle<StandardMaterial>>,
    fallback: HashMap<SurfaceMaterial, Handle<StandardMaterial>>,
    /// Reinforcement plates: the library metal, darkened to steel.
    plate: Option<Handle<StandardMaterial>>,
}

impl SurfaceMaterials {
    pub fn get(&self, material: SurfaceMaterial) -> Option<Handle<StandardMaterial>> {
        self.by_name
            .get(material.library_name())
            .or_else(|| self.fallback.get(&material))
            .cloned()
    }
}

/// How much rebuilding has cost (for performance checks).
#[derive(Resource, Default, Debug)]
pub struct RebuildStats {
    pub meshes: u32,
    pub mesh_time: std::time::Duration,
    pub slowest_mesh: std::time::Duration,
    pub colliders: u32,
    pub collider_time: std::time::Duration,
    pub slowest_collider: std::time::Duration,
}

/// Children of a converted surface.
#[derive(Component)]
pub struct SurfaceParts {
    pub collider: Entity,
    /// Mesh entity and handle per [`SurfaceMaterial`].
    pub meshes: [(Entity, Handle<Mesh>); 3],
    collider_revision: u32,
    mesh_revision: u32,
}

/// Mesh child still waiting for its library material (frames waited).
#[derive(Component)]
struct NeedsMaterial {
    material: SurfaceMaterial,
    /// Metal on a reinforceable surface is the reinforcement plate.
    plate: bool,
    waited: u32,
}

/// Tint applied to the library metal for reinforcement plates.
const PLATE_TINT: Color = Color::linear_rgb(0.42, 0.42, 0.44);

#[allow(clippy::too_many_arguments)]
fn convert_piece(
    insert: On<Insert<SurfaceKind>>,
    names: Query<&Name>,
    transforms: Query<&Transform>,
    converted: Query<(), With<SoftSurface>>,
    locked: Query<(), With<NotReinforceable>>,
    children: Query<&Children>,
    gltf_materials: Query<(&GltfMaterialName, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<SurfaceMaterials>,
    mut meshes: ResMut<Assets<Mesh>>,
    tuning: Res<Tuning>,
    draws: Res<SurfaceMeshes>,
    mut commands: Commands,
) {
    let root = insert.entity;
    // Collect library materials from every kit piece that loads.
    for child in children.iter_descendants(root) {
        if let Ok((name, material)) = gltf_materials.get(child)
            && SurfaceMaterial::ALL.iter().any(|m| m.library_name() == name.0)
        {
            materials
                .by_name
                .entry(name.0.clone())
                .or_insert_with(|| material.0.clone());
        }
    }
    // Re-inserting SurfaceKind (reinforcement) must not rebuild the piece.
    if converted.contains(root) {
        return;
    }
    let Ok(name) = names.get(root) else { return };
    let t = &tuning.destruction;
    let Some((mut spec, grid)) = layout_for(name.as_str(), t) else {
        return;
    };
    if locked.contains(root) {
        spec.reinforceable = false;
    }
    let transform = transforms.get(root).copied().unwrap_or_default();
    let surface = SoftSurface::new(spec, grid, placement_seed(&transform));

    // Replace the glTF content with our own parts.
    commands
        .entity(root)
        .despawn_related::<Children>()
        .remove::<WorldAssetRoot>();
    let shapes = collider_shapes(&surface, t);
    let collider = commands
        .spawn((
            Name::new("Soft surface collider"),
            Transform::default(),
            ChildOf(root),
        ))
        .id();
    if !shapes.is_empty() {
        commands.entity(collider).insert(Collider::compound(shapes));
    }
    let buffers = if draws.0 { build_meshes(&surface, t) } else { Default::default() };
    let mut parts: Vec<(Entity, Handle<Mesh>)> = Vec::with_capacity(3);
    for (material, data) in SurfaceMaterial::ALL.into_iter().zip(buffers) {
        let visible = !data.is_empty();
        let handle = meshes.add(data.into_mesh());
        let entity = commands
            .spawn((
                Name::new(format!("Soft surface {}", material.library_name())),
                Mesh3d(handle.clone()),
                Transform::default(),
                if visible { Visibility::Inherited } else { Visibility::Hidden },
                NeedsMaterial {
                    material,
                    plate: material == SurfaceMaterial::Metal && surface.spec.reinforceable,
                    waited: 0,
                },
                ChildOf(root),
            ))
            .id();
        parts.push((entity, handle));
    }
    let revision = surface.revision;
    commands.entity(root).insert((
        surface,
        SurfaceParts {
            collider,
            meshes: [parts[0].clone(), parts[1].clone(), parts[2].clone()],
            collider_revision: revision,
            mesh_revision: revision,
        },
    ));
}

/// Gives mesh children their library material once it has been seen on a
/// kit piece; after a second without one, a plain stand-in is made.
fn assign_materials(
    mut waiting: Query<(Entity, &mut NeedsMaterial)>,
    mut materials: ResMut<SurfaceMaterials>,
    mut assets: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    for (entity, mut need) in &mut waiting {
        let handle = match materials.get(need.material) {
            Some(handle) if need.plate => match &materials.plate {
                Some(plate) => plate.clone(),
                None => {
                    let mut steel = assets.get(&handle).cloned().unwrap_or_default();
                    steel.base_color = PLATE_TINT;
                    let plate = assets.add(steel);
                    materials.plate = Some(plate.clone());
                    plate
                }
            },
            Some(handle) => handle,
            None if need.waited > 60 => {
                let colour = match need.material {
                    SurfaceMaterial::Plaster => Color::srgb(0.86, 0.80, 0.70),
                    SurfaceMaterial::Wood => Color::srgb(0.45, 0.33, 0.22),
                    SurfaceMaterial::Metal => Color::srgb(0.35, 0.35, 0.36),
                };
                let handle = assets.add(StandardMaterial {
                    base_color: colour,
                    perceptual_roughness: 0.8,
                    metallic: if need.material == SurfaceMaterial::Metal { 0.8 } else { 0.0 },
                    ..default()
                });
                materials.fallback.insert(need.material, handle.clone());
                handle
            }
            None => {
                need.waited += 1;
                continue;
            }
        };
        commands
            .entity(entity)
            .insert(MeshMaterial3d(handle))
            .remove::<NeedsMaterial>();
    }
}

/// Fixed tick: new collider for every surface whose cells changed.
fn rebuild_colliders(
    mut surfaces: Query<(&SoftSurface, &mut SurfaceParts), Changed<SoftSurface>>,
    tuning: Res<Tuning>,
    mut stats: ResMut<RebuildStats>,
    mut commands: Commands,
) {
    for (surface, mut parts) in &mut surfaces {
        if parts.collider_revision == surface.revision {
            continue;
        }
        parts.collider_revision = surface.revision;
        let start = std::time::Instant::now();
        let shapes = collider_shapes(surface, &tuning.destruction);
        if shapes.is_empty() {
            commands.entity(parts.collider).remove::<Collider>();
        } else {
            commands
                .entity(parts.collider)
                .insert(Collider::compound(shapes));
        }
        let spent = start.elapsed();
        stats.colliders += 1;
        stats.collider_time += spent;
        stats.slowest_collider = stats.slowest_collider.max(spent);
    }
}

/// Every frame: new meshes for every surface whose cells changed.
fn rebuild_meshes(
    mut surfaces: Query<(&SoftSurface, &mut SurfaceParts), Changed<SoftSurface>>,
    mut visibility: Query<&mut Visibility>,
    mut meshes: ResMut<Assets<Mesh>>,
    tuning: Res<Tuning>,
    mut stats: ResMut<RebuildStats>,
) {
    for (surface, mut parts) in &mut surfaces {
        if parts.mesh_revision == surface.revision {
            continue;
        }
        parts.mesh_revision = surface.revision;
        let start = std::time::Instant::now();
        let buffers = build_meshes(surface, &tuning.destruction);
        for ((entity, handle), data) in parts.meshes.iter().zip(buffers) {
            let visible = !data.is_empty();
            if let Ok(mut v) = visibility.get_mut(*entity) {
                v.set_if_neq(if visible { Visibility::Inherited } else { Visibility::Hidden });
            }
            let _ = meshes.insert(handle, data.into_mesh());
        }
        let spent = start.elapsed();
        stats.meshes += 1;
        stats.mesh_time += spent;
        stats.slowest_mesh = stats.slowest_mesh.max(spent);
    }
}
