//! Developer tools: FPS overlay and screenshots.
//!
//! - `F3` toggles the FPS overlay.
//! - `F12` saves a screenshot to `screenshots/`.
//! - `--screenshot <path.png>` on the command line saves one screenshot after
//!   the scene has settled, then quits. Used for automated visual checks.
//!   `--screenshot-frames <n>` changes how many frames it waits (default 120).
//! - `--spawn x,y,z,yaw,pitch` spawns the player there (angles in degrees).
//! - `--auto-move strafe,forward[,sprint]` holds a movement input.

use std::path::PathBuf;

use bevy::{
    dev_tools::fps_overlay::{FpsOverlayConfig, FpsOverlayPlugin},
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
};

use crate::player::{CursorGrabDisabled, LocalPlayer, PlayerInput, SpawnPoint};

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FpsOverlayPlugin::default())
            .add_systems(Update, (toggle_fps_overlay, screenshot_on_key));

        if let Some(auto) = AutoScreenshot::from_args() {
            // Automated run: don't steal the user's mouse.
            app.insert_resource(auto)
                .insert_resource(CursorGrabDisabled)
                .add_systems(Update, auto_screenshot.run_if(resource_exists::<AutoScreenshot>));
        }
        if let Some(spawn) = spawn_from_args() {
            app.insert_resource(spawn);
        }
        if let Some(auto_move) = AutoMove::from_args() {
            app.insert_resource(auto_move)
                .add_systems(FixedPreUpdate, apply_auto_move);
        }
        if std::env::args().any(|a| a == "--trace-player") {
            app.add_systems(FixedPostUpdate, trace_player);
        }
    }
}

/// `--trace-player`: log the local player's feet position 4 times a second.
fn trace_player(
    players: Query<&Transform, With<LocalPlayer>>,
    mut ticks: Local<u32>,
) {
    *ticks += 1;
    if *ticks % 16 != 0 {
        return;
    }
    for transform in &players {
        info!("tick {:>5}: feet at {:.2}", *ticks, transform.translation);
    }
}

/// `--auto-move strafe,forward[,sprint]`: hold a movement input for automated
/// tests, ignoring the keyboard. Example: `--auto-move 0,1,sprint`.
#[derive(Resource)]
struct AutoMove {
    axis: Vec2,
    sprint: bool,
}

impl AutoMove {
    fn from_args() -> Option<Self> {
        let raw = arg_value("--auto-move")?;
        let parts: Vec<&str> = raw.split(',').map(str::trim).collect();
        let (Some(x), Some(y)) = (
            parts.first().and_then(|s| s.parse().ok()),
            parts.get(1).and_then(|s| s.parse().ok()),
        ) else {
            warn!("--auto-move expects strafe,forward[,sprint], got {raw:?}");
            return None;
        };
        Some(Self {
            axis: Vec2::new(x, y),
            sprint: parts.contains(&"sprint"),
        })
    }
}

fn apply_auto_move(
    auto: Res<AutoMove>,
    mut players: Query<&mut PlayerInput, With<LocalPlayer>>,
) {
    for mut input in &mut players {
        input.move_axis = auto.axis;
        input.sprint = auto.sprint;
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

fn toggle_fps_overlay(keys: Res<ButtonInput<KeyCode>>, mut config: ResMut<FpsOverlayConfig>) {
    if keys.just_pressed(KeyCode::F3) {
        config.enabled = !config.enabled;
    }
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
    players: Query<&Transform, With<LocalPlayer>>,
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
    for transform in &players {
        info!("local player feet at {:.2}", transform.translation);
    }
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(shot.path.clone()))
        .observe(|_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
            exit.write(AppExit::Success);
        });
}
