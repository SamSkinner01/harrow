//! Maps: environment (sun, sky, desert), the fortress kit loader and
//! material library, and the playable areas.
//!
//! `--map keep` (default, also `--map` alone): Harrow Keep, the full map.
//! `--map section_a`: the first small section (M2), one building and a courtyard.
//! `--map greybox`: the M1 controller test area.

mod batch;
mod environment;
mod greybox;
mod harrow_keep;
mod keep;
mod kit;
mod lamps;
mod materials;
mod props;

use bevy::prelude::*;

pub use kit::KitLoaded;

pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            materials::MaterialsPlugin,
            kit::KitPlugin,
            environment::EnvironmentPlugin,
            lamps::LampsPlugin,
            batch::BatchPlugin,
            props::PropsPlugin,
        ));
        // A dedicated server's config file can choose the map (`MapOverride`,
        // inserted before this plugin); otherwise `--map`.
        let choice = match app.world().get_resource::<MapOverride>() {
            Some(name) => MapChoice::from_name(&name.0),
            None => MapChoice::parse_args(),
        };
        CHOICE.get_or_init(|| choice);
        match choice {
            MapChoice::Greybox => app.add_plugins(greybox::GreyboxPlugin),
            MapChoice::SectionA => app.add_plugins(keep::KeepPlugin),
            MapChoice::Keep => app.add_plugins(harrow_keep::HarrowKeepPlugin),
        };
        CURRENT_MAP.get_or_init(|| choice.name().to_string());
    }
}

/// Which map is played. `MapPlugin` settles it once.
static CHOICE: std::sync::OnceLock<MapChoice> = std::sync::OnceLock::new();

/// Which map `--map` (or the server config) asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapChoice {
    /// Harrow Keep, the full map (default).
    Keep,
    /// The first section built in M2.
    SectionA,
    /// The grey-box movement test area.
    Greybox,
}

impl MapChoice {
    /// The map being played: the dedicated server's config (`MapOverride`)
    /// or `--map`. Settled when `MapPlugin` builds; before that, `--map`.
    pub fn from_args() -> Self {
        CHOICE.get().copied().unwrap_or_else(Self::parse_args)
    }

    fn parse_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let value = args
            .iter()
            .position(|a| a == "--map")
            .and_then(|i| args.get(i + 1))
            .filter(|v| !v.starts_with("--"));
        // Test flags written against section A's geometry (fixed positions
        // in its courtyard and rooms) load it unless a map is named.
        const SECTION_A_TESTS: &[&str] = &[
            "--weapon-test",
            "--dummies",
            "--lineup",
            "--lineup-state",
            "--lineup-crowd",
            "--m4",
            "--screenshot-on-hit",
            "--gadget-test",
        ];
        if value.is_none()
            && !args.iter().any(|a| a == "--map")
            && let Some(flag) = args.iter().find(|a| SECTION_A_TESTS.contains(&a.as_str()))
        {
            info!("{flag} uses section A's layout: loading --map section_a");
            return MapChoice::SectionA;
        }
        value.map_or(MapChoice::Keep, |name| MapChoice::from_name(name))
    }

    /// `keep`, `section_a` or `greybox` (unknown names load the keep).
    pub fn from_name(name: &str) -> Self {
        match name {
            "keep" => MapChoice::Keep,
            "section_a" => MapChoice::SectionA,
            "greybox" => MapChoice::Greybox,
            other => {
                warn!("unknown map {other:?} (keep, section_a or greybox); loading the keep");
                MapChoice::Keep
            }
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            MapChoice::Keep => "keep",
            MapChoice::SectionA => "section_a",
            MapChoice::Greybox => "greybox",
        }
    }

    /// Maps with bomb sites and spawns (the round loop runs on these).
    pub fn has_objectives(self) -> bool {
        self != MapChoice::Greybox
    }
}

/// Use this map instead of `--map` (dedicated server config).
#[derive(Resource, Debug, Clone)]
pub struct MapOverride(pub String);

static CURRENT_MAP: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// The loaded map (`keep`, `section_a`, `greybox`). Networked players must
/// agree on it.
pub fn current_map_name() -> String {
    CURRENT_MAP.get().cloned().unwrap_or_else(|| "keep".into())
}
