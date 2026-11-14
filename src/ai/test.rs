//! Bot statistics, cost measurement, debug drawing and `--ai-test`.
//!
//! `--ai-test`: an automated match of bots only (the local player is a bot
//! too), with the `ai.test_timers` round timers and `ai.test_rounds_to_win`.
//! No mouse grab, no sounds. Logs every kill, plant, defuse, reinforcement,
//! stuck bot and round result, then a summary per round and per match, and
//! quits after the matches. Options:
//! - `--ai-matches N`: matches to play (default 1).
//! - `--ai-speed X`: run the game clock X times faster (default 1).
//! - `--ai-shots DIR`: screenshots every `--ai-shot-every S` game seconds
//!   (default 7) from a chase camera on an interesting bot (up to 16).
//! - `--ai-spectate`: the chase camera without screenshots.
//! - `--ai-max-seconds T`: give up after T real seconds (default 600).
//!
//! The cost of the bot systems (fixed-tick senses, decisions and actions)
//! is measured every frame and logged every 10 seconds with the frame rate
//! (`ai: cost ...`), in every run with bots, not just tests.

use std::{path::PathBuf, time::Instant};

use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};

use super::{AiSettings, AiSystems, Bot, BotState, nav::NavGrid};
use crate::{
    combat::{Dead, Team},
    config::Tuning,
    destruction::{Reinforced, SoftSurface, SurfaceRole},
    gadgets::GadgetEvent,
    player::{AutomatedRun, Player, PlayerCamera},
    round::{Round, RoundEvent, RoundPhase, RoundSettings, team_label},
};

pub struct AiTestPlugin;

impl Plugin for AiTestPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AiStats>()
            .init_resource::<AiPerf>()
            .add_systems(FixedPreUpdate, start_timer.before(AiSystems::Senses))
            .add_systems(FixedPreUpdate, stop_timer.after(AiSystems::Act))
            .add_systems(Update, (count_events, report_cost));
        let settings = app.world().resource::<AiSettings>().clone();
        if settings.debug {
            app.add_systems(Startup, configure_debug_gizmos)
                .add_systems(Update, (super::nav::draw_nav, draw_bots));
        }
        let args: Vec<String> = std::env::args().collect();
        if args.iter().any(|a| a == "--ai-navtest") {
            app.add_systems(Update, nav_test);
        }
        let value = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
                .cloned()
        };
        let shots = value("--ai-shots").map(PathBuf::from);
        let spectate = shots.is_some() || args.iter().any(|a| a == "--ai-spectate");
        if spectate {
            let front = value("--ai-cam").as_deref() == Some("front");
            app.insert_resource(Spectator { front, ..default() }).add_systems(
                PostUpdate,
                chase_camera.before(TransformSystems::Propagate),
            );
        }
        if !settings.test {
            return;
        }
        if let Some(mut round) = app.world_mut().get_resource_mut::<RoundSettings>() {
            round.test = true;
        }
        let test = AiTest {
            matches: value("--ai-matches").and_then(|n| n.parse().ok()).unwrap_or(1),
            speed: value("--ai-speed").and_then(|n| n.parse().ok()).unwrap_or(1.0),
            shots,
            shot_every: value("--ai-shot-every").and_then(|n| n.parse().ok()).unwrap_or(7.0),
            max_seconds: value("--ai-max-seconds").and_then(|n| n.parse().ok()).unwrap_or(600.0),
            ..default()
        };
        info!(
            "ai test: {} match(es) at {}x speed{}",
            test.matches,
            test.speed,
            test.shots.as_ref().map(|d| format!(", screenshots to {}", d.display())).unwrap_or_default()
        );
        app.insert_resource(AutomatedRun)
            .insert_resource(test)
            .add_systems(Startup, set_speed)
            .add_systems(PreUpdate, test_timers)
            .add_systems(Update, (log_test, log_bots, take_screenshots, quit_test).chain());
    }
}

