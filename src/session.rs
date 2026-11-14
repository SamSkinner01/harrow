//! Session flow: main menu -> (connecting) -> in game -> back to the menu,
//! and which machine owns the simulation.
//!
//! - [`AppState`]: `MainMenu`, `Connecting` (joining a server), `InGame`.
//!   The map is built once at startup and stays loaded (it is the menu's
//!   backdrop); everything that belongs to one session (characters, round
//!   state, deployed gadgets, network links) is created on entering
//!   `InGame` and removed again on leaving it.
//! - [`NetMode`]: solo (`Offline`), `Host` (listen server, the host plays),
//!   `Client` (joined someone else's server) or `Dedicated` (headless
//!   server). Everything except `Client` is the *authority*: it runs the
//!   whole simulation. A client only predicts its own movement and renders
//!   what the server replicates; see `src/net/`.
//! - [`Launch`]: the command line. Any `--flag` skips the menu and goes
//!   straight in (solo unless `--host`, `--connect` or `--server` says
//!   otherwise), so every automated test keeps working unchanged.
//!
//! Gameplay modules gate their fixed-tick simulation with the run
//! conditions here ([`simulating`]); `NetPlugin` already applies them to
//! the round, gadget, destruction and combat system sets.

use std::net::{SocketAddr, ToSocketAddrs};

use bevy::prelude::*;

use crate::{
    combat::Team,
    player::Player,
    round::{
        Round, RoundReset,
        defuser::{DefuserDevice, DroppedDefuser},
    },
};

/// Default UDP port for hosting and joining.
pub const DEFAULT_PORT: u16 = 5888;

pub struct SessionPlugin {
    pub launch: Launch,
}

impl Plugin for SessionPlugin {
    fn build(&self, app: &mut App) {
        let launch = self.launch.clone();
        app.insert_state(launch.initial_state())
            .insert_resource(launch.mode.clone().unwrap_or(NetMode::Offline))
            .insert_resource(launch.clone())
            .init_resource::<MenuNotice>()
            .add_systems(OnExit(AppState::InGame), teardown_session)
            .add_systems(
                OnEnter(AppState::InGame),
                crate::player::spawn_local_player.run_if(has_local_player),
            )
            .add_systems(Update, log_state_changes);
    }
}

/// Where the app is.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    /// Main menu over the map. No characters exist.
    #[default]
    MainMenu,
    /// Joining a server (handshake in progress).
    Connecting,
    /// Playing (solo, hosting, joined, or serving).
    InGame,
}

/// Who owns the simulation in this session.
#[derive(Resource, Debug, Clone, PartialEq)]
pub enum NetMode {
    /// Solo: no networking at all.
    Offline,
    /// Listen server: this machine simulates and also has a local player.
    Host { port: u16 },
    /// Joined a server: only the local player's movement is predicted here.
    Client { server: SocketAddr },
    /// Headless dedicated server.
    Dedicated { port: u16 },
}

impl NetMode {
    /// Runs the authoritative simulation (round loop, damage, gadgets...).
    pub fn is_authority(&self) -> bool {
        !matches!(self, NetMode::Client { .. })
    }

    /// Accepts connections from clients.
    pub fn is_server(&self) -> bool {
        matches!(self, NetMode::Host { .. } | NetMode::Dedicated { .. })
    }

    /// Has a character controlled from this machine's keyboard.
    pub fn has_local_player(&self) -> bool {
        matches!(self, NetMode::Offline | NetMode::Host { .. })
    }
}

/// Run condition: this machine owns the simulation.
pub fn is_authority(mode: Res<NetMode>) -> bool {
    mode.is_authority()
}

/// Run condition: joined someone else's server.
pub fn is_client(mode: Res<NetMode>) -> bool {
    !mode.is_authority()
}

/// Run condition: hosting or serving.
pub fn is_server(mode: Res<NetMode>) -> bool {
    mode.is_server()
}

/// Run condition: in game and this machine owns the simulation. Use it for
/// fixed-tick gameplay that only the server may decide (rounds, damage,
/// gadgets, destruction).
pub fn simulating(mode: Res<NetMode>, state: Res<State<AppState>>) -> bool {
    mode.is_authority() && *state.get() == AppState::InGame
}

fn has_local_player(mode: Res<NetMode>) -> bool {
    mode.has_local_player()
}

/// A line for the main menu explaining why we're back there
/// ("Disconnected: server closed").
#[derive(Resource, Default, Debug)]
pub struct MenuNotice(pub Option<String>);

/// The command line, parsed once at startup.
#[derive(Resource, Debug, Clone, Default)]
pub struct Launch {
    /// `--solo`, `--host [port]`, `--connect addr`, `--server [port]`.
    pub mode: Option<NetMode>,
    /// Any `--flag` was given: skip the main menu.
    pub skip_menu: bool,
    /// `--server`: headless, no window/audio/input devices.
    pub dedicated: bool,
    /// `--config path` (dedicated server settings).
    pub config: Option<std::path::PathBuf>,
    /// `--name`: player name shown to others.
    pub name: Option<String>,
    /// `--net-latency/--net-jitter/--net-loss`: simulated bad network on
    /// everything this instance receives.
    pub conditioner: Option<ConditionerSettings>,
    /// `--net-shoot-test`: server fires lag-compensated test shots.
    pub shoot_test: bool,
    /// `--net-trace`: log round phase, players and rollbacks once a second.
    pub net_trace: bool,
    /// Problems with the flags, reported once logging is up.
    pub errors: Vec<String>,
}

