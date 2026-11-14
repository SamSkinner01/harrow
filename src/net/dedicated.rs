//! Dedicated server: headless app setup and the server config file.
//!
//! `harrow --server [port] [--config server.ron]` runs without a window,
//! GPU, audio or input devices: Bevy's render plugin starts with no
//! graphics backend (no render world; meshes still load, because the map's
//! colliders are built from them), the window, audio and gamepad plugins
//! are left out, and a plain loop runs the app at the tick rate. Logs go to
//! stdout without colours when stdout isn't a terminal (journald), and
//! SIGTERM/SIGINT exit cleanly.
//!
//! The lean build (`--no-default-features --features dedicated`) doesn't
//! even link the window system, audio or gamepad libraries.
//!
//! Config file (RON), every field optional; see `assets/config/server.ron`:
//! name, port, map, max_players, log_level, round_loop, and round overrides
//! (test_timers, rounds_to_win, timers).

use std::{io::IsTerminal, path::Path, time::Duration};

use bevy::{
    app::ScheduleRunnerPlugin,
    log::{BoxedFmtLayer, Level, LogPlugin, tracing_subscriber},
    prelude::*,
    render::{
        RenderPlugin,
        settings::{RenderCreation, WgpuSettings},
    },
    window::ExitCondition,
};
use serde::Deserialize;

use crate::{
    config::{RoundTimers, Tuning, TuningSystems},
    physics::TICK_HZ,
    round::RoundSettings,
    session::{DEFAULT_PORT, Launch},
};

/// Settings for a dedicated server (`--config path`).
#[derive(Resource, Deserialize, Debug, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    /// Shown to players when they join.
    pub name: String,
    /// UDP port (`--server <port>` overrides it).
    pub port: u16,
    /// `keep` or `greybox`.
    pub map: String,
    pub max_players: u32,
    /// `error`, `warn`, `info`, `debug` or `trace`.
    pub log_level: String,
    /// Play rounds (only on the keep). Off: free play.
    pub round_loop: bool,
    pub round: RoundOverrides,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct RoundOverrides {
    /// Use the short test timers (`round.test_timers` in the tuning file).
    pub test_timers: bool,
    pub rounds_to_win: Option<u32>,
    /// Replace the round timers (all of them).
    pub timers: Option<RoundTimers>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            name: "Harrow server".into(),
            port: DEFAULT_PORT,
            map: "keep".into(),
            max_players: 10,
            log_level: "info".into(),
            round_loop: true,
            round: RoundOverrides::default(),
        }
    }
}

impl ServerConfig {
    /// The config for this launch: `--config` file (or defaults), with the
    /// command line's `--server <port>`, `--map`, `--no-round` and
    /// `--round-test` on top. Errors are returned for logging later.
    pub fn for_launch(launch: &Launch) -> (Self, Option<String>) {
        let (mut config, error) = match &launch.config {
            Some(path) => match Self::load(path) {
                Ok(config) => (config, None),
                Err(err) => (Self::default(), Some(err)),
            },
            None => (Self::default(), None),
        };
        let args: Vec<String> = std::env::args().collect();
        let value = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
                .filter(|v| !v.starts_with("--"))
                .cloned()
        };
        if let Some(port) = value("--server").and_then(|p| p.parse().ok()) {
            config.port = port;
        }
        if let Some(map) = value("--map") {
            config.map = map;
        }
        if args.iter().any(|a| a == "--no-round") {
            config.round_loop = false;
        }
        if args.iter().any(|a| a == "--round-test") {
            config.round.test_timers = true;
        }
        // Bomb sites exist on the keep and section A, not the grey-box.
        if !crate::map::MapChoice::from_name(&config.map).has_objectives() {
            config.round_loop = false;
        }
        (config, error)
    }

    fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|err| format!("can't read server config {}: {err}", path.display()))?;
        ron::from_str(&text)
            .map_err(|err| format!("server config {} is invalid: {err}", path.display()))
    }

    fn level(&self) -> Level {
        match self.log_level.to_ascii_lowercase().as_str() {
            "error" => Level::ERROR,
            "warn" | "warning" => Level::WARN,
            "debug" => Level::DEBUG,
            "trace" => Level::TRACE,
            _ => Level::INFO,
        }
    }
}

