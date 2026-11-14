//! Cosmetic billboard particles for gadgets: dust shed by a working ring
//! cutter, sparks off its blade, blast puffs and flashes. Presentation
//! only (no colliders, never read by the simulation). Capped; the oldest
//! go first.
//!
//! Anything may write a [`Burst`]; particles spawn on the next frame.

use std::collections::VecDeque;

use bevy::{
    asset::RenderAssetUsages,
    light::NotShadowCaster,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use super::remote_view::view_camera;

pub struct ParticlesPlugin;

impl Plugin for ParticlesPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Burst>()
            .init_resource::<ParticlePool>()
            .add_systems(Startup, create_particle_assets)
            .add_systems(Update, (spawn_bursts, animate_particles).chain());
    }
}

/// Most particles alive at once.
const MAX_PARTICLES: usize = 360;
/// Alpha steps per kind (each a shared material).
const FADE_STEPS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParticleKind {
    /// Pale plaster / stone dust.
    Dust,
    /// Dark blast smoke.
    Soot,
    /// Bright orange sparks (additive).
    Spark,
    /// A brief white-hot flash (additive).
    Flash,
}

impl ParticleKind {
    const ALL: [ParticleKind; 4] = [Self::Dust, Self::Soot, Self::Spark, Self::Flash];

    fn index(self) -> usize {
        self as usize
    }
}

/// Spawn `count` particles of `kind` around `origin`.
#[derive(Message, Clone, Debug)]
pub struct Burst {
    pub kind: ParticleKind,
    pub origin: Vec3,
    /// Main direction of travel (zero = all around).
    pub direction: Vec3,
    pub speed: f32,
    /// 0 = exactly along `direction`, 1 = anywhere in the hemisphere.
    pub spread: f32,
    pub count: u32,
    /// Start and end size, metres.
    pub size: (f32, f32),
    pub life: f32,
    /// Random start offset radius, metres.
    pub jitter: f32,
}

impl Burst {
    pub fn new(kind: ParticleKind, origin: Vec3) -> Self {
        let (size, life, speed) = match kind {
            ParticleKind::Dust => ((0.25, 0.9), 1.6, 0.6),
            ParticleKind::Soot => ((0.5, 2.2), 2.2, 1.2),
            ParticleKind::Spark => ((0.03, 0.01), 0.35, 4.0),
            ParticleKind::Flash => ((0.6, 2.0), 0.12, 0.0),
        };
        Self {
            kind,
            origin,
            direction: Vec3::ZERO,
            speed,
            spread: 1.0,
            count: 6,
            size,
            life,
            jitter: 0.05,
        }
    }

    pub fn toward(mut self, direction: Vec3, spread: f32) -> Self {
        self.direction = direction.normalize_or_zero();
        self.spread = spread;
        self
    }

    pub fn count(mut self, count: u32) -> Self {
        self.count = count;
        self
    }

    pub fn speed(mut self, speed: f32) -> Self {
        self.speed = speed;
        self
    }

    pub fn size(mut self, start: f32, end: f32) -> Self {
        self.size = (start, end);
        self
    }

    pub fn life(mut self, life: f32) -> Self {
        self.life = life;
        self
    }

    pub fn jitter(mut self, jitter: f32) -> Self {
        self.jitter = jitter;
        self
    }
}

#[derive(Resource)]
struct ParticleAssets {
    quad: Handle<Mesh>,
    /// [kind][fade step]
    materials: Vec<Vec<Handle<StandardMaterial>>>,
}

#[derive(Resource)]
struct ParticlePool {
    alive: VecDeque<Entity>,
    seed: u32,
}

impl Default for ParticlePool {
    fn default() -> Self {
        Self {
            alive: VecDeque::new(),
            seed: 0x9E37_79B9,
        }
    }
}

impl ParticlePool {
    /// Cheap repeatable random number in 0..1.
    fn next(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed as f32 / u32::MAX as f32
    }

    fn unit_vector(&mut self) -> Vec3 {
        let z = self.next() * 2.0 - 1.0;
        let a = self.next() * std::f32::consts::TAU;
        let r = (1.0 - z * z).max(0.0).sqrt();
        Vec3::new(r * a.cos(), z, r * a.sin())
    }
}

#[derive(Component)]
struct Particle {
    kind: ParticleKind,
    velocity: Vec3,
    age: f32,
    life: f32,
    size: (f32, f32),
    step: usize,
}

/// Soft round puff texture (white, alpha falls off to the edge).
pub(crate) fn puff_texture(images: &mut Assets<Image>, size: u32, hardness: f32) -> Handle<Image> {
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let d = Vec2::new(x as f32 + 0.5, y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let wobble = 0.82 + 0.18 * ((x as f32 * 0.43).sin() * (y as f32 * 0.57).cos());
            let a = (1.0 - d.length() / wobble).clamp(0.0, 1.0).powf(hardness);
            data.extend([255, 255, 255, (a * 255.0) as u8]);
        }
    }
    images.add(Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    ))
}

