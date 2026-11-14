//! Player settings (client only): controls, audio, graphics and display.
//!
//! Saved as `settings.ron` in the user's config directory, next to the
//! menu's `menu.ron` ([`config_file`]): `~/.config/harrow/` on Linux,
//! `%APPDATA%\harrow\` on Windows. Edited on the settings screen
//! (`src/menu/settings_screen.rs`).
//!
//! Settings sit on top of the tuning file: control values left at "tuning"
//! (`None`) follow `assets/config/tuning.ron`; set ones replace the tuning
//! value at runtime (the file is never written). Volumes multiply the
//! tuning volumes. Graphics settings choose the per-camera effects and
//! shadow quality ([`crate::map::GraphicsOptions`]).
//!
//! Automated runs (`--screenshot`, `--weapon-test`, scripts...) ignore the
//! saved file and use the defaults, so results don't depend on who runs
//! them. `--quality low|medium|high|ultra` forces a graphics preset and
//! `--window WIDTHxHEIGHT` sets the window size (both also for tests).

use std::path::PathBuf;

use bevy::{
    audio::{GlobalVolume, Volume},
    dev_tools::fps_overlay::FpsOverlayConfig,
    prelude::*,
    window::{MonitorSelection, PresentMode, PrimaryWindow, WindowMode},
};
use serde::{Deserialize, Serialize};

use crate::{
    config::{ButtonMode, Tuning, TuningSystems},
    map::{AntiAliasing, GraphicsOptions, ShadowQuality, SsaoQuality},
    player::AutomatedRun,
    ui::style,
};

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PlayerSettings::default())
            .insert_resource(SettingsFile { save: false })
            .add_systems(PreStartup, load_settings)
            .add_systems(Startup, (window_from_args, style_fps_overlay))
            .add_systems(
                PreUpdate,
                (
                    apply_to_tuning
                        .after(TuningSystems)
                        .run_if(resource_changed::<PlayerSettings>.or_eager(resource_changed::<Tuning>)),
                    (apply_graphics, apply_audio, apply_display)
                        .run_if(resource_changed::<PlayerSettings>),
                ),
            );
    }
}

/// Where per-user files live: `<config dir>/harrow/<name>`.
/// - Linux: `$XDG_CONFIG_HOME/harrow/` (default `~/.config/harrow/`)
/// - Windows: `%APPDATA%\harrow\`
/// - macOS: `~/Library/Application Support/harrow/`
pub fn config_file(name: &str) -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    base.map(|b| b.join("harrow").join(name))
}

/// Hold or toggle (a copy of the tuning's `ButtonMode` that can be saved).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    Hold,
    Toggle,
}

impl From<Press> for ButtonMode {
    fn from(p: Press) -> Self {
        match p {
            Press::Hold => ButtonMode::Hold,
            Press::Toggle => ButtonMode::Toggle,
        }
    }
}

impl From<ButtonMode> for Press {
    fn from(m: ButtonMode) -> Self {
        match m {
            ButtonMode::Hold => Press::Hold,
            ButtonMode::Toggle => Press::Toggle,
        }
    }
}

/// Graphics presets. Picking one sets every graphics option; changing an
/// option by hand makes it `Custom`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    Low,
    Medium,
    High,
    Ultra,
    Custom,
}

impl Quality {
    pub const PRESETS: [Quality; 4] = [Quality::Low, Quality::Medium, Quality::High, Quality::Ultra];

