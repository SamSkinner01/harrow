//! Physics setup: Avian, the fixed simulation tick, and collision layers.
//!
//! All gameplay simulation (movement, stances, lean) runs in `FixedUpdate` at
//! [`TICK_HZ`]. Rendering runs at whatever the display allows and interpolates
//! between ticks. Keeping simulation on a fixed tick is what lets Lightyear
//! add prediction/rollback later without rewriting gameplay code.

use avian3d::prelude::*;
use bevy::prelude::*;

/// Simulation ticks per second.
pub const TICK_HZ: f64 = 64.0;

pub struct PhysicsSetupPlugin;

impl Plugin for PhysicsSetupPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(PhysicsPlugins::default())
            .insert_resource(Time::<Fixed>::from_hz(TICK_HZ));
    }
}

#[derive(PhysicsLayer, Default, Clone, Copy, Debug)]
pub enum GameLayer {
    /// Level geometry and props. Colliders without explicit layers land here.
    #[default]
    World,
    /// Character movement bodies.
    Character,
    /// Character hit zones. Sensors: only bullets (spatial queries) see them.
    Hitbox,
}
