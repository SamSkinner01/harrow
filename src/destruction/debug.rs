//! Test flags for destruction (automated runs).
//!
//! `--m4 "<script>"` runs a timed script against the local player; commands
//! are separated by `;`, each starts with the time in seconds (simulation
//! time since start):
//!
//! ```text
//! --m4 "1 look 0 -5; 1.5 fire bullet 30 4; 4 shot /tmp/a.png; 4.5 quit"
//! ```
//!
//! | Command | Effect |
//! |---|---|
//! | `team attackers\|defenders` | switch the local player's side |
//! | `look <yaw> <pitch>` | aim (degrees; yaw 0 = -Z, 90 = -X) |
//! | `tp <x> <y> <z>` | teleport (feet) |
//! | `move <strafe> <forward>` | hold a movement input (`move 0 0` stops) |
//! | `stance stand\|crouch\|prone` | hold a stance |
//! | `fire bullet\|pellet <n> [spread]` | n shots from the eyes, one per tick (pellet: 8 per shot), spread in degrees; they go through soft surfaces like M3 bullets |
//! | `melee` | press V |
//! | `interact <secs>` / `gadget <secs>` | hold F / 4 (0 = a tap) |
//! | `explode <x> <y> <z> [breach] [damage_radius] [damage]` | an explosion |
//! | `hardbreach [radius]` | hard breach where the player aims |
//! | `reinforce` | instant reinforcement where the player aims |
//! | `lamp <x> <y> <z> [lumens]` | a point light (interiors are dark) |
//! | `log` | log surfaces, player and FPS |
//! | `shot <path>` | screenshot |
//! | `quit` | exit |
//!
//! The script implies an automated run (the mouse isn't grabbed).

use avian3d::prelude::*;
use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};

use super::{
    Activity, BreachCharges, DestructionIntent, DestructionSystems, Explosion, HardBreach,
    ReinforceRequest, Reinforcements, eye_and_aim, impacts::surface_root,
    surface::SoftSurface,
};
use crate::{
    combat::{Health, ImpactKind, SurfaceImpact, SurfaceKind, Team},
    config::Tuning,
    physics::GameLayer,
    player::{AutomatedRun, CharacterState, LocalPlayer, PlayerInput, Stance},
};

pub struct DestructionDebugPlugin;

impl Plugin for DestructionDebugPlugin {
    fn build(&self, app: &mut App) {
        let Some(raw) = arg_value("--m4") else { return };
        let script = Script::parse(&raw);
        info!("m4 script: {} commands", script.commands.len());
        app.insert_resource(script)
            .insert_resource(AutomatedRun)
            .init_resource::<Held>()
            .add_systems(FixedPreUpdate, (run_script, apply_held).chain())
            .add_systems(
                FixedUpdate,
                fire_queued_shots.in_set(DestructionSystems::Actions),
            );
    }
}

#[derive(Debug, Clone)]
enum Command {
    Team(Team),
    Look(f32, f32),
    Teleport(Vec3),
    Move(Vec2),
    Stance(Stance),
    Fire { pellets: bool, count: u32, spread: f32 },
    Melee,
    Interact(f32),
    Gadget(f32),
    Explode { centre: Vec3, breach: f32, radius: f32, damage: f32 },
    HardBreach(f32),
    Reinforce,
    Lamp(Vec3, f32),
    Log,
    Shot(String),
    Quit,
}

#[derive(Resource)]
struct Script {
    commands: Vec<(f32, Command)>,
    next: usize,
}

impl Script {
    fn parse(raw: &str) -> Self {
        let mut commands = Vec::new();
        for part in raw.split(';').map(str::trim).filter(|p| !p.is_empty()) {
            let words: Vec<&str> = part.split_whitespace().collect();
            let num = |i: usize, default: f32| -> f32 {
                words.get(i).and_then(|w| w.parse().ok()).unwrap_or(default)
            };
            let Some(time) = words.first().and_then(|w| w.parse::<f32>().ok()) else {
                warn!("m4 script: no time in {part:?}");
                continue;
            };
            let command = match words.get(1).copied().unwrap_or("") {
                "team" => Command::Team(if words.get(2) == Some(&"defenders") {
                    Team::Defenders
                } else {
                    Team::Attackers
                }),
                "look" => Command::Look(num(2, 0.0), num(3, 0.0)),
                "tp" => Command::Teleport(Vec3::new(num(2, 0.0), num(3, 0.0), num(4, 0.0))),
                "move" => Command::Move(Vec2::new(num(2, 0.0), num(3, 0.0))),
                "stance" => Command::Stance(match words.get(2).copied() {
                    Some("crouch") => Stance::Crouch,
                    Some("prone") => Stance::Prone,
                    _ => Stance::Stand,
                }),
                "fire" => Command::Fire {
                    pellets: words.get(2) == Some(&"pellet"),
                    count: num(3, 1.0) as u32,
                    spread: num(4, 0.0),
                },
                "melee" => Command::Melee,
                "interact" => Command::Interact(num(2, 0.0)),
                "gadget" => Command::Gadget(num(2, 0.0)),
                "explode" => Command::Explode {
                    centre: Vec3::new(num(2, 0.0), num(3, 0.0), num(4, 0.0)),
                    breach: num(5, 0.85),
                    radius: num(6, 3.5),
                    damage: num(7, 120.0),
                },
                "hardbreach" => Command::HardBreach(num(2, 0.5)),
                "reinforce" => Command::Reinforce,
                "lamp" => Command::Lamp(Vec3::new(num(2, 0.0), num(3, 0.0), num(4, 0.0)), num(5, 2.0e7)),
                "log" => Command::Log,
                "shot" => Command::Shot(words.get(2).copied().unwrap_or("m4.png").to_string()),
                "quit" => Command::Quit,
                other => {
                    warn!("m4 script: unknown command {other:?}");
                    continue;
                }
            };
            commands.push((time, command));
        }
        commands.sort_by(|a, b| a.0.total_cmp(&b.0));
        Self { commands, next: 0 }
    }
}

