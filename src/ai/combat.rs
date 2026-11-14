//! Fighting: picking a target, aiming like a person, and shooting.
//!
//! - **Target**: the visible enemy already being fought, else the nearest
//!   (weighted by how far the view has to turn).
//! - **Reaction**: no shot before `reaction_time` (+/- jitter, more at the
//!   edge of the view) after a new target appears.
//! - **Aim**: the view turns at the difficulty's turn speed towards the
//!   chosen hit zone (head with `head_aim` chance, else torso, else
//!   whatever shows) plus an error that starts at `aim_error` degrees and
//!   settles over `aim_settle` seconds; a moving target drags the aim
//!   behind it (`tracking_error`). The trigger is only pulled when the aim
//!   is on the zone.
//! - **Recoil**: the look angles written to `PlayerInput` pull against
//!   the weapon's recoil by `recoil_control` (see `motor.rs`), so long
//!   sprays still climb a bit.
//! - **Bursts**: `burst_close` rounds up close down to `burst_far` far away,
//!   with `burst_pause` between; the sidearm is clicked.
//! - **Stance**: aim down sights when fighting; crouch (`crouch_chance`)
//!   when standing still; lean out from cover (`lean_chance`) when a wall
//!   is beside the bot.
//! - **Ammo**: reload when empty (or switch to the sidearm up close), and
//!   top up when the fight is over.
//! - **Friendly fire**: no shot when a team mate is in the line of fire.
//! - **Through walls**: never at someone it can't see, except, rarely
//!   (`wallbang_chance`), a short burst at an enemy it just heard behind a
//!   soft wall. Never through stone or a reinforced wall: the line is
//!   checked first and must only cross soft surfaces.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::{
    AiSettings, AiSystems, Bot, BotState, angles_to, eye_position,
    brain::GoalKind,
    senses::ZoneCache,
    test::AiStats,
};
use crate::{
    combat::{Dead, SurfaceKind, Team},
    config::Tuning,
    physics::GameLayer,
    player::{CharacterState, HitZone, Lean, Player, PlayerInput},
    round::{Round, RoundPhase},
    weapons::{Slot, WeaponAction, WeaponState},
};

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CombatSystems;

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedPreUpdate,
            fight.in_set(CombatSystems).in_set(AiSystems::Act),
        );
    }
}

