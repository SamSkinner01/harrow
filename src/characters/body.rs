//! Third-person bodies for characters other than the local player: bots
//! now, remote players (M8) later. Presentation only: it reads simulation
//! state and never changes it.
//!
//! **Which characters.** Every [`Player`] without [`LocalPlayer`] (and
//! without [`NoBody`]) gets a [`Body`]: an operator model (see
//! [`spawn_character`](super::spawn_character)) spawned as a child of the
//! player entity, so it follows the (interpolated) feet transform and yaw.
//! The model is the character's round operator (`round::operators::Operator`)
//! or, before one is picked, its side's default; it is swapped when either
//! changes.
//!
//! **What drives it** (all read every frame; nothing else is needed, so a
//! networked remote player only has to carry these):
//! - `Transform` (feet, yaw), [`CharacterState`] (stance, height and lean,
//!   blended between ticks), `LinearVelocity` (walk/run speed),
//! - [`PlayerInput`] `pitch`: the spine bends to look up and down,
//! - [`WeaponState`]: active weapon (model swap), aiming, firing (recent
//!   shot), reloading, holstering; `hands_busy` lowers it,
//! - [`Team`], [`Dead`]: the body tips over on death and stands up again
//!   when revived.
//!
//! **Weapons.** The prop carbine baked into the operator model is hidden;
//! the real weapon `.glb` of the active slot (the tuning file's
//! `weapons.<slot>.model`, the same file as the first-person view) is
//! attached to the `weapon_r` bone. Bone axes: +Y along the barrel, +Z up,
//! origin at the top of the grip; weapon models point down -Z with +Y up
//! and the origin at the grip, so the model gets a +90 degree turn about X.
//! Shots by these characters ([`ShotFired`]) show a muzzle flash at the
//! model's `MUZZLE` node and a brief tracer.

use std::f32::consts::FRAC_PI_2;

use avian3d::prelude::LinearVelocity;
use bevy::{
    app::AnimationSystems,
    gltf::{GltfAssetLabel, GltfMaterialName},
    light::NotShadowCaster,
    prelude::*,
    world_serialization::{WorldAssetRoot, WorldInstanceReady},
};

use super::{Character, CharacterAnim, CharacterParts, Operator};
use crate::{
    combat::{Dead, Team},
    config::Tuning,
    player::{CharacterState, LocalPlayer, Player, PlayerInput, Stance},
    round::operators::{Operator as RoundOperator, OperatorId},
    weapons::{ShotFired, Slot, WeaponAction, WeaponMaterials, WeaponState},
};

pub struct BodyPlugin;

impl Plugin for BodyPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(prepare_body_weapon)
            .add_systems(
                Update,
                (
                    attach_bodies,
                    swap_models,
                    attach_weapons,
                    drive_bodies,
                    fall_when_dead,
                    shot_effects,
                    fade_effects,
                )
                    .chain(),
            )
            .add_systems(
                PostUpdate,
                bend_spines
                    .after(AnimationSystems)
                    .before(TransformSystems::Propagate),
            );
    }
}

/// A character's third-person body. On the player entity.
#[derive(Component, Debug)]
pub struct Body {
    /// The operator model (a child of the player entity).
    pub model: Entity,
    pub operator: Operator,
    /// The attached weapon model and its slot.
    weapon: Option<(Slot, Entity)>,
    /// 0 upright .. 1 lying (death).
    fall: f32,
    /// Sideways roll of the fall, radians (varies per character).
    fall_roll: f32,
}

/// Put on a [`Player`] that must not get a third-person body.
#[derive(Component, Debug, Default)]
pub struct NoBody;

/// A weapon model attached to a body's hand.
#[derive(Component, Debug)]
pub struct BodyWeapon {
    pub slot: Slot,
    /// The `MUZZLE` node once loaded.
    pub muzzle: Option<Entity>,
}

/// Short-lived shot effect (flash, light or tracer).
#[derive(Component)]
struct ShotEffect {
    left: f32,
}

/// The model for a round operator.
pub fn model_for(op: OperatorId) -> Operator {
    match op {
        OperatorId::Auger => Operator::Auger,
        OperatorId::Lark => Operator::Lark,
        OperatorId::Mortise => Operator::Mortise,
        OperatorId::Fathom => Operator::Fathom,
    }
}

