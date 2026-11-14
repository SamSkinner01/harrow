//! First-person camera. Presentation only: reads the player's simulation
//! state every frame (interpolated between fixed ticks) and never changes
//! gameplay state.

use std::f32::consts::{PI, TAU};

use avian3d::prelude::LinearVelocity;
use bevy::prelude::*;

use super::{CharacterState, Footstep, LocalPlayer, PlayerInput, movement::stride_and_loudness};
use crate::{config::Tuning, debug::DebugView};

pub struct PlayerCameraPlugin;

impl Plugin for PlayerCameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_camera)
            .add_systems(Update, (update_view_bob, follow_local_player).chain());
    }
}

#[derive(Component)]
pub struct PlayerCamera;

/// Head-bob phase. `cycle` runs 0..2 over two footsteps (left, right);
/// `amount` fades bob in and out with movement speed.
#[derive(Component, Default)]
struct ViewBob {
    cycle: f32,
    amount: f32,
}

fn spawn_camera(mut commands: Commands, tuning: Res<Tuning>) {
    commands.spawn((
        Name::new("Player camera"),
        PlayerCamera,
        ViewBob::default(),
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: tuning.look.fov.to_radians(),
            near: 0.05,
            ..default()
        }),
    ));
}

fn update_view_bob(
    player: Single<(Entity, &CharacterState, &LinearVelocity), With<LocalPlayer>>,
    mut bob: Single<&mut ViewBob>,
    mut steps: MessageReader<Footstep>,
    tuning: Res<Tuning>,
    time: Res<Time>,
) {
    let (entity, state, velocity) = player.into_inner();
    let dt = time.delta_secs();
    let speed = velocity.0.with_y(0.0).length();
    let (stride, _) = stride_and_loudness(state, &tuning);

    let target = if state.grounded {
        (speed / tuning.movement.walk_speed).clamp(0.0, 1.3)
    } else {
        0.0
    };
    bob.amount += (target - bob.amount) * (1.0 - (-10.0 * dt).exp());
    bob.cycle = (bob.cycle + speed * dt / stride).rem_euclid(2.0);

    // Re-sync with the simulation: a footstep lands exactly on a whole cycle.
    if steps.read().any(|step| step.entity == entity) {
        bob.cycle = bob.cycle.round().rem_euclid(2.0);
    }
}

/// Third-person debug camera offset in look space: right, up, back.
const THIRD_PERSON_OFFSET: Vec3 = Vec3::new(0.7, 0.3, 3.2);

fn follow_local_player(
    player: Single<(&Transform, &PlayerInput, &CharacterState), With<LocalPlayer>>,
    camera: Single<
        (&mut Transform, &mut Projection, &ViewBob),
        (With<PlayerCamera>, Without<LocalPlayer>),
    >,
    tuning: Res<Tuning>,
    fixed_time: Res<Time<Fixed>>,
    view: Res<DebugView>,
) {
    let (body, input, state) = player.into_inner();
    let (mut transform, mut projection, bob) = camera.into_inner();

    // Body position is already interpolated by Avian; blend height and lean
    // the same way so crouch and lean are smooth at any frame rate.
    let (height, lean) = state.interpolated(fixed_time.overstep_fraction());

    // Look angles come straight from input (not the fixed tick), so turning
    // is never delayed or stepped.
    let look = Quat::from_euler(EulerRot::YXZ, input.yaw, input.pitch, 0.0);
    let right = Quat::from_rotation_y(input.yaw) * Vec3::X;
    let eye = body.translation
        + Vec3::Y * (height - tuning.body.eye_from_top)
        + right * (lean * tuning.lean.offset);

    if view.third_person {
        transform.translation = eye + look * THIRD_PERSON_OFFSET;
        transform.rotation = look;
    } else {
        // Head bob: lowest as each foot lands, swaying side to side once per
        // two steps.
        let v = &tuning.view;
        let up = -0.5 * v.bob_height * bob.amount * (TAU * bob.cycle).cos();
        let side_wave = (PI * bob.cycle).sin() * bob.amount;
        let roll = -lean * tuning.lean.angle.to_radians() + side_wave * v.bob_roll.to_radians();
        transform.translation = eye + Vec3::Y * up + right * (side_wave * v.bob_side);
        transform.rotation = look * Quat::from_rotation_z(roll);
    }

    if let Projection::Perspective(perspective) = projection.as_mut() {
        let fov = tuning.look.fov.to_radians();
        if perspective.fov != fov {
            perspective.fov = fov;
        }
    }
}
