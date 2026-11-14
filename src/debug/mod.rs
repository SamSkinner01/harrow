//! Developer tools.
//!
//! Keys:
//! - `F3` FPS overlay, `F4` show hit zones and movement capsule,
//!   `F5` third-person view (also shows hit zones), `F12` screenshot to
//!   `screenshots/`.
//!
//! Command-line flags (for automated checks; combine freely):
//! - `--screenshot <path.png>`: save one screenshot after the scene settles,
//!   then quit. `--screenshot-frames <n>` sets the wait (default 120).
//!   These runs don't grab the mouse.
//! - `--spawn x,y,z[,yaw[,pitch]]`: spawn point, angles in degrees.
//! - `--auto-move strafe,forward[,sprint]`: hold a movement input.
//! - `--stance stand|crouch|prone`, `--lean left|right`: hold a stance/lean.
//! - `--third-person`, `--hitboxes`: start with those views on.
//! - `--trace-player`: log the player's feet position 4 times a second.
//! - `--look yaw[,pitch]`: the local player's starting look, degrees (also
//!   for multiplayer clients, whose spawn the server picks).
//! - `--auto-fire <seconds>`: hold the trigger from that long after the
//!   local player appears.
//! - `--screenshot-secs <s>`: take the `--screenshot` after that many real
//!   seconds instead of a frame count (unfocused windows run slower).
//! - `--exit-after <seconds>`: quit after that long; with it, `--screenshot`
//!   no longer quits.
//! - `--no-vsync`: uncapped frame rate, for performance measurements.
//! - `--map keep|greybox` is handled by the map module.

use std::path::PathBuf;

// The FPS overlay comes with Bevy's dev tools, which pull in audio: client
// build only.
#[cfg(feature = "client")]
use bevy::dev_tools::fps_overlay::{FpsOverlayConfig, FpsOverlayPlugin};
use bevy::{
    color::palettes::css,
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
};

use crate::{
    config::Tuning,
    player::{
        AutomatedRun, CharacterState, Footstep, HitZone, Lean, LocalPlayer, Player, PlayerInput,
        SpawnPoint, Stance,
    },
};

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        let has_flag = |flag: &str| std::env::args().any(|a| a == flag);
        #[cfg(feature = "client")]
        app.add_plugins(FpsOverlayPlugin::default());
        app.insert_resource(DebugView {
                hitboxes: has_flag("--hitboxes"),
                third_person: has_flag("--third-person"),
            })
            .add_systems(
                Update,
                (debug_keys, screenshot_on_key, draw_character_gizmos),
            );

        if let Some(auto) = AutoScreenshot::from_args() {
            // Automated run: don't steal the user's mouse.
            app.insert_resource(auto)
                .insert_resource(AutomatedRun)
                .add_systems(Update, auto_screenshot.run_if(resource_exists::<AutoScreenshot>));
        }
        if let Some(spawn) = spawn_from_args() {
            app.insert_resource(spawn);
        }
        let forced = ForcedInput::from_args();
        if forced.is_active() {
            app.insert_resource(forced)
                // Before a client puts its input on the wire.
                .add_systems(
                    FixedPreUpdate,
                    apply_forced_input.before(crate::net::input::CaptureInputSystems),
                );
        }
        if has_flag("--no-vsync") {
            app.add_systems(Startup, disable_vsync);
        }
        if has_flag("--trace-player") {
            app.add_systems(FixedPostUpdate, trace_player)
                .add_systems(Update, trace_footsteps);
        }
        if let Some(seconds) = arg_value("--auto-fire").and_then(|s| s.parse::<f32>().ok()) {
            // Hold the trigger from `seconds` after the local player appears
            // (also for multiplayer clients: the input goes to the server).
            app.add_systems(
                FixedPreUpdate,
                (move |mut players: Query<(&mut crate::weapons::WeaponInput, &mut AutoFireClock), With<LocalPlayer>>,
                       time: Res<Time>| {
                    for (mut weapon, mut clock) in &mut players {
                        clock.0 += time.delta_secs();
                        if clock.0 >= seconds && !weapon.trigger {
                            weapon.trigger = true;
                            weapon.trigger_presses = weapon.trigger_presses.wrapping_add(1);
                            if clock.0 < seconds + 0.1 {
                                info!("--auto-fire: trigger held");
                            }
                        }
                    }
                })
                .before(crate::net::input::CaptureInputSystems),
            )
            .add_observer(|add: On<Add<LocalPlayer>>, mut commands: Commands| {
                commands.entity(add.entity).insert(AutoFireClock(0.0));
            });
        }
        if let Some(seconds) = arg_value("--exit-after").and_then(|s| s.parse::<f32>().ok()) {
            app.insert_resource(ExitAfter(seconds))
                .add_systems(Update, exit_after);
        }
        if let Some(look) = look_from_args() {
            // Also works for a networked client, whose character arrives
            // from the server (multiplayer tests).
            app.add_observer(
                move |add: On<Add<LocalPlayer>>, mut inputs: Query<&mut PlayerInput>| {
                    if let Ok(mut input) = inputs.get_mut(add.entity) {
                        input.yaw = look.x;
                        input.pitch = look.y;
                    }
                },
            );
        }
    }
}

