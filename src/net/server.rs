//! Server side (host and dedicated): listening, the handshake, spawning a
//! character for each client, teams, late joiners and disconnects.
//!
//! Connection flow:
//! 1. A client connects (netcode over UDP) and sends [`Hello`] with its
//!    build id.
//! 2. Wrong build or full server: [`Refused`], then a disconnect half a
//!    second later (so the message gets there).
//! 3. Otherwise the link gets `ReplicationSender` (from now on it receives
//!    the world), a character is spawned for it and [`Welcome`] goes out
//!    (server name, map, movement tuning).
//!
//! Teams: each client joins the squad with fewer members (the host is
//! squad Blue), and plays that squad's side. Joining during operator
//! selection (or free play) puts you straight in; joining mid-round makes
//! you a spectator: `Dead`, no `Health` (so you don't count for wipes),
//! until the next round starts.
//!
//! Disconnect: the character is removed (dropping the defuser if it had
//! it); team sizes follow automatically.

use std::net::{Ipv4Addr, SocketAddr};

use avian3d::prelude::*;
use bevy::prelude::*;
use lightyear::{
    input::native::prelude::ActionState,
    netcode::NetcodeServer,
    prelude::{server::*, *},
};

use super::{
    BUILD_ID, NetAim, NetPlayer, PROTOCOL_ID, SimPosition, netcode_key,
    dedicated::ServerConfig,
    input::{NetInput, SeenPresses},
    lag_comp::PoseHistory,
    protocol::{ControlChannel, Hello, Refused, Welcome},
    tuning_sync::PredictionTuning,
};
use crate::{
    combat::{Dead, Health, Team},
    config::Tuning,
    player::{CharacterState, LocalPlayer, Player, PlayerInput, SpawnPoint, spawn_hitboxes},
    round::{
        Round, RoundEvent, RoundPhase, RoundReset, RoundSettings, Squad,
        defuser::{DefuserAssets, DefuserCarrier, DroppedDefuser, spawn_device_model},
        sites::KeepLayout,
    },
    session::{AppState, Launch, NetMode, is_server},
};

pub(super) struct ServerPlugin;

impl Plugin for ServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::InGame), start_server.run_if(is_server))
            .add_systems(OnExit(AppState::InGame), stop_server)
            .add_observer(client_connected)
            .add_observer(client_disconnected)
            .add_observer(network_character)
            .add_systems(
                Update,
                (
                    receive_hello,
                    kick_refused,
                    idle_empty_server,
                    check_started,
                )
                    .run_if(is_server),
            )
            .add_systems(
                FixedUpdate,
                revive_spectators
                    .run_if(is_server)
                    .in_set(crate::round::RoundSystems::Reactions),
            )
            // On every machine that runs movement (the server for everyone,
            // a client for its predicted character), before Lightyear
            // records the predicted history.
            .add_systems(
                FixedPostUpdate,
                write_sim_state.before(lightyear::prelude::PredictionSystems::UpdateHistory),
            );
    }
}

/// The listening endpoint.
#[derive(Component)]
pub(super) struct NetServer {
    port: u16,
    /// When `start_server` ran.
    since: f32,
    reported: bool,
}

/// Server-side bookkeeping on a client's link entity.
#[derive(Component, Debug, Default)]
pub(super) struct ClientInfo {
    pub name: String,
    pub character: Option<Entity>,
}

/// Disconnect this link once the clock passes this time (refused joins).
#[derive(Component)]
struct KickAt(f32);

fn start_server(
    mode: Res<NetMode>,
    launch: Res<Launch>,
    config: Option<Res<ServerConfig>>,
    time: Res<Time>,
    mut commands: Commands,
) {
    let port = match *mode {
        NetMode::Host { port } | NetMode::Dedicated { port } => port,
        _ => return,
    };
    let max_clients = config.as_ref().map_or(10, |c| c.max_players as usize);
    // A dedicated server's key comes from its config (which the command
    // line and HARROW_KEY override); a host's from the command line.
    let secret = match &config {
        Some(c) => c.key.clone(),
        None => launch.key.clone(),
    };
    let address = SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), port);
    let server = commands
        .spawn((
            Name::new("Server"),
            NetServer {
                port,
                since: time.elapsed_secs(),
                reported: false,
            },
            Endpoint::new(super::client::conditioner(&launch)),
            NetcodeServer::new(NetcodeConfig {
                protocol_id: PROTOCOL_ID,
                private_key: netcode_key(secret.as_deref()),
                max_clients,
                // Clients type whatever address reaches us (LAN IP, public
                // IP, VPN name), which never equals our 0.0.0.0 bind
                // address, so don't check it.
                server_addr_check: false,
                client_timeout_secs: 5,
                ..default()
            }),
            LocalAddr(address),
            ServerUdpIo,
        ))
        .id();
    commands.trigger(Start { entity: server });
    info!(
        "net: server listening on UDP {address} (build {BUILD_ID}, up to {max_clients} clients, {})",
        if secret.is_some() {
            "private: clients need the server's key"
        } else {
            "open: development key, anyone with the game can join"
        }
    );
    // Replicate at most once per tick.
    commands.insert_resource(ReplicationMetadata::new(super::tick_duration()));
}

