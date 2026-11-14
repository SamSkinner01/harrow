//! Recon throwbots (attackers, `drone.count` each; DESIGN.md "Operator
//! structure").
//!
//! A small two-wheeled robot. Thrown with G (it tumbles and bounces until
//! it settles), then driven through its camera: W/S drive, A/D and the
//! mouse turn, the mouse tilts the camera. It can't jump; it climbs ledges
//! up to `step_height` and ramps up to `max_slope`, and its body is a
//! sphere of `radius` (fits through holes a little bigger than that). Its
//! battery runs down while driving. Defenders can see and shoot it
//! (`Health`, hit sensor on `GameLayer::Hitbox`).
//!
//! Driving is simulation on the fixed tick from the owner's
//! [`DroneIntent`]; which drone a player is looking through is
//! [`RemoteView`] (see `remote_view.rs`, which also handles G).

use avian3d::prelude::*;
use bevy::prelude::*;

use super::{
    DeployedGadget, DroneIntent, GadgetAssets, GadgetOwner, GadgetStage, RoundScoped,
    remote_view::RemoteView,
};
use crate::{
    combat::{Health, Noise, NoiseKind, Team},
    config::Tuning,
    physics::GameLayer,
    round::RoundReset,
};

pub struct DronePlugin;

impl Plugin for DronePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (
                refill_drones.in_set(GadgetStage::Input),
                drive_drones.in_set(GadgetStage::Simulate),
            ),
        )
        .add_systems(Update, (dress_drones, spin_wheels));
    }
}

