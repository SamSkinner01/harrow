//! `--net-script "<seconds> <command> [args]; ..."` on a server (dedicated
//! or host): setup and checks for networked tests
//! (`tools/scripts/net_test.sh`). Clients drive their own characters with
//! real input (`--gadget-test` on the client: keys, look, move, drive);
//! this changes the world, which only the server may do. Characters are
//! named by their player name (`--name` on the client).
//!
//! | Command | Effect |
//! |---|---|
//! | `equip NAME OP [SECONDARY]` | NAME becomes that operator (free play), on its side, with that secondary (frag, flash, smoke, wire, cover, impact, alarm, breach) |
//! | `tp NAME x y z` | move NAME there (its client gets corrected) |
//! | `reinforce NAME` | reinforce the wall or hatch NAME is looking at, plate on NAME's side |
//! | `log` | every character (team, operator, health, place, slow, view, stock) and gadget, and every damaged or reinforced surface |
//! | `quit` | exit |
//!
//! Commands run in order at their time; one naming a character that hasn't
//! joined yet waits for it.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::{NetPlayer, SimPosition};
use crate::{
    combat::{Health, Team},
    config::Tuning,
    destruction::{ReinforceRequest, SoftSurface, surface_root},
    gadgets::{
        BallisticCover, CctvCamera, Drone, Flashed, GadgetSpeedScale, Grenade, ProximityAlarm,
        RazorWire, ReconStock, RemoteView, RingCutter, SecondaryGadget, SecondaryStock,
        SmokeCloud, UniqueGadget, eye_and_look,
    },
    physics::GameLayer,
    player::{CharacterState, PlayerInput},
    round::operators::{Operator, OperatorId, equip_operator},
};

pub(super) struct NetScriptPlugin;

impl Plugin for NetScriptPlugin {
    fn build(&self, app: &mut App) {
        let Some(raw) = arg_value("--net-script") else {
            return;
        };
        let script = Script::parse(&raw);
        info!("net script: {} commands", script.commands.len());
        app.insert_resource(script)
            .add_systems(FixedPreUpdate, run_script.run_if(crate::session::is_authority));
    }
}

fn arg_value(flag: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

#[derive(Debug, Clone)]
enum Command {
    Equip(String, OperatorId, Option<SecondaryGadget>),
    Teleport(String, Vec3),
    Reinforce(String),
    Log,
    Quit,
}

impl Command {
    fn target(&self) -> Option<&str> {
        match self {
            Command::Equip(name, ..) | Command::Teleport(name, _) | Command::Reinforce(name) => {
                Some(name)
            }
            Command::Log | Command::Quit => None,
        }
    }
}

#[derive(Resource)]
struct Script {
    commands: Vec<(f32, Command)>,
    next: usize,
    /// Already said we're waiting for this command's character.
    waiting: bool,
}

impl Script {
    fn parse(raw: &str) -> Self {
        let mut commands = Vec::new();
        for part in raw.split(';').map(str::trim).filter(|p| !p.is_empty()) {
            let words: Vec<&str> = part.split_whitespace().collect();
            let num = |i: usize| -> f32 { words.get(i).and_then(|w| w.parse().ok()).unwrap_or(0.0) };
            let name = || words.get(2).map(|w| w.to_string()).unwrap_or_default();
            let Some(time) = words.first().and_then(|w| w.parse::<f32>().ok()) else {
                warn!("net script: no time in {part:?}");
                continue;
            };
            let command = match words.get(1).copied() {
                Some("equip") => match words.get(3).and_then(|w| OperatorId::from_name(w)) {
                    Some(op) => Command::Equip(
                        name(),
                        op,
                        words.get(4).and_then(|w| SecondaryGadget::from_name(w)),
                    ),
                    None => {
                        warn!("net script: equip NAME auger|lark|mortise|fathom [secondary], got {part:?}");
                        continue;
                    }
                },
                Some("tp") => Command::Teleport(name(), Vec3::new(num(3), num(4), num(5))),
                Some("reinforce") => Command::Reinforce(name()),
                Some("log") => Command::Log,
                Some("quit") => Command::Quit,
                other => {
                    warn!("net script: unknown command {other:?} in {part:?}");
                    continue;
                }
            };
            commands.push((time, command));
        }
        commands.sort_by(|a, b| a.0.total_cmp(&b.0));
        Self {
            commands,
            next: 0,
            waiting: false,
        }
    }
}

#[allow(clippy::type_complexity)]
type Characters<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static NetPlayer,
        &'static mut Transform,
        Option<&'static mut SimPosition>,
        Option<&'static mut LinearVelocity>,
        &'static CharacterState,
        &'static PlayerInput,
    ),
>;

