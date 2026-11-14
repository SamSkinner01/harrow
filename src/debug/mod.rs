//! Developer tools: FPS overlay and screenshots.
//!
//! - `F3` toggles the FPS overlay.
//! - `F12` saves a screenshot to `screenshots/`.
//! - `--screenshot <path.png>` on the command line saves one screenshot after
//!   the scene has settled, then quits. Used for automated visual checks.
//!   `--screenshot-frames <n>` changes how many frames it waits (default 120).

use std::path::PathBuf;

use bevy::{
    dev_tools::fps_overlay::{FpsOverlayConfig, FpsOverlayPlugin},
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
};

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FpsOverlayPlugin::default())
            .add_systems(Update, (toggle_fps_overlay, screenshot_on_key));

        if let Some(auto) = AutoScreenshot::from_args() {
            app.insert_resource(auto)
                .add_systems(Update, auto_screenshot.run_if(resource_exists::<AutoScreenshot>));
        }
    }
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
        let args: Vec<String> = std::env::args().collect();
        let value_after = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
                .cloned()
        };
        let path = value_after("--screenshot")?;
        let wait_frames = value_after("--screenshot-frames")
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

fn auto_screenshot(mut commands: Commands, mut shot: ResMut<AutoScreenshot>) {
    if shot.requested {
        return;
    }
    shot.frames_seen += 1;
    if shot.frames_seen < shot.wait_frames {
        return;
    }
    shot.requested = true;
    info!("auto screenshot -> {}", shot.path.display());
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(shot.path.clone()))
        .observe(|_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
            exit.write(AppExit::Success);
        });
}
