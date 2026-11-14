//! The defuser objective.
//!
//! - One attacker carries the defuser (handed out when prep starts; the
//!   local player first). Key 5 equips it; holding F inside a site also
//!   equips it.
//! - Plant: hold F inside a bomb site during the action phase for
//!   `timers.plant` seconds. Releasing F, leaving the site or dying cancels
//!   it and progress resets. The planter can't walk while planting.
//! - Once planted the defuser runs for `timers.defuser` seconds.
//! - Disable: a defender holds F within `disable_reach` of the device for
//!   `timers.disable` seconds (same interruptions; one defender at a time).
//! - A carrier who dies drops the defuser; any living attacker who walks
//!   over it picks it up.
//!
//! The device is an original procedural model: a ribbed twin-canister unit
//! on an olive base plate, with an amber read-out and a whip mast whose red
//! tip blinks faster as time runs out.

use bevy::{ecs::entity::EntityHashSet, prelude::*};

use super::{
    InteractionKind, InterruptReason, ObjectiveInteraction, Round, RoundEvent, RoundIntent,
    RoundPhase, RoundSettings, RoundSystems, WinReason,
    sites::{KeepLayout, SiteId},
};
use crate::{
    combat::{Dead, Team},
    config::Tuning,
    player::{LocalPlayer, Player},
};

pub struct DefuserPlugin;

impl Plugin for DefuserPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, create_defuser_assets)
            .add_systems(
                FixedUpdate,
                (
                    (equip_defuser, plant_defuser, disable_defuser, drop_and_pick_up)
                        .chain()
                        .in_set(RoundSystems::Objective),
                    (clear_objective, hand_out_defuser)
                        .chain()
                        .in_set(RoundSystems::Reactions),
                ),
            )
            .add_systems(Update, blink_device);
    }
}

/// On the attacker carrying the defuser.
#[derive(Component, Debug, Clone)]
pub struct DefuserCarrier {
    /// In hand (weapons should be holstered) rather than on the back.
    pub equipped: bool,
}

/// A planted defuser.
#[derive(Component, Debug, Clone)]
pub struct DefuserDevice {
    pub site: SiteId,
    /// 0..1, by the defender currently disabling it.
    pub disable_progress: f32,
    pub disabler: Option<Entity>,
    pub disabled: bool,
}

/// A defuser lying on the floor after its carrier died.
#[derive(Component, Debug)]
pub struct DroppedDefuser;

/// The blinking tip of the device's mast.
#[derive(Component)]
struct DeviceLight;

#[derive(Resource)]
pub(crate) struct DefuserAssets {
    base: Handle<Mesh>,
    canister: Handle<Mesh>,
    rib: Handle<Mesh>,
    housing: Handle<Mesh>,
    display: Handle<Mesh>,
    mast: Handle<Mesh>,
    tip: Handle<Mesh>,
    olive: Handle<StandardMaterial>,
    metal: Handle<StandardMaterial>,
    dark: Handle<StandardMaterial>,
    amber: Handle<StandardMaterial>,
    red: Handle<StandardMaterial>,
}

fn create_defuser_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut mat = |color: Color, metallic: f32, roughness: f32, emissive: LinearRgba| {
        materials.add(StandardMaterial {
            base_color: color,
            metallic,
            perceptual_roughness: roughness,
            emissive,
            ..default()
        })
    };
    commands.insert_resource(DefuserAssets {
        base: meshes.add(Cuboid::new(0.44, 0.05, 0.30)),
        canister: meshes.add(Cylinder::new(0.065, 0.22)),
        rib: meshes.add(Cylinder::new(0.072, 0.02)),
        housing: meshes.add(Cuboid::new(0.17, 0.10, 0.06)),
        display: meshes.add(Cuboid::new(0.12, 0.045, 0.005)),
        mast: meshes.add(Cylinder::new(0.006, 0.30)),
        tip: meshes.add(Sphere::new(0.018)),
        olive: mat(Color::srgb(0.25, 0.27, 0.19), 0.0, 0.85, LinearRgba::BLACK),
        metal: mat(Color::srgb(0.55, 0.53, 0.47), 0.7, 0.45, LinearRgba::BLACK),
        dark: mat(Color::srgb(0.06, 0.06, 0.06), 0.2, 0.6, LinearRgba::BLACK),
        amber: mat(Color::srgb(1.0, 0.6, 0.1), 0.0, 0.4, LinearRgba::rgb(4.0, 1.8, 0.2)),
        red: mat(Color::srgb(1.0, 0.1, 0.05), 0.0, 0.4, LinearRgba::rgb(120.0, 4.0, 2.0)),
    });
}

