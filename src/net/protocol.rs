//! The network protocol: every message, input and replicated component.
//!
//! **This is the one place features plug into networking.** Registration
//! order defines the wire ids, so client and server must be the same build
//! (the handshake in `server.rs` checks that, using the messages registered
//! first, whose ids never change).
//!
//! How to network a feature (see DESIGN.md "M8 networking"):
//! - *Intent* (what a player wants, read by the fixed-tick simulation):
//!   add a field to [`NetInput`](super::input::NetInput) and one
//!   `sync_intent` line in `input.rs`. The server copies it onto the
//!   character every tick; inputs are redundant and buffered by Lightyear.
//! - *State* that clients display (HUD, models): `app.component::<C>()
//!   .replicate()` below. `C` needs `Serialize + Deserialize`, and entity
//!   fields must be marked `#[entities]` so they are mapped.
//! - *State the local player's movement reads* must also be predicted:
//!   `.replicate().predict()` (plus a rollback tolerance for floats).
//! - *State of other players that moves smoothly*: add interpolation.
//! - *One-off happenings* (sounds, notices): forward the message in
//!   `events.rs`.

use avian3d::prelude::LinearVelocity;
use bevy::prelude::*;
use lightyear::{
    input::{config::InputConfig, native::prelude::InputPlugin},
    prelude::*,
};
use serde::{Deserialize, Serialize};

use super::input::NetInput;
use crate::{
    combat::{Dead, Health, Team},
    gadgets::{GadgetEvent, GadgetOwner, Reveal, SeismicStake, SensorDart, UniqueGadget},
    player::{CharacterState, Player},
    round::{
        MovementLock, ObjectiveInteraction, Round, RoundEvent, Squad,
        defuser::{DefuserCarrier, DefuserDevice, DroppedDefuser},
        operators::{Operator, OperatorChoice},
    },
    weapons::{BulletHit, MoveSpeedScale, ShotFired, WeaponEvent, WeaponState},
};

pub struct ProtocolPlugin;

impl Plugin for ProtocolPlugin {
    fn build(&self, app: &mut App) {
        // --- Handshake: registered first so the ids survive protocol
        // changes, and a version mismatch can be reported cleanly. ---------
        app.add_channel::<ControlChannel>(ChannelSettings {
            mode: ChannelMode::OrderedReliable(ReliableSettings::default()),
            ..default()
        })
        .add_direction(NetworkDirection::Bidirectional);
        app.register_message::<Hello>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<Welcome>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<Refused>()
            .add_direction(NetworkDirection::ServerToClient);

        // --- Inputs (every tick, redundant, buffered) ----------------------
        app.add_plugins(InputPlugin::<NetInput> {
            config: InputConfig {
                // Clients send their interpolation delay with every input,
                // so the server can rewind hit zones (`lag_comp.rs`).
                lag_compensation: true,
                // Survive ~10 lost packets in a row.
                packet_redundancy: 10,
                ..default()
            },
        });

        // --- Events the HUD and sounds react to (server -> client) --------
        app.add_channel::<EventChannel>(ChannelSettings {
            mode: ChannelMode::OrderedReliable(ReliableSettings::default()),
            ..default()
        })
        .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<RoundEvent>()
            .add_map_entities()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<GadgetEvent>()
            .add_map_entities()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<Reveal>()
            .add_map_entities()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<ShotFired>()
            .add_map_entities()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<WeaponEvent>()
            .add_map_entities()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<BulletHit>()
            .add_map_entities()
            .add_direction(NetworkDirection::ServerToClient);
        // Destruction (M4): the server's surface edits, replayed by clients.
        app.register_message::<super::destruction_sync::SurfaceEdit>()
            .add_direction(NetworkDirection::ServerToClient);

        // --- Characters -----------------------------------------------------
        app.component::<Player>().replicate();
        app.component::<NetPlayer>().replicate();
        // Movement state: predicted for the owner, interpolated for others.
        app.component::<SimPosition>()
            .replicate()
            .predict()
            .with_rollback_condition(|a, b| a.0.distance_squared(b.0) > 1e-6)
            .add_interpolation_with(|a, b, t| SimPosition(a.0.lerp(b.0, t)));
        app.component::<LinearVelocity>()
            .replicate()
            .predict()
            .with_rollback_condition(|a, b| a.0.distance_squared(b.0) > 1e-4);
        app.component::<CharacterState>()
            .replicate()
            .predict()
            .with_rollback_condition(CharacterState::differs)
            .add_interpolation_with(CharacterState::lerp);
        app.component::<NetAim>()
            .replicate()
            .add_interpolation_with(NetAim::lerp);
        // Round-loop state of each character (HUD, locks).
        app.component::<Health>().replicate();
        app.component::<Team>().replicate();
        app.component::<Squad>().replicate();
        app.component::<Dead>().replicate();
        app.component::<MovementLock>().replicate();
        app.component::<Operator>().replicate();
        app.component::<OperatorChoice>().replicate();
        app.component::<UniqueGadget>().replicate();
        app.component::<DefuserCarrier>().replicate();
        app.component::<ObjectiveInteraction>().replicate();
        // Weapons (M3): the server simulates them; clients show the
        // replicated state (HUD, view) and the forwarded shot events.
        app.component::<WeaponState>().replicate();
        // Movement reads it, so the owner's prediction needs it.
        app.component::<MoveSpeedScale>().replicate();

        // --- Match state ----------------------------------------------------
        app.component::<RoundSync>().replicate();

        // --- Objective and gadget entities ----------------------------------
        // Characters send `SimPosition` instead (their `Transform` is eased
        // between ticks for rendering).
        app.component::<Transform>()
            .replicate_filtered::<Without<Player>>();
        app.component::<DefuserDevice>().replicate();
        app.component::<DroppedDefuser>().replicate();
        app.component::<SensorDart>().replicate();
        app.component::<SeismicStake>().replicate();
        app.component::<GadgetOwner>().replicate();
        // M4: destruction travels as surface edits (`destruction_sync.rs`).
    }
}

