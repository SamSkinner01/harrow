//! Looking through a drone or a CCTV camera (key G).
//!
//! Simulation (fixed tick, `use_recon`):
//! - Attackers: tap G to connect to your latest drone, or throw one if none
//!   is out; hold G to throw another; tap G again to come back. Q/E switch
//!   between your drones. Allowed in prep, action and planted.
//! - Defenders: tap G to watch the CCTV cameras, Q/E to switch, tap G to
//!   come back. Same phases.
//! - Viewing is [`RemoteView`] on the character (plain data). While it is
//!   there the body stands still: its intents are frozen at what they were
//!   (`FrozenInput`, reapplied before every fixed tick: no walking, turning,
//!   shooting or gadget use) and it carries the round's `MovementLock`.
//!   The view ends if the drone or camera breaks, the viewer dies, or the
//!   phase no longer allows it.
//!
//! Presentation: one extra 3D camera (`RemoteViewCamera`, child of a rig so
//! other modules' "top-level camera" queries never see it). When the local
//! player is viewing, every other active camera is switched off
//! (`Camera::is_active`), the remote one on, and it becomes the default UI
//! camera so the HUD still draws; a grainy, tinted overlay shows battery,
//! signal or the camera name.


use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use super::{
    DroneIntent, GadgetButtons, GadgetEvent, GadgetIntent, GadgetStage,
    cctv::CctvCamera,
    drone::{Drone, ReconStock, spawn_drone},
    eye_and_look,
};
use crate::{
    combat::{Dead, Team},
    config::Tuning,
    destruction::DestructionIntent,
    player::{CharacterState, LocalPlayer, Player, PlayerInput},
    round::{Round, RoundIntent, RoundPhase, RoundReset, RoundSystems},
    weapons::WeaponInput,
};

pub struct RemoteViewPlugin;

impl Plugin for RemoteViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Listener>()
            .init_resource::<CameraSwap>()
            .add_systems(Startup, (spawn_remote_camera, spawn_overlay))
            .add_systems(
                FixedUpdate,
                (
                    use_recon.in_set(GadgetStage::Use),
                    // Right after respawning, before the next frozen-input
                    // restore could undo the new spawn's facing.
                    end_views_on_reset
                        .after(RoundSystems::Reactions)
                        .run_if(crate::session::simulating),
                ),
            )
            .add_observer(freeze_networked_view)
            .add_observer(thaw_networked_view)
            .add_systems(
                RunFixedMainLoop,
                freeze_viewers
                    .in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop)
                    .in_set(FreezeViewers),
            )
            .add_systems(
                Update,
                (place_remote_camera, switch_cameras, update_overlay).chain(),
            );
    }
}

/// Where viewers' intents are frozen (before the fixed tick). Test input
/// should be written before this.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FreezeViewers;

