//! Keyboard and mouse -> [`PlayerInput`] for the local player.
//!
//! Bindings live on a separate controller entity (`OnFoot` context), so the
//! player entity itself only ever sees plain intent data. Hold-vs-toggle
//! behaviour (from the tuning file) is resolved here, so the simulation only
//! ever sees "the stance/lean the player wants".

use bevy::{
    prelude::*,
    window::{CursorGrabMode, CursorOptions},
};
use bevy_enhanced_input::prelude::*;

use super::{Lean, LocalPlayer, PlayerInput, Stance};
use crate::config::{ButtonMode, Tuning};

pub struct PlayerInputPlugin;

impl Plugin for PlayerInputPlugin {
    fn build(&self, app: &mut App) {
        app.add_input_context::<OnFoot>()
            .add_systems(Startup, spawn_controller)
            .add_systems(
                Startup,
                grab_cursor.run_if(not(resource_exists::<CursorGrabDisabled>)),
            )
            // Right after bindings are evaluated, so the fixed-tick simulation
            // that runs later this frame sees this frame's input.
            .add_systems(
                PreUpdate,
                gather_input.after(EnhancedInputSystems::Update),
            )
            .add_systems(
                Update,
                (
                    grab_cursor.run_if(on_click_while_free),
                    release_cursor.run_if(on_escape),
                ),
            );
    }
}

/// Insert to stop the game grabbing the mouse at startup (automated runs).
#[derive(Resource)]
pub struct CursorGrabDisabled;

/// Input context for moving around on foot.
#[derive(Component, Default, Clone)]
struct OnFoot;

#[derive(InputAction)]
#[action_output(Vec2)]
struct Move;

#[derive(InputAction)]
#[action_output(Vec2)]
struct Look;

#[derive(InputAction)]
#[action_output(bool)]
struct Sprint;

#[derive(InputAction)]
#[action_output(bool)]
struct Crouch;

#[derive(InputAction)]
#[action_output(bool)]
struct Prone;

#[derive(InputAction)]
#[action_output(bool)]
struct LeanLeft;

#[derive(InputAction)]
#[action_output(bool)]
struct LeanRight;

fn spawn_controller(mut commands: Commands) {
    commands.spawn_scene(bsn! {
        OnFoot
        Actions<OnFoot> [
            Action::<Move>::new()
            DeadZone
            Bindings [
                {preset::cardinal::wasd_keys()}
            ]
            --
            Action::<Look>::new()
            Bindings [
                Binding::mouse_motion()
            ]
            --
            Action::<Sprint>::new()
            Bindings [
                Binding(KeyCode::ShiftLeft)
            ]
            --
            Action::<Crouch>::new()
            Bindings [
                Binding(KeyCode::KeyC)
            ]
            --
            Action::<Prone>::new()
            Bindings [
                Binding(KeyCode::KeyZ)
            ]
            --
            Action::<LeanLeft>::new()
            Bindings [
                Binding(KeyCode::KeyQ)
            ]
            --
            Action::<LeanRight>::new()
            Bindings [
                Binding(KeyCode::KeyE)
            ]
        ]
    });
}

const PITCH_LIMIT: f32 = 89.0_f32.to_radians();

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
    movement: Single<&Action<Move>>,
    look: Single<&Action<Look>>,
    sprint: Single<(&Action<Sprint>, &ActionEvents)>,
    crouch: Single<(&Action<Crouch>, &ActionEvents)>,
    prone: Single<(&Action<Prone>, &ActionEvents)>,
    lean_left: Single<(&Action<LeanLeft>, &ActionEvents)>,
    lean_right: Single<(&Action<LeanRight>, &ActionEvents)>,
    cursor: Single<&CursorOptions>,
    tuning: Res<Tuning>,
    mut input: Single<&mut PlayerInput, With<LocalPlayer>>,
) {
    if cursor.grab_mode == CursorGrabMode::None {
        // Mouse is free (menus, alt-tab): stand still, ignore the mouse.
        input.move_axis = Vec2::ZERO;
        input.sprint = false;
        return;
    }
    let controls = &tuning.controls;
    let sprint = button(sprint.into_inner());
    let crouch = button(crouch.into_inner());
    let prone = button(prone.into_inner());
    let lean_left = button(lean_left.into_inner());
    let lean_right = button(lean_right.into_inner());

    input.move_axis = movement.clamp_length_max(1.0);

    // Look.
    let sensitivity = tuning.look.sensitivity.to_radians();
    let invert = if tuning.look.invert_y { -1.0 } else { 1.0 };
    let delta = look.into_inner();
    input.yaw = (input.yaw - delta.x * sensitivity).rem_euclid(std::f32::consts::TAU);
    input.pitch = (input.pitch - delta.y * sensitivity * invert).clamp(-PITCH_LIMIT, PITCH_LIMIT);

    // Stance. Toggles flip relative to what was last asked for, so pressing
    // crouch again under a low ceiling cancels a pending stand-up.
    match controls.crouch {
        ButtonMode::Toggle => {
            if crouch.pressed {
                input.stance = toggled(input.stance, Stance::Crouch);
            }
            if prone.pressed {
                input.stance = toggled(input.stance, Stance::Prone);
            }
        }
        ButtonMode::Hold => {
            input.stance = if prone.held {
                Stance::Prone
            } else if crouch.held {
                Stance::Crouch
            } else {
                Stance::Stand
            };
        }
    }

    // Lean.
    match controls.lean {
        ButtonMode::Toggle => {
            if lean_left.pressed {
                input.lean = if input.lean == Lean::Left { Lean::Center } else { Lean::Left };
            }
            if lean_right.pressed {
                input.lean = if input.lean == Lean::Right { Lean::Center } else { Lean::Right };
            }
        }
        ButtonMode::Hold => {
            input.lean = match (lean_left.held, lean_right.held) {
                (true, false) => Lean::Left,
                (false, true) => Lean::Right,
                _ => Lean::Center,
            };
        }
    }
    if input.stance == Stance::Prone {
        input.lean = Lean::Center;
    }

    // Sprint. Starting a sprint stands you up and straightens a lean.
    let moving_forward = input.move_axis.y > 0.5;
    match controls.sprint {
        ButtonMode::Hold => input.sprint = sprint.held,
        ButtonMode::Toggle => {
            if sprint.pressed {
                input.sprint = !input.sprint;
            }
            if !moving_forward {
                input.sprint = false;
            }
        }
    }
    if sprint.pressed && moving_forward {
        input.stance = Stance::Stand;
        input.lean = Lean::Center;
    }
}

/// Pressing a stance toggle: go to it, or back to standing if already there.
fn toggled(current: Stance, pressed: Stance) -> Stance {
    if current == pressed { Stance::Stand } else { pressed }
}

fn grab_cursor(mut cursor: Single<&mut CursorOptions>) {
    cursor.grab_mode = CursorGrabMode::Locked;
    cursor.visible = false;
}

fn release_cursor(mut cursor: Single<&mut CursorOptions>) {
    cursor.grab_mode = CursorGrabMode::None;
    cursor.visible = true;
}

fn on_click_while_free(mouse: Res<ButtonInput<MouseButton>>, cursor: Single<&CursorOptions>) -> bool {
    cursor.grab_mode == CursorGrabMode::None && mouse.just_pressed(MouseButton::Left)
}

fn on_escape(keys: Res<ButtonInput<KeyCode>>) -> bool {
    keys.just_pressed(KeyCode::Escape)
}