/// Recon drones a character has left to throw this round.
#[derive(Component, Clone, Copy, Debug)]
pub struct ReconStock {
    pub drones: u8,
    pub max: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DroneState {
    /// Thrown, tumbling; `age` seconds in the air.
    Thrown { age: f32 },
    /// On its wheels.
    Driving,
}

/// A recon drone. Its `Transform` is the centre of its body sphere
/// (`radius` above the floor when it stands).
#[derive(Component, Clone, Debug)]
pub struct Drone {
    pub owner: Entity,
    /// 1-based number among the owner's drones (HUD).
    pub number: u8,
    pub state: DroneState,
    /// Heading (radians, 0 faces -Z) and camera tilt.
    pub yaw: f32,
    pub pitch: f32,
    pub velocity: Vec3,
    pub grounded: bool,
    /// Seconds of driving left.
    pub battery: f32,
    pub noise_timer: f32,
    /// Increasing per throw: the most recent drone is the highest.
    pub serial: u32,
}

fn refill_drones(
    mut resets: MessageReader<RoundReset>,
    tuning: Res<Tuning>,
    mut stocks: Query<&mut ReconStock>,
) {
    if resets.read().last().is_none() {
        return;
    }
    let n = tuning.round.gadgets.drone.count;
    for mut stock in &mut stocks {
        *stock = ReconStock { drones: n, max: n };
    }
}

/// Throw a drone from `eye` along `look` for `owner`.
#[allow(clippy::too_many_arguments)]
pub fn spawn_drone(
    commands: &mut Commands,
    owner: Entity,
    team: Team,
    number: u8,
    serial: u32,
    eye: Vec3,
    look: Vec3,
    yaw: f32,
    tuning: &Tuning,
) -> Entity {
    let d = &tuning.round.gadgets.drone;
    let start = eye + look * 0.35 - Vec3::Y * 0.25;
    let flat = look.with_y(0.0).normalize_or(Vec3::NEG_Z);
    let velocity = (flat * 0.9 + Vec3::Y * 0.25).normalize() * d.throw_speed;
    commands
        .spawn((
            Name::new(format!("Recon drone {number}")),
            Drone {
                owner,
                number,
                state: DroneState::Thrown { age: 0.0 },
                yaw,
                pitch: 0.0,
                velocity,
                grounded: false,
                battery: d.battery,
                noise_timer: 0.0,
                serial,
            },
            GadgetOwner {
                entity: Some(owner),
                team,
            },
            DeployedGadget,
            RoundScoped,
            Health::new(d.health),
            Transform::from_translation(start).with_rotation(Quat::from_rotation_y(yaw)),
            Visibility::default(),
            (
                RigidBody::Kinematic,
                CustomPositionIntegration,
                TransformInterpolation,
                Collider::sphere(d.radius * 1.15),
                Sensor,
                CollisionLayers::new(GameLayer::Hitbox, LayerMask::NONE),
            ),
        ))
        .id()
}

/// Thrown drones tumble and bounce; settled drones drive for whoever is
/// looking through them.
#[allow(clippy::too_many_arguments)]
fn drive_drones(
    tuning: Res<Tuning>,
    time: Res<Time>,
    move_and_slide: MoveAndSlide,
    mut drones: Query<(Entity, &mut Drone, &mut Transform)>,
    drivers: Query<(Entity, &DroneIntent, &RemoteView)>,
    mut noise: MessageWriter<Noise>,
) {
    let d = &tuning.round.gadgets.drone;
    let dt = time.delta_secs();
    let spatial = &move_and_slide.spatial_query;
    let world = SpatialQueryFilter::from_mask(GameLayer::World);
    let shape = Collider::sphere(d.radius);
    let max_slope = d.max_slope.to_radians();
    for (entity, mut drone, mut transform) in &mut drones {
        let mut position = transform.translation;
        match drone.state {
            DroneState::Thrown { age } => {
                drone.velocity.y -= d.gravity * dt;
                let mut remaining = drone.velocity * dt;
                let mut settled = false;
                for _ in 0..3 {
                    let Ok((dir, length)) = Dir3::new_and_length(remaining) else {
                        break;
                    };
                    let hit = spatial.cast_shape(&shape, position, Quat::IDENTITY, dir, &ShapeCastConfig::from_max_distance(length), &world);
                    let Some(hit) = hit else {
                        position += remaining;
                        break;
                    };
                    position += *dir * (hit.distance - 0.002).max(0.0);
                    let n = hit.normal1;
                    let vn = drone.velocity.dot(n);
                    if vn < 0.0 {
                        drone.velocity -= n * vn * 1.35;
                        drone.velocity *= 0.65;
                    }
                    if n.y > 0.7 && drone.velocity.length() < 1.6 {
                        settled = true;
                        break;
                    }
                    remaining = *dir * (length - hit.distance).max(0.0);
                    remaining -= n * remaining.dot(n);
                }
                let age = age + dt;
                if settled || age > 4.0 {
                    drone.state = DroneState::Driving;
                    drone.velocity = Vec3::ZERO;
                } else {
                    drone.state = DroneState::Thrown { age };
                }
            }
            DroneState::Driving => {
                let intent = drivers
                    .iter()
                    .find(|(viewer, _, view)| *viewer == drone.owner && **view == RemoteView::Drone(entity))
                    .map(|(_, intent, _)| intent);
                let mut forward_input = 0.0;
                if let Some(intent) = intent {
                    drone.yaw = intent.yaw;
                    drone.pitch = intent.pitch;
                    if drone.battery > 0.0 {
                        forward_input = intent.drive.y;
                    }
                }
                let heading = Quat::from_rotation_y(drone.yaw) * Vec3::NEG_Z;
                let speed = if forward_input >= 0.0 { d.speed } else { d.reverse_speed };
                let target = heading * forward_input * speed;
                let horizontal = drone.velocity.with_y(0.0).move_towards(target, d.acceleration * dt);
                let vertical = (drone.velocity.y - d.gravity * dt).max(-30.0);
                let desired = horizontal + Vec3::Y * vertical;
                if horizontal.length_squared() > 0.01 {
                    drone.battery = (drone.battery - dt).max(0.0);
                    drone.noise_timer -= dt;
                    if drone.noise_timer <= 0.0 {
                        drone.noise_timer = 0.5;
                        noise.write(Noise {
                            source: Some(drone.owner),
                            position,
                            loudness: d.noise,
                            kind: NoiseKind::Other,
                        });
                    }
                }

                let mut grounded = false;
                let out = move_and_slide.move_and_slide(
                    &shape,
                    position,
                    Quat::IDENTITY,
                    desired,
                    time.delta(),
                    &MoveAndSlideConfig::default(),
                    &world,
                    |hit| {
                        if hit.normal.angle_between(Vec3::Y) <= max_slope {
                            grounded = true;
                        }
                        MoveAndSlideHitResponse::Accept
                    },
                );
                let wanted = horizontal * dt;
                let moved = (out.position - position).with_y(0.0);
                let mut new_position = out.position;
                let mut velocity = out.projected_velocity;
                // Blocked by a low ledge: step up onto it.
                if drone.grounded && wanted.length() > 1e-4 && moved.length() < wanted.length() * 0.5 {
                    if let Some(stepped) = step_up(spatial, &shape, position, wanted, d.step_height, max_slope, &world) {
                        new_position = stepped;
                        velocity = horizontal;
                        grounded = true;
                    }
                }
                // Stay on the floor going down small steps and ramps.
                if !grounded && drone.grounded && velocity.y <= 0.0 {
                    if let Some(hit) = spatial.cast_shape(&shape, new_position, Quat::IDENTITY, Dir3::NEG_Y, &ShapeCastConfig::from_max_distance(d.step_height), &world)
                        && hit.normal1.angle_between(Vec3::Y) <= max_slope
                    {
                        new_position.y -= hit.distance;
                        grounded = true;
                    }
                }
                if grounded {
                    velocity.y = velocity.y.max(0.0).min(horizontal.length());
                    if velocity.y < 0.05 {
                        velocity.y = 0.0;
                    }
                }
                drone.grounded = grounded;
                drone.velocity = velocity;
                position = new_position;
            }
        }
        transform.translation = position;
        transform.rotation = Quat::from_rotation_y(drone.yaw);
    }
}

/// Up `step` metres, along `wanted`, then back down onto walkable ground.
fn step_up(
    spatial: &SpatialQuery,
    shape: &Collider,
    from: Vec3,
    wanted: Vec3,
    step: f32,
    max_slope: f32,
    filter: &SpatialQueryFilter,
) -> Option<Vec3> {
    let config = |d: f32| ShapeCastConfig::from_max_distance(d);
    if spatial.cast_shape(shape, from, Quat::IDENTITY, Dir3::Y, &config(step), filter).is_some() {
        return None;
    }
    let raised = from + Vec3::Y * step;
    let (dir, length) = Dir3::new_and_length(wanted).ok()?;
    if spatial.cast_shape(shape, raised, Quat::IDENTITY, dir, &config(length), filter).is_some() {
        return None;
    }
    let ahead = raised + wanted;
    let down = spatial.cast_shape(shape, ahead, Quat::IDENTITY, Dir3::NEG_Y, &config(step + 0.02), filter)?;
    if down.normal1.angle_between(Vec3::Y) > max_slope || down.distance < 0.002 {
        return None;
    }
    Some(ahead - Vec3::Y * (down.distance - 0.002))
}

// ---- Model ----------------------------------------------------------------

/// A wheel (spins with speed).
#[derive(Component)]
struct DroneWheel;

/// Two big wheels on an axle, a body between them with a camera eye, a
/// whip antenna and a blinking light.
fn dress_drones(new: Query<(Entity, &Drone), Added<Drone>>, assets: Res<GadgetAssets>, tuning: Res<Tuning>, mut commands: Commands) {
    let a = &*assets;
    let r = tuning.round.gadgets.drone.radius;
    let lift = tuning.round.gadgets.drone.camera_height - r;
    for (entity, _) in &new {
        commands.entity(entity).with_children(|c| {
            for side in [-1.0_f32, 1.0] {
                c.spawn((
                    DroneWheel,
                    Mesh3d(a.unit_cylinder.clone()),
                    MeshMaterial3d(a.rubber.clone()),
                    Transform::from_xyz(side * r * 0.85, 0.0, 0.0)
                        .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2))
                        .with_scale(Vec3::new(r * 1.95, r * 0.45, r * 1.95)),
                ))
                .with_children(|w| {
                    // A hub mark so the spin shows.
                    w.spawn(super::part(a, &a.steel, Vec3::new(0.0, 0.0, 0.3), Vec3::new(0.2, 1.05, 0.25)));
                });
            }
            c.spawn(super::part(a, &a.olive, Vec3::new(0.0, 0.0, 0.0), Vec3::new(r * 1.2, r * 0.9, r * 1.3)));
            c.spawn(super::part(a, &a.dark_metal, Vec3::new(0.0, lift * 0.6, -r * 0.45), Vec3::new(r * 0.55, r * 0.5, r * 0.4)));
            c.spawn((
                Mesh3d(a.unit_sphere.clone()),
                MeshMaterial3d(a.glass.clone()),
                Transform::from_xyz(0.0, lift * 0.6, -r * 0.66).with_scale(Vec3::splat(r * 0.32)),
            ));
            c.spawn(super::part(a, &a.rubber, Vec3::new(r * 0.35, r * 1.4, r * 0.4), Vec3::new(0.006, r * 2.0, 0.006)));
            c.spawn((
                Mesh3d(a.unit_sphere.clone()),
                MeshMaterial3d(a.green_light.clone()),
                Transform::from_xyz(-r * 0.3, r * 0.47, r * 0.3).with_scale(Vec3::splat(r * 0.12)),
            ));
        });
    }
}

fn spin_wheels(
    drones: Query<(&Drone, &Children)>,
    mut wheels: Query<&mut Transform, With<DroneWheel>>,
    tuning: Res<Tuning>,
    time: Res<Time>,
) {
    let r = tuning.round.gadgets.drone.radius;
    for (drone, children) in &drones {
        let heading = Quat::from_rotation_y(drone.yaw) * Vec3::NEG_Z;
        let speed = drone.velocity.with_y(0.0).dot(heading);
        let spin = -speed / r.max(0.01) * time.delta_secs();
        for child in children.iter() {
            if let Ok(mut transform) = wheels.get_mut(child) {
                transform.rotation = Quat::from_rotation_x(spin) * transform.rotation;
            }
        }
    }
}
