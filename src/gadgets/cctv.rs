//! Defender CCTV cameras at fixed, map-defined spots (DESIGN.md "Operator
//! structure"). Defenders press G to look through them, Q/E to switch,
//! the mouse (and A/D) to pan and tilt within limits. Attackers can see
//! them (red light) and shoot them out (`Health`); every round brings them
//! back.
//!
//! The spots live in [`CctvLayout`], keyed to Harrow Keep section A (two to
//! three per site area, plus the courtyard). A map can replace the
//! resource with its own spots.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::{DeployedGadget, GadgetAssets, GadgetOwner, GadgetStage};
use crate::{
    combat::{Health, Team},
    config::Tuning,
    physics::GameLayer,
    round::RoundReset,
};

pub struct CctvPlugin;

impl Plugin for CctvPlugin {
    fn build(&self, app: &mut App) {
        let keep = map_is_keep();
        app.insert_resource(if keep { CctvLayout::keep_section_a() } else { CctvLayout::default() })
            .add_systems(PostStartup, spawn_cameras)
            .add_systems(FixedUpdate, respawn_on_reset.in_set(GadgetStage::Input))
            .add_systems(Update, dress_cameras);
    }
}

fn map_is_keep() -> bool {
    let args: Vec<String> = std::env::args().collect();
    let map = args.iter().position(|a| a == "--map").and_then(|i| args.get(i + 1));
    map.is_none_or(|m| m == "keep")
}

/// One camera spot.
#[derive(Clone, Debug)]
pub struct CctvSpot {
    pub name: &'static str,
    /// Lens position (world).
    pub position: Vec3,
    /// Resting direction: yaw (degrees, 0 faces -Z, 90 faces -X) and pitch
    /// (degrees, negative looks down).
    pub yaw: f32,
    pub pitch: f32,
}

/// Camera spots of the current map.
#[derive(Resource, Clone, Debug, Default)]
pub struct CctvLayout {
    pub spots: Vec<CctvSpot>,
}

impl CctvLayout {
    /// Harrow Keep, section A (coordinates match `src/map/keep.rs`): inside
    /// faces of the stone walls are 0.25 m off the grid lines; ceilings at
    /// 3.0 (ground) and 6.2 (upper).
    pub fn keep_section_a() -> Self {
        let spot = |name, x, y, z, yaw, pitch| CctvSpot {
            name,
            position: Vec3::new(x, y, z),
            yaw,
            pitch,
        };
        Self {
            spots: vec![
                // Courtyard: high on the keep's facade, watching the gate.
                spot("COURTYARD", 13.6, 2.7, 0.45, 160.0, -17.0),
                // Site A area (ground floor).
                spot("WEST HALL", 5.55, 2.65, -7.5, 145.0, -24.0),
                spot("EAST ROOM", 11.5, 2.65, -0.5, 45.0, -20.0),
                spot("STOREROOM (A)", 11.5, 2.65, -7.5, 130.0, -22.0),
                // Site B area (upper floor).
                spot("UPPER WEST", 0.55, 5.85, -7.5, 230.0, -22.0),
                spot("UPPER EAST (B)", 11.5, 5.85, -0.5, 40.0, -22.0),
            ],
        }
    }
}

/// A CCTV camera. Its `Transform` is the lens; the view rests at
/// `yaw`/`pitch` and the viewer pans it within limits.
#[derive(Component, Clone, Debug)]
pub struct CctvCamera {
    /// Index into [`CctvLayout::spots`].
    pub index: usize,
    pub name: &'static str,
    /// Resting direction, radians.
    pub yaw: f32,
    pub pitch: f32,
}

impl CctvCamera {
    /// View rotation for a pan/tilt offset (radians, clamped by `tuning`).
    pub fn view(&self, pan: f32, tilt: f32, tuning: &Tuning) -> Quat {
        let c = &tuning.round.gadgets.cctv;
        let pan = wrap(pan).clamp(-c.pan_limit.to_radians(), c.pan_limit.to_radians());
        let tilt = tilt.clamp(-c.tilt_limit.to_radians(), c.tilt_limit.to_radians());
        Quat::from_euler(EulerRot::YXZ, self.yaw + pan, (self.pitch + tilt).clamp(-1.4, 1.4), 0.0)
    }
}

/// Angle to -PI..PI.
fn wrap(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

fn spawn_all(commands: &mut Commands, layout: &CctvLayout, tuning: &Tuning) {
    for (index, spot) in layout.spots.iter().enumerate() {
        commands.spawn((
            Name::new(format!("CCTV {}", spot.name)),
            CctvCamera {
                index,
                name: spot.name,
                yaw: spot.yaw.to_radians(),
                pitch: spot.pitch.to_radians(),
            },
            GadgetOwner {
                entity: None,
                team: Team::Defenders,
            },
            DeployedGadget,
            Health::new(tuning.round.gadgets.cctv.health),
            Transform::from_translation(spot.position).with_rotation(Quat::from_euler(
                EulerRot::YXZ,
                spot.yaw.to_radians(),
                spot.pitch.to_radians(),
                0.0,
            )),
            Visibility::default(),
            Collider::cuboid(0.16, 0.14, 0.3),
            Sensor,
            CollisionLayers::new(GameLayer::Hitbox, LayerMask::NONE),
            children![super::blast_anchor(Vec3::ZERO)],
        ));
    }
}

fn spawn_cameras(mut commands: Commands, layout: Res<CctvLayout>, tuning: Res<Tuning>) {
    spawn_all(&mut commands, &layout, &tuning);
}

/// Shot-out cameras come back every round.
fn respawn_on_reset(
    mut resets: MessageReader<RoundReset>,
    cameras: Query<Entity, With<CctvCamera>>,
    layout: Res<CctvLayout>,
    tuning: Res<Tuning>,
    mut commands: Commands,
) {
    if resets.read().last().is_none() {
        return;
    }
    for camera in &cameras {
        commands.entity(camera).despawn();
    }
    spawn_all(&mut commands, &layout, &tuning);
}

/// A housing on a wall bracket, a lens and a red recording light.
fn dress_cameras(new: Query<Entity, Added<CctvCamera>>, assets: Res<GadgetAssets>, mut commands: Commands) {
    let a = &*assets;
    for entity in &new {
        commands.entity(entity).with_children(|c| {
            // Housing (local -Z is where it looks).
            c.spawn(super::part(a, &a.steel, Vec3::new(0.0, 0.0, 0.06), Vec3::new(0.12, 0.11, 0.26)));
            c.spawn(super::part(a, &a.steel, Vec3::new(0.0, 0.065, 0.02), Vec3::new(0.15, 0.015, 0.32)));
            c.spawn((
                Mesh3d(a.unit_cylinder.clone()),
                MeshMaterial3d(a.glass.clone()),
                Transform::from_xyz(0.0, 0.0, -0.075)
                    .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2))
                    .with_scale(Vec3::new(0.075, 0.02, 0.075)),
            ));
            c.spawn((
                Mesh3d(a.unit_sphere.clone()),
                MeshMaterial3d(a.red_light.clone()),
                Transform::from_xyz(0.045, -0.035, -0.07).with_scale(Vec3::splat(0.014)),
            ));
            // Bracket back to the wall.
            c.spawn(super::part(a, &a.dark_metal, Vec3::new(0.0, 0.0, 0.23), Vec3::new(0.035, 0.035, 0.14)));
            c.spawn(super::part(a, &a.dark_metal, Vec3::new(0.0, 0.0, 0.31), Vec3::new(0.12, 0.12, 0.02)));
        });
    }
}