#[derive(Default, Debug)]
pub struct CombatState {
    pub target: Option<Entity>,
    /// Hit zone aimed at: 0 head, 1 torso, 2 legs.
    zone: usize,
    pub aim_point: Vec3,
    pub last_seen: f32,
    react_at: f32,
    /// Aim error (yaw, pitch), radians.
    error: Vec2,
    last_angles: Option<(f32, f32)>,
    burst_left: u32,
    pause_until: f32,
    last_shots: u32,
    tap_at: f32,
    pub stand_still: bool,
    pub crouch: bool,
    pub lean: Lean,
    ff_check_at: f32,
    ff_blocked: bool,
    wallbang_until: f32,
    wallbang_at: Vec3,
    /// Something heard or a hit from out of sight: look there.
    pub glance: Option<Vec3>,
    lost_at: f32,
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn fight(
    mut bots: Query<
        (
            Entity,
            &mut Bot,
            &mut BotState,
            &Transform,
            &CharacterState,
            &PlayerInput,
            &Team,
            &WeaponState,
        ),
        Without<Dead>,
    >,
    teams: Query<&Team, With<Player>>,
    dead: Query<(), (With<Dead>, With<Player>)>,
    zones: Query<(&HitZone, &ChildOf)>,
    colliders: Query<&ColliderOf>,
    surfaces: Query<&SurfaceKind>,
    cache: Res<ZoneCache>,
    spatial: SpatialQuery,
    mut stats: ResMut<AiStats>,
    settings: Res<AiSettings>,
    tuning: Res<Tuning>,
    time: Res<Time>,
    round: Res<Round>,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    // No shooting before the action starts (attackers are held at their
    // spawn in prep) or after the round is decided.
    let shooting_allowed = matches!(round.phase, RoundPhase::Action | RoundPhase::Planted | RoundPhase::Inactive);
    let d = tuning.ai.preset(settings.level(&tuning));
    let world = SpatialQueryFilter::from_mask(GameLayer::World);
    let hitboxes = SpatialQueryFilter::from_mask([GameLayer::World, GameLayer::Hitbox]);
    let b = &tuning.ai.behaviour;

    for (me, mut bot, mut state, transform, body, input, team, weapon) in &mut bots {
        let state = &mut *state;
        let c = &mut state.combat;
        state.buttons.trigger = false;
        state.buttons.aim = false;
        state.look.combat = None;
        c.stand_still = false;
        if !shooting_allowed {
            c.target = None;
            c.burst_left = 0;
            c.crouch = false;
            c.lean = Lean::Center;
            c.glance = None;
            continue;
        }
        let feet = transform.translation;
        let eye = eye_position(feet, input.yaw, body.height, body.lean, &tuning);
        let def = weapon.active.def(&tuning);
        let ammo = weapon.ammo();

        // --- Target ------------------------------------------------------------
        // Someone who died since the last look is no longer a target.
        state.senses.visible.retain(|s| !dead.contains(s.entity));
        let current = c.target.and_then(|t| state.senses.visible.iter().find(|s| s.entity == t)).copied();
        let pick = current.or_else(|| {
            state
                .senses
                .visible
                .iter()
                .min_by(|a, b| {
                    let score = |s: &super::senses::Sighting| {
                        let (yaw, _) = angles_to(s.feet - feet);
                        s.distance + super::angle_diff(yaw, state.look.yaw).abs() * 6.0
                    };
                    score(a).total_cmp(&score(b))
                })
                .copied()
        });
        if let Some(seen) = pick {
            if c.target != Some(seen.entity) {
                // A new engagement.
                let back_soon = now - c.lost_at < 1.0;
                let mut reaction = d.reaction_time + d.reaction_jitter * bot.rng.signed();
                if seen.peripheral {
                    reaction += d.peripheral_delay;
                }
                if back_soon {
                    reaction *= 0.5;
                }
                c.react_at = now + reaction.max(0.05);
                let angle = bot.rng.f32() * std::f32::consts::TAU;
                let size = d.aim_error.to_radians() * (0.7 + 0.5 * bot.rng.f32());
                c.error = Vec2::new(angle.cos(), angle.sin() * 0.6) * size;
                c.zone = if bot.rng.chance(d.head_aim) { 0 } else { 1 };
                c.last_angles = None;
                c.burst_left = 0;
                let holding = state.nav.arrived || state.goal.at.is_none();
                c.crouch = holding && bot.rng.chance(d.crouch_chance) && seen.distance > 6.0;
                c.lean = Lean::Center;
                if holding && bot.rng.chance(d.lean_chance) {
                    c.lean = lean_out(&spatial, eye, seen.feet - feet);
                }
                c.target = Some(seen.entity);
            }
            c.last_seen = now;
            // Aim at the chosen zone if it shows, else whatever does.
            if seen.zones[c.zone].is_none() {
                c.zone = [1, 0, 2].into_iter().find(|&i| seen.zones[i].is_some()).unwrap_or(1);
            }
            if let Some(z) = cache.zones.get(&seen.entity) {
                c.aim_point = z[c.zone];
            }
        } else if let Some(target) = c.target {
            // Lost sight: hold the angle for a moment.
            if now - c.last_seen > 1.6 {
                c.target = None;
                c.lost_at = now;
                c.crouch = false;
                c.lean = Lean::Center;
            } else if let Some(z) = cache.zones.get(&target) {
                // Keep tracking the last known spot, not the live one.
                let _ = z;
            }
        }

        // --- Aim and fire ------------------------------------------------------
        let visible_now = pick.is_some();
        if let Some(target) = c.target {
            let to = c.aim_point - eye;
            let distance = to.length().max(0.5);
            let (yaw0, pitch0) = angles_to(to);
            c.error *= (-dt / d.aim_settle.max(0.05)).exp();
            if let Some((ly, lp)) = c.last_angles {
                let drift = Vec2::new(super::angle_diff(yaw0, ly), pitch0 - lp);
                c.error += drift * d.tracking_error;
            }
            c.last_angles = Some((yaw0, pitch0));
            state.look.combat = Some((yaw0 + c.error.x, pitch0 + c.error.y));
            let retreating = state.goal.kind == GoalKind::Retreat;
            // Stop to shoot when holding, or when it's close; a long-range
            // duel while on the move (crossing open ground) keeps moving.
            let on_the_move = state.goal.at.is_some() && !state.nav.arrived && distance > 18.0;
            c.stand_still = !retreating && !on_the_move;
            state.buttons.aim = !retreating || distance < 12.0;

            // Friendly fire check, a few times a second.
            if now >= c.ff_check_at {
                c.ff_check_at = now + 0.06;
                let look = super::look_dir(state.look.yaw, state.look.pitch);
                c.ff_blocked = Dir3::new(look).ok().and_then(|dir| {
                    spatial.cast_ray_predicate(eye, dir, distance + 0.5, true, &hitboxes, &|e| {
                        colliders.get(e).map(|c| c.body).unwrap_or(e) != me
                    })
                }).is_some_and(|hit| {
                    zones
                        .get(hit.entity)
                        .ok()
                        .and_then(|(_, parent)| teams.get(parent.parent()).ok())
                        .is_some_and(|t| t == team)
                });
            }

            let off_yaw = super::angle_diff(yaw0, state.look.yaw);
            let off_pitch = pitch0 - state.look.pitch;
            let off = (off_yaw * off_yaw + off_pitch * off_pitch).sqrt();
            let radius = if c.zone == 0 { 0.11 } else { 0.2 };
            let tolerance = (radius / distance).atan() + 0.006;
            let ready = weapon.action == WeaponAction::Ready && ammo.magazine > 0 && !weapon.hands_busy;
            let shots = weapon.shots_fired.wrapping_sub(c.last_shots);
            c.last_shots = weapon.shots_fired;
            if def.full_auto && c.burst_left > 0 {
                c.burst_left = c.burst_left.saturating_sub(shots);
                if c.burst_left == 0 {
                    c.pause_until = now + d.burst_pause * (0.7 + 0.6 * bot.rng.f32());
                }
            }
            let may_fire = visible_now && now >= c.react_at && ready && !c.ff_blocked && off <= tolerance;
            if may_fire {
                if def.full_auto {
                    if c.burst_left == 0 && now >= c.pause_until {
                        let t = ((distance - 8.0) / 17.0).clamp(0.0, 1.0);
                        let size = (d.burst_close as f32).lerp(d.burst_far as f32, t).round().max(1.0);
                        c.burst_left = size as u32;
                    }
                    state.buttons.trigger = c.burst_left > 0;
                } else if now >= c.tap_at {
                    state.buttons.trigger = true;
                    c.tap_at = now + 0.16 + 0.1 * bot.rng.f32();
                }
                if state.buttons.trigger && bot.rng.chance(0.05) {
                    // Spot check: is there stone between us and the target?
                    if stone_between(&spatial, &world, &colliders, &surfaces, eye, c.aim_point) {
                        stats.stone_shots += 1;
                    }
                    stats.checked_shots += 1;
                }
            } else if def.full_auto && c.burst_left > 0 && (!visible_now || off > tolerance * 3.0) {
                // Stop spraying at nothing.
                c.burst_left = 0;
                c.pause_until = now + d.burst_pause;
            }
            let _ = target;
        } else {
            c.burst_left = 0;
        }

        // --- Shooting through a soft wall at something heard ---------------------
        if c.target.is_none() {
            if now < c.wallbang_until {
                let to = c.wallbang_at - eye;
                let (yaw, pitch) = angles_to(to);
                state.look.combat = Some((yaw, pitch));
                state.buttons.aim = true;
                c.stand_still = true;
                let off = super::angle_diff(yaw, state.look.yaw).abs() + (pitch - state.look.pitch).abs();
                state.buttons.trigger = off < 0.03 && ammo.magazine > 0 && weapon.action == WeaponAction::Ready;
            } else if let Some(heard) = state.senses.heard
                && now - heard.time < 1.2
                && d.wallbang_chance > 0.0
                && heard.source.is_some_and(|s| teams.get(s).is_ok_and(|t| t != team))
                && heard.pos.distance(eye) < 9.0
                && bot.rng.chance(d.wallbang_chance * dt)
            {
                let at = heard.pos + Vec3::Y * 0.9;
                if soft_only_between(&spatial, &world, &colliders, &surfaces, eye, at) {
                    c.wallbang_until = now + 0.7;
                    c.wallbang_at = at;
                    stats.wallbangs += 1;
                    debug!("ai: bot {me} sprays through a soft wall at {at:.1}");
                }
            }
        }

        // --- Something heard or felt, out of sight: look that way ------------------
        c.glance = None;
        if c.target.is_none() && now >= c.wallbang_until {
            let hurt = state.senses.hurt.filter(|(_, _, t)| now - t < 2.0).map(|(_, p, _)| p);
            let heard = state
                .senses
                .heard
                .filter(|h| now - h.time < 1.5 && h.pos.distance(feet) < 16.0 && h.source.is_some_and(|s| teams.get(s).is_ok_and(|t| t != team)))
                .map(|h| h.pos);
            c.glance = hurt.or(heard).map(|p| p + Vec3::Y * 1.2);
        }

        // --- Ammo ---------------------------------------------------------------------
        let in_fight = c.target.is_some();
        let primary = weapon.ammo[Slot::Primary.index()];
        let sidearm = weapon.ammo[Slot::Sidearm.index()];
        if weapon.action == WeaponAction::Ready && ammo.magazine == 0 {
            let close = in_fight && c.aim_point.distance(eye) < 12.0;
            if weapon.active == Slot::Primary && close && sidearm.magazine > 0 {
                state.buttons.slot = Slot::Sidearm;
            } else if ammo.reserve > 0 {
                state.buttons.reload = true;
            } else if weapon.active == Slot::Primary && sidearm.magazine + sidearm.reserve > 0 {
                state.buttons.slot = Slot::Sidearm;
            } else if weapon.active == Slot::Sidearm && primary.magazine + primary.reserve > 0 {
                state.buttons.slot = Slot::Primary;
            }
        } else if !in_fight && now - c.last_seen > 2.0 && weapon.action == WeaponAction::Ready {
            if weapon.active == Slot::Sidearm && primary.magazine + primary.reserve > 0 {
                state.buttons.slot = Slot::Primary;
            } else if (ammo.magazine as f32) < def.magazine as f32 * b.reload_below && ammo.reserve > 0 {
                state.buttons.reload = true;
            }
        }
        if !matches!(state.buttons.slot, Slot::Primary | Slot::Sidearm) {
            state.buttons.slot = Slot::Primary;
        }
    }
}

/// Lean away from a wall beside the eye, if there is one on one side only.
fn lean_out(spatial: &SpatialQuery, eye: Vec3, towards: Vec3) -> Lean {
    let filter = SpatialQueryFilter::from_mask(GameLayer::World);
    let forward = towards.with_y(0.0).normalize_or(Vec3::NEG_Z);
    let right = Vec3::new(-forward.z, 0.0, forward.x);
    let blocked = |dir: Vec3| {
        Dir3::new(dir)
            .ok()
            .and_then(|d| spatial.cast_ray(eye, d, 0.7, true, &filter))
            .is_some()
    };
    match (blocked(-right), blocked(right)) {
        (true, false) => Lean::Right,
        (false, true) => Lean::Left,
        _ => Lean::Center,
    }
}

/// Surfaces between two points: (soft count, any hard).
fn surfaces_between(
    spatial: &SpatialQuery,
    filter: &SpatialQueryFilter,
    colliders: &Query<&ColliderOf>,
    surfaces: &Query<&SurfaceKind>,
    from: Vec3,
    to: Vec3,
) -> (u32, bool) {
    let delta = to - from;
    let Ok(dir) = Dir3::new(delta) else { return (0, false) };
    let mut soft = 0;
    let mut hard = false;
    for hit in spatial.ray_hits(from, dir, delta.length() - 0.05, 8, true, filter) {
        let body = colliders.get(hit.entity).map(|c| c.body).unwrap_or(hit.entity);
        let kind = surfaces.get(body).or_else(|_| surfaces.get(hit.entity)).copied().unwrap_or(SurfaceKind::Hard);
        match kind {
            SurfaceKind::Soft => soft += 1,
            SurfaceKind::Hard => hard = true,
        }
    }
    (soft, hard)
}

fn soft_only_between(
    spatial: &SpatialQuery,
    filter: &SpatialQueryFilter,
    colliders: &Query<&ColliderOf>,
    surfaces: &Query<&SurfaceKind>,
    from: Vec3,
    to: Vec3,
) -> bool {
    let (soft, hard) = surfaces_between(spatial, filter, colliders, surfaces, from, to);
    !hard && (1..=2).contains(&soft)
}

fn stone_between(
    spatial: &SpatialQuery,
    filter: &SpatialQueryFilter,
    colliders: &Query<&ColliderOf>,
    surfaces: &Query<&SurfaceKind>,
    from: Vec3,
    to: Vec3,
) -> bool {
    surfaces_between(spatial, filter, colliders, surfaces, from, to).1
}
