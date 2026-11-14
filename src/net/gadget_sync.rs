//! Gadgets (M6 part two) in a networked session. The server runs every
//! gadget's simulation; clients show it.
//!
//! - **World objects** (replicated to everyone, components registered in
//!   `protocol.rs`): ring cutters and their exposed blades, cut-out plugs
//!   (sent once, animated locally), backplate charges, grenades in flight
//!   and drones (`Transform` interpolated, like other players), smoke
//!   clouds (clients pour out their own puffs), razor wire, ballistic cover
//!   (clients add its collider: their predicted movement must bump into
//!   it, and bullet impacts on it must find it), proximity alarms and CCTV
//!   cameras. The model code in `gadgets/` dresses them on `Added<..>`, on
//!   every machine.
//! - **Per-player state**, replicated to that player only ([`PrivateTo`]):
//!   `Flashed`, the loadout and secondary, breach-charge and drone stock
//!   (operator card), `GadgetActivity` (HUD progress bars), `RemoteView`
//!   and [`ViewSerial`]. Clients don't recompute what the loadout implies
//!   (`stock_from_loadout`, `apply_loadout` skip them).
//! - **One-off happenings** (sounds, blasts, notices): the forwarded
//!   `GadgetEvent`s (`events.rs`); `gadgets/` draws blasts from the events,
//!   so clients get them too.
//!
//! CCTV cameras exist on every machine from startup (the map's spots). A
//! client uses the server's (their `Health` and destruction are the
//! server's): it drops its own while connected and respawns them at the
//! menu.
//!
//! Remote views: the server puts `RemoteView` on the viewer; the client
//! freezes its body like the server does (`gadgets/remote_view.rs`) and
//! switches cameras. Look angles in `DroneIntent` are absolute and start
//! from the drone's or camera's resting direction, so the client resets
//! them when a new view arrives and echoes its [`ViewSerial`] in its input
//! (`input.rs`); the server ignores look angles meant for an older view.

use bevy::prelude::*;
use bevy_replicon::prelude::{AuthorizedClient, VisibilityFilter};
use lightyear::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    config::Tuning,
    destruction::BreachCharges,
    round::operators::Loadout,
    gadgets::{
        BackplateCharge, BallisticCover, CctvCamera, CctvLayout, CutterBlade, Drone, DroneIntent,
        FallingPlug, Flashed, GadgetActivity, Grenade, ProximityAlarm, RazorWire, ReconStock,
        RemoteView, RingCutter, SecondaryGadget, SecondaryStock, SmokeCloud, cover_body,
        spawn_cctv_cameras,
    },
    player::LocalPlayer,
    session::{AppState, NetMode, is_client, is_server},
};

pub(super) struct GadgetSyncPlugin;

impl Plugin for GadgetSyncPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LocalCctvRemoved>()
            // Server: gadget entities are replicated as they appear.
            .add_observer(super::round_sync::replicate_spawned::<RingCutter>)
            .add_observer(super::round_sync::replicate_spawned::<CutterBlade>)
            .add_observer(super::round_sync::replicate_spawned::<FallingPlug>)
            .add_observer(super::round_sync::replicate_spawned::<BackplateCharge>)
            .add_observer(super::round_sync::replicate_spawned::<SmokeCloud>)
            .add_observer(super::round_sync::replicate_spawned::<RazorWire>)
            .add_observer(super::round_sync::replicate_spawned::<BallisticCover>)
            .add_observer(super::round_sync::replicate_spawned::<ProximityAlarm>)
            .add_observer(super::round_sync::replicate_spawned::<CctvCamera>)
            .add_observer(replicate_moving::<Grenade>)
            .add_observer(replicate_moving::<Drone>)
            .add_observer(bump_view_serial)
            .add_systems(
                OnEnter(AppState::InGame),
                (replicate_existing_cameras.run_if(is_server), drop_local_cameras.run_if(is_client)),
            )
            .add_systems(OnEnter(AppState::MainMenu), restore_local_cameras)
            // Client: what arrives needs a place in the world.
            .add_observer(show_arrival::<RingCutter>)
            .add_observer(show_arrival::<CutterBlade>)
            .add_observer(show_arrival::<BackplateCharge>)
            .add_observer(show_arrival::<SmokeCloud>)
            .add_observer(show_arrival::<RazorWire>)
            .add_observer(show_arrival::<ProximityAlarm>)
            .add_observer(show_arrival::<CctvCamera>)
            .add_observer(hide_until_placed::<Grenade>)
            .add_observer(hide_until_placed::<Drone>)
            .add_observer(cover_arrived)
            .add_observer(plug_arrived)
            .add_systems(Update, show_placed.run_if(is_client))
            .add_systems(
                PreUpdate,
                follow_new_view
                    .run_if(is_client)
                    .after(ReplicationSystems::Receive),
            );
    }
}

