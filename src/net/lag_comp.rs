//! Lag compensation: validating shots against what the shooter saw.
//!
//! A client sees other players about 100 ms in the past (interpolation)
//! plus half its ping. When its shot reaches the server, the target has
//! moved on. So the server keeps a short history of every character's pose
//! ([`PoseHistory`]: feet, yaw, stance height, lean, one sample per tick,
//! ~300 ms) and, for a shot, rebuilds the hit zones as they were at the
//! shooter's *view time*: the shot's tick minus the interpolation delay the
//! client sent with its input (Lightyear's `lag_compensation` input
//! option). Hit zones are rebuilt with the same `zone_pose` the live ones
//! use, so stance and lean are rewound too. World geometry is not rewound
//! (walls don't move; destruction changes are rare enough).
//!
//! API for weapons (M3), in a fixed-tick system on the authority:
//! ```ignore
//! fn fire(lag: LagCompensation, ...) {
//!     let view = lag.view_time(shooter);
//!     if let Some(hit) = lag.cast_ray(view, eye, look, range, shooter) {
//!         match hit.target {
//!             HitTarget::Character { entity, zone } => { /* DamageMessage */ }
//!             HitTarget::World { collider } => { /* SurfaceImpact */ }
//!         }
//!     }
//!     // or, for custom queries: lag.with_rewound_hitboxes(view, |zones| ...)
//! }
//! ```
//! Solo, the host's own shots and bots use `view_time` = now, which is
//! exactly the live hit zones.
//!
//! `--net-shoot-test` (on the server) fires a test ray every 2 seconds from
//! one networked player at another using rewound poses and logs the result
//! next to what the live poses would have said.

use std::collections::VecDeque;

use avian3d::prelude::*;
use bevy::{ecs::system::SystemParam, prelude::*};
use lightyear::{
    interpolation::plugin::InterpolationDelay,
    prelude::{ControlledBy, LocalTimeline, Tick},
};

use super::NetPlayer;
use crate::{
    config::Tuning,
    physics::GameLayer,
    player::{CharacterState, HitZone, Player, PlayerInput, zone_pose},
    session::{Launch, is_authority},
};

/// Pose samples kept per character: 20 ticks = ~310 ms at 64 Hz.
pub const HISTORY_TICKS: u32 = 20;

pub(super) struct LagCompPlugin;

impl Plugin for LagCompPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedPostUpdate,
            record_poses
                .run_if(is_authority)
                .after(PhysicsSystems::Writeback),
        );
        if app.world().resource::<Launch>().shoot_test {
            app.add_systems(
                FixedPostUpdate,
                shoot_test.run_if(is_authority).after(record_poses),
            );
        }
    }
}

/// Where a character was at one tick: everything its hit zones depend on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub feet: Vec3,
    pub yaw: f32,
    pub height: f32,
    pub lean: f32,
}

impl Pose {
    fn lerp(self, other: Pose, t: f32) -> Pose {
        Pose {
            feet: self.feet.lerp(other.feet, t),
            yaw: Quat::from_rotation_y(self.yaw)
                .slerp(Quat::from_rotation_y(other.yaw), t)
                .to_euler(EulerRot::YXZ)
                .0,
            height: self.height.lerp(other.height, t),
            lean: self.lean.lerp(other.lean, t),
        }
    }

    /// The character root (feet, facing yaw).
    pub fn root(&self) -> Transform {
        Transform::from_translation(self.feet).with_rotation(Quat::from_rotation_y(self.yaw))
    }
}

/// Recent poses of a character (authority only), oldest first.
#[derive(Component, Default, Debug)]
pub struct PoseHistory {
    samples: VecDeque<(u32, Pose)>,
}

impl PoseHistory {
    /// Pose at `view` (between two ticks), or the oldest/newest sample if
    /// `view` is outside the history.
    pub fn at(&self, view: ViewTime) -> Option<Pose> {
        let (first, last) = (self.samples.front()?, self.samples.back()?);
        if !tick_after(view.tick, first.0) {
            return Some(first.1);
        }
        if !tick_after(last.0, view.tick) {
            return Some(last.1);
        }
        let index = self.samples.iter().position(|(t, _)| *t == view.tick)?;
        let (_, a) = self.samples[index];
        let b = self.samples.get(index + 1).map_or(a, |s| s.1);
        Some(a.lerp(b, view.fraction))
    }
}

/// `a` is strictly later than `b` (wrapping tick counter).
fn tick_after(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) > 0
}

/// A moment in simulation time: `fraction` of the way from `tick` to the
/// next tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewTime {
    pub tick: u32,
    pub fraction: f32,
}

