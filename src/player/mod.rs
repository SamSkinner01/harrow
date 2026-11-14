//! Player characters: movement, stances, lean, camera, aim-down-sights.
//!
//! Data flow, kept strictly one-way so bots and networking can plug in later:
//!
//! 1. **Input** (`input.rs`, every frame): keyboard/mouse -> [`PlayerInput`]
//!    on the local player. Bots and network clients will write the same
//!    component instead.
//! 2. **Simulation** (`movement.rs`, fixed tick): reads [`PlayerInput`],
//!    updates [`CharacterState`], velocity and position. Never touches input
//!    devices or the camera.
//! 3. **Presentation** (`camera.rs`, every frame): reads the interpolated
//!    simulation state and places the camera. Never changes gameplay state.

mod camera;
mod input;
mod movement;

pub use input::CursorGrabDisabled;

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::config::Tuning;

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SpawnPoint>()
            .add_plugins((
                input::PlayerInputPlugin,
                movement::MovementPlugin,
                camera::PlayerCameraPlugin,
            ))
            .add_systems(Startup, spawn_local_player);
    }
}

/// A character controlled by a player or (later) a bot.
#[derive(Component)]
pub struct Player;

/// The character this machine controls and the camera follows.
#[derive(Component)]
pub struct LocalPlayer;

/// What the controlling player wants this tick. Pure intent: the simulation
/// decides what is actually possible.
#[derive(Component, Default, Debug, Clone)]
pub struct PlayerInput {
    /// x = strafe right, y = forward. Length at most 1.
    pub move_axis: Vec2,
    /// Absolute look angles in radians. Yaw 0 faces -Z.
    pub yaw: f32,
    pub pitch: f32,
    pub sprint: bool,
}

/// Simulation state owned by the movement code.
#[derive(Component, Debug)]
pub struct CharacterState {
    /// Current body height (collider height), metres.
    pub height: f32,
    pub grounded: bool,
}

/// Where the local player spawns. Debug flags can override it.
#[derive(Resource, Debug, Clone)]
pub struct SpawnPoint {
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
}

impl Default for SpawnPoint {
    fn default() -> Self {
        Self {
            position: Vec3::new(0.0, 0.0, 10.0),
            yaw: 0.0,
            pitch: 0.0,
        }
    }
}

fn spawn_local_player(mut commands: Commands, spawn: Res<SpawnPoint>, tuning: Res<Tuning>) {
    commands.spawn((
        Name::new("Local player"),
        Player,
        LocalPlayer,
        PlayerInput {
            yaw: spawn.yaw,
            pitch: spawn.pitch,
            ..default()
        },
        CharacterState {
            height: tuning.body.stand_height,
            grounded: false,
        },
        // The transform origin is at the feet. The movement capsule is built
        // from `CharacterState::height` each tick rather than stored as a
        // collider, so stance changes are a single number.
        Transform::from_translation(spawn.position),
        RigidBody::Kinematic,
        CustomPositionIntegration,
        TransformInterpolation,
        LinearVelocity::ZERO,
    ));
}
