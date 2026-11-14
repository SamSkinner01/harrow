//! Bots (M7): characters on both teams driven by the computer.
//!
//! **Bots are ordinary characters.** A bot is a [`Player`] entity like the
//! local player (health, team, squad, operator, loadout, weapons, hit
//! zones), plus a [`Bot`] and a [`BotState`]. Bots only ever write the same
//! intent components a human's input writes ([`PlayerInput`],
//! `WeaponInput`, `DestructionIntent`, `RoundIntent`, `GadgetIntent`); the
//! simulation decides what actually happens. That keeps them honest (no
//! wall hacks, same recoil and spread) and lets a server run them later.
//! They read simulation state and spatial queries to decide.
//!
//! **Pieces** (all on the fixed tick, in `FixedPreUpdate`, before the
//! simulation reads the intents):
//! - `nav.rs` / `path.rs`: navigation grid generated from the level's
//!   colliders, A* and smoothing (see `nav.rs` for how it is built and
//!   kept up to date as walls and barricades break).
//! - `senses.rs`: sight (view cone, range, line of sight to hit zones),
//!   hearing (footsteps, gunfire and other `Noise`), team knowledge
//!   ([`senses::TeamIntel`]) and callouts.
//! - `tactics.rs`: what the map means: site entrances, hold spots, walls to
//!   reinforce, doorways to barricade, stake spots, attack routes.
//! - `brain.rs`: what each bot wants to do (defend, roam, attack, plant,
//!   disable, retreat), a few times a second, staggered.
//! - `motor.rs`: following paths, avoiding team mates, getting unstuck,
//!   breaking barricades and breaching walls, and turning the view.
//! - `combat.rs`: picking a target, human-like aim (reaction time, error
//!   that settles, imperfect recoil control), bursts, crouching and leaning.
//! - `hud.rs`: kill feed and team callouts. `test.rs`: `--ai-test`.
//!
//! **Flags:** `--bots N` (total bots; default 9 = 5v5 with you),
//! `--no-bots`, `--difficulty easy|normal|hard`, `--ai-debug` (draw the
//! nav grid and bot paths), `--ai-test` (automated bot matches, see
//! `test.rs`). Bots are off by default in the other automated test modes
//! (`--round-script`, `--weapon-test`, `--m4`, `--no-round`, ...) unless
//! `--bots` is given. Numbers live in the `ai` section of the tuning file.

mod brain;
mod combat;
mod hud;
mod motor;
mod nav;
mod path;
mod senses;
mod tactics;
mod test;
mod tuning;

pub use tuning::AiTuning;

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::{
    combat::{Health, Team},
    config::Tuning,
    physics::GameLayer,
    player::{CharacterState, HitZone, LocalPlayer, Player, PlayerInput, Stance},
    round::{
        Round, RoundIntent, RoundPhase, RoundSettings, Squad, other_team,
        operators::{OperatorChoice, OperatorId},
        sites::KeepLayout,
    },
};
use tuning::DifficultyLevel;

pub struct AiPlugin;

impl Plugin for AiPlugin {
    fn build(&self, app: &mut App) {
        let round_enabled = app
            .world()
            .get_resource::<RoundSettings>()
            .is_some_and(|s| s.enabled);
        let settings = AiSettings::from_args(round_enabled);
        info!(
            "ai: {} bots, difficulty {:?}{}",
            settings.bots,
            settings.difficulty,
            if settings.local_bot { ", the local player is a bot too" } else { "" }
        );
        app.insert_resource(settings)
            .configure_sets(
                FixedPreUpdate,
                (AiSystems::Senses, AiSystems::Think, AiSystems::Act).chain(),
            )
            .add_systems(Startup, spawn_bots)
            .add_systems(
                FixedPreUpdate,
                pick_operators.in_set(AiSystems::Think),
            )
            .add_plugins((
                nav::NavPlugin,
                senses::SensesPlugin,
                tactics::TacticsPlugin,
                brain::BrainPlugin,
                motor::MotorPlugin,
                combat::CombatPlugin,
                hud::AiHudPlugin,
                test::AiTestPlugin,
            ));
    }
}

/// Order of the bot systems on the fixed tick (all in `FixedPreUpdate`).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum AiSystems {
    /// Perception and team knowledge.
    Senses,
    /// Decisions.
    Think,
    /// Movement, aim and shooting -> intents.
    Act,
}

/// Command-line options for bots.
#[derive(Resource, Debug, Clone)]
pub struct AiSettings {
    /// Bots to spawn in total (split between the teams so they're even).
    pub bots: u32,
    /// `--difficulty`; `None` = the tuning file's.
    pub difficulty: Option<DifficultyLevel>,
    /// `--ai-debug`: draw the nav grid, paths and what bots look at.
    pub debug: bool,
    /// `--ai-test`: automated bot matches.
    pub test: bool,
    /// The local player is driven by a bot brain too (`--ai-test`,
    /// `--ai-player`).
    pub local_bot: bool,
    /// `--ai-navtest`: plan test paths once the grid is built, then quit.
    pub navtest: bool,
}

