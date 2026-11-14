//! `--gadget-test "<script>"`: automated checks for gadgets, drones and
//! CCTV. A timed list of `<seconds> <command> [args]` separated by `;`
//! (seconds since start; the map needs a second or two to load). Combine
//! with `--no-round --operator <name>` for free play, or with the round loop
//! (`--round-test`). The run doesn't grab the mouse and plays no sounds.
//!
//! | Command | Effect |
//! |---|---|
//! | `op auger\|lark\|mortise\|fathom` | become that operator (free play), on its side |
//! | `secondary <name>` | carry that secondary (breach, frag, flash, smoke, wire, cover, impact, alarm) |
//! | `tp x y z [yaw pitch]`, `look yaw pitch` | place / aim the player (degrees; yaw 0 = -Z, 90 = -X) |
//! | `aimat x y z` | aim the player's eyes at a point |
//! | `move strafe forward`, `stance stand\|crouch\|prone` | hold a movement input / stance |
//! | `hold3 secs`, `tap3` | hold / tap the unique-gadget key |
//! | `hold4 secs`, `tap4` | hold / tap the secondary-gadget key |
//! | `g [secs]`, `next`, `prev` | recon key (tap, or hold), switch drone / camera |
//! | `drive strafe forward` | drone drive axis while connected (`drive 0 0` stops) |
//! | `dlook yaw pitch` | drone heading and camera tilt, or CCTV pan and tilt (degrees) |
//! | `dface x z` | turn the drone you are driving towards a point |
//! | `reinforce` | reinforce the wall/hatch aimed at, plate on your side |
//! | `hardbreach r` | hard breach (radius r) where you aim |
//! | `fire` | pull the trigger once (a real M3 shot) |
//! | `shoot [damage]` | a test bullet from the eye: the first thing with health takes the damage |
//! | `aimblade`, `shootblade [damage]` | aim at an exposed ring-cutter blade; aim and shoot it (test bullet) |
//! | `dummy x y z` | a target with health and hit zones |
//! | `walker attackers\|defenders x y z [yaw]`, `walker-move strafe forward` | stand-in characters |
//! | `explode x y z [breach radius damage]` | an explosion |
//! | `lamp x y z [lumens]` | a point light |
//! | `reset` | a new-round reset (`RoundReset`) |
//! | `log` | gadget state, stock, dummies, walkers, surfaces near the player |
//! | `shot path.png`, `quit` | screenshot; exit |

use avian3d::prelude::*;
use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};

use super::{
    DroneIntent, GadgetActivity, GadgetIntent, SecondaryGadget, UniqueGadget,
    backplate::BackplateCharge,
    cctv::CctvCamera,
    cutter::{CutterBlade, RingCutter},
    deployables::{BallisticCover, GadgetSpeedScale, ProximityAlarm, RazorWire},
    drone::{Drone, ReconStock},
    effects::Flashed,
    eye_and_look,
    remote_view::{FreezeViewers, RemoteView},
    secondary::SecondaryStock,
    throwables::{Grenade, SmokeCloud},
};
use crate::{
    combat::{DamageKind, DamageMessage, Health, Team},
    config::Tuning,
    destruction::{
        BreachCharge, BreachCharges, DestructionIntent, Explosion, HardBreach, ReinforceRequest, SoftSurface, SurfaceChanged,
        surface_root,
    },
    physics::GameLayer,
    player::{AutomatedRun, CharacterState, HitZone, LocalPlayer, Player, PlayerInput, Stance},
    round::{
        RoundReset,
        operators::{Loadout, OperatorChoice, OperatorId, equip_operator},
    },
    weapons::WeaponInput,
};

pub struct GadgetTestPlugin;