/// On a character: the client link that owns it. Per-player gadget state
/// (the scope below) is replicated to that client only; a character
/// without it (the host's own) shows it to nobody.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[component(immutable)]
pub struct PrivateTo(pub Entity);

impl VisibilityFilter for PrivateTo {
    type ClientComponent = AuthorizedClient;
    type Scope = (
        Flashed,
        Loadout,
        SecondaryStock,
        BreachCharges,
        ReconStock,
        GadgetActivity,
        RemoteView,
        ViewSerial,
    );

    fn is_visible(&self, client: Entity, _: Option<&AuthorizedClient>) -> bool {
        self.0 == client
    }
}

/// Counts the views a character has entered (server side, replicated to
/// its player). The client echoes it in `DroneButtons::view_serial` once
/// its look angles belong to that view.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewSerial(pub u8);

/// [`Loadout`] on the wire (its weapon ids are `&'static str`).
#[derive(Serialize, Deserialize)]
pub struct NetLoadout {
    primary: String,
    sidearm: String,
    secondary: SecondaryGadget,
}

impl From<Loadout> for NetLoadout {
    fn from(loadout: Loadout) -> Self {
        Self {
            primary: loadout.primary.to_string(),
            sidearm: loadout.sidearm.to_string(),
            secondary: loadout.secondary,
        }
    }
}

impl From<NetLoadout> for Loadout {
    fn from(loadout: NetLoadout) -> Self {
        // The weapon ids M3 has (the operator card shows them).
        let id = |name: &str| match name {
            "rifle" => "rifle",
            "sidearm" => "sidearm",
            _ => "?",
        };
        Self {
            primary: id(&loadout.primary),
            sidearm: id(&loadout.sidearm),
            secondary: loadout.secondary,
        }
    }
}

/// [`GadgetActivity`] on the wire: what the HUD shows (the entity fields
/// are server-side targets, not sent).
#[derive(Serialize, Deserialize)]
pub enum NetActivity {
    Idle,
    Clamping(f32),
    Reseating(f32),
    Bolting(f32),
    Placing(SecondaryGadget, f32),
    Cooking(SecondaryGadget, Option<f32>, f32),
}

impl From<GadgetActivity> for NetActivity {
    fn from(activity: GadgetActivity) -> Self {
        match activity {
            GadgetActivity::Idle => Self::Idle,
            GadgetActivity::Clamping { progress, .. } => Self::Clamping(progress),
            GadgetActivity::Reseating { progress, .. } => Self::Reseating(progress),
            GadgetActivity::Bolting { progress, .. } => Self::Bolting(progress),
            GadgetActivity::Placing { kind, progress } => Self::Placing(kind, progress),
            GadgetActivity::Cooking { kind, fuse, total } => Self::Cooking(kind, fuse, total),
        }
    }
}

impl From<NetActivity> for GadgetActivity {
    fn from(activity: NetActivity) -> Self {
        let none = Entity::PLACEHOLDER;
        match activity {
            NetActivity::Idle => Self::Idle,
            NetActivity::Clamping(progress) => Self::Clamping { surface: none, progress },
            NetActivity::Reseating(progress) => Self::Reseating { cutter: none, progress },
            NetActivity::Bolting(progress) => Self::Bolting { surface: none, progress },
            NetActivity::Placing(kind, progress) => Self::Placing { kind, progress },
            NetActivity::Cooking(kind, fuse, total) => Self::Cooking { kind, fuse, total },
        }
    }
}

/// Server: moving gadgets (grenades, drones) are replicated with an
/// interpolated `Transform`.
fn replicate_moving<C: Component>(trigger: On<Add<C>>, mode: Res<NetMode>, mut commands: Commands) {
    if mode.is_server() {
        commands.entity(trigger.entity).insert_if_new((
            Replicate::to_clients(NetworkTarget::All),
            InterpolationTarget::to_clients(NetworkTarget::All),
            DisableReplicateHierarchy,
        ));
    }
}

