//! Cosmetic debris and dust from [`SurfaceChanged`]: presentation only.
//!
//! - Chunks: small boxes in the surface's material, thrown along the impact
//!   (or out from a blast), falling under gravity. Each moving chunk casts
//!   one short ray per frame so it lands on floors and bounces off walls; it
//!   has no collider, so it never blocks anyone. Chunks rest, then shrink
//!   away after `debris.lifetime` seconds.
//! - Dust: camera-facing soft puffs that grow and fade.
//!
//! Both are capped (`debris.max_chunks`, `debris.max_dust`); the oldest go
//! first. Randomness here is cosmetic and not deterministic.

use std::collections::VecDeque;

use avian3d::prelude::*;
use bevy::{
    asset::RenderAssetUsages,
    light::NotShadowCaster,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use super::{SurfaceChanged, spawn::SurfaceMaterials, surface::SurfaceMaterial};
use crate::{config::Tuning, physics::GameLayer};

pub struct DebrisPlugin;

impl Plugin for DebrisPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DebrisPool>()
            .add_systems(Startup, setup)
            .add_systems(Update, (spawn_debris, move_chunks, animate_dust).chain());
    }
}

/// Fade levels for dust (one material each, so puffs can fade without a
/// material per puff).
const FADE_STEPS: usize = 8;
const GRAVITY: f32 = 9.8;

#[derive(Resource)]
struct DebrisAssets {
    chunk: Handle<Mesh>,
    puff: Handle<Mesh>,
    /// Per surface material, per fade step (most opaque first).
    dust: Vec<Vec<Handle<StandardMaterial>>>,
}

#[derive(Resource, Default)]
struct DebrisPool {
    chunks: VecDeque<Entity>,
    dust: VecDeque<Entity>,
    seed: u32,
}

impl DebrisPool {
    /// Cheap cosmetic random number, 0..1.
    fn random(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed >> 8) as f32 / (1u32 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.random()
    }

    fn direction(&mut self) -> Vec3 {
        Vec3::new(self.range(-1.0, 1.0), self.range(-1.0, 1.0), self.range(-1.0, 1.0))
            .normalize_or(Vec3::Y)
    }
}

#[derive(Component)]
struct Chunk {
    velocity: Vec3,
    spin: Vec3,
    age: f32,
    resting: bool,
    half_height: f32,
}

#[derive(Component)]
struct Dust {
    velocity: Vec3,
    age: f32,
    life: f32,
    size: (f32, f32),
    material: usize,
    step: usize,
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut pool: ResMut<DebrisPool>,
) {
    pool.seed = 0x1234_5679;
    // Soft round puff texture.
    let size = 64u32;
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let d = Vec2::new(x as f32 + 0.5, y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let wobble = 0.85 + 0.15 * ((x as f32 * 0.7).sin() * (y as f32 * 0.9).cos());
            let a = (1.0 - d.length() / wobble).clamp(0.0, 1.0).powf(1.6);
            data.extend([255, 255, 255, (a * 255.0) as u8]);
        }
    }
    let texture = images.add(Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    ));
    let colours = [
        Color::srgb(0.86, 0.80, 0.70), // plaster
        Color::srgb(0.55, 0.47, 0.37), // wood
        Color::srgb(0.45, 0.45, 0.45), // metal
    ];
    let dust = colours
        .iter()
        .map(|colour| {
            (0..FADE_STEPS)
                .map(|step| {
                    let alpha = 0.55 * (1.0 - step as f32 / FADE_STEPS as f32);
                    materials.add(StandardMaterial {
                        base_color: colour.with_alpha(alpha),
                        base_color_texture: Some(texture.clone()),
                        alpha_mode: AlphaMode::Blend,
                        perceptual_roughness: 1.0,
                        ..default()
                    })
                })
                .collect()
        })
        .collect();
    commands.insert_resource(DebrisAssets {
        chunk: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        puff: meshes.add(Rectangle::new(1.0, 1.0)),
        dust,
    });
}

