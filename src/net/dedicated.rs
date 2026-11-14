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
//! Lean assets: without a render world, Bevy keeps every decoded texture in
//! RAM (section A: ~750 MB of the server's memory; the full map's props
//! add ~380 MB more). A headless server reads its assets through
//! [`LeanAssetReader`], which hands out a 1x1 stand-in for every image
//! under `textures/` (material and character detail maps) and for every
//! image embedded in a `.glb` model. The code that asks for them works
//! unchanged, models keep their meshes (kit and prop collision), skeletons
//! and animations, and the server bundle doesn't need `textures/`.
//!
//! Config file (RON), every field optional; see `assets/config/server.ron`:
//! name, port, map, max_players, log_level, round_loop, key (shared
//! secret), and round overrides (test_timers, rounds_to_win, timers).

use std::{io::IsTerminal, path::Path, time::Duration};

use bevy::{
    app::ScheduleRunnerPlugin,
    asset::{
        AssetApp, AssetPlugin,
        io::{
            AssetReader, AssetReaderError, AssetSource, AssetSourceBuilder, AssetSourceId,
            ErasedAssetReader, PathStream, Reader, SliceReader, VecReader,
        },
    },
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
    /// Shared secret: only clients started with the same key (`--key` or
    /// `HARROW_KEY`) can join. `None`: the development key, anyone with the
    /// game can join. `HARROW_KEY` / `--key` on the server override it.
    pub key: Option<String>,
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
            key: None,
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
        if launch.key.is_some() {
            config.key = launch.key.clone();
        }
        config.key = config.key.filter(|k| !k.trim().is_empty());
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
            // Without a render world some engine plugins complain at
            // startup that it's missing (an ERROR line and a few WARNs that
            // mean nothing for a server): keep the server's log clean.
            filter: format!(
                "{},bevy_extract=off,bevy_gizmos_render=error,bevy_render::texture=error,bevy_gltf=error",
                bevy::log::DEFAULT_FILTER
            ),
            fmt_layer: plain_stdout_logs,
            ..default()
        })
        .add(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
            1.0 / TICK_HZ,
        )))
        // Must come before the asset plugin, which builds the sources.
        .add_before::<AssetPlugin>(LeanAssetsPlugin);
    #[cfg(feature = "client")]
    let plugins = plugins
        .disable::<bevy::winit::WinitPlugin>()
        .disable::<bevy::audio::AudioPlugin>()
        .disable::<bevy::gilrs::GilrsPlugin>();
    plugins
}

/// Replaces the default asset source with [`LeanAssetReader`] over the
/// usual `assets/` folder (`BEVY_ASSET_ROOT` still applies).
struct LeanAssetsPlugin;

impl Plugin for LeanAssetsPlugin {
    fn build(&self, app: &mut App) {
        app.register_asset_source(
            AssetSourceId::Default,
            AssetSourceBuilder::new(|| {
                Box::new(LeanAssetReader(AssetSource::get_default_reader("assets".into())()))
            }),
        );
    }
}

/// The dedicated server's view of `assets/`: images under `textures/` and
/// inside `.glb` models are a 1x1 stand-in (a server never draws them);
/// everything else is read from disk as usual. See the module docs.
pub struct LeanAssetReader(Box<dyn ErasedAssetReader>);

/// A 1x1 grey JPEG and PNG (the image loader picks the format by
/// extension).
const STAND_IN_JPEG: &[u8] = &[
    255, 216, 255, 224, 0, 16, 74, 70, 73, 70, 0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 255, 219, 0, 67, 0, 8,
    6, 6, 7, 6, 5, 8, 7, 7, 7, 9, 9, 8, 10, 12, 20, 13, 12, 11, 11, 12, 25, 18, 19, 15, 20, 29, 26,
    31, 30, 29, 26, 28, 28, 32, 36, 46, 39, 32, 34, 44, 35, 28, 28, 40, 55, 41, 44, 48, 49, 52, 52,
    52, 31, 39, 57, 61, 56, 50, 60, 46, 51, 52, 50, 255, 219, 0, 67, 1, 9, 9, 9, 12, 11, 12, 24, 13,
    13, 24, 50, 33, 28, 33, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50,
    50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50,
    50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 255, 192, 0, 17, 8, 0, 1, 0, 1, 3, 1, 34, 0, 2,
    17, 1, 3, 17, 1, 255, 196, 0, 20, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 196,
    0, 20, 16, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 196, 0, 20, 1, 1, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 196, 0, 20, 17, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 255, 218, 0, 12, 3, 1, 0, 2, 17, 3, 17, 0, 63, 0, 0, 15, 255, 217,
];
const STAND_IN_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0,
    0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 218, 99, 104, 104, 104, 0, 0, 3, 4,
    1, 129, 117, 46, 1, 188, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