/// Counters for logs and tests.
#[derive(Resource, Default, Debug, Clone)]
pub struct AiStats {
    pub stuck: u32,
    pub no_path: u32,
    pub breaches: u32,
    pub darts: u32,
    pub smokes: u32,
    pub stakes: u32,
    pub wallbangs: u32,
    /// Spot checks of shots at a target: stone in the way (should stay 0).
    pub stone_shots: u32,
    pub checked_shots: u32,
    pub reinforced: u32,
    pub barricades: u32,
    pub kills: [u32; 2],
    pub plants: u32,
    pub disables: u32,
}

#[derive(Resource, Default)]
struct AiPerf {
    started: Option<Instant>,
    /// Bot time this frame and since the last report.
    total_secs: f64,
    frames: u32,
    worst_frame: f64,
    this_frame: f64,
    last_report: f32,
}

fn start_timer(mut perf: ResMut<AiPerf>) {
    perf.started = Some(Instant::now());
}

fn stop_timer(mut perf: ResMut<AiPerf>) {
    if let Some(start) = perf.started.take() {
        perf.this_frame += start.elapsed().as_secs_f64();
    }
}

fn report_cost(
    mut perf: ResMut<AiPerf>,
    bots: Query<(), With<Bot>>,
    diagnostics: Res<DiagnosticsStore>,
    time: Res<Time<Real>>,
) {
    let frame = std::mem::take(&mut perf.this_frame);
    perf.total_secs += frame;
    perf.frames += 1;
    perf.worst_frame = perf.worst_frame.max(frame);
    let now = time.elapsed_secs();
    if now - perf.last_report < 10.0 || bots.is_empty() {
        return;
    }
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.average())
        .unwrap_or(0.0);
    info!(
        "ai: cost {:.3} ms/frame on average (worst {:.2} ms) for {} bots; {:.0} fps",
        perf.total_secs * 1000.0 / perf.frames.max(1) as f64,
        perf.worst_frame * 1000.0,
        bots.iter().count(),
        fps
    );
    perf.total_secs = 0.0;
    perf.frames = 0;
    perf.worst_frame = 0.0;
    perf.last_report = now;
}

/// Count what happens (all runs; logged by tests).
fn count_events(
    mut stats: ResMut<AiStats>,
    mut round_events: MessageReader<RoundEvent>,
    mut reinforced: MessageReader<Reinforced>,
    mut gadgets: MessageReader<GadgetEvent>,
    mut died: MessageReader<crate::combat::Died>,
    teams: Query<&Team, With<Player>>,
    new_surfaces: Query<&SoftSurface, Added<SoftSurface>>,
    round: Res<Round>,
) {
    for event in round_events.read() {
        match event {
            RoundEvent::Planted { .. } => stats.plants += 1,
            RoundEvent::Disabled { .. } => stats.disables += 1,
            _ => {}
        }
    }
    for r in reinforced.read() {
        stats.reinforced += 1;
        info!("ai: surface {} reinforced by {:?}", r.surface, r.by);
    }
    for g in gadgets.read() {
        if let GadgetEvent::StakePlaced { .. } = g {
            stats.stakes += 1;
        }
    }
    for d in died.read() {
        if let Ok(team) = teams.get(d.entity) {
            // Kills of each side's characters: [attackers lost, defenders lost].
            stats.kills[usize::from(*team == Team::Defenders)] += 1;
        }
    }
    if round.phase.is_live() {
        stats.barricades += new_surfaces.iter().filter(|s| s.role() == SurfaceRole::Barricade).count() as u32;
    }
}

#[derive(Resource, Default)]
struct AiTest {
    matches: u32,
    speed: f32,
    shots: Option<PathBuf>,
    shot_every: f32,
    max_seconds: f32,
    played: u32,
    round_start: AiStats,
    results: Vec<String>,
    match_results: Vec<String>,
    next_shot: f32,
    shots_taken: u32,
    quit_in: Option<u32>,
    fps: Vec<f64>,
}

fn set_speed(test: Res<AiTest>, mut time: ResMut<Time<Virtual>>) {
    if test.speed != 1.0 {
        time.set_relative_speed(test.speed.clamp(0.1, 8.0));
        // Let several fixed ticks run per frame when sped up.
        time.set_max_delta(std::time::Duration::from_millis(250));
    }
}

