//! Turns a surface's cell grid into render meshes (one per material) and a
//! compound collider. Both are rebuilt from scratch for one surface when it
//! changes: a few hundred cells, merged greedily into rectangles, so a
//! rebuild is a fraction of a millisecond.
//!
//! UVs follow the fortress kit (`tools/blender/kit/geometry.py`, after the
//! glTF exporter's v flip): metres, walls `(p . (Y x n), 1 + y)`, floors
//! `(x, 1 - z)`, so textures match the kit pieces they replace.

use avian3d::prelude::*;
use bevy::{
    asset::RenderAssetUsages, mesh::Indices, prelude::*,
    render::render_resource::PrimitiveTopology,
};

use super::{
    grid::{BRACE, CORE, PLATE, SKIN},
    surface::{SoftSurface, SurfaceMaterial},
};
use crate::config::DestructionTuning;

/// Vertex data for one mesh.
#[derive(Default)]
pub struct MeshBuffers {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    tangents: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl MeshBuffers {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    /// A flat quad facing `normal`; the winding is fixed up to match.
    pub fn quad(&mut self, mut corners: [Vec3; 4], normal: Vec3) {
        let geometric = (corners[1] - corners[0]).cross(corners[2] - corners[0]);
        if geometric.dot(normal) < 0.0 {
            corners.swap(1, 3);
        }
        let (tangent, bitangent) = uv_axes(normal);
        let w = if normal.cross(tangent).dot(bitangent) >= 0.0 { 1.0 } else { -1.0 };
        let base = self.positions.len() as u32;
        for p in corners {
            self.positions.push(p.into());
            self.normals.push(normal.into());
            self.uvs.push(uv(p, normal).into());
            self.tangents.push([tangent.x, tangent.y, tangent.z, w]);
        }
        self.indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Axis-aligned box.
    pub fn cuboid(&mut self, min: Vec3, max: Vec3) {
        let c = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
        let (a, b) = (min, max);
        self.quad([c(a.x, a.y, b.z), c(b.x, a.y, b.z), c(b.x, b.y, b.z), c(a.x, b.y, b.z)], Vec3::Z);
        self.quad([c(a.x, a.y, a.z), c(b.x, a.y, a.z), c(b.x, b.y, a.z), c(a.x, b.y, a.z)], Vec3::NEG_Z);
        self.quad([c(b.x, a.y, a.z), c(b.x, b.y, a.z), c(b.x, b.y, b.z), c(b.x, a.y, b.z)], Vec3::X);
        self.quad([c(a.x, a.y, a.z), c(a.x, b.y, a.z), c(a.x, b.y, b.z), c(a.x, a.y, b.z)], Vec3::NEG_X);
        self.quad([c(a.x, b.y, a.z), c(b.x, b.y, a.z), c(b.x, b.y, b.z), c(a.x, b.y, b.z)], Vec3::Y);
        self.quad([c(a.x, a.y, a.z), c(b.x, a.y, a.z), c(b.x, a.y, b.z), c(a.x, a.y, b.z)], Vec3::NEG_Y);
    }

    /// The finished mesh. An empty buffer gives a single degenerate
    /// triangle (hide the entity): GPU buffers can't be empty.
    pub fn into_mesh(mut self) -> Mesh {
        if self.is_empty() {
            for _ in 0..3 {
                self.positions.push([0.0; 3]);
                self.normals.push([0.0, 1.0, 0.0]);
                self.uvs.push([0.0; 2]);
                self.tangents.push([1.0, 0.0, 0.0, 1.0]);
            }
            self.indices.extend([0, 1, 2]);
        }
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs)
            .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, self.tangents)
            .with_inserted_indices(Indices::U32(self.indices))
    }
}

/// Kit UV for point `p` on a face with `normal` (piece-local space).
fn uv(p: Vec3, normal: Vec3) -> Vec2 {
    if normal.y.abs() > 0.7 {
        Vec2::new(p.x, 1.0 - p.z)
    } else {
        let right = Vec3::Y.cross(normal).normalize_or(Vec3::X);
        Vec2::new(p.dot(right), 1.0 + p.y)
    }
}

