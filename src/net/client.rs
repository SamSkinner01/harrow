//! Client side: connecting, the handshake, setting up the character we
//! control (predicted) and the ones we only watch (interpolated), and
//! going back to the menu on any failure or disconnect.
//!
//! - `Connecting`: link up, send [`Hello`]; [`Welcome`] -> `InGame`;
//!   [`Refused`], a map mismatch, a disconnect or 12 s of silence -> main
//!   menu with a message.
//! - Our character arrives with `Controlled`: it becomes the
//!   `LocalPlayer` (keyboard, camera, HUD) and gets the same physics
//!   components as a solo player, so the movement code runs on it
//!   unchanged. At the start of every tick its `Transform` is set from the
//!   predicted [`SimPosition`], so Lightyear's rollbacks move it.
//! - Other characters get their `Transform` from the interpolated
//!   [`SimPosition`] and [`NetAim`] every frame.

use std::{
    hash::{BuildHasher, RandomState},
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};

use avian3d::prelude::*;
use bevy::prelude::*;
use lightyear::{
    input::native::prelude::InputMarker,
    link::RecvLinkConditioner,
    netcode::{NetcodeClient, client_plugin::NetcodeConfig},
    prelude::{LinkConditionerConfig, client::*, *},
};

use super::{
    BUILD_ID, DEV_PRIVATE_KEY, NetAim, NetPlayer, PROTOCOL_ID, SimPosition,
    input::NetInput,
    protocol::{ControlChannel, Hello, Refused, Welcome},
    tuning_sync::ServerTuning,
};
use crate::{
    destruction::DestructionIntent,
    player::{LocalPlayer, PlayerInput, spawn_hitboxes},
    session::{AppState, Launch, MenuNotice, NetMode},
};

pub(super) struct ClientPlugin;

impl Plugin for ClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Connecting), start_client)
            .add_systems(OnEnter(AppState::MainMenu), close_client)
            .add_observer(send_hello)
            .add_observer(attempt_started)
            .add_observer(link_lost)
            .add_observer(control_character)
            .add_observer(watch_character)
            .add_systems(
                Update,
                (receive_welcome, receive_refused, connect_timeout)
                    .run_if(in_state(AppState::Connecting)),
            )
            .add_systems(Update, receive_refused.run_if(in_state(AppState::InGame)))
            .add_systems(FixedPreUpdate, restore_predicted_position)
            .add_systems(
                PostUpdate,
                place_remote_characters.before(TransformSystems::Propagate),
            );
    }
}

/// Our link to the server.
#[derive(Component)]
pub(super) struct NetClient {
    server: SocketAddr,
    started: f32,
    /// The connection attempt has begun (a new link starts out
    /// `Disconnected`, which isn't a failure).
    attempted: bool,
}

/// The server we joined.
#[derive(Resource, Debug, Clone)]
pub struct ServerInfo {
    pub name: String,
    pub address: SocketAddr,
}

/// `--net-latency/--net-jitter/--net-loss` as a Lightyear receive
/// conditioner (applies to everything this instance receives).
pub(super) fn conditioner(launch: &Launch) -> Option<RecvLinkConditioner> {
    let c = launch.conditioner?;
    Some(RecvLinkConditioner::new(LinkConditionerConfig {
        incoming_latency: Duration::from_millis(c.latency_ms.into()),
        incoming_jitter: Duration::from_millis(c.jitter_ms.into()),
        good_loss: c.loss_percent / 100.0,
        bad_loss: c.loss_percent / 100.0,
        good_to_bad: 0.0,
        bad_to_good: 1.0,
    }))
}