fn wanted_model(team: Team, operator: Option<&RoundOperator>) -> Operator {
    match operator {
        Some(op) if op.0.info().side == team => model_for(op.0),
        _ => match team {
            Team::Attackers => Operator::Auger,
            Team::Defenders => Operator::Mortise,
        },
    }
}

/// The operator model, as a child of the player, with the prop carbine
/// hidden (the real weapon attaches to `weapon_r`).
fn spawn_model(commands: &mut Commands, assets: &AssetServer, player: Entity, operator: Operator) -> Entity {
    commands
        .spawn((
            Name::new(format!("Body ({})", operator.name())),
            Character {
                operator,
                prop_weapon: false,
            },
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(operator.model_path()))),
            Transform::IDENTITY,
            ChildOf(player),
        ))
        .id()
}

#[allow(clippy::type_complexity)]
fn attach_bodies(
    players: Query<
        (Entity, &Team, Option<&RoundOperator>, Has<Visibility>),
        (With<Player>, Without<LocalPlayer>, Without<Body>, Without<NoBody>),
    >,
    assets: Res<AssetServer>,
    mut commands: Commands,
) {
    for (entity, team, operator, visible) in &players {
        let op = wanted_model(*team, operator);
        let model = spawn_model(&mut commands, &assets, entity, op);
        if !visible {
            commands.entity(entity).insert(Visibility::default());
        }
        let roll = ((entity.index_u32() % 7) as f32 - 3.0) * 0.08;
        commands.entity(entity).insert(Body {
            model,
            operator: op,
            weapon: None,
            fall: 0.0,
            fall_roll: roll,
        });
    }
}

#[allow(clippy::type_complexity)]
fn swap_models(
    mut players: Query<
        (Entity, &mut Body, &Team, Option<&RoundOperator>),
        Or<(Changed<RoundOperator>, Changed<Team>)>,
    >,
    assets: Res<AssetServer>,
    mut commands: Commands,
) {
    for (entity, mut body, team, operator) in &mut players {
        let op = wanted_model(*team, operator);
        if op == body.operator {
            continue;
        }
        commands.entity(body.model).despawn();
        body.model = spawn_model(&mut commands, &assets, entity, op);
        body.operator = op;
        body.weapon = None;
    }
}

/// Attach (or swap) the active weapon's model to the `weapon_r` bone.
fn attach_weapons(
    mut players: Query<(&mut Body, &WeaponState)>,
    models: Query<&CharacterParts>,
    assets: Res<AssetServer>,
    tuning: Res<Tuning>,
    mut commands: Commands,
) {
    for (mut body, state) in &mut players {
        if body.weapon.is_some_and(|(slot, _)| slot == state.active) {
            continue;
        }
        let Ok(parts) = models.get(body.model) else { continue };
        let Some(socket) = parts.bone("weapon_r") else { continue };
        if let Some((_, old)) = body.weapon.take() {
            commands.entity(old).despawn();
        }
        let def = state.active.def(&tuning);
        let m = &def.model;
        let (yaw, pitch, roll) = m.rotation;
        let model_rotation = Quat::from_euler(
            EulerRot::YXZ,
            yaw.to_radians(),
            pitch.to_radians(),
            roll.to_radians(),
        );
        let weapon = commands
            .spawn((
                Name::new(format!("Body weapon {}", def.name)),
                BodyWeapon {
                    slot: state.active,
                    muzzle: None,
                },
                WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(m.path.clone()))),
                Transform {
                    translation: Vec3::ZERO,
                    rotation: Quat::from_rotation_x(FRAC_PI_2) * model_rotation,
                    scale: Vec3::splat(m.scale.max(1e-4)),
                },
                ChildOf(socket),
            ))
            .id();
        body.weapon = Some((state.active, weapon));
    }
}