/// Server: the CCTV cameras were spawned at startup (before a host chose
/// to host): replicate them now.
fn replicate_existing_cameras(cameras: Query<Entity, (With<CctvCamera>, Without<Replicate>)>, mut commands: Commands) {
    for camera in &cameras {
        commands
            .entity(camera)
            .insert((Replicate::to_clients(NetworkTarget::All), DisableReplicateHierarchy));
    }
}

/// Client: use the server's cameras, not our own.
#[derive(Resource, Default)]
struct LocalCctvRemoved(bool);

fn drop_local_cameras(
    cameras: Query<Entity, (With<CctvCamera>, Without<Remote>)>,
    mut removed: ResMut<LocalCctvRemoved>,
    mut commands: Commands,
) {
    for camera in &cameras {
        commands.entity(camera).despawn();
    }
    removed.0 = true;
}

/// Back at the menu after a networked session: our own cameras again (the
/// server's went with the session).
fn restore_local_cameras(
    mut removed: ResMut<LocalCctvRemoved>,
    layout: Res<CctvLayout>,
    tuning: Res<Tuning>,
    mut commands: Commands,
) {
    if std::mem::take(&mut removed.0) {
        spawn_cctv_cameras(&mut commands, &layout, &tuning);
    }
}

/// Client: a replicated gadget gets visibility for its model (dressed by
/// `gadgets/`).
fn show_arrival<C: Component>(trigger: On<Add<C>>, remote: Query<(), With<Remote>>, mut commands: Commands) {
    if remote.contains(trigger.entity) {
        commands.entity(trigger.entity).insert_if_new(Visibility::default());
    }
}

/// Client: an interpolated gadget has no `Transform` until interpolation
/// starts (about 100 ms): keep its model hidden until then.
fn hide_until_placed<C: Component>(trigger: On<Add<C>>, remote: Query<(), With<Remote>>, mut commands: Commands) {
    if remote.contains(trigger.entity) {
        commands.entity(trigger.entity).insert((Visibility::Hidden, ShowWhenPlaced));
    }
}

#[derive(Component)]
struct ShowWhenPlaced;

fn show_placed(
    mut placed: Query<(Entity, &mut Visibility), (With<ShowWhenPlaced>, Added<Transform>)>,
    mut commands: Commands,
) {
    for (entity, mut visibility) in &mut placed {
        *visibility = Visibility::Inherited;
        commands.entity(entity).remove::<ShowWhenPlaced>();
    }
}

/// Client: ballistic cover is solid world geometry here too.
fn cover_arrived(
    trigger: On<Add<BallisticCover>>,
    remote: Query<(), With<Remote>>,
    tuning: Res<Tuning>,
    mut commands: Commands,
) {
    if remote.contains(trigger.entity) {
        commands
            .entity(trigger.entity)
            .insert((cover_body(&tuning), Visibility::default()));
    }
}

/// Client: a cut-out plug arrives without a `Transform` (it is animated
/// locally from its state: `gadgets/cutter.rs`).
fn plug_arrived(trigger: On<Add<FallingPlug>>, remote: Query<(), With<Remote>>, mut commands: Commands) {
    if remote.contains(trigger.entity) {
        commands
            .entity(trigger.entity)
            .insert((Transform::default(), Visibility::default()));
    }
}

/// Server: every view a character enters (or switches to) gets a new
/// serial.
fn bump_view_serial(
    trigger: On<Insert<RemoteView>>,
    mode: Res<NetMode>,
    serials: Query<&ViewSerial>,
    mut commands: Commands,
) {
    if !mode.is_server() {
        return;
    }
    let next = serials.get(trigger.entity).map_or(1, |s| s.0.wrapping_add(1));
    commands.entity(trigger.entity).insert(ViewSerial(next));
}

/// Client: the server put us in a new view: look from where the drone
/// faces (or the camera's resting direction), like the server does. The
/// serial goes back with our input from now on (`input.rs`).
fn follow_new_view(
    mut local: Query<(&RemoteView, &mut DroneIntent), (With<LocalPlayer>, Changed<ViewSerial>)>,
    drones: Query<&Drone>,
) {
    for (view, mut intent) in &mut local {
        let (yaw, pitch) = match view {
            RemoteView::Drone(drone) => drones.get(*drone).map_or((intent.yaw, 0.0), |d| (d.yaw, d.pitch)),
            RemoteView::Camera(_) => (0.0, 0.0),
        };
        intent.yaw = yaw;
        intent.pitch = pitch;
        intent.drive = Vec2::ZERO;
    }
}