fn stop_server(servers: Query<Entity, With<NetServer>>, time: Res<Time>, mut commands: Commands) {
    for server in &servers {
        commands.trigger(Stop { entity: server });
        // A moment to tell the clients before the endpoint goes away.
        commands
            .entity(server)
            .remove::<NetServer>()
            .insert(super::DespawnAt(time.elapsed_secs() + 0.5));
        info!("net: server stopped");
    }
}

/// The UDP port couldn't be opened (in use, no permission): the host goes
/// back to the menu with a message; a dedicated server exits with an error.
fn check_started(
    mut servers: Query<(&mut NetServer, Has<Started>)>,
    mode: Res<NetMode>,
    time: Res<Time>,
    mut notice: ResMut<crate::session::MenuNotice>,
    mut next: ResMut<NextState<AppState>>,
    mut exit: MessageWriter<AppExit>,
) {
    for (mut server, started) in &mut servers {
        if started || server.reported || time.elapsed_secs() - server.since < 1.0 {
            continue;
        }
        server.reported = true;
        let message = format!(
            "Can't open UDP port {} (is another server using it?)",
            server.port
        );
        error!("net: {message}");
        if matches!(*mode, NetMode::Dedicated { .. }) {
            exit.write(AppExit::error());
        } else {
            notice.0 = Some(message);
            next.set(AppState::MainMenu);
        }
    }
}

fn client_connected(
    trigger: On<Add<Connected>>,
    links: Query<&RemoteId, With<ClientOf>>,
    mut commands: Commands,
) {
    let Ok(remote) = links.get(trigger.entity) else {
        return;
    };
    info!("net: client {} connected, waiting for hello", remote.0);
    commands
        .entity(trigger.entity)
        .insert(ClientInfo::default());
}

#[allow(clippy::too_many_arguments)]
fn receive_hello(
    mut links: Query<
        (
            Entity,
            &RemoteId,
            &mut MessageReceiver<Hello>,
            &mut MessageSender<Welcome>,
            &mut MessageSender<Refused>,
            &mut ClientInfo,
        ),
        With<ClientOf>,
    >,
    players: Query<(Entity, Option<&Squad>, Has<NetPlayer>), With<Player>>,
    round: Res<Round>,
    layout: Res<KeepLayout>,
    spawn_point: Res<SpawnPoint>,
    tuning: Res<Tuning>,
    config: Option<Res<ServerConfig>>,
    time: Res<Time>,
    mut commands: Commands,
) {
    let server_name = config
        .as_ref()
        .map_or_else(|| "Harrow".to_string(), |c| c.name.clone());
    let max_players = config.as_ref().map_or(10, |c| c.max_players);
    // Characters spawned this frame aren't in `players` yet.
    let mut joined = [0usize; 2];
    for (link, remote, mut hello, mut welcome, mut refused, mut info) in &mut links {
        for message in hello.receive() {
            if info.character.is_some() {
                continue;
            }
            let name = sanitize_name(&message.name, remote.0.to_bits());
            let reason = if message.build != BUILD_ID {
                Some(format!(
                    "Version mismatch: the server runs build {BUILD_ID}, you run {}. \
                     Everyone needs the same build (same commit).",
                    message.build
                ))
            } else if (players.iter().filter(|p| p.2).count() + joined[0] + joined[1]) as u32
                >= max_players
            {
                Some(format!("Server full ({max_players} players)"))
            } else {
                None
            };
            if let Some(reason) = reason {
                warn!("net: refusing {name} ({}): {reason}", remote.0);
                refused.send::<ControlChannel>(Refused { reason });
                commands
                    .entity(link)
                    .insert(KickAt(time.elapsed_secs() + 0.5));
                continue;
            }
            // Balance squads: join the smaller one (ties: Red, since the
            // host is Blue).
            let blue = players.iter().filter(|p| p.1 == Some(&Squad::Blue)).count() + joined[0];
            let red = players.iter().filter(|p| p.1 == Some(&Squad::Red)).count() + joined[1];
            let squad = if blue < red { Squad::Blue } else { Squad::Red };
            joined[usize::from(squad == Squad::Red)] += 1;
            let team = round.side_of(squad);
            let late = matches!(
                round.phase,
                RoundPhase::Prep | RoundPhase::Action | RoundPhase::Planted | RoundPhase::RoundEnd
            );
            let index = blue + red;
            let (position, yaw) = spawn_spot(&round, &layout, &spawn_point, team, index);
            let character = spawn_client_character(
                &mut commands,
                link,
                remote.0,
                &name,
                squad,
                team,
                position,
                yaw,
                late,
                &tuning,
            );
            info.name = name.clone();
            info.character = Some(character);
            commands.entity(link).insert(ReplicationSender);
            welcome.send::<ControlChannel>(Welcome {
                server_name: server_name.clone(),
                build: BUILD_ID.to_string(),
                map: crate::map::current_map_name(),
                tuning: PredictionTuning::from(&*tuning),
            });
            info!(
                "net: {name} ({}) joined squad {squad:?} ({team:?}){}",
                remote.0,
                if late {
                    ", spectating until the next round"
                } else {
                    ""
                }
            );
        }
    }
}