fn record_poses(
    timeline: Res<LocalTimeline>,
    mut characters: Query<(&Transform, &CharacterState, &PlayerInput, &mut PoseHistory)>,
) {
    let tick = timeline.tick().0;
    for (transform, state, input, mut history) in &mut characters {
        history.samples.push_back((
            tick,
            Pose {
                feet: transform.translation,
                yaw: input.yaw,
                height: state.height,
                lean: state.lean,
            },
        ));
        while history
            .samples
            .front()
            .is_some_and(|(t, _)| tick.wrapping_sub(*t) > HISTORY_TICKS)
        {
            history.samples.pop_front();
        }
    }
}

/// One hit zone as it was at some moment.
#[derive(Clone, Copy, Debug)]
pub struct RewoundZone {
    pub character: Entity,
    pub zone: HitZone,
    /// World-space pose of the zone's capsule.
    pub transform: Transform,
}

/// Every character's hit zones at one moment.
pub struct RewoundHitboxes {
    pub zones: Vec<RewoundZone>,
}

impl RewoundHitboxes {
    /// Nearest zone along a ray, skipping `exclude`'s own zones.
    pub fn cast_ray(
        &self,
        origin: Vec3,
        direction: Dir3,
        max_distance: f32,
        exclude: Option<Entity>,
    ) -> Option<ZoneHit> {
        self.zones
            .iter()
            .filter(|z| Some(z.character) != exclude)
            .filter_map(|z| {
                let (radius, length) = z.zone.shape();
                let collider = Collider::capsule(radius, length);
                collider
                    .cast_ray(
                        z.transform.translation,
                        z.transform.rotation,
                        origin,
                        *direction,
                        max_distance,
                        true,
                    )
                    .map(|(distance, normal)| ZoneHit {
                        character: z.character,
                        zone: z.zone,
                        distance,
                        point: origin + *direction * distance,
                        normal,
                    })
            })
            .min_by(|a, b| a.distance.total_cmp(&b.distance))
    }
}

/// A ray hit on a rewound hit zone.
#[derive(Clone, Copy, Debug)]
pub struct ZoneHit {
    pub character: Entity,
    pub zone: HitZone,
    pub distance: f32,
    pub point: Vec3,
    pub normal: Vec3,
}

