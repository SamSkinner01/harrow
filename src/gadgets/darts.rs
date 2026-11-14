//! Lark's sensor darts.
//!
//! Key 3 fires a dart from the wrist launcher along the look direction. It
//! flies (fast, with a little drop), ray-casting each tick, and sticks to the
//! first world surface it hits. A stuck dart listens for `listen_time`
//! seconds: every defender sound within `radius` metres at least
//! `min_loudness` loud (footsteps above crouch-walk, plus `Noise` such as
//! reloads and gadget use) is revealed to all attackers as a ripple at its
//! source, through walls and floors. Defenders nearby hear a hiss when it
//! lands, and can shoot it (it has `Health`).

use avian3d::prelude::*;
use bevy::prelude::*;

use super::{
    DeployedGadget, GadgetAssets, GadgetEvent, GadgetIntent, GadgetKind, GadgetOwner,
    GadgetSystems, Reveal, RevealSource, UniqueGadget, eye_and_look, take_gadget_use,
};
use crate::{
    combat::{Dead, Health, Noise, NoiseKind, Team},
    config::Tuning,
    physics::GameLayer,
    player::{CharacterState, Footstep, Player, PlayerInput},
    round::{Round, other_team},
};

pub struct DartsPlugin;

impl Plugin for DartsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (fire_darts, fly_darts, listen).chain().in_set(GadgetSystems),
        );
    }
}

#[derive(Component, Debug, Clone)]
pub struct SensorDart {
    pub state: DartState,
}

#[derive(Debug, Clone)]
pub enum DartState {
    Flying { velocity: Vec3, age: f32 },
    /// Stuck to a surface; seconds of listening left.
    Listening { left: f32 },
}

