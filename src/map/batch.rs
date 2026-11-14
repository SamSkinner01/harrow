//! Static batching: once every kit piece has loaded, the meshes of all hard
//! (indestructible) pieces are merged into a few large meshes, one per
//! material and 16 m map cell (and storey band), and the per-piece meshes
//! stop rendering. Colliders, names and transforms of the pieces are left
//! alone; soft pieces (which destruction rebuilds) are never merged.
//!
//! Why: the full map has ~6000 kit pieces. Rendering cost is mostly per
//! entity (visibility for the camera, the sun's cascades and every
//! shadow-casting lamp), not per triangle, so a few hundred merged meshes
//! are much cheaper than thousands of small ones.
//!
//! Runs on maps that ask for it ([`StaticBatching`]); section A and the
//! grey-box are small enough without.

use std::collections::HashMap;

use avian3d::prelude::Collider;
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, VertexAttributeValues},
    prelude::*,
    render::render_resource::PrimitiveTopology,
};

use super::kit::{KitLoaded, KitPiece};
use crate::combat::SurfaceKind;

pub struct BatchPlugin;

impl Plugin for BatchPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            merge_static_pieces
                .run_if(resource_exists::<StaticBatching>)
                .run_if(resource_added::<KitLoaded>),
        );
    }
}

/// Insert to merge static kit geometry once the map has loaded.
#[derive(Resource, Default)]
pub struct StaticBatching;

/// A merged mesh.
#[derive(Component)]
pub struct MergedStatic;

/// Size of a batching cell, metres.
const CELL: f32 = 16.0;

#[derive(Default)]
struct Batch {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    tangents: Vec<[f32; 4]>,
    indices: Vec<u32>,
    has_tangents: bool,
    pieces: u32,
}

#[allow(clippy::type_complexity)]
fn merge_static_pieces(
    pieces: Query<(Entity, &SurfaceKind), With<KitPiece>>,
    children: Query<&Children>,
    parts: Query<(&Mesh3d, &MeshMaterial3d<StandardMaterial>, &GlobalTransform, &Visibility), Without<Collider>>,
    collider_sources: Query<(), (With<Collider>, With<Mesh3d>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    let start = std::time::Instant::now();

    let mut batches: HashMap<(AssetId<StandardMaterial>, i32, i32, i32), (Handle<StandardMaterial>, Batch)> =
        HashMap::new();
    let mut merged_parts = Vec::new();
    let mut unused_sources = Vec::new();
    for (root, kind) in &pieces {
        if *kind != SurfaceKind::Hard {
            continue;
        }
        for entity in children.iter_descendants(root) {
            // Collider source meshes are hidden and no longer needed.
            if collider_sources.contains(entity) {
                unused_sources.push(entity);
                continue;
            }
            let Ok((mesh3d, material, global, visibility)) = parts.get(entity) else {
                continue;
            };
            if *visibility == Visibility::Hidden {
                continue;
            }
            let Some(mesh) = meshes.get(&mesh3d.0) else {
                continue;
            };
            let at = global.translation();
            let band = if at.y < -1.0 { 0 } else if at.y < 6.0 { 1 } else { 2 };
            let key = (
                material.0.id(),
                (at.x / CELL).floor() as i32,
                (at.z / CELL).floor() as i32,
                band,
            );
            let (_, batch) = batches
                .entry(key)
                .or_insert_with(|| (material.0.clone(), Batch { has_tangents: true, ..default() }));
            if append(batch, mesh, global) {
                merged_parts.push(entity);
            }
        }
    }

    let mut vertices = 0;
    let count = batches.len();
    for (_, (material, batch)) in batches {
        vertices += batch.positions.len();
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, batch.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, batch.normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, batch.uvs)
            .with_inserted_indices(Indices::U32(batch.indices));
        if batch.has_tangents {
            mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, batch.tangents);
        } else if let Err(err) = mesh.generate_tangents() {
            warn!("static batch: no tangents ({err})");
        }
        commands.spawn((
            Name::new(format!("Merged static ({} pieces)", batch.pieces)),
            MergedStatic,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material),
            Transform::IDENTITY,
        ));
    }
    for entity in merged_parts.iter().chain(&unused_sources) {
        commands.entity(*entity).remove::<Mesh3d>();
    }
    info!(
        "static batching: {} meshes merged into {count} ({vertices} vertices) in {:.0} ms",
        merged_parts.len(),
        start.elapsed().as_secs_f32() * 1000.0
    );
}

/// Append `mesh`, moved by `transform`, to `batch`. False if the mesh has
/// an unexpected layout (it is then left to render on its own).
fn append(batch: &mut Batch, mesh: &Mesh, transform: &GlobalTransform) -> bool {
    let attr = |id| mesh.try_attribute_option(id).ok().flatten();
    let (
        Some(VertexAttributeValues::Float32x3(positions)),
        Some(VertexAttributeValues::Float32x3(normals)),
        Some(VertexAttributeValues::Float32x2(uvs)),
    ) = (
        attr(Mesh::ATTRIBUTE_POSITION),
        attr(Mesh::ATTRIBUTE_NORMAL),
        attr(Mesh::ATTRIBUTE_UV_0),
    )
    else {
        return false;
    };
    let Ok(Some(indices)) = mesh.try_indices_option() else {
        return false;
    };
    if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
        return false;
    }
    let affine = transform.affine();
    let rotation = transform.rotation();
    let base = batch.positions.len() as u32;
    batch
        .positions
        .extend(positions.iter().map(|p| affine.transform_point3(Vec3::from(*p)).to_array()));
    batch
        .normals
        .extend(normals.iter().map(|n| (rotation * Vec3::from(*n)).to_array()));
    batch.uvs.extend_from_slice(uvs);
    match attr(Mesh::ATTRIBUTE_TANGENT) {
        Some(VertexAttributeValues::Float32x4(tangents)) if batch.has_tangents => {
            batch.tangents.extend(tangents.iter().map(|t| {
                let v = rotation * Vec3::new(t[0], t[1], t[2]);
                [v.x, v.y, v.z, t[3]]
            }));
        }
        _ => batch.has_tangents = false,
    }
    batch.indices.extend(indices.iter().map(|i| base + i as u32));
    batch.pieces += 1;
    true
}