impl Plugin for GadgetTestPlugin {
    fn build(&self, app: &mut App) {
        let Some(raw) = arg_value("--gadget-test") else { return };
        let script = Script::parse(&raw);
        info!("gadget test: {} commands", script.commands.len());
        app.insert_resource(script)
            .insert_resource(AutomatedRun)
            .init_resource::<Held>()
            .add_systems(FixedPreUpdate, run_script)
            .add_systems(
                RunFixedMainLoop,
                apply_held
                    .in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop)
                    .before(FreezeViewers),
            )
            .add_systems(Update, (log_changes, quit_when_done));
    }
}

#[derive(Debug, Clone)]
enum Command {
    Op(OperatorId),
    Secondary(SecondaryGadget),
    Teleport(Vec3, Option<(f32, f32)>),
    Look(f32, f32),
    AimAt(Vec3),
    Move(Vec2),
    Stance(Stance),
    Unique(f32),
    Second(f32),
    Recon(f32),
    Cycle(i32),
    Drive(Vec2),
    DroneLook(f32, f32),
    DroneFace(f32, f32),
    Reinforce,
    HardBreach(f32),
    Fire,
    Shoot(f32),
    AimBlade,
    ShootBlade(f32),
    Dummy(Vec3),
    Walker(Team, Vec3, f32),
    WalkerMove(Vec2),
    Explode(Vec3, f32, f32, f32),
    Lamp(Vec3, f32),
    Reset,
    Log,
    Shot(String),
    Quit,
}

#[derive(Resource)]
struct Script {
    commands: Vec<(f32, Command)>,
    next: usize,
}

/// A tap: held for a few ticks (shorter than `tap_time`).
const TAP: f32 = 0.06;

impl Script {
    fn parse(raw: &str) -> Self {
        let mut commands = Vec::new();
        for part in raw.split(';').map(str::trim).filter(|p| !p.is_empty()) {
            let words: Vec<&str> = part.split_whitespace().collect();
            let num = |i: usize, default: f32| -> f32 { words.get(i).and_then(|w| w.parse().ok()).unwrap_or(default) };
            let v3 = |i: usize| Vec3::new(num(i, 0.0), num(i + 1, 0.0), num(i + 2, 0.0));
            let Some(time) = words.first().and_then(|w| w.parse::<f32>().ok()) else {
                warn!("gadget test: no time in {part:?}");
                continue;
            };
            let arg = words.get(2).copied().unwrap_or("");
            let command = match words.get(1).copied().unwrap_or("") {
                "op" => match OperatorId::from_name(arg) {
                    Some(op) => Command::Op(op),
                    None => {
                        warn!("gadget test: unknown operator {arg:?}");
                        continue;
                    }
                },
                "secondary" => match SecondaryGadget::from_name(arg) {
                    Some(g) => Command::Secondary(g),
                    None => {
                        warn!("gadget test: unknown secondary {arg:?}");
                        continue;
                    }
                },
                "tp" => Command::Teleport(v3(2), (words.len() >= 7).then(|| (num(5, 0.0), num(6, 0.0)))),
                "look" => Command::Look(num(2, 0.0), num(3, 0.0)),
                "aimat" => Command::AimAt(v3(2)),
                "move" => Command::Move(Vec2::new(num(2, 0.0), num(3, 0.0))),
                "stance" => Command::Stance(match arg {
                    "crouch" => Stance::Crouch,
                    "prone" => Stance::Prone,
                    _ => Stance::Stand,
                }),
                "hold3" => Command::Unique(num(2, 1.0)),
                "tap3" => Command::Unique(TAP),
                "hold4" => Command::Second(num(2, 1.0)),
                "tap4" => Command::Second(TAP),
                "g" => Command::Recon(num(2, TAP)),
                "next" => Command::Cycle(1),
                "prev" => Command::Cycle(-1),
                "drive" => Command::Drive(Vec2::new(num(2, 0.0), num(3, 0.0))),
                "dlook" => Command::DroneLook(num(2, 0.0), num(3, 0.0)),
                "dface" => Command::DroneFace(num(2, 0.0), num(3, 0.0)),
                "reinforce" => Command::Reinforce,
                "hardbreach" => Command::HardBreach(num(2, 0.5)),
                "fire" => Command::Fire,
                "shoot" => Command::Shoot(num(2, 100.0)),
                "aimblade" => Command::AimBlade,
                "shootblade" => Command::ShootBlade(num(2, 10.0)),
                "dummy" => Command::Dummy(v3(2)),
                "walker" => {
                    let team = if arg == "defenders" { Team::Defenders } else { Team::Attackers };
                    Command::Walker(team, v3(3), num(6, 0.0))
                }
                "walker-move" => Command::WalkerMove(Vec2::new(num(2, 0.0), num(3, 0.0))),
                "explode" => Command::Explode(v3(2), num(5, 0.85), num(6, 3.5), num(7, 120.0)),
                "lamp" => Command::Lamp(v3(2), num(5, 2.0e7)),
                "reset" => Command::Reset,
                "log" => Command::Log,
                "shot" => Command::Shot(if arg.is_empty() { "gadget.png".into() } else { arg.into() }),
                "quit" => Command::Quit,
                other => {
                    warn!("gadget test: unknown command {other:?}");
                    continue;
                }
            };
            commands.push((time, command));
        }
        commands.sort_by(|a, b| a.0.total_cmp(&b.0));
        Self { commands, next: 0 }
    }
}

