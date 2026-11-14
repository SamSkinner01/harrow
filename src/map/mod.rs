//! Maps: environment (sun, sky, desert), the fortress kit loader and
//! material library, and the playable areas.
//!
//! `--map keep` (default): Harrow Keep, first section, built from the kit.
//! `--map greybox`: the M1 controller test area.

mod environment;
mod greybox;
mod keep;
mod kit;
mod lamps;
mod materials;

use bevy::prelude::*;

pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            materials::MaterialsPlugin,
            kit::KitPlugin,
            environment::EnvironmentPlugin,
            lamps::LampsPlugin,
        ));
        // A dedicated server's config file can choose the map (`MapOverride`,
        // inserted before this plugin); otherwise `--map`.
        let choice = app
            .world()
            .get_resource::<MapOverride>()
            .map(|m| m.0.clone())
            .or_else(map_from_args);
        let name = match choice.as_deref() {
            Some("greybox") => {
                app.add_plugins(greybox::GreyboxPlugin);
                "greybox"
            }
            Some("keep") | None => {
                app.add_plugins(keep::KeepPlugin);
                "keep"
            }
            Some(other) => {
                warn!("unknown --map {other:?}; loading the keep");
                app.add_plugins(keep::KeepPlugin);
                "keep"
            }
        };
        CURRENT_MAP.get_or_init(|| name.to_string());
    }
}

/// Use this map instead of `--map` (dedicated server config).
#[derive(Resource, Debug, Clone)]
pub struct MapOverride(pub String);

static CURRENT_MAP: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// The loaded map (`keep`, `greybox`). Networked players must agree on it.
pub fn current_map_name() -> String {
    CURRENT_MAP.get().cloned().unwrap_or_else(|| "keep".into())
}

fn map_from_args() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == "--map")
        .and_then(|i| args.get(i + 1))
        .cloned()
}
