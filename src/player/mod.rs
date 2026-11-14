//! Player characters: movement, stances, lean, camera, aim-down-sights.
//!
//! Data flow, kept strictly one-way so bots and networking can plug in later:
//!
//! 1. **Input** (`input.rs`, every frame): keyboard/mouse -> [`PlayerInput`]
//!    on the local player. Bots and network clients will write the same
//!    component instead.
//! 2. **Simulation** (`movement.rs`, `hitbox.rs`, fixed tick): reads
//!    [`PlayerInput`], updates [`CharacterState`], velocity, position and hit
//!    zones. Never touches input devices or the camera.
//! 3. **Presentation** (`camera.rs`, every frame): reads the simulation state,
//!    interpolated between ticks, and places the camera. Never changes
//!    gameplay state.

mod camera;
mod hitbox;
mod input;
mod movement;

pub use hitbox::HitZone;
pub use input::CursorGrabDisabled;

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::config::{BodyTuning, Tuning};

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SpawnPoint>()
            .add_plugins((
                input::PlayerInputPlugin,
                movement::MovementPlugin,
                hitbox::HitboxPlugin,
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

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stance {
    #[default]
    Stand,
    Crouch,
    Prone,
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lean {
    Left,
    #[default]
    Center,
    Right,
}

/// What the controlling player wants. Pure intent: the simulation decides
/// what is actually possible (e.g. no standing up under a low ceiling).
#[derive(Component, Default, Debug, Clone)]
pub struct PlayerInput {
    /// x = strafe right, y = forward. Length at most 1.
    pub move_axis: Vec2,
    /// Absolute look angles in radians. Yaw 0 faces -Z.
    pub yaw: f32,
    pub pitch: f32,
    pub sprint: bool,
    pub stance: Stance,
    pub lean: Lean,
}

/// Simulation state owned by the movement code.
#[derive(Component, Debug)]
pub struct CharacterState {
    /// Stance the body is in or moving towards.
    pub stance: Stance,
    /// Current body height (movement capsule height), metres.
    pub height: f32,
    /// Current lean, -1 (full left) to 1 (full right).
    pub lean: f32,
    /// `height` and `lean` at the previous tick, for smooth rendering.
    pub prev_height: f32,
    pub prev_lean: f32,
    pub grounded: bool,
    pub sprinting: bool,
}

impl CharacterState {
    fn new(height: f32) -> Self {
        Self {
            stance: Stance::Stand,
            height,
            lean: 0.0,
            prev_height: height,
            prev_lean: 0.0,
            grounded: false,
            sprinting: false,
        }
    }

    /// Height and lean blended between the last two ticks. `t` is the
    /// fraction of a tick elapsed since the latest one.
    pub fn interpolated(&self, t: f32) -> (f32, f32) {
        (
            self.prev_height.lerp(self.height, t),
            self.prev_lean.lerp(self.lean, t),
        )
    }
}

impl BodyTuning {
    pub fn height_for(&self, stance: Stance) -> f32 {
        match stance {
            Stance::Stand => self.stand_height,
            Stance::Crouch => self.crouch_height,
            Stance::Prone => self.prone_height,
        }
    }
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
    commands
        .spawn((
            Name::new("Local player"),
            Player,
            LocalPlayer,
            PlayerInput {
                yaw: spawn.yaw,
                pitch: spawn.pitch,
                ..default()
            },
            CharacterState::new(tuning.body.stand_height),
            // The transform origin is at the feet and rotates with yaw. The
            // movement capsule is built from `CharacterState::height` each
            // tick rather than stored as a collider, so a stance change is
            // a single number.
            Transform::from_translation(spawn.position)
                .with_rotation(Quat::from_rotation_y(spawn.yaw)),
            RigidBody::Kinematic,
            CustomPositionIntegration,
            TransformInterpolation,
            LinearVelocity::ZERO,
        ))
        .with_children(hitbox::spawn_hitboxes);
}
