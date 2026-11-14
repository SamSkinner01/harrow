//! Match state and objective/gadget entities.
//!
//! - The server owns [`Round`]; a replicated entity carries a copy
//!   ([`RoundSync`]) that clients copy back into their `Round` resource,
//!   so the HUD, sounds and `RoundPhase` state work unchanged on clients.
//!   The round loop itself never runs on a client.
//! - Defusers (planted and dropped) and deployed gadgets (sensor darts,
//!   seismic stakes) spawned by the server are replicated (component state
//!   and `Transform`); clients attach the same models the server uses.

use bevy::prelude::*;
use lightyear::prelude::*;

use super::RoundSync;
use crate::{
    gadgets::{
        DartState, GadgetAssets, SeismicStake, SensorDart, attach_dart_model, attach_stake_model,
    },
    round::{
        Round,
        defuser::{DefuserAssets, DefuserDevice, DroppedDefuser, attach_device_model},
    },
    session::{AppState, NetMode, is_client, is_server},
};

pub(super) struct RoundSyncPlugin;

impl Plugin for RoundSyncPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::InGame),
            spawn_round_sync.run_if(is_server),
        )
        .add_systems(OnExit(AppState::InGame), despawn_round_sync)
        .add_systems(Update, publish_round.run_if(is_server))
        .add_systems(
            PreUpdate,
            apply_round
                .run_if(is_client)
                .after(ReplicationSystems::Receive),
        )
        .add_systems(Update, show_dart_lights.run_if(is_client))
        .add_observer(replicate_spawned::<DefuserDevice>)
        .add_observer(replicate_spawned::<DroppedDefuser>)
        .add_observer(replicate_spawned::<SensorDart>)
        .add_observer(replicate_spawned::<SeismicStake>)
        .add_observer(device_arrived)
        .add_observer(dropped_arrived)
        .add_observer(dart_arrived)
        .add_observer(stake_arrived);
    }
}

fn spawn_round_sync(round: Res<Round>, mut commands: Commands) {
    commands.spawn((
        Name::new("Round sync"),
        RoundSync(round.clone()),
        Replicate::to_clients(NetworkTarget::All),
    ));
}

fn despawn_round_sync(sync: Query<Entity, With<RoundSync>>, mut commands: Commands) {
    for entity in &sync {
        commands.entity(entity).try_despawn();
    }
}

fn publish_round(round: Res<Round>, mut sync: Query<&mut RoundSync>) {
    if !round.is_changed() {
        return;
    }
    for mut sync in &mut sync {
        if sync.0 != *round {
            sync.0 = round.clone();
        }
    }
}

fn apply_round(sync: Query<Ref<RoundSync>>, mut round: ResMut<Round>) {
    for sync in &sync {
        if sync.is_changed() && *round != sync.0 {
            *round = sync.0.clone();
        }
    }
}

/// Server: objective and gadget entities are replicated to everyone. Their
/// mesh children are local (clients build their own).
pub(super) fn replicate_spawned<C: Component>(
    trigger: On<Add<C>>,
    mode: Res<NetMode>,
    mut commands: Commands,
) {
    if mode.is_server() {
        commands.entity(trigger.entity).insert_if_new((
            Replicate::to_clients(NetworkTarget::All),
            DisableReplicateHierarchy,
        ));
    }
}

fn device_arrived(
    trigger: On<Add<DefuserDevice>>,
    remote: Query<(), With<Remote>>,
    assets: Res<DefuserAssets>,
    mut commands: Commands,
) {
    if remote.contains(trigger.entity) {
        attach_device_model(&mut commands, &assets, trigger.entity);
    }
}

fn dropped_arrived(
    trigger: On<Add<DroppedDefuser>>,
    remote: Query<(), With<Remote>>,
    assets: Res<DefuserAssets>,
    mut commands: Commands,
) {
    if remote.contains(trigger.entity) {
        attach_device_model(&mut commands, &assets, trigger.entity);
    }
}

fn dart_arrived(
    trigger: On<Add<SensorDart>>,
    darts: Query<&SensorDart, With<Remote>>,
    assets: Res<GadgetAssets>,
    mut commands: Commands,
) {
    if let Ok(dart) = darts.get(trigger.entity) {
        let listening = matches!(dart.state, DartState::Listening { .. });
        attach_dart_model(&mut commands, &assets, trigger.entity, listening);
    }
}

fn stake_arrived(
    trigger: On<Add<SeismicStake>>,
    remote: Query<(), With<Remote>>,
    assets: Res<GadgetAssets>,
    mut commands: Commands,
) {
    if remote.contains(trigger.entity) {
        attach_stake_model(&mut commands, &assets, trigger.entity);
    }
}

/// A replicated dart's tail light comes on when it sticks.
fn show_dart_lights(
    darts: Query<(&SensorDart, &Children), (With<Remote>, Changed<SensorDart>)>,
    mut lights: Query<&mut Visibility, With<crate::gadgets::DartLight>>,
) {
    for (dart, children) in &darts {
        let on = matches!(dart.state, DartState::Listening { .. });
        for child in children.iter() {
            if let Ok(mut visibility) = lights.get_mut(child) {
                visibility.set_if_neq(if on {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                });
            }
        }
    }
}