/// Use the bot test timers (re-applied if the tuning file reloads).
fn test_timers(mut tuning: ResMut<Tuning>) {
    let wanted = tuning.ai.test_timers.clone();
    let rounds = tuning.ai.test_rounds_to_win;
    let t = &tuning.round.test_timers;
    if t.action != wanted.action || t.prep != wanted.prep || t.select != wanted.select || tuning.round.test_rounds_to_win != rounds {
        tuning.round.test_timers = wanted;
        tuning.round.test_rounds_to_win = rounds;
        info!("ai test: round timers from ai.test_timers, {rounds} round wins per match");
    }
}

fn log_test(
    mut test: ResMut<AiTest>,
    stats: Res<AiStats>,
    mut events: MessageReader<RoundEvent>,
    grid: Res<NavGrid>,
    diagnostics: Res<DiagnosticsStore>,
) {
    if let Some(fps) = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.smoothed()) {
        test.fps.push(fps);
    }
    for event in events.read() {
        match event {
            RoundEvent::PhaseStarted { phase: RoundPhase::Prep, round } => {
                test.round_start = stats.clone();
                info!("ai test: round {round} prep (nav {} nodes)", grid.node_count());
            }
            RoundEvent::RoundWon { round, result, score_blue, score_red } => {
                let s = &*stats;
                let r = &test.round_start;
                let line = format!(
                    "round {round}: {} win by {} | dead: {} attackers, {} defenders | reinforced {}, barricades {}, stakes {}, darts {}, smokes {}, breaches {}, plants {}, disables {} | stuck {}, no-path {}, wallbangs {}, stone-shots {}/{} checked | blue {score_blue} - red {score_red}",
                    team_label(result.winner),
                    result.reason.label(),
                    s.kills[0] - r.kills[0],
                    s.kills[1] - r.kills[1],
                    s.reinforced - r.reinforced,
                    s.barricades - r.barricades,
                    s.stakes - r.stakes,
                    s.darts - r.darts,
                    s.smokes - r.smokes,
                    s.breaches - r.breaches,
                    s.plants - r.plants,
                    s.disables - r.disables,
                    s.stuck - r.stuck,
                    s.no_path - r.no_path,
                    s.wallbangs - r.wallbangs,
                    s.stone_shots - r.stone_shots,
                    s.checked_shots - r.checked_shots,
                );
                info!("AI TEST {line}");
                test.results.push(line);
            }
            RoundEvent::MatchWon { squad, score_blue, score_red } => {
                test.played += 1;
                let line = format!("match {}: squad {squad:?} wins {score_blue}-{score_red}", test.played);
                info!("AI TEST {line}");
                test.match_results.push(line);
                if test.played >= test.matches {
                    test.quit_in = Some(20);
                }
            }
            _ => {}
        }
    }
}

fn quit_test(mut test: ResMut<AiTest>, stats: Res<AiStats>, time: Res<Time<Real>>, mut exit: MessageWriter<AppExit>) {
    if test.quit_in.is_none() && time.elapsed_secs() > test.max_seconds {
        warn!("ai test: out of time after {:.0} s", time.elapsed_secs());
        test.quit_in = Some(20);
    }
    let Some(n) = test.quit_in else { return };
    if n > 0 {
        test.quit_in = Some(n - 1);
        return;
    }
    let fps = &test.fps;
    let avg = if fps.is_empty() { 0.0 } else { fps.iter().sum::<f64>() / fps.len() as f64 };
    let min = fps.iter().copied().fold(f64::MAX, f64::min);
    info!("AI TEST SUMMARY ({} matches, {:.0} s real time)", test.played, time.elapsed_secs());
    for line in test.results.iter().chain(&test.match_results) {
        info!("AI TEST   {line}");
    }
    info!(
        "AI TEST   totals: stuck {}, no-path {}, wallbangs {}, stone-shots {} of {} checked, breaches {}, darts {}; fps avg {:.0} min {:.0}",
        stats.stuck, stats.no_path, stats.wallbangs, stats.stone_shots, stats.checked_shots, stats.breaches, stats.darts, avg, if min == f64::MAX { 0.0 } else { min }
    );
    exit.write(AppExit::Success);
}

/// Chase camera state.
#[derive(Resource, Default)]
struct Spectator {
    subject: Option<Entity>,
    chosen_at: f32,
    position: Option<Vec3>,
    weapon_camera_off: bool,
    /// `--ai-cam front`.
    front: bool,
}