/// Weapon model loaded: shared weapon materials, find the muzzle.
fn prepare_body_weapon(
    ready: On<WorldInstanceReady>,
    mut weapons: Query<&mut BodyWeapon>,
    children: Query<&Children>,
    names: Query<&Name>,
    material_names: Query<&GltfMaterialName>,
    library: Res<WeaponMaterials>,
    tuning: Res<Tuning>,
    mut commands: Commands,
) {
    let Ok(mut weapon) = weapons.get_mut(ready.entity) else {
        return;
    };
    let muzzle_name = weapon.slot.def(&tuning).model.muzzle_node.clone();
    for entity in children.iter_descendants(ready.entity) {
        if let Ok(material) = material_names.get(entity)
            && let Some(handle) = library.get(&material.0)
        {
            commands.entity(entity).insert(MeshMaterial3d(handle));
        }
        if names.get(entity).is_ok_and(|n| n.as_str() == muzzle_name) {
            weapon.muzzle = Some(entity);
        }
    }
}

/// Simulation state -> animation, every frame.
#[allow(clippy::type_complexity)]
fn drive_bodies(
    players: Query<(
        &Body,
        &CharacterState,
        &LinearVelocity,
        Option<&WeaponState>,
        Has<Dead>,
    )>,
    mut anims: Query<&mut CharacterAnim>,
    mut visibility: Query<&mut Visibility, With<BodyWeapon>>,
    tuning: Res<Tuning>,
    fixed: Res<Time<Fixed>>,
) {
    let t = fixed.overstep_fraction();
    let b = &tuning.body;
    for (body, state, velocity, weapon, dead) in &players {
        let Ok(mut anim) = anims.get_mut(body.model) else { continue };
        let (height, lean) = state.interpolated(t);
        let stance = if height < (b.crouch_height + b.prone_height) / 2.0 {
            Stance::Prone
        } else if height < (b.stand_height + b.crouch_height) / 2.0 {
            Stance::Crouch
        } else {
            Stance::Stand
        };
        let (aiming, firing, reloading, lowered) = match weapon {
            Some(w) => (
                w.aim > 0.35,
                w.since_shot < 0.12,
                matches!(w.action, WeaponAction::Reloading { .. }),
                w.hands_busy || matches!(w.action, WeaponAction::Holstering { .. }),
            ),
            None => (false, false, false, false),
        };
        let next = if dead {
            CharacterAnim::default()
        } else {
            CharacterAnim {
                speed: velocity.0.with_y(0.0).length(),
                stance,
                lean,
                aiming: aiming && !lowered,
                firing: firing && !lowered,
                reloading,
            }
        };
        // Only write when it changes (keeps change detection quiet).
        let a = &*anim;
        if a.stance != next.stance
            || (a.speed - next.speed).abs() > 0.02
            || (a.lean - next.lean).abs() > 0.01
            || a.aiming != next.aiming
            || a.firing != next.firing
            || a.reloading != next.reloading
        {
            *anim = next;
        }
        if let Some((_, entity)) = body.weapon
            && let Ok(mut v) = visibility.get_mut(entity)
        {
            v.set_if_neq(if lowered { Visibility::Hidden } else { Visibility::Inherited });
        }
    }
}

/// Bend the spine for the look pitch, after the animation has posed it.
fn bend_spines(
    players: Query<(&Body, &PlayerInput, Has<Dead>)>,
    models: Query<&CharacterParts>,
    parents: Query<&ChildOf>,
    mut transforms: Query<&mut Transform>,
) {
    const SHARES: [(&str, f32); 3] = [("spine_01", 0.25), ("spine_02", 0.35), ("spine_03", 0.4)];
    for (body, input, dead) in &players {
        if dead || body.fall > 0.0 {
            continue;
        }
        let Ok(parts) = models.get(body.model) else { continue };
        let pitch = input.pitch.clamp(-1.1, 1.1);
        for (bone, share) in SHARES {
            let Some(entity) = parts.bone(bone) else { continue };
            // The parent's rotation in model space (current local poses).
            let mut model_rotation = Quat::IDENTITY;
            let mut current = entity;
            while let Ok(parent) = parents.get(current) {
                let p = parent.parent();
                if p == body.model {
                    break;
                }
                if let Ok(t) = transforms.get(p) {
                    model_rotation = t.rotation * model_rotation;
                }
                current = p;
            }
            let bend = Quat::from_rotation_x(pitch * share);
            if let Ok(mut t) = transforms.get_mut(entity) {
                let local = model_rotation.inverse() * bend * model_rotation;
                t.rotation = (local * t.rotation).normalize();
            }
        }
    }
}

