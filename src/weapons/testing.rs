//! Automated weapon checks and screenshot helpers.
//!
//! Flags (combine with the debug flags, e.g. `--screenshot`, `--spawn`):
//! - `--weapon-test`: scripted test run in Harrow Keep. Teleports the local
//!   player between spots, aims at training dummies (spawned automatically)
//!   and walls, fires, reloads and switches, then logs PASS/FAIL with the
//!   numbers and quits (exit code 1 if anything failed). Doesn't grab the
//!   mouse or play sounds.
//! - `--weapon primary|sidearm`: hold that weapon.
//! - `--ads`: hold aim-down-sights.
//! - `--hold-fire`: hold the trigger (full auto keeps firing).
//! - `--fire-at <tick>` / `--reload-at <tick>`: pull the trigger / press
//!   reload once at that fixed tick (64 per second).
//! - `--dummies` (in `dummies.rs`): training dummies.
//!
//! The test drives the same intent components a bot will (`PlayerInput`,
//! `WeaponInput`), so it exercises the real input-to-damage path.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::{
    BulletHit, ShotFired, Slot, WeaponAction, WeaponEvent, WeaponEventKind, WeaponInput,
    WeaponState, WeaponSystems,
    dummies::TrainingDummy,
    firing::{aim_rotation, eye_position},
};
use crate::{
    combat::{Dead, Health, SurfaceImpact, SurfaceKind},
    config::Tuning,
    player::{AutomatedRun, CharacterState, HitZone, Lean, LocalPlayer, PlayerInput, Stance},
};

pub struct WeaponTestPlugin;

impl Plugin for WeaponTestPlugin {
    fn build(&self, app: &mut App) {
        let forced = ForcedWeaponInput::from_args();
        if forced.is_active() {
            app.insert_resource(forced)
                .add_systems(FixedPreUpdate, apply_forced_weapon_input);
        }
        if let Some(path) = arg_value("--screenshot-on-hit") {
            app.insert_resource(AutomatedRun)
                .insert_resource(ShotOnHit {
                    path: path.into(),
                    frames_after: None,
                })
                .add_systems(Update, screenshot_on_hit);
        }
        if std::env::args().any(|a| a == "--weapon-test") {
            app.insert_resource(AutomatedRun)
                .insert_resource(WeaponTest::new(scenarios()))
                .add_systems(FixedPreUpdate, drive_test)
                .add_systems(FixedUpdate, record_test.after(WeaponSystems::Simulate));
        }
    }
}

// --- Screenshot helpers -------------------------------------------------------

#[derive(Resource, Default)]
struct ForcedWeaponInput {
    slot: Option<Slot>,
    ads: bool,
    hold_fire: bool,
    fire_at: Option<u32>,
    reload_at: Option<u32>,
    tick: u32,
}

impl ForcedWeaponInput {
    fn from_args() -> Self {
        let slot = arg_value("--weapon").and_then(|s| match s.as_str() {
            "primary" | "rifle" => Some(Slot::Primary),
            "sidearm" | "pistol" => Some(Slot::Sidearm),
            other => {
                warn!("--weapon expects primary|sidearm, got {other:?}");
                None
            }
        });
        let has = |flag: &str| std::env::args().any(|a| a == flag);
        Self {
            slot,
            ads: has("--ads"),
            hold_fire: has("--hold-fire"),
            fire_at: arg_value("--fire-at").and_then(|s| s.parse().ok()),
            reload_at: arg_value("--reload-at").and_then(|s| s.parse().ok()),
            tick: 0,
        }
    }

    fn is_active(&self) -> bool {
        self.slot.is_some()
            || self.ads
            || self.hold_fire
            || self.fire_at.is_some()
            || self.reload_at.is_some()
    }
}

/// Fixed tick: hold the inputs asked for on the command line (the input
/// module lets go of them while the mouse isn't captured).
fn apply_forced_weapon_input(
    mut forced: ResMut<ForcedWeaponInput>,
    mut inputs: Query<&mut WeaponInput, With<LocalPlayer>>,
) {
    forced.tick += 1;
    for mut input in &mut inputs {
        if let Some(slot) = forced.slot {
            input.slot = slot;
        }
        if forced.ads {
            input.aim = true;
        }
        if forced.hold_fire {
            if !input.trigger {
                input.trigger_presses = input.trigger_presses.wrapping_add(1);
            }
            input.trigger = true;
        }
        if forced.fire_at == Some(forced.tick) {
            input.trigger_presses = input.trigger_presses.wrapping_add(1);
            info!("--fire-at: trigger pulled at tick {}", forced.tick);
        }
        if forced.reload_at == Some(forced.tick) {
            input.reload_requests = input.reload_requests.wrapping_add(1);
            info!("--reload-at: reload pressed at tick {}", forced.tick);
        }
    }
}

