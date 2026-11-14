//! Interact (hold F), defenders only:
//!
//! - **Reinforce** the soft wall or hatch you look at: hold for
//!   `reinforce.time` seconds without moving or looking away. Uses one of
//!   the defender's [`Reinforcements`]. The plate goes on your side.
//! - **Barricade** the empty doorway or window you look at: hold for
//!   `barricade.build_time` seconds. Unlimited.
//!
//! Both start on a press of F and are gated by [`DestructionGate`] (the
//! round loop closes them outside prep/action).

use avian3d::prelude::*;
use bevy::prelude::*;

use super::{
    Activity, DestructionGate, DestructionIntent, DestructionSystems, ReinforceCancelled,
    ReinforceRequest, ReinforceStarted, Reinforcements, eye_and_aim, impacts::surface_root,
    melee::MeleeState,
    surface::{SoftSurface, SurfaceRole},
};
use crate::{
    combat::{Dead, SurfaceKind, Team},
    round::ObjectiveInteraction,
    config::Tuning,
    physics::GameLayer,
    player::{CharacterState, PlayerInput},
};

pub struct InteractPlugin;

impl Plugin for InteractPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(register_opening)
            .add_observer(stop_when_dead)
            .add_systems(
                FixedUpdate,
                interact
                    .after(super::melee::melee)
                    .in_set(DestructionSystems::Actions),
            );
    }
}

/// A barricade a defender put up (removed when a new round starts; the
/// map's own barricades are restored instead).
#[derive(Component, Debug)]
pub struct PlacedBarricade;

/// A doorway or window in a wall piece that can take a barricade. Added to
/// the wall piece's root when it loads.
#[derive(Component, Debug, Clone, Copy)]
pub struct BarricadeSlot {
    pub window: bool,
    /// Opening in the piece's local x/y (it is centred on z = 0).
    pub min: Vec2,
    pub max: Vec2,
}

impl BarricadeSlot {
    /// Barricade placement, relative to the wall piece.
    pub fn local_transform(&self) -> Transform {
        Transform::from_xyz((self.min.x + self.max.x) / 2.0, 0.0, 0.0)
    }

    pub fn piece_name(&self) -> &'static str {
        if self.window { "barricade_window" } else { "barricade_door" }
    }
}

/// Dying stops whatever you were doing.
fn stop_when_dead(
    add: On<Add<Dead>>,
    mut activities: Query<&mut Activity>,
    mut cancelled: MessageWriter<ReinforceCancelled>,
) {
    if let Ok(mut activity) = activities.get_mut(add.entity) {
        if let Activity::Reinforcing { surface, .. } = *activity {
            cancelled.write(ReinforceCancelled { player: add.entity, surface });
        }
        *activity = Activity::Idle;
    }
}

fn register_opening(insert: On<Insert<SurfaceKind>>, names: Query<&Name>, mut commands: Commands) {
    let Ok(name) = names.get(insert.entity) else { return };
    // Opening sizes from `pieces.py`.
    let slot = match name.as_str() {
        "stone_wall_door_2m" => BarricadeSlot {
            window: false,
            min: Vec2::new(0.45, 0.0),
            max: Vec2::new(1.55, 2.2),
        },
        "soft_wall_door_2m" => BarricadeSlot {
            window: false,
            min: Vec2::new(0.5, 0.0),
            max: Vec2::new(1.5, 2.1),
        },
        "stone_wall_window_2m" | "soft_wall_window_2m" => BarricadeSlot {
            window: true,
            min: Vec2::new(0.5, 1.0),
            max: Vec2::new(1.5, 2.1),
        },
        _ => return,
    };
    commands.entity(insert.entity).try_insert(slot);
}

