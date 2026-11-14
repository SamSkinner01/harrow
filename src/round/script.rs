//! Test scripts for automated checks of the round loop and gadgets.
//!
//! `--round-script "<step>; <step>; ..."` acts as the local player (and as
//! missing opponents) on a timeline. Each step is
//! `[rN.]<phase>@<seconds>: <command> [args]` and runs once, in order, as
//! soon as the round is in that phase (and round N, if given) for at least
//! that many seconds. Phases: select, prep, action, planted, end, matchend.
//!
//! Commands:
//! - `hold` / `release`: hold or let go of F (interact).
//! - `equip`: press 5 (defuser). `gadget`: press 3. `pick N`: press N on the
//!   operator screen.
//! - `move strafe,forward[,sprint]`, `stop`, `stance stand|crouch|prone`,
//!   `look yaw,pitch` (degrees), `teleport x,y,z[,yaw[,pitch]]`.
//! - `kill`: the local player dies (1000 damage).
//! - `walker attackers|defenders x,y,z[,yaw]`: spawn a stand-in character
//!   (a grey capsule) on that team; `walker-move strafe,forward[,sprint]`,
//!   `walker-stance stand|crouch|prone` and `walker-kill` drive all of them.
//!   They are ordinary characters (footsteps, wipes, gadgets see them).
//! - `dart x,y,z[,dx,dy,dz]`: an attackers' sensor dart, already listening,
//!   stuck at x,y,z (flight direction dx,dy,dz, default straight down).
//! - `stake x,y,z`: a defenders' seismic stake standing at x,y,z.
//! - `plant A|B`: a defuser is planted at that site (as if by an attacker).
//! - `noise x,y,z,loudness`: a world sound (e.g. an explosion is 6).
//! - `shot path.png`: save a screenshot. `log text`: write to the log.
//! - `quit`: exit (after pending screenshots are written).
//!
//! A script run implies an automated run: no mouse grab, no sounds. The
//! round state is logged once a second.

use std::path::PathBuf;

use avian3d::prelude::{CustomPositionIntegration, LinearVelocity, RigidBody, TransformInterpolation};
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};

use super::{
    Round, RoundEvent, RoundIntent, RoundPhase, RoundSettings, RoundSystems,
    defuser::{DefuserAssets, plant_device},
    sites::{KeepLayout, SiteId},
};
use crate::{
    combat::{DamageKind, DamageMessage, Health, Noise, NoiseKind, Team},
    config::Tuning,
    gadgets::{GadgetAssets, GadgetIntent, spawn_listening_dart, spawn_stake},
    player::{CharacterState, LocalPlayer, Player, PlayerInput, Stance},
};

pub struct RoundScriptPlugin;

/// Where scripted input is copied into the player's intents, just before
/// the fixed tick.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScriptInputSystems;

impl Plugin for RoundScriptPlugin {
    fn build(&self, app: &mut App) {
        let Some(raw) = arg_value("--round-script") else {
            return;
        };
        let steps = match parse_script(&raw) {
            Ok(steps) => steps,
            Err(err) => {
                error!("--round-script: {err}");
                Vec::new()
            }
        };
        info!("round script: {} steps", steps.len());
        app.insert_resource(Script { steps, next: 0 })
            .init_resource::<ScriptInput>()
            .add_systems(Startup, create_walker_assets)
            .add_systems(FixedUpdate, (run_script, log_status).in_set(RoundSystems::Script))
            .add_systems(
                RunFixedMainLoop,
                apply_script_input
                    .in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop)
                    .in_set(ScriptInputSystems),
            )
            .add_systems(Update, quit_when_done);
    }
}

#[derive(Resource)]
struct Script {
    steps: Vec<Step>,
    next: usize,
}

#[derive(Debug, Clone)]
struct Step {
    round: Option<u32>,
    phase: RoundPhase,
    at: f32,
    command: Command,
    text: String,
}

#[derive(Debug, Clone)]
enum Command {
    Hold(bool),
    Equip,
    Gadget,
    Pick(u8),
    Move(Vec2, bool),
    Stance(Stance),
    Look(f32, f32),
    Teleport(Vec3, Option<f32>, Option<f32>),
    Kill,
    Walker(Team, Vec3, f32),
    WalkerMove(Vec2, bool),
    WalkerStance(Stance),
    WalkerKill,
    Dart(Vec3, Vec3),
    Stake(Vec3),
    Plant(SiteId),
    Noise(Vec3, f32),
    Shot(PathBuf),
    Log(String),
    Quit,
}