/// Follow an interesting bot from behind its shoulder (replaces the
/// first-person view; the weapon camera is switched off).
#[allow(clippy::type_complexity)]
fn chase_camera(
    mut spectator: ResMut<Spectator>,
    bots: Query<(Entity, &GlobalTransform, &BotState, &crate::player::PlayerInput, Has<Dead>, Has<crate::round::defuser::DefuserCarrier>), With<Bot>>,
    mut camera: Query<(Entity, &mut Transform), With<PlayerCamera>>,
    mut cameras: Query<(&mut Camera, &ChildOf)>,
    spatial: avian3d::prelude::SpatialQuery,
    time: Res<Time<Real>>,
    round: Res<Round>,
) {
    let now = time.elapsed_secs();
    let Ok((camera_entity, mut transform)) = camera.single_mut() else { return };
    if !spectator.weapon_camera_off {
        for (mut cam, parent) in &mut cameras {
            if parent.parent() == camera_entity {
                cam.is_active = false;
                spectator.weapon_camera_off = true;
            }
        }
    }
    let alive = |e: Entity| bots.get(e).is_ok_and(|b| !b.4);
    let keep = spectator.subject.is_some_and(|s| alive(s)) && now - spectator.chosen_at < 5.0;
    if !keep {
        // Someone fighting, else the defuser carrier, else anyone moving.
        let fighting = bots.iter().filter(|b| !b.4 && b.2.combat.target.is_some()).map(|b| b.0).next();
        let carrier = bots.iter().filter(|b| !b.4 && b.5).map(|b| b.0).next();
        let moving = bots
            .iter()
            .filter(|b| !b.4 && b.2.nav.move_dir != Vec3::ZERO)
            .map(|b| b.0)
            .nth((now as usize / 5) % 3);
        let any = bots.iter().filter(|b| !b.4).map(|b| b.0).nth(round.number as usize % 4);
        let pick = fighting.or(if round.phase == RoundPhase::Action { carrier } else { None }).or(moving).or(any);
        if pick != spectator.subject {
            spectator.position = None;
        }
        spectator.subject = pick;
        spectator.chosen_at = now;
    }
    let Some((_, gt, _, input, _, _)) = spectator.subject.and_then(|s| bots.get(s).ok()) else { return };
    let feet = gt.translation();
    let yaw = Quat::from_rotation_y(input.yaw);
    let forward = yaw * Vec3::NEG_Z;
    let right = yaw * Vec3::X;
    let head = feet + Vec3::Y * 1.55;
    // `--ai-cam front`: a close look at the body and weapon from the front
    // right; otherwise over the shoulder.
    let wanted = if spectator.front {
        head + forward * 1.9 + right * 0.9 - Vec3::Y * 0.25
    } else {
        head - forward * 2.4 + right * 0.75 + Vec3::Y * 0.35
    };
    // Don't put the camera inside a wall.
    let filter = avian3d::prelude::SpatialQueryFilter::from_mask(crate::physics::GameLayer::World);
    let delta = wanted - head;
    let target = Dir3::new(delta)
        .ok()
        .and_then(|dir| spatial.cast_ray(head, dir, delta.length(), true, &filter))
        .map(|hit| head + delta.normalize() * (hit.distance - 0.25).max(0.3))
        .unwrap_or(wanted);
    let position = match spectator.position {
        Some(p) => p.lerp(target, (time.delta_secs() * 8.0).min(1.0)),
        None => target,
    };
    spectator.position = Some(position);
    let look_at = if spectator.front {
        head - Vec3::Y * 0.4
    } else {
        head + forward * 6.0 + Vec3::Y * (input.pitch.sin() * 6.0)
    };
    *transform = Transform::from_translation(position).looking_at(look_at, Vec3::Y);
}

