//! Acting on the goal every tick: paths, steering, special moves, the view,
//! and finally the intent components.
//!
//! - **Paths**: planned with A* to the goal (a couple of searches per tick
//!   across all bots at most), re-planned when the goal moves, when the
//!   grid changes along the path, or when stuck.
//! - **Following**: steer at the next waypoint; crouch where only crouching
//!   fits; walk off ledges for drop links.
//! - **Barricades** on the path: stop, face it and melee until it falls.
//! - **Breaches** on the path: stick a breach charge on the wall, back off,
//!   set it off, then go through the hole.
//! - **Avoidance**: keep `avoid_radius` from everyone, and keep stack
//!   spacing from the attacker ahead.
//! - **Stuck**: no progress for `stuck_time` while trying to move. First a
//!   re-plan, then a sidestep, then break whatever soft thing is in front.
//!   Every case is logged ("ai: X stuck at ...") and counted.
//! - **Looking**: fight target > activity > what the goal watches > the
//!   next corner or opening (careful pace) > the way ahead, turned at the
//!   difficulty's turn speed.
//! - **Intents**: move axis relative to the facing, stance, lean, buttons
//!   (with presses as edges), and recoil compensation on the look angles.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::{
    AiSettings, AiSystems, Bot, BotState, angle_diff, angles_to,
    brain::{GoalKind, Pace, TeamPlans},
    eye_position, look_dir,
    nav::{LinkKind, NavGrid},
    path::{
        COST_BARRICADE, COST_BARRICADE_DEFENDER, COST_BREACH, COST_BREACH_PREFERRED, Path, PathOptions,
        PathScratch, find_path,
    },
    test::AiStats,
};
use crate::{
    combat::{Dead, Team},
    config::Tuning,
    destruction::{BreachCharge, BreachCharges, DestructionIntent, SoftSurface, SurfaceRole},
    gadgets::GadgetIntent,
    physics::GameLayer,
    player::{CharacterState, Lean, Player, PlayerInput, Stance},
    round::{MovementLock, RoundIntent},
    weapons::{Slot, WeaponInput, WeaponState},
};

pub struct MotorPlugin;

impl Plugin for MotorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PathScratch>().add_systems(
            FixedPreUpdate,
            (drive, turn, write_intents)
                .chain()
                .after(super::combat::CombatSystems)
                .in_set(AiSystems::Act),
        );
    }
}

