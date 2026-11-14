//! Melee (V): a short sweep in front of the eyes on the fixed tick. Hits a
//! person (melee damage through their hit zones) or a soft surface (a melee
//! [`SurfaceImpact`]); repeated hits on one spot grow the hole.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::{
    DestructionIntent, DestructionSystems, MeleeSwing, eye_and_aim, impacts::surface_root,
    surface::SoftSurface,
};
use crate::{
    combat::{DamageKind, DamageMessage, Dead, ImpactKind, SurfaceImpact},
    config::Tuning,
    physics::GameLayer,
    player::{CharacterState, HitZone, PlayerInput},
};

pub struct MeleePlugin;

impl Plugin for MeleePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, melee.in_set(DestructionSystems::Actions));
    }
}

/// Simulation state for melee and for spotting button presses.
#[derive(Component, Default, Debug, Clone)]
pub struct MeleeState {
    /// Seconds until the next swing is allowed.
    pub cooldown: f32,
    /// Swings so far (presentation watches this for feedback).
    pub swings: u32,
    /// Buttons at the previous tick, to find presses.
    pub prev: DestructionIntent,
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn melee(
    mut characters: Query<
        (
            Entity,
            &PlayerInput,
            &CharacterState,
            &Transform,
            &DestructionIntent,
            &mut MeleeState,
            Option<&Children>,
        ),
        Without<Dead>,
    >,
    zones: Query<(&HitZone, &ChildOf)>,
    spatial: SpatialQuery,
    colliders: Query<&ColliderOf>,
    surfaces: Query<(&SoftSurface, &GlobalTransform)>,
    tuning: Res<Tuning>,
    time: Res<Time>,
    mut impacts: MessageWriter<SurfaceImpact>,
    mut damage: MessageWriter<DamageMessage>,
    mut swings: MessageWriter<MeleeSwing>,
) {
    let m = &tuning.destruction.melee;
    for (entity, input, state, transform, intent, mut melee, children) in &mut characters {
        melee.cooldown = (melee.cooldown - time.delta_secs()).max(0.0);
        let pressed = intent.melee && !melee.prev.melee;
        melee.prev.melee = intent.melee;
        if !pressed || melee.cooldown > 0.0 {
            continue;
        }
        melee.cooldown = m.cooldown;
        melee.swings += 1;

        let (eye, dir) = eye_and_aim(transform, state, input, &tuning);
        let own: Vec<Entity> = children
            .map(|c| c.iter().collect())
            .unwrap_or_default();
        let filter = SpatialQueryFilter::from_mask([GameLayer::World, GameLayer::Hitbox])
            .with_excluded_entities(own.iter().copied().chain([entity]));
        let config = ShapeCastConfig::from_max_distance(m.range);
        let sweep = Collider::sphere(m.sweep_radius);
        let mut hit_something = false;

        let surface_impact = |collider: Entity, fallback: Vec3, normal: Vec3| {
            let root = surface_root(collider, &colliders);
            let (surface, transform) = surfaces.get(root).ok()?;
            let point = surface
                .aim_point(transform, eye, *dir, m.range + m.edge_radius)
                .unwrap_or(fallback);
            Some(SurfaceImpact {
                collider,
                point,
                normal,
                direction: *dir,
                kind: ImpactKind::Melee,
                source: Some(entity),
            })
        };

        if let Some(hit) = spatial.cast_shape(&sweep, eye, Quat::IDENTITY, dir, &config, &filter) {
            hit_something = true;
            if let Ok((zone, parent)) = zones.get(hit.entity) {
                damage.write(DamageMessage {
                    target: parent.parent(),
                    amount: m.damage,
                    kind: DamageKind::Melee,
                    zone: Some(*zone),
                    source: Some(entity),
                });
            } else if let Some(impact) = surface_impact(hit.entity, hit.point1, hit.normal1) {
                impacts.write(impact);
            }
        } else {
            // Swinging into an existing hole: catch its edges so it grows.
            let world = SpatialQueryFilter::from_mask(GameLayer::World);
            let edge = Collider::sphere(m.edge_radius);
            if let Some(hit) = spatial.cast_shape(&edge, eye, Quat::IDENTITY, dir, &config, &world)
                && let Some(impact) = surface_impact(hit.entity, hit.point1, hit.normal1)
            {
                hit_something = true;
                impacts.write(impact);
            }
        }
        swings.write(MeleeSwing {
            attacker: entity,
            hit: hit_something,
        });
    }
}