fn spawn_debris(
    mut changes: MessageReader<SurfaceChanged>,
    assets: Option<Res<DebrisAssets>>,
    surface_materials: Res<SurfaceMaterials>,
    mut pool: ResMut<DebrisPool>,
    tuning: Res<Tuning>,
    mut commands: Commands,
) {
    let Some(assets) = assets else { return };
    let t = &tuning.destruction.debris;
    for change in changes.read() {
        if change.removed.is_empty() {
            continue;
        }
        let Some(material) = surface_materials.get(change.material) else {
            continue;
        };
        let count = if change.violent {
            t.chunks_per_impact
        } else {
            change.removed.len().min(3)
        };
        let step = (change.removed.len() as f32 / count.max(1) as f32).max(1.0);
        let wood = change.material == SurfaceMaterial::Wood;
        for k in 0..count.min(change.removed.len()) {
            let at = change.removed[((k as f32 * step) as usize).min(change.removed.len() - 1)];
            let outward = if change.direction == Vec3::ZERO {
                (at - change.origin).normalize_or(Vec3::Y)
            } else {
                change.direction
            };
            let speed = if change.violent { pool.range(3.0, 7.0) } else { pool.range(1.0, 3.0) };
            let velocity = outward * speed
                + pool.direction() * speed * 0.35
                + Vec3::Y * pool.range(0.5, 2.0);
            let s = change.cell_size * pool.range(0.35, 0.85);
            let scale = if wood {
                Vec3::new(s * 2.5, s * 0.35, s * 0.6)
            } else {
                Vec3::new(s, s * 0.6, s * 0.8)
            };
            let spin = pool.direction() * pool.range(4.0, 14.0);
            let entity = commands
                .spawn((
                    Name::new("Debris"),
                    Mesh3d(assets.chunk.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(at).with_scale(scale),
                    NotShadowCaster,
                    Chunk {
                        velocity,
                        spin,
                        age: 0.0,
                        resting: false,
                        half_height: scale.y / 2.0,
                    },
                ))
                .id();
            pool.chunks.push_back(entity);
        }

        let puffs = if change.violent { 6 } else { 1 };
        let index = change.material.index();
        for k in 0..puffs {
            let at = change.removed[(k * change.removed.len() / puffs).min(change.removed.len() - 1)];
            let (start, end) = if change.violent {
                (pool.range(0.3, 0.6), pool.range(1.2, 2.2))
            } else {
                (0.08, pool.range(0.3, 0.5))
            };
            let drift = if change.direction == Vec3::ZERO {
                (at - change.origin).normalize_or(Vec3::Y) * 0.8
            } else {
                change.direction * 0.4
            };
            let entity = commands
                .spawn((
                    Name::new("Dust"),
                    Mesh3d(assets.puff.clone()),
                    MeshMaterial3d(assets.dust[index][0].clone()),
                    Transform::from_translation(at).with_scale(Vec3::splat(start)),
                    NotShadowCaster,
                    Dust {
                        velocity: drift + pool.direction() * 0.25 + Vec3::Y * 0.15,
                        age: 0.0,
                        life: if change.violent { pool.range(1.6, 2.6) } else { 0.9 },
                        size: (start, end),
                        material: index,
                        step: 0,
                    },
                ))
                .id();
            pool.dust.push_back(entity);
        }
    }
    // Caps: drop the oldest.
    while pool.chunks.len() > t.max_chunks {
        if let Some(old) = pool.chunks.pop_front() {
            commands.entity(old).try_despawn();
        }
    }
    while pool.dust.len() > t.max_dust {
        if let Some(old) = pool.dust.pop_front() {
            commands.entity(old).try_despawn();
        }
    }
}

fn move_chunks(
    mut chunks: Query<(Entity, &mut Chunk, &mut Transform)>,
    spatial: SpatialQuery,
    tuning: Res<Tuning>,
    time: Res<Time>,
    mut pool: ResMut<DebrisPool>,
    mut commands: Commands,
) {
    let dt = time.delta_secs().min(0.05);
    let lifetime = tuning.destruction.debris.lifetime;
    let world = SpatialQueryFilter::from_mask(GameLayer::World);
    for (entity, mut chunk, mut transform) in &mut chunks {
        chunk.age += dt;
        if !chunk.resting {
            chunk.velocity.y -= GRAVITY * dt;
            let step = chunk.velocity * dt;
            let spin = chunk.spin * dt;
            transform.rotate(Quat::from_scaled_axis(spin));
            match Dir3::new(step) {
                Ok(dir) => {
                    let length = step.length();
                    if let Some(hit) = spatial.cast_ray(transform.translation, dir, length, true, &world) {
                        if hit.normal.y > 0.6 || hit.normal == Vec3::ZERO {
                            transform.translation += *dir * hit.distance + Vec3::Y * chunk.half_height;
                            chunk.resting = true;
                        } else {
                            transform.translation += *dir * hit.distance + hit.normal * 0.01;
                            chunk.velocity = chunk.velocity.reflect(hit.normal) * 0.35;
                        }
                    } else {
                        transform.translation += step;
                    }
                }
                Err(_) => chunk.resting = true,
            }
            if chunk.age > 4.0 || transform.translation.y < -20.0 {
                chunk.resting = true;
            }
        }
        if chunk.age > lifetime {
            let fade = 1.0 - (chunk.age - lifetime) / 0.6;
            if fade <= 0.0 {
                commands.entity(entity).despawn();
                pool.chunks.retain(|e| *e != entity);
            } else {
                transform.scale *= 1.0 - (1.0 - fade.max(0.05)) * dt * 6.0;
            }
        }
    }
}

fn animate_dust(
    mut dust: Query<(Entity, &mut Dust, &mut Transform, &mut MeshMaterial3d<StandardMaterial>)>,
    camera: Query<&GlobalTransform, (With<Camera3d>, Without<ChildOf>)>,
    assets: Option<Res<DebrisAssets>>,
    time: Res<Time>,
    mut pool: ResMut<DebrisPool>,
    mut commands: Commands,
) {
    let Some(assets) = assets else { return };
    let facing = camera.iter().next().map(|c| c.rotation()).unwrap_or_default();
    let dt = time.delta_secs();
    for (entity, mut puff, mut transform, mut material) in &mut dust {
        puff.age += dt;
        let f = puff.age / puff.life;
        if f >= 1.0 {
            commands.entity(entity).despawn();
            pool.dust.retain(|e| *e != entity);
            continue;
        }
        puff.velocity *= 1.0 - dt * 1.5;
        transform.translation += puff.velocity * dt;
        let grow = 1.0 - (1.0 - f).powi(3);
        transform.scale = Vec3::splat(puff.size.0 + (puff.size.1 - puff.size.0) * grow);
        transform.rotation = facing;
        let step = ((f * FADE_STEPS as f32) as usize).min(FADE_STEPS - 1);
        if step != puff.step {
            puff.step = step;
            material.0 = assets.dust[puff.material][step].clone();
        }
    }
}