impl AiSettings {
    fn from_args(round_enabled: bool) -> Self {
        let args: Vec<String> = std::env::args().collect();
        let has = |flag: &str| args.iter().any(|a| a == flag);
        let value = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
                .cloned()
        };
        let test = has("--ai-test");
        // Other automated checks expect a solo world.
        let solo_tests = [
            "--round-script",
            "--weapon-test",
            "--m4",
            "--no-round",
            "--lineup",
            "--lineup-crowd",
            "--lineup-state",
            "--screenshot-on-hit",
            "--dummies",
        ];
        let default_bots = if round_enabled && !solo_tests.iter().any(|f| has(f)) { 9 } else { 0 };
        let bots = if has("--no-bots") {
            0
        } else {
            match value("--bots").map(|n| n.parse::<u32>()) {
                Some(Ok(n)) => n.min(19),
                Some(Err(_)) => {
                    warn!("--bots expects a number");
                    default_bots
                }
                None => default_bots,
            }
        };
        let difficulty = value("--difficulty").and_then(|d| {
            let level = DifficultyLevel::from_name(&d);
            if level.is_none() {
                warn!("--difficulty expects easy|normal|hard, got {d:?}");
            }
            level
        });
        Self {
            bots,
            difficulty,
            debug: has("--ai-debug"),
            test,
            local_bot: test || has("--ai-player"),
            navtest: has("--ai-navtest"),
        }
    }

    /// Something needs the nav grid.
    pub fn nav_wanted(&self) -> bool {
        self.bots > 0 || self.local_bot || self.debug || self.navtest
    }

    pub fn level(&self, tuning: &Tuning) -> DifficultyLevel {
        self.difficulty.unwrap_or(tuning.ai.difficulty)
    }
}

/// A computer-controlled character.
#[derive(Component, Debug)]
pub struct Bot {
    /// Stable number (spawn order); staggers work between bots.
    pub index: u32,
    pub rng: Rng,
    /// Next perception and decision times (fixed-clock seconds).
    pub next_sense: f32,
    pub next_think: f32,
}

/// Small deterministic random numbers (xorshift).
#[derive(Debug, Clone, Copy)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9).max(1))
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// 0..1
    pub fn f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// -1..1
    pub fn signed(&mut self) -> f32 {
        self.f32() * 2.0 - 1.0
    }

    pub fn chance(&mut self, p: f32) -> bool {
        self.f32() < p
    }

    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { self.next_u32() as usize % n }
    }
}

pub use brain::BotState;

/// Names for bots (original callsigns), per squad.
const BLUE_NAMES: [&str; 10] = [
    "Kestrel", "Juniper", "Basalt", "Sable", "Wren", "Cobalt", "Heron", "Quill", "Rowan", "Slate",
];
const RED_NAMES: [&str; 10] = [
    "Corvid", "Ember", "Flint", "Gorse", "Marten", "Pike", "Thorn", "Vesper", "Yarrow", "Zinc",
];

fn spawn_bots(
    mut commands: Commands,
    settings: Res<AiSettings>,
    round: Option<Res<RoundSettings>>,
    layout: Option<Res<KeepLayout>>,
    tuning: Res<Tuning>,
    local: Query<Entity, With<LocalPlayer>>,
) {
    if settings.local_bot {
        for entity in &local {
            commands.entity(entity).insert((
                Bot {
                    index: 0,
                    rng: Rng::new(7),
                    next_sense: 0.0,
                    next_think: 0.0,
                },
                BotState::default(),
            ));
        }
    }
    if settings.bots == 0 {
        return;
    }
    // Even teams: the other squad gets the extra bot.
    let red = settings.bots.div_ceil(2).max(1).min(settings.bots + 1);
    let red = if settings.bots == 0 { 0 } else { red.min(settings.bots) };
    let blue = settings.bots - red;
    let blue_side = round.map_or(Team::Attackers, |r| r.start_side);
    let mut index = 1;
    for (squad, count, names) in [(Squad::Blue, blue, &BLUE_NAMES), (Squad::Red, red, &RED_NAMES)] {
        let team = if squad == Squad::Blue { blue_side } else { other_team(blue_side) };
        for i in 0..count {
            let spawn = layout
                .as_ref()
                .map(|l| l.spawns(team))
                .and_then(|s| s.get((i as usize + 1) % s.len().max(1)).copied());
            let (position, yaw) = spawn
                .map(|s| (s.position, s.yaw))
                .unwrap_or((Vec3::new(i as f32 * 1.5, 0.0, 14.0), 0.0));
            let name = names[i as usize % names.len()];
            spawn_bot(&mut commands, &tuning, name, squad, team, position, yaw, index);
            index += 1;
        }
    }
    info!("ai: spawned {blue} team mates and {red} opponents");
}