fn take_screenshots(mut test: ResMut<AiTest>, round: Res<Round>, bots: Query<&BotState, With<Bot>>, time: Res<Time>, mut commands: Commands) {
    let Some(dir) = test.shots.clone() else { return };
    if test.shots_taken >= 16 || !round.phase.is_live() {
        return;
    }
    let now = time.elapsed_secs();
    let fighting = bots.iter().any(|b| b.combat.target.is_some());
    if now < test.next_shot && !(fighting && now > test.next_shot - test.shot_every * 0.6) {
        return;
    }
    test.next_shot = now + test.shot_every;
    test.shots_taken += 1;
    std::fs::create_dir_all(&dir).ok();
    let path = dir.join(format!(
        "ai-r{}-{:?}-{:02}{}.png",
        round.number,
        round.phase,
        test.shots_taken,
        if fighting { "-fight" } else { "" }
    ));
    info!("ai test: screenshot -> {}", path.display());
    commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
}

fn configure_debug_gizmos(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<super::nav::NavGizmos>();
    config.line.width = 1.5;
}

/// `--ai-debug`: each bot's path (white), goal (cyan) and look point.
fn draw_bots(bots: Query<(&GlobalTransform, &BotState), (With<Bot>, Without<Dead>)>, mut gizmos: Gizmos) {
    for (gt, state) in &bots {
        let feet = gt.translation();
        if let Some(path) = &state.nav.path {
            let mut prev = feet + Vec3::Y * 0.1;
            for w in path.points.iter().skip(state.nav.index) {
                let p = w.pos + Vec3::Y * 0.1;
                gizmos.line(prev, p, Color::WHITE);
                prev = p;
            }
        }
        if let Some(at) = state.goal.at {
            gizmos.sphere(Isometry3d::from_translation(at + Vec3::Y * 0.1), 0.25, Color::srgb(0.2, 0.9, 1.0));
        }
        if state.combat.target.is_some() {
            gizmos.line(feet + Vec3::Y * 1.6, state.combat.aim_point, Color::srgb(1.0, 0.2, 0.2));
        }
    }
}