/// Spawn the device model. `position` is on the floor.
pub(crate) fn spawn_device_model(
    commands: &mut Commands,
    assets: &DefuserAssets,
    position: Vec3,
    yaw: f32,
    bundle: impl Bundle,
) -> Entity {
    let a = assets;
    let part = |mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, at: Vec3| {
        (
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(at),
        )
    };
    let root = commands
        .spawn((
            bundle,
            Transform::from_translation(position).with_rotation(Quat::from_rotation_y(yaw)),
            Visibility::default(),
        ))
        .id();
    let mut children = vec![
        commands.spawn(part(&a.base, &a.olive, Vec3::Y * 0.025)).id(),
        commands.spawn(part(&a.housing, &a.dark, Vec3::new(0.0, 0.10, 0.11))).id(),
        commands.spawn(part(&a.display, &a.amber, Vec3::new(0.0, 0.11, 0.1425))).id(),
        commands.spawn(part(&a.mast, &a.dark, Vec3::new(0.17, 0.20, 0.10))).id(),
        commands
            .spawn((part(&a.tip, &a.red, Vec3::new(0.17, 0.355, 0.10)), DeviceLight))
            .id(),
    ];
    for x in [-0.1, 0.1] {
        children.push(commands.spawn(part(&a.canister, &a.metal, Vec3::new(x, 0.16, -0.03))).id());
        for y in [0.09, 0.16, 0.23] {
            children.push(commands.spawn(part(&a.rib, &a.dark, Vec3::new(x, y, -0.03))).id());
        }
    }
    commands.entity(root).add_children(&children);
    root
}