fn create_particle_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let soft = puff_texture(&mut images, 64, 1.5);
    let hard = puff_texture(&mut images, 32, 0.6);
    let materials = ParticleKind::ALL
        .iter()
        .map(|kind| {
            (0..FADE_STEPS)
                .map(|step| {
                    let fade = 1.0 - step as f32 / FADE_STEPS as f32;
                    let (color, texture, alpha_mode, unlit, emissive) = match kind {
                        ParticleKind::Dust => (
                            Color::srgba(0.82, 0.76, 0.66, 0.5 * fade),
                            soft.clone(),
                            AlphaMode::Blend,
                            false,
                            LinearRgba::BLACK,
                        ),
                        ParticleKind::Soot => (
                            Color::srgba(0.18, 0.17, 0.16, 0.6 * fade),
                            soft.clone(),
                            AlphaMode::Blend,
                            false,
                            LinearRgba::BLACK,
                        ),
                        ParticleKind::Spark => (
                            Color::srgba(1.0, 0.6, 0.2, fade),
                            hard.clone(),
                            AlphaMode::Add,
                            true,
                            LinearRgba::rgb(8.0, 3.0, 0.5) * fade,
                        ),
                        ParticleKind::Flash => (
                            Color::srgba(1.0, 0.95, 0.85, fade),
                            soft.clone(),
                            AlphaMode::Add,
                            true,
                            LinearRgba::rgb(20.0, 18.0, 14.0) * fade,
                        ),
                    };
                    materials.add(StandardMaterial {
                        base_color: color,
                        base_color_texture: Some(texture),
                        alpha_mode,
                        unlit,
                        emissive,
                        perceptual_roughness: 1.0,
                        ..default()
                    })
                })
                .collect()
        })
        .collect();
    commands.insert_resource(ParticleAssets {
        quad: meshes.add(Rectangle::new(1.0, 1.0)),
        materials,
    });
}

fn spawn_bursts(
    mut bursts: MessageReader<Burst>,
    assets: Option<Res<ParticleAssets>>,
    mut pool: ResMut<ParticlePool>,
    mut commands: Commands,
) {
    let Some(assets) = assets else { return };
    for burst in bursts.read() {
        for _ in 0..burst.count {
            let random = pool.unit_vector();
            let direction = if burst.direction == Vec3::ZERO {
                random
            } else {
                let mut d = burst.direction.lerp(random, burst.spread);
                if d.dot(burst.direction) < 0.0 {
                    d -= burst.direction * 2.0 * d.dot(burst.direction);
                }
                d.normalize_or(burst.direction)
            };
            let speed = burst.speed * (0.5 + pool.next());
            let start = burst.origin + pool.unit_vector() * burst.jitter * pool.next();
            let life = burst.life * (0.7 + 0.6 * pool.next());
            let entity = commands
                .spawn((
                    Particle {
                        kind: burst.kind,
                        velocity: direction * speed,
                        age: 0.0,
                        life,
                        size: burst.size,
                        step: 0,
                    },
                    Mesh3d(assets.quad.clone()),
                    MeshMaterial3d(assets.materials[burst.kind.index()][0].clone()),
                    Transform::from_translation(start).with_scale(Vec3::splat(burst.size.0)),
                    NotShadowCaster,
                ))
                .id();
            pool.alive.push_back(entity);
            while pool.alive.len() > MAX_PARTICLES {
                if let Some(old) = pool.alive.pop_front() {
                    commands.entity(old).try_despawn();
                }
            }
        }
    }
}

fn animate_particles(
    mut particles: Query<(Entity, &mut Particle, &mut Transform, &mut MeshMaterial3d<StandardMaterial>)>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    assets: Option<Res<ParticleAssets>>,
    time: Res<Time>,
    mut pool: ResMut<ParticlePool>,
    mut commands: Commands,
) {
    let Some(assets) = assets else { return };
    let facing = view_camera(&cameras).map(|c| c.rotation()).unwrap_or_default();
    let dt = time.delta_secs();
    for (entity, mut p, mut transform, mut material) in &mut particles {
        p.age += dt;
        let f = p.age / p.life;
        if f >= 1.0 {
            commands.entity(entity).despawn();
            pool.alive.retain(|e| *e != entity);
            continue;
        }
        let (gravity, drag) = match p.kind {
            ParticleKind::Dust => (0.4, 1.8),
            ParticleKind::Soot => (-0.3, 1.5),
            ParticleKind::Spark => (9.0, 0.5),
            ParticleKind::Flash => (0.0, 0.0),
        };
        p.velocity.y -= gravity * dt;
        p.velocity *= 1.0 - (drag * dt).min(0.9);
        transform.translation += p.velocity * dt;
        let grow = 1.0 - (1.0 - f).powi(2);
        transform.scale = Vec3::splat(p.size.0 + (p.size.1 - p.size.0) * grow);
        transform.rotation = facing;
        let step = ((f * FADE_STEPS as f32) as usize).min(FADE_STEPS - 1);
        if step != p.step {
            p.step = step;
            material.0 = assets.materials[p.kind.index()][step].clone();
        }
    }
}