/// Directions in which u and v increase on a face (tangent, bitangent).
fn uv_axes(normal: Vec3) -> (Vec3, Vec3) {
    if normal.y.abs() > 0.7 {
        (Vec3::X, Vec3::NEG_Z)
    } else {
        (Vec3::Y.cross(normal).normalize_or(Vec3::X), Vec3::Y)
    }
}

/// One slab of material in a surface: the cells where it is present, how
/// deep it is, and which neighbours hide its side faces.
struct Layer {
    required: u8,
    excluded: u8,
    /// A neighbour with any of these flags covers this layer's side face.
    hider: u8,
    depth: (f32, f32),
    material: SurfaceMaterial,
}

impl Layer {
    fn has(&self, cell: u8) -> bool {
        cell & self.required == self.required && cell & self.excluded == 0
    }
}

fn layers(surface: &SoftSurface, tuning: &DestructionTuning) -> Vec<Layer> {
    let spec = &surface.spec;
    let mut out = vec![
        Layer {
            required: SKIN,
            excluded: 0,
            hider: SKIN,
            depth: spec.skin_depth,
            material: spec.skin_material,
        },
        Layer {
            required: CORE,
            excluded: SKIN,
            hider: SKIN | CORE,
            depth: spec.core_depth,
            material: SurfaceMaterial::Wood,
        },
    ];
    if let Some(r) = surface.reinforced {
        let (plate, brace) = surface.plate_depth(r.side, tuning);
        out.push(Layer {
            required: PLATE,
            excluded: 0,
            hider: PLATE,
            depth: plate,
            material: SurfaceMaterial::Metal,
        });
        out.push(Layer {
            required: BRACE | PLATE,
            excluded: 0,
            hider: BRACE,
            depth: brace,
            material: SurfaceMaterial::Metal,
        });
    }
    out
}

