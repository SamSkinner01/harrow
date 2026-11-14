//! The server's movement tuning wins on clients.
//!
//! A client predicts its own movement; if its `assets/config/tuning.ron`
//! had different speeds or heights than the server's, every tick would be
//! a misprediction. The server sends the values movement depends on in
//! [`Welcome`](super::protocol::Welcome); while connected the client
//! overrides those sections of its `Tuning` (also after a hot reload).
//! Look, controls, view and lighting stay the player's own.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::config::{
    BodyTuning, FootstepTuning, LeanTuning, MovementTuning, Tuning, TuningSystems,
};

pub struct TuningSyncPlugin;

impl Plugin for TuningSyncPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreUpdate, apply_server_tuning.after(TuningSystems));
    }
}

/// The tuning sections that movement (and so prediction) reads.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PredictionTuning {
    pub movement: MovementTuning,
    pub body: BodyTuning,
    pub lean: LeanTuning,
    pub footsteps: FootstepTuning,
    /// Razor wire's speed factor (`round.gadgets.razor_wire.slow`): the
    /// client predicts the slow-down.
    pub wire_slow: f32,
    /// Ballistic cover's size (`round.gadgets.cover`): clients collide
    /// with its shape.
    pub cover_width: f32,
    pub cover_height: f32,
}

impl From<&Tuning> for PredictionTuning {
    fn from(tuning: &Tuning) -> Self {
        Self {
            movement: tuning.movement.clone(),
            body: tuning.body.clone(),
            lean: tuning.lean.clone(),
            footsteps: tuning.footsteps.clone(),
            wire_slow: tuning.round.gadgets.razor_wire.slow,
            cover_width: tuning.round.gadgets.cover.width,
            cover_height: tuning.round.gadgets.cover.height,
        }
    }
}

/// The joined server's values (client only).
#[derive(Resource, Debug, Clone)]
pub struct ServerTuning(pub PredictionTuning);

fn apply_server_tuning(server: Option<Res<ServerTuning>>, mut tuning: ResMut<Tuning>) {
    let Some(server) = server else {
        return;
    };
    if !(server.is_changed() || tuning.is_changed()) {
        return;
    }
    let t = tuning.bypass_change_detection();
    t.movement = server.0.movement.clone();
    t.body = server.0.body.clone();
    t.lean = server.0.lean.clone();
    t.footsteps = server.0.footsteps.clone();
    t.round.gadgets.razor_wire.slow = server.0.wire_slow;
    t.round.gadgets.cover.width = server.0.cover_width;
    t.round.gadgets.cover.height = server.0.cover_height;
    if server.is_changed() {
        info!("net: using the server's movement tuning");
    }
}
