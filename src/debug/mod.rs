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

use std::path::PathBuf;

use bevy::{
    color::palettes::css,
    dev_tools::fps_overlay::{FpsOverlayConfig, FpsOverlayPlugin},
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
};

use crate::{
    config::Tuning,
    player::{
        CharacterState, CursorGrabDisabled, HitZone, Lean, LocalPlayer, Player, PlayerInput,
        SpawnPoint, Stance,
    },
};

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        let has_flag = |flag: &str| std::env::args().any(|a| a == flag);
        app.add_plugins(FpsOverlayPlugin::default())
            .insert_resource(DebugView {
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
                .insert_resource(CursorGrabDisabled)
                .add_systems(Update, auto_screenshot.run_if(resource_exists::<AutoScreenshot>));
        }
        if let Some(spawn) = spawn_from_args() {
            app.insert_resource(spawn);
        }
        let forced = ForcedInput::from_args();
        if forced.is_active() {
            app.insert_resource(forced)
                .add_systems(FixedPreUpdate, apply_forced_input);
        }
        if has_flag("--trace-player") {
            app.add_systems(FixedPostUpdate, trace_player);
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
    mut fps: ResMut<FpsOverlayConfig>,
    mut view: ResMut<DebugView>,
) {
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
    requested: bool,
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
            requested: false,
        })
    }
}

fn auto_screenshot(
    mut commands: Commands,
    mut shot: ResMut<AutoScreenshot>,
    players: Query<(&Transform, &CharacterState), With<LocalPlayer>>,
) {
    if shot.requested {
        return;
    }
    shot.frames_seen += 1;
    if shot.frames_seen < shot.wait_frames {
        return;
    }
    shot.requested = true;
    info!("auto screenshot -> {}", shot.path.display());
    for (transform, state) in &players {
        info!(
            "local player feet at {:.2} height {:.2} lean {:+.2} {:?}",
            transform.translation, state.height, state.lean, state.stance
        );
    }
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(shot.path.clone()))
        .observe(|_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
            exit.write(AppExit::Success);
        });
}