/// Inputs the script is holding down.
#[derive(Resource, Default)]
struct ScriptInput {
    interact: bool,
    move_axis: Vec2,
    sprint: bool,
    stance: Option<Stance>,
    walker_move: Vec2,
    walker_sprint: bool,
    walker_stance: Stance,
}

/// A stand-in character spawned by the script.
#[derive(Component)]
struct ScriptWalker;

#[derive(Resource)]
struct WalkerAssets {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

fn create_walker_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(WalkerAssets {
        mesh: meshes.add(Capsule3d::new(0.3, 1.0)),
        material: materials.add(Color::srgb(0.55, 0.55, 0.6)),
    });
}

fn parse_stance(s: &str) -> Result<Stance, String> {
    match s {
        "stand" => Ok(Stance::Stand),
        "crouch" => Ok(Stance::Crouch),
        "prone" => Ok(Stance::Prone),
        other => Err(format!("bad stance {other:?}")),
    }
}

fn parse_move(args: &str) -> Result<(Vec2, bool), String> {
    let n = floats(args.trim_end_matches(",sprint"))?;
    let [x, y] = n[..] else {
        return Err("move expects strafe,forward[,sprint]".into());
    };
    Ok((Vec2::new(x, y), args.ends_with("sprint")))
}

/// Counts frames down to exit, so screenshots finish writing first.
#[derive(Resource)]
struct QuitIn(u32);