fn start_client(
    mode: Res<NetMode>,
    launch: Res<Launch>,
    time: Res<Time>,
    mut notice: ResMut<MenuNotice>,
    mut next: ResMut<NextState<AppState>>,
    mut commands: Commands,
) {
    let NetMode::Client { server } = *mode else {
        next.set(AppState::MainMenu);
        return;
    };
    // Any unique id will do: the dev key lets clients make their own
    // connect tokens.
    let client_id = RandomState::new().hash_one(std::time::SystemTime::now()) | 1;
    let auth = Authentication::Manual {
        server_addr: server,
        client_id,
        private_key: DEV_PRIVATE_KEY,
        protocol_id: PROTOCOL_ID,
    };
    let netcode = match NetcodeClient::new(
        auth,
        NetcodeConfig {
            client_timeout_secs: 5,
            token_expire_secs: -1,
            ..default()
        },
    ) {
        Ok(netcode) => netcode,
        Err(err) => {
            notice.0 = Some(format!("Can't connect: {err}"));
            next.set(AppState::MainMenu);
            return;
        }
    };
    commands.insert_resource(PredictionManager::default());
    let client = commands
        .spawn((
            Name::new("Client"),
            NetClient {
                server,
                started: time.elapsed_secs(),
                attempted: false,
            },
            Client,
            ReplicationReceiver,
            Link::default().with_conditioner(conditioner(&launch)),
            LocalAddr(SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0)),
            PeerAddr(server),
            netcode,
            UdpIo::default(),
        ))
        .id();
    commands.trigger(Connect { entity: client });
    info!("net: connecting to {server} (build {BUILD_ID})");
}

fn send_hello(
    trigger: On<Add<Connected>>,
    mut clients: Query<&mut MessageSender<Hello>, With<NetClient>>,
    launch: Res<Launch>,
) {
    let Ok(mut sender) = clients.get_mut(trigger.entity) else {
        return;
    };
    info!("net: connected, saying hello");
    sender.send::<ControlChannel>(Hello {
        build: BUILD_ID.to_string(),
        name: launch.name.clone().unwrap_or_else(default_name),
    });
}

/// The user's login name, else "Player".
fn default_name() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "Player".into())
}

fn receive_welcome(
    mut clients: Query<(Entity, &NetClient, &mut MessageReceiver<Welcome>)>,
    mut notice: ResMut<MenuNotice>,
    mut next: ResMut<NextState<AppState>>,
    mut commands: Commands,
) {
    for (entity, client, mut receiver) in &mut clients {
        for welcome in receiver.receive() {
            let ours = crate::map::current_map_name();
            if welcome.map != ours {
                notice.0 = Some(format!(
                    "{} runs map \"{}\" (you have \"{ours}\"): start the game with --map {}",
                    welcome.server_name, welcome.map, welcome.map
                ));
                commands.trigger(Disconnect { entity });
                next.set(AppState::MainMenu);
                return;
            }
            info!(
                "net: joined \"{}\" at {}",
                welcome.server_name, client.server
            );
            commands.insert_resource(ServerInfo {
                name: welcome.server_name,
                address: client.server,
            });
            commands.insert_resource(ServerTuning(welcome.tuning));
            next.set(AppState::InGame);
        }
    }
}

fn receive_refused(
    mut clients: Query<(Entity, &mut MessageReceiver<Refused>), With<NetClient>>,
    mut notice: ResMut<MenuNotice>,
    mut next: ResMut<NextState<AppState>>,
    mut commands: Commands,
) {
    for (entity, mut receiver) in &mut clients {
        for refused in receiver.receive() {
            warn!("net: refused: {}", refused.reason);
            notice.0 = Some(refused.reason);
            commands.trigger(Disconnect { entity });
            next.set(AppState::MainMenu);
        }
    }
}

fn connect_timeout(
    clients: Query<&NetClient>,
    time: Res<Time>,
    mut notice: ResMut<MenuNotice>,
    mut next: ResMut<NextState<AppState>>,
) {
    for client in &clients {
        if time.elapsed_secs() - client.started > 12.0 {
            notice.0 = Some(format!(
                "Server not responding ({}). Check the address and port, that the server \
                 is running, and its firewall (UDP).",
                client.server
            ));
            next.set(AppState::MainMenu);
        }
    }
}

fn attempt_started(trigger: On<Add<Connecting>>, mut clients: Query<&mut NetClient>) {
    if let Ok(mut client) = clients.get_mut(trigger.entity) {
        client.attempted = true;
    }
}