/// Where a new character appears: its team's spawns during a round, near
/// the map's spawn point in free play.
fn spawn_spot(
    round: &Round,
    layout: &KeepLayout,
    spawn: &SpawnPoint,
    team: Team,
    index: usize,
) -> (Vec3, f32) {
    let spawns = layout.spawns(team);
    if round.phase != RoundPhase::Inactive && !spawns.is_empty() {
        let s = spawns[(index + 1) % spawns.len()];
        return (s.position, s.yaw);
    }
    // Free play: a row next to the spawn point, 1.5 m apart.
    let right = Quat::from_rotation_y(spawn.yaw) * Vec3::X;
    let offset = (index as f32 + 1.0) * 1.5 * if index % 2 == 0 { 1.0 } else { -1.0 };
    (spawn.position + right * offset, spawn.yaw)
}

fn sanitize_name(name: &str, id: u64) -> String {
    let clean: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || " -_.".contains(*c))
        .take(20)
        .collect();
    let clean = clean.trim();
    if clean.is_empty() {
        format!("Player {}", id % 1000)
    } else {
        clean.to_string()
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_client_character(
    commands: &mut Commands,
    link: Entity,
    peer: PeerId,
    name: &str,
    squad: Squad,
    team: Team,
    position: Vec3,
    yaw: f32,
    late: bool,
    tuning: &Tuning,
) -> Entity {
    let mut entity = commands.spawn((
        Name::new(format!("Player {name}")),
        NetPlayer {
            id: peer.to_bits(),
            name: name.to_string(),
        },
        squad,
        team,
        PlayerInput { yaw, ..default() },
        ActionState::<NetInput>::default(),
        SeenPresses::default(),
        CharacterState::new(tuning.body.stand_height),
        Transform::from_translation(position).with_rotation(Quat::from_rotation_y(yaw)),
        (
            RigidBody::Kinematic,
            CustomPositionIntegration,
            TransformInterpolation,
            LinearVelocity::ZERO,
        ),
        (
            Replicate::to_clients(NetworkTarget::All),
            PredictionTarget::to_clients(NetworkTarget::Single(peer)),
            InterpolationTarget::to_clients(NetworkTarget::AllExceptSingle(peer)),
            ControlledBy {
                owner: link,
                // We despawn it ourselves, to drop the defuser first.
                lifetime: Lifetime::Persistent,
            },
            // Its gadget stock, flash and views go to this client only.
            super::gadget_sync::PrivateTo(link),
        ),
    ));
    if late {
        entity.insert(Dead);
    } else {
        entity.insert(Health::new(100.0));
    }
    // `Player` last: its observers see everything above.
    entity.insert(Player);
    entity.with_children(spawn_hitboxes);
    entity.id()
}

/// Every character on a server is networked: the host's own, clients',
/// test-script walkers.
fn network_character(
    trigger: On<Add<Player>>,
    mode: Res<NetMode>,
    characters: Query<(
        &Transform,
        Option<&PlayerInput>,
        Has<LocalPlayer>,
        Has<ControlledBy>,
        Has<NetPlayer>,
    )>,
    launch: Res<Launch>,
    mut commands: Commands,
) {
    if !mode.is_server() {
        return;
    }
    let Ok((transform, input, local, owned, named)) = characters.get(trigger.entity) else {
        return;
    };
    let mut entity = commands.entity(trigger.entity);
    entity.insert_if_new((
        SimPosition(transform.translation),
        NetAim {
            yaw: input.map_or(0.0, |i| i.yaw),
            pitch: input.map_or(0.0, |i| i.pitch),
        },
        PoseHistory::default(),
        Replicate::to_clients(NetworkTarget::All),
        // Hit zones and models are local children, not replicated.
        DisableReplicateHierarchy,
    ));
    if !owned {
        entity.insert_if_new(InterpolationTarget::to_clients(NetworkTarget::All));
    }
    if local && !named {
        entity.insert(NetPlayer {
            id: 0,
            name: launch.name.clone().unwrap_or_else(|| "Host".into()),
        });
    }
}

/// After every tick: the networked copies of movement state (feet
/// position, look angles) of every character simulated here.
fn write_sim_state(
    mut characters: Query<(&Transform, &PlayerInput, &mut SimPosition, &mut NetAim)>,
) {
    for (transform, input, mut sim, mut aim) in &mut characters {
        sim.set_if_neq(SimPosition(transform.translation));
        aim.set_if_neq(NetAim {
            yaw: input.yaw,
            pitch: input.pitch,
        });
    }
}

fn kick_refused(links: Query<(Entity, &KickAt)>, time: Res<Time>, mut commands: Commands) {
    for (link, kick) in &links {
        if time.elapsed_secs() >= kick.0 {
            commands.entity(link).remove::<KickAt>();
            commands.trigger(Disconnect { entity: link });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn client_disconnected(
    trigger: On<Add<Disconnected>>,
    links: Query<(&ClientInfo, Option<&RemoteId>, &Disconnected), With<ClientOf>>,
    characters: Query<(&Transform, Has<DefuserCarrier>)>,
    assets: Option<Res<DefuserAssets>>,
    mut events: MessageWriter<RoundEvent>,
    mut commands: Commands,
) {
    let Ok((info, remote, disconnected)) = links.get(trigger.entity) else {
        return;
    };
    info!(
        "net: {} ({}) left: {}",
        if info.name.is_empty() {
            "a client"
        } else {
            &info.name
        },
        remote.map_or_else(|| "?".to_string(), |r| r.0.to_string()),
        disconnected.reason
    );
    let Some(character) = info.character else {
        return;
    };
    if let Ok((transform, carrier)) = characters.get(character) {
        if carrier && let Some(assets) = assets {
            let position = transform.translation;
            spawn_device_model(
                &mut commands,
                &assets,
                position,
                0.0,
                (Name::new("Dropped defuser"), DroppedDefuser),
            );
            events.write(RoundEvent::DefuserDropped { position });
        }
        commands.entity(character).try_despawn();
    }
}

/// New round: spectators who joined mid-round become players again
/// (`reset_characters` in the round module moves them to a spawn).
fn revive_spectators(
    mut events: MessageReader<RoundEvent>,
    spectators: Query<Entity, (With<Player>, With<Dead>, Without<Health>)>,
    mut commands: Commands,
) {
    let new_round = events.read().any(|e| {
        matches!(
            e,
            RoundEvent::PhaseStarted {
                phase: RoundPhase::OperatorSelect,
                ..
            }
        )
    });
    if !new_round {
        return;
    }
    for entity in &spectators {
        commands
            .entity(entity)
            .remove::<Dead>()
            .insert(Health::new(100.0));
        info!("net: {entity} joins the round");
    }
}

/// A dedicated server with nobody on it doesn't play rounds: the loop waits
/// in `Inactive` until someone joins, and goes back there when the last
/// player leaves.
fn idle_empty_server(
    mode: Res<NetMode>,
    players: Query<(), With<NetPlayer>>,
    mut settings: ResMut<RoundSettings>,
    mut round: ResMut<Round>,
    mut reset: MessageWriter<RoundReset>,
    config: Option<Res<ServerConfig>>,
) {
    if !matches!(*mode, NetMode::Dedicated { .. }) {
        return;
    }
    let wanted = config.as_ref().is_none_or(|c| c.round_loop);
    let anyone = !players.is_empty();
    if anyone && wanted && !settings.enabled {
        settings.enabled = true;
        info!("net: players present, starting the round loop");
    } else if !anyone && settings.enabled {
        settings.enabled = false;
        *round = Round::default();
        reset.write(RoundReset { round: 0 });
        info!("net: server empty, round loop idle");
    }
}