/// Inputs the script holds down.
#[derive(Resource, Default)]
struct Held {
    movement: Option<Vec2>,
    stance: Option<Stance>,
    unique_until: f32,
    secondary_until: f32,
    recon_until: f32,
    drive: Vec2,
    cycle: i32,
    look: Option<(f32, f32)>,
    walker_move: Vec2,
}

/// A stand-in character spawned by the script.
#[derive(Component)]
struct TestWalker;

/// A target spawned by the script.
#[derive(Component)]
struct TestDummy;

/// Frames left before exiting (so screenshots finish).
#[derive(Resource)]
struct QuitIn(u32);

fn arg_value(flag: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn run_script(
    mut script: ResMut<Script>,
    mut held: ResMut<Held>,
    time: Res<Time>,
    tuning: Res<Tuning>,
    mut player: Query<
        (
            Entity,
            &mut Transform,
            &mut PlayerInput,
            &CharacterState,
            &OperatorChoice,
            Option<&Loadout>,
            Option<&mut WeaponInput>,
            Option<&Children>,
        ),
        With<LocalPlayer>,
    >,
    spatial: SpatialQuery,
    colliders: Query<&ColliderOf>,
    healthy: Query<(&Name, &Health)>,
    logs: LogQueries,
    mut damage: MessageWriter<DamageMessage>,
    mut explosions: MessageWriter<Explosion>,
    mut breaches: MessageWriter<HardBreach>,
    mut reinforce: MessageWriter<ReinforceRequest>,
    mut resets: MessageWriter<RoundReset>,
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    let Ok((me, mut transform, mut input, state, choice, loadout, weapon, children)) = player.single_mut() else {
        return;
    };
    let mut weapon = weapon;
    while let Some((at, command)) = script.commands.get(script.next).cloned() {
        if at > now {
            break;
        }
        script.next += 1;
        info!("gadget test t={now:.2}: {command:?}");
        let (eye, look) = eye_and_look(&transform, state, &input, &tuning.body, tuning.lean.offset);
        let own: Vec<Entity> = children.map(|c| c.iter().collect()).unwrap_or_default();
        let aim_world = || {
            let filter = SpatialQueryFilter::from_mask(GameLayer::World);
            spatial.cast_ray(eye, look, 8.0, true, &filter).map(|hit| (hit.entity, eye + *look * hit.distance))
        };
        match command {
            Command::Op(op) => {
                commands.entity(me).insert(op.info().side);
                equip_operator(&mut commands, me, op, &tuning.round, choice.secondary_for(op));
            }
            Command::Secondary(gadget) => match loadout {
                Some(l) => {
                    let mut l = l.clone();
                    l.secondary = gadget;
                    commands.entity(me).insert(l);
                }
                None => warn!("gadget test: secondary needs an operator first (op <name>)"),
            },
            Command::Teleport(p, angles) => {
                transform.translation = p;
                if let Some((yaw, pitch)) = angles {
                    input.yaw = yaw.to_radians().rem_euclid(std::f32::consts::TAU);
                    input.pitch = pitch.to_radians();
                    transform.rotation = Quat::from_rotation_y(input.yaw);
                }
            }
            Command::Look(yaw, pitch) => {
                input.yaw = yaw.to_radians().rem_euclid(std::f32::consts::TAU);
                input.pitch = pitch.to_radians();
            }
            Command::AimAt(p) => {
                let to = p - eye;
                input.yaw = (-to.x).atan2(-to.z).rem_euclid(std::f32::consts::TAU);
                input.pitch = (to.y / to.length().max(1e-4)).asin();
            }
            Command::Move(axis) => held.movement = Some(axis),
            Command::Stance(stance) => held.stance = Some(stance),
            Command::Unique(secs) => held.unique_until = now + secs,
            Command::Second(secs) => held.secondary_until = now + secs,
            Command::Recon(secs) => held.recon_until = now + secs,
            Command::Cycle(by) => held.cycle += by,
            Command::Drive(axis) => held.drive = axis,
            Command::DroneLook(yaw, pitch) => held.look = Some((yaw.to_radians(), pitch.to_radians())),
            Command::DroneFace(x, z) => {
                let driving = logs.player.get(me).ok().and_then(|p| p.7.copied());
                match driving.and_then(|v| match v {
                    RemoteView::Drone(d) => logs.drones.get(d).ok(),
                    RemoteView::Camera(_) => None,
                }) {
                    Some((_, _, at, _)) => {
                        let to = Vec3::new(x, at.translation.y, z) - at.translation;
                        held.look = Some(((-to.x).atan2(-to.z), 0.0));
                    }
                    None => warn!("gadget test: dface: not driving a drone"),
                }
            }
            Command::Reinforce => match aim_world() {
                Some((collider, _)) => {
                    reinforce.write(ReinforceRequest {
                        surface: surface_root(collider, &colliders),
                        from: eye,
                        by: Some(me),
                    });
                }
                None => warn!("gadget test: reinforce: not aiming at anything"),
            },
            Command::HardBreach(radius) => match aim_world() {
                Some((collider, point)) => {
                    breaches.write(HardBreach {
                        surface: surface_root(collider, &colliders),
                        centre: point,
                        radius,
                        source: Some(me),
                    });
                }
                None => warn!("gadget test: hardbreach: not aiming at anything"),
            },
            Command::Fire => match weapon.as_mut() {
                Some(w) => {
                    w.trigger_presses = w.trigger_presses.wrapping_add(1);
                }
                None => warn!("gadget test: fire: no weapon"),
            },
            Command::AimBlade | Command::ShootBlade(_) => {
                let Some((_, _, blade_at)) = logs.blades.iter().next() else {
                    warn!("gadget test: no exposed blade");
                    continue;
                };
                let to = blade_at.translation() - eye;
                input.yaw = (-to.x).atan2(-to.z).rem_euclid(std::f32::consts::TAU);
                input.pitch = (to.y / to.length()).asin();
                if let Command::ShootBlade(amount) = command {
                    let dir = Dir3::new(to).unwrap_or(look);
                    let filter = SpatialQueryFilter::from_mask([GameLayer::World, GameLayer::Hitbox])
                        .with_excluded_entities(own.iter().copied().chain([me]));
                    match spatial.cast_ray(eye, dir, 60.0, true, &filter) {
                        Some(hit) => {
                            let target = healthy.get(hit.entity);
                            info!(
                                "  shot at the blade hit {} ({}) at {:.2} m",
                                hit.entity,
                                target.map(|(n, _)| n.as_str()).unwrap_or("no health"),
                                hit.distance
                            );
                            if target.is_ok() {
                                damage.write(DamageMessage {
                                    target: hit.entity,
                                    amount,
                                    kind: DamageKind::Bullet,
                                    zone: None,
                                    source: Some(me),
                                });
                            }
                        }
                        None => info!("  shot at the blade hit nothing"),
                    }
                }
            }
            Command::Shoot(amount) => {
                let filter = SpatialQueryFilter::from_mask([GameLayer::World, GameLayer::Hitbox])
                    .with_excluded_entities(own.iter().copied().chain([me]));
                match spatial.cast_ray(eye, look, 60.0, true, &filter) {
                    Some(hit) => {
                        let target = healthy.get(hit.entity);
                        info!(
                            "  shot hit {} ({}) at {:.2} m, health {:?}",
                            hit.entity,
                            target.map(|(n, _)| n.as_str()).unwrap_or("no health"),
                            hit.distance,
                            target.map(|(_, h)| h.current).ok()
                        );
                        if target.is_ok() {
                            damage.write(DamageMessage {
                                target: hit.entity,
                                amount,
                                kind: DamageKind::Bullet,
                                zone: None,
                                source: Some(me),
                            });
                        }
                    }
                    None => info!("  shot hit nothing"),
                }
            }
            Command::Dummy(p) => {
                commands
                    .spawn((Name::new("Test dummy"), TestDummy, Health::new(100.0), Transform::from_translation(p), Visibility::default()))
                    .with_children(|parent| {
                        for (zone, y) in [(HitZone::Head, 1.66), (HitZone::Torso, 1.17), (HitZone::Legs, 0.4)] {
                            let (radius, length) = zone.shape();
                            parent.spawn((
                                zone,
                                Collider::capsule(radius, length),
                                Sensor,
                                CollisionLayers::new(GameLayer::Hitbox, LayerMask::NONE),
                                Transform::from_xyz(0.0, y, 0.0),
                            ));
                        }
                    });
            }
            Command::Walker(walker_team, p, yaw) => {
                let yaw = yaw.to_radians();
                let height = tuning.body.stand_height;
                commands.spawn((
                    Name::new(format!("Test walker ({walker_team:?})")),
                    TestWalker,
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
                    walker_team,
                    Transform::from_translation(p).with_rotation(Quat::from_rotation_y(yaw)),
                    Visibility::default(),
                    (RigidBody::Kinematic, CustomPositionIntegration, TransformInterpolation, LinearVelocity::ZERO),
                    // A stand-in body so screenshots show where it is.
                    children![(
                        Mesh3d(meshes.add(Capsule3d::new(0.3, 1.2))),
                        MeshMaterial3d(materials.add(Color::srgb(0.55, 0.55, 0.6))),
                        Transform::from_xyz(0.0, 0.9, 0.0),
                    )],
                ));
            }
            Command::WalkerMove(axis) => held.walker_move = axis,
            Command::Explode(centre, breach, radius, amount) => {
                explosions.write(Explosion {
                    centre,
                    breach_radius: breach,
                    damage_radius: radius,
                    max_damage: amount,
                    source: None,
                    attached_to: None,
                });
            }
            Command::Lamp(p, lumens) => {
                commands.spawn((
                    Name::new("Test lamp"),
                    PointLight {
                        intensity: lumens,
                        range: 30.0,
                        shadow_maps_enabled: true,
                        ..default()
                    },
                    Transform::from_translation(p),
                ));
            }
            Command::Reset => {
                resets.write(RoundReset { round: 99 });
            }
            Command::Log => log_state(&logs, me, transform.translation),
            Command::Shot(path) => {
                if let Some(dir) = std::path::Path::new(&path).parent() {
                    std::fs::create_dir_all(dir).ok();
                }
                info!("  screenshot -> {path}");
                commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
            }
            Command::Quit => commands.insert_resource(QuitIn(20)),
        }
    }
}

/// Everything `log` reports.
#[derive(bevy::ecs::system::SystemParam)]
struct LogQueries<'w, 's> {
    player: Query<
        'w,
        's,
        (
            &'static Team,
            &'static Health,
            &'static GadgetActivity,
            Option<&'static UniqueGadget>,
            Option<&'static SecondaryStock>,
            Option<&'static BreachCharges>,
            Option<&'static ReconStock>,
            Option<&'static RemoteView>,
            Option<&'static Flashed>,
            Option<&'static GadgetSpeedScale>,
            Option<&'static Loadout>,
        ),
    >,
    cutters: Query<'w, 's, (Entity, &'static RingCutter, &'static Health)>,
    blades: Query<'w, 's, (Entity, &'static CutterBlade, &'static GlobalTransform)>,
    backplates: Query<'w, 's, (Entity, &'static BackplateCharge)>,
    breach_charges: Query<'w, 's, (Entity, &'static BreachCharge)>,
    grenades: Query<'w, 's, (&'static Grenade, &'static Transform), Without<LocalPlayer>>,
    smoke: Query<'w, 's, &'static SmokeCloud>,
    wires: Query<'w, 's, (Entity, &'static RazorWire, &'static Health, &'static GlobalTransform)>,
    covers: Query<'w, 's, (Entity, &'static BallisticCover, &'static GlobalTransform)>,
    alarms: Query<'w, 's, (Entity, &'static ProximityAlarm, &'static GlobalTransform)>,
    drones: Query<'w, 's, (Entity, &'static Drone, &'static Transform, &'static Health), Without<LocalPlayer>>,
    cameras: Query<'w, 's, (Entity, &'static CctvCamera, &'static Health)>,
    dummies: Query<'w, 's, (Entity, &'static Health), With<TestDummy>>,
    walkers: Query<
        'w,
        's,
        (Entity, &'static Transform, &'static Health, Option<&'static GadgetSpeedScale>, &'static CharacterState),
        (With<TestWalker>, Without<LocalPlayer>),
    >,
    surfaces: Query<'w, 's, (Entity, &'static Name, &'static SoftSurface, &'static GlobalTransform)>,
    diagnostics: Res<'w, DiagnosticsStore>,
}

fn log_state(q: &LogQueries, me: Entity, at: Vec3) {
    if let Ok((team, health, activity, unique, stock, breach, recon, view, flashed, slow, loadout)) = q.player.get(me) {
        info!(
            "  player {team:?} at {at:.2} health {:.0} activity {activity:?} view {view:?} slow {:?}",
            health.current,
            slow.map(|s| s.0)
        );
        info!(
            "  stock: unique {:?} | loadout {:?} secondary {:?} | breach charges {:?} | drones {:?} | flashed {:?}",
            unique.map(|u| (u.kind, u.remaining, u.max)),
            loadout.map(|l| l.secondary),
            stock.map(|s| (s.kind, s.remaining, s.max)),
            breach.map(|b| b.0),
            recon.map(|r| (r.drones, r.max)),
            flashed.map(|f| (f.blind, f.deaf))
        );
    }
    for (e, c, h) in &q.cutters {
        info!(
            "  ring cutter {e}: {:?} {:.0}% exposed {} surface {} reinforced {} health {:.0}",
            c.state,
            c.progress * 100.0,
            c.exposed,
            c.surface,
            c.reinforced,
            h.current
        );
    }
    for (e, b, at) in &q.blades {
        info!("  blade {e} of {} at {:.2}", b.cutter, at.translation());
    }
    for (e, b) in &q.backplates {
        info!("  backplate {e} on {} at {:.2}", b.surface, b.centre);
    }
    for (e, b) in &q.breach_charges {
        info!("  breach charge {e} on {}", b.surface);
    }
    for (g, t) in &q.grenades {
        info!("  grenade {:?} at {:.2} fuse {:?}", g.kind, t.translation, g.fuse);
    }
    for s in &q.smoke {
        info!("  smoke cloud at {:.2} age {:.1}", s.centre, s.age);
    }
    for (e, _, h, at) in &q.wires {
        info!("  razor wire {e} at {:.2} health {:.0}", at.translation(), h.current);
    }
    for (e, c, at) in &q.covers {
        info!("  ballistic cover {e} at {:.2} health {:.0}", at.translation(), c.health);
    }
    for (e, a, at) in &q.alarms {
        info!("  proximity alarm {e} at {:.2} triggered {}", at.translation(), a.triggered);
    }
    for (e, d, t, h) in &q.drones {
        info!(
            "  drone {e} #{} {:?} at {:.2} yaw {:.0} battery {:.0}s health {:.0}",
            d.number,
            d.state,
            t.translation,
            d.yaw.to_degrees(),
            d.battery,
            h.current
        );
    }
    if let Some(fps) = q.diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.average()) {
        info!("  average fps {fps:.1}");
    }
    let cams: Vec<String> = q.cameras.iter().map(|(_, c, h)| format!("{}({:.0})", c.name, h.current)).collect();
    info!("  cctv: {}", cams.join(", "));
    for (e, h) in &q.dummies {
        info!("  dummy {e} health {:.1}", h.current);
    }
    for (e, t, h, slow, state) in &q.walkers {
        info!(
            "  walker {e} at {:.2} health {:.0} slow {:?} stance {:?}",
            t.translation,
            h.current,
            slow.map(|s| s.0),
            state.stance
        );
    }
    for (e, name, s, gt) in &q.surfaces {
        if gt.translation().distance(at) < 6.5 {
            info!(
                "  surface {e} {name}: {:.0}% intact, reinforced {}, broken {}",
                s.intact_fraction() * 100.0,
                s.is_reinforced(),
                s.broken
            );
        }
    }
}

/// Writes held inputs every frame (after the keyboard, which is ignored in
/// automated runs).
#[allow(clippy::type_complexity)]
fn apply_held(
    mut held: ResMut<Held>,
    time: Res<Time>,
    mut player: Query<
        (&mut PlayerInput, &mut GadgetIntent, &mut DroneIntent, Option<&mut DestructionIntent>),
        (With<LocalPlayer>, Without<TestWalker>),
    >,
    mut walkers: Query<&mut PlayerInput, (With<TestWalker>, Without<LocalPlayer>)>,
) {
    let now = time.elapsed_secs();
    for (mut input, mut gadget, mut drone, destruction) in &mut player {
        if let Some(axis) = held.movement {
            input.move_axis = axis;
        }
        if let Some(stance) = held.stance {
            input.stance = stance;
        }
        gadget.unique_held = now < held.unique_until;
        gadget.secondary_held = now < held.secondary_until;
        // Key 4 is also destruction's breach charge.
        if let Some(mut d) = destruction {
            d.gadget = gadget.secondary_held;
        }
        drone.recon_held = now < held.recon_until;
        drone.drive = held.drive;
        drone.cycle += std::mem::take(&mut held.cycle);
        if let Some((yaw, pitch)) = held.look.take() {
            drone.yaw = yaw;
            drone.pitch = pitch;
        }
    }
    for mut walker in &mut walkers {
        walker.move_axis = held.walker_move;
    }
}

/// Logs hard breaches and other surface changes while testing.
fn log_changes(mut changed: MessageReader<SurfaceChanged>, time: Res<Time>) {
    for c in changed.read() {
        info!(
            "t={:.2} surface {:?} lost {} cells (violent {})",
            time.elapsed_secs(),
            c.surface,
            c.removed.len(),
            c.violent
        );
    }
}

fn quit_when_done(quit: Option<ResMut<QuitIn>>, mut exit: MessageWriter<AppExit>) {
    if let Some(mut quit) = quit {
        if quit.0 == 0 {
            info!("gadget test: done, exiting");
            exit.write(AppExit::Success);
        } else {
            quit.0 -= 1;
        }
    }
}