fn equip_defuser(
    mut carriers: Query<(Entity, &mut RoundIntent, Option<&mut DefuserCarrier>), With<Player>>,
    mut events: MessageWriter<RoundEvent>,
) {
    for (entity, mut intent, carrier) in &mut carriers {
        if !std::mem::take(&mut intent.equip_defuser) {
            continue;
        }
        match carrier {
            Some(mut carrier) => {
                carrier.equipped = !carrier.equipped;
                events.write(RoundEvent::DefuserEquipped {
                    entity,
                    equipped: carrier.equipped,
                });
            }
            None => {
                events.write(RoundEvent::PlantRefused {
                    entity,
                    reason: "not carrying the defuser",
                });
            }
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn plant_defuser(
    mut round: ResMut<Round>,
    settings: Res<RoundSettings>,
    tuning: Res<Tuning>,
    time: Res<Time>,
    layout: Res<KeepLayout>,
    assets: Res<DefuserAssets>,
    mut planters: Query<
        (
            Entity,
            &Transform,
            &RoundIntent,
            &Team,
            Option<&mut DefuserCarrier>,
            Option<&mut ObjectiveInteraction>,
            Has<Dead>,
        ),
        With<Player>,
    >,
    mut was_holding: Local<EntityHashSet>,
    mut events: MessageWriter<RoundEvent>,
    mut commands: Commands,
) {
    let timers = settings.timers(&tuning.round).clone();
    for (entity, transform, intent, team, carrier, interaction, dead) in &mut planters {
        let pressed = intent.interact && was_holding.insert(entity);
        if !intent.interact {
            was_holding.remove(&entity);
        }
        if *team != Team::Attackers {
            continue;
        }
        let feet = transform.translation;
        let site = layout.site_at(feet);
        let live = round.phase == RoundPhase::Action;
        let planting = interaction.as_ref().is_some_and(|i| i.kind == InteractionKind::Plant);

        let Some(mut carrier) = carrier else {
            if pressed && live && !dead {
                events.write(RoundEvent::PlantRefused {
                    entity,
                    reason: "not carrying the defuser",
                });
            }
            continue;
        };
        let can_plant = live && !dead && intent.interact && site.is_some();
        if !can_plant {
            if planting {
                let reason = if dead {
                    InterruptReason::Died
                } else if !live {
                    InterruptReason::PhaseEnded
                } else if !intent.interact {
                    InterruptReason::Released
                } else {
                    InterruptReason::LeftArea
                };
                let progress = interaction.map_or(0.0, |i| i.progress);
                commands.entity(entity).remove::<ObjectiveInteraction>();
                events.write(RoundEvent::PlantInterrupted { entity, reason, progress });
            } else if pressed && !dead {
                let reason = match round.phase {
                    RoundPhase::Prep => "the defuser can only be planted in the action phase",
                    RoundPhase::Action => "not inside a bomb site",
                    _ => "not now",
                };
                events.write(RoundEvent::PlantRefused { entity, reason });
            }
            continue;
        }
        let site = site.expect("checked above");
        let step = time.delta_secs() / timers.plant.max(0.01);
        let progress = match interaction {
            Some(mut i) if i.kind == InteractionKind::Plant => {
                i.progress += step;
                i.progress
            }
            _ => {
                carrier.equipped = true;
                commands.entity(entity).insert(ObjectiveInteraction {
                    kind: InteractionKind::Plant,
                    progress: step,
                });
                events.write(RoundEvent::PlantStarted { entity, site: site.id });
                step
            }
        };
        if progress >= 1.0 {
            let forward = transform.rotation * Vec3::NEG_Z;
            let at = site.clamp(feet + forward.with_y(0.0) * 0.45);
            let yaw = transform.rotation.to_euler(EulerRot::YXZ).0;
            commands
                .entity(entity)
                .remove::<(ObjectiveInteraction, DefuserCarrier)>();
            plant_device(&mut commands, &assets, &mut round, &timers, site.id, at, yaw, Some(entity), &mut events);
        }
    }
}

/// Put a running defuser at `position` and start the planted phase.
#[allow(clippy::too_many_arguments)]
pub(crate) fn plant_device(
    commands: &mut Commands,
    assets: &DefuserAssets,
    round: &mut Round,
    timers: &crate::config::RoundTimers,
    site: SiteId,
    position: Vec3,
    yaw: f32,
    planter: Option<Entity>,
    events: &mut MessageWriter<RoundEvent>,
) {
    spawn_device_model(
        commands,
        assets,
        position,
        yaw,
        (
            Name::new("Defuser"),
            DefuserDevice {
                site,
                disable_progress: 0.0,
                disabler: None,
                disabled: false,
            },
        ),
    );
    events.write(RoundEvent::Planted {
        entity: planter,
        site,
        position,
    });
    round.enter(RoundPhase::Planted, timers.defuser, events);
}

/// Within reach of the device: close sideways and on the same floor.
pub fn in_reach(feet: Vec3, device: Vec3, reach: f32) -> bool {
    feet.with_y(0.0).distance(device.with_y(0.0)) <= reach && (feet.y - device.y).abs() <= 1.2
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn disable_defuser(
    mut round: ResMut<Round>,
    settings: Res<RoundSettings>,
    tuning: Res<Tuning>,
    time: Res<Time>,
    mut devices: Query<(&Transform, &mut DefuserDevice)>,
    mut defenders: Query<
        (
            Entity,
            &Transform,
            &RoundIntent,
            &Team,
            Option<&mut ObjectiveInteraction>,
            Has<Dead>,
        ),
        (With<Player>, Without<DefuserDevice>),
    >,
    mut events: MessageWriter<RoundEvent>,
    mut commands: Commands,
) {
    let timers = settings.timers(&tuning.round).clone();
    let Some((device_at, mut device)) = devices
        .iter_mut()
        .find(|(_, d)| !d.disabled)
        .map(|(t, d)| (t.translation, d))
    else {
        // No live device: nobody can be disabling.
        for (entity, _, _, _, interaction, _) in &mut defenders {
            if let Some(i) = interaction
                && i.kind == InteractionKind::Disable
            {
                commands.entity(entity).remove::<ObjectiveInteraction>();
            }
        }
        return;
    };
    for (entity, transform, intent, team, interaction, dead) in &mut defenders {
        if *team != Team::Defenders {
            continue;
        }
        let live = round.phase == RoundPhase::Planted;
        let reach = in_reach(transform.translation, device_at, tuning.round.disable_reach);
        let free = device.disabler.is_none_or(|d| d == entity);
        let disabling = interaction.as_ref().is_some_and(|i| i.kind == InteractionKind::Disable);
        let can = live && !dead && intent.interact && reach && free;
        if !can {
            if disabling {
                let reason = if dead {
                    InterruptReason::Died
                } else if !live {
                    InterruptReason::PhaseEnded
                } else if !intent.interact {
                    InterruptReason::Released
                } else {
                    InterruptReason::LeftArea
                };
                let progress = interaction.map_or(0.0, |i| i.progress);
                commands.entity(entity).remove::<ObjectiveInteraction>();
                if device.disabler == Some(entity) {
                    device.disabler = None;
                    device.disable_progress = 0.0;
                }
                events.write(RoundEvent::DisableInterrupted { entity, reason, progress });
            }
            continue;
        }
        let step = time.delta_secs() / timers.disable.max(0.01);
        let progress = match interaction {
            Some(mut i) if i.kind == InteractionKind::Disable => {
                i.progress += step;
                i.progress
            }
            _ => {
                commands.entity(entity).insert(ObjectiveInteraction {
                    kind: InteractionKind::Disable,
                    progress: step,
                });
                events.write(RoundEvent::DisableStarted { entity });
                step
            }
        };
        device.disabler = Some(entity);
        device.disable_progress = progress.min(1.0);
        if progress >= 1.0 {
            device.disabled = true;
            device.disabler = None;
            commands.entity(entity).remove::<ObjectiveInteraction>();
            events.write(RoundEvent::Disabled { entity });
            round.finish(Team::Defenders, WinReason::DefuserDisabled, &timers, &mut events);
            break;
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn drop_and_pick_up(
    round: Res<Round>,
    tuning: Res<Tuning>,
    assets: Res<DefuserAssets>,
    dead_carriers: Query<(Entity, &Transform), (With<DefuserCarrier>, With<Dead>)>,
    dropped: Query<(Entity, &Transform), With<DroppedDefuser>>,
    attackers: Query<(Entity, &Transform, &Team), (With<Player>, Without<Dead>, Without<DefuserCarrier>)>,
    mut events: MessageWriter<RoundEvent>,
    mut commands: Commands,
) {
    for (entity, transform) in &dead_carriers {
        commands.entity(entity).remove::<DefuserCarrier>();
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
    if !matches!(round.phase, RoundPhase::Prep | RoundPhase::Action) {
        return;
    }
    for (item, at) in &dropped {
        let taker = attackers.iter().find(|(_, t, team)| {
            **team == Team::Attackers
                && in_reach(t.translation, at.translation, tuning.round.pickup_reach)
        });
        if let Some((entity, _, _)) = taker {
            commands.entity(item).despawn();
            commands.entity(entity).insert(DefuserCarrier { equipped: false });
            events.write(RoundEvent::DefuserPickedUp { entity });
            break;
        }
    }
}

/// New round: remove last round's devices.
fn clear_objective(
    mut events: MessageReader<RoundEvent>,
    devices: Query<Entity, Or<(With<DefuserDevice>, With<DroppedDefuser>)>>,
    mut commands: Commands,
) {
    let new_round = events.read().any(|e| {
        matches!(e, RoundEvent::PhaseStarted { phase: RoundPhase::OperatorSelect, .. })
    });
    if new_round {
        for entity in &devices {
            commands.entity(entity).despawn();
        }
    }
}

/// Prep starts: give the defuser to an attacker (the local player if it is
/// attacking, otherwise the first one).
fn hand_out_defuser(
    mut events: MessageReader<RoundEvent>,
    players: Query<(Entity, &Team, Has<LocalPlayer>), (With<Player>, Without<Dead>)>,
    mut commands: Commands,
) {
    let prep = events
        .read()
        .any(|e| matches!(e, RoundEvent::PhaseStarted { phase: RoundPhase::Prep, .. }));
    if !prep {
        return;
    }
    let carrier = players
        .iter()
        .filter(|(_, team, _)| **team == Team::Attackers)
        .min_by_key(|&(entity, _, local)| (!local, entity));
    if let Some((entity, _, _)) = carrier {
        commands.entity(entity).insert(DefuserCarrier { equipped: false });
        // Deferred: this system also reads round events.
        commands.write_message(RoundEvent::DefuserAssigned { entity });
    }
}

/// Blink the mast light: once a second, four times a second in the last ten
/// seconds; off once disabled.
fn blink_device(
    round: Res<Round>,
    time: Res<Time>,
    devices: Query<(&DefuserDevice, &Children)>,
    mut lights: Query<&mut Visibility, With<DeviceLight>>,
) {
    for (device, children) in &devices {
        let rate = if round.time_left < 10.0 { 4.0 } else { 1.0 };
        let on = !device.disabled && (time.elapsed_secs() * rate).fract() < 0.35;
        for child in children.iter() {
            if let Ok(mut visibility) = lights.get_mut(child) {
                visibility.set_if_neq(if on { Visibility::Inherited } else { Visibility::Hidden });
            }
        }
    }
}