/// What a character is looking through. Replicated to the viewer
/// (`net/protocol.rs`), whose client then switches cameras.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RemoteView {
    Drone(#[entities] Entity),
    Camera(#[entities] Entity),
}

/// The character's intents when the view started; reapplied every frame.
#[derive(Component, Clone)]
pub(crate) struct FrozenInput {
    input: PlayerInput,
    weapon: Option<WeaponInput>,
}

/// The camera used for drone and CCTV views.
#[derive(Component)]
pub struct RemoteViewCamera;

/// Parent of [`RemoteViewCamera`]; placed every frame.
#[derive(Component)]
struct RemoteViewRig;

/// Where sounds are heard from (the local player, or what they look
/// through).
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct Listener {
    pub position: Vec3,
}

/// Cameras switched off while the remote view is on.
#[derive(Resource, Default)]
struct CameraSwap {
    active: bool,
    switched_off: Vec<Entity>,
}

/// The camera the player sees the world through right now: the active one
/// with the lowest order (the player camera, or the remote view).
pub(crate) fn view_camera(cameras: &Query<(&Camera, &GlobalTransform)>) -> Option<GlobalTransform> {
    cameras
        .iter()
        .filter(|(c, _)| c.is_active)
        .min_by_key(|(c, _)| c.order)
        .map(|(_, t)| *t)
}

fn recon_allowed(phase: RoundPhase) -> bool {
    phase.is_live() || phase == RoundPhase::Inactive
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn use_recon(
    round: Res<Round>,
    tuning: Res<Tuning>,
    time: Res<Time>,
    mut players: Query<
        (
            Entity,
            &Transform,
            &CharacterState,
            &PlayerInput,
            Option<&WeaponInput>,
            &Team,
            &GadgetButtons,
            &mut DroneIntent,
            Option<&RemoteView>,
            Option<&mut ReconStock>,
            Has<Dead>,
        ),
        With<Player>,
    >,
    drones: Query<(Entity, &Drone), Without<Dead>>,
    cameras: Query<(Entity, &CctvCamera), Without<Dead>>,
    mut events: MessageWriter<GadgetEvent>,
    mut serial: Local<u32>,
    mut commands: Commands,
) {
    let g = &tuning.round.gadgets;
    let dt = time.delta_secs();
    let allowed_phase = recon_allowed(round.phase);
    for (entity, transform, state, input, weapon, team, buttons, mut intent, view, stock, dead) in &mut players {
        let button = buttons.recon;
        let cycle = std::mem::take(&mut intent.cycle);
        let allowed = allowed_phase && !dead;
        let mut own_drones: Vec<(Entity, &Drone)> = drones.iter().filter(|(_, d)| d.owner == entity).collect();
        own_drones.sort_by_key(|(_, d)| d.serial);
        let mut live_cameras: Vec<(Entity, &CctvCamera)> = cameras.iter().collect();
        live_cameras.sort_by_key(|(_, c)| c.index);

        let enter = |commands: &mut Commands, intent: &mut DroneIntent, target: RemoteView, events: &mut MessageWriter<GadgetEvent>| {
            match target {
                RemoteView::Drone(d) => {
                    let (yaw, pitch) = drones.get(d).map(|(_, d)| (d.yaw, d.pitch)).unwrap_or((input.yaw, 0.0));
                    intent.yaw = yaw;
                    intent.pitch = pitch;
                }
                RemoteView::Camera(_) => {
                    intent.yaw = 0.0;
                    intent.pitch = 0.0;
                }
            }
            intent.drive = Vec2::ZERO;
            commands.entity(entity).insert((
                target,
                FrozenInput {
                    input: input.clone(),
                    weapon: weapon.cloned(),
                },
            ));
            events.write(GadgetEvent::ViewEntered { by: entity, view: target });
        };
        let leave = |commands: &mut Commands, events: &mut MessageWriter<GadgetEvent>, lost: bool| {
            commands.entity(entity).remove::<(RemoteView, FrozenInput)>();
            events.write(GadgetEvent::ViewLeft { by: entity, lost });
        };
        // Step `by` through `list` from `current`.
        fn step<T: Copy + PartialEq>(list: &[T], current: Option<T>, by: i32) -> Option<T> {
            if list.is_empty() {
                return None;
            }
            let i = current.and_then(|c| list.iter().position(|x| *x == c)).map_or(0, |i| i as i32);
            Some(list[(i + by).rem_euclid(list.len() as i32) as usize])
        }

        // Throwing a drone: tap with none out, or hold (as the hold crosses
        // the tap time).
        let crossed_hold = button.held && button.held_for >= g.tap_time && button.held_for - dt < g.tap_time;
        let mut threw = None;
        if *team == Team::Attackers && allowed {
            let none_out = own_drones.is_empty();
            let wants = crossed_hold || (button.tapped && view.is_none() && none_out);
            if wants && let Some(mut stock) = stock {
                if stock.drones > 0 {
                    stock.drones -= 1;
                    *serial += 1;
                    let (eye, look) = eye_and_look(transform, state, input, &tuning.body, tuning.lean.offset);
                    let number = stock.max - stock.drones;
                    let drone = spawn_drone(&mut commands, entity, *team, number, *serial, eye, *look, input.yaw, &tuning);
                    events.write(GadgetEvent::DroneThrown { by: entity, drone });
                    threw = Some(drone);
                } else {
                    events.write(GadgetEvent::Refused { by: entity, reason: "no drones left".into() });
                }
            }
        }
        if let Some(drone) = threw {
            if view.is_some() {
                commands.entity(entity).insert(RemoteView::Drone(drone));
                intent.yaw = input.yaw;
                intent.pitch = 0.0;
                events.write(GadgetEvent::ViewEntered { by: entity, view: RemoteView::Drone(drone) });
            } else {
                enter(&mut commands, &mut intent, RemoteView::Drone(drone), &mut events);
            }
            continue;
        }

        match view.copied() {
            Some(current) => {
                let valid = allowed
                    && match current {
                        RemoteView::Drone(d) => drones.contains(d),
                        RemoteView::Camera(c) => cameras.contains(c),
                    };
                if !valid {
                    // Lost the feed: try the next one, else come back.
                    let next = if !allowed {
                        None
                    } else {
                        match current {
                            RemoteView::Drone(_) => own_drones.last().map(|(e, _)| RemoteView::Drone(*e)),
                            RemoteView::Camera(_) => live_cameras.first().map(|(e, _)| RemoteView::Camera(*e)),
                        }
                    };
                    match next {
                        Some(next) => {
                            // Static on screen, then the next feed.
                            events.write(GadgetEvent::ViewLeft { by: entity, lost: true });
                            enter(&mut commands, &mut intent, next, &mut events);
                        }
                        None => leave(&mut commands, &mut events, allowed),
                    }
                } else if button.tapped {
                    leave(&mut commands, &mut events, false);
                } else if cycle != 0 {
                    let next = match current {
                        RemoteView::Drone(d) => {
                            let list: Vec<Entity> = own_drones.iter().map(|(e, _)| *e).collect();
                            step(&list, Some(d), cycle).map(RemoteView::Drone)
                        }
                        RemoteView::Camera(c) => {
                            let list: Vec<Entity> = live_cameras.iter().map(|(e, _)| *e).collect();
                            step(&list, Some(c), cycle).map(RemoteView::Camera)
                        }
                    };
                    if let Some(next) = next.filter(|n| *n != current) {
                        // Keep the frozen body input from the first entry.
                        commands.entity(entity).insert(next);
                        match next {
                            RemoteView::Drone(d) => {
                                if let Ok((_, drone)) = drones.get(d) {
                                    intent.yaw = drone.yaw;
                                    intent.pitch = drone.pitch;
                                }
                            }
                            RemoteView::Camera(_) => {
                                intent.yaw = 0.0;
                                intent.pitch = 0.0;
                            }
                        }
                        events.write(GadgetEvent::ViewEntered { by: entity, view: next });
                    }
                }
            }
            None if button.tapped => {
                if !allowed {
                    events.write(GadgetEvent::Refused {
                        by: entity,
                        reason: if dead { "dead" } else { "recon can't be used right now" }.into(),
                    });
                    continue;
                }
                let target = match team {
                    Team::Attackers => own_drones.last().map(|(e, _)| RemoteView::Drone(*e)),
                    Team::Defenders => live_cameras.first().map(|(e, _)| RemoteView::Camera(*e)),
                };
                match target {
                    Some(target) => enter(&mut commands, &mut intent, target, &mut events),
                    None => {
                        let reason = match team {
                            Team::Attackers => "no drones left",
                            Team::Defenders => "no cameras left",
                        };
                        events.write(GadgetEvent::Refused { by: entity, reason: reason.into() });
                    }
                }
            }
            None => {}
        }
    }
}

/// A new round: everyone is back in their body.
fn end_views_on_reset(
    mut resets: MessageReader<RoundReset>,
    viewers: Query<Entity, With<RemoteView>>,
    mut events: MessageWriter<GadgetEvent>,
    mut commands: Commands,
) {
    if resets.read().last().is_none() {
        return;
    }
    for viewer in &viewers {
        commands.entity(viewer).remove::<(RemoteView, FrozenInput)>();
        events.write(GadgetEvent::ViewLeft { by: viewer, lost: false });
    }
}

/// A view that arrived from the server (a client's own character): freeze
/// the body here too, so the predicted character and the input sent to the
/// server stay put. (The server inserts `FrozenInput` with the view.)
fn freeze_networked_view(
    add: On<Add<RemoteView>>,
    viewers: Query<(&PlayerInput, Option<&WeaponInput>), Without<FrozenInput>>,
    mut commands: Commands,
) {
    if let Ok((input, weapon)) = viewers.get(add.entity) {
        commands.entity(add.entity).insert(FrozenInput {
            input: input.clone(),
            weapon: weapon.cloned(),
        });
    }
}

/// The view ended (removed by the server, or replicated away): unfreeze.
fn thaw_networked_view(remove: On<Remove<RemoteView>>, mut commands: Commands) {
    if let Ok(mut entity) = commands.get_entity(remove.entity) {
        entity.try_remove::<FrozenInput>();
    }
}

/// The body stands still while viewing: its intents stay as they were.
/// Also run by the server right after it applies a client's input
/// (`net/input.rs`), which would otherwise undo the freeze.
#[allow(clippy::type_complexity)]
pub(crate) fn freeze_viewers(
    mut viewers: Query<
        (
            &FrozenInput,
            &mut PlayerInput,
            Option<&mut WeaponInput>,
            Option<&mut DestructionIntent>,
            &mut GadgetIntent,
            Option<&mut RoundIntent>,
        ),
        With<RemoteView>,
    >,
) {
    for (frozen, mut input, weapon, destruction, mut gadget, round) in &mut viewers {
        *input = frozen.input.clone();
        input.move_axis = Vec2::ZERO;
        input.sprint = false;
        if let (Some(mut weapon), Some(saved)) = (weapon, &frozen.weapon) {
            *weapon = saved.clone();
            weapon.trigger = false;
            weapon.aim = false;
        }
        if let Some(mut destruction) = destruction {
            *destruction = DestructionIntent::default();
        }
        gadget.use_unique = false;
        gadget.unique_held = false;
        gadget.secondary_held = false;
        if let Some(mut round) = round {
            round.interact = false;
        }
    }
}

// ---- Presentation ---------------------------------------------------------

fn spawn_remote_camera(mut commands: Commands, tuning: Res<Tuning>) {
    commands
        .spawn((Name::new("Remote view rig"), RemoteViewRig, Transform::default(), Visibility::default()))
        .with_children(|rig| {
            rig.spawn((
                Name::new("Remote view camera"),
                RemoteViewCamera,
                Camera3d::default(),
                Camera {
                    // Below the player camera, so it never becomes the default
                    // UI camera by order; off until someone looks through it.
                    order: -1,
                    is_active: false,
                    ..default()
                },
                Projection::Perspective(PerspectiveProjection {
                    fov: tuning.round.gadgets.drone.fov.to_radians(),
                    near: 0.02,
                    ..default()
                }),
                Transform::default(),
            ));
        });
}

/// Puts the rig where the local player is looking from.
#[allow(clippy::type_complexity)]
fn place_remote_camera(
    local: Query<(&Transform, &CharacterState, Option<&RemoteView>, &DroneIntent), With<LocalPlayer>>,
    drones: Query<&Transform, (With<Drone>, Without<LocalPlayer>, Without<RemoteViewRig>)>,
    cctv: Query<(&Transform, &CctvCamera), (Without<LocalPlayer>, Without<RemoteViewRig>)>,
    mut rig: Single<&mut Transform, (With<RemoteViewRig>, Without<LocalPlayer>)>,
    mut camera: Single<&mut Projection, With<RemoteViewCamera>>,
    tuning: Res<Tuning>,
    mut listener: ResMut<Listener>,
) {
    let Ok((body, state, view, intent)) = local.single() else {
        return;
    };
    let g = &tuning.round.gadgets;
    let pose = match view {
        Some(RemoteView::Drone(d)) => drones.get(*d).ok().map(|t| {
            let rotation = Quat::from_euler(EulerRot::YXZ, intent.yaw, intent.pitch, 0.0);
            let heading = Quat::from_rotation_y(intent.yaw) * Vec3::NEG_Z;
            (t.translation + Vec3::Y * (g.drone.camera_height - g.drone.radius) + heading * 0.07, rotation, g.drone.fov)
        }),
        Some(RemoteView::Camera(c)) => cctv.get(*c).ok().map(|(t, cam)| {
            let rotation = cam.view(intent.yaw, intent.pitch, &tuning);
            (t.translation + rotation * Vec3::NEG_Z * 0.12, rotation, g.cctv.fov)
        }),
        None => None,
    };
    match pose {
        Some((at, rotation, fov)) => {
            rig.translation = at;
            rig.rotation = rotation;
            listener.position = at;
            if let Projection::Perspective(p) = camera.as_mut() {
                let fov = fov.to_radians();
                if p.fov != fov {
                    p.fov = fov;
                }
            }
        }
        None => {
            listener.position = body.translation + Vec3::Y * (state.height - tuning.body.eye_from_top);
        }
    }
}

/// Switch the world cameras off and the remote one on (and back).
fn switch_cameras(
    local: Query<Has<RemoteView>, With<LocalPlayer>>,
    mut cameras: Query<(Entity, &mut Camera, Has<RemoteViewCamera>)>,
    mut swap: ResMut<CameraSwap>,
    mut commands: Commands,
) {
    let viewing = local.single().unwrap_or(false);
    if viewing == swap.active {
        return;
    }
    swap.active = viewing;
    if viewing {
        swap.switched_off.clear();
        for (entity, mut camera, remote) in &mut cameras {
            if remote {
                camera.is_active = true;
                commands.entity(entity).insert(IsDefaultUiCamera);
            } else if camera.is_active {
                camera.is_active = false;
                swap.switched_off.push(entity);
            }
        }
    } else {
        for (entity, mut camera, remote) in &mut cameras {
            if remote {
                camera.is_active = false;
                commands.entity(entity).remove::<IsDefaultUiCamera>();
            } else if swap.switched_off.contains(&entity) {
                camera.is_active = true;
            }
        }
        swap.switched_off.clear();
    }
}

#[derive(Component)]
struct Overlay;
#[derive(Component)]
struct OverlayTint;
#[derive(Component)]
struct OverlayGrain;
#[derive(Component)]
struct OverlayInfo;
#[derive(Component)]
struct OverlayHint;
#[derive(Component)]
struct OverlayReticle;
#[derive(Component)]
struct OverlayRec;
#[derive(Component)]
struct StaticFlash;

/// Grain frames, cycled.
#[derive(Resource)]
struct GrainFrames(Vec<Handle<Image>>);

fn image(images: &mut Assets<Image>, size: u32, mut pixel: impl FnMut(u32, u32) -> [u8; 4]) -> Handle<Image> {
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            data.extend(pixel(x, y));
        }
    }
    images.add(Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    ))
}