/// `--screenshot-on-hit <path>`: screenshot a few frames after the local
/// player's first bullet hit (to see the hit marker and dummy feedback),
/// then quit.
#[derive(Resource)]
struct ShotOnHit {
    path: std::path::PathBuf,
    frames_after: Option<u32>,
}

fn screenshot_on_hit(
    mut shot: ResMut<ShotOnHit>,
    mut hits: MessageReader<BulletHit>,
    player: Option<Single<Entity, With<LocalPlayer>>>,
    mut commands: Commands,
) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    let Some(me) = player.map(|p| *p) else { return };
    if shot.frames_after.is_none()
        && let Some(hit) = hits.read().find(|h| h.shooter == me)
    {
        info!(
            "--screenshot-on-hit: {:?} hit for {:.1} (lethal {})",
            hit.zone, hit.damage, hit.lethal
        );
        shot.frames_after = Some(0);
    }
    let Some(frames) = shot.frames_after.map(|f| f + 1) else { return };
    shot.frames_after = Some(frames);
    if frames == 4 {
        info!("--screenshot-on-hit -> {}", shot.path.display());
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(shot.path.clone()));
    }
    // Quit a second later: quitting while the GPU driver is still compiling
    // the new effect shaders can crash on some drivers.
    if frames == 90 {
        commands.write_message(AppExit::Success);
    }
}