/// Bevy's plugins for a headless server.
pub fn headless_plugins(config: &ServerConfig) -> impl PluginGroup {
    let plugins = DefaultPlugins
        .set(WindowPlugin {
            primary_window: None,
            exit_condition: ExitCondition::DontExit,
            ..default()
        })
        .set(RenderPlugin {
            // No graphics backend: no GPU needed, no render world.
            render_creation: RenderCreation::Automatic(Box::new(WgpuSettings {
                backends: None,
                ..default()
            })),
            ..default()
        })
        .set(LogPlugin {
            level: config.level(),
            fmt_layer: plain_stdout_logs,
            ..default()
        })
        .add(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
            1.0 / TICK_HZ,
        )));
    #[cfg(feature = "client")]
    let plugins = plugins
        .disable::<bevy::winit::WinitPlugin>()
        .disable::<bevy::audio::AudioPlugin>()
        .disable::<bevy::gilrs::GilrsPlugin>();
    plugins
}

/// A server has no ears: sounds started by presentation code (client build
/// only) are removed instead of piling up.
#[cfg(feature = "client")]
pub fn drop_sounds(sounds: Query<Entity, With<bevy::audio::AudioPlayer>>, mut commands: Commands) {
    for sound in &sounds {
        commands.entity(sound).despawn();
    }
}

/// Log lines to stdout; colours only on a terminal (journald gets plain
/// text).
fn plain_stdout_logs(_app: &mut App) -> Option<BoxedFmtLayer> {
    Some(Box::new(
        tracing_subscriber::fmt::Layer::default()
            .with_writer(std::io::stdout)
            .with_ansi(std::io::stdout().is_terminal()),
    ))
}

/// Applies the config's round settings (once, and again after every tuning
/// reload so a hot-reloaded tuning file doesn't undo them).
pub struct ServerConfigPlugin {
    pub config: ServerConfig,
    pub error: Option<String>,
}

impl Plugin for ServerConfigPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.config.clone())
            .add_systems(Startup, apply_round_settings)
            .add_systems(PreUpdate, apply_round_tuning.after(TuningSystems));
        if let Some(err) = self.error.clone() {
            app.add_systems(Startup, move || error!("{err}; using defaults"));
        }
        let config = self.config.clone();
        app.add_systems(Startup, move || {
            info!(
                "dedicated server \"{}\": map {}, UDP port {}, up to {} players, rounds {}{}",
                config.name,
                config.map,
                config.port,
                config.max_players,
                if config.round_loop {
                    "on"
                } else {
                    "off (free play)"
                },
                if config.round.test_timers {
                    " (test timers)"
                } else {
                    ""
                },
            )
        });
    }
}

fn apply_round_settings(config: Res<ServerConfig>, mut settings: ResMut<RoundSettings>) {
    settings.test = config.round.test_timers;
    // The round loop starts when someone joins (`server.rs`).
    settings.enabled = false;
}

fn apply_round_tuning(
    config: Res<ServerConfig>,
    mut tuning: ResMut<Tuning>,
    mut done: Local<bool>,
) {
    if *done && !tuning.is_changed() {
        return;
    }
    *done = true;
    let tuning = tuning.bypass_change_detection();
    if let Some(n) = config.round.rounds_to_win {
        tuning.round.rounds_to_win = n;
        tuning.round.test_rounds_to_win = n;
    }
    if let Some(timers) = &config.round.timers {
        tuning.round.timers = timers.clone();
        tuning.round.test_timers = timers.clone();
    }
}
