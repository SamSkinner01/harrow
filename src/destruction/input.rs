//! Keyboard -> [`DestructionIntent`] for the local player: V melee, F (hold)
//! interact, 4 secondary gadget. Same pattern as `src/player/input.rs`: the
//! bindings live on their own context entity and only plain held/not-held
//! values reach the player. The simulation finds presses itself (by
//! comparing with the previous tick), so bots and the network can write the
//! same component.

use bevy::prelude::*;
use bevy_enhanced_input::prelude::*;

use super::DestructionIntent;
use crate::player::{LocalPlayer, input_captured};

pub struct DestructionInputPlugin;

impl Plugin for DestructionInputPlugin {
    fn build(&self, app: &mut App) {
        app.add_input_context::<DestructionControls>()
            .add_systems(Startup, spawn_controls)
            .add_systems(
                PreUpdate,
                (
                    gather_input.run_if(input_captured),
                    release_all.run_if(not(input_captured)),
                )
                    .after(EnhancedInputSystems::Update),
            );
    }
}

/// Input context for destruction actions (melee, interact, gadget).
#[derive(Component, Default, Clone)]
struct DestructionControls;

#[derive(InputAction)]
#[action_output(bool)]
struct Melee;

#[derive(InputAction)]
#[action_output(bool)]
struct Interact;

#[derive(InputAction)]
#[action_output(bool)]
struct SecondaryGadget;

fn spawn_controls(mut commands: Commands) {
    commands.spawn_scene(bsn! {
        DestructionControls
        Actions<DestructionControls> [
            Action::<Melee>::new()
            Bindings [
                Binding(KeyCode::KeyV)
            ]
            --
            Action::<Interact>::new()
            Bindings [
                Binding(KeyCode::KeyF)
            ]
            --
            Action::<SecondaryGadget>::new()
            Bindings [
                Binding(KeyCode::Digit4)
            ]
        ]
    });
}

fn gather_input(
    melee: Single<&Action<Melee>>,
    interact: Single<&Action<Interact>>,
    gadget: Single<&Action<SecondaryGadget>>,
    mut intent: Query<&mut DestructionIntent, With<LocalPlayer>>,
) {
    let held = DestructionIntent {
        melee: **melee.into_inner(),
        interact: **interact.into_inner(),
        gadget: **gadget.into_inner(),
    };
    for mut intent in &mut intent {
        *intent = held;
    }
}

/// Mouse released (menus, alt-tab): let go of everything.
fn release_all(mut intent: Query<&mut DestructionIntent, With<LocalPlayer>>) {
    for mut intent in &mut intent {
        *intent = DestructionIntent::default();
    }
}