/// What a lag-compensated ray hit first.
#[derive(Clone, Copy, Debug)]
pub enum HitTarget {
    Character {
        entity: Entity,
        zone: HitZone,
    },
    /// Level geometry (or anything else on the world layer).
    World {
        collider: Entity,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct LagCompHit {
    pub target: HitTarget,
    pub distance: f32,
    pub point: Vec3,
    pub normal: Vec3,
}

/// System parameter for lag-compensated hit detection (authority only).
#[derive(SystemParam)]
pub struct LagCompensation<'w, 's> {
    timeline: Res<'w, LocalTimeline>,
    tuning: Res<'w, Tuning>,
    histories: Query<'w, 's, (Entity, &'static PoseHistory), With<Player>>,
    owners: Query<'w, 's, &'static ControlledBy>,
    delays: Query<'w, 's, &'static InterpolationDelay>,
    spatial: SpatialQuery<'w, 's>,
}

impl LagCompensation<'_, '_> {
    /// The current tick.
    pub fn now(&self) -> ViewTime {
        ViewTime {
            tick: self.timeline.tick().0,
            fraction: 0.0,
        }
    }

    /// When the shooter saw the world: now minus the interpolation delay
    /// its client last reported. Now for the host's own character and bots.
    pub fn view_time(&self, shooter: Entity) -> ViewTime {
        let now = self.timeline.tick();
        let delay = self
            .owners
            .get(shooter)
            .ok()
            .and_then(|owner| self.delays.get(owner.owner).ok());
        match delay {
            Some(delay) => {
                let (tick, fraction): (Tick, f32) = delay.tick_and_overstep(now);
                // Never further back than the history goes.
                let max_back = HISTORY_TICKS - 1;
                if now.0.wrapping_sub(tick.0) > max_back {
                    ViewTime {
                        tick: now.0.wrapping_sub(max_back),
                        fraction: 0.0,
                    }
                } else {
                    ViewTime {
                        tick: tick.0,
                        fraction,
                    }
                }
            }
            None => self.now(),
        }
    }

    /// How many ticks `view` is behind now.
    pub fn ticks_behind(&self, view: ViewTime) -> f32 {
        self.timeline.tick().0.wrapping_sub(view.tick) as f32 - view.fraction
    }

    /// A character's pose at `view`.
    pub fn pose_at(&self, character: Entity, view: ViewTime) -> Option<Pose> {
        self.histories.get(character).ok()?.1.at(view)
    }

    /// Run `f` with every character's hit zones as they were at `view`.
    pub fn with_rewound_hitboxes<R>(
        &self,
        view: ViewTime,
        f: impl FnOnce(&RewoundHitboxes) -> R,
    ) -> R {
        let mut zones = Vec::new();
        for (character, history) in &self.histories {
            let Some(pose) = history.at(view) else {
                continue;
            };
            let root = pose.root();
            for zone in [HitZone::Head, HitZone::Torso, HitZone::Legs] {
                let local = zone_pose(zone, pose.height, pose.lean, &self.tuning);
                zones.push(RewoundZone {
                    character,
                    zone,
                    transform: root.mul_transform(local),
                });
            }
        }
        f(&RewoundHitboxes { zones })
    }

    /// A ray against the world (as it is now) and every character's hit
    /// zones as they were at `view`; the shooter's own zones are skipped.
    pub fn cast_ray(
        &self,
        view: ViewTime,
        origin: Vec3,
        direction: Dir3,
        max_distance: f32,
        shooter: Entity,
    ) -> Option<LagCompHit> {
        let world = self.spatial.cast_ray(
            origin,
            direction,
            max_distance,
            true,
            &SpatialQueryFilter::from_mask(GameLayer::World),
        );
        let reach = world.map_or(max_distance, |w| w.distance);
        let zone = self.with_rewound_hitboxes(view, |zones| {
            zones.cast_ray(origin, direction, reach, Some(shooter))
        });
        match (zone, world) {
            (Some(z), _) => Some(LagCompHit {
                target: HitTarget::Character {
                    entity: z.character,
                    zone: z.zone,
                },
                distance: z.distance,
                point: z.point,
                normal: z.normal,
            }),
            (None, Some(w)) => Some(LagCompHit {
                target: HitTarget::World { collider: w.entity },
                distance: w.distance,
                point: origin + *direction * w.distance,
                normal: w.normal,
            }),
            (None, None) => None,
        }
    }
}

/// `--net-shoot-test`: every 2 s, a networked player "shoots" at another
/// one's rewound torso; log the rewound result next to the live one.
fn shoot_test(
    lag: LagCompensation,
    shooters: Query<
        (
            Entity,
            &Transform,
            &CharacterState,
            &PlayerInput,
            Option<&NetPlayer>,
            Has<ControlledBy>,
        ),
        With<Player>,
    >,
    tuning: Res<Tuning>,
    mut ticks: Local<u32>,
) {
    *ticks += 1;
    if *ticks % 128 != 0 {
        return;
    }
    // Prefer a client's character as the shooter (it has a real view
    // delay), and someone else as the target.
    let mut players: Vec<_> = shooters.iter().collect();
    players.sort_by_key(|p| (!p.5, p.0));
    let (Some(shooter), Some(target)) = (players.first(), players.get(1)) else {
        info!("lag-comp test: need two players");
        return;
    };
    let name = |p: &(
        Entity,
        &Transform,
        &CharacterState,
        &PlayerInput,
        Option<&NetPlayer>,
        bool,
    )| { p.4.map_or_else(|| format!("{}", p.0), |n| n.name.clone()) };
    let (eye, _) = crate::gadgets::eye_and_look(
        shooter.1,
        shooter.2,
        shooter.3,
        &tuning.body,
        tuning.lean.offset,
    );
    let view = lag.view_time(shooter.0);
    let Some(then) = lag.pose_at(target.0, view) else {
        return;
    };
    let now_pose = lag.pose_at(target.0, lag.now());
    let torso = then
        .root()
        .mul_transform(zone_pose(HitZone::Torso, then.height, then.lean, &tuning))
        .translation;
    let Ok(direction) = Dir3::new(torso - eye) else {
        return;
    };
    let rewound = lag.cast_ray(view, eye, direction, 100.0, shooter.0);
    let live = lag.cast_ray(lag.now(), eye, direction, 100.0, shooter.0);
    let describe = |hit: Option<LagCompHit>| match hit {
        Some(LagCompHit {
            target: HitTarget::Character { entity, zone },
            distance,
            point,
            normal,
        }) => format!(
            "HIT {entity} {zone:?} at {distance:.2} m ({:.2}, {:.2}, {:.2}), normal {normal:.2}",
            point.x, point.y, point.z
        ),
        Some(LagCompHit {
            target: HitTarget::World { collider },
            distance,
            ..
        }) => format!("blocked by the world ({collider}) at {distance:.2} m"),
        None => "MISS".into(),
    };
    info!(
        "lag-comp test: {} -> {} rewound {:.1} ticks (to tick {}+{:.2}): {}; live poses: {} (target moved {:.2} m since)",
        name(shooter),
        name(target),
        lag.ticks_behind(view),
        view.tick,
        view.fraction,
        describe(rewound),
        describe(live),
        now_pose.map_or(0.0, |p| p.feet.distance(then.feet)),
    );
}
