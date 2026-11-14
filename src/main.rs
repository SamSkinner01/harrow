//! HARROW: a tactical first-person attack/defense shooter.
//!
//! Each gameplay area lives in its own module and exposes a Bevy plugin.
//! Every plugin is registered here up front; a module that is still a
//! placeholder simply does nothing.

mod ai;
mod audio;
mod characters;
mod combat;
mod config;
mod debug;
mod destruction;
mod gadgets;
mod map;
mod physics;
mod player;
mod round;
mod ui;
mod weapons;

use bevy::prelude::*;
use bevy_enhanced_input::prelude::*;

fn main() -> AppExit {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Harrow".into(),
                ..default()
            }),
            ..default()
        }))
        // Engine-level setup first: input, physics, tuning values.
        .add_plugins((
            EnhancedInputPlugin,
            physics::PhysicsSetupPlugin,
            config::TuningPlugin,
        ))
        // World and player.
        .add_plugins((
            map::MapPlugin,
            combat::CombatPlugin,
            player::PlayerPlugin,
        ))
        // Gameplay systems (M3+).
        .add_plugins((
            weapons::WeaponsPlugin,
            destruction::DestructionPlugin,
            gadgets::GadgetsPlugin,
            round::RoundPlugin,
            characters::CharactersPlugin,
            ai::AiPlugin,
        ))
        // Presentation and tools last.
        .add_plugins((ui::UiPlugin, debug::DebugPlugin))
        .run()
}
