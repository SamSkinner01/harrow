//! Maps: environment (sun, sky, desert), the fortress kit loader and
//! material library, and the playable areas.
//!
//! `--map keep` (default): Harrow Keep, first section, built from the kit.
//! `--map greybox`: the M1 controller test area.

mod environment;
mod greybox;
mod keep;
mod kit;
mod materials;

use bevy::prelude::*;

pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            materials::MaterialsPlugin,
            kit::KitPlugin,
            environment::EnvironmentPlugin,
        ));
        match map_from_args().as_deref() {
            Some("greybox") => app.add_plugins(greybox::GreyboxPlugin),
            Some("keep") | None => app.add_plugins(keep::KeepPlugin),
            Some(other) => {
                warn!("unknown --map {other:?}; loading the keep");
                app.add_plugins(keep::KeepPlugin)
            }
        };
    }
}

fn map_from_args() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == "--map")
        .and_then(|i| args.get(i + 1))
        .cloned()
}
