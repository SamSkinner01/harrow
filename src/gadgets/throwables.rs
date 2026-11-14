//! Thrown secondary gadgets: frag grenade, flashbang, smoke grenade (Avian
//! rigid bodies that bounce around), and the impact charge (goes off on
//! the first thing it touches).
//!
//! - **Frag** (attackers): the fuse starts when the pin is pulled (4
//!   pressed); hold 4 to cook it. An [`Explosion`] with damage and a small
//!   breach radius: it opens small holes in soft walls and floors, never in
//!   reinforced ones (destruction ignores those).
//! - **Flashbang** (attackers): after its fuse, everyone with a clear line
//!   of sight within `radius` is blinded (white screen) and deafened
//!   (muffled audio) for a time that depends on distance and on how
//!   directly they were looking at it ([`Flashed`]).
//! - **Smoke grenade** (attackers): after its fuse it pours out a
//!   [`SmokeCloud`] of billboard smoke that blocks vision for `duration`
//!   seconds, then thins out. Bots can test sight lines with
//!   [`smoke_blocks`].
//! - **Impact charge** (defenders): thrown flat and fast; on contact an
//!   [`Explosion`] with a breach radius that opens soft walls, floors and
//!   hatches, and only a little damage.


use avian3d::prelude::*;
use bevy::{light::NotShadowCaster, prelude::*};

use super::{
    GadgetEvent, GadgetOwner, GadgetStage, RoundScoped, SecondaryGadget, eye_and_look,
    effects::{Flashed, LightFlash},
    particles::{Burst, ParticleKind, puff_texture},
    remote_view::view_camera,
    tuning::SmokeTuning,
    GadgetAssets,
};
use crate::{
    combat::{Dead, Noise, NoiseKind, Team},
    config::Tuning,
    destruction::Explosion,
    physics::GameLayer,
    player::{CharacterState, Player, PlayerInput},
};

pub struct ThrowablesPlugin;

impl Plugin for ThrowablesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (detonate_grenades, age_smoke).chain().in_set(GadgetStage::Simulate),
        )
        .add_systems(Startup, create_smoke_assets)
        .add_systems(Update, (dress_grenades, (spawn_puffs, animate_puffs).chain()));
    }
}

/// Collision layer bit for thrown objects: they bounce off the world but
/// never block characters, and bullets don't see them.
pub const PROJECTILE_LAYER: LayerMask = LayerMask(1 << 8);

/// A thrown grenade in flight or rolling around.
#[derive(Component, Clone, Debug)]
pub struct Grenade {
    pub kind: SecondaryGadget,
    pub owner: Entity,
    /// Seconds until it goes off (`None`: on contact).
    pub fuse: Option<f32>,
    pub age: f32,
}

/// A smoke screen. `centre` is about chest height above where the canister
/// landed.
#[derive(Component, Clone, Debug)]
pub struct SmokeCloud {
    pub centre: Vec3,
    pub age: f32,
}

impl SmokeCloud {
    /// Current radius, metres (grows over `grow_time`).
    pub fn radius(&self, t: &SmokeTuning) -> f32 {
        let f = (self.age / t.grow_time.max(0.01)).min(1.0);
        t.radius * (0.35 + 0.65 * (1.0 - (1.0 - f).powi(2)))
    }

    /// 0..1: how thick it is (1 while it lasts, thinning out at the end).
    pub fn density(&self, t: &SmokeTuning) -> f32 {
        let end = t.grow_time + t.duration;
        if self.age <= end {
            (self.age / 0.6).min(1.0)
        } else {
            (1.0 - (self.age - end) / t.fade_time.max(0.01)).max(0.0)
        }
    }

    fn finished(&self, t: &SmokeTuning) -> bool {
        self.age > t.grow_time + t.duration + t.fade_time
    }
}