/// The glowing tail light (visible while listening).
#[derive(Component)]
pub(super) struct DartLight;

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn fire_darts(
    round: Res<Round>,
    tuning: Res<Tuning>,
    assets: Res<GadgetAssets>,
    mut shooters: Query<
        (
            Entity,
            &Transform,
            &CharacterState,
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
    for (entity, transform, state, input, team, mut intent, mut gadget, dead) in &mut shooters {
        if !take_gadget_use(entity, &mut intent, &gadget, GadgetKind::SensorDart, *team, dead, &round, &mut events) {
            continue;
        }
        gadget.remaining -= 1;
        let (eye, look) = eye_and_look(transform, state, input, &tuning.body, tuning.lean.offset);
        // Launched from the wrist: a little right of and below the eye.
        let right = Quat::from_rotation_y(input.yaw) * Vec3::X;
        let start = eye + right * 0.15 - Vec3::Y * 0.12 + *look * 0.2;
        let dart = commands
            .spawn((
                Name::new("Sensor dart"),
                SensorDart {
                    state: DartState::Flying {
                        velocity: *look * tuning.round.darts.speed,
                        age: 0.0,
                    },
                },
                GadgetOwner {
                    entity: Some(entity),
                    team: *team,
                },
                Transform::from_translation(start).with_rotation(align_tail(*look)),
                Visibility::default(),
            ))
            .id();
        spawn_dart_model(&mut commands, &assets, dart, false);
        noise.write(Noise {
            source: Some(entity),
            position: eye,
            loudness: 0.3,
            kind: NoiseKind::Gadget,
        });
        events.write(GadgetEvent::DartFired { by: entity });
    }
}

/// Rotation that points the dart's tail (+Y of the cylinder) back along
/// its flight, so the tip leads.
fn align_tail(direction: Vec3) -> Quat {
    Quat::from_rotation_arc(Vec3::Y, -direction.normalize_or(Vec3::NEG_Z))
}

fn spawn_dart_model(commands: &mut Commands, assets: &GadgetAssets, dart: Entity, listening: bool) {
    let a = assets;
    commands.entity(dart).with_children(|parent| {
        parent.spawn((
            Mesh3d(a.dart_body.clone()),
            MeshMaterial3d(a.dark_metal.clone()),
            Transform::from_xyz(0.0, 0.065, 0.0),
        ));
        for angle in [0.0, std::f32::consts::FRAC_PI_2] {
            parent.spawn((
                Mesh3d(a.dart_fin.clone()),
                MeshMaterial3d(a.steel.clone()),
                Transform::from_xyz(0.0, 0.11, 0.0).with_rotation(Quat::from_rotation_y(angle)),
            ));
        }
        parent.spawn((
            Mesh3d(a.dart_light.clone()),
            MeshMaterial3d(a.dart_glow.clone()),
            Transform::from_xyz(0.0, 0.135, 0.0),
            if listening { Visibility::Inherited } else { Visibility::Hidden },
            DartLight,
        ));
    });
}

/// A dart already stuck and listening at `point` (used by `fly_darts` and
/// by test scripts). `direction` is the flight direction into the surface.
pub(crate) fn stick_dart(
    commands: &mut Commands,
    dart: Entity,
    point: Vec3,
    direction: Vec3,
    tuning: &Tuning,
) {
    let darts = &tuning.round.darts;
    commands.entity(dart).insert((
        SensorDart {
            state: DartState::Listening {
                left: darts.listen_time,
            },
        },
        // Tip 3 cm into the surface.
        Transform::from_translation(point + direction.normalize_or_zero() * 0.03)
            .with_rotation(align_tail(direction)),
        DeployedGadget,
        Health::new(darts.health),
        Collider::sphere(0.06),
        Sensor,
        // Only spatial queries (bullets) find it; it never blocks anything.
        CollisionLayers::new(GameLayer::Hitbox, LayerMask::NONE),
    ));
}

/// Spawn an attackers' dart already listening at `point` (test scripts).
pub(crate) fn spawn_listening_dart(
    commands: &mut Commands,
    assets: &GadgetAssets,
    tuning: &Tuning,
    point: Vec3,
    direction: Vec3,
    team: Team,
) -> Entity {
    let dart = commands
        .spawn((
            Name::new("Sensor dart"),
            GadgetOwner { entity: None, team },
            Visibility::default(),
        ))
        .id();
    stick_dart(commands, dart, point, direction, tuning);
    spawn_dart_model(commands, assets, dart, true);
    commands.write_message(GadgetEvent::DartStuck {
        position: point,
        team,
    });
    dart
}

#[allow(clippy::too_many_arguments)]
fn fly_darts(
    tuning: Res<Tuning>,
    time: Res<Time>,
    spatial: SpatialQuery,
    mut darts: Query<(Entity, &mut SensorDart, &mut Transform, &GadgetOwner, &Children)>,
    mut lights: Query<&mut Visibility, With<DartLight>>,
    mut events: MessageWriter<GadgetEvent>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    let tune = &tuning.round.darts;
    let filter = SpatialQueryFilter::from_mask(GameLayer::World);
    for (entity, mut dart, mut transform, owner, children) in &mut darts {
        let DartState::Flying { velocity, age } = &mut dart.state else {
            continue;
        };
        *age += dt;
        velocity.y -= tune.drop * dt;
        let step = *velocity * dt;
        let Ok(direction) = Dir3::new(step) else {
            continue;
        };
        let from = transform.translation;
        if let Some(hit) = spatial.cast_ray(from, direction, step.length(), true, &filter) {
            let point = from + *direction * hit.distance;
            stick_dart(&mut commands, entity, point, *direction, &tuning);
            for child in children.iter() {
                if let Ok(mut visibility) = lights.get_mut(child) {
                    *visibility = Visibility::Inherited;
                }
            }
            events.write(GadgetEvent::DartStuck {
                position: point,
                team: owner.team,
            });
        } else if *age > tune.max_flight {
            commands.entity(entity).despawn();
            events.write(GadgetEvent::DartLost);
        } else {
            transform.translation += step;
            transform.rotation = align_tail(*velocity);
        }
    }
}

/// Stuck darts reveal enemy sounds nearby, then go quiet.
#[allow(clippy::too_many_arguments)]
fn listen(
    tuning: Res<Tuning>,
    time: Res<Time>,
    mut darts: Query<(Entity, &mut SensorDart, &Transform, &GadgetOwner), Without<Dead>>,
    teams: Query<&Team>,
    mut footsteps: MessageReader<Footstep>,
    mut noises: MessageReader<Noise>,
    mut reveals: MessageWriter<Reveal>,
    mut events: MessageWriter<GadgetEvent>,
    mut commands: Commands,
) {
    let tune = &tuning.round.darts;
    // Every sound this tick: (who, where, how loud).
    let sounds: Vec<(Entity, Vec3, f32)> = footsteps
        .read()
        .map(|s| (s.entity, s.position, s.loudness))
        .chain(
            noises
                .read()
                .filter_map(|n| n.source.map(|source| (source, n.position, n.loudness))),
        )
        .filter(|&(_, _, loudness)| loudness >= tune.min_loudness)
        .collect();

    let listening: Vec<(Vec3, Team)> = darts
        .iter()
        .filter(|(_, d, _, _)| matches!(d.state, DartState::Listening { .. }))
        .map(|(_, _, t, owner)| (t.translation, owner.team))
        .collect();
    for (source, position, _) in sounds {
        let Ok(source_team) = teams.get(source) else {
            continue;
        };
        // One reveal per sound, however many darts hear it.
        if let Some(&(_, team)) = listening.iter().find(|(at, team)| {
            *source_team == other_team(*team) && at.distance(position) <= tune.radius
        }) {
            reveals.write(Reveal {
                for_team: team,
                position,
                source: RevealSource::SensorDart,
                revealed: Some(source),
            });
        }
    }

    for (entity, mut dart, transform, _) in &mut darts {
        if let DartState::Listening { left } = &mut dart.state {
            *left -= time.delta_secs();
            if *left <= 0.0 {
                commands.entity(entity).despawn();
                events.write(GadgetEvent::DartExpired {
                    position: transform.translation,
                });
            }
        }
    }
}
