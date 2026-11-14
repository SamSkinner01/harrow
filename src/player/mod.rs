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
#[cfg(feature = "client")]
mod footsteps;
mod hitbox;
mod input;
mod movement;

pub use camera::PlayerCamera;
pub use hitbox::{HitZone, spawn_hitboxes, zone_pose};
#[allow(unused_imports)] // used by M3+ input systems
pub use input::{AutomatedRun, input_captured};

use avian3d::prelude::*;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    combat::{Health, Team},
    config::{BodyTuning, Tuning},
};

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SpawnPoint>()
            .add_message::<Footstep>()
            .add_plugins((
                input::PlayerInputPlugin,
                movement::MovementPlugin,
                hitbox::HitboxPlugin,
                camera::PlayerCameraPlugin,
            ));
        // The local player is spawned when a session starts
        // (`session.rs`), not at startup.
        #[cfg(feature = "client")]
        app.add_plugins(footsteps::FootstepAudioPlugin);
    }
}

/// A character controlled by a player or (later) a bot.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Player;

/// The character this machine controls and the camera follows.
#[derive(Component)]
pub struct LocalPlayer;

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Reflect)]
pub enum Stance {
    #[default]
    Stand,
    Crouch,
    Prone,
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Reflect)]
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

/// A foot hit the ground. Written by the simulation; the hook for footstep
/// audio now, and for bots and detection gadgets later.
#[derive(Message, Clone, Debug)]
pub struct Footstep {
    pub entity: Entity,
    pub position: Vec3,
    /// 0 = silent .. 1 = sprinting. See `footsteps` in the tuning file.
    pub loudness: f32,
}

/// Simulation state owned by the movement code. Networked: predicted for
/// the owning client, interpolated for everyone else (`src/net/`).
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Ground distance covered since the last footstep, metres.
    pub stride: f32,
}

impl CharacterState {
    pub fn new(height: f32) -> Self {
        Self {
            stance: Stance::Stand,
            height,
            lean: 0.0,
            prev_height: height,
            prev_lean: 0.0,
            grounded: false,
            sprinting: false,
            stride: 0.0,
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

    /// The server's state and a predicted one differ enough to correct
    /// (networking rollback check).
    pub fn differs(a: &Self, b: &Self) -> bool {
        a.stance != b.stance
            || a.grounded != b.grounded
            || a.sprinting != b.sprinting
            || (a.height - b.height).abs() > 1e-3
            || (a.lean - b.lean).abs() > 1e-3
    }

    /// Blend for watching another player (networking interpolation).
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self {
            height: a.height.lerp(b.height, t),
            lean: a.lean.lerp(b.lean, t),
            prev_height: a.prev_height.lerp(b.prev_height, t),
            prev_lean: a.prev_lean.lerp(b.prev_lean, t),
            ..b
        }
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

/// Spawn the character this machine controls (solo and host), when the
/// session starts.
pub fn spawn_local_player(mut commands: Commands, spawn: Res<SpawnPoint>, tuning: Res<Tuning>) {
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
            Health::new(100.0),
            // Until the round loop assigns sides (M6).
            Team::Attackers,
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
