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
//!   state ([`RoundSync`]), defusers and deployed gadgets (`round_sync.rs`,
//!   `gadget_sync.rs`: cutters, charges, grenades, smoke, wire, cover,
//!   alarms, drones, CCTV, remote views, per-player gadget state). HUD and
//!   sound events are forwarded as messages (`events.rs`).
//! - **Lag compensation** (`lag_comp.rs`): the server keeps ~300 ms of
//!   hit-zone history per character and can rewind to what a shooter saw.
//! - **Session** (`server.rs`, `client.rs`): handshake with a build check,
//!   team balancing, late joiners, disconnects.

mod bodies;
mod client;
pub mod dedicated;
mod destruction_sync;
mod events;
mod gadget_sync;
pub mod input;
pub mod lag_comp;
pub mod protocol;
mod round_sync;
mod server;
mod test_script;
mod trace;
pub mod tuning_sync;

use std::time::Duration;

use bevy::{ecs::system::SystemParam, prelude::*};
use lightyear::prelude::{PredictionManager, client::ClientPlugins, server::ServerPlugins};

pub use protocol::{NetAim, NetPlayer, RoundSync, SimPosition};

use crate::{
    combat::CombatSystems,
    destruction::DestructionSystems,
    gadgets::{GadgetSpeedSystems, GadgetSystems},
    physics::TICK_HZ,
    player::PlayerInputSystems,
    round::RoundSystems,
    session::{AppState, Launch, simulating},
    weapons::WeaponSystems,
};

/// Netcode protocol id. Bump the last number when the wire format changes
/// incompatibly; builds are also compared by [`BUILD_ID`] on connect.
pub const PROTOCOL_ID: u64 = 0x4841_5252_4f57_0008;

/// Netcode private key when no secret is set. **Development only**: every
/// build has it, so it only keeps out other games' packets, not people.
pub const DEV_PRIVATE_KEY: [u8; 32] = *b"harrow-dev-key-do-not-trust-this";

/// The netcode private key for a shared secret (`--key`, `HARROW_KEY`, or
/// `key` in the server config). Clients make their own connect tokens, so
/// the key is a shared secret: a server with one only accepts clients that
/// use the same secret (others time out: "Server not responding"). No
/// secret: [`DEV_PRIVATE_KEY`].
pub fn netcode_key(secret: Option<&str>) -> [u8; 32] {
    match secret.map(str::trim).filter(|s| !s.is_empty()) {
        None => DEV_PRIVATE_KEY,
        Some(secret) => blake3::derive_key("harrow 2026-11 netcode private key", secret.as_bytes()),
    }
}

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
            gadget_sync::GadgetSyncPlugin,
            test_script::NetScriptPlugin,
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
        // Razor wire's slow-down is predicted: set before the predicted
        // history is recorded (every machine, rollback replays included).
        .configure_sets(
            FixedPostUpdate,
            GadgetSpeedSystems.before(lightyear::prelude::PredictionSystems::UpdateHistory),
        )
        // A rollback replay restores old input into `PlayerInput`; read this
        // frame's mouse and keys after it, so none of it is lost.
        .configure_sets(
            PreUpdate,
            PlayerInputSystems.after(lightyear::prelude::PredictionSystems::Rollback),
        )
        .add_systems(OnExit(AppState::InGame), forget_prediction)
        .add_systems(Update, despawn_closed_links)
        .add_systems(
            PostUpdate,
            say_goodbye_on_exit.before(lightyear::prelude::LinkSystems::Send),
        );
    }
}

/// Quitting (window closed, Ctrl+C, SIGTERM): tell the other side now, so
/// it doesn't wait for a timeout. The goodbye packets go out with this
/// frame's last send.
fn say_goodbye_on_exit(
    mut exit: MessageReader<AppExit>,
    clients: Query<Entity, (With<lightyear::prelude::client::Client>, With<lightyear::prelude::Connected>)>,
    servers: Query<Entity, With<lightyear::prelude::server::Started>>,
    mut commands: Commands,
) {
    if exit.read().next().is_none() {
        return;
    }
    for client in &clients {
        commands.trigger(lightyear::prelude::client::Disconnect { entity: client });
    }
    for server in &servers {
        commands.trigger(lightyear::prelude::server::Stop { entity: server });
    }
}

/// A closed link or server endpoint, despawned once its goodbye packets
/// are out.
#[derive(Component)]
struct DespawnAt(f32);

fn despawn_closed_links(
    links: Query<(Entity, &DespawnAt)>,
    time: Res<Time>,
    mut commands: Commands,
) {
    for (entity, at) in &links {
        if time.elapsed_secs() >= at.0 {
            commands.entity(entity).try_despawn();
        }
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
