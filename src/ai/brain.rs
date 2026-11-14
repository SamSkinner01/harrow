//! Decisions: what each bot wants to do right now, as a [`Goal`] (where to
//! be, how to move, what to watch, what to do there). Runs a few times a
//! second per bot, staggered; the motor and combat code act on it every
//! tick.
//!
//! **Defenders.** When prep starts the team splits into anchors (one or
//! two per site) and `behaviour.roamers` roamers, and shares out jobs:
//! reinforce the walls and hatches around the sites (each defender has
//! two), stakes outside the entrances (Fathoms), then barricades on the
//! site doorways. With the jobs done, anchors hold angles on the entrances
//! from hold spots (crouching or not, swapping spots now and then, turning
//! towards anything heard or pinged), roamers move between indoor spots
//! away from the sites and go after fresh contacts. Once the defuser is
//! down, the nearest defender goes to disable it and the rest cover it.
//!
//! **Attackers.** They wait at their spawn in prep. In the action phase the
//! team picks a site and splits into two stacks that take different routes
//! when the map allows (the second avoids the first's route). Far from the
//! site they run; within `careful_range` they walk, check corners and
//! pre-aim openings. Larks fire sensor darts at the site on the way in.
//! The carrier plants inside the site once no enemy is in sight; the others
//! take hold spots covering the entrances, and then cover the defuser. A
//! dropped defuser is picked up by the nearest attacker.
//!
//! **Everyone** breaks off to cover when badly hurt in a fight (reloading
//! there), and acts on what the team knows ([`TeamIntel`]).

use avian3d::prelude::*;
use bevy::{platform::collections::HashSet, prelude::*};

use super::{
    AiSettings, AiSystems, Bot,
    combat::CombatState,
    motor::{Buttons, LookState, NavState},
    nav::{NavGrid, NodeId},
    path::{PathOptions, PathScratch, find_path},
    senses::{ContactKind, Sighting, TeamIntel},
    tactics::{DoorJob, Tactics, WallJob, find_cover},
};
use crate::{
    combat::{Dead, Health, Team},
    config::Tuning,
    destruction::{Reinforcements, SoftSurface, SurfaceRole},
    gadgets::{GadgetKind, UniqueGadget},
    player::{Lean, Player},
    round::{
        Round, RoundPhase, RoundReset,
        defuser::{DefuserCarrier, DefuserDevice, DroppedDefuser},
        sites::{KeepLayout, SiteId},
    },
    weapons::WeaponState,
};

pub struct BrainPlugin;

impl Plugin for BrainPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TeamPlans>().add_systems(
            FixedPreUpdate,
            (reset_minds, plan_rounds, think).chain().in_set(AiSystems::Think),
        );
    }
}

/// Everything a bot carries between ticks.
#[derive(Component, Default, Debug)]
pub struct BotState {
    pub goal: Goal,
    pub senses: SenseState,
    pub nav: NavState,
    pub look: LookState,
    pub combat: CombatState,
    pub buttons: Buttons,
    pub mind: Mind,
}

/// What the bot noticed (written by `senses.rs`).
#[derive(Default, Debug)]
pub struct SenseState {
    pub visible: Vec<Sighting>,
    pub heard: Option<super::senses::Heard>,
    /// Last hit by (who, where they were, when).
    pub hurt: Option<(Entity, Vec3, f32)>,
}

/// Where to be and what to do there.
#[derive(Clone, Debug)]
pub struct Goal {
    pub kind: GoalKind,
    /// Where to go (None = stay).
    pub at: Option<Vec3>,
    pub pace: Pace,
    /// What to look at when not fighting.
    pub watch: Option<Vec3>,
    /// Crouch once there.
    pub crouch: bool,
    pub lean: Lean,
    /// Close enough, metres.
    pub radius: f32,
    /// Take the second attack route.
    pub alt_route: bool,
}

