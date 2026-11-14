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
    pub fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let value = args
            .iter()
            .position(|a| a == "--map")
            .and_then(|i| args.get(i + 1))
            .filter(|v| !v.starts_with("--"));
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