fn spawn_overlay(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut seed = 0x2545_F491_u32;
    let mut random = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    let grain: Vec<Handle<Image>> = (0..4)
        .map(|_| {
            image(&mut images, 192, |_, _| {
                let v = random();
                let light = (v & 0xFF) as u8;
                let alpha = ((v >> 8) & 0x1F) as u8;
                [light, light, light, alpha]
            })
        })
        .collect();
    let scanlines = image(&mut images, 4, |_, y| if y == 0 { [0, 0, 0, 70] } else { [0, 0, 0, 0] });
    let vignette = image(&mut images, 256, |x, y| {
        let d = Vec2::new(x as f32 + 0.5, y as f32 + 0.5) / 128.0 - 1.0;
        let a = ((d.length() - 0.55) / 0.85).clamp(0.0, 1.0).powf(1.6);
        [0, 0, 0, (a * 190.0) as u8]
    });
    let full = || Node {
        position_type: PositionType::Absolute,
        width: percent(100.0),
        height: percent(100.0),
        ..default()
    };
    let font = |size: f32| TextFont {
        font_size: FontSize::Px(size),
        ..default()
    };
    let text = Color::srgba(0.85, 1.0, 0.92, 0.92);
    commands
        .spawn((
            Name::new("Remote view overlay"),
            Overlay,
            full(),
            // Under the HUD (score, timer, notices).
            GlobalZIndex(-5),
            Visibility::Hidden,
            Pickable::IGNORE,
        ))
        .with_children(|o| {
            o.spawn((OverlayTint, full(), BackgroundColor(Color::srgba(0.1, 0.35, 0.3, 0.16))));
            o.spawn((
                ImageNode {
                    image: vignette,
                    image_mode: NodeImageMode::Stretch,
                    ..default()
                },
                full(),
            ));
            o.spawn((
                ImageNode {
                    image: scanlines,
                    image_mode: NodeImageMode::Tiled {
                        tile_x: true,
                        tile_y: true,
                        stretch_value: 1.0,
                    },
                    ..default()
                },
                full(),
            ));
            o.spawn((
                OverlayGrain,
                ImageNode {
                    image: grain[0].clone(),
                    image_mode: NodeImageMode::Tiled {
                        tile_x: true,
                        tile_y: true,
                        stretch_value: 1.0,
                    },
                    color: Color::srgba(1.0, 1.0, 1.0, 0.55),
                    ..default()
                },
                full(),
            ));
            // Reticle brackets (drones).
            o.spawn((
                OverlayReticle,
                full(),
                Visibility::Hidden,
            ))
            .with_children(|r| {
                for (dx, dy, w, h) in [
                    (-40.0, -30.0, 14.0, 2.0),
                    (-40.0, -30.0, 2.0, 14.0),
                    (26.0, -30.0, 14.0, 2.0),
                    (38.0, -30.0, 2.0, 14.0),
                    (-40.0, 28.0, 14.0, 2.0),
                    (-40.0, 16.0, 2.0, 14.0),
                    (26.0, 28.0, 14.0, 2.0),
                    (38.0, 16.0, 2.0, 14.0),
                ] {
                    r.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Percent(50.0),
                            top: Val::Percent(50.0),
                            margin: UiRect {
                                left: px(dx),
                                top: px(dy),
                                ..default()
                            },
                            width: px(w),
                            height: px(h),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.8, 1.0, 0.9, 0.7)),
                    ));
                }
            });
            o.spawn((
                OverlayInfo,
                Text::new(""),
                font(17.0),
                TextColor(text),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(22.0),
                    top: px(84.0),
                    padding: UiRect::axes(px(8.0), px(4.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
            ));
            o.spawn((
                OverlayRec,
                Text::new("REC"),
                font(16.0),
                TextColor(Color::srgb(1.0, 0.25, 0.2)),
                Node {
                    position_type: PositionType::Absolute,
                    right: px(26.0),
                    top: px(84.0),
                    ..default()
                },
                Visibility::Hidden,
            ));
            o.spawn((
                OverlayHint,
                Text::new(""),
                font(14.0),
                TextColor(Color::srgba(0.85, 1.0, 0.92, 0.75)),
                Node {
                    position_type: PositionType::Absolute,
                    bottom: px(150.0),
                    width: percent(100.0),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                TextLayout::justify(Justify::Center),
            ));
        });
    // A burst of static when a feed is lost.
    commands.spawn((
        Name::new("Static flash"),
        StaticFlash,
        ImageNode {
            image: grain[1].clone(),
            image_mode: NodeImageMode::Tiled {
                tile_x: true,
                tile_y: true,
                stretch_value: 1.0,
            },
            color: Color::srgba(1.0, 1.0, 1.0, 0.0),
            ..default()
        },
        full(),
        GlobalZIndex(90),
        Pickable::IGNORE,
        children![(
            Text::new("SIGNAL LOST"),
            font(30.0),
            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.0)),
            Node {
                position_type: PositionType::Absolute,
                top: percent(40.0),
                width: percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            TextLayout::justify(Justify::Center),
        )],
    ));
    commands.insert_resource(GrainFrames(grain));
}

