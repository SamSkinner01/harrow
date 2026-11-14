//! Breach charge (shared attacker secondary gadget, key 4).
//!
//! Hold 4 while looking at a soft wall, a wooden floor or hatch (from
//! above) or a barricade for `breach_charge.place_time` seconds to stick a
//! charge on it; press 4 again to set it off. One charge out at a time; a
//! character carries [`BreachCharges`] of them.
//!
//! The blast is an ordinary [`Explosion`] attached to the surface: walls
//! get a walk-through hole (the charge always sits at `wall_height`), floors
//! a drop-through hole, hatches and barricades break. Reinforced and stone
//! surfaces don't take a charge (and a wall reinforced after placing shrugs
//! the blast off).
//!
//! Lives here rather than in `gadgets/` because it is mostly destruction;
//! the loadout (who carries it) belongs to the round loop / gadgets.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::{
    Activity, BreachCharges, DestructionGate, DestructionIntent, DestructionSystems, Explosion,
    eye_and_aim, impacts::surface_root,
    melee::MeleeState,
    surface::{SoftSurface, SurfaceRole},
};
use crate::{
    combat::Dead,
    config::Tuning,
    physics::GameLayer,
    player::{CharacterState, PlayerInput},
};

pub struct BreachChargePlugin;

impl Plugin for BreachChargePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (breach_charges, drop_orphaned_charges)
                .chain()
                .in_set(DestructionSystems::Actions),
        )
        .add_systems(Update, dress_charges);
    }
}

/// A breach charge stuck to a surface, waiting for its owner to set it off.
/// Its `Transform` is the charge's centre, +Z pointing out of the surface.
#[derive(Component, Debug, Clone, Copy)]
pub struct BreachCharge {
    pub owner: Entity,
    /// Root of the surface it is stuck to.
    pub surface: Entity,
}

/// Charge body size (x, y, depth), metres.
const CHARGE_SIZE: Vec3 = Vec3::new(0.32, 0.42, 0.05);

