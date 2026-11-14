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
        match MapChoice::from_args() {
            MapChoice::Greybox => app.add_plugins(greybox::GreyboxPlugin),
            MapChoice::SectionA => app.add_plugins(keep::KeepPlugin),
            MapChoice::Keep => app.add_plugins(harrow_keep::HarrowKeepPlugin),
        };
    }
}

/// Which map `--map` asks for.
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
    /// The map chosen on the command line (worked out once).
    pub fn from_args() -> Self {
        static CHOICE: std::sync::OnceLock<MapChoice> = std::sync::OnceLock::new();
        *CHOICE.get_or_init(Self::parse_args)
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
        ];
        if value.is_none()
            && !args.iter().any(|a| a == "--map")
            && let Some(flag) = args.iter().find(|a| SECTION_A_TESTS.contains(&a.as_str()))
        {
            info!("{flag} uses section A's layout: loading --map section_a");
            return MapChoice::SectionA;
        }
        match value.map(String::as_str) {
            None | Some("keep") => MapChoice::Keep,
            Some("section_a") => MapChoice::SectionA,
            Some("greybox") => MapChoice::Greybox,
            Some(other) => {
                warn!("unknown --map {other:?} (keep, section_a or greybox); loading the keep");
                MapChoice::Keep
            }
        }
    }

    /// Maps with bomb sites and spawns (the round loop runs on these).
    pub fn has_objectives(self) -> bool {
        self != MapChoice::Greybox
    }
}