/// Distance along the aim ray to the opening of `slot`, if the ray goes
/// through it within `reach`.
fn aim_through(slot: &BarricadeSlot, transform: &GlobalTransform, eye: Vec3, dir: Vec3, reach: f32) -> Option<f32> {
    let inverse = transform.affine().inverse();
    let (e, d) = (inverse.transform_point3(eye), inverse.transform_vector3(dir));
    if d.z.abs() < 1e-3 {
        return None;
    }
    let t = -e.z / d.z;
    if !(0.0..=reach).contains(&t) {
        return None;
    }
    let p = e + d * t;
    (p.x >= slot.min.x && p.x <= slot.max.x && p.y >= slot.min.y && p.y <= slot.max.y).then_some(t)
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn interact(
    mut characters: Query<
        (
            Entity,
            &PlayerInput,
            &CharacterState,
            &Transform,
            &DestructionIntent,
            &Team,
            &mut Activity,
            &mut MeleeState,
            &mut Reinforcements,
            Has<ObjectiveInteraction>,
        ),
        Without<Dead>,
    >,
    surfaces: Query<(Entity, &SoftSurface, &GlobalTransform)>,
    slots: Query<(Entity, &BarricadeSlot, &GlobalTransform)>,
    spatial: SpatialQuery,
    colliders: Query<&ColliderOf>,
    gate: Res<DestructionGate>,
    tuning: Res<Tuning>,
    time: Res<Time>,
    mut requests: MessageWriter<ReinforceRequest>,
    mut started: MessageWriter<ReinforceStarted>,
    mut cancelled: MessageWriter<ReinforceCancelled>,
    mut commands: Commands,
) {
    let t = &tuning.destruction;
    let dt = time.delta_secs();
    let world = SpatialQueryFilter::from_mask(GameLayer::World);
    for (entity, input, state, transform, intent, team, mut activity, mut buttons, mut left, objective) in
        &mut characters
    {
        // Planting or disabling the defuser: F belongs to the round loop.
        let intent = if objective {
            &DestructionIntent { interact: false, ..*intent }
        } else {
            intent
        };
        let pressed = intent.interact && !buttons.prev.interact;
        buttons.prev.interact = intent.interact;
        let (eye, dir) = eye_and_aim(transform, state, input, &tuning);
        let still = input.move_axis.length_squared() < 0.01;

        // A standing barricade within 0.3 m of a slot blocks a new one; a
        // broken one is replaced.
        let barricade_at = |at: Vec3| {
            surfaces.iter().find(|(_, s, gt)| {
                s.role() == SurfaceRole::Barricade && gt.translation().distance(at) < 0.3
            })
        };

        match *activity {
            Activity::Reinforcing { surface, progress } => {
                let ok = intent.interact
                    && still
                    && gate.reinforce
                    && left.0 > 0
                    && surfaces.get(surface).is_ok_and(|(_, s, gt)| {
                        s.can_reinforce() && s.aim_point(gt, eye, *dir, t.reinforce.reach + 0.3).is_some()
                    });
                if !ok {
                    *activity = Activity::Idle;
                    cancelled.write(ReinforceCancelled { player: entity, surface });
                    continue;
                }
                let progress = progress + dt / t.reinforce.time.max(0.01);
                if progress >= 1.0 {
                    requests.write(ReinforceRequest {
                        surface,
                        from: eye,
                        by: Some(entity),
                    });
                    left.0 -= 1;
                    *activity = Activity::Idle;
                } else {
                    *activity = Activity::Reinforcing { surface, progress };
                }
            }
            Activity::Barricading { slot, progress } => {
                let Ok((_, slot_data, slot_transform)) = slots.get(slot) else {
                    *activity = Activity::Idle;
                    continue;
                };
                let ok = intent.interact
                    && still
                    && gate.barricade
                    && aim_through(slot_data, slot_transform, eye, *dir, t.barricade.reach + 0.3).is_some();
                if !ok {
                    *activity = Activity::Idle;
                    continue;
                }
                let progress = progress + dt / t.barricade.build_time.max(0.01);
                if progress < 1.0 {
                    *activity = Activity::Barricading { slot, progress };
                    continue;
                }
                *activity = Activity::Idle;
                let at = slot_transform.mul_transform(slot_data.local_transform());
                match barricade_at(at.translation()) {
                    Some((_, s, _)) if !s.broken => continue,
                    Some((old, _, _)) => commands.entity(old).despawn(),
                    None => {}
                }
                commands.spawn((
                    Name::new(slot_data.piece_name()),
                    PlacedBarricade,
                    at.compute_transform(),
                    Visibility::default(),
                    RigidBody::Static,
                    SurfaceKind::Soft,
                ));
            }
            Activity::Idle => {
                if !pressed || *team != Team::Defenders {
                    continue;
                }
                // Reinforce the soft surface in front of you...
                if gate.reinforce
                    && left.0 > 0
                    && let Some(hit) = spatial.cast_ray(eye, dir, t.reinforce.reach, true, &world)
                {
                    let root = surface_root(hit.entity, &colliders);
                    if surfaces.get(root).is_ok_and(|(_, s, _)| s.can_reinforce()) {
                        *activity = Activity::Reinforcing { surface: root, progress: 0.0 };
                        started.write(ReinforceStarted { player: entity, surface: root });
                        continue;
                    }
                }
                // ...or barricade the opening you look through.
                if gate.barricade {
                    let nearest = slots
                        .iter()
                        .filter_map(|(slot, data, gt)| {
                            let d = aim_through(data, gt, eye, *dir, t.barricade.reach)?;
                            let at = gt.mul_transform(data.local_transform()).translation();
                            let standing = barricade_at(at).is_some_and(|(_, s, _)| !s.broken);
                            (!standing).then_some((slot, d))
                        })
                        .min_by(|a, b| a.1.total_cmp(&b.1));
                    if let Some((slot, _)) = nearest {
                        *activity = Activity::Barricading { slot, progress: 0.0 };
                    }
                }
            }
            Activity::PlacingCharge { .. } => {}
        }
    }
}