/// Where a charge would go on `surface`, looking along `dir` from `eye`, or
/// `None` if it can't take one.
fn placement(
    surface: &SoftSurface,
    transform: &GlobalTransform,
    eye: Vec3,
    dir: Vec3,
    hit_normal: Vec3,
    tuning: &Tuning,
) -> Option<Transform> {
    if surface.is_reinforced() || surface.broken {
        return None;
    }
    let plane = surface.spec.plane;
    let point = surface.aim_point(transform, eye, dir, tuning.destruction.breach_charge.reach + 0.3)?;
    let inverse = transform.affine().inverse();
    let (mut uv, _) = plane.from_local(inverse.transform_point3(point));
    let side = surface.side_of(transform, eye);
    let (w0, w1) = surface.spec.skin_depth;
    let (min, max) = (surface.grid.min(), surface.grid.max());
    match surface.role() {
        SurfaceRole::Wall => {
            // Always at the same height, so the hole is walk-through.
            uv.y = tuning.destruction.breach_charge.wall_height;
            let half = CHARGE_SIZE.x / 2.0 + 0.05;
            uv.x = uv.x.clamp(min.x + half, (max.x - half).max(min.x + half));
        }
        SurfaceRole::Floor | SurfaceRole::Hatch => {
            // Floors and hatches only from above.
            if hit_normal.y < 0.7 || side < 0.0 {
                return None;
            }
        }
        SurfaceRole::Barricade => {
            uv = uv.clamp(min + 0.15, max - 0.15);
        }
    }
    let w = if side >= 0.0 { w1 + CHARGE_SIZE.z / 2.0 } else { w0 - CHARGE_SIZE.z / 2.0 };
    let centre = transform.transform_point(plane.to_local(uv.x, uv.y, w));
    let out = (transform.rotation() * plane.normal() * side).normalize();
    // +Z out of the surface; "up" along the wall (or towards the player on
    // a floor).
    let up = if out.y.abs() > 0.9 {
        (eye - centre).with_y(0.0).normalize_or(Vec3::Z)
    } else {
        Vec3::Y
    };
    Some(Transform::from_translation(centre).looking_to(-out, up))
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn breach_charges(
    mut characters: Query<
        (
            Entity,
            &PlayerInput,
            &CharacterState,
            &Transform,
            &DestructionIntent,
            &mut Activity,
            &mut MeleeState,
            &mut BreachCharges,
        ),
        Without<Dead>,
    >,
    charges: Query<(Entity, &BreachCharge, &Transform), Without<PlayerInput>>,
    surfaces: Query<(&SoftSurface, &GlobalTransform)>,
    spatial: SpatialQuery,
    colliders: Query<&ColliderOf>,
    gate: Res<DestructionGate>,
    tuning: Res<Tuning>,
    time: Res<Time>,
    mut explosions: MessageWriter<Explosion>,
    mut commands: Commands,
) {
    let t = &tuning.destruction.breach_charge;
    let world = SpatialQueryFilter::from_mask(GameLayer::World);
    for (entity, input, state, transform, intent, mut activity, mut buttons, mut left) in
        &mut characters
    {
        let pressed = intent.gadget && !buttons.prev.gadget;
        buttons.prev.gadget = intent.gadget;
        let placed = charges.iter().find(|(_, c, _)| c.owner == entity);

        // Press with a charge out: detonate.
        if pressed
            && let Some((charge_entity, charge, at)) = placed
        {
            if gate.breach_charge {
                explosions.write(Explosion {
                    centre: at.translation,
                    breach_radius: t.breach_radius,
                    damage_radius: t.damage_radius,
                    max_damage: t.max_damage,
                    source: Some(entity),
                    attached_to: Some(charge.surface),
                });
                commands.entity(charge_entity).despawn();
            }
            continue;
        }

        let (eye, dir) = eye_and_aim(transform, state, input, &tuning);
        let target = || {
            let hit = spatial.cast_ray(eye, dir, t.reach, true, &world)?;
            let root = surface_root(hit.entity, &colliders);
            let (surface, gt) = surfaces.get(root).ok()?;
            Some((root, placement(surface, gt, eye, *dir, hit.normal, &tuning)?))
        };
        match *activity {
            Activity::PlacingCharge { surface, progress } => {
                let still = input.move_axis.length_squared() < 0.01;
                let on_target = target().filter(|(root, _)| *root == surface);
                let Some((_, at)) = on_target.filter(|_| intent.gadget && still && gate.breach_charge && left.0 > 0)
                else {
                    *activity = Activity::Idle;
                    continue;
                };
                let progress = progress + time.delta_secs() / t.place_time.max(0.01);
                if progress < 1.0 {
                    *activity = Activity::PlacingCharge { surface, progress };
                    continue;
                }
                *activity = Activity::Idle;
                left.0 -= 1;
                commands.spawn((
                    Name::new("Breach charge"),
                    BreachCharge {
                        owner: entity,
                        surface,
                    },
                    at,
                    Visibility::default(),
                ));
            }
            Activity::Idle => {
                if pressed
                    && placed.is_none()
                    && left.0 > 0
                    && gate.breach_charge
                    && let Some((surface, _)) = target()
                {
                    *activity = Activity::PlacingCharge { surface, progress: 0.0 };
                }
            }
            _ => {}
        }
    }
}

/// A charge whose surface broke away (or vanished) falls off and is lost.
fn drop_orphaned_charges(
    charges: Query<(Entity, &BreachCharge)>,
    surfaces: Query<&SoftSurface>,
    mut commands: Commands,
) {
    for (entity, charge) in &charges {
        if surfaces.get(charge.surface).ok().is_none_or(|s| s.broken) {
            commands.entity(entity).despawn();
        }
    }
}

/// Looks for new charges: a dark box with a red light.
fn dress_charges(
    new: Query<Entity, Added<BreachCharge>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<Option<(Handle<Mesh>, Handle<Mesh>, Handle<StandardMaterial>, Handle<StandardMaterial>)>>,
    mut commands: Commands,
) {
    if new.is_empty() {
        return;
    }
    let (body, light, body_material, light_material) = assets
        .get_or_insert_with(|| {
            (
                meshes.add(Cuboid::new(CHARGE_SIZE.x, CHARGE_SIZE.y, CHARGE_SIZE.z)),
                meshes.add(Cuboid::new(0.03, 0.03, 0.012)),
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.16, 0.17, 0.15),
                    perceptual_roughness: 0.7,
                    ..default()
                }),
                materials.add(StandardMaterial {
                    base_color: Color::srgb(1.0, 0.1, 0.05),
                    emissive: LinearRgba::rgb(40.0, 2.0, 1.0),
                    ..default()
                }),
            )
        })
        .clone();
    for entity in &new {
        commands.entity(entity).with_children(|parent| {
            parent.spawn((Mesh3d(body.clone()), MeshMaterial3d(body_material.clone())));
            parent.spawn((
                Mesh3d(light.clone()),
                MeshMaterial3d(light_material.clone()),
                Transform::from_xyz(0.1, 0.15, CHARGE_SIZE.z / 2.0),
            ));
        });
    }
}
