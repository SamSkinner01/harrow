//! Fathom's seismic stakes.
//!
//! Key 3 hammers a stake into the floor `place_distance` in front of you
//! (a ray straight down must find a floor). Stakes are passive: every
//! attacker footstep at least `min_loudness` loud (walking or running, not
//! crouch-walking) within `radius` metres sideways and `vertical_reach`
//! metres up or down (so the floor above or below too) pings its position
//! to all defenders, at most once per `ping_interval` per attacker and
//! stake. A [`Noise`] at least `saturate_loudness` loud within
//! `saturate_radius` blinds a stake for `saturate_time` seconds. Stakes are
//! visible and can be shot (`Health`).

use avian3d::prelude::*;
use bevy::{platform::collections::HashMap, prelude::*};

use super::{
    DeployedGadget, GadgetAssets, GadgetEvent, GadgetIntent, GadgetKind, GadgetOwner,
    GadgetSystems, Reveal, RevealSource, UniqueGadget, take_gadget_use,
};
use crate::{
    combat::{Dead, Health, Noise, NoiseKind, Team},
    config::Tuning,
    physics::GameLayer,
    player::{Footstep, Player, PlayerInput},
    round::{Round, RoundReset, other_team},
};

pub struct StakesPlugin;

impl Plugin for StakesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (place_stakes, saturate, detect).chain().in_set(GadgetSystems),
        )
        .add_systems(Update, show_saturation);
    }
}

#[derive(Component, Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SeismicStake {
    /// Seconds the stake stays blinded by a loud noise. Feed it through a
    /// loud [`Noise`] message (explosions, ring cutter, gunfire).
    pub saturated_for: f32,
}

/// The stake's status light: green when listening, amber when saturated.
#[derive(Component)]
struct StakeLight;

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn place_stakes(
    round: Res<Round>,
    tuning: Res<Tuning>,
    assets: Res<GadgetAssets>,
    spatial: SpatialQuery,
    mut users: Query<
        (
            Entity,
            &Transform,
            &PlayerInput,
            &Team,
            &mut GadgetIntent,
            &mut UniqueGadget,
            Has<Dead>,
        ),
        With<Player>,
    >,
    mut events: MessageWriter<GadgetEvent>,
    mut noise: MessageWriter<Noise>,
    mut commands: Commands,
) {
    let filter = SpatialQueryFilter::from_mask(GameLayer::World);
    for (entity, transform, input, team, mut intent, mut gadget, dead) in &mut users {
        if !take_gadget_use(entity, &mut intent, &gadget, GadgetKind::SeismicStake, *team, dead, &round, &mut events) {
            continue;
        }
        let forward = Quat::from_rotation_y(input.yaw) * Vec3::NEG_Z;
        let above = transform.translation + forward * tuning.round.stakes.place_distance + Vec3::Y * 0.6;
        let floor = spatial
            .cast_ray(above, Dir3::NEG_Y, 1.5, true, &filter)
            .filter(|hit| hit.normal.y > 0.75)
            .map(|hit| above - Vec3::Y * hit.distance);
        let Some(point) = floor else {
            events.write(GadgetEvent::Refused {
                by: entity,
                reason: "no floor there for a stake".into(),
            });
            continue;
        };
        gadget.remaining -= 1;
        spawn_stake(&mut commands, &assets, &tuning, point, *team, Some(entity));
        noise.write(Noise {
            source: Some(entity),
            position: point,
            loudness: 0.5,
            kind: NoiseKind::Gadget,
        });
        events.write(GadgetEvent::StakePlaced {
            by: entity,
            position: point,
        });
    }
}

/// Height of the stake's hit volume centre above the floor.
const HIT_CENTRE: f32 = 0.12;

/// A stake standing in the floor at `point` (also used by test scripts).
/// The entity sits at the centre of its hit volume, so bullets that hit
/// the collider hit the entity with the `Health`.
pub(crate) fn spawn_stake(
    commands: &mut Commands,
    assets: &GadgetAssets,
    tuning: &Tuning,
    point: Vec3,
    team: Team,
    owner: Option<Entity>,
) -> Entity {
    let stake = commands
        .spawn((
            Name::new("Seismic stake"),
            SeismicStake::default(),
            GadgetOwner { entity: owner, team },
            DeployedGadget,
            Health::new(tuning.round.stakes.health),
            Transform::from_translation(point + Vec3::Y * HIT_CENTRE),
            Visibility::default(),
            // Hit volume around the exposed part, for bullets.
            Collider::capsule(0.06, 0.12),
            Sensor,
            CollisionLayers::new(GameLayer::Hitbox, LayerMask::NONE),
        ))
        .id();
    attach_stake_model(commands, assets, stake);
    stake
}