fn arg_value(flag: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn parse_script(raw: &str) -> Result<Vec<Step>, String> {
    raw.split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(parse_step)
        .collect()
}

fn floats(s: &str) -> Result<Vec<f32>, String> {
    s.split(',')
        .map(|v| v.trim().parse::<f32>().map_err(|_| format!("bad number {v:?}")))
        .collect()
}

fn vec3(n: &[f32]) -> Result<Vec3, String> {
    match n {
        [x, y, z, ..] => Ok(Vec3::new(*x, *y, *z)),
        _ => Err("expected x,y,z".into()),
    }
}

fn parse_step(text: &str) -> Result<Step, String> {
    let (when, what) = text
        .split_once(':')
        .ok_or_else(|| format!("{text:?}: expected <phase>@<seconds>: <command>"))?;
    let (round, when) = match when.trim().strip_prefix('r') {
        Some(rest) if rest.contains('.') => {
            let (n, rest) = rest.split_once('.').expect("checked");
            (Some(n.parse::<u32>().map_err(|_| format!("bad round in {text:?}"))?), rest)
        }
        _ => (None, when.trim()),
    };
    let (phase, at) = when
        .split_once('@')
        .ok_or_else(|| format!("{text:?}: expected <phase>@<seconds>"))?;
    let phase = match phase.trim() {
        "select" => RoundPhase::OperatorSelect,
        "prep" => RoundPhase::Prep,
        "action" => RoundPhase::Action,
        "planted" => RoundPhase::Planted,
        "end" => RoundPhase::RoundEnd,
        "matchend" => RoundPhase::MatchEnd,
        other => return Err(format!("unknown phase {other:?}")),
    };
    let at: f32 = at.trim().parse().map_err(|_| format!("bad time in {text:?}"))?;
    let what = what.trim();
    let (name, args) = what.split_once(' ').unwrap_or((what, ""));
    let args = args.trim();
    let command = match name {
        "hold" => Command::Hold(true),
        "release" => Command::Hold(false),
        "equip" => Command::Equip,
        "gadget" => Command::Gadget,
        "pick" => Command::Pick(args.parse().map_err(|_| "pick expects a number")?),
        "move" => {
            let (axis, sprint) = parse_move(args)?;
            Command::Move(axis, sprint)
        }
        "stop" => Command::Move(Vec2::ZERO, false),
        "stance" => Command::Stance(parse_stance(args)?),
        "walker" => {
            let (team, rest) = args.split_once(' ').ok_or("walker expects <team> x,y,z[,yaw]")?;
            let team = match team {
                "attackers" => Team::Attackers,
                "defenders" => Team::Defenders,
                other => return Err(format!("bad team {other:?}")),
            };
            let n = floats(rest)?;
            Command::Walker(team, vec3(&n)?, n.get(3).copied().unwrap_or(0.0))
        }
        "walker-move" => {
            let (axis, sprint) = parse_move(args)?;
            Command::WalkerMove(axis, sprint)
        }
        "walker-stance" => Command::WalkerStance(parse_stance(args)?),
        "walker-kill" => Command::WalkerKill,
        "look" => {
            let n = floats(args)?;
            let [yaw, pitch] = n[..] else {
                return Err("look expects yaw,pitch".into());
            };
            Command::Look(yaw, pitch)
        }
        "teleport" => {
            let n = floats(args)?;
            Command::Teleport(vec3(&n)?, n.get(3).copied(), n.get(4).copied())
        }
        "kill" => Command::Kill,
        "dart" => {
            let n = floats(args)?;
            let dir = if n.len() >= 6 { Vec3::new(n[3], n[4], n[5]) } else { Vec3::NEG_Y };
            Command::Dart(vec3(&n)?, dir)
        }
        "stake" => Command::Stake(vec3(&floats(args)?)?),
        "plant" => Command::Plant(match args {
            "A" | "a" => SiteId::A,
            "B" | "b" => SiteId::B,
            other => return Err(format!("bad site {other:?}")),
        }),
        "noise" => {
            let n = floats(args)?;
            Command::Noise(vec3(&n)?, n.get(3).copied().unwrap_or(6.0))
        }
        "shot" => Command::Shot(args.into()),
        "log" => Command::Log(args.into()),
        "quit" => Command::Quit,
        other => return Err(format!("unknown command {other:?}")),
    };
    Ok(Step {
        round,
        phase,
        at,
        command,
        text: text.to_string(),
    })
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn run_script(
    mut script: ResMut<Script>,
    mut input: ResMut<ScriptInput>,
    mut round: ResMut<Round>,
    settings: Res<RoundSettings>,
    tuning: Res<Tuning>,
    layout: Res<KeepLayout>,
    defuser_assets: Res<DefuserAssets>,
    gadget_assets: Res<GadgetAssets>,
    walker_assets: Res<WalkerAssets>,
    walkers: Query<Entity, With<ScriptWalker>>,
    mut player: Query<
        (
            Entity,
            &mut Transform,
            &mut PlayerInput,
            &mut LinearVelocity,
            &mut RoundIntent,
            &mut GadgetIntent,
        ),
        With<LocalPlayer>,
    >,
    mut damage: MessageWriter<DamageMessage>,
    mut noise: MessageWriter<Noise>,
    mut events: MessageWriter<RoundEvent>,
    mut commands: Commands,
) {
    let Ok((me, mut transform, mut player_input, mut velocity, mut intent, mut gadget)) =
        player.single_mut()
    else {
        return;
    };
    while let Some(step) = script.steps.get(script.next).cloned() {
        let ready = round.phase == step.phase
            && round.elapsed >= step.at
            && step.round.is_none_or(|n| n == round.number);
        if !ready {
            break;
        }
        script.next += 1;
        info!(
            "script: [round {} {:?} {:.2}s] {}",
            round.number, round.phase, round.elapsed, step.text
        );
        match step.command {
            Command::Hold(on) => input.interact = on,
            Command::Equip => intent.equip_defuser = true,
            Command::Gadget => gadget.use_unique = true,
            Command::Pick(n) => intent.pick_operator = Some(n.saturating_sub(1)),
            Command::Move(axis, sprint) => {
                input.move_axis = axis;
                input.sprint = sprint;
            }
            Command::Stance(stance) => input.stance = Some(stance),
            Command::Look(yaw, pitch) => {
                player_input.yaw = yaw.to_radians().rem_euclid(std::f32::consts::TAU);
                player_input.pitch = pitch.to_radians();
            }
            Command::Teleport(at, yaw, pitch) => {
                transform.translation = at;
                velocity.0 = Vec3::ZERO;
                if let Some(yaw) = yaw {
                    player_input.yaw = yaw.to_radians().rem_euclid(std::f32::consts::TAU);
                    transform.rotation = Quat::from_rotation_y(player_input.yaw);
                }
                if let Some(pitch) = pitch {
                    player_input.pitch = pitch.to_radians();
                }
            }
            Command::Kill => {
                damage.write(DamageMessage {
                    target: me,
                    amount: 1000.0,
                    kind: DamageKind::Bullet,
                    zone: None,
                    source: None,
                });
            }
            Command::Walker(team, at, yaw) => {
                let yaw = yaw.to_radians();
                let height = tuning.body.stand_height;
                commands.spawn((
                    Name::new(format!("Script walker ({team:?})")),
                    ScriptWalker,
                    Player,
                    PlayerInput { yaw, ..default() },
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
                    team,
                    Transform::from_translation(at).with_rotation(Quat::from_rotation_y(yaw)),
                    Visibility::default(),
                    (RigidBody::Kinematic, CustomPositionIntegration, TransformInterpolation, LinearVelocity::ZERO),
                    children![(
                        Mesh3d(walker_assets.mesh.clone()),
                        MeshMaterial3d(walker_assets.material.clone()),
                        Transform::from_xyz(0.0, 0.8, 0.0),
                    )],
                ));
            }
            Command::WalkerMove(axis, sprint) => {
                input.walker_move = axis;
                input.walker_sprint = sprint;
            }
            Command::WalkerStance(stance) => input.walker_stance = stance,
            Command::WalkerKill => {
                for walker in &walkers {
                    damage.write(DamageMessage {
                        target: walker,
                        amount: 1000.0,
                        kind: DamageKind::Bullet,
                        zone: None,
                        source: None,
                    });
                }
            }
            Command::Dart(at, direction) => {
                spawn_listening_dart(&mut commands, &gadget_assets, &tuning, at, direction, Team::Attackers);
                info!("script: attackers' dart listening at {at:.2}");
            }
            Command::Stake(at) => {
                spawn_stake(&mut commands, &gadget_assets, &tuning, at, Team::Defenders, None);
                info!("script: defenders' stake at {at:.2}");
            }
            Command::Plant(site) => {
                if round.phase != RoundPhase::Action {
                    warn!("script: plant only works in the action phase");
                } else if let Some(site) = layout.site(site) {
                    let timers = settings.timers(&tuning.round).clone();
                    plant_device(
                        &mut commands,
                        &defuser_assets,
                        &mut round,
                        &timers,
                        site.id,
                        site.centre(),
                        0.0,
                        None,
                        &mut events,
                    );
                }
            }
            Command::Noise(at, loudness) => {
                noise.write(Noise {
                    source: None,
                    position: at,
                    loudness,
                    kind: NoiseKind::Explosion,
                });
            }
            Command::Shot(path) => {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).ok();
                }
                info!("script: screenshot -> {}", path.display());
                commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
            }
            Command::Log(text) => info!("script: {text}"),
            Command::Quit => commands.insert_resource(QuitIn(30)),
        }
    }
}

