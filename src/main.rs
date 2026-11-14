//! HARROW: a tactical first-person attack/defense shooter.
//!
//! Each gameplay area lives in its own module and exposes a Bevy plugin.
//! Modules that are still empty are placeholders for later milestones.

mod ai;
mod debug;
mod destruction;
mod gadgets;
mod map;
mod player;
mod round;
mod ui;
mod weapons;

use bevy::prelude::*;

fn main() -> AppExit {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Harrow".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins((map::MapPlugin, debug::DebugPlugin))
        .run()
}