/// Whether smoke hides `b` from `a` (for bots' sight checks and gadgets):
/// the segment passes through the thick part of a cloud.
pub fn smoke_blocks<'a>(a: Vec3, b: Vec3, clouds: impl IntoIterator<Item = &'a SmokeCloud>, t: &SmokeTuning) -> bool {
    clouds.into_iter().any(|cloud| {
        if cloud.density(t) < 0.5 {
            return false;
        }
        let ab = b - a;
        let s = ((cloud.centre - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
        let closest = a + ab * s;
        // The cloud is squashed vertically.
        let d = (closest - cloud.centre) * Vec3::new(1.0, 1.6, 1.0);
        d.length() < cloud.radius(t) * 0.8
    })
}

/// Throw (or drop, with `fuse = Some(0)`) a grenade of `kind` from a
/// character's hand. `fuse` overrides the kind's own fuse (a cooked frag).
#[allow(clippy::too_many_arguments)]
pub fn spawn_grenade(
    commands: &mut Commands,
    owner: Entity,
    team: Team,
    kind: SecondaryGadget,
    transform: &Transform,
    state: &CharacterState,
    input: &PlayerInput,
    tuning: &Tuning,
    fuse: Option<f32>,
) -> Entity {
    let g = &tuning.round.gadgets;
    let (eye, look) = eye_and_look(transform, state, input, &tuning.body, tuning.lean.offset);
    let right = Quat::from_rotation_y(input.yaw) * Vec3::X;
    let hand = eye + right * 0.18 - Vec3::Y * 0.12 + *look * 0.3;
    let (speed, lift) = match kind {
        SecondaryGadget::ImpactCharge => (g.impact.speed, g.throw.lift * 0.5),
        _ => (g.throw.speed, g.throw.lift),
    };
    let dropped = fuse == Some(0.0);
    let velocity = if dropped { Vec3::ZERO } else { *look * speed + Vec3::Y * lift };
    let fuse = match kind {
        SecondaryGadget::Frag => fuse.or(Some(g.frag.fuse)),
        SecondaryGadget::Flashbang => fuse.or(Some(g.flashbang.fuse)),
        SecondaryGadget::Smoke => fuse.or(Some(g.smoke.fuse)),
        _ => fuse,
    };
    let r = g.throw.radius;
    commands
        .spawn((
            Name::new(kind.name()),
            Grenade {
                kind,
                owner,
                fuse,
                age: 0.0,
            },
            GadgetOwner {
                entity: Some(owner),
                team,
            },
            RoundScoped,
            Transform::from_translation(if dropped { transform.translation + Vec3::Y * 0.3 } else { hand }),
            Visibility::default(),
            RigidBody::Dynamic,
            Collider::sphere(r),
            Mass(0.4),
            Restitution::new(g.throw.restitution).with_combine_rule(CoefficientCombine::Max),
            Friction::new(g.throw.friction),
            LinearVelocity(velocity),
            AngularVelocity(right * 12.0),
            (
                LinearDamping(0.05),
                AngularDamping(1.5),
                SweptCcd::default(),
                CollisionLayers::new(PROJECTILE_LAYER, GameLayer::World),
                TransformInterpolation,
            ),
        ))
        .id()
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn detonate_grenades(
    tuning: Res<Tuning>,
    time: Res<Time>,
    spatial: SpatialQuery,
    mut grenades: Query<(Entity, &mut Grenade, &Transform, &LinearVelocity)>,
    mut people: Query<(Entity, &Transform, &CharacterState, &PlayerInput, Option<&mut Flashed>), (With<Player>, Without<Dead>, Without<Grenade>)>,
    clouds: Query<&SmokeCloud>,
    mut explosions: MessageWriter<Explosion>,
    mut noise: MessageWriter<Noise>,
    mut events: MessageWriter<GadgetEvent>,
    mut bursts: MessageWriter<Burst>,
    mut commands: Commands,
) {
    let g = &tuning.round.gadgets;
    let dt = time.delta_secs();
    let world = SpatialQueryFilter::from_mask(GameLayer::World);
    for (entity, mut grenade, transform, velocity) in &mut grenades {
        grenade.age += dt;
        let mut at = transform.translation;
        let go = match grenade.fuse.as_mut() {
            Some(fuse) => {
                *fuse -= dt;
                *fuse <= 0.0
            }
            None => {
                // Impact charge: look ahead one tick for a contact.
                let step = velocity.0 * dt;
                let hit = Dir3::new(step).ok().and_then(|dir| {
                    spatial.cast_shape(
                        &Collider::sphere(g.throw.radius * 1.5),
                        at,
                        Quat::IDENTITY,
                        dir,
                        &ShapeCastConfig::from_max_distance(step.length() + 0.02),
                        &world,
                    )
                });
                if let Some(hit) = hit {
                    at = hit.point1 + hit.normal1 * 0.05;
                    true
                } else if grenade.age > g.impact.max_flight {
                    commands.entity(entity).despawn();
                    continue;
                } else {
                    false
                }
            }
        };
        if !go {
            continue;
        }
        commands.entity(entity).despawn();
        events.write(GadgetEvent::Detonated { kind: grenade.kind, position: at });
        let source = Some(grenade.owner);
        match grenade.kind {
            SecondaryGadget::Frag => {
                explosions.write(Explosion {
                    centre: at + Vec3::Y * 0.05,
                    breach_radius: g.frag.breach_radius,
                    damage_radius: g.frag.damage_radius,
                    max_damage: g.frag.max_damage,
                    source,
                    attached_to: None,
                });
                blast_look(&mut bursts, &mut commands, at, 1.0);
            }
            SecondaryGadget::ImpactCharge => {
                explosions.write(Explosion {
                    centre: at,
                    breach_radius: g.impact.breach_radius,
                    damage_radius: g.impact.damage_radius,
                    max_damage: g.impact.max_damage,
                    source,
                    attached_to: None,
                });
                blast_look(&mut bursts, &mut commands, at, 0.6);
            }
            SecondaryGadget::Flashbang => {
                let t = &g.flashbang;
                noise.write(Noise {
                    source,
                    position: at,
                    loudness: t.loudness,
                    kind: NoiseKind::Explosion,
                });
                bursts.write(Burst::new(ParticleKind::Flash, at + Vec3::Y * 0.1).count(3).size(0.8, 3.0).life(0.18));
                commands.spawn((LightFlash::new(3.0e8, 0.25), Transform::from_translation(at + Vec3::Y * 0.2)));
                let bang = at + Vec3::Y * 0.08;
                for (person, feet, state, input, flashed) in &mut people {
                    let (eye, look) = eye_and_look(feet, state, input, &tuning.body, tuning.lean.offset);
                    let to = eye - bang;
                    let distance = to.length();
                    if distance > t.radius {
                        continue;
                    }
                    let blocked = Dir3::new(to)
                        .ok()
                        .and_then(|dir| spatial.cast_ray(bang, dir, distance - 0.05, true, &world))
                        .is_some();
                    if blocked {
                        continue;
                    }
                    let near = 1.0 - (distance / t.radius).powi(2);
                    let facing = ((-to.normalize_or_zero()).dot(*look) + 1.0) / 2.0;
                    let mut strength = near * (t.look_away + (1.0 - t.look_away) * facing * facing);
                    if smoke_blocks(bang, eye, clouds.iter(), &g.smoke) {
                        strength *= 0.3;
                    }
                    let fresh = Flashed {
                        blind: t.blind_time * strength,
                        blind_max: t.blind_time * strength,
                        deaf: t.deaf_time * near,
                        deaf_max: t.deaf_time * near,
                    };
                    match flashed {
                        Some(mut f) => f.merge(fresh),
                        None => {
                            commands.entity(person).insert(fresh);
                        }
                    }
                    events.write(GadgetEvent::Flashed { entity: person, strength });
                }
            }
            SecondaryGadget::Smoke => {
                noise.write(Noise {
                    source,
                    position: at,
                    loudness: 0.5,
                    kind: NoiseKind::Gadget,
                });
                // Sit the cloud on the floor below the canister.
                let floor = spatial
                    .cast_ray(at + Vec3::Y * 0.1, Dir3::NEG_Y, 3.0, true, &world)
                    .map(|hit| at.y + 0.1 - hit.distance)
                    .unwrap_or(at.y);
                commands.spawn((
                    Name::new("Smoke cloud"),
                    SmokeCloud {
                        centre: Vec3::new(at.x, floor + 1.1, at.z),
                        age: 0.0,
                    },
                    RoundScoped,
                    Transform::from_translation(Vec3::new(at.x, floor + 1.1, at.z)),
                    Visibility::default(),
                ));
            }
            _ => {}
        }
    }
}

/// Flash, smoke, dust and sparks for a blast of relative `size`.
fn blast_look(bursts: &mut MessageWriter<Burst>, commands: &mut Commands, at: Vec3, size: f32) {
    bursts.write(Burst::new(ParticleKind::Flash, at + Vec3::Y * 0.15).count(2).size(0.6 * size, 2.4 * size));
    bursts.write(
        Burst::new(ParticleKind::Soot, at + Vec3::Y * 0.3)
            .toward(Vec3::Y, 0.9)
            .count((10.0 * size) as u32)
            .speed(2.5 * size)
            .size(0.5 * size, 2.0 * size)
            .jitter(0.3),
    );
    bursts.write(
        Burst::new(ParticleKind::Dust, at + Vec3::Y * 0.1)
            .count((8.0 * size) as u32)
            .speed(3.0 * size)
            .size(0.3, 1.5 * size)
            .jitter(0.2),
    );
    bursts.write(Burst::new(ParticleKind::Spark, at + Vec3::Y * 0.1).count((16.0 * size) as u32).speed(9.0));
    commands.spawn((LightFlash::new(1.5e8 * size, 0.18), Transform::from_translation(at + Vec3::Y * 0.4)));
}

fn age_smoke(
    tuning: Res<Tuning>,
    time: Res<Time>,
    mut clouds: Query<(Entity, &mut SmokeCloud)>,
    mut commands: Commands,
) {
    let t = &tuning.round.gadgets.smoke;
    for (entity, mut cloud) in &mut clouds {
        cloud.age += time.delta_secs();
        if cloud.finished(t) {
            commands.entity(entity).despawn();
        }
    }
}

// ---- Presentation ---------------------------------------------------------

/// Grenade bodies.
fn dress_grenades(
    new: Query<(Entity, &Grenade), Added<Grenade>>,
    assets: Res<GadgetAssets>,
    tuning: Res<Tuning>,
    mut commands: Commands,
) {
    let a = &*assets;
    let r = tuning.round.gadgets.throw.radius;
    for (entity, grenade) in &new {
        commands.entity(entity).with_children(|c| match grenade.kind {
            SecondaryGadget::Frag => {
                c.spawn((
                    Mesh3d(a.unit_sphere.clone()),
                    MeshMaterial3d(a.olive.clone()),
                    Transform::from_scale(Vec3::new(2.0, 2.3, 2.0) * r),
                ));
                c.spawn(super::part(a, &a.steel, Vec3::Y * r * 1.2, Vec3::new(0.6, 0.6, 0.6) * r));
            }
            SecondaryGadget::Flashbang | SecondaryGadget::Smoke => {
                let material = if grenade.kind == SecondaryGadget::Smoke { &a.dark_metal } else { &a.steel };
                c.spawn((
                    Mesh3d(a.unit_cylinder.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_scale(Vec3::new(1.8 * r, 2.8 * r, 1.8 * r)),
                ));
                c.spawn(super::part(a, &a.rubber, Vec3::Y * r * 1.5, Vec3::new(0.7, 0.4, 0.7) * r));
            }
            _ => {
                // Impact charge: a squat puck with a red contact cap.
                c.spawn((
                    Mesh3d(a.unit_cylinder.clone()),
                    MeshMaterial3d(a.olive.clone()),
                    Transform::from_scale(Vec3::new(2.2 * r, 1.2 * r, 2.2 * r)),
                ));
                c.spawn((
                    Mesh3d(a.unit_sphere.clone()),
                    MeshMaterial3d(a.red_light.clone()),
                    Transform::from_xyz(0.0, 0.6 * r, 0.0).with_scale(Vec3::splat(0.8 * r)),
                ));
            }
        });
    }
}

/// Alpha steps for smoke puffs.
const SMOKE_STEPS: usize = 10;

#[derive(Resource)]
struct SmokeAssets {
    quad: Handle<Mesh>,
    materials: Vec<Handle<StandardMaterial>>,
}

fn create_smoke_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let texture = puff_texture(&mut images, 64, 0.75);
    let materials = (0..SMOKE_STEPS)
        .map(|step| {
            let alpha = 0.88 * (step + 1) as f32 / SMOKE_STEPS as f32;
            materials.add(StandardMaterial {
                base_color: Color::srgba(0.66, 0.66, 0.64, alpha),
                base_color_texture: Some(texture.clone()),
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 1.0,
                ..default()
            })
        })
        .collect();
    commands.insert_resource(SmokeAssets {
        quad: meshes.add(Rectangle::new(1.0, 1.0)),
        materials,
    });
}

/// One billboard of a smoke cloud.
#[derive(Component)]
struct SmokePuff {
    cloud: Entity,
    /// Offset from the cloud centre at full size (already pulled in from walls).
    offset: Vec3,
    size: f32,
    phase: f32,
    step: usize,
}

/// Puffs already spawned per cloud.
#[derive(Component, Default)]
struct PuffCount(usize);

/// Clouds pour out puffs while they grow, up to the per-cloud and total caps.
#[allow(clippy::too_many_arguments)]
fn spawn_puffs(
    mut clouds: Query<(Entity, &SmokeCloud, Option<&mut PuffCount>)>,
    puffs: Query<(), With<SmokePuff>>,
    assets: Option<Res<SmokeAssets>>,
    tuning: Res<Tuning>,
    spatial: SpatialQuery,
    mut commands: Commands,
) {
    let Some(assets) = assets else { return };
    let t = &tuning.round.gadgets.smoke;
    let mut total = puffs.iter().count();
    let world = SpatialQueryFilter::from_mask(GameLayer::World);
    for (entity, cloud, count) in &mut clouds {
        let Some(mut count) = count else {
            commands.entity(entity).insert(PuffCount::default());
            continue;
        };
        let f = (cloud.age / t.grow_time.max(0.01)).min(1.0);
        let wanted = ((t.particles as f32) * (0.25 + 0.75 * f)) as usize;
        while count.0 < wanted.min(t.particles) && total < t.max_particles {
            let i = count.0 as u32;
            count.0 += 1;
            total += 1;
            // Golden-angle spiral on a squashed sphere, so puffs fill the cloud evenly.
            let k = (i as f32 + 0.5) / t.particles as f32;
            let theta = i as f32 * 2.399_963;
            let up = 1.0 - 2.0 * k;
            let ring = (1.0 - up * up).max(0.0).sqrt();
            let reach = t.radius * (0.2 + 0.8 * ((i * 7919 % 97) as f32 / 97.0).powf(0.7));
            let mut offset = Vec3::new(ring * theta.cos(), up * 0.42 + 0.1, ring * theta.sin()) * reach;
            let size = 1.9 + 1.3 * ((i * 104_729 % 89) as f32 / 89.0);
            // Keep puffs on this side of walls and floors.
            if let Ok(dir) = Dir3::new(offset)
                && let Some(hit) = spatial.cast_ray(cloud.centre, dir, offset.length() + size * 0.4, true, &world)
            {
                offset = *dir * (hit.distance - size * 0.4).max(0.0);
            }
            commands.spawn((
                SmokePuff {
                    cloud: entity,
                    offset,
                    size,
                    phase: theta,
                    step: 0,
                },
                Mesh3d(assets.quad.clone()),
                MeshMaterial3d(assets.materials[0].clone()),
                Transform::from_translation(cloud.centre).with_scale(Vec3::splat(0.1)),
                NotShadowCaster,
            ));
        }
    }
}

fn animate_puffs(
    mut puffs: Query<(Entity, &mut SmokePuff, &mut Transform, &mut MeshMaterial3d<StandardMaterial>)>,
    clouds: Query<&SmokeCloud>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    assets: Option<Res<SmokeAssets>>,
    tuning: Res<Tuning>,
    time: Res<Time>,
    mut commands: Commands,
) {
    let Some(assets) = assets else { return };
    let t = &tuning.round.gadgets.smoke;
    let facing = view_camera(&cameras).map(|c| c.rotation()).unwrap_or_default();
    let now = time.elapsed_secs();
    for (entity, mut puff, mut transform, mut material) in &mut puffs {
        let Ok(cloud) = clouds.get(puff.cloud) else {
            commands.entity(entity).despawn();
            continue;
        };
        let grow = cloud.radius(t) / t.radius.max(0.01);
        let drift = Vec3::new((now * 0.21 + puff.phase).sin(), (now * 0.13 + puff.phase).cos() * 0.4, (now * 0.17 + puff.phase * 1.3).cos()) * 0.25;
        transform.translation = cloud.centre + puff.offset * grow + drift;
        transform.scale = Vec3::splat(puff.size * (0.55 + 0.45 * grow));
        transform.rotation = facing * Quat::from_rotation_z(puff.phase + now * 0.05 * (puff.phase.sin()));
        let density = cloud.density(t);
        let step = ((density * SMOKE_STEPS as f32).ceil() as usize).clamp(1, SMOKE_STEPS) - 1;
        if step != puff.step || density <= 0.0 {
            puff.step = step;
            material.0 = assets.materials[step].clone();
        }
    }
}