/// Copy held script inputs into the local player's intents every frame
/// (after the keyboard, which is ignored in automated runs).
fn apply_script_input(
    input: Res<ScriptInput>,
    mut player: Query<(&mut PlayerInput, &mut RoundIntent), (With<LocalPlayer>, Without<ScriptWalker>)>,
    mut walkers: Query<&mut PlayerInput, With<ScriptWalker>>,
) {
    for (mut player_input, mut intent) in &mut player {
        player_input.move_axis = input.move_axis;
        player_input.sprint = input.sprint;
        if let Some(stance) = input.stance {
            player_input.stance = stance;
        }
        intent.interact = input.interact;
    }
    for mut walker in &mut walkers {
        walker.move_axis = input.walker_move;
        walker.sprint = input.walker_sprint;
        walker.stance = input.walker_stance;
    }
}

/// Once a second: round, phase, timer, score, and where the player is.
fn log_status(
    round: Res<Round>,
    player: Query<(&Transform, &Team), With<LocalPlayer>>,
    mut ticks: Local<u32>,
) {
    *ticks += 1;
    if *ticks % 64 != 0 {
        return;
    }
    let (at, team) = player
        .single()
        .map(|(t, team)| (t.translation, format!("{team:?}")))
        .unwrap_or((Vec3::ZERO, "-".into()));
    info!(
        "status: round {} {:?} {:.1}s left, score blue {} red {}, local {team} at {at:.2}",
        round.number, round.phase, round.time_left, round.score_blue, round.score_red
    );
}

fn quit_when_done(quit: Option<ResMut<QuitIn>>, mut exit: MessageWriter<AppExit>) {
    if let Some(mut quit) = quit {
        if quit.0 == 0 {
            info!("script: done, exiting");
            exit.write(AppExit::Success);
        } else {
            quit.0 -= 1;
        }
    }
}
