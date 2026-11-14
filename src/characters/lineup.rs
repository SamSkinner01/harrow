//! `--lineup`: the four operators in a row in the keep courtyard, facing
//! the default spawn point, cycling through their animations so they can be
//! looked at and screenshotted.
//!
//! - `--lineup`: each operator steps through [`STATES`] every
//!   [`STATE_SECONDS`], offset from its neighbours so four different states
//!   are on screen at once.
//! - `--lineup-state <name>`: all four hold one state (names in [`STATES`]).
//! - `--lineup-crowd`: two more rows behind (12 characters), for checking
//!   what a full match costs (combine with `--no-vsync` and F3).

use std::f32::consts::PI;

use bevy::prelude::*;

use super::{CharacterAnim, CharacterParts, Operator, spawn_character};
use crate::player::Stance;

pub(super) struct LineupPlugin;

impl Plugin for LineupPlugin {
    fn build(&self, app: &mut App) {
        let args: Vec<String> = std::env::args().collect();
        let held = args
            .iter()
            .position(|a| a == "--lineup-state")
            .and_then(|i| args.get(i + 1))
            .cloned();
        let crowd = args.iter().any(|a| a == "--lineup-crowd");
        if !args.iter().any(|a| a == "--lineup") && held.is_none() && !crowd {
            return;
        }
        let held = held.and_then(|name| {
            let found = STATES.iter().position(|s| s.name == name);
            if found.is_none() {
                let names: Vec<&str> = STATES.iter().map(|s| s.name).collect();
                warn!("--lineup-state: unknown state {name:?}; expected one of {names:?}");
            }
            found
        });
        let rows = if crowd { 3 } else { 1 };
        app.insert_resource(Lineup { held, rows })
            .add_systems(Startup, spawn_lineup)
            .add_systems(Update, (cycle_states, report_heights));
    }
}

/// x positions along the courtyard (z = 6), facing +Z (towards the default
/// spawn at (8, 0, 9)).
const SLOTS: [f32; 4] = [5.0, 7.0, 9.0, 11.0];
const LINE_Z: f32 = 6.0;
const STATE_SECONDS: f32 = 2.5;

struct State {
    name: &'static str,
    stance: Stance,
    speed: f32,
    lean: f32,
    aiming: bool,
    firing: bool,
    reloading: bool,
}

const fn state(name: &'static str, stance: Stance, speed: f32) -> State {
    State {
        name,
        stance,
        speed,
        lean: 0.0,
        aiming: false,
        firing: false,
        reloading: false,
    }
}

const STATES: [State; 12] = [
    state("idle", Stance::Stand, 0.0),
    state("walk", Stance::Stand, 3.4),
    state("run", Stance::Stand, 5.6),
    State {
        lean: -1.0,
        ..state("lean_left", Stance::Stand, 0.0)
    },
    State {
        aiming: true,
        ..state("aim", Stance::Stand, 0.0)
    },
    State {
        firing: true,
        ..state("fire", Stance::Stand, 0.0)
    },
    State {
        reloading: true,
        ..state("reload", Stance::Stand, 0.0)
    },
    state("crouch", Stance::Crouch, 0.0),
    state("crouch_walk", Stance::Crouch, 1.9),
    State {
        lean: 1.0,
        aiming: true,
        ..state("crouch_lean_right", Stance::Crouch, 0.0)
    },
    state("prone", Stance::Prone, 0.0),
    state("crawl", Stance::Prone, 0.8),
];

#[derive(Resource)]
struct Lineup {
    held: Option<usize>,
    rows: usize,
}

#[derive(Component)]
struct LineupSlot {
    index: usize,
    current: Option<usize>,
}

fn spawn_lineup(mut commands: Commands, assets: Res<AssetServer>, lineup: Res<Lineup>) {
    for row in 0..lineup.rows {
        let z = LINE_Z - 2.0 * row as f32;
        for (slot, (&operator, x)) in Operator::ALL.iter().zip(SLOTS).enumerate() {
            let transform = Transform::from_xyz(x, 0.0, z).with_rotation(Quat::from_rotation_y(PI));
            if row == 0 {
                info!(
                    "lineup: {} ({:?}) at x = {x}",
                    operator.name(),
                    operator.team()
                );
            }
            let entity = spawn_character(&mut commands, &assets, operator, transform);
            commands.entity(entity).insert(LineupSlot {
                index: slot + row,
                current: None,
            });
        }
    }
}

fn cycle_states(
    lineup: Res<Lineup>,
    time: Res<Time>,
    mut characters: Query<(&Name, &mut LineupSlot, &mut CharacterAnim)>,
) {
    let step = (time.elapsed_secs() / STATE_SECONDS) as usize;
    for (name, mut slot, mut anim) in &mut characters {
        let index = lineup
            .held
            .unwrap_or((step + slot.index * 3) % STATES.len());
        if slot.current != Some(index) {
            slot.current = Some(index);
            info!("lineup: {name} -> {}", STATES[index].name);
        }
        let s = &STATES[index];
        *anim = CharacterAnim {
            speed: s.speed,
            stance: s.stance,
            lean: s.lean,
            aiming: s.aiming,
            firing: s.firing,
            reloading: s.reloading,
        };
    }
}

/// Logs each operator's head height once, as a scale check against the
/// 1.8 m player capsule.
fn report_heights(
    characters: Query<(&Name, &CharacterParts), Added<CharacterParts>>,
    transforms: Query<&GlobalTransform>,
) {
    for (name, parts) in &characters {
        let height = |bone: &str| {
            parts
                .bone(bone)
                .and_then(|e| transforms.get(e).ok())
                .map(|t| t.translation().y)
                .unwrap_or(f32::NAN)
        };
        info!(
            "lineup: {name} bind pose: head bone at {:.2} m, neck {:.2} m, pelvis {:.2} m",
            height("head"),
            height("neck_01"),
            height("pelvis")
        );
    }
}
