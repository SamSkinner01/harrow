//! Mouse and keyboard -> [`WeaponInput`] for the local player.
//!
//! Bindings live on their own controller entity (`WeaponControls` context),
//! following the pattern in `src/player/input.rs`. Hold-vs-toggle aiming is
//! resolved here, so the simulation only sees "wants to aim".

use bevy::prelude::*;
use bevy_enhanced_input::prelude::*;

use super::{Slot, WeaponInput};
use crate::{
    config::{ButtonMode, Tuning},
    player::{LocalPlayer, input_captured},
};

pub struct WeaponInputPlugin;

impl Plugin for WeaponInputPlugin {
    fn build(&self, app: &mut App) {
        app.add_input_context::<WeaponControls>()
            .init_resource::<TriggerGuard>()
            .add_systems(Startup, spawn_controller)
            .add_systems(
                PreUpdate,
                (
                    gather_input.run_if(input_captured),
                    release_input.run_if(not(input_captured)),
                )
                    .after(EnhancedInputSystems::Update),
            );
    }
}

/// After the mouse is (re)captured, the trigger stays dead until released
/// once, so the click that captures the mouse doesn't also shoot.
#[derive(Resource, Default)]
struct TriggerGuard {
    wait_for_release: bool,
}

/// Input context for weapon handling.
#[derive(Component, Default, Clone)]
struct WeaponControls;

#[derive(InputAction)]
#[action_output(bool)]
struct Fire;

#[derive(InputAction)]
#[action_output(bool)]
struct Aim;

#[derive(InputAction)]
#[action_output(bool)]
struct Reload;

#[derive(InputAction)]
#[action_output(bool)]
struct SelectPrimary;

#[derive(InputAction)]
#[action_output(bool)]
struct SelectSidearm;

/// Mouse wheel; y is the vertical scroll.
#[derive(InputAction)]
#[action_output(Vec2)]
struct CycleWeapon;

fn spawn_controller(mut commands: Commands) {
    commands.spawn_scene(bsn! {
        WeaponControls
        Actions<WeaponControls> [
            Action::<Fire>::new()
            Bindings [
                Binding(MouseButton::Left)
            ]
            --
            Action::<Aim>::new()
            Bindings [
                Binding(MouseButton::Right)
            ]
            --
            Action::<Reload>::new()
            Bindings [
                Binding(KeyCode::KeyR)
            ]
            --
            Action::<SelectPrimary>::new()
            Bindings [
                Binding(KeyCode::Digit1)
            ]
            --
            Action::<SelectSidearm>::new()
            Bindings [
                Binding(KeyCode::Digit2)
            ]
            --
            Action::<CycleWeapon>::new()
            Bindings [
                Binding::mouse_wheel()
            ]
        ]
    });
}

/// A button's state this frame.
#[derive(Clone, Copy)]
struct Button {
    held: bool,
    pressed: bool,
}

fn button<A: InputAction<Output = bool>>(action: (&Action<A>, &ActionEvents)) -> Button {
    let (value, events) = action;
    Button {
        held: **value,
        pressed: events.contains(ActionEvents::START),
    }
}

#[allow(clippy::too_many_arguments)]
fn gather_input(
    fire: Single<(&Action<Fire>, &ActionEvents)>,
    aim: Single<(&Action<Aim>, &ActionEvents)>,
    reload: Single<(&Action<Reload>, &ActionEvents)>,
    primary: Single<(&Action<SelectPrimary>, &ActionEvents)>,
    sidearm: Single<(&Action<SelectSidearm>, &ActionEvents)>,
    wheel: Single<&Action<CycleWeapon>>,
    tuning: Res<Tuning>,
    mut input: Single<&mut WeaponInput, With<LocalPlayer>>,
    mut guard: ResMut<TriggerGuard>,
    time: Res<Time>,
    mut last_wheel: Local<f32>,
) {
    let fire = button(fire.into_inner());
    let aim = button(aim.into_inner());
    let reload = button(reload.into_inner());
    let primary = button(primary.into_inner());
    let sidearm = button(sidearm.into_inner());

    if guard.wait_for_release && !fire.held {
        guard.wait_for_release = false;
    }
    let trigger_live = !guard.wait_for_release;

    input.trigger = trigger_live && fire.held;
    if trigger_live && fire.pressed {
        input.trigger_presses = input.trigger_presses.wrapping_add(1);
    }
    if reload.pressed {
        input.reload_requests = input.reload_requests.wrapping_add(1);
    }

    match tuning.weapons.ads_mode {
        ButtonMode::Hold => input.aim = aim.held,
        ButtonMode::Toggle => {
            if aim.pressed {
                input.aim = !input.aim;
            }
        }
    }

    if primary.pressed {
        input.slot = Slot::Primary;
    } else if sidearm.pressed {
        input.slot = Slot::Sidearm;
    } else if wheel.y != 0.0 && time.elapsed_secs() - *last_wheel > WHEEL_REPEAT {
        // Two slots: any wheel step swaps. Smooth-scrolling wheels and
        // touchpads send a stream of small steps, so swap at most every
        // WHEEL_REPEAT seconds.
        *last_wheel = time.elapsed_secs();
        input.slot = input.slot.other();
    }
}

const WHEEL_REPEAT: f32 = 0.25;

/// Mouse released (menus, alt-tab): let go of the trigger and the aim.
fn release_input(
    mut input: Single<&mut WeaponInput, With<LocalPlayer>>,
    mut guard: ResMut<TriggerGuard>,
) {
    input.trigger = false;
    input.aim = false;
    guard.wait_for_release = true;
}