impl Default for Goal {
    fn default() -> Self {
        Self {
            kind: GoalKind::Idle,
            at: None,
            pace: Pace::Walk,
            watch: None,
            crouch: false,
            lean: Lean::Center,
            radius: 0.5,
            alt_route: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum GoalKind {
    #[default]
    Idle,
    Move,
    Hold,
    Reinforce { surface: Entity, aim: Vec3 },
    Barricade { slot: Entity, aim: Vec3 },
    Stake { facing: Vec3 },
    Dart { aim: Vec3 },
    Plant,
    Disable,
    PickUp,
    Retreat,
    Investigate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Pace {
    /// Sprint when the way ahead is straight.
    Run,
    #[default]
    Walk,
    /// Walk, checking corners and openings.
    Careful,
    /// Crouch-walk (quiet).
    Sneak,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Role {
    #[default]
    None,
    Anchor,
    Roamer,
    Attacker,
}

#[derive(Clone, Debug)]
pub enum Job {
    Reinforce(WallJob),
    Barricade(DoorJob),
    Stake(Vec3, Vec3),
}

/// The brain's own memory.
#[derive(Default, Debug)]
pub struct Mind {
    pub role: Role,
    pub round: u32,
    pub site: Option<SiteId>,
    pub jobs: Vec<Job>,
    /// When the bot reached the current job's spot.
    pub job_arrived: Option<f32>,
    /// Gadget count when the current stake/dart job started.
    pub gadget_before: Option<u8>,
    pub hold: usize,
    pub hold_since: f32,
    pub alt_route: bool,
    pub retreat_until: f32,
    pub retreat_cooldown: f32,
    pub roam: Option<Vec3>,
    pub roam_until: f32,
    pub dart_at: f32,
    /// When the bot arrived at its goal (for activities).
    pub arrived_at: Option<f32>,
    /// Activity button timing (press once, then wait).
    pub pressed_at: Option<f32>,
}

/// Team-level plans made at phase changes.
#[derive(Resource, Default)]
pub struct TeamPlans {
    pub defense_round: u32,
    pub attack_round: u32,
    pub target: Option<SiteId>,
    /// Nodes near the first stack's route (the second avoids them).
    pub route: HashSet<NodeId>,
    pub route_ready: bool,
}

fn same_goal(a: &Goal, b: &Goal) -> bool {
    std::mem::discriminant(&a.kind) == std::mem::discriminant(&b.kind)
        && match (a.at, b.at) {
            (Some(x), Some(y)) => x.distance(y) < 0.6,
            (None, None) => true,
            _ => false,
        }
}

fn set_goal(state: &mut BotState, goal: Goal) {
    if !same_goal(&state.goal, &goal) {
        state.mind.arrived_at = None;
        state.mind.pressed_at = None;
    }
    state.goal = goal;
}

/// New round: forget the last one (team knowledge, goals, paths).
fn reset_minds(
    mut resets: MessageReader<RoundReset>,
    mut intel: ResMut<TeamIntel>,
    mut bots: Query<&mut BotState, With<Bot>>,
) {
    if resets.read().last().is_none() {
        return;
    }
    intel.clear();
    for mut state in &mut bots {
        let state = &mut *state;
        state.goal = Goal::default();
        state.senses = SenseState::default();
        state.combat = CombatState::default();
        state.nav = NavState::default();
        state.mind.jobs.clear();
        state.mind.arrived_at = None;
        state.mind.pressed_at = None;
        state.mind.retreat_until = 0.0;
        state.mind.retreat_cooldown = 0.0;
        state.mind.roam = None;
        state.mind.dart_at = 0.0;
    }
}

/// Plan the defence when prep starts and the attack when action starts.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn plan_rounds(
    round: Res<Round>,
    mut plans: ResMut<TeamPlans>,
    tactics: Res<Tactics>,
    grid: Res<NavGrid>,
    mut scratch: ResMut<PathScratch>,
    layout: Option<Res<KeepLayout>>,
    mut bots: Query<
        (Entity, &Transform, &Team, &mut BotState, &mut Bot, Option<&Reinforcements>, Option<&UniqueGadget>),
        Without<Dead>,
    >,
    tuning: Res<Tuning>,
) {
    // Defence: at prep (operators and gadget counts are set by then).
    if round.phase == RoundPhase::Prep && plans.defense_round != round.number && tactics.ready && round.elapsed > 0.05 {
        plans.defense_round = round.number;
        plan_defence(&tactics, &mut bots, &tuning);
    }
    // Attack: as soon as the map is understood (routes are worked out in prep).
    if matches!(round.phase, RoundPhase::Prep | RoundPhase::Action)
        && plans.attack_round != round.number
        && tactics.ready
        && grid.ready
    {
        plans.attack_round = round.number;
        let Some(layout) = layout else { return };
        let mut attackers: Vec<_> = bots.iter_mut().filter(|b| *b.2 == Team::Attackers).collect();
        if attackers.is_empty() || tactics.sites.is_empty() {
            return;
        }
        let pick = (round.number as usize + attackers[0].4.rng.below(2)) % tactics.sites.len();
        let site = &tactics.sites[pick];
        plans.target = Some(site.id);
        // The first stack's route, from the spawns to the site.
        let spawn_centre = layout.attacker_spawns.iter().map(|s| s.position).sum::<Vec3>()
            / layout.attacker_spawns.len().max(1) as f32;
        plans.route.clear();
        plans.route_ready = false;
        if let (Some(from), Some(to)) = (
            grid.locate(spawn_centre + Vec3::Y * 0.3, 4),
            grid.locate(site.inside_point(&grid, 0) + Vec3::Y * 0.3, 2),
        ) {
            let options = PathOptions { barricades: Some(super::path::COST_BARRICADE), breach: false, max_expansions: 120_000 };
            if let Some(path) = find_path(&grid, &mut scratch, from, to, options, &|_| 0.0) {
                for w in &path.points {
                    for id in grid.nodes_near(w.pos, 1.6, 1.0) {
                        plans.route.insert(id);
                    }
                }
                // Points between sparse waypoints too.
                for pair in path.points.windows(2) {
                    let (a, b) = (pair[0].pos, pair[1].pos);
                    let steps = (a.distance(b) / 0.8).ceil() as i32;
                    for i in 0..steps {
                        let p = a.lerp(b, i as f32 / steps as f32);
                        for id in grid.nodes_near(p, 1.6, 1.0) {
                            plans.route.insert(id);
                        }
                    }
                }
                plans.route_ready = true;
            }
        }
        attackers.sort_by_key(|b| b.0);
        let n = attackers.len();
        for (i, (_, _, _, state, ..)) in attackers.iter_mut().enumerate() {
            state.mind.role = Role::Attacker;
            state.mind.site = Some(site.id);
            state.mind.round = round.number;
            // First half (rounded up) is the first stack.
            state.mind.alt_route = i >= n.div_ceil(2);
            state.mind.jobs.clear();
        }
        info!(
            "ai: attackers go for site {} ({} bots, second stack {} route)",
            site.id.letter(),
            n,
            if plans.route_ready { "avoids the first" } else { "shares the" }
        );
    }
}

#[allow(clippy::type_complexity)]
fn plan_defence(
    tactics: &Tactics,
    bots: &mut Query<
        (Entity, &Transform, &Team, &mut BotState, &mut Bot, Option<&Reinforcements>, Option<&UniqueGadget>),
        Without<Dead>,
    >,
    tuning: &Tuning,
) {
    let mut defenders: Vec<_> = bots.iter_mut().filter(|b| *b.2 == Team::Defenders).collect();
    if defenders.is_empty() || tactics.sites.is_empty() {
        return;
    }
    defenders.sort_by_key(|b| b.0);
    let n = defenders.len();
    let roamers = (tuning.ai.behaviour.roamers as usize).min(n.saturating_sub(1));
    // Fathoms roam first (stakes go out on the way).
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| {
        let fathom = defenders[i].6.is_some_and(|g| g.kind == GadgetKind::SeismicStake);
        (!fathom, i)
    });
    let roamer_set: Vec<usize> = order.iter().take(roamers).copied().collect();
    let mut anchor_index = 0;
    for (i, d) in defenders.iter_mut().enumerate() {
        let state = &mut d.3;
        state.mind.jobs.clear();
        state.mind.job_arrived = None;
        state.mind.gadget_before = None;
        if roamer_set.contains(&i) {
            state.mind.role = Role::Roamer;
            // Roamers still look after the site nearest them.
            state.mind.site = Some(tactics.sites[i % tactics.sites.len()].id);
        } else {
            state.mind.role = Role::Anchor;
            state.mind.site = Some(tactics.sites[anchor_index % tactics.sites.len()].id);
            state.mind.hold = anchor_index / tactics.sites.len();
            anchor_index += 1;
        }
    }
    // Reinforcements: each wall to the nearest defender of its site with
    // some left, else anyone.
    let mut left: Vec<u32> = defenders.iter().map(|d| d.5.map_or(0, |r| r.0)).collect();
    for site in &tactics.sites {
        for wall in &site.walls {
            let pick = (0..n)
                .filter(|&i| left[i] > 0)
                .min_by(|&a, &b| {
                    let cost = |i: usize| {
                        let d = defenders[i].1.translation.distance(wall.stand);
                        let other_site = defenders[i].3.mind.site != Some(site.id);
                        d + if other_site { 25.0 } else { 0.0 }
                    };
                    cost(a).total_cmp(&cost(b))
                });
            if let Some(i) = pick {
                left[i] -= 1;
                defenders[i].3.mind.jobs.push(Job::Reinforce(wall.clone()));
            }
        }
    }
    // Stakes for Fathoms.
    let mut stake_spots: Vec<(Vec3, Vec3)> = Vec::new();
    for site in &tactics.sites {
        for &(p, dir) in &site.stakes {
            if stake_spots.iter().all(|(q, _)| q.distance(p) > 3.0) {
                stake_spots.push((p, dir));
            }
        }
    }
    for d in defenders.iter_mut() {
        let Some(gadget) = d.6 else { continue };
        if gadget.kind != GadgetKind::SeismicStake {
            continue;
        }
        let from = d.1.translation;
        stake_spots.sort_by(|a, b| a.0.distance(from).total_cmp(&b.0.distance(from)));
        let take = (gadget.remaining as usize).min(stake_spots.len());
        for (p, dir) in stake_spots.drain(..take) {
            d.3.mind.jobs.push(Job::Stake(p, dir));
        }
    }
    // Barricades: site doorways, to that site's anchors (last job).
    for site in &tactics.sites {
        for (k, door) in site.doors.iter().enumerate() {
            let anchors: Vec<usize> = (0..n)
                .filter(|&i| defenders[i].3.mind.site == Some(site.id) && defenders[i].3.mind.role == Role::Anchor)
                .collect();
            let i = if anchors.is_empty() { k % n } else { anchors[k % anchors.len()] };
            defenders[i].3.mind.jobs.push(Job::Barricade(door.clone()));
        }
    }
    let summary: Vec<String> = defenders
        .iter()
        .map(|d| {
            let s = &d.3.mind;
            format!(
                "{:?}/{:?} {} jobs",
                s.role,
                s.site.map(|x| x.letter()).unwrap_or("-"),
                s.jobs.len()
            )
        })
        .collect();
    info!("ai: defence plan: {}", summary.join(", "));
}

/// Read-only facts about every character, gathered once per tick.
struct Others {
    list: Vec<(Entity, Vec3, Team, bool)>,
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn think(
    mut bots: Query<(
        Entity,
        &mut Bot,
        &mut BotState,
        &Transform,
        &Team,
        &Health,
        Has<Dead>,
        Option<&DefuserCarrier>,
        Option<&UniqueGadget>,
        Option<&Reinforcements>,
        Option<&WeaponState>,
    )>,
    characters: Query<(Entity, &Transform, &Team, Has<Dead>), With<Player>>,
    round: Res<Round>,
    plans: Res<TeamPlans>,
    tactics: Res<Tactics>,
    grid: Res<NavGrid>,
    intel: Res<TeamIntel>,
    layout: Option<Res<KeepLayout>>,
    devices: Query<(&Transform, &DefuserDevice), Without<Player>>,
    dropped: Query<&Transform, (With<DroppedDefuser>, Without<Player>)>,
    surfaces: Query<(&'static SoftSurface, &'static GlobalTransform)>,
    spatial: SpatialQuery,
    settings: Res<AiSettings>,
    tuning: Res<Tuning>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs();
    let b = &tuning.ai.behaviour;
    let d = tuning.ai.preset(settings.level(&tuning));
    let others = Others {
        list: characters
            .iter()
            .map(|(e, t, team, dead)| (e, t.translation, *team, dead))
            .collect(),
    };
    let device = devices.iter().find(|(_, d)| !d.disabled).map(|(t, d)| (t.translation, d.disabler, d.site));
    let dropped_at = dropped.iter().next().map(|t| t.translation);
    let eye_height = tuning.body.stand_height - tuning.body.eye_from_top;

    for (me, mut bot, mut state, transform, team, health, dead, carrier, gadget, reinforcements, weapon) in &mut bots {
        if now < bot.next_think {
            continue;
        }
        bot.next_think = if bot.next_think == 0.0 {
            now + b.think_interval * (bot.index % 5) as f32 / 5.0
        } else {
            now + b.think_interval
        };
        let feet = transform.translation;
        if dead {
            set_goal(&mut state, Goal::default());
            continue;
        }
        if state.goal.at.is_some_and(|at| at.distance(feet) <= state.goal.radius.max(0.4) + 0.3) {
            state.mind.arrived_at.get_or_insert(now);
        }

        // Badly hurt in a fight: break off to cover (not mid-plant/disable).
        let busy_objective = matches!(state.goal.kind, GoalKind::Plant | GoalKind::Disable)
            && state.mind.arrived_at.is_some();
        let threat = state.senses.visible.first().map(|s| s.feet);
        if health.current < b.retreat_health
            && !busy_objective
            && now > state.mind.retreat_cooldown
            && state.goal.kind != GoalKind::Retreat
            && let Some(threat) = threat
            && let Some(cover) = find_cover(&grid, &spatial, feet, threat + Vec3::Y * eye_height, eye_height, 7.0)
        {
            state.mind.retreat_until = now + 6.0;
            state.mind.retreat_cooldown = now + 14.0;
            set_goal(&mut state, Goal {
                kind: GoalKind::Retreat,
                at: Some(cover),
                pace: Pace::Run,
                watch: Some(threat + Vec3::Y * 1.2),
                crouch: true,
                radius: 0.5,
                ..default()
            });
            continue;
        }
        if state.goal.kind == GoalKind::Retreat {
            let reloading_or_full = weapon.is_none_or(|w| {
                let def = w.active.def(&tuning);
                w.ammo().magazine + 1 >= def.magazine || w.ammo().reserve == 0
            });
            if now < state.mind.retreat_until && !(state.mind.arrived_at.is_some_and(|t| now - t > 2.5) && reloading_or_full) {
                continue;
            }
        }

        let ctx = Ctx {
            me,
            feet,
            team: *team,
            now,
            plans: &plans,
            tactics: &tactics,
            grid: &grid,
            intel: &intel,
            layout: layout.as_deref(),
            others: &others,
            device,
            dropped_at,
            surfaces: &surfaces,
            tuning: &tuning,
            crouch_chance: d.crouch_chance,
        };
        let goal = match round.phase {
            RoundPhase::Prep if *team == Team::Defenders => {
                defender_jobs(&ctx, &mut state, &mut bot, gadget, reinforcements)
                    .unwrap_or_else(|| defend(&ctx, &mut state, &mut bot))
            }
            RoundPhase::Prep => idle_towards_objective(&ctx),
            RoundPhase::Action if *team == Team::Defenders => {
                let late = round.elapsed > 20.0;
                let jobs = if late { None } else { defender_jobs(&ctx, &mut state, &mut bot, gadget, reinforcements) };
                jobs.unwrap_or_else(|| defend(&ctx, &mut state, &mut bot))
            }
            RoundPhase::Action => attack(&ctx, &mut state, &mut bot, carrier, gadget),
            RoundPhase::Planted if *team == Team::Defenders => retake(&ctx, &mut state),
            RoundPhase::Planted => guard_plant(&ctx, &mut state, &mut bot),
            _ => Goal::default(),
        };
        set_goal(&mut state, goal);
    }
}

struct Ctx<'a, 'w, 's> {
    me: Entity,
    feet: Vec3,
    team: Team,
    now: f32,
    plans: &'a TeamPlans,
    tactics: &'a Tactics,
    grid: &'a NavGrid,
    intel: &'a TeamIntel,
    layout: Option<&'a KeepLayout>,
    others: &'a Others,
    device: Option<(Vec3, Option<Entity>, SiteId)>,
    dropped_at: Option<Vec3>,
    surfaces: &'a Query<'w, 's, (&'static SoftSurface, &'static GlobalTransform)>,
    tuning: &'a Tuning,
    crouch_chance: f32,
}

impl Ctx<'_, '_, '_> {
    /// Living team mates (not me).
    fn mates(&self) -> impl Iterator<Item = (Entity, Vec3)> + '_ {
        self.others
            .list
            .iter()
            .filter(move |(e, _, t, dead)| *e != self.me && *t == self.team && !dead)
            .map(|(e, p, _, _)| (*e, *p))
    }

    /// Am I the closest living team mate to `p`?
    fn closest_to(&self, p: Vec3) -> bool {
        let mine = self.feet.distance(p);
        self.mates().all(|(_, q)| q.distance(p) >= mine)
    }

    /// Rank (0 = closest) among living team mates by distance to `p`.
    fn rank_to(&self, p: Vec3) -> usize {
        let mine = self.feet.distance(p);
        self.mates().filter(|(_, q)| q.distance(p) < mine).count()
    }
}

/// Prep for attackers: stand at the spawn, looking towards the objective.
fn idle_towards_objective(ctx: &Ctx) -> Goal {
    let watch = ctx.tactics.centre + Vec3::Y * 1.5;
    Goal {
        watch: Some(watch),
        ..default()
    }
}

/// The next unfinished defender job, as a goal.
fn defender_jobs(
    ctx: &Ctx,
    state: &mut BotState,
    bot: &mut Bot,
    gadget: Option<&UniqueGadget>,
    reinforcements: Option<&Reinforcements>,
) -> Option<Goal> {
    loop {
        let job = state.mind.jobs.first()?.clone();
        let arrived = state.mind.job_arrived;
        let done = match &job {
            Job::Reinforce(w) => {
                let reinforced = ctx.surfaces.get(w.surface).map_or(true, |(s, _)| !s.can_reinforce());
                reinforced
                    || reinforcements.is_none_or(|r| r.0 == 0)
                    || arrived.is_some_and(|t| ctx.now - t > 9.0)
            }
            Job::Barricade(door) => {
                let standing = ctx.surfaces.iter().any(|(s, gt)| {
                    s.role() == SurfaceRole::Barricade && !s.broken && gt.translation().distance(door.at) < 0.35
                });
                standing || arrived.is_some_and(|t| ctx.now - t > 5.0)
            }
            Job::Stake(..) => {
                let used = match (gadget, state.mind.gadget_before) {
                    (Some(g), Some(before)) => g.remaining < before,
                    (None, _) => true,
                    _ => false,
                };
                used || gadget.is_some_and(|g| g.remaining == 0) || arrived.is_some_and(|t| ctx.now - t > 3.0)
            }
        };
        if done {
            state.mind.jobs.remove(0);
            state.mind.job_arrived = None;
            state.mind.gadget_before = None;
            continue;
        }
        let (kind, at, crouch, watch, radius) = match &job {
            Job::Reinforce(w) => (
                GoalKind::Reinforce { surface: w.surface, aim: w.aim },
                w.stand,
                w.crouch,
                Some(w.aim),
                0.35,
            ),
            Job::Barricade(door) => (
                GoalKind::Barricade { slot: door.slot, aim: door.aim },
                door.stand,
                false,
                Some(door.aim),
                0.4,
            ),
            Job::Stake(p, dir) => {
                if state.mind.gadget_before.is_none() {
                    state.mind.gadget_before = gadget.map(|g| g.remaining);
                }
                (
                    GoalKind::Stake { facing: *dir },
                    *p,
                    false,
                    Some(*p + *dir * 1.2 + Vec3::Y * 0.4),
                    0.5,
                )
            }
        };
        if at.distance(ctx.feet) <= radius + 0.35 {
            state.mind.job_arrived.get_or_insert(ctx.now);
        }
        let _ = bot;
        return Some(Goal {
            kind,
            at: Some(at),
            pace: Pace::Run,
            watch,
            crouch,
            radius,
            ..default()
        });
    }
}

/// Defenders without jobs: anchors hold, roamers roam and hunt.
fn defend(ctx: &Ctx, state: &mut BotState, bot: &mut Bot) -> Goal {
    let b = &ctx.tuning.ai.behaviour;
    let Some(site) = state.mind.site.and_then(|s| ctx.tactics.site(s)).or(ctx.tactics.sites.first()) else {
        return Goal::default();
    };
    // Fresh enemy information near me.
    let contact = ctx
        .intel
        .contacts(ctx.team)
        .iter()
        .filter(|c| ctx.now - c.time < 4.0)
        .min_by(|a, b| a.pos.distance(ctx.feet).total_cmp(&b.pos.distance(ctx.feet)))
        .copied();
    if state.mind.role == Role::Roamer {
        if let Some(c) = contact.filter(|c| c.pos.distance(ctx.feet) < 25.0 && c.kind != ContactKind::Heard || c.pos.distance(ctx.feet) < 10.0) {
            return Goal {
                kind: GoalKind::Investigate,
                at: Some(c.pos),
                pace: Pace::Careful,
                watch: Some(c.pos + Vec3::Y * 1.2),
                radius: 3.5,
                ..default()
            };
        }
        let need_new = state.mind.roam.is_none_or(|r| {
            (r.distance(ctx.feet) < 1.0 && ctx.now > state.mind.roam_until) || ctx.now > state.mind.roam_until + 25.0
        });
        if need_new && !ctx.tactics.roam.is_empty() {
            // Somewhere 6..22 m from the site, not where I am.
            let options: Vec<Vec3> = ctx
                .tactics
                .roam
                .iter()
                .copied()
                .filter(|p| {
                    let d = p.distance(site.centre);
                    (6.0..22.0).contains(&d) && p.distance(ctx.feet) > 4.0
                })
                .collect();
            let pool = if options.is_empty() { ctx.tactics.roam.clone() } else { options };
            state.mind.roam = Some(pool[bot.rng.below(pool.len())]);
            state.mind.roam_until = ctx.now + 6.0 + bot.rng.f32() * 8.0;
        }
        if let Some(r) = state.mind.roam {
            let arrived = r.distance(ctx.feet) < 1.0;
            let watch = if arrived {
                // Look back towards where attackers come from.
                Some(r + (r - site.centre).with_y(0.0).normalize_or(Vec3::Z) * 6.0 + Vec3::Y * 1.4)
            } else {
                None
            };
            return Goal {
                kind: GoalKind::Move,
                at: Some(r),
                pace: Pace::Walk,
                watch,
                crouch: arrived && bot.rng.chance(0.3),
                radius: 0.6,
                ..default()
            };
        }
    }
    // Anchor: a hold spot on the site, swapped now and then.
    if site.holds.is_empty() {
        let at = site.inside_point(ctx.grid, bot.index as usize * 7);
        return Goal {
            kind: GoalKind::Hold,
            at: Some(at),
            watch: Some(site.centre + Vec3::Y * 1.3),
            ..default()
        };
    }
    let quiet = contact.is_none_or(|c| c.pos.distance(site.centre) > 15.0);
    if quiet && ctx.now - state.mind.hold_since > 30.0 + (bot.index % 4) as f32 * 5.0 {
        state.mind.hold += 1;
        state.mind.hold_since = ctx.now;
    }
    // Spread anchors over the spots: offset by my rank at this site.
    let spot = &site.holds[(state.mind.hold + bot.index as usize) % site.holds.len()];
    let mut watch = spot.watch;
    if let Some(c) = contact.filter(|c| c.pos.distance(ctx.feet) < 14.0 && c.pos.distance(spot.pos) > 1.5) {
        watch = c.pos + Vec3::Y * 1.2;
    }
    let crouch = (bot.index as f32 * 0.37 + state.mind.hold as f32 * 0.61).fract() < ctx.crouch_chance;
    let _ = b;
    Goal {
        kind: GoalKind::Hold,
        at: Some(spot.pos),
        pace: if contact.is_some() { Pace::Sneak } else { Pace::Walk },
        watch: Some(watch),
        crouch,
        radius: 0.45,
        ..default()
    }
}

/// Attackers in the action phase.
fn attack(ctx: &Ctx, state: &mut BotState, bot: &mut Bot, carrier: Option<&DefuserCarrier>, gadget: Option<&UniqueGadget>) -> Goal {
    let b = &ctx.tuning.ai.behaviour;
    let site_id = state.mind.site.or(ctx.plans.target);
    let Some(site) = site_id.and_then(|s| ctx.tactics.site(s)).or(ctx.tactics.sites.first()) else {
        return Goal::default();
    };
    let in_site = ctx.layout.and_then(|l| l.site_at(ctx.feet + Vec3::Y * 0.05)).is_some();
    let enemy_seen = !state.senses.visible.is_empty();
    let recently_hurt = state.senses.hurt.is_some_and(|(_, _, t)| ctx.now - t < 1.5);

    // Plant as soon as we're inside a site and it's quiet.
    if carrier.is_some() {
        if in_site && !enemy_seen && !recently_hurt {
            let watch = site
                .entrances
                .first()
                .map(|e| e.inside + Vec3::Y * 1.2)
                .unwrap_or(site.centre + Vec3::Y);
            let at = if state.goal.kind == GoalKind::Plant { state.goal.at.unwrap_or(ctx.feet) } else { ctx.feet };
            return Goal { kind: GoalKind::Plant, at: Some(at), watch: Some(watch), radius: 1.0, ..default() };
        }
        let target = site.inside_point(ctx.grid, site.inside.len() / 2);
        return approach(ctx, state, site.centre, target, false);
    }
    // A dropped defuser: the closest attacker fetches it.
    if let Some(at) = ctx.dropped_at
        && ctx.closest_to(at)
    {
        return Goal {
            kind: GoalKind::PickUp,
            at: Some(at),
            pace: Pace::Run,
            watch: None,
            radius: 0.4,
            ..default()
        };
    }
    // Larks: a dart at the site on the way in.
    let dist = ctx.feet.distance(site.centre);
    if let Some(g) = gadget
        && g.kind == GadgetKind::SensorDart
        && g.remaining > 0
        && dist < b.dart_range
        && ctx.now > state.mind.dart_at
        && !enemy_seen
    {
        state.mind.dart_at = ctx.now + 10.0 + bot.rng.f32() * 6.0;
        state.mind.pressed_at = None;
        let aim = site.centre + Vec3::Y * 1.0;
        return Goal {
            kind: GoalKind::Dart { aim },
            at: None,
            watch: Some(aim),
            ..default()
        };
    }
    if matches!(state.goal.kind, GoalKind::Dart { .. }) {
        // Keep the dart goal until it has been fired.
        if state.mind.pressed_at.is_none_or(|t| ctx.now - t < 0.4) && ctx.now < state.mind.dart_at - 8.5 {
            return state.goal.clone();
        }
    }
    if site.contains(ctx.feet + Vec3::Y * 0.05) || dist < 3.0 {
        // In: cover an entrance from a hold spot.
        return hold_site(ctx, state, bot, site);
    }
    let pick = (bot.index as usize * 5) % site.inside.len().max(1);
    let target = site.inside_point(ctx.grid, pick);
    approach(ctx, state, site.centre, target, state.mind.alt_route)
}

/// Move towards a site: run far out, walk carefully close in.
fn approach(ctx: &Ctx, state: &BotState, centre: Vec3, target: Vec3, alt: bool) -> Goal {
    let careful = ctx.tuning.ai.behaviour.careful_range;
    let dist = ctx.feet.distance(centre);
    let pace = if dist > careful + 6.0 { Pace::Run } else { Pace::Careful };
    let _ = state;
    Goal {
        kind: GoalKind::Move,
        at: Some(target),
        pace,
        watch: None,
        radius: 0.8,
        alt_route: alt,
        ..default()
    }
}

/// Hold an entrance of `site` from inside it.
fn hold_site(ctx: &Ctx, state: &mut BotState, bot: &mut Bot, site: &super::tactics::SiteTactics) -> Goal {
    if site.holds.is_empty() {
        let at = site.inside_point(ctx.grid, bot.index as usize * 3);
        return Goal {
            kind: GoalKind::Hold,
            at: Some(at),
            watch: site.entrances.first().map(|e| e.inside + Vec3::Y * 1.2),
            ..default()
        };
    }
    let rank = ctx.rank_to(site.centre);
    let spot = &site.holds[(rank + bot.index as usize) % site.holds.len()];
    let crouch = (bot.index as f32 * 0.53).fract() < ctx.crouch_chance;
    let _ = state;
    Goal {
        kind: GoalKind::Hold,
        at: Some(spot.pos),
        pace: Pace::Careful,
        watch: Some(spot.watch),
        crouch,
        radius: 0.45,
        ..default()
    }
}

/// Defenders once the defuser is down: the nearest disables, others cover.
fn retake(ctx: &Ctx, state: &mut BotState) -> Goal {
    let Some((device, disabler, site_id)) = ctx.device else {
        return Goal::default();
    };
    let site = ctx.tactics.site(site_id);
    let watch = site
        .and_then(|s| {
            s.entrances
                .iter()
                .min_by(|a, b| a.inside.distance(ctx.feet).total_cmp(&b.inside.distance(ctx.feet)))
        })
        .map(|e| e.inside + Vec3::Y * 1.2);
    let me_disabling = disabler == Some(ctx.me);
    let someone_else = disabler.is_some_and(|e| e != ctx.me);
    let enemy_seen = !state.senses.visible.is_empty();
    if me_disabling || (!someone_else && ctx.rank_to(device) == 0) {
        let pace = if ctx.feet.distance(device) > 8.0 { Pace::Run } else { Pace::Careful };
        return Goal {
            kind: if enemy_seen && !me_disabling { GoalKind::Move } else { GoalKind::Disable },
            at: Some(device),
            pace,
            watch: Some(device + Vec3::Y * 0.2),
            radius: 0.8,
            crouch: false,
            ..default()
        };
    }
    // Cover: somewhere 2..5 m from the device.
    let offset = Vec3::new((ctx.me.index_u32() % 3) as f32 - 1.0, 0.0, (ctx.me.index_u32() % 2) as f32 * 2.0 - 1.0)
        .normalize_or(Vec3::X)
        * 3.0;
    let at = ctx
        .grid
        .locate(device + offset + Vec3::Y * 0.3, 3)
        .map(|n| ctx.grid.pos(n))
        .unwrap_or(device);
    Goal {
        kind: GoalKind::Move,
        at: Some(at),
        pace: if ctx.feet.distance(device) > 10.0 { Pace::Run } else { Pace::Careful },
        watch,
        radius: 1.0,
        ..default()
    }
}

/// Attackers once planted: hold spots covering the defuser's site.
fn guard_plant(ctx: &Ctx, state: &mut BotState, bot: &mut Bot) -> Goal {
    let Some((device, _, site_id)) = ctx.device else {
        return Goal::default();
    };
    let Some(site) = ctx.tactics.site(site_id) else {
        return Goal { kind: GoalKind::Move, at: Some(device), radius: 2.5, ..default() };
    };
    let mut goal = hold_site(ctx, state, bot, site);
    if goal.at.is_some_and(|p| p.distance(device) > 9.0) {
        goal.at = Some(device);
        goal.radius = 3.0;
    }
    goal
}
