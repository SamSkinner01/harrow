//! Keyboard and mouse -> [`PlayerInput`] for the local player.
//!
//! Bindings live on a separate controller entity (`OnFoot` context), so the
//! player entity itself only ever sees plain intent data.

use bevy::{
    prelude::*,
    window::{CursorGrabMode, CursorOptions},
};
use bevy_enhanced_input::prelude::*;

use super::{LocalPlayer, PlayerInput};
use crate::config::Tuning;

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
        ]
    });
}

const PITCH_LIMIT: f32 = 89.0_f32.to_radians();

fn gather_input(
    movement: Single<&Action<Move>>,
    look: Single<&Action<Look>>,
    sprint: Single<&Action<Sprint>>,
    cursor: Single<&CursorOptions>,
    tuning: Res<Tuning>,
    mut player: Single<&mut PlayerInput, With<LocalPlayer>>,
) {
    if cursor.grab_mode == CursorGrabMode::None {
        // Mouse is free (menus, alt-tab): stand still, ignore the mouse.
        player.move_axis = Vec2::ZERO;
        player.sprint = false;
        return;
    }

    player.move_axis = movement.clamp_length_max(1.0);
    player.sprint = **sprint.into_inner();

    let sensitivity = tuning.look.sensitivity.to_radians();
    let invert = if tuning.look.invert_y { -1.0 } else { 1.0 };
    let delta = look.into_inner();
    player.yaw = (player.yaw - delta.x * sensitivity).rem_euclid(std::f32::consts::TAU);
    player.pitch = (player.pitch - delta.y * sensitivity * invert).clamp(-PITCH_LIMIT, PITCH_LIMIT);
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