#[allow(clippy::too_many_arguments)]
fn spawn_bot(
    commands: &mut Commands,
    tuning: &Tuning,
    name: &str,
    squad: Squad,
    team: Team,
    position: Vec3,
    yaw: f32,
    index: u32,
) {
    let height = tuning.body.stand_height;
    commands
        .spawn((
            Name::new(name.to_string()),
            Player,
            Bot {
                index,
                rng: Rng::new(index * 7919 + 13),
                next_sense: 0.0,
                next_think: 0.0,
            },
            BotState::default(),
            squad,
            team,
            PlayerInput {
                yaw,
                ..default()
            },
            CharacterState {
                stance: Stance::Stand,
                height,
                lean: 0.0,
                prev_height: height,
                prev_lean: 0.0,
                grounded: false,
                sprinting: false,
                stride: 0.0,
            },
            Health::new(100.0),
            Transform::from_translation(position).with_rotation(Quat::from_rotation_y(yaw)),
            Visibility::default(),
            (
                RigidBody::Kinematic,
                CustomPositionIntegration,
                TransformInterpolation,
                LinearVelocity::ZERO,
            ),
        ))
        .with_children(|parent| {
            // Hit zones, like the local player's (placed every tick by the
            // player module from `CharacterState`).
            for zone in [HitZone::Head, HitZone::Torso, HitZone::Legs] {
                let (radius, length) = zone.shape();
                parent.spawn((
                    Name::new(format!("{zone:?} hit zone")),
                    zone,
                    Collider::capsule(radius, length),
                    Sensor,
                    CollisionLayers::new(GameLayer::Hitbox, LayerMask::NONE),
                    Transform::default(),
                ));
            }
        });
}

/// On the selection screen every bot picks an operator, spreading each
/// team over the roster (fewest of each first, counting human picks).
fn pick_operators(
    round: Res<Round>,
    mut bots: Query<(Entity, &Team, &OperatorChoice, &mut RoundIntent), With<Bot>>,
    humans: Query<(&Team, &OperatorChoice), (With<Player>, Without<Bot>)>,
    mut done: Local<u32>,
) {
    if round.phase != RoundPhase::OperatorSelect || *done == round.number {
        return;
    }
    *done = round.number;
    for team in [Team::Attackers, Team::Defenders] {
        let roster: Vec<OperatorId> = OperatorId::for_side(team).collect();
        let mut counts = vec![0u32; roster.len()];
        for (t, choice) in &humans {
            if *t == team
                && let Some(op) = choice.locked.or(choice.preferred(team))
                && let Some(i) = roster.iter().position(|&r| r == op)
            {
                counts[i] += 1;
            }
        }
        let mut members: Vec<_> = bots.iter_mut().filter(|(_, t, _, _)| **t == team).collect();
        members.sort_by_key(|(e, ..)| *e);
        for (_, _, _, intent) in &mut members {
            let (i, _) = counts
                .iter()
                .enumerate()
                .min_by_key(|&(i, c)| (*c, i))
                .expect("roster is not empty");
            counts[i] += 1;
            intent.pick_operator = Some(i as u8);
        }
    }
}

/// Eye position of a character (same as the weapons' and the camera's).
pub fn eye_position(feet: Vec3, yaw: f32, height: f32, lean: f32, tuning: &Tuning) -> Vec3 {
    let right = Quat::from_rotation_y(yaw) * Vec3::X;
    feet + Vec3::Y * (height - tuning.body.eye_from_top) + right * (lean * tuning.lean.offset)
}

/// Look angles (yaw, pitch) that face along `dir`. Yaw 0 faces -Z.
pub fn angles_to(dir: Vec3) -> (f32, f32) {
    let d = dir.normalize_or(Vec3::NEG_Z);
    let yaw = (-d.x).atan2(-d.z).rem_euclid(std::f32::consts::TAU);
    let pitch = d.y.clamp(-1.0, 1.0).asin();
    (yaw, pitch)
}

/// Look direction for angles.
pub fn look_dir(yaw: f32, pitch: f32) -> Vec3 {
    Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0) * Vec3::NEG_Z
}

/// Shortest signed difference between two angles.
pub fn angle_diff(to: f32, from: f32) -> f32 {
    let d = (to - from).rem_euclid(std::f32::consts::TAU);
    if d > std::f32::consts::PI { d - std::f32::consts::TAU } else { d }
}