/// Inputs the script holds down, and shots waiting to be fired.
#[derive(Resource, Default)]
struct Held {
    movement: Option<Vec2>,
    stance: Option<Stance>,
    interact_until: f32,
    gadget_until: f32,
    melee_tick: bool,
    gadget_tap: bool,
    shots: Vec<(bool, f32, u32, u32)>,
}

#[allow(clippy::too_many_arguments)]
fn run_script(
    mut script: ResMut<Script>,
    mut held: ResMut<Held>,
    time: Res<Time>,
    mut player: Query<
        (Entity, &mut Transform, &mut PlayerInput, &CharacterState, Option<&Health>),
        With<LocalPlayer>,
    >,
    surfaces: Query<(Entity, &Name, &SoftSurface, &SurfaceKind)>,
    loadout: Query<(&Activity, &Reinforcements, &BreachCharges, &Team)>,
    spatial: SpatialQuery,
    colliders: Query<&ColliderOf>,
    tuning: Res<Tuning>,
    diagnostics: Res<DiagnosticsStore>,
    mut explosions: MessageWriter<Explosion>,
    mut breaches: MessageWriter<HardBreach>,
    mut reinforce: MessageWriter<ReinforceRequest>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    let Ok((entity, mut transform, mut input, state, health)) = player.single_mut() else {
        return;
    };
    while let Some((at, command)) = script.commands.get(script.next).cloned() {
        if at > now {
            break;
        }
        script.next += 1;
        info!("m4 script t={now:.2}: {command:?}");
        let aim = || {
            let (eye, dir) = eye_and_aim(&transform, state, &input, &tuning);
            let filter = SpatialQueryFilter::from_mask(GameLayer::World);
            spatial
                .cast_ray(eye, dir, 6.0, true, &filter)
                .map(|hit| (hit.entity, eye + *dir * hit.distance, eye))
        };
        match command {
            Command::Team(team) => {
                let t = &tuning.destruction;
                let charges = if team == Team::Attackers { t.breach_charge.count } else { 0 };
                commands.entity(entity).insert((team, BreachCharges(charges)));
            }
            Command::Look(yaw, pitch) => {
                input.yaw = yaw.to_radians();
                input.pitch = pitch.to_radians();
            }
            Command::Teleport(p) => transform.translation = p,
            Command::Move(axis) => held.movement = Some(axis),
            Command::Stance(stance) => held.stance = Some(stance),
            Command::Fire { pellets, count, spread } => {
                held.shots.push((pellets, spread, count, 0));
            }
            Command::Melee => held.melee_tick = true,
            Command::Interact(secs) => held.interact_until = now + secs.max(0.02),
            Command::Gadget(secs) => {
                if secs <= 0.0 {
                    held.gadget_tap = true;
                } else {
                    held.gadget_until = now + secs;
                }
            }
            Command::Explode { centre, breach, radius, damage } => {
                explosions.write(Explosion {
                    centre,
                    breach_radius: breach,
                    damage_radius: radius,
                    max_damage: damage,
                    source: None,
                    attached_to: None,
                });
            }
            Command::HardBreach(radius) => match aim() {
                Some((collider, point, _)) => {
                    breaches.write(HardBreach {
                        surface: collider,
                        centre: point,
                        radius,
                        source: Some(entity),
                    });
                }
                None => warn!("m4 script: hardbreach: not aiming at anything"),
            },
            Command::Reinforce => match aim() {
                Some((collider, _, eye)) => {
                    reinforce.write(ReinforceRequest {
                        surface: surface_root(collider, &colliders),
                        from: eye,
                        by: Some(entity),
                    });
                }
                None => warn!("m4 script: reinforce: not aiming at anything"),
            },
            Command::Lamp(at, lumens) => {
                commands.spawn((
                    Name::new("Debug lamp"),
                    PointLight {
                        intensity: lumens,
                        range: 30.0,
                        shadow_maps_enabled: true,
                        ..default()
                    },
                    Transform::from_translation(at),
                ));
            }
            Command::Log => {
                for (e, name, s, kind) in &surfaces {
                    info!(
                        "  surface {e:?} {name} {:?} {kind:?}: {:.0}% intact, reinforced {}, broken {}, revision {}",
                        s.role(),
                        s.intact_fraction() * 100.0,
                        s.is_reinforced(),
                        s.broken,
                        s.revision
                    );
                }
                if let Ok((activity, r, b, team)) = loadout.get(entity) {
                    info!("  player {team:?} {activity:?} reinforcements {} charges {}", r.0, b.0);
                }
                info!(
                    "  player feet {:.2} health {:?}",
                    transform.translation,
                    health.map(|h| h.current)
                );
                if let Some(fps) = diagnostics
                    .get(&FrameTimeDiagnosticsPlugin::FPS)
                    .and_then(|d| d.average())
                {
                    info!("  average fps {fps:.1}");
                }
            }
            Command::Shot(path) => {
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(path));
            }
            Command::Quit => {
                exit.write(AppExit::Success);
            }
        }
    }
}