/// The link went down (server closed, timed out, kicked).
fn link_lost(
    trigger: On<Add<Disconnected>>,
    clients: Query<(&NetClient, &Disconnected)>,
    state: Res<State<AppState>>,
    mut notice: ResMut<MenuNotice>,
    mut next: ResMut<NextState<AppState>>,
) {
    let Ok((client, disconnected)) = clients.get(trigger.entity) else {
        return;
    };
    if *state.get() == AppState::MainMenu || !client.attempted {
        return;
    }
    let reason = disconnected.reason.to_string();
    warn!("net: disconnected from {}: {reason}", client.server);
    if notice.0.is_none() {
        notice.0 = Some(match *state.get() {
            AppState::Connecting => format!(
                "Server not responding ({}): {reason}. Check the address, the port and \
                 the server's firewall (UDP).",
                client.server
            ),
            _ => format!("Disconnected from the server: {reason}"),
        });
    }
    next.set(AppState::MainMenu);
}

/// Back at the menu: drop the link and everything the server gave us.
fn close_client(
    clients: Query<Entity, With<NetClient>>,
    replicated: Query<Entity, With<Remote>>,
    mut mode: ResMut<NetMode>,
    time: Res<Time>,
    mut commands: Commands,
) {
    for client in &clients {
        commands.trigger(Disconnect { entity: client });
        // Keep the link a moment so the disconnect packets go out (the
        // server sees "left" at once instead of a timeout).
        commands
            .entity(client)
            .remove::<NetClient>()
            .insert(super::DespawnAt(time.elapsed_secs() + 0.5));
    }
    for entity in &replicated {
        commands.entity(entity).try_despawn();
    }
    commands.remove_resource::<PredictionManager>();
    commands.remove_resource::<ServerInfo>();
    commands.remove_resource::<ServerTuning>();
    if *mode != NetMode::Offline {
        *mode = NetMode::Offline;
    }
}

/// Marks a character whose hit zones exist on this machine.
#[derive(Component)]
struct HasHitZones;

/// Our own character arrived: make it the local player.
fn control_character(
    trigger: On<Add<Controlled>>,
    characters: Query<(Option<&SimPosition>, Option<&NetAim>, Has<HasHitZones>)>,
    mut commands: Commands,
) {
    let Ok((position, aim, zoned)) = characters.get(trigger.entity) else {
        return;
    };
    let yaw = aim.map_or(0.0, |a| a.yaw);
    let feet = position.map_or(Vec3::ZERO, |p| p.0);
    info!("net: our character is {} at {feet:.2}", trigger.entity);
    let mut entity = commands.entity(trigger.entity);
    if !zoned {
        entity.insert(HasHitZones).with_children(spawn_hitboxes);
    }
    entity.insert((
        LocalPlayer,
        PlayerInput { yaw, ..default() },
        InputMarker::<NetInput>::default(),
        DestructionIntent::default(),
        Transform::from_translation(feet).with_rotation(Quat::from_rotation_y(yaw)),
        Visibility::default(),
        RigidBody::Kinematic,
        CustomPositionIntegration,
        TransformInterpolation,
    ));
}

/// Someone else's character arrived: give it a place in the world (its
/// model comes from `characters/body.rs`) and hit zones posed from its
/// state.
fn watch_character(
    trigger: On<Add<NetPlayer>>,
    characters: Query<(
        Has<Controlled>,
        Has<Remote>,
        Has<HasHitZones>,
        Option<&SimPosition>,
    )>,
    mut commands: Commands,
) {
    let Ok((controlled, remote, zoned, position)) = characters.get(trigger.entity) else {
        return;
    };
    if controlled || !remote || zoned {
        return;
    }
    commands
        .entity(trigger.entity)
        .insert((
            HasHitZones,
            Transform::from_translation(position.map_or(Vec3::ZERO, |p| p.0)),
            Visibility::default(),
        ))
        .with_children(spawn_hitboxes);
}

/// Start of every tick (including replayed ones): the predicted feet
/// position is the truth.
fn restore_predicted_position(
    mut players: Query<(&SimPosition, &mut Transform), With<Controlled>>,
) {
    for (sim, mut transform) in &mut players {
        if transform.translation != sim.0 {
            transform.translation = sim.0;
        }
    }
}

/// Every frame: interpolated state -> `Transform` of other players.
fn place_remote_characters(
    mut characters: Query<
        (&SimPosition, Option<&NetAim>, &mut Transform),
        (With<NetPlayer>, With<Remote>, Without<Controlled>),
    >,
) {
    for (sim, aim, mut transform) in &mut characters {
        transform.translation = sim.0;
        if let Some(aim) = aim {
            transform.rotation = Quat::from_rotation_y(aim.yaw);
        }
    }
}
