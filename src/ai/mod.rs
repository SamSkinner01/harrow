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
    combat::{Dead, Health, Team},
    config::Tuning,
    net::NetPlayer,
    player::{CharacterState, LocalPlayer, Player, PlayerInput, spawn_hitboxes},
    round::{
        Round, RoundIntent, RoundPhase, RoundSettings, Squad,
        defuser::DefuserCarrier,
        other_team,
        operators::{OperatorChoice, OperatorId},
        sites::KeepLayout,
    },
    session::{AppState, NetMode, is_authority, simulating},
};
use tuning::DifficultyLevel;

pub struct AiPlugin;

impl Plugin for AiPlugin {
    fn build(&self, app: &mut App) {
        let round_enabled = app
            .world()
            .get_resource::<RoundSettings>()
            .is_some_and(|s| s.enabled);
        // Bots fill teams by default for solo play and hosting; a dedicated
        // server only gets them with `--bots`, a client never runs them.
        let default_fill = app
            .world()
            .get_resource::<NetMode>()
            .is_none_or(|m| matches!(m, NetMode::Offline | NetMode::Host { .. }));
        let settings = AiSettings::from_args(round_enabled && default_fill);
        info!(
            "ai: {} bots, difficulty {:?}{}",
            settings.bots,
            settings.difficulty,
            if settings.local_bot { ", the local player is a bot too" } else { "" }
        );
        // Rounds teleport everyone to the spawns as soon as a session
        // starts; when the menu is skipped (command-line flags) the level's
        // colliders may still be loading, and characters put on an upper
        // floor would fall into it. Hold the round loop until the nav grid
        // (built once the map has settled) is ready.
        let hold_rounds = settings.nav_wanted()
            && settings.bots + u32::from(settings.local_bot) > 0
            && app
                .world()
                .get_resource::<NetMode>()
                .is_some_and(|m| matches!(m, NetMode::Offline | NetMode::Host { .. }));
        if hold_rounds {
            app.add_systems(PostStartup, hold_rounds_for_map)
                .add_systems(Update, start_rounds_when_ready);
        }
        app.insert_resource(settings)
            .init_resource::<RoundsHeld>()
            .configure_sets(
                FixedPreUpdate,
                (AiSystems::Senses, AiSystems::Think, AiSystems::Act)
                    .chain()
                    // Bots only run where the simulation runs (never on a
                    // client).
                    .run_if(simulating),
            )
            .init_resource::<BotRoster>()
            .add_observer(take_over_local_player)
            .add_systems(
                OnEnter(AppState::InGame),
                spawn_bots.run_if(is_authority),
            )
            .add_systems(
                FixedPreUpdate,
                (pick_operators, keep_squads_filled).in_set(AiSystems::Think),
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
            "--net-shoot-test",
            "--gadget-test",
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

/// `NetPlayer` ids for bots: far from netcode's client ids and the host's 0.
const BOT_ID_BASE: u64 = 0xB07_0000_0000;

/// Bots taking part this session, and the squad size they keep up.
#[derive(Resource, Debug, Default)]
pub struct BotRoster {
    /// Players per squad the bots fill up to (humans count).
    pub squad_size: u32,
    /// Bots spawned so far (numbers names and ids).
    pub spawned: u32,
}

/// A new session: fill both squads with bots (authority only).
#[allow(clippy::too_many_arguments)]
fn spawn_bots(
    mut commands: Commands,
    settings: Res<AiSettings>,
    round: Option<Res<RoundSettings>>,
    layout: Option<Res<KeepLayout>>,
    tuning: Res<Tuning>,
    mode: Res<NetMode>,
    mut roster: ResMut<BotRoster>,
) {
    *roster = BotRoster::default();
    if settings.bots == 0 {
        return;
    }
    // Even squads, with the local player (if any) on Blue: 9 bots = 5v5.
    let humans = u32::from(mode.has_local_player());
    roster.squad_size = (settings.bots + humans).div_ceil(2);
    let red = roster.squad_size.min(settings.bots);
    let blue = settings.bots - red;
    let blue_side = round.map_or(Team::Attackers, |r| r.start_side);
    for (squad, count) in [(Squad::Blue, blue), (Squad::Red, red)] {
        let team = if squad == Squad::Blue { blue_side } else { other_team(blue_side) };
        for i in 0..count {
            let spawn = layout
                .as_ref()
                .map(|l| l.spawns(team))
                .and_then(|s| s.get((i as usize + 1) % s.len().max(1)).copied());
            let (position, yaw) = spawn
                .map(|s| (s.position, s.yaw))
                .unwrap_or((Vec3::new(i as f32 * 1.5, 0.0, 14.0), 0.0));
            spawn_bot(&mut commands, &tuning, &mut roster, squad, team, position, yaw);
        }
    }
    info!("ai: spawned {blue} team mates and {red} opponents ({} a side)", roster.squad_size);
}

/// `--ai-test` / `--ai-player`: the local player is driven by a bot brain.
fn take_over_local_player(
    add: On<Add<LocalPlayer>>,
    settings: Res<AiSettings>,
    mode: Res<NetMode>,
    mut commands: Commands,
) {
    if settings.local_bot && mode.is_authority() {
        commands.entity(add.entity).insert((
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

/// Hosting: keep the squads at their size as people join and leave. A
/// human joining takes a bot's place (a dead one first, never the defuser
/// carrier); a squad left short gets a bot back between rounds.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn keep_squads_filled(
    mode: Res<NetMode>,
    round: Res<Round>,
    mut roster: ResMut<BotRoster>,
    members: Query<(Entity, &Squad, Has<Bot>, Has<LocalPlayer>, Has<Dead>, Has<DefuserCarrier>), With<Player>>,
    layout: Option<Res<KeepLayout>>,
    tuning: Res<Tuning>,
    mut commands: Commands,
) {
    if !mode.is_server() || roster.squad_size == 0 {
        return;
    }
    for squad in [Squad::Blue, Squad::Red] {
        let mut in_squad: Vec<_> = members.iter().filter(|m| *m.1 == squad).collect();
        let count = in_squad.len() as u32;
        if count > roster.squad_size {
            in_squad.sort_by_key(|m| (!m.4, m.5, std::cmp::Reverse(m.0)));
            if let Some(bot) = in_squad.iter().find(|m| m.2 && !m.3 && !m.5) {
                info!("ai: a player joined squad {squad:?}; bot {} leaves", bot.0);
                commands.entity(bot.0).try_despawn();
            }
        } else if count < roster.squad_size
            && matches!(round.phase, RoundPhase::OperatorSelect | RoundPhase::Inactive)
        {
            let team = round.side_of(squad);
            let spawn = layout.as_ref().and_then(|l| l.spawns(team).get(count as usize % 5).copied());
            let (position, yaw) = spawn.map(|s| (s.position, s.yaw)).unwrap_or((Vec3::new(0.0, 0.0, 14.0), 0.0));
            info!("ai: squad {squad:?} is short; a bot joins");
            spawn_bot(&mut commands, &tuning, &mut roster, squad, team, position, yaw);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_bot(
    commands: &mut Commands,
    tuning: &Tuning,
    roster: &mut BotRoster,
    squad: Squad,
    team: Team,
    position: Vec3,
    yaw: f32,
) {
    roster.spawned += 1;
    let index = roster.spawned;
    let names = if squad == Squad::Blue { &BLUE_NAMES } else { &RED_NAMES };
    let name = names[(index as usize - 1) % names.len()];
    commands
        .spawn((
            Name::new(name.to_string()),
            Bot {
                index,
                rng: Rng::new(index * 7919 + 13),
                next_sense: 0.0,
                next_think: 0.0,
            },
            BotState::default(),
            // Networked like a player, so clients see and place it.
            NetPlayer {
                id: BOT_ID_BASE + index as u64,
                name: name.to_string(),
            },
            squad,
            team,
            PlayerInput {
                yaw,
                ..default()
            },
            CharacterState::new(tuning.body.stand_height),
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
        // `Player` last, so its observers (weapons, round, gadgets,
        // networking) see everything above.
        .insert(Player)
        .with_children(spawn_hitboxes);
}

/// After startup (so start-up work that checks the flag still ran): hold
/// the round loop until the map is ready.
fn hold_rounds_for_map(mut round: ResMut<RoundSettings>, mut held: ResMut<RoundsHeld>) {
    if round.enabled {
        round.enabled = false;
        held.0 = true;
    }
}

/// Whether `hold_rounds_for_map` paused the round loop.
#[derive(Resource, Default)]
struct RoundsHeld(bool);

/// The map is loaded (the nav grid is built): let the round loop start.
fn start_rounds_when_ready(grid: Res<nav::NavGrid>, mut round: ResMut<RoundSettings>, mut held: ResMut<RoundsHeld>) {
    if held.0 && grid.ready {
        held.0 = false;
        round.enabled = true;
        info!("ai: map ready, starting the round loop");
    }
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