fn arg_value(flag: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

// --- Scripted test ------------------------------------------------------------

const TICK_HZ: f32 = 64.0;
/// Give up on any step after this many ticks.
const TIMEOUT: u32 = 64 * 8;

#[derive(Clone, Copy, Debug)]
enum AimAt {
    /// The centre of a dummy's hit zone (by dummy label).
    Dummy(&'static str, HitZone),
    /// Fixed look angles, degrees.
    Angles(f32, f32),
}

#[derive(Clone, Copy, Debug)]
enum Op {
    /// Fire this many shots (hold the trigger, or click for semi-auto).
    Fire(u32),
    /// One trigger press, e.g. on an empty magazine.
    PullTrigger,
    Reload,
    Select(Slot),
    /// Wait until the weapon is ready (reload/switch finished).
    WaitReady,
    WaitTicks(u32),
}

#[derive(Clone, Copy, Debug)]
enum Check {
    /// `count` bullet hits on `dummy`, all in `zone` with `damage` each
    /// (None = lethal head shot), after `layers` soft surfaces; the dummy
    /// ends dead or alive as `dead`.
    Hits {
        dummy: &'static str,
        zone: HitZone,
        count: usize,
        damage: Option<f32>,
        layers: u32,
        dead: bool,
    },
    /// The bullet stops at a `surface` wall: no hits, dummy untouched.
    Blocked { dummy: &'static str, surface: SurfaceKind },
    /// Log the recoil pattern and spread; optionally require identical
    /// bullet directions to an earlier scenario (determinism).
    Spray { same_as: Option<usize> },
    /// Log the spread of every shot.
    Spread,
    /// Ammo after the ops (magazine, reserve) and, if set, the measured time
    /// of the last reload (seconds) against `reload` in the tuning.
    Ammo { magazine: u32, reserve: u32, reload: Option<bool> },
    /// Seconds from asking for the other weapon to having it ready.
    Switch,
    /// The dummy was killed and is back up with full health.
    Respawned { dummy: &'static str },
}

struct Scenario {
    name: &'static str,
    slot: Slot,
    feet: Vec3,
    stance: Stance,
    lean: Lean,
    move_axis: Vec2,
    aim: AimAt,
    ads: bool,
    ops: Vec<Op>,
    check: Check,
}

impl Scenario {
    fn new(name: &'static str, slot: Slot, feet: Vec3, aim: AimAt) -> Self {
        Self {
            name,
            slot,
            feet,
            stance: Stance::Stand,
            lean: Lean::Center,
            move_axis: Vec2::ZERO,
            aim,
            ads: true,
            ops: vec![Op::Fire(1)],
            check: Check::Spread,
        }
    }
    fn hip(mut self) -> Self {
        self.ads = false;
        self
    }
    fn stance(mut self, stance: Stance) -> Self {
        self.stance = stance;
        self
    }
    fn lean(mut self, lean: Lean) -> Self {
        self.lean = lean;
        self
    }
    fn moving(mut self, axis: Vec2) -> Self {
        self.move_axis = axis;
        self
    }
    fn ops(mut self, ops: &[Op]) -> Self {
        self.ops = ops.to_vec();
        self
    }
    fn check(mut self, check: Check) -> Self {
        self.check = check;
        self
    }
}

const COURTYARD: Vec3 = Vec3::new(8.0, 0.0, 9.0);
/// East side of the courtyard, 10 m south of the stone wall at z = 0.
const RANGE_SPOT: Vec3 = Vec3::new(13.0, 0.0, 10.0);
/// Courtyard-side room, south of the storeroom's plaster wall.
const ROOM_PLASTER: Vec3 = Vec3::new(7.0, 0.0, -1.5);
/// Same room, east of the stone wall into the west hall.
const ROOM_STONE: Vec3 = Vec3::new(7.5, 0.0, -1.0);

fn scenarios() -> Vec<Scenario> {
    use HitZone::*;
    use Slot::*;
    let a = "courtyard A";
    let b = "courtyard B";
    let c = "courtyard C";
    let plaster = "behind plaster";
    let stone = "behind stone";
    let wall = AimAt::Angles(0.0, 0.0);
    let hits = |dummy, zone, count, damage, layers, dead| Check::Hits {
        dummy,
        zone,
        count,
        damage,
        layers,
        dead,
    };
    vec![
        // 0-6: damage by zone, both weapons.
        Scenario::new("rifle, ADS, head shot at 5 m kills", Primary, COURTYARD, AimAt::Dummy(a, Head))
            .check(hits(a, Head, 1, None, 0, true)),
        Scenario::new("rifle, ADS, torso at 7.6 m", Primary, COURTYARD, AimAt::Dummy(b, Torso))
            .check(hits(b, Torso, 1, Some(42.0), 0, false)),
        Scenario::new("rifle, ADS, legs at 5 m", Primary, COURTYARD, AimAt::Dummy(a, Legs))
            .check(hits(a, Legs, 1, Some(31.5), 0, false)),
        Scenario::new("rifle, ADS, 3-round burst to the torso kills", Primary, COURTYARD, AimAt::Dummy(b, Torso))
            .ops(&[Op::Fire(3)])
            .check(hits(b, Torso, 3, Some(42.0), 0, true)),
        Scenario::new("sidearm, ADS, torso at 7.6 m", Sidearm, COURTYARD, AimAt::Dummy(b, Torso))
            .check(hits(b, Torso, 1, Some(48.0), 0, false)),
        Scenario::new("sidearm, hip, head at 7.5 m kills", Sidearm, COURTYARD, AimAt::Dummy(c, Head))
            .hip()
            .check(hits(c, Head, 1, None, 0, true)),
        Scenario::new("sidearm, 2 clicks to the legs", Sidearm, COURTYARD, AimAt::Dummy(a, Legs))
            .ops(&[Op::Fire(2)])
            .check(hits(a, Legs, 2, Some(36.0), 0, false)),
        // 7-9: penetration.
        Scenario::new("rifle through the plaster wall, torso", Primary, ROOM_PLASTER, AimAt::Dummy(plaster, Torso))
            .check(hits(plaster, Torso, 1, Some(42.0 * 0.6), 1, false)),
        Scenario::new("rifle through the plaster wall, head kills", Primary, ROOM_PLASTER, AimAt::Dummy(plaster, Head))
            .check(hits(plaster, Head, 1, None, 1, true)),
        Scenario::new("rifle into the stone wall: stopped", Primary, ROOM_STONE, AimAt::Dummy(stone, Torso))
            .ops(&[Op::Fire(3)])
            .check(Check::Blocked { dummy: stone, surface: SurfaceKind::Hard }),
        // 10-12: recoil and spread over a full magazine; 12 repeats 10.
        Scenario::new("rifle spray, ADS, 30 rounds", Primary, RANGE_SPOT, wall)
            .ops(&[Op::Fire(30)])
            .check(Check::Spray { same_as: None }),
        Scenario::new("rifle spray, hip, 30 rounds", Primary, RANGE_SPOT, wall)
            .hip()
            .ops(&[Op::Fire(30)])
            .check(Check::Spray { same_as: None }),
        Scenario::new("rifle spray, ADS, repeated (determinism)", Primary, RANGE_SPOT, wall)
            .ops(&[Op::Fire(30)])
            .check(Check::Spray { same_as: Some(10) }),
        // 13-15: spread standing, walking, aimed (5-round bursts);
        // crouched and prone are last (23, 24).
        Scenario::new("spread: hip, standing", Primary, RANGE_SPOT, wall)
            .hip()
            .ops(&[Op::Fire(5)]),
        Scenario::new("spread: hip, walking", Primary, RANGE_SPOT, wall)
            .hip()
            .moving(Vec2::new(-1.0, 0.0))
            .ops(&[Op::WaitTicks(32), Op::Fire(5)]),
        Scenario::new("spread: ADS, standing", Primary, RANGE_SPOT, wall).ops(&[Op::Fire(5)]),
        // 16-19: magazines and reloads.
        Scenario::new("rifle tactical reload: 31 - 5, reload -> 31 (30 + chambered)", Primary, RANGE_SPOT, wall)
            .ops(&[Op::Fire(5), Op::Reload, Op::WaitReady])
            .check(Check::Ammo { magazine: 31, reserve: 145, reload: Some(false) }),
        Scenario::new("rifle empty: 31 shots, dry click auto-reloads -> 30", Primary, RANGE_SPOT, wall)
            .ops(&[Op::Fire(31), Op::WaitTicks(16), Op::PullTrigger, Op::WaitReady])
            .check(Check::Ammo { magazine: 30, reserve: 120, reload: Some(true) }),
        Scenario::new("sidearm tactical reload: 16 - 3, reload -> 16", Sidearm, RANGE_SPOT, wall)
            .ops(&[Op::Fire(3), Op::Reload, Op::WaitReady])
            .check(Check::Ammo { magazine: 16, reserve: 57, reload: Some(false) }),
        Scenario::new("reload with a full magazine does nothing", Primary, RANGE_SPOT, wall)
            .ops(&[Op::Reload, Op::WaitTicks(32), Op::WaitReady])
            .check(Check::Ammo { magazine: 31, reserve: 150, reload: None }),
        // 20: switching.
        Scenario::new("switch rifle -> sidearm", Primary, RANGE_SPOT, wall)
            .ops(&[Op::Select(Sidearm), Op::WaitTicks(2), Op::WaitReady])
            .check(Check::Switch),
        // 21: dummies stand back up. 22: the shot leaves from the leaned
        // eye (aimed from there; from the upright eye it would miss).
        Scenario::new("dummy respawns after a kill", Primary, COURTYARD, AimAt::Dummy(a, Head))
            .ops(&[Op::Fire(1), Op::WaitTicks(16)])
            .check(Check::Respawned { dummy: a }),
        Scenario::new("leaning right, head shot from the true eye", Primary, COURTYARD, AimAt::Dummy(b, Head))
            .lean(Lean::Right)
            .check(hits(b, Head, 1, None, 0, true)),
        // 23-24: last, because standing back up from crouch/prone is refused
        // at this spot (the movement code's stand-up probe reports the
        // desert-floor slab under the courtyard; reported, not a weapons
        // issue).
        Scenario::new("spread: hip, crouched", Primary, RANGE_SPOT, wall)
            .hip()
            .stance(Stance::Crouch)
            .ops(&[Op::Fire(5)]),
        Scenario::new("spread: hip, prone", Primary, RANGE_SPOT, wall)
            .hip()
            .stance(Stance::Prone)
            .ops(&[Op::Fire(5)]),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Setup,
    Settle,
    Run,
    Observe,
}

/// Everything recorded during one scenario.
#[derive(Default)]
struct Record {
    /// (tick, shot, recoil after the shot, magazine after)
    shots: Vec<(u32, ShotFired, Vec2)>,
    hits: Vec<BulletHit>,
    impacts: Vec<(SurfaceImpact, SurfaceKind)>,
    events: Vec<(u32, WeaponEventKind)>,
    /// Look angles the shots were aimed with (radians).
    base_yaw: f32,
    base_pitch: f32,
    health_before: Vec<(&'static str, f32)>,
    /// Tick when a switch was requested.
    switch_asked: Option<u32>,
    ready_at: Option<u32>,
}

#[derive(Resource)]
struct WeaponTest {
    scenarios: Vec<Scenario>,
    index: usize,
    phase: Phase,
    /// Ticks in the current phase / op.
    ticks: u32,
    tick: u32,
    op: usize,
    shots_at_op: usize,
    presses_sent: usize,
    record: Record,
    /// Bullet directions of each finished scenario (for determinism checks).
    directions: Vec<Vec<Vec3>>,
    results: Vec<(String, bool, String)>,
    done: bool,
}

impl WeaponTest {
    fn new(scenarios: Vec<Scenario>) -> Self {
        Self {
            scenarios,
            index: 0,
            phase: Phase::Setup,
            ticks: 0,
            tick: 0,
            op: 0,
            shots_at_op: 0,
            presses_sent: 0,
            record: Record::default(),
            directions: Vec::new(),
            results: Vec::new(),
            done: false,
        }
    }

    fn next_phase(&mut self, phase: Phase) {
        self.phase = phase;
        self.ticks = 0;
    }
}

type PlayerParts<'a> = (
    Entity,
    &'a mut Transform,
    &'a mut LinearVelocity,
    &'a mut PlayerInput,
    &'a mut WeaponInput,
    &'a mut WeaponState,
    &'a CharacterState,
);

#[allow(clippy::too_many_arguments)]
fn drive_test(
    mut test: ResMut<WeaponTest>,
    player: Single<PlayerParts, With<LocalPlayer>>,
    mut dummies: Query<
        (&mut TrainingDummy, &mut Health, &Transform, Has<Dead>, &Children, Entity),
        Without<LocalPlayer>,
    >,
    zones: Query<(), With<HitZone>>,
    tuning: Res<Tuning>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    if test.done {
        return;
    }
    test.tick += 1;
    test.ticks += 1;
    let (_, mut transform, mut velocity, mut look, mut input, mut state, body) = player.into_inner();

    if test.index >= test.scenarios.len() {
        finish(&mut test, &mut exit);
        return;
    }
    let scenario = &test.scenarios[test.index];
    let (slot, feet, stance, lean, move_axis, aim_at, ads) = (
        scenario.slot,
        scenario.feet,
        scenario.stance,
        scenario.lean,
        scenario.move_axis,
        scenario.aim,
        scenario.ads,
    );

    // Keep aiming at the target (also corrects for the eye height while
    // the stance settles), except while firing so recoil shows.
    let aim_target = |dummies: &Query<
        (&mut TrainingDummy, &mut Health, &Transform, Has<Dead>, &Children, Entity),
        Without<LocalPlayer>,
    >| match aim_at {
        AimAt::Angles(yaw, pitch) => Some((yaw.to_radians(), pitch.to_radians())),
        AimAt::Dummy(label, zone) => {
            let (_, _, dummy, ..) = dummies.iter().find(|(d, ..)| d.label == label)?;
            let height = match zone {
                HitZone::Head => tuning.body.stand_height - 0.14,
                HitZone::Torso => tuning.body.stand_height - 0.63,
                HitZone::Legs => 0.4,
            };
            let point = dummy.translation + Vec3::Y * height;
            let eye = eye_position(transform.translation, look.yaw, body.height, body.lean, &tuning);
            let d = point - eye;
            Some((
                (-d.x).atan2(-d.z),
                d.y.atan2(Vec2::new(d.x, d.z).length()),
            ))
        }
    };

    // The input module lets go of aim and trigger every frame while the
    // mouse isn't captured (as in automated runs), so hold the aim each tick.
    input.aim = ads;

    match test.phase {
        Phase::Setup => {
            info!("--- scenario {}: {}", test.index, scenario.name);
            // Drop in from just above the floor: exactly on it, the
            // movement code's stand-up check counts the floor as in the way.
            transform.translation = feet + Vec3::Y * 0.05;
            velocity.0 = Vec3::ZERO;
            look.stance = stance;
            look.lean = lean;
            look.sprint = false;
            look.move_axis = Vec2::ZERO;
            input.slot = slot;
            input.aim = ads;
            input.trigger = false;
            // Fresh weapons: full ammo, no recoil, shot counter from zero
            // (so spread is reproducible), already holding `slot`.
            let mut fresh = WeaponState::new(&tuning);
            fresh.active = slot;
            fresh.seen_trigger = input.trigger_presses;
            fresh.seen_reload = input.reload_requests;
            *state = fresh;
            // Fresh dummies.
            for (mut dummy, mut health, _, dead, children, entity) in &mut dummies {
                dummy.down_for = 0.0;
                health.current = health.max;
                if dead {
                    commands.entity(entity).remove::<Dead>();
                    for child in children.iter().filter(|c| zones.contains(*c)) {
                        commands.entity(child).remove::<ColliderDisabled>();
                    }
                }
            }
            test.record = Record::default();
            test.next_phase(Phase::Settle);
        }
        Phase::Settle => {
            if let Some((yaw, pitch)) = aim_target(&dummies) {
                look.yaw = yaw;
                look.pitch = pitch;
            }
            let aimed = !ads || state.aim >= 1.0;
            let target_height = tuning.body.height_for(stance);
            let target_lean = match lean {
                Lean::Left => -1.0,
                Lean::Center => 0.0,
                Lean::Right => 1.0,
            };
            let settled = (body.height - target_height).abs() < 0.01
                && (body.lean - target_lean).abs() < 0.01;
            if test.ticks >= 24 && aimed && settled && state.action == WeaponAction::Ready {
                look.move_axis = move_axis;
                test.record.base_yaw = look.yaw;
                test.record.base_pitch = look.pitch;
                test.record.health_before = dummies
                    .iter()
                    .map(|(d, h, ..)| (d.label, h.current))
                    .collect();
                test.op = 0;
                test.shots_at_op = 0;
                test.presses_sent = 0;
                test.next_phase(Phase::Run);
            } else if test.ticks > TIMEOUT {
                let why = format!(
                    "timed out getting ready (height {:.2}/{target_height:.2}, aim {:.2}, {:?})",
                    body.height, state.aim, state.action
                );
                fail(&mut test, &why);
            }
        }
        Phase::Run => {
            input.trigger = false;
            // Re-applied every tick: the player input module zeroes movement
            // while the mouse is free.
            look.move_axis = move_axis;
            let ops = test.scenarios[test.index].ops.clone();
            let Some(&op) = ops.get(test.op) else {
                look.move_axis = Vec2::ZERO;
                test.next_phase(Phase::Observe);
                return;
            };
            let shots_in_op = test.record.shots.len() - test.shots_at_op;
            let def = slot.def(&tuning);
            let op_done = match op {
                Op::Fire(n) => {
                    let n = n as usize;
                    if shots_in_op >= n {
                        true
                    } else {
                        if def.full_auto && state.active.def(&tuning).full_auto {
                            input.trigger = true;
                            if test.presses_sent == 0 {
                                input.trigger_presses = input.trigger_presses.wrapping_add(1);
                                test.presses_sent = 1;
                            }
                        } else if test.presses_sent == shots_in_op {
                            input.trigger = true;
                            input.trigger_presses = input.trigger_presses.wrapping_add(1);
                            test.presses_sent += 1;
                        }
                        false
                    }
                }
                Op::PullTrigger => {
                    input.trigger = true;
                    input.trigger_presses = input.trigger_presses.wrapping_add(1);
                    true
                }
                Op::Reload => {
                    input.reload_requests = input.reload_requests.wrapping_add(1);
                    true
                }
                Op::Select(slot) => {
                    input.slot = slot;
                    test.record.switch_asked = Some(test.tick);
                    true
                }
                Op::WaitReady => {
                    if state.action == WeaponAction::Ready {
                        let tick = test.tick;
                        test.record.ready_at.get_or_insert(tick);
                        true
                    } else {
                        false
                    }
                }
                Op::WaitTicks(n) => test.ticks >= n,
            };
            if op_done {
                test.op += 1;
                test.ticks = 0;
                test.shots_at_op = test.record.shots.len();
                test.presses_sent = 0;
            } else if test.ticks > TIMEOUT {
                fail(&mut test, &format!("timed out on {op:?}"));
            }
        }
        Phase::Observe => {
            // Damage lands in FixedPostUpdate of the shot's tick; respawn
            // checks wait for the dummy to come back.
            let wait = match test.scenarios[test.index].check {
                Check::Respawned { .. } => {
                    (tuning.weapons.dummies.respawn_time * TICK_HZ) as u32 + 16
                }
                _ => 4,
            };
            if test.ticks < wait {
                return;
            }
            let after: Vec<(&'static str, f32, bool)> = dummies
                .iter()
                .map(|(d, h, _, dead, ..)| (d.label, h.current, dead))
                .collect();
            let (ok, detail) = evaluate(&test, &after, &state, &tuning);
            let name = test.scenarios[test.index].name.to_string();
            info!("{} scenario {}: {name}: {detail}", if ok { "PASS" } else { "FAIL" }, test.index);
            test.results.push((name, ok, detail));
            let directions = test.record.shots.iter().map(|(_, s, _)| s.direction).collect();
            test.directions.push(directions);
            test.index += 1;
            test.next_phase(Phase::Setup);
        }
    }
}

fn fail(test: &mut WeaponTest, why: &str) {
    let name = test.scenarios[test.index].name.to_string();
    info!("FAIL scenario {}: {name}: {why}", test.index);
    test.results.push((name, false, why.to_string()));
    test.directions.push(Vec::new());
    test.index += 1;
    test.next_phase(Phase::Setup);
}

fn finish(test: &mut WeaponTest, exit: &mut MessageWriter<AppExit>) {
    test.done = true;
    let failed = test.results.iter().filter(|(_, ok, _)| !ok).count();
    info!("=== weapon test: {} passed, {} failed ===", test.results.len() - failed, failed);
    for (name, ok, detail) in &test.results {
        info!("  [{}] {name}: {detail}", if *ok { "PASS" } else { "FAIL" });
    }
    exit.write(if failed == 0 {
        AppExit::Success
    } else {
        AppExit::from_code(1)
    });
}

/// After the simulation each tick: collect what happened to the local player's shots.
#[allow(clippy::too_many_arguments)]
fn record_test(
    mut test: ResMut<WeaponTest>,
    player: Single<(Entity, &WeaponState), With<LocalPlayer>>,
    mut shots: MessageReader<ShotFired>,
    mut hits: MessageReader<BulletHit>,
    mut impacts: MessageReader<SurfaceImpact>,
    mut events: MessageReader<WeaponEvent>,
    colliders: Query<&ColliderOf>,
    surfaces: Query<&SurfaceKind>,
) {
    let (me, state) = player.into_inner();
    let tick = test.tick;
    let recording = matches!(test.phase, Phase::Run | Phase::Observe);
    for shot in shots.read() {
        if shot.shooter == me && recording {
            test.record.shots.push((tick, shot.clone(), state.recoil));
        }
    }
    for hit in hits.read() {
        if hit.shooter == me && recording {
            test.record.hits.push(hit.clone());
        }
    }
    for impact in impacts.read() {
        if impact.source == Some(me) && recording {
            let body = colliders.get(impact.collider).map(|c| c.body).unwrap_or(impact.collider);
            let kind = surfaces.get(body).copied().unwrap_or(SurfaceKind::Hard);
            test.record.impacts.push((impact.clone(), kind));
        }
    }
    for event in events.read() {
        if event.shooter == me && recording {
            test.record.events.push((tick, event.kind));
        }
    }
}

fn evaluate(
    test: &WeaponTest,
    after: &[(&'static str, f32, bool)],
    state: &WeaponState,
    tuning: &Tuning,
) -> (bool, String) {
    let scenario = &test.scenarios[test.index];
    let r = &test.record;
    let before = |label: &str| {
        r.health_before
            .iter()
            .find(|(l, _)| *l == label)
            .map_or(0.0, |(_, h)| *h)
    };
    let now = |label: &str| after.iter().find(|(l, ..)| *l == label).copied();
    let impacts_text = || {
        r.impacts
            .iter()
            .map(|(i, k)| format!("{k:?}@{:.2}", i.point))
            .collect::<Vec<_>>()
            .join(", ")
    };

    match scenario.check {
        Check::Hits {
            dummy,
            zone,
            count,
            damage,
            layers,
            dead,
        } => {
            let Some((_, health, is_dead)) = now(dummy) else {
                return (false, format!("no dummy '{dummy}'"));
            };
            let on_target: Vec<&BulletHit> = r.hits.iter().collect();
            let zones_ok = on_target.iter().all(|h| h.zone == zone);
            let layers_ok = on_target.iter().all(|h| h.layers == layers);
            let damage_ok = match damage {
                Some(expected) => on_target.iter().all(|h| (h.damage - expected).abs() < 0.01),
                None => on_target.iter().all(|h| h.lethal),
            };
            let ok = on_target.len() == count && zones_ok && layers_ok && damage_ok && is_dead == dead;
            let detail = format!(
                "{} hit(s) [{}], health {:.1} -> {:.1}, dead {is_dead}; surfaces entered: [{}]",
                on_target.len(),
                on_target
                    .iter()
                    .map(|h| format!(
                        "{:?} {:.1} dmg at {:.1} m through {} soft",
                        h.zone, h.damage, h.distance, h.layers
                    ))
                    .collect::<Vec<_>>()
                    .join("; "),
                before(dummy),
                health,
                impacts_text(),
            );
            (ok, detail)
        }
        Check::Blocked { dummy, surface } => {
            let Some((_, health, _)) = now(dummy) else {
                return (false, format!("no dummy '{dummy}'"));
            };
            let first_ok = r.impacts.first().is_some_and(|(_, k)| *k == surface);
            let untouched = (health - before(dummy)).abs() < 1e-3;
            let ok = r.hits.is_empty() && untouched && first_ok && r.shots.len() == 3;
            (
                ok,
                format!(
                    "{} shots, {} hits, health {:.1} -> {health:.1}; surfaces entered: [{}]",
                    r.shots.len(),
                    r.hits.len(),
                    before(dummy),
                    impacts_text()
                ),
            )
        }
        Check::Spray { same_as } => spray_report(r, same_as.map(|i| &test.directions[i][..]), tuning),
        Check::Spread => {
            let spreads: Vec<String> = r.shots.iter().map(|(_, s, _)| format!("{:.2}", s.spread)).collect();
            let devs = deviations(r);
            (
                !r.shots.is_empty() && devs.iter().all(|(d, cone)| *d <= cone + 1e-3),
                format!(
                    "eye height {:.2} m; spread per shot (deg): [{}]; measured deviation from aim+recoil (deg): [{}]",
                    r.shots.first().map_or(0.0, |(_, s, _)| s.origin.y),
                    spreads.join(", "),
                    devs.iter().map(|(d, _)| format!("{d:.2}")).collect::<Vec<_>>().join(", ")
                ),
            )
        }
        Check::Ammo {
            magazine,
            reserve,
            reload,
        } => {
            let ammo = state.ammo();
            let mut ok = ammo.magazine == magazine && ammo.reserve == reserve;
            let mut detail = format!(
                "{} shots; ammo now {}/{} (expected {magazine}/{reserve})",
                r.shots.len(),
                ammo.magazine,
                ammo.reserve
            );
            let start = r.events.iter().find_map(|(t, e)| match e {
                WeaponEventKind::ReloadStart { empty } => Some((*t, *empty)),
                _ => None,
            });
            let done = r.events.iter().find_map(|(t, e)| (*e == WeaponEventKind::ReloadDone).then_some(*t));
            let dry = r.events.iter().any(|(_, e)| *e == WeaponEventKind::DryFire);
            match (reload, start, done) {
                (Some(empty), Some((t0, was_empty)), Some(t1)) => {
                    let def = state.active.def(tuning);
                    let expected = if empty { def.reload_empty } else { def.reload_tactical };
                    let measured = (t1 - t0) as f32 / TICK_HZ;
                    ok &= was_empty == empty && (measured - expected).abs() <= 1.5 / TICK_HZ;
                    detail += &format!(
                        "; {} reload took {measured:.3} s (tuning {expected:.2} s){}",
                        if was_empty { "empty" } else { "tactical" },
                        if dry { "; dry-fire click first" } else { "" }
                    );
                }
                (None, None, None) => detail += "; no reload started (correct)",
                _ => {
                    ok = false;
                    detail += &format!("; reload events wrong: {:?}", r.events);
                }
            }
            (ok, detail)
        }
        Check::Switch => {
            let (Some(t0), Some(t1)) = (r.switch_asked, r.ready_at) else {
                return (false, "switch never finished".into());
            };
            let measured = (t1 - t0) as f32 / TICK_HZ;
            let expected = tuning.weapons.holster_time + tuning.weapons.sidearm.draw_time;
            let ok = state.active == Slot::Sidearm && (measured - expected).abs() <= 2.5 / TICK_HZ;
            (
                ok,
                format!(
                    "now holding {:?} after {measured:.3} s (holster {:.2} + draw {:.2} = {expected:.2} s)",
                    state.active, tuning.weapons.holster_time, tuning.weapons.sidearm.draw_time
                ),
            )
        }
        Check::Respawned { dummy } => {
            let Some((_, health, dead)) = now(dummy) else {
                return (false, format!("no dummy '{dummy}'"));
            };
            let killed = r.hits.iter().any(|h| h.lethal);
            (
                killed && !dead && (health - tuning.weapons.dummies.health).abs() < 1e-3,
                format!(
                    "killed {killed}; {:.1} s later: dead {dead}, health {health:.1}",
                    tuning.weapons.dummies.respawn_time
                ),
            )
        }
    }
}

/// For each shot: angle between the bullet and the aim direction including
/// that shot's recoil (= the random spread actually applied), and the cone.
fn deviations(r: &Record) -> Vec<(f32, f32)> {
    let mut recoil_before = Vec2::ZERO;
    let mut out = Vec::new();
    for (_, shot, recoil_after) in &r.shots {
        let aim = aim_rotation(r.base_yaw, r.base_pitch, recoil_before) * Vec3::NEG_Z;
        let angle = aim.angle_between(shot.direction).to_degrees();
        out.push((angle, shot.spread));
        recoil_before = *recoil_after;
    }
    out
}

fn spray_report(r: &Record, reference: Option<&[Vec3]>, tuning: &Tuning) -> (bool, String) {
    let def = &tuning.weapons.primary;
    let devs = deviations(r);
    let within_cone = devs.iter().all(|(d, cone)| *d <= cone + 1e-3);
    let mean_dev = devs.iter().map(|(d, _)| d).sum::<f32>() / devs.len().max(1) as f32;
    let final_recoil = r.shots.last().map_or(Vec2::ZERO, |(_, _, rc)| *rc);
    // Where the bullets landed on the wall, relative to the first one (cm).
    let first = r.shots.first().map_or(Vec3::ZERO, |(_, s, _)| s.end);
    let path: Vec<String> = r
        .shots
        .iter()
        .step_by(5)
        .map(|(_, s, _)| {
            let d = (s.end - first) * 100.0;
            format!("({:+.0},{:+.0})", d.x, d.y)
        })
        .collect();
    let kicks: Vec<String> = r
        .shots
        .iter()
        .take(8)
        .map(|(_, _, rc)| format!("({:+.2},{:.2})", rc.x, rc.y))
        .collect();
    let gaps: Vec<u32> = r.shots.windows(2).map(|w| w[1].0 - w[0].0).collect();
    let duration = r.shots.last().map_or(0, |s| s.0) - r.shots.first().map_or(0, |s| s.0);
    let rpm = if duration > 0 {
        (r.shots.len() - 1) as f32 * 60.0 * TICK_HZ / duration as f32
    } else {
        0.0
    };
    let mut ok = r.shots.len() == 30 && within_cone && (rpm - def.rpm).abs() < def.rpm * 0.03;
    let mut detail = format!(
        "{} shots at {rpm:.0} rpm (ticks between shots {:?}..{:?}); recoil after shots 1-8 (right,up deg): {}; \
         after 30: ({:+.2},{:.2}); spread cone first {:.3} deg, last {:.2} deg; mean deviation {mean_dev:.3} deg, \
         all inside cone: {within_cone}; impact path every 5th shot (cm right,up from the first): {}",
        r.shots.len(),
        gaps.iter().min().copied().unwrap_or(0),
        gaps.iter().max().copied().unwrap_or(0),
        kicks.join(" "),
        final_recoil.x,
        final_recoil.y,
        r.shots.first().map_or(0.0, |s| s.1.spread),
        r.shots.last().map_or(0.0, |s| s.1.spread),
        path.join(" "),
    );
    if let Some(reference) = reference {
        let same = reference.len() == r.shots.len()
            && reference
                .iter()
                .zip(&r.shots)
                .all(|(a, (_, s, _))| a.angle_between(s.direction) < 1e-5);
        ok &= same;
        detail += &format!("; identical to the earlier run: {same}");
    }
    (ok, detail)
}