/// The stake's model (spike, head, status light) under `stake` (also used
/// for stakes replicated to clients).
pub(crate) fn attach_stake_model(commands: &mut Commands, assets: &GadgetAssets, stake: Entity) {
    let a = assets;
    commands
        .entity(stake)
        .insert_if_new(Visibility::default())
        .with_children(|parent| {
            // Spike half buried, head and light on top.
            parent.spawn((
                Mesh3d(a.stake_spike.clone()),
                MeshMaterial3d(a.steel.clone()),
                Transform::from_xyz(0.0, 0.05 - HIT_CENTRE, 0.0),
            ));
            parent.spawn((
                Mesh3d(a.stake_head.clone()),
                MeshMaterial3d(a.dark_metal.clone()),
                Transform::from_xyz(0.0, 0.2 - HIT_CENTRE, 0.0),
            ));
            parent.spawn((
                Mesh3d(a.stake_light.clone()),
                MeshMaterial3d(a.stake_glow.clone()),
                Transform::from_xyz(0.0, 0.225 - HIT_CENTRE, 0.0),
                StakeLight,
            ));
        });
}

/// Loud noises blind nearby stakes; blindness wears off.
fn saturate(
    tuning: Res<Tuning>,
    time: Res<Time>,
    mut noises: MessageReader<Noise>,
    mut stakes: Query<(Entity, &Transform, &mut SeismicStake)>,
    mut events: MessageWriter<GadgetEvent>,
) {
    let tune = &tuning.round.stakes;
    let loud: Vec<Vec3> = noises
        .read()
        .filter(|n| n.loudness >= tune.saturate_loudness)
        .map(|n| n.position)
        .collect();
    for (entity, transform, mut stake) in &mut stakes {
        stake.saturated_for = (stake.saturated_for - time.delta_secs()).max(0.0);
        if loud
            .iter()
            .any(|p| p.distance(transform.translation) <= tune.saturate_radius)
        {
            if stake.saturated_for <= 0.0 {
                events.write(GadgetEvent::StakeSaturated { stake: entity });
            }
            stake.saturated_for = tune.saturate_time;
        }
    }
}

/// Footsteps near a stake ping the stake's team.
#[allow(clippy::too_many_arguments)]
fn detect(
    tuning: Res<Tuning>,
    time: Res<Time>,
    stakes: Query<(Entity, &Transform, &SeismicStake, &GadgetOwner), Without<Dead>>,
    teams: Query<&Team>,
    mut footsteps: MessageReader<Footstep>,
    mut resets: MessageReader<RoundReset>,
    mut reveals: MessageWriter<Reveal>,
    mut events: MessageWriter<GadgetEvent>,
    mut last_ping: Local<HashMap<(Entity, Entity), f32>>,
) {
    if resets.read().count() > 0 {
        last_ping.clear();
    }
    let tune = &tuning.round.stakes;
    let now = time.elapsed_secs();
    for step in footsteps.read() {
        if step.loudness < tune.min_loudness {
            continue;
        }
        let Ok(walker_team) = teams.get(step.entity) else {
            continue;
        };
        for (stake, at, state, owner) in &stakes {
            if *walker_team != other_team(owner.team) || state.saturated_for > 0.0 {
                continue;
            }
            let offset = step.position - (at.translation - Vec3::Y * HIT_CENTRE);
            if offset.with_y(0.0).length() > tune.radius || offset.y.abs() > tune.vertical_reach {
                continue;
            }
            let last = last_ping.entry((stake, step.entity)).or_insert(f32::NEG_INFINITY);
            if now - *last < tune.ping_interval {
                continue;
            }
            *last = now;
            reveals.write(Reveal {
                for_team: owner.team,
                position: step.position,
                source: RevealSource::SeismicStake,
                revealed: Some(step.entity),
            });
            events.write(GadgetEvent::StakePing {
                stake,
                position: step.position,
                team: owner.team,
            });
            // One ping per footstep, however many stakes feel it.
            break;
        }
    }
}

/// Status light colour.
fn show_saturation(
    assets: Res<GadgetAssets>,
    stakes: Query<(&SeismicStake, &Children)>,
    mut lights: Query<&mut MeshMaterial3d<StandardMaterial>, With<StakeLight>>,
) {
    for (stake, children) in &stakes {
        let wanted = if stake.saturated_for > 0.0 {
            &assets.stake_saturated
        } else {
            &assets.stake_glow
        };
        for child in children.iter() {
            if let Ok(mut material) = lights.get_mut(child)
                && material.0 != *wanted
            {
                material.0 = wanted.clone();
            }
        }
    }
}