/// `--ai-navtest`: once the grid is ready, plan paths between every spawn
/// and every site and log them (length, waypoints, special links), then
/// quit. Also lists what the map analysis found.
fn nav_test(
    grid: Res<NavGrid>,
    tactics: Res<super::tactics::Tactics>,
    layout: Option<Res<crate::round::sites::KeepLayout>>,
    mut scratch: ResMut<super::path::PathScratch>,
    spatial: avian3d::prelude::SpatialQuery,
    colliders: Query<&avian3d::prelude::ColliderOf>,
    names: Query<&Name>,
    mut done: Local<bool>,
    mut exit: MessageWriter<AppExit>,
) {
    if *done || !grid.ready || !tactics.ready {
        return;
    }
    *done = true;
    let Some(layout) = layout else { return };
    let mut points: Vec<(String, Vec3)> = Vec::new();
    for (i, s) in layout.attacker_spawns.iter().enumerate() {
        points.push((format!("attacker spawn {i}"), s.position));
    }
    for (i, s) in layout.defender_spawns.iter().enumerate() {
        points.push((format!("defender spawn {i}"), s.position));
    }
    let mut targets: Vec<(String, Vec3)> = Vec::new();
    for site in &tactics.sites {
        targets.push((format!("site {} inside", site.id.letter()), site.inside_point(&grid, site.inside.len() / 2)));
        for (i, e) in site.entrances.iter().enumerate() {
            info!("navtest: site {} entrance {i}: {:?} inside {:.1} outside {:.1}", site.id.letter(), e.kind, e.inside, e.outside);
        }
        for w in &site.walls {
            info!("navtest: site {} wall job: stand {:.1} aim {:.1} crouch {}", site.id.letter(), w.stand, w.aim, w.crouch);
            targets.push((format!("site {} wall job", site.id.letter()), w.stand));
        }
        for d in &site.doors {
            info!("navtest: site {} door job: stand {:.1} aim {:.1}", site.id.letter(), d.stand, d.aim);
        }
        for h in &site.holds {
            info!("navtest: site {} hold {:.1} watching {:.1}", site.id.letter(), h.pos, h.watch);
        }
    }
    if let Some(raw) = std::env::args().skip_while(|a| a != "--ai-navdump").nth(1) {
        // x0,z0,x1,z1: dump columns along the line.
        let n: Vec<f32> = raw.split(',').filter_map(|v| v.parse().ok()).collect();
        if let [x0, z0, x1, z1] = n[..] {
            let (a, b) = (Vec3::new(x0, 0.0, z0), Vec3::new(x1, 0.0, z1));
            let steps = (a.distance(b) / grid.cell).ceil() as i32;
            let filter = avian3d::prelude::SpatialQueryFilter::from_mask(crate::physics::GameLayer::World);
            let capsule = avian3d::prelude::Collider::capsule(0.22, 1.24);
            for i in 0..=steps {
                let p = a.lerp(b, i as f32 / steps.max(1) as f32);
                info!("navdump: {}", grid.describe(p));
                // What blocks a standing capsule on the ground floor here?
                let hits = spatial.shape_intersections(&capsule, p + Vec3::Y * 0.96, Quat::IDENTITY, &filter);
                let what: Vec<String> = hits
                    .iter()
                    .map(|e| {
                        let body = colliders.get(*e).map(|c| c.body).unwrap_or(*e);
                        names.get(body).map(|n| n.as_str().to_string()).unwrap_or_else(|_| format!("{body}"))
                    })
                    .collect();
                info!("navdump:   ground capsule hits {what:?}");
            }
        }
    }
    let mut failures = 0;
    for (from_name, from) in &points {
        for (to_name, to) in &targets {
            let a = grid.locate(*from + Vec3::Y * 0.3, 3);
            let b = grid.locate(*to + Vec3::Y * 0.3, 3);
            for (attacker, breacher) in [(true, false), (false, false), (true, true)] {
                let options = super::path::PathOptions {
                    barricades: Some(if attacker { 6.0 } else { 40.0 }),
                    breach: breacher.then_some(super::path::COST_BREACH_PREFERRED),
                    max_expansions: 200_000,
                    greed: 1.0,
                };
                let before = scratch.expansions;
                let path = match (a, b) {
                    (Some(a), Some(b)) => super::path::find_path(&grid, &mut scratch, a, b, options, &|_| 0.0),
                    _ => None,
                };
                let expanded = scratch.expansions - before;
                match path {
                    Some(p) => {
                        let specials: Vec<String> = p
                            .points
                            .iter()
                            .filter(|w| w.kind != super::nav::LinkKind::Walk || w.blocker.is_some())
                            .map(|w| format!("{:?}{}@{:.1}", w.kind, if w.blocker.is_some() { "+barricade" } else { "" }, w.pos))
                            .collect();
                        info!(
                            "navtest: {from_name} -> {to_name} ({}): cost {:.1}, {} waypoints, {expanded} nodes searched {}",
                            if breacher { "breacher" } else if attacker { "attacker" } else { "defender" },
                            p.cost,
                            p.points.len(),
                            specials.join(" ")
                        );
                    }
                    None => {
                        failures += 1;
                        warn!(
                            "navtest: {from_name} {from:.1} -> {to_name} {to:.1} ({}): NO PATH (nodes {a:?} -> {b:?})",
                            if attacker { "attacker" } else { "defender" }
                        );
                    }
                }
            }
        }
    }
    info!("navtest: {failures} failures");
    exit.write(AppExit::Success);
}

/// Every 5 s of game time: one line per bot (goal, where, path, fight).
fn log_bots(
    bots: Query<(&Name, &Transform, &crate::combat::Team, &crate::combat::Health, &BotState, Has<Dead>), With<Bot>>,
    round: Res<Round>,
    time: Res<Time>,
    mut next: Local<f32>,
) {
    let now = time.elapsed_secs();
    if now < *next || !round.phase.is_live() {
        return;
    }
    *next = now + 5.0;
    for (name, transform, team, health, state, dead) in &bots {
        if dead {
            continue;
        }
        let path = state.nav.path.as_ref().map_or(0, |p| p.points.len().saturating_sub(state.nav.index));
        info!(
            "bot {:<8} {:?} hp {:>3.0} at {:.1} goal {:?} -> {:?} path {path} arrived {} target {}",
            name.as_str(),
            team,
            health.current,
            transform.translation,
            state.goal.kind,
            state.goal.at.map(|p| (p * 10.0).round() / 10.0),
            state.nav.arrived,
            state.combat.target.is_some()
        );
    }
}
