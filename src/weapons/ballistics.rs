//! Hitscan bullet tracing with soft-surface penetration.
//!
//! One ray cast per surface the bullet meets, against world geometry
//! (`GameLayer::World`) and character hit zones (`GameLayer::Hitbox`, sensor
//! colliders: Avian's ray casts include sensors, filtered by layer):
//!
//! - a hit zone: the bullet stops there (no over-penetration through bodies);
//! - a soft world piece (`SurfaceKind::Soft` on its rigid body): reported,
//!   then the bullet carries on from the entry point with that whole piece
//!   ignored, losing damage, up to a maximum number of soft layers;
//! - anything else (hard, or no `SurfaceKind`): reported, the bullet stops.
//!
//! The shooter's own hit zones are skipped (the eye sits inside the head).

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::{
    combat::{ImpactKind, SurfaceImpact, SurfaceKind},
    config::PenetrationTuning,
    physics::GameLayer,
    player::HitZone,
};

/// Read-only world access a trace needs.
pub struct TraceWorld<'a, 'w, 's> {
    pub spatial: &'a SpatialQuery<'w, 's>,
    pub colliders: &'a Query<'w, 's, &'static ColliderOf>,
    pub zones: &'a Query<'w, 's, &'static HitZone>,
    pub surfaces: &'a Query<'w, 's, &'static SurfaceKind>,
}

/// A character hit zone struck by a bullet.
#[derive(Debug, Clone, Copy)]
pub struct ZoneHit {
    /// The character (rigid body of the hit zone).
    pub target: Entity,
    pub zone: HitZone,
    pub point: Vec3,
    pub distance: f32,
    /// Share of damage left after penetration (1 = none lost).
    pub damage_scale: f32,
    pub layers: u32,
}

#[derive(Debug, Clone)]
pub struct Trace {
    /// Where the bullet stopped, or the end of its range.
    pub end: Vec3,
    pub hit: Option<ZoneHit>,
    /// Every world surface the bullet entered, in order, with its kind.
    pub impacts: Vec<(SurfaceImpact, SurfaceKind)>,
}

/// Small step into a surface so a ray cast starting at an entry point
/// doesn't report the same face again.
const EPSILON: f32 = 1e-3;
/// Safety cap on ray casts per bullet.
const MAX_CASTS: usize = 8;

pub fn trace(
    world: &TraceWorld,
    shooter: Entity,
    origin: Vec3,
    direction: Dir3,
    pen: &PenetrationTuning,
) -> Trace {
    let filter = SpatialQueryFilter::from_mask([GameLayer::World, GameLayer::Hitbox]);
    let body_of = |collider: Entity| {
        world
            .colliders
            .get(collider)
            .map(|c| c.body)
            .unwrap_or(collider)
    };

    let mut passed: Vec<Entity> = Vec::new();
    let mut impacts = Vec::new();
    let mut travelled = 0.0;
    let mut start = origin;
    let mut damage_scale = 1.0;
    let mut layers = 0;

    for _ in 0..MAX_CASTS {
        let remaining = pen.range - travelled;
        if remaining <= 0.0 {
            break;
        }
        let predicate = |collider: Entity| {
            let body = body_of(collider);
            body != shooter && !passed.contains(&body)
        };
        let Some(hit) =
            world
                .spatial
                .cast_ray_predicate(start, direction, remaining, true, &filter, &predicate)
        else {
            break;
        };
        let point = start + *direction * hit.distance;
        travelled += hit.distance;

        if let Ok(&zone) = world.zones.get(hit.entity) {
            return Trace {
                end: point,
                hit: Some(ZoneHit {
                    target: body_of(hit.entity),
                    zone,
                    point,
                    distance: travelled,
                    damage_scale,
                    layers,
                }),
                impacts,
            };
        }

        let body = body_of(hit.entity);
        let kind = world
            .surfaces
            .get(body)
            .or_else(|_| world.surfaces.get(hit.entity))
            .copied()
            .unwrap_or(SurfaceKind::Hard);
        impacts.push((
            SurfaceImpact {
                collider: hit.entity,
                point,
                normal: hit.normal,
                direction: *direction,
                kind: ImpactKind::Bullet,
                source: Some(shooter),
            },
            kind,
        ));
        if kind == SurfaceKind::Soft && layers < pen.max_soft_layers {
            layers += 1;
            damage_scale *= pen.soft_damage_kept;
            passed.push(body);
            start = point + *direction * EPSILON;
            travelled += EPSILON;
            continue;
        }
        return Trace {
            end: point,
            hit: None,
            impacts,
        };
    }
    Trace {
        end: origin + *direction * pen.range,
        hit: None,
        impacts,
    }
}