/// Artificial latency/jitter/loss on incoming packets (testing).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConditionerSettings {
    pub latency_ms: u32,
    pub jitter_ms: u32,
    /// 0..100.
    pub loss_percent: f32,
}

impl Launch {
    pub fn from_args() -> Self {
        Self::parse(std::env::args().skip(1).collect())
    }

    pub fn parse(args: Vec<String>) -> Self {
        let mut launch = Launch {
            skip_menu: args.iter().any(|a| a.starts_with("--")),
            ..default()
        };
        let value = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
                .filter(|v| !v.starts_with("--"))
                .cloned()
        };
        let has = |flag: &str| args.iter().any(|a| a == flag);
        let port = |flag: &str, errors: &mut Vec<String>| match value(flag) {
            None => DEFAULT_PORT,
            Some(raw) => raw.parse().unwrap_or_else(|_| {
                errors.push(format!("{flag}: bad port {raw:?}, using {DEFAULT_PORT}"));
                DEFAULT_PORT
            }),
        };
        let mut errors = Vec::new();
        if has("--server") {
            launch.dedicated = true;
            launch.mode = Some(NetMode::Dedicated {
                port: port("--server", &mut errors),
            });
        } else if has("--host") {
            launch.mode = Some(NetMode::Host {
                port: port("--host", &mut errors),
            });
        } else if has("--connect") {
            match value("--connect").map(|raw| resolve_address(&raw)) {
                Some(Ok(server)) => launch.mode = Some(NetMode::Client { server }),
                Some(Err(err)) => errors.push(format!("--connect: {err}")),
                None => errors.push("--connect needs an address (host[:port])".into()),
            }
        } else if has("--solo") {
            launch.mode = Some(NetMode::Offline);
        }
        launch.config = value("--config").map(Into::into);
        launch.name = value("--name");
        launch.shoot_test = has("--net-shoot-test");
        launch.net_trace = has("--net-trace");
        let number = |flag: &str| value(flag).and_then(|v| v.parse::<f32>().ok());
        let (latency, jitter, loss) = (
            number("--net-latency"),
            number("--net-jitter"),
            number("--net-loss"),
        );
        if latency.is_some() || jitter.is_some() || loss.is_some() {
            launch.conditioner = Some(ConditionerSettings {
                latency_ms: latency.unwrap_or(0.0).max(0.0) as u32,
                jitter_ms: jitter.unwrap_or(0.0).max(0.0) as u32,
                loss_percent: loss.unwrap_or(0.0).clamp(0.0, 100.0),
            });
        }
        launch.errors = errors;
        launch
    }

    fn initial_state(&self) -> AppState {
        match (&self.mode, self.skip_menu) {
            (Some(NetMode::Client { .. }), _) => AppState::Connecting,
            (Some(_), _) | (None, true) => AppState::InGame,
            (None, false) => AppState::MainMenu,
        }
    }
}

/// `host[:port]` (or an IP) to a socket address; the port defaults to 5888.
pub fn resolve_address(raw: &str) -> Result<SocketAddr, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("no address given".into());
    }
    if let Ok(addr) = raw.parse::<SocketAddr>() {
        return Ok(addr);
    }
    let with_port = if raw.contains(':') && !raw.contains("]:") && raw.matches(':').count() == 1 {
        raw.to_string()
    } else if raw.starts_with('[') && raw.contains("]:") {
        raw.to_string()
    } else {
        format!("{raw}:{DEFAULT_PORT}")
    };
    with_port
        .to_socket_addrs()
        .map_err(|err| format!("can't resolve {raw:?}: {err}"))?
        // Prefer IPv4: the server listens on 0.0.0.0.
        .min_by_key(|a| a.is_ipv6())
        .ok_or_else(|| format!("can't resolve {raw:?}"))
}

fn log_state_changes(state: Res<State<AppState>>, mode: Res<NetMode>, launch: Res<Launch>, mut once: Local<bool>) {
    if !*once {
        *once = true;
        for err in &launch.errors {
            warn!("{err}");
        }
    }
    if state.is_changed() {
        info!("session: {:?} ({:?})", state.get(), *mode);
    }
}

/// Leaving the game: remove everything that belongs to the session and put
/// the round loop back to its idle state. Network links are closed by
/// `NetPlugin` in the same schedule.
#[allow(clippy::type_complexity)]
fn teardown_session(
    session_entities: Query<
        Entity,
        Or<(
            With<Player>,
            With<Team>,
            With<DefuserDevice>,
            With<DroppedDefuser>,
            With<crate::gadgets::SensorDart>,
            With<crate::gadgets::SeismicStake>,
            With<crate::destruction::BreachCharge>,
        )>,
    >,
    mut round: ResMut<Round>,
    mut reset: MessageWriter<RoundReset>,
    mut commands: Commands,
) {
    let mut count = 0;
    for entity in &session_entities {
        commands.entity(entity).try_despawn();
        count += 1;
    }
    *round = Round::default();
    // Restores destroyed walls and clears placed gadgets (destruction and
    // gadget modules listen for this).
    reset.write(RoundReset { round: 0 });
    info!("session: ended, removed {count} entities");
}
