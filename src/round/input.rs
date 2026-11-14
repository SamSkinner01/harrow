//! Keys -> [`RoundIntent`] for the local player: F (hold) interact,
//! 5 equip defuser, 1/2 pick an operator on the selection screen.
//!
//! Own input context, copied into the intent right after bindings are
//! evaluated, and only while the mouse is captured.

use bevy::prelude::*;
use bevy_enhanced_input::prelude::*;

use super::RoundIntent;
use crate::player::{LocalPlayer, input_captured};

pub struct RoundInputPlugin;

impl Plugin for RoundInputPlugin {
    fn build(&self, app: &mut App) {
        app.add_input_context::<RoundControls>()
            .add_systems(Startup, spawn_controls)
            .add_systems(
                PreUpdate,
                (
                    gather_round_input.run_if(input_captured),
                    release_when_free.run_if(not(input_captured)),
                )
                    .after(EnhancedInputSystems::Update),
            );
    }
}

#[derive(Component, Default, Clone)]
struct RoundControls;

#[derive(InputAction)]
#[action_output(bool)]
struct Interact;

#[derive(InputAction)]
#[action_output(bool)]
struct EquipDefuser;

#[derive(InputAction)]
#[action_output(bool)]
struct PickFirst;

#[derive(InputAction)]
#[action_output(bool)]
struct PickSecond;

fn spawn_controls(mut commands: Commands) {
    commands.spawn_scene(bsn! {
        RoundControls
        Actions<RoundControls> [
            Action::<Interact>::new()
            Bindings [
                Binding(KeyCode::KeyF)
            ]
            --
            Action::<EquipDefuser>::new()
            Bindings [
                Binding(KeyCode::Digit5)
            ]
            --
            Action::<PickFirst>::new()
            Bindings [
                Binding(KeyCode::Digit1)
            ]
            --
            Action::<PickSecond>::new()
            Bindings [
                Binding(KeyCode::Digit2)
            ]
        ]
    });
}

fn pressed<A: InputAction<Output = bool>>(action: (&Action<A>, &ActionEvents)) -> bool {
    action.1.contains(ActionEvents::START)
}

fn gather_round_input(
    interact: Single<&Action<Interact>>,
    equip: Single<(&Action<EquipDefuser>, &ActionEvents)>,
    first: Single<(&Action<PickFirst>, &ActionEvents)>,
    second: Single<(&Action<PickSecond>, &ActionEvents)>,
    mut intent: Single<&mut RoundIntent, With<LocalPlayer>>,
) {
    intent.interact = **interact.into_inner();
    // Presses are latched until the fixed tick handles them.
    if pressed(equip.into_inner()) {
        intent.equip_defuser = true;
    }
    if pressed(first.into_inner()) {
        intent.pick_operator = Some(0);
    }
    if pressed(second.into_inner()) {
        intent.pick_operator = Some(1);
    }
}

/// Mouse freed (menu, alt-tab): let go of F.
fn release_when_free(mut intent: Single<&mut RoundIntent, With<LocalPlayer>>) {
    if intent.interact {
        intent.interact = false;
    }
}