/// Render meshes for a surface, indexed by [`SurfaceMaterial::index`].
pub fn build_meshes(surface: &SoftSurface, tuning: &DestructionTuning) -> [MeshBuffers; 3] {
    let mut out: [MeshBuffers; 3] = Default::default();
    let spec = &surface.spec;
    let grid = &surface.grid;
    let plane = spec.plane;
    let (nu, nv) = (grid.nu() as isize, grid.nv() as isize);
    let normal_w = plane.normal();
    let normal_u = plane.to_local(1.0, 0.0, 0.0);
    let normal_v = plane.to_local(0.0, 1.0, 0.0);

    for layer in layers(surface, tuning) {
        let buffers = &mut out[layer.material.index()];
        let (w0, w1) = layer.depth;

        // Front and back faces over merged rectangles.
        for (_, r) in grid.greedy(|c| layer.has(c) as u8) {
            let (ua, ub) = (grid.us[r.u0], grid.us[r.u1]);
            let (va, vb) = (grid.vs[r.v0], grid.vs[r.v1]);
            for (w, n) in [(w1, normal_w), (w0, -normal_w)] {
                buffers.quad(
                    [
                        plane.to_local(ua, va, w),
                        plane.to_local(ub, va, w),
                        plane.to_local(ub, vb, w),
                        plane.to_local(ua, vb, w),
                    ],
                    n,
                );
            }
        }

        // Side faces wherever the layer meets a neighbour that doesn't cover
        // it (hole edges), merged into runs along each grid line.
        let shows = |from: (isize, isize), to: (isize, isize)| {
            let inside = to.0 >= 0 && to.1 >= 0 && to.0 < nu && to.1 < nv;
            layer.has(grid.get(from.0, from.1))
                && (if inside { grid.get(to.0, to.1) & layer.hider == 0 } else { spec.closed_borders })
        };
        // Lines of constant u: faces pointing +u (from cell u-1) and -u.
        for u in 0..=nu {
            let x = grid.us[u as usize];
            for (dir, from_offset, to_offset) in [(1.0, -1, 0), (-1.0, 0, -1)] {
                let mut start: Option<isize> = None;
                for v in 0..=nv {
                    let on = v < nv && shows((u + from_offset, v), (u + to_offset, v));
                    match (on, start) {
                        (true, None) => start = Some(v),
                        (false, Some(s)) => {
                            let (va, vb) = (grid.vs[s as usize], grid.vs[v as usize]);
                            buffers.quad(
                                [
                                    plane.to_local(x, va, w0),
                                    plane.to_local(x, vb, w0),
                                    plane.to_local(x, vb, w1),
                                    plane.to_local(x, va, w1),
                                ],
                                normal_u * dir,
                            );
                            start = None;
                        }
                        _ => {}
                    }
                }
            }
        }
        // Lines of constant v.
        for v in 0..=nv {
            let y = grid.vs[v as usize];
            for (dir, from_offset, to_offset) in [(1.0, -1, 0), (-1.0, 0, -1)] {
                let mut start: Option<isize> = None;
                for u in 0..=nu {
                    let on = u < nu && shows((u, v + from_offset), (u, v + to_offset));
                    match (on, start) {
                        (true, None) => start = Some(u),
                        (false, Some(s)) => {
                            let (ua, ub) = (grid.us[s as usize], grid.us[u as usize]);
                            buffers.quad(
                                [
                                    plane.to_local(ua, y, w0),
                                    plane.to_local(ub, y, w0),
                                    plane.to_local(ub, y, w1),
                                    plane.to_local(ua, y, w1),
                                ],
                                normal_v * dir,
                            );
                            start = None;
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    for fixture in spec.fixtures.iter().filter(|f| f.present) {
        out[fixture.material.index()].cuboid(fixture.min, fixture.max);
    }
    out
}

/// Boxes for the surface's compound collider (piece-local space): one per
/// merged rectangle of cells with the same layers, plus fixtures.
pub fn collider_shapes(surface: &SoftSurface, tuning: &DestructionTuning) -> Vec<(Vec3, Quat, Collider)> {
    let spec = &surface.spec;
    let plane = spec.plane;
    let grid = &surface.grid;
    let plate = surface
        .reinforced
        .map(|r| surface.plate_depth(r.side, tuning).0);
    let key = |c: u8| -> u8 {
        let mut k = 0;
        if c & SKIN != 0 {
            k |= 1;
        } else if c & CORE != 0 {
            k |= 2;
        }
        if c & PLATE != 0 && plate.is_some() {
            k |= 4;
        }
        k
    };
    let mut shapes = Vec::new();
    for (k, r) in grid.greedy(key) {
        let mut range = (f32::MAX, f32::MIN);
        let mut add = |(a, b): (f32, f32)| range = (range.0.min(a), range.1.max(b));
        if k & 1 != 0 {
            add(spec.skin_depth);
        }
        if k & 2 != 0 {
            add(spec.core_depth);
        }
        if k & 4 != 0
            && let Some(p) = plate
        {
            add(p);
        }
        let (ua, ub) = (grid.us[r.u0], grid.us[r.u1]);
        let (va, vb) = (grid.vs[r.v0], grid.vs[r.v1]);
        let centre = plane.to_local((ua + ub) / 2.0, (va + vb) / 2.0, (range.0 + range.1) / 2.0);
        let size = plane.size(ub - ua, vb - va, range.1 - range.0);
        shapes.push((centre, Quat::IDENTITY, Collider::cuboid(size.x, size.y, size.z)));
    }
    for fixture in spec.fixtures.iter().filter(|f| f.present) {
        let size = fixture.max - fixture.min;
        shapes.push((
            (fixture.min + fixture.max) / 2.0,
            Quat::IDENTITY,
            Collider::cuboid(size.x, size.y, size.z),
        ));
    }
    shapes
}