/// Tip over backwards on death; stand up again when revived.
fn fall_when_dead(
    mut players: Query<(&mut Body, Has<Dead>)>,
    mut transforms: Query<&mut Transform, With<Character>>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    for (mut body, dead) in &mut players {
        let target = if dead { 1.0 } else { 0.0 };
        if body.fall == target {
            continue;
        }
        body.fall = if dead {
            (body.fall + dt / 0.55).min(1.0)
        } else {
            0.0
        };
        let Ok(mut t) = transforms.get_mut(body.model) else { continue };
        // Ease in, like a fall under gravity.
        let f = body.fall * body.fall;
        t.rotation = Quat::from_rotation_x(FRAC_PI_2 * f) * Quat::from_rotation_z(body.fall_roll * f);
        t.translation = Vec3::new(0.0, 0.12 * f, 0.0);
    }
}

#[derive(Resource)]
struct EffectAssets {
    flash: Handle<Mesh>,
    flash_material: Handle<StandardMaterial>,
    tracer: Handle<Mesh>,
    tracer_material: Handle<StandardMaterial>,
}

/// Muzzle flash, a flicker of light and a tracer for other people's shots.
#[allow(clippy::too_many_arguments)]
fn shot_effects(
    mut shots: MessageReader<ShotFired>,
    players: Query<&Body>,
    weapons: Query<&BodyWeapon>,
    globals: Query<&GlobalTransform>,
    mut assets: Local<Option<EffectAssets>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
    mut count: Local<u32>,
) {
    for shot in shots.read() {
        let Ok(body) = players.get(shot.shooter) else { continue };
        let a = assets.get_or_insert_with(|| EffectAssets {
            flash: meshes.add(Sphere::new(0.05)),
            flash_material: materials.add(StandardMaterial {
                base_color: Color::linear_rgb(6.0, 3.6, 1.4),
                unlit: true,
                alpha_mode: AlphaMode::Add,
                ..default()
            }),
            tracer: meshes.add(Cuboid::new(0.012, 0.012, 1.0)),
            tracer_material: materials.add(StandardMaterial {
                base_color: Color::linear_rgba(4.0, 3.0, 1.6, 0.6),
                unlit: true,
                alpha_mode: AlphaMode::Add,
                ..default()
            }),
        });
        let muzzle = body
            .weapon
            .and_then(|(_, w)| weapons.get(w).ok())
            .and_then(|w| w.muzzle)
            .and_then(|m| globals.get(m).ok())
            .map(|g| g.translation())
            .unwrap_or(shot.origin + shot.direction * 0.7 - Vec3::Y * 0.15);
        *count += 1;
        let stretch = Vec3::new(1.0 + (*count % 3) as f32 * 0.3, 1.0, 1.6);
        commands.spawn((
            Mesh3d(a.flash.clone()),
            MeshMaterial3d(a.flash_material.clone()),
            Transform::from_translation(muzzle)
                .looking_to(shot.direction, Vec3::Y)
                .with_scale(stretch),
            NotShadowCaster,
            ShotEffect { left: 0.045 },
        ));
        commands.spawn((
            PointLight {
                intensity: 60_000.0,
                range: 5.0,
                color: Color::srgb(1.0, 0.75, 0.45),
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(muzzle),
            ShotEffect { left: 0.04 },
        ));
        // A tracer on every other shot, from the muzzle to where it stopped.
        let span = shot.end - muzzle;
        if *count % 2 == 0 && span.length() > 1.0 {
            let length = span.length().min(60.0);
            let mid = muzzle + span.normalize() * length / 2.0;
            commands.spawn((
                Mesh3d(a.tracer.clone()),
                MeshMaterial3d(a.tracer_material.clone()),
                Transform::from_translation(mid)
                    .looking_to(span, Vec3::Y)
                    .with_scale(Vec3::new(1.0, 1.0, length)),
                NotShadowCaster,
                ShotEffect { left: 0.035 },
            ));
        }
    }
}

fn fade_effects(mut effects: Query<(Entity, &mut ShotEffect)>, time: Res<Time>, mut commands: Commands) {
    for (entity, mut effect) in &mut effects {
        effect.left -= time.delta_secs();
        if effect.left <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}