/// Signal bars for a distance.
fn bars(distance: f32, range: f32) -> String {
    let n = (4.0 * (1.0 - distance / range.max(1.0))).ceil().clamp(0.0, 4.0) as usize;
    format!("[{}{}]", "|".repeat(n), ".".repeat(4 - n))
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update_overlay(
    local: Query<(Entity, &Transform, Option<&RemoteView>, Option<&ReconStock>), With<LocalPlayer>>,
    drones: Query<(&Drone, &Transform), Without<LocalPlayer>>,
    cameras: Query<&CctvCamera>,
    all_cameras: Query<(), With<CctvCamera>>,
    grain_frames: Option<Res<GrainFrames>>,
    tuning: Res<Tuning>,
    time: Res<Time>,
    mut events: MessageReader<GadgetEvent>,
    (mut overlay, mut tint, mut grain): (
        Single<&mut Visibility, (With<Overlay>, Without<OverlayReticle>, Without<OverlayRec>)>,
        Single<&mut BackgroundColor, With<OverlayTint>>,
        Single<&mut ImageNode, (With<OverlayGrain>, Without<StaticFlash>)>,
    ),
    (mut info, mut hint): (
        Single<&mut Text, (With<OverlayInfo>, Without<OverlayHint>)>,
        Single<&mut Text, (With<OverlayHint>, Without<OverlayInfo>)>,
    ),
    (mut reticle, mut rec): (
        Single<&mut Visibility, (With<OverlayReticle>, Without<Overlay>, Without<OverlayRec>)>,
        Single<&mut Visibility, (With<OverlayRec>, Without<Overlay>, Without<OverlayReticle>)>,
    ),
    (mut flash, mut flash_text): (
        Single<(&mut ImageNode, &Children), (With<StaticFlash>, Without<OverlayGrain>)>,
        Query<&mut TextColor>,
    ),
    mut lost_at: Local<f32>,
) {
    let now = time.elapsed_secs();
    let Ok((me, body, view, stock)) = local.single() else {
        return;
    };
    for event in events.read() {
        if let GadgetEvent::ViewLeft { by, lost: true } = event
            && *by == me
        {
            *lost_at = now;
        }
    }
    // Static burst after a lost feed.
    let since = now - *lost_at;
    let burst = if *lost_at > 0.0 && since < 0.9 { (1.0 - since / 0.9).powf(0.5) } else { 0.0 };
    let (flash_image, flash_children) = &mut *flash;
    flash_image.color = Color::srgba(1.0, 1.0, 1.0, burst);
    if let Some(frames) = &grain_frames {
        flash_image.image = frames.0[((now * 24.0) as usize + 2) % frames.0.len()].clone();
    }
    for child in flash_children.iter() {
        if let Ok(mut color) = flash_text.get_mut(child) {
            color.0 = Color::srgba(1.0, 1.0, 1.0, burst);
        }
    }

    let Some(view) = view else {
        overlay.set_if_neq(Visibility::Hidden);
        return;
    };
    overlay.set_if_neq(Visibility::Inherited);
    if let Some(frames) = &grain_frames {
        grain.image = frames.0[(now * 20.0) as usize % frames.0.len()].clone();
    }
    let clock = {
        let s = (now + 13.0 * 3600.0) as u32;
        format!("{:02}:{:02}:{:02}", (s / 3600) % 24, (s / 60) % 60, s % 60)
    };
    let (text, help, tint_color, is_drone) = match view {
        RemoteView::Drone(d) => match drones.get(*d) {
            Ok((drone, at)) => {
                let battery = (drone.battery / tuning.round.gadgets.drone.battery.max(1.0) * 100.0).round();
                let distance = at.translation.distance(body.translation);
                let left = stock.map_or(0, |s| s.drones);
                (
                    format!(
                        "DRONE {}   BATTERY {battery:.0}%   SIGNAL {}\n{distance:.0} m from you   {left} in reserve",
                        drone.number,
                        bars(distance, tuning.round.gadgets.drone.signal_range)
                    ),
                    "W/S drive   A/D, mouse turn   Q/E switch drone   hold G throw another   G exit",
                    Color::srgba(0.1, 0.35, 0.3, 0.10),
                    true,
                )
            }
            Err(_) => ("NO SIGNAL".into(), "", Color::srgba(0.2, 0.2, 0.2, 0.4), true),
        },
        RemoteView::Camera(c) => match cameras.get(*c) {
            Ok(cam) => (
                format!(
                    "CAM {}/{}  {}\n{clock}",
                    cam.index + 1,
                    all_cameras.iter().count(),
                    cam.name
                ),
                "mouse, A/D pan   Q/E switch camera   G exit",
                Color::srgba(0.18, 0.22, 0.3, 0.08),
                false,
            ),
            Err(_) => ("NO SIGNAL".into(), "", Color::srgba(0.2, 0.2, 0.2, 0.4), false),
        },
    };
    if info.0 != text {
        info.0 = text;
    }
    if hint.0 != help {
        hint.0 = help.to_string();
    }
    if tint.0 != tint_color {
        tint.0 = tint_color;
    }
    reticle.set_if_neq(if is_drone { Visibility::Inherited } else { Visibility::Hidden });
    let blink = (now * 1.5).fract() < 0.6;
    rec.set_if_neq(if !is_drone && blink { Visibility::Inherited } else { Visibility::Hidden });
}
