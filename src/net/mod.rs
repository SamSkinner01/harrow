//! Multiplayer (M8): Lightyear 0.31 client/server networking with a
//! server-authoritative simulation at the 64 Hz fixed tick.
//!
//! Architecture (details in DESIGN.md "M8 networking"):
//! - **Modes** ([`NetMode`](crate::session::NetMode)): solo runs with no
//!   network at all; *host* is solo plus a UDP server, the host's character
//!   is simulated exactly like a solo player; a *dedicated* server is the
//!   same without a local player, window or GPU; a *client* simulates only
//!   its own movement (predicted) and renders everything else from what the
//!   server replicates.
//! - **Input** (`input.rs`): clients send [`NetInput`](input::NetInput)
//!   (every intent component of their character) every tick; the server
//!   copies it back into the same intent components, so remote and local
//!   players run through identical gameplay code.
//! - **Prediction** (client's own character): movement state
//!   ([`SimPosition`], `LinearVelocity`, `CharacterState`) is predicted and
//!   rolled back when the server disagrees. Everything movement reads is
//!   either predicted, comes from input, or is replicated
//!   (`MovementLock`).
//! - **Interpolation** (other players): position, look angles, stance
//!   height and lean are interpolated ~100 ms in the past; `bodies.rs`
//!   shows them as M5 character models.
//! - **Replication** (`protocol.rs`): characters' round state, the match
//!   state ([`RoundSync`]), defusers and deployed gadgets. HUD and sound
//!   events are forwarded as messages (`events.rs`).
//! - **Lag compensation** (`lag_comp.rs`): the server keeps ~300 ms of
//!   hit-zone history per character and can rewind to what a shooter saw.
//! - **Session** (`server.rs`, `client.rs`): handshake with a build check,
//!   team balancing, late joiners, disconnects.

mod bodies;
mod client;
pub mod dedicated;
mod destruction_sync;
mod events;
pub mod input;
pub mod lag_comp;
pub mod protocol;
mod round_sync;
mod server;
mod trace;
pub mod tuning_sync;

use std::time::Duration;

use bevy::{ecs::system::SystemParam, prelude::*};
use lightyear::prelude::{
    PredictionManager,
    client::ClientPlugins,
    server::ServerPlugins,
};

pub use protocol::{NetAim, NetPlayer, RoundSync, SimPosition};

use crate::{
    combat::CombatSystems,
    destruction::DestructionSystems,
    gadgets::GadgetSystems,
    physics::TICK_HZ,
    player::PlayerInputSystems,
    round::RoundSystems,
    session::{AppState, Launch, simulating},
    weapons::WeaponSystems,
};

/// Netcode protocol id. Bump the last number when the wire format changes
/// incompatibly; builds are also compared by [`BUILD_ID`] on connect.
pub const PROTOCOL_ID: u64 = 0x4841_5252_4f57_0008;

/// Netcode private key. **Development only**: every build has the same
/// key, so it only keeps out other games' packets, not attackers. A public
/// server needs a real key from a matchmaking backend.
pub const DEV_PRIVATE_KEY: [u8; 32] = *b"harrow-dev-key-do-not-trust-this";

/// Identifies this exact build: crate version and git commit. Client and
/// server must match.
pub const BUILD_ID: &str = concat!(env!("CARGO_PKG_VERSION"), "+", env!("HARROW_GIT_COMMIT"));

/// One fixed tick.
pub fn tick_duration() -> Duration {
    Duration::from_secs_f64(1.0 / TICK_HZ)
}

pub struct NetPlugin {
    pub launch: Launch,
}

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        // Lightyear first, then the protocol, then anything that spawns
        // links (at runtime, from the menu or the command line).
        app.add_plugins((
            ClientPlugins {
                tick_duration: tick_duration(),
            },
            ServerPlugins {
                tick_duration: tick_duration(),
            },
        ))
        .add_plugins(protocol::ProtocolPlugin)
        .add_plugins((
            input::NetInputPlugin,
            server::ServerPlugin,
            client::ClientPlugin,
            round_sync::RoundSyncPlugin,
            events::EventsPlugin,
            destruction_sync::DestructionSyncPlugin,
            lag_comp::LagCompPlugin,
            tuning_sync::TuningSyncPlugin,
        ));
        if !self.launch.dedicated {
            app.add_plugins(bodies::BodiesPlugin);
        }
        if self.launch.net_trace {
            app.add_plugins(trace::NetTracePlugin);
        }

        // The authoritative simulation only runs in game, and only where
        // this machine owns it (not on clients).
        app.configure_sets(
            FixedUpdate,
            (
                RoundSystems::Script.run_if(simulating),
                RoundSystems::Objective.run_if(simulating),
                RoundSystems::Flow.run_if(simulating),
                RoundSystems::Reactions.run_if(simulating),
                RoundSystems::Locks.run_if(simulating),
                GadgetSystems.run_if(simulating),
                DestructionSystems::Actions.run_if(simulating),
                DestructionSystems::Explosions.run_if(simulating),
                WeaponSystems::Simulate.run_if(simulating),
            ),
        )
        .configure_sets(FixedPostUpdate, CombatSystems.run_if(simulating))
        // A rollback replay restores old input into `PlayerInput`; read this
        // frame's mouse and keys after it, so none of it is lost.
        .configure_sets(
            PreUpdate,
            PlayerInputSystems.after(lightyear::prelude::PredictionSystems::Rollback),
        )
        .add_systems(OnExit(AppState::InGame), forget_prediction);
    }
}

/// True while Lightyear is replaying ticks to correct a misprediction.
/// Systems with side effects that must happen once (sounds, footsteps,
/// sending input) check this.
#[derive(SystemParam)]
pub struct Replay<'w> {
    manager: Option<Res<'w, PredictionManager>>,
}

impl Replay<'_> {
    pub fn active(&self) -> bool {
        self.manager.as_ref().is_some_and(|m| m.is_rollback())
    }
}

fn forget_prediction(mut commands: Commands) {
    commands.remove_resource::<PredictionManager>();
}