    pub fn label(self) -> &'static str {
        match self {
            Quality::Low => "Low",
            Quality::Medium => "Medium",
            Quality::High => "High",
            Quality::Ultra => "Ultra",
            Quality::Custom => "Custom",
        }
    }

    /// The graphics options of a preset (`Custom` has none).
    pub fn graphics(self) -> Option<Graphics> {
        let g = |aa, ao, shadows, bloom, contact| Graphics {
            anti_aliasing: aa,
            ambient_occlusion: ao,
            shadows,
            bloom,
            contact_shadows: contact,
        };
        Some(match self {
            Quality::Low => g(Aa::Fxaa, Ao::Off, Shadows::Low, false, false),
            Quality::Medium => g(Aa::Smaa, Ao::Low, Shadows::Medium, true, false),
            Quality::High => g(Aa::Taa, Ao::Medium, Shadows::High, true, true),
            Quality::Ultra => g(Aa::Taa, Ao::High, Shadows::Ultra, true, true),
            Quality::Custom => return None,
        })
    }

    fn from_arg(name: &str) -> Option<Self> {
        Some(match name {
            "low" => Quality::Low,
            "medium" => Quality::Medium,
            "high" => Quality::High,
            "ultra" => Quality::Ultra,
            _ => return None,
        })
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aa {
    Off,
    Fxaa,
    Smaa,
    Taa,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ao {
    Off,
    Low,
    Medium,
    High,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shadows {
    Low,
    Medium,
    High,
    Ultra,
}

/// The individual graphics options.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Graphics {
    pub anti_aliasing: Aa,
    pub ambient_occlusion: Ao,
    pub shadows: Shadows,
    pub bloom: bool,
    pub contact_shadows: bool,
}

impl Graphics {
    fn options(self) -> GraphicsOptions {
        GraphicsOptions {
            anti_aliasing: match self.anti_aliasing {
                Aa::Off => AntiAliasing::Off,
                Aa::Fxaa => AntiAliasing::Fxaa,
                Aa::Smaa => AntiAliasing::Smaa,
                Aa::Taa => AntiAliasing::Taa,
            },
            ssao: match self.ambient_occlusion {
                Ao::Off => None,
                Ao::Low => Some(SsaoQuality::Low),
                Ao::Medium => Some(SsaoQuality::Medium),
                Ao::High => Some(SsaoQuality::High),
            },
            bloom: self.bloom,
            contact_shadows: self.contact_shadows,
            shadows: match self.shadows {
                Shadows::Low => ShadowQuality::Low,
                Shadows::Medium => ShadowQuality::Medium,
                Shadows::High => ShadowQuality::High,
                Shadows::Ultra => ShadowQuality::Ultra,
            },
        }
    }
}

/// Everything on the settings screen. `None` = follow the tuning file.
#[derive(Resource, Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct PlayerSettings {
    /// Degrees of turn per mouse count.
    pub sensitivity: Option<f32>,
    pub invert_y: Option<bool>,
    /// Vertical field of view, degrees.
    pub fov: Option<f32>,
    pub sprint: Option<Press>,
    pub crouch: Option<Press>,
    pub lean: Option<Press>,
    pub aim: Option<Press>,
    /// 0..1, every sound.
    pub master_volume: f32,
    /// 0..1, weapons, gadgets, round sounds (not footsteps).
    pub effects_volume: f32,
    pub quality: Quality,
    pub graphics: Graphics,
    pub fullscreen: bool,
    pub vsync: bool,
    pub show_fps: bool,
}

impl Default for PlayerSettings {
    fn default() -> Self {
        Self {
            sensitivity: None,
            invert_y: None,
            fov: None,
            sprint: None,
            crouch: None,
            lean: None,
            aim: None,
            master_volume: 1.0,
            effects_volume: 1.0,
            quality: Quality::High,
            graphics: Quality::High.graphics().expect("preset"),
            fullscreen: false,
            vsync: true,
            show_fps: false,
        }
    }
}

impl PlayerSettings {
    /// Pick a preset (sets every graphics option).
    pub fn set_quality(&mut self, quality: Quality) {
        self.quality = quality;
        if let Some(graphics) = quality.graphics() {
            self.graphics = graphics;
        }
    }

    /// After changing a graphics option by hand: the matching preset, or
    /// `Custom`.
    pub fn match_preset(&mut self) {
        self.quality = Quality::PRESETS
            .into_iter()
            .find(|q| q.graphics() == Some(self.graphics))
            .unwrap_or(Quality::Custom);
    }

    fn load() -> Self {
        let Some(path) = config_file("settings.ron") else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => ron::from_str(&text).unwrap_or_else(|err| {
                warn!("settings {} unreadable ({err}); using defaults", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }
}

/// Whether settings may be written back (not in automated runs).
#[derive(Resource)]
pub struct SettingsFile {
    save: bool,
}

impl SettingsFile {
    /// Write the settings file (called when the settings screen closes).
    pub fn save(&self, settings: &PlayerSettings) {
        if !self.save {
            return;
        }
        let Some(path) = config_file("settings.ron") else {
            return;
        };
        let result = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| {
            let text = ron::ser::to_string_pretty(settings, ron::ser::PrettyConfig::default())
                .map_err(std::io::Error::other)?;
            std::fs::write(&path, text)
        });
        match result {
            Ok(()) => info!("settings saved to {}", path.display()),
            Err(err) => warn!("can't save settings to {}: {err}", path.display()),
        }
    }
}

fn arg_value(flag: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
}

fn load_settings(
    automated: Option<Res<AutomatedRun>>,
    mut settings: ResMut<PlayerSettings>,
    mut file: ResMut<SettingsFile>,
) {
    if automated.is_none() {
        *settings = PlayerSettings::load();
        file.save = true;
    }
    if let Some(raw) = arg_value("--quality") {
        match Quality::from_arg(&raw) {
            Some(q) => settings.set_quality(q),
            None => warn!("--quality expects low, medium, high or ultra, got {raw:?}"),
        }
    }
    if std::env::args().any(|a| a == "--no-vsync") {
        settings.vsync = false;
    }
}

/// `--window WIDTHxHEIGHT`: window size in logical pixels.
fn window_from_args(mut window: Single<&mut Window, With<PrimaryWindow>>) {
    let Some(raw) = arg_value("--window") else {
        return;
    };
    let size: Vec<f32> = raw.split('x').filter_map(|v| v.trim().parse().ok()).collect();
    match size.as_slice() {
        [w, h] if *w >= 320.0 && *h >= 240.0 => window.resolution.set(*w, *h),
        _ => warn!("--window expects WIDTHxHEIGHT, e.g. 1920x1080, got {raw:?}"),
    }
}

/// The FPS counter (F3, or the setting) in the UI's type.
fn style_fps_overlay(mut overlay: ResMut<FpsOverlayConfig>) {
    overlay.text_config = style::bold(style::size::BODY);
    overlay.text_color = style::palette::TEXT;
    overlay.frame_time_graph_config.enabled = false;
}

/// Control settings over the tuning values. Remembers the tuning file's
/// own values (re-read whenever the file changes), so a setting put back to
/// "follow the tuning file" restores them; volumes multiply the file's.
/// Only writes what differs, so it settles after one pass.
fn apply_to_tuning(
    settings: Res<PlayerSettings>,
    mut tuning: ResMut<Tuning>,
    mut memory: Local<Option<(Controlled, Controlled)>>,
) {
    let current = Controlled::read(&tuning);
    // (the file's values, what we last wrote)
    let file = match memory.as_ref() {
        Some((file, written)) if *written == current => *file,
        // First run, or the tuning file was (re)loaded.
        _ => current,
    };
    let wanted = file.with(&settings);
    *memory = Some((file, wanted));
    if wanted != current {
        wanted.write(&mut tuning);
    }
}

/// The tuning values the settings control.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Controlled {
    sensitivity: f32,
    invert_y: bool,
    fov: f32,
    sprint: ButtonMode,
    crouch: ButtonMode,
    lean: ButtonMode,
    aim: ButtonMode,
    weapons_volume: f32,
    round_volume: f32,
}

impl Controlled {
    fn read(t: &Tuning) -> Self {
        Self {
            sensitivity: t.look.sensitivity,
            invert_y: t.look.invert_y,
            fov: t.look.fov,
            sprint: t.controls.sprint,
            crouch: t.controls.crouch,
            lean: t.controls.lean,
            aim: t.weapons.ads_mode,
            weapons_volume: t.weapons.effects.volume,
            round_volume: t.round.volume,
        }
    }

    fn write(&self, t: &mut Tuning) {
        t.look.sensitivity = self.sensitivity;
        t.look.invert_y = self.invert_y;
        t.look.fov = self.fov;
        t.controls.sprint = self.sprint;
        t.controls.crouch = self.crouch;
        t.controls.lean = self.lean;
        t.weapons.ads_mode = self.aim;
        t.weapons.effects.volume = self.weapons_volume;
        t.round.volume = self.round_volume;
    }

    /// These file values with the player's settings on top.
    fn with(self, s: &PlayerSettings) -> Self {
        let mode = |m: Option<Press>, file: ButtonMode| m.map_or(file, ButtonMode::from);
        Self {
            sensitivity: s.sensitivity.unwrap_or(self.sensitivity),
            invert_y: s.invert_y.unwrap_or(self.invert_y),
            fov: s.fov.unwrap_or(self.fov),
            sprint: mode(s.sprint, self.sprint),
            crouch: mode(s.crouch, self.crouch),
            lean: mode(s.lean, self.lean),
            aim: mode(s.aim, self.aim),
            weapons_volume: self.weapons_volume * s.effects_volume,
            round_volume: self.round_volume * s.effects_volume,
        }
    }
}

fn apply_graphics(settings: Res<PlayerSettings>, options: Option<ResMut<GraphicsOptions>>) {
    if let Some(mut options) = options {
        let wanted = settings.graphics.options();
        if *options != wanted {
            *options = wanted;
        }
    }
}

fn apply_audio(settings: Res<PlayerSettings>, global: Option<ResMut<GlobalVolume>>) {
    if let Some(mut global) = global {
        global.volume = Volume::Linear(settings.master_volume.clamp(0.0, 1.0));
    }
}

fn apply_display(
    settings: Res<PlayerSettings>,
    mut window: Query<&mut Window, With<PrimaryWindow>>,
    mut overlay: ResMut<FpsOverlayConfig>,
) {
    if overlay.enabled != settings.show_fps {
        overlay.enabled = settings.show_fps;
    }
    let Ok(mut window) = window.single_mut() else {
        return;
    };
    let mode = if settings.fullscreen {
        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    } else {
        WindowMode::Windowed
    };
    if window.mode != mode {
        window.mode = mode;
    }
    let present = if settings.vsync { PresentMode::AutoVsync } else { PresentMode::AutoNoVsync };
    if window.present_mode != present {
        window.present_mode = present;
    }
}