/// `--look yaw[,pitch]` (degrees): the local player's starting look.
fn look_from_args() -> Option<Vec2> {
    let raw = arg_value("--look")?;
    let n: Vec<f32> = raw.split(',').filter_map(|s| s.trim().parse().ok()).collect();
    let yaw = |y: f32| y.to_radians().rem_euclid(std::f32::consts::TAU);
    match n.as_slice() {
        [y] => Some(Vec2::new(yaw(*y), 0.0)),
        [y, pitch, ..] => Some(Vec2::new(yaw(*y), pitch.to_radians())),
        _ => {
            warn!("--look expects yaw[,pitch] in degrees, got {raw:?}");
            None
        }
    }
}

/// Debug view toggles.
#[derive(Resource, Debug)]
pub struct DebugView {
    pub hitboxes: bool,
    pub third_person: bool,
}

fn debug_keys(
    keys: Res<ButtonInput<KeyCode>>,
    #[cfg(feature = "client")] mut fps: ResMut<FpsOverlayConfig>,
    mut view: ResMut<DebugView>,
) {
    #[cfg(feature = "client")]
    if keys.just_pressed(KeyCode::F3) {
        fps.enabled = !fps.enabled;
    }
    if keys.just_pressed(KeyCode::F4) {
        view.hitboxes = !view.hitboxes;
    }
    if keys.just_pressed(KeyCode::F5) {
        view.third_person = !view.third_person;
    }
}

/// Wireframes for hit zones (red head, orange torso, yellow legs) and the
/// movement capsule (cyan).
fn draw_character_gizmos(
    view: Res<DebugView>,
    tuning: Res<Tuning>,
    zones: Query<(&HitZone, &GlobalTransform)>,
    bodies: Query<(&Transform, &CharacterState), With<Player>>,
    mut gizmos: Gizmos,
) {
    if !(view.hitboxes || view.third_person) {
        return;
    }
    for (zone, global) in &zones {
        let (_, rotation, translation) = global.to_scale_rotation_translation();
        let (radius, length) = zone.shape();
        let color = match zone {
            HitZone::Head => css::RED,
            HitZone::Torso => css::ORANGE,
            HitZone::Legs => css::YELLOW,
        };
        gizmos.primitive_3d(
            &Capsule3d::new(radius, length),
            Isometry3d::new(translation, rotation),
            color,
        );
    }
    let radius = tuning.body.radius;
    for (transform, state) in &bodies {
        let center = transform.translation + Vec3::Y * state.height / 2.0;
        gizmos.primitive_3d(
            &Capsule3d::new(radius, (state.height - 2.0 * radius).max(0.0)),
            Isometry3d::from_translation(center),
            css::AQUA,
        );
    }
}

/// `--trace-player`: log the local player's feet position 4 times a second.
fn trace_player(
    players: Query<(&Transform, &CharacterState), With<LocalPlayer>>,
    mut ticks: Local<u32>,
) {
    *ticks += 1;
    if *ticks % 16 != 0 {
        return;
    }
    for (transform, state) in &players {
        info!(
            "tick {:>5}: feet at {:.2} height {:.2} lean {:+.2} {:?}",
            *ticks, transform.translation, state.height, state.lean, state.stance
        );
    }
}

/// `--no-vsync`: render as fast as possible, for measuring performance.
fn disable_vsync(mut window: Single<&mut Window>) {
    window.present_mode = bevy::window::PresentMode::AutoNoVsync;
}

/// `--trace-player`: log every footstep.
fn trace_footsteps(mut steps: MessageReader<Footstep>) {
    for step in steps.read() {
        info!("footstep at {:.2} loudness {:.2}", step.position, step.loudness);
    }
}

/// Inputs held by command-line flags, overriding the keyboard.
#[derive(Resource, Default)]
struct ForcedInput {
    movement: Option<(Vec2, bool)>,
    stance: Option<Stance>,
    lean: Option<Lean>,
}

impl ForcedInput {
    fn from_args() -> Self {
        let movement = arg_value("--auto-move").and_then(|raw| {
            let parts: Vec<&str> = raw.split(',').map(str::trim).collect();
            let x = parts.first().and_then(|s| s.parse().ok());
            let y = parts.get(1).and_then(|s| s.parse().ok());
            if x.is_none() || y.is_none() {
                warn!("--auto-move expects strafe,forward[,sprint], got {raw:?}");
            }
            Some((Vec2::new(x?, y?), parts.contains(&"sprint")))
        });
        let stance = arg_value("--stance").and_then(|s| match s.as_str() {
            "stand" => Some(Stance::Stand),
            "crouch" => Some(Stance::Crouch),
            "prone" => Some(Stance::Prone),
            other => {
                warn!("--stance expects stand|crouch|prone, got {other:?}");
                None
            }
        });
        let lean = arg_value("--lean").and_then(|s| match s.as_str() {
            "left" => Some(Lean::Left),
            "right" => Some(Lean::Right),
            other => {
                warn!("--lean expects left|right, got {other:?}");
                None
            }
        });
        Self {
            movement,
            stance,
            lean,
        }
    }