/// Buttons a bot holds this tick (written as intents at the end).
#[derive(Default, Debug, Clone)]
pub struct Buttons {
    pub trigger: bool,
    pub aim: bool,
    /// One-shot: reload.
    pub reload: bool,
    pub slot: Slot,
    pub melee: bool,
    /// Destruction interact (reinforce, barricade).
    pub build: bool,
    /// Breach charge button.
    pub charge: bool,
    /// Round interact (plant, disable).
    pub objective: bool,
    /// One-shot: unique gadget.
    pub gadget: bool,
    /// Secondary gadget key held (grenades: release to throw).
    pub secondary: bool,
    pub crouch: bool,
    pub lean: Lean,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum BreachStep {
    #[default]
    None,
    Placing { since: f32 },
    BackingOff { since: f32, from: Vec3, away: Vec3 },
    Detonated { at: f32 },
}

#[derive(Default, Debug)]
pub struct NavState {
    pub path: Option<Path>,
    pub index: usize,
    planned_for: Option<Vec3>,
    planned_alt: bool,
    next_plan: f32,
    pub failures: u32,
    pub arrived: bool,
    /// Wanted horizontal movement (world, length 0..1).
    pub move_dir: Vec3,
    pub sprint: bool,
    pub crouch: bool,
    progress_pos: Vec3,
    progress_time: f32,
    pub stuck_count: u32,
    nudge_until: f32,
    nudge_dir: Vec3,
    pub breach: BreachStep,
    no_breach_until: f32,
    melee_at: f32,
    /// Look point for activities (overrides the goal's watch).
    pub activity_look: Option<Vec3>,
    /// Corner being checked, until when.
    corner: Option<(Vec3, f32)>,
    scan_at: f32,
    checked: Vec<(Vec3, f32)>,
}

#[derive(Default, Debug)]
pub struct LookState {
    /// Current look angles (what the bot is trying to point at).
    pub yaw: f32,
    pub pitch: f32,
    pub initialised: bool,
    /// Set by combat: angles to aim at this tick (error included).
    pub combat: Option<(f32, f32)>,
    /// Angle to the desired point last tick (for "settled" checks).
    pub error: f32,
}

/// Searches allowed per tick across all bots.
const SEARCHES_PER_TICK: u32 = 1;

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn drive(
    mut bots: Query<(
        Entity,
        &Name,
        &mut Bot,
        &mut BotState,
        &Transform,
        &CharacterState,
        &PlayerInput,
        &Team,
        Has<Dead>,
        Option<&BreachCharges>,
        Has<MovementLock>,
    )>,
    characters: Query<(Entity, &Transform, &Team, Has<Dead>), With<Player>>,
    grid: Res<NavGrid>,
    mut scratch: ResMut<PathScratch>,
    plans: Res<TeamPlans>,
    surfaces: Query<(&SoftSurface, &GlobalTransform)>,
    charges: Query<(&BreachCharge, &Transform), Without<Player>>,
    spatial: SpatialQuery,
    mut stats: ResMut<AiStats>,
    settings: Res<AiSettings>,
    tuning: Res<Tuning>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs();
    let b = &tuning.ai.behaviour;
    let others: Vec<(Entity, Vec3, Team, bool)> = characters
        .iter()
        .map(|(e, t, team, dead)| (e, t.translation, *team, dead))
        .collect();
    let mut searches = 0;
    let filter = SpatialQueryFilter::from_mask(GameLayer::World);
    let _ = &settings;

    for (me, name, mut bot, mut state, transform, body, input, team, dead, charges_left, locked) in &mut bots {
        let feet = transform.translation;
        let state = &mut *state;
        state.buttons.melee = false;
        state.buttons.secondary = false;
        state.buttons.build = false;
        state.buttons.objective = false;
        state.buttons.crouch = false;
        state.buttons.lean = Lean::Center;
        if !matches!(state.nav.breach, BreachStep::Placing { .. }) {
            state.buttons.charge = false;
        }
        state.nav.activity_look = None;
        state.nav.move_dir = Vec3::ZERO;
        state.nav.sprint = false;
        if dead || !grid.ready {
            state.nav.path = None;
            state.nav.breach = BreachStep::None;
            continue;
        }
        let eye = eye_position(feet, input.yaw, body.height, body.lean, &tuning);
        let goal = state.goal.clone();

        // --- Arrival -----------------------------------------------------------
        let arrived = goal.at.is_some_and(|g| {
            let flat = feet.with_y(0.0).distance(g.with_y(0.0));
            flat <= goal.radius + 0.05 && (feet.y - g.y).abs() < 1.3
        });
        state.nav.arrived = arrived || goal.at.is_none();
        let wants_move = goal.at.is_some() && !arrived && !locked;

        // --- Plan ------------------------------------------------------------------
        if wants_move {
            let target = goal.at.expect("checked");
            let moved_goal = state
                .nav
                .planned_for
                .is_none_or(|p| p.distance(target) > 0.6 || state.nav.planned_alt != goal.alt_route);
            let invalidated = state.nav.path.as_ref().is_some_and(|path| {
                grid.changes.iter().any(|(rev, lo, hi)| {
                    *rev > path.revision
                        && path.points[state.nav.index.min(path.points.len().saturating_sub(1))..]
                            .iter()
                            .any(|w| w.pos.x >= lo.x - 0.5 && w.pos.x <= hi.x + 0.5 && w.pos.z >= lo.y - 0.5 && w.pos.z <= hi.y + 0.5)
                })
            });
            let need = state.nav.path.is_none() || moved_goal || invalidated;
            if need && now >= state.nav.next_plan && searches < SEARCHES_PER_TICK {
                searches += 1;
                let from = grid.locate(feet, 3);
                let to = grid.locate(target + Vec3::Y * 0.3, 3);
                let options = PathOptions {
                    barricades: Some(if *team == Team::Attackers { COST_BARRICADE } else { COST_BARRICADE_DEFENDER }),
                    // The second stack opens its own way in when it can.
                    breach: (*team == Team::Attackers
                        && charges_left.is_some_and(|c| c.0 > 0)
                        && now > state.nav.no_breach_until)
                        .then_some(if goal.alt_route { COST_BREACH_PREFERRED } else { COST_BREACH }),
                    max_expansions: 60_000,
                };
                let route = &plans.route;
                let alt = goal.alt_route && plans.route_ready;
                let penalty = move |n| if alt && route.contains(&n) { 2.0 } else { 0.0 };
                let path = match (from, to) {
                    (Some(a), Some(z)) => find_path(&grid, &mut scratch, a, z, options, &penalty),
                    _ => None,
                };
                state.nav.planned_for = Some(target);
                state.nav.planned_alt = goal.alt_route;
                state.nav.index = 0;
                match path {
                    Some(p) => {
                        state.nav.path = Some(p);
                        state.nav.failures = 0;
                        state.nav.next_plan = now + 0.3;
                    }
                    None => {
                        state.nav.path = None;
                        state.nav.failures += 1;
                        state.nav.next_plan = now + 1.0;
                        if state.nav.failures == 1 || state.nav.failures % 10 == 0 {
                            info!(
                                "ai: {name}: no path from {feet:.1} (node {:?}) to {target:.1} (node {:?})",
                                from.map(|n| grid.pos(n)),
                                to.map(|n| grid.pos(n))
                            );
                        }
                        stats.no_path += 1;
                    }
                }
            }
        } else if arrived {
            state.nav.path = None;
            state.nav.planned_for = None;
        }

        // --- Follow --------------------------------------------------------------
        let mut steer = Vec3::ZERO;
        let mut busy = false;
        // Work on the path outside the state (put back unless dropped).
        let mut clear_path = false;
        let taken = state.nav.path.take();
        if wants_move && let Some(path) = taken.as_ref() {
            let mut index = state.nav.index;
            while index < path.points.len() {
                let w = &path.points[index];
                let flat = feet.with_y(0.0).distance(w.pos.with_y(0.0));
                let reached = match w.kind {
                    LinkKind::Drop => flat < 0.6 && feet.y < w.pos.y + 0.6,
                    _ => flat < 0.42 && (feet.y - w.pos.y).abs() < 1.4,
                };
                // Barricades and breaches are handled before stepping on.
                let blocked = w.blocker.is_some_and(|b| surfaces.get(b).is_ok_and(|(s, _)| !s.broken));
                if reached && !blocked {
                    index += 1;
                } else {
                    break;
                }
            }
            state.nav.index = index;
            if let Some(w) = path.points.get(index).cloned() {
                let flat_to = (w.pos - feet).with_y(0.0);
                // Barricade ahead: break it.
                let barricade = w
                    .blocker
                    .and_then(|b| surfaces.get(b).ok().map(|(s, gt)| (s, gt.translation())))
                    .filter(|(s, _)| !s.broken && s.role() == SurfaceRole::Barricade);
                if let Some((_, centre)) = barricade
                    && flat_to.length() < 1.5
                {
                    busy = true;
                    let target = centre + Vec3::Y * 1.0;
                    state.nav.activity_look = Some(target);
                    if state.look.error < 0.25 && now >= state.nav.melee_at {
                        state.buttons.melee = true;
                        state.nav.melee_at = now + 0.9;
                    }
                } else if w.kind == LinkKind::Breach
                    && let Some(link) = w.breach
                {
                    let from = grid.pos(link.from);
                    match state.nav.breach {
                        BreachStep::None if feet.with_y(0.0).distance(from.with_y(0.0)) < 0.5 => {
                            state.nav.breach = BreachStep::Placing { since: now };
                            busy = true;
                        }
                        BreachStep::None => steer = (from - feet).with_y(0.0).normalize_or_zero(),
                        BreachStep::Placing { since } => {
                            busy = true;
                            state.nav.activity_look = Some(link.aim);
                            // Press and hold once facing the wall.
                            state.buttons.charge = state.look.error < 0.12 && now - since > 0.25;
                            if charges.iter().any(|(c, _)| c.owner == me) {
                                let away_dir = (from - grid.pos(link.to)).with_y(0.0).normalize_or_zero();
                                let away = grid
                                    .locate(from + away_dir * 3.0 + Vec3::Y * 0.3, 3)
                                    .map(|n| grid.pos(n))
                                    .unwrap_or(from + away_dir * 3.0);
                                state.nav.breach = BreachStep::BackingOff { since: now, from, away };
                                state.buttons.charge = false;
                            } else if now - since > 4.0
                                || surfaces.get(link.surface).is_ok_and(|(s, _)| s.is_reinforced())
                            {
                                info!("ai: {name} couldn't place a breach charge; going round");
                                state.nav.breach = BreachStep::None;
                                state.nav.no_breach_until = now + 30.0;
                                clear_path = true;
                                state.buttons.charge = false;
                            }
                        }
                        BreachStep::BackingOff { since, from, away } => {
                            busy = true;
                            let far = feet.distance(from) > 2.6 || now - since > 2.5;
                            let mates_clear = others.iter().all(|(e, p, t, d)| {
                                *e == me || *t != *team || *d || p.distance(link.aim) > 3.2
                            });
                            if far && (mates_clear || now - since > 5.0) {
                                state.buttons.charge = true;
                                state.nav.breach = BreachStep::Detonated { at: now };
                                stats.breaches += 1;
                                info!("ai: {name} blew a breach charge at {:.1}", link.aim);
                            } else {
                                steer = (away - feet).with_y(0.0).normalize_or_zero();
                                state.nav.activity_look = Some(link.aim);
                            }
                        }
                        BreachStep::Detonated { at } => {
                            busy = true;
                            if now - at > 0.9 {
                                state.nav.breach = BreachStep::None;
                                clear_path = true;
                                state.nav.next_plan = now;
                            }
                        }
                    }
                } else {
                    steer = flat_to.normalize_or_zero();
                    if flat_to.length() < 0.15 && index + 1 < path.points.len() {
                        steer = (path.points[index + 1].pos - feet).with_y(0.0).normalize_or_zero();
                    }
                }
                state.nav.crouch = w.crouch
                    || path.points.get(index + 1).is_some_and(|n| n.crouch && n.pos.distance(feet) < 1.2);
            }
        }
        if !clear_path && state.nav.path.is_none() {
            state.nav.path = taken;
        }
        if !matches!(state.nav.breach, BreachStep::None) && !wants_move {
            state.nav.breach = BreachStep::None;
        }

        // --- Avoidance and spacing -----------------------------------------------
        if steer != Vec3::ZERO {
            let mut push = Vec3::ZERO;
            let mut slow: f32 = 1.0;
            for (other, at, other_team, other_dead) in &others {
                if *other == me || *other_dead || (at.y - feet.y).abs() > 1.5 {
                    continue;
                }
                let away = (feet - *at).with_y(0.0);
                let d = away.length();
                if d < b.avoid_radius {
                    let dir = if d > 1e-3 { away / d } else { Vec3::new(steer.z, 0.0, -steer.x) };
                    let closeness = 1.0 - d / b.avoid_radius;
                    let ahead = steer.dot(-dir);
                    if ahead > 0.2 {
                        // In the way: step round them (to whichever side
                        // needs the smaller turn) rather than back off.
                        let right = Vec3::new(-steer.z, 0.0, steer.x);
                        let side = if right.dot(dir) >= 0.0 { right } else { -right };
                        push += side * closeness * 0.8 + dir * closeness * 0.3;
                    } else {
                        push += dir * closeness * 0.6;
                    }
                }
                if *other_team == *team
                    && *team == Team::Attackers
                    && matches!(goal.pace, Pace::Careful)
                    && d < b.stack_spacing
                    && d > 1e-3
                    && steer.dot(-away / d) > 0.6
                {
                    slow = slow.min(0.35);
                }
            }
            // Gentle on ramps and ledges: a shove there means a fall.
            let footing = grid
                .locate(feet, 0)
                .map_or(0, |n| grid.node(n).flags & (super::nav::SLOPE | super::nav::LEDGE));
            let push_weight = if footing != 0 { 0.35 } else { 1.0 };
            steer = steer * slow + push * push_weight;
            if steer.length() > 1.0 {
                steer = steer.normalize();
            }
        }

        // Standing still to shoot isn't being stuck.
        let fighting = state.combat.target.is_some();
        if fighting && state.combat.stand_still {
            steer = Vec3::ZERO;
        }

        // --- Stuck -----------------------------------------------------------------
        let trying = steer.length() > 0.2 && !busy;
        if !trying || now < state.nav.nudge_until {
            state.nav.progress_pos = feet;
            state.nav.progress_time = now;
        } else if now - state.nav.progress_time > b.stuck_time {
            if feet.distance(state.nav.progress_pos) < 0.3 {
                state.nav.stuck_count += 1;
                stats.stuck += 1;
                let n = state.nav.stuck_count;
                let next = state.nav.path.as_ref().and_then(|p| p.points.get(state.nav.index)).map(|w| (w.pos, w.kind, w.crouch));
                info!(
                    "ai: {name} stuck at {feet:.2} (try {n}); goal {:?}, waypoint {:?}, steer {:.2}",
                    goal.at.map(|g| (g * 10.0).round() / 10.0),
                    next,
                    steer
                );
                state.nav.path = None;
                state.nav.next_plan = now;
                if n >= 2 {
                    let side = if bot.rng.chance(0.5) { 1.0 } else { -1.0 };
                    let back = if n >= 3 { -0.7 } else { 0.0 };
                    state.nav.nudge_dir = (Vec3::new(-steer.z, 0.0, steer.x) * side + steer * back).normalize_or_zero();
                    state.nav.nudge_until = now + 0.7;
                }
                if n >= 3 {
                    // Something in front at chest height (a barricade the
                    // grid doesn't know about yet)? Hit it.
                    let ahead = Dir3::new(steer).unwrap_or(Dir3::NEG_Z);
                    if spatial.cast_ray(feet + Vec3::Y * 1.0, ahead, 1.3, true, &filter).is_some() {
                        state.buttons.melee = true;
                    }
                }
            } else {
                state.nav.stuck_count = state.nav.stuck_count.saturating_sub(1);
            }
            state.nav.progress_pos = feet;
            state.nav.progress_time = now;
        }
        if now < state.nav.nudge_until {
            steer = state.nav.nudge_dir;
        }

        // --- Pace ------------------------------------------------------------------
        let look = look_dir(input.yaw, 0.0);
        let forward = steer.dot(look) > 0.85;
        state.nav.sprint = matches!(goal.pace, Pace::Run) && !fighting && forward && !state.nav.crouch;
        state.nav.move_dir = steer;

        // --- Activities at the goal -------------------------------------------------
        let at_goal = state.nav.arrived;
        match goal.kind {
            GoalKind::Reinforce { aim, .. } | GoalKind::Barricade { aim, .. } if at_goal => {
                state.nav.activity_look = Some(aim);
                state.nav.move_dir = Vec3::ZERO;
                state.buttons.build = state.look.error < 0.07 && input.move_axis == Vec2::ZERO;
            }
            GoalKind::Stake { facing } if at_goal => {
                state.nav.move_dir = Vec3::ZERO;
                state.nav.activity_look = Some(feet + facing * 1.8 + Vec3::Y * 0.6);
                if state.look.error < 0.08 && state.mind.pressed_at.is_none_or(|t| now - t > 1.5) {
                    state.buttons.gadget = true;
                    state.mind.pressed_at = Some(now);
                }
            }
            GoalKind::Throw { aim } => {
                // Face it with a little arc, hold the key briefly, let go.
                state.nav.move_dir = Vec3::ZERO;
                let lob = aim + Vec3::Y * (aim.distance(feet) * 0.12).min(2.0);
                state.nav.activity_look = Some(lob);
                match state.mind.pressed_at {
                    None if state.look.error < 0.05 => {
                        state.mind.pressed_at = Some(now);
                        state.buttons.secondary = true;
                    }
                    Some(t) if now - t < 0.35 => state.buttons.secondary = true,
                    Some(t) if now - t < 0.4 => {
                        state.buttons.secondary = false;
                        stats.smokes += 1;
                        info!("ai: {name} throws a smoke at {aim:.1}");
                    }
                    _ => state.buttons.secondary = false,
                }
            }
            GoalKind::Dart { aim } => {
                state.nav.move_dir = Vec3::ZERO;
                state.nav.activity_look = Some(aim);
                if state.look.error < 0.05 && state.mind.pressed_at.is_none() {
                    state.buttons.gadget = true;
                    state.mind.pressed_at = Some(now);
                    stats.darts += 1;
                }
            }
            GoalKind::Plant if at_goal => {
                state.nav.move_dir = Vec3::ZERO;
                state.buttons.objective = true;
            }
            GoalKind::Disable => {
                let at = goal.at.unwrap_or(feet);
                let reach = tuning.round.disable_reach - 0.25;
                if feet.with_y(0.0).distance(at.with_y(0.0)) <= reach && (feet.y - at.y).abs() < 1.1 {
                    state.nav.move_dir = Vec3::ZERO;
                    state.buttons.objective = true;
                }
            }
            _ => {}
        }
        state.buttons.crouch = state.nav.crouch
            || (at_goal && goal.crouch && !matches!(goal.kind, GoalKind::Move | GoalKind::PickUp))
            || matches!(goal.pace, Pace::Sneak)
            || state.combat.crouch;
        state.buttons.lean = if state.combat.target.is_some() { state.combat.lean } else if at_goal { goal.lean } else { Lean::Center };

        // --- Corner checks while moving carefully ------------------------------------
        if matches!(goal.pace, Pace::Careful | Pace::Sneak) && !at_goal && state.nav.move_dir != Vec3::ZERO {
            if state.nav.corner.is_some_and(|(_, until)| now > until) {
                state.nav.corner = None;
            }
            if state.nav.corner.is_none() && now >= state.nav.scan_at {
                state.nav.scan_at = now + 0.35 + bot.rng.f32() * 0.2;
                state.nav.checked.retain(|(_, t)| now - t < 3.0);
                if let Some(point) = scan_corners(&spatial, eye, state.nav.move_dir, &state.nav.checked) {
                    state.nav.checked.push((point, now));
                    state.nav.corner = Some((point, now + 0.6 + bot.rng.f32() * 0.3));
                }
            }
        } else {
            state.nav.corner = None;
        }
    }
}

/// Fan of rays around the walking direction; where the depth jumps there
/// is a corner or an opening someone could be behind. Returns the point
/// just past the nearest one (not looked at recently).
fn scan_corners(spatial: &SpatialQuery, eye: Vec3, heading: Vec3, checked: &[(Vec3, f32)]) -> Option<Vec3> {
    let filter = SpatialQueryFilter::from_mask(GameLayer::World);
    const RAYS: usize = 9;
    const RANGE: f32 = 14.0;
    let base = heading.with_y(0.0).normalize_or_zero();
    if base == Vec3::ZERO {
        return None;
    }
    let mut lengths = [RANGE; RAYS];
    let mut dirs = [Vec3::ZERO; RAYS];
    for i in 0..RAYS {
        let angle = (i as f32 / (RAYS - 1) as f32 - 0.5) * 160f32.to_radians();
        let dir = Quat::from_rotation_y(angle) * base;
        dirs[i] = dir;
        if let Ok(d) = Dir3::new(dir) {
            lengths[i] = spatial.cast_ray(eye, d, RANGE, true, &filter).map_or(RANGE, |h| h.distance);
        }
    }
    let mut best: Option<(f32, Vec3)> = None;
    for i in 0..RAYS - 1 {
        let (a, b) = (lengths[i], lengths[i + 1]);
        if (a - b).abs() < 2.5 {
            continue;
        }
        let (short, long_dir) = if a < b { (a, dirs[i + 1]) } else { (b, dirs[i]) };
        if short > 10.0 {
            continue;
        }
        let point = eye + long_dir * (short + 1.5);
        if checked.iter().any(|(p, _)| p.distance(point) < 1.5) {
            continue;
        }
        if best.is_none_or(|(s, _)| short < s) {
            best = Some((short, point));
        }
    }
    best.map(|(_, p)| p)
}

/// Turn the view towards what matters, at the difficulty's turn speed.
#[allow(clippy::type_complexity)]
fn turn(
    mut bots: Query<(&mut BotState, &Transform, &CharacterState, &PlayerInput, Has<Dead>), With<Bot>>,
    settings: Res<AiSettings>,
    tuning: Res<Tuning>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    let d = tuning.ai.preset(settings.level(&tuning));
    let max_turn = d.turn_speed.to_radians() * dt;
    for (mut state, transform, body, input, dead) in &mut bots {
        let state = &mut *state;
        if !state.look.initialised {
            state.look.yaw = input.yaw;
            state.look.pitch = input.pitch;
            state.look.initialised = true;
        }
        if dead {
            continue;
        }
        let feet = transform.translation;
        let eye = eye_position(feet, state.look.yaw, body.height, body.lean, &tuning);
        let wanted: Option<(f32, f32, f32)> = if let Some((yaw, pitch)) = state.look.combat {
            Some((yaw, pitch, 1.0))
        } else {
            let point = state
                .nav
                .activity_look
                .or(state.combat.glance)
                .or_else(|| state.nav.corner.map(|(p, _)| p))
                .or(if state.nav.arrived { state.goal.watch } else { None })
                .or_else(|| {
                    // The way ahead, at eye height; a doorway coming up is
                    // pre-aimed a little beyond.
                    let path = state.nav.path.as_ref()?;
                    let w = path.points.get(state.nav.index)?;
                    let next = path.points.get(state.nav.index + 1).map(|n| n.pos).unwrap_or(w.pos);
                    let dir = (next - w.pos).with_y(0.0).normalize_or((w.pos - feet).with_y(0.0).normalize_or(Vec3::NEG_Z));
                    let near = w.pos.distance(feet) < 5.0;
                    let aim = if near { w.pos + dir * 2.5 } else { w.pos };
                    Some(aim.with_y(feet.y + 1.5))
                })
                .or(state.goal.watch);
            point.and_then(|p| {
                let to = p - eye;
                (to.length() > 0.3).then(|| {
                    let (yaw, pitch) = angles_to(to);
                    (yaw, pitch, 0.6)
                })
            })
        };
        let (yaw, pitch, speed) = wanted.unwrap_or((state.look.yaw, state.look.pitch * 0.9, 0.5));
        let dy = angle_diff(yaw, state.look.yaw);
        let dp = pitch - state.look.pitch;
        state.look.error = (dy * dy + dp * dp).sqrt();
        // Ease in, capped by the turn speed.
        let ease = 1.0 - (-dt * 14.0).exp();
        let step = max_turn * speed;
        state.look.yaw = (state.look.yaw + (dy * ease.max(0.35)).clamp(-step, step)).rem_euclid(std::f32::consts::TAU);
        state.look.pitch = (state.look.pitch + (dp * ease.max(0.35)).clamp(-step, step)).clamp(-1.4, 1.4);
        let _ = now;
    }
}

/// Write the intents the simulation reads.
#[allow(clippy::type_complexity)]
fn write_intents(
    mut bots: Query<
        (
            &mut BotState,
            &mut PlayerInput,
            &mut WeaponInput,
            &mut DestructionIntent,
            &mut RoundIntent,
            &mut GadgetIntent,
            Option<&WeaponState>,
            Has<Dead>,
            Has<MovementLock>,
        ),
        With<Bot>,
    >,
    settings: Res<AiSettings>,
    tuning: Res<Tuning>,
) {
    let d = tuning.ai.preset(settings.level(&tuning));
    for (mut state, mut input, mut weapon, mut destruction, mut round, mut gadget, weapon_state, dead, locked) in &mut bots {
        let state = &mut *state;
        if dead {
            input.move_axis = Vec2::ZERO;
            input.sprint = false;
            // Lie flat so the hit zones go down with the body.
            input.stance = Stance::Prone;
            input.lean = Lean::Center;
            weapon.trigger = false;
            weapon.aim = false;
            *destruction = DestructionIntent::default();
            round.interact = false;
            continue;
        }
        // Look, with imperfect recoil control while shooting.
        let recoil = weapon_state.map(|w| w.recoil).unwrap_or(Vec2::ZERO);
        let k = d.recoil_control;
        input.yaw = (state.look.yaw + (recoil.x * k).to_radians()).rem_euclid(std::f32::consts::TAU);
        input.pitch = (state.look.pitch - (recoil.y * k).to_radians()).clamp(-1.5, 1.5);
        // Move relative to the facing.
        let facing = Quat::from_rotation_y(input.yaw);
        let local = facing.inverse() * state.nav.move_dir;
        input.move_axis = if locked { Vec2::ZERO } else { Vec2::new(local.x, -local.z).clamp_length_max(1.0) };
        input.sprint = state.nav.sprint && !locked && input.move_axis.y > 0.6;
        input.stance = if state.buttons.crouch && !input.sprint { Stance::Crouch } else { Stance::Stand };
        input.lean = if input.sprint { Lean::Center } else { state.buttons.lean };

        let b = &state.buttons;
        if b.trigger && !weapon.trigger {
            weapon.trigger_presses = weapon.trigger_presses.wrapping_add(1);
        }
        weapon.trigger = b.trigger;
        weapon.aim = b.aim;
        weapon.slot = b.slot;
        if b.reload {
            weapon.reload_requests = weapon.reload_requests.wrapping_add(1);
        }
        destruction.melee = b.melee;
        destruction.interact = b.build;
        destruction.gadget = b.charge;
        round.interact = b.objective;
        if b.gadget {
            gadget.use_unique = true;
        }
        gadget.secondary_held = b.secondary;
        state.buttons.reload = false;
        state.buttons.gadget = false;
    }
}