/// The stand-in for `path`, if a server doesn't need the real file.
fn stand_in(path: &Path) -> Option<&'static [u8]> {
    if !path.starts_with("textures") {
        return None;
    }
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => Some(STAND_IN_JPEG),
        "png" => Some(STAND_IN_PNG),
        _ => None,
    }
}

/// [`STAND_IN_PNG`] as a data URI, for images inside glTF files.
const STAND_IN_PNG_URI: &str =
    "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR42mNoaGgAAAMEAYF1LgG8AAAAAElFTkSuQmCC";

/// A binary glTF (`.glb`) with every image replaced by the 1x1 stand-in
/// (an inline data URI); meshes, skeletons, animations and the binary
/// buffer are untouched (the old image bytes stay in the buffer, unused,
/// and are dropped after loading). `None`: not a glb, or no images.
fn strip_glb_images(glb: &[u8]) -> Option<Vec<u8>> {
    const JSON: u32 = 0x4E4F_534A;
    let u32_at = |i: usize| glb.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    if glb.get(0..4)? != b"glTF" || u32_at(4)? != 2 || u32_at(16)? != JSON {
        return None;
    }
    let json_end = 20 + u32_at(12)? as usize;
    let mut json: serde_json::Value = serde_json::from_slice(glb.get(20..json_end)?).ok()?;
    let images = json.get_mut("images")?.as_array_mut()?;
    if images.is_empty() {
        return None;
    }
    for image in images.iter_mut() {
        let mut stand_in = serde_json::json!({ "uri": STAND_IN_PNG_URI });
        if let Some(name) = image.get("name") {
            stand_in["name"] = name.clone();
        }
        *image = stand_in;
    }
    let mut text = serde_json::to_vec(&json).ok()?;
    while text.len() % 4 != 0 {
        text.push(b' ');
    }
    let rest = &glb[json_end..];
    let total = 20 + text.len() + rest.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(&JSON.to_le_bytes());
    out.extend_from_slice(&text);
    out.extend_from_slice(rest);
    Some(out)
}

impl AssetReader for LeanAssetReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<Box<dyn Reader + 'a>, AssetReaderError> {
        if let Some(bytes) = stand_in(path) {
            return Ok(Box::new(SliceReader::new(bytes)));
        }
        let mut reader = self.0.read(path).await?;
        if path.extension().is_none_or(|e| !e.eq_ignore_ascii_case("glb")) {
            return Ok(reader);
        }
        // Models: keep the geometry, drop the pictures.
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let bytes = strip_glb_images(&bytes).unwrap_or(bytes);
        Ok(Box::new(VecReader::new(bytes)))
    }

    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<Box<dyn Reader + 'a>, AssetReaderError> {
        self.0.read_meta(path).await
    }

    async fn read_directory<'a>(&'a self, path: &'a Path) -> Result<Box<PathStream>, AssetReaderError> {
        self.0.read_directory(path).await
    }

    async fn is_directory<'a>(&'a self, path: &'a Path) -> Result<bool, AssetReaderError> {
        self.0.is_directory(path).await
    }
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
        // Bevy's own SIGINT/SIGTERM handler politely declines when the
        // server was started with SIGINT ignored (a background job of a
        // script, nohup), and SIGTERM would then kill it mid-tick. In that
        // case install it ourselves (if Bevy's is in place this fails,
        // harmlessly).
        if ctrlc::set_handler(bevy::app::TerminalCtrlCHandlerPlugin::gracefully_exit).is_ok() {
            info!("SIGINT/SIGTERM handler installed (SIGINT was ignored at start)");
        }
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