    fn is_active(&self) -> bool {
        self.movement.is_some() || self.stance.is_some() || self.lean.is_some()
    }
}

fn apply_forced_input(
    forced: Res<ForcedInput>,
    mut players: Query<&mut PlayerInput, With<LocalPlayer>>,
) {
    for mut input in &mut players {
        if let Some((axis, sprint)) = forced.movement {
            input.move_axis = axis;
            input.sprint = sprint;
        }
        if let Some(stance) = forced.stance {
            input.stance = stance;
        }
        if let Some(lean) = forced.lean {
            input.lean = lean;
        }
    }
}

/// The value following `flag` on the command line, if present.
fn arg_value(flag: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn spawn_from_args() -> Option<SpawnPoint> {
    let raw = arg_value("--spawn")?;
    let n: Vec<f32> = raw.split(',').filter_map(|s| s.trim().parse().ok()).collect();
    let [x, y, z, rest @ ..] = n.as_slice() else {
        warn!("--spawn expects x,y,z[,yaw[,pitch]], got {raw:?}");
        return None;
    };
    Some(SpawnPoint {
        position: Vec3::new(*x, *y, *z),
        yaw: rest.first().copied().unwrap_or(0.0).to_radians(),
        pitch: rest.get(1).copied().unwrap_or(0.0).to_radians(),
    })
}

fn screenshot_on_key(mut commands: Commands, keys: Res<ButtonInput<KeyCode>>) {
    if !keys.just_pressed(KeyCode::F12) {
        return;
    }
    let dir = PathBuf::from("screenshots");
    if let Err(err) = std::fs::create_dir_all(&dir) {
        error!("could not create {}: {err}", dir.display());
        return;
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let path = dir.join(format!("harrow-{stamp}.png"));
    info!("saving screenshot to {}", path.display());
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
}

/// One-shot screenshot requested on the command line.
#[derive(Resource)]
struct AutoScreenshot {
    path: PathBuf,
    wait_frames: u32,
    frames_seen: u32,
    /// `--screenshot-secs`: wait this long (real time) instead of frames.
    wait_secs: Option<f32>,
    requested: bool,
    /// Quit once it's saved (not with `--exit-after`).
    quit: bool,
}

impl AutoScreenshot {
    fn from_args() -> Option<Self> {
        let path = arg_value("--screenshot")?;
        let wait_frames = arg_value("--screenshot-frames")
            .and_then(|n| n.parse().ok())
            .unwrap_or(120);
        Some(Self {
            path: path.into(),
            wait_frames,
            frames_seen: 0,
            wait_secs: arg_value("--screenshot-secs").and_then(|s| s.parse().ok()),
            requested: false,
            quit: arg_value("--exit-after").is_none(),
        })
    }
}

fn auto_screenshot(
    mut commands: Commands,
    mut shot: ResMut<AutoScreenshot>,
    players: Query<(&Transform, &CharacterState), With<LocalPlayer>>,
    diagnostics: Res<DiagnosticsStore>,
    time: Res<Time<Real>>,
) {
    if shot.requested {
        return;
    }
    shot.frames_seen += 1;
    let waiting = match shot.wait_secs {
        Some(secs) => time.elapsed_secs() < secs,
        None => shot.frames_seen < shot.wait_frames,
    };
    if waiting {
        return;
    }
    shot.requested = true;
    info!("auto screenshot -> {}", shot.path.display());
    if let Some(fps) = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.average())
    {
        info!("average fps {fps:.1}");
    }
    for (transform, state) in &players {
        info!(
            "local player feet at {:.2} height {:.2} lean {:+.2} {:?}",
            transform.translation, state.height, state.lean, state.stance
        );
    }
    let mut screenshot = commands.spawn(Screenshot::primary_window());
    screenshot.observe(save_to_disk(shot.path.clone()));
    if shot.quit {
        screenshot.observe(|_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
            exit.write(AppExit::Success);
        });
    }
}

/// `--exit-after <seconds>`: quit after that long (multiplayer tests, so a
/// client can take its screenshot and stay connected a while longer).
fn exit_after(seconds: Res<ExitAfter>, time: Res<Time<Real>>, mut exit: MessageWriter<AppExit>, mut done: Local<bool>) {
    if !*done && time.elapsed_secs() >= seconds.0 {
        *done = true;
        info!("--exit-after: {} s, quitting", seconds.0);
        exit.write(AppExit::Success);
    }
}

#[derive(Resource)]
struct ExitAfter(f32);

/// `--auto-fire`: seconds since the local player appeared.
#[derive(Component)]
struct AutoFireClock(f32);