/// Handshake and connection control (both directions, reliable).
pub struct ControlChannel;

/// Gameplay events for HUD and sound (server to clients, reliable).
pub struct EventChannel;

/// Client -> server, first thing after connecting.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Hello {
    /// [`super::BUILD_ID`] of the client.
    pub build: String,
    pub name: String,
}

/// Server -> client: you're in.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Welcome {
    pub server_name: String,
    pub build: String,
    /// The map the server runs (`keep`, `greybox`).
    pub map: String,
    /// Movement values the client must predict with (the server's tuning).
    pub tuning: super::tuning_sync::PredictionTuning,
}

/// Server -> client: you can't join (then the server disconnects you).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Refused {
    pub reason: String,
}

/// A character in a networked session (on every machine).
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct NetPlayer {
    /// Netcode client id; 0 for the host's own character.
    pub id: u64,
    pub name: String,
}

/// Feet position at the end of the latest fixed tick: the networked copy of
/// `Transform::translation` for characters. `Transform` itself is eased
/// between ticks for rendering, so it can't be sent as simulation state.
/// - Server: written from `Transform` after every tick.
/// - Owning client: predicted; restored into `Transform` at the start of
///   every tick (so rollbacks move the character).
/// - Other clients: interpolated and written into `Transform` every frame.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub struct SimPosition(pub Vec3);

/// Look angles of a character, for machines that don't have its input
/// (remote bodies, their head and aim direction).
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub struct NetAim {
    pub yaw: f32,
    pub pitch: f32,
}

impl NetAim {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        // Shortest way round for yaw.
        let tau = std::f32::consts::TAU;
        let mut delta = (b.yaw - a.yaw).rem_euclid(tau);
        if delta > std::f32::consts::PI {
            delta -= tau;
        }
        Self {
            yaw: (a.yaw + delta * t).rem_euclid(tau),
            pitch: a.pitch.lerp(b.pitch, t),
        }
    }
}

/// The match state ([`Round`]) as a component on one replicated entity.
/// The server copies the resource in; clients copy it out (`round_sync.rs`).
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RoundSync(pub Round);
