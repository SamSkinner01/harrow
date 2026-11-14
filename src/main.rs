//! HARROW: a tactical first-person attack/defense shooter.
//!
//! Each gameplay area lives in its own module and exposes a Bevy plugin.
//! Every plugin is registered here up front; a module that is still a
//! placeholder simply does nothing.
//!
//! The same binary plays solo, hosts, joins, or runs a headless dedicated
//! server (`--server`); see `session.rs` and `net/`.

mod ai;
#[cfg(feature = "client")]
mod audio;
mod characters;
mod combat;
mod config;
mod debug;
mod destruction;
mod gadgets;
mod map;
#[cfg(feature = "client")]
mod menu;
mod net;
mod physics;
mod player;
mod round;
mod session;
mod ui;
mod weapons;

use bevy::prelude::*;
use bevy_enhanced_input::prelude::*;

use net::dedicated::{ServerConfig, ServerConfigPlugin};
use session::{Launch, NetMode};

fn main() -> AppExit {
    let mut launch = Launch::from_args();
    let mut app = App::new();

    if launch.dedicated {
        // Headless dedicated server: no window, GPU, audio or input devices.
        let (config, error) = ServerConfig::for_launch(&launch);
        launch.mode = Some(NetMode::Dedicated { port: config.port });
        app.add_plugins(net::dedicated::headless_plugins(&config))
            .insert_resource(map::MapOverride(config.map.clone()))
            .add_plugins(ServerConfigPlugin { config, error });
        // The client build compiles the sound code: give it somewhere to
        // put the sounds and drop any that get started, since the audio
        // plugin is off.
        #[cfg(feature = "client")]
        app.init_asset::<bevy::audio::AudioSource>()
            .add_systems(Last, net::dedicated::drop_sounds);
    } else {
        #[cfg(feature = "client")]
        app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Harrow".into(),
                ..default()
            }),
            ..default()
        }));
        #[cfg(not(feature = "client"))]
        {
            eprintln!("This is the dedicated-server build: run it with --server [port] [--config file].");
            return AppExit::error();
        }
    }

    app
        // A failing system or observer (e.g. a network socket that can't
        // open) logs an error instead of crashing the game.
        .set_error_handler(bevy::ecs::error::error)
        // Engine-level setup first: input, physics, tuning values.
        .add_plugins((
            EnhancedInputPlugin,
            physics::PhysicsSetupPlugin,
            config::TuningPlugin,
        ))
        // Session flow and networking (before gameplay: the protocol
        // registers gameplay components, and session states gate systems).
        .add_plugins((
            session::SessionPlugin {
                launch: launch.clone(),
            },
            net::NetPlugin {
                launch: launch.clone(),
            },
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
        .add_plugins((ui::UiPlugin, debug::DebugPlugin));
    #[cfg(feature = "client")]
    if !launch.dedicated {
        app.add_plugins(menu::MenuPlugin);
    }
    app.run()
}