/// Writes held inputs onto the local player every tick.
fn apply_held(
    mut held: ResMut<Held>,
    time: Res<Time>,
    mut player: Query<(&mut PlayerInput, &mut DestructionIntent), With<LocalPlayer>>,
) {
    let now = time.elapsed_secs();
    for (mut input, mut intent) in &mut player {
        if let Some(axis) = held.movement {
            input.move_axis = axis;
        }
        if let Some(stance) = held.stance {
            input.stance = stance;
        }
        intent.melee = held.melee_tick;
        intent.interact = now < held.interact_until;
        intent.gadget = held.gadget_tap || now < held.gadget_until;
    }
    held.melee_tick = false;
    held.gadget_tap = false;
}

/// Fires one queued shot per tick from the player's eyes. Each ray writes a
/// bullet/pellet impact on every surface it enters, passing through soft
/// ones and stopping at the first hard one (a stand-in for M3's bullets).
fn fire_queued_shots(
    mut held: ResMut<Held>,
    player: Query<(Entity, &Transform, &PlayerInput, &CharacterState), With<LocalPlayer>>,
    spatial: SpatialQuery,
    colliders: Query<&ColliderOf>,
    kinds: Query<&SurfaceKind>,
    tuning: Res<Tuning>,
    mut impacts: MessageWriter<SurfaceImpact>,
) {
    let Some(shot) = held.shots.first_mut() else { return };
    let (pellets, spread, count, fired) = *shot;
    shot.3 += 1;
    if shot.3 >= count {
        held.shots.remove(0);
    }
    let Ok((entity, transform, input, state)) = player.single() else {
        return;
    };
    let (eye, _) = eye_and_aim(transform, state, input, &tuning);
    // Golden-angle spiral: an even, repeatable spread pattern.
    let spiral = |i: u32, n: u32, radius: f32| {
        let r = radius.to_radians() * ((i as f32 + 0.5) / n.max(1) as f32).sqrt();
        let a = i as f32 * 2.399_963;
        (r * a.cos(), r * a.sin())
    };
    let (dy, dp) = spiral(fired, count, spread);
    let rays: Vec<(f32, f32)> = if pellets {
        (0..8)
            .map(|k| {
                let (py, pp) = spiral(k, 8, 3.0);
                (dy + py, dp + pp)
            })
            .collect()
    } else {
        vec![(dy, dp)]
    };
    let filter = SpatialQueryFilter::from_mask(GameLayer::World);
    for (yaw, pitch) in rays {
        let look = Quat::from_euler(EulerRot::YXZ, input.yaw + yaw, input.pitch + pitch, 0.0);
        let Ok(dir) = Dir3::new(look * Vec3::NEG_Z) else { continue };
        let mut hits = spatial.ray_hits(eye, dir, 60.0, 16, true, &filter);
        hits.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        for hit in hits {
            let root = surface_root(hit.entity, &colliders);
            let soft = kinds.get(root) == Ok(&SurfaceKind::Soft);
            impacts.write(SurfaceImpact {
                collider: hit.entity,
                point: eye + *dir * hit.distance,
                normal: hit.normal,
                direction: *dir,
                kind: if pellets { ImpactKind::Pellet } else { ImpactKind::Bullet },
                source: Some(entity),
            });
            if !soft {
                break;
            }
        }
    }
}

fn arg_value(flag: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}