#[allow(clippy::too_many_arguments)]
fn run_script(
    mut script: ResMut<Script>,
    time: Res<Time>,
    tuning: Res<Tuning>,
    mut characters: Characters,
    spatial: SpatialQuery,
    colliders: Query<&ColliderOf>,
    logs: LogQueries,
    mut reinforce: MessageWriter<ReinforceRequest>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    while let Some((at, command)) = script.commands.get(script.next).cloned() {
        if at > now {
            break;
        }
        let target = match command.target() {
            Some(name) => match characters.iter().find(|c| c.1.name == name).map(|c| c.0) {
                Some(entity) => Some(entity),
                None => {
                    if !script.waiting {
                        script.waiting = true;
                        info!("net script t={now:.2}: waiting for {name} to join");
                    }
                    break;
                }
            },
            None => None,
        };
        script.next += 1;
        script.waiting = false;
        info!("net script t={now:.2}: {command:?}");
        match (command, target) {
            (Command::Equip(_, op, secondary), Some(entity)) => {
                let secondary = secondary.unwrap_or(op.info().default_secondary);
                commands.entity(entity).insert(op.info().side);
                equip_operator(&mut commands, entity, op, &tuning.round, secondary);
            }
            (Command::Teleport(_, to), Some(entity)) => {
                if let Ok((_, _, mut transform, sim, velocity, ..)) = characters.get_mut(entity) {
                    transform.translation = to;
                    if let Some(mut sim) = sim {
                        sim.0 = to;
                    }
                    if let Some(mut velocity) = velocity {
                        velocity.0 = Vec3::ZERO;
                    }
                }
            }
            (Command::Reinforce(name), Some(entity)) => {
                let Ok((_, _, transform, _, _, state, input)) = characters.get(entity) else {
                    continue;
                };
                let (eye, look) = eye_and_look(&transform, state, input, &tuning.body, tuning.lean.offset);
                let filter = SpatialQueryFilter::from_mask(GameLayer::World);
                match spatial.cast_ray(eye, look, 6.0, true, &filter) {
                    Some(hit) => {
                        reinforce.write(ReinforceRequest {
                            surface: surface_root(hit.entity, &colliders),
                            from: eye,
                            by: Some(entity),
                        });
                    }
                    None => warn!("net script: {name} isn't looking at a wall"),
                }
            }
            (Command::Log, _) => log_state(&logs, &characters),
            (Command::Quit, _) => {
                exit.write(AppExit::Success);
            }
            _ => {}
        }
    }
}

/// Everything `log` reports.
#[derive(bevy::ecs::system::SystemParam)]
struct LogQueries<'w, 's> {
    people: Query<
        'w,
        's,
        (
            Option<&'static Team>,
            Option<&'static Operator>,
            Option<&'static Health>,
            Option<&'static GadgetSpeedScale>,
            Option<&'static RemoteView>,
            Option<&'static UniqueGadget>,
            Option<&'static SecondaryStock>,
            Option<&'static ReconStock>,
            Option<&'static Flashed>,
        ),
        With<NetPlayer>,
    >,
    cutters: Query<'w, 's, (Entity, &'static RingCutter)>,
    grenades: Query<'w, 's, (&'static Grenade, &'static Transform), Without<NetPlayer>>,
    smoke: Query<'w, 's, &'static SmokeCloud>,
    wires: Query<'w, 's, (Entity, &'static Transform), (With<RazorWire>, Without<NetPlayer>)>,
    covers: Query<'w, 's, (Entity, &'static BallisticCover, &'static Transform), Without<NetPlayer>>,
    alarms: Query<'w, 's, (Entity, &'static Transform), (With<ProximityAlarm>, Without<NetPlayer>)>,
    drones: Query<'w, 's, (Entity, &'static Drone, &'static Transform), Without<NetPlayer>>,
    cameras: Query<'w, 's, &'static CctvCamera>,
    surfaces: Query<'w, 's, (Entity, &'static Name, &'static SoftSurface)>,
}

fn log_state(q: &LogQueries, characters: &Characters) {
    for (entity, player, transform, ..) in characters.iter() {
        let Ok((team, op, health, slow, view, unique, secondary, drones, flashed)) = q.people.get(entity) else {
            continue;
        };
        info!(
            "  {} {entity}: {team:?} {:?} hp {:.0} at {:.2} slow {:?} view {view:?} | unique {:?} secondary {:?} drones {:?} flashed {:?}",
            player.name,
            op.map(|o| o.0),
            health.map_or(0.0, |h| h.current),
            transform.translation,
            slow.map(|s| s.0),
            unique.map(|u| (u.kind, u.remaining)),
            secondary.map(|s| (s.kind, s.remaining)),
            drones.map(|d| d.drones),
            flashed.map(|f| (f.blind, f.deaf)),
        );
    }
    for (e, c) in &q.cutters {
        info!(
            "  ring cutter {e}: {:?} {:.0}% exposed {} at {:.2}",
            c.state,
            c.progress * 100.0,
            c.exposed,
            c.near
        );
    }
    for (g, t) in &q.grenades {
        info!("  grenade {:?} at {:.2} fuse {:?}", g.kind, t.translation, g.fuse);
    }
    for s in &q.smoke {
        info!("  smoke cloud at {:.2} age {:.1}", s.centre, s.age);
    }
    for (e, t) in &q.wires {
        info!("  razor wire {e} at {:.2}", t.translation);
    }
    for (e, c, t) in &q.covers {
        info!("  ballistic cover {e} at {:.2} health {:.0}", t.translation, c.health);
    }
    for (e, t) in &q.alarms {
        info!("  proximity alarm {e} at {:.2}", t.translation);
    }
    for (e, d, t) in &q.drones {
        info!(
            "  drone {e} #{} {:?} at {:.2} yaw {:.0}",
            d.number,
            d.state,
            t.translation,
            d.yaw.to_degrees()
        );
    }
    info!("  cctv cameras: {}", q.cameras.iter().count());
    for (e, name, s) in &q.surfaces {
        if s.intact_fraction() < 0.999 || s.is_reinforced() {
            info!(
                "  surface {e} {name}: {:.0}% intact, reinforced {}, broken {}",
                s.intact_fraction() * 100.0,
                s.is_reinforced(),
                s.broken
            );
        }
    }
}
