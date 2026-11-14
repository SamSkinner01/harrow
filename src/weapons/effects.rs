//! Cosmetic bullet impacts: a short dust puff and a few chips (plus a spark
//! on hard surfaces), and a red puff on character hits. Presentation only:
//! nothing permanent (no decals); destruction (M4) handles holes.
//!
//! Cheap by design: shared meshes and materials, a handful of entities per
//! impact for ~0.3 s, capped by `effects.max_impacts`.

use bevy::{light::NotShadowCaster, prelude::*};

use avian3d::prelude::ColliderOf;

use super::BulletHit;
use crate::{
    combat::{ImpactKind, SurfaceImpact, SurfaceKind},
    config::Tuning,
};

pub struct ImpactEffectsPlugin;

impl Plugin for ImpactEffectsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, create_assets)
            .add_systems(Update, (spawn_impacts, animate_particles).chain());
    }
}

#[derive(Resource)]
struct ImpactAssets {
    puff: Handle<Mesh>,
    chip: Handle<Mesh>,
    plaster_dust: Handle<StandardMaterial>,
    stone_dust: Handle<StandardMaterial>,
    chip_material: Handle<StandardMaterial>,
    spark: Handle<StandardMaterial>,
    blood: Handle<StandardMaterial>,
}

#[derive(Component)]
struct Particle {
    age: f32,
    life: f32,
    velocity: Vec3,
    gravity: f32,
    /// Puffs grow then shrink; chips keep their size.
    puff: bool,
    size: f32,
}

/// One impact = one puff; counts against `max_impacts`.
#[derive(Component)]
struct Puff;

fn create_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dust = |materials: &mut Assets<StandardMaterial>, r, g, b| {
        materials.add(StandardMaterial {
            base_color: Color::srgba(r, g, b, 0.55),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 1.0,
            ..default()
        })
    };
    let assets = ImpactAssets {
        puff: meshes.add(Sphere::new(1.0).mesh().ico(1).expect("ico sphere")),
        chip: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        plaster_dust: dust(&mut materials, 0.9, 0.86, 0.78),
        stone_dust: dust(&mut materials, 0.72, 0.62, 0.48),
        chip_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.55, 0.48, 0.38),
            perceptual_roughness: 1.0,
            ..default()
        }),
        spark: materials.add(StandardMaterial {
            base_color: Color::linear_rgb(6.0, 3.0, 0.8),
            unlit: true,
            ..default()
        }),
        blood: materials.add(StandardMaterial {
            base_color: Color::srgba(0.45, 0.02, 0.02, 0.8),
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
    };
    commands.insert_resource(assets);
}

/// Small deterministic noise for particle directions.
fn next_random(seed: &mut u32) -> f32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    *seed as f32 / u32::MAX as f32
}

#[allow(clippy::too_many_arguments)]
fn spawn_impacts(
    mut commands: Commands,
    mut impacts: MessageReader<SurfaceImpact>,
    mut hits: MessageReader<BulletHit>,
    colliders: Query<&ColliderOf>,
    surfaces: Query<&SurfaceKind>,
    puffs: Query<(), With<Puff>>,
    assets: Res<ImpactAssets>,
    tuning: Res<Tuning>,
    mut seed: Local<u32>,
) {
    if *seed == 0 {
        *seed = 0x2545_F491;
    }
    let fx = &tuning.weapons.effects;
    let mut live = puffs.iter().count();
    for impact in impacts.read() {
        if impact.kind != ImpactKind::Bullet || live >= fx.max_impacts {
            continue;
        }
        live += 1;
        let body = colliders
            .get(impact.collider)
            .map(|c| c.body)
            .unwrap_or(impact.collider);
        let soft = surfaces.get(body).is_ok_and(|k| *k == SurfaceKind::Soft);
        let normal = impact.normal.normalize_or(-impact.direction);
        // Bounce direction: the bullet's path mirrored off the surface.
        let bounce = (impact.direction - 2.0 * impact.direction.dot(normal) * normal).normalize_or(normal);
        let at = impact.point + normal * 0.02;

        commands.spawn((
            Puff,
            Particle {
                age: 0.0,
                life: fx.impact_time,
                velocity: normal * 0.6,
                gravity: -0.3,
                puff: true,
                size: if soft { 0.11 } else { 0.08 },
            },
            Mesh3d(assets.puff.clone()),
            MeshMaterial3d(if soft {
                assets.plaster_dust.clone()
            } else {
                assets.stone_dust.clone()
            }),
            Transform::from_translation(at).with_scale(Vec3::splat(0.01)),
            NotShadowCaster,
        ));
        let chips = if soft { 4 } else { 3 };
        for i in 0..chips + usize::from(!soft) {
            let spark = !soft && i == chips;
            let jitter = Vec3::new(
                next_random(&mut seed) - 0.5,
                next_random(&mut seed) - 0.5,
                next_random(&mut seed) - 0.5,
            );
            let dir = (normal * 0.7 + bounce * 0.5 + jitter * 0.9).normalize_or(normal);
            let speed = if spark { 7.0 } else { 1.5 + 2.0 * next_random(&mut seed) };
            let size = if spark { 0.012 } else { 0.012 + 0.012 * next_random(&mut seed) };
            commands.spawn((
                Particle {
                    age: 0.0,
                    life: if spark { 0.08 } else { fx.impact_time * 1.2 },
                    velocity: dir * speed,
                    gravity: if spark { 0.0 } else { 9.8 },
                    puff: false,
                    size,
                },
                Mesh3d(assets.chip.clone()),
                MeshMaterial3d(if spark {
                    assets.spark.clone()
                } else {
                    assets.chip_material.clone()
                }),
                Transform::from_translation(at)
                    .with_scale(if spark { Vec3::new(size, size, size * 6.0) } else { Vec3::splat(size) })
                    .looking_to(dir, Vec3::Y),
                NotShadowCaster,
            ));
        }
    }
    for hit in hits.read() {
        if live >= fx.max_impacts {
            break;
        }
        live += 1;
        commands.spawn((
            Puff,
            Particle {
                age: 0.0,
                life: fx.impact_time * 0.7,
                velocity: Vec3::Y * 0.2,
                gravity: 0.0,
                puff: true,
                size: 0.07,
            },
            Mesh3d(assets.puff.clone()),
            // Characters bleed; gadgets and other hit volumes throw dust.
            MeshMaterial3d(if hit.zone.is_some() {
                assets.blood.clone()
            } else {
                assets.stone_dust.clone()
            }),
            Transform::from_translation(hit.point).with_scale(Vec3::splat(0.01)),
            NotShadowCaster,
        ));
    }
}

fn animate_particles(
    mut commands: Commands,
    mut particles: Query<(Entity, &mut Particle, &mut Transform)>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    for (entity, mut p, mut transform) in &mut particles {
        p.age += dt;
        if p.age >= p.life {
            commands.entity(entity).despawn();
            continue;
        }
        let t = p.age / p.life;
        p.velocity.y -= p.gravity * dt;
        // Drag.
        p.velocity *= (-3.0 * dt).exp();
        transform.translation += p.velocity * dt;
        if p.puff {
            // Quick bloom, slow shrink.
            let grow = (t * 5.0).min(1.0);
            let fade = 1.0 - t * t;
            transform.scale = Vec3::splat(p.size * (0.4 + 0.8 * grow) * fade + 1e-4);
        }
    }
}
