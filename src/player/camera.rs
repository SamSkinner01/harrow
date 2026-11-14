//! First-person camera. Presentation only: reads the player's simulation
//! state every frame (interpolated between fixed ticks) and never changes
//! gameplay state.

use bevy::prelude::*;

use super::{CharacterState, LocalPlayer, PlayerInput};
use crate::{config::Tuning, debug::DebugView};

pub struct PlayerCameraPlugin;

impl Plugin for PlayerCameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_camera)
            .add_systems(Update, follow_local_player);
    }
}

#[derive(Component)]
pub struct PlayerCamera;

fn spawn_camera(mut commands: Commands, tuning: Res<Tuning>) {
    commands.spawn((
        Name::new("Player camera"),
        PlayerCamera,
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: tuning.look.fov.to_radians(),
            near: 0.05,
            ..default()
        }),
    ));
}

/// Third-person debug camera offset in look space: right, up, back.
const THIRD_PERSON_OFFSET: Vec3 = Vec3::new(0.7, 0.3, 3.2);

fn follow_local_player(
    player: Single<(&Transform, &PlayerInput, &CharacterState), With<LocalPlayer>>,
    camera: Single<(&mut Transform, &mut Projection), (With<PlayerCamera>, Without<LocalPlayer>)>,
    tuning: Res<Tuning>,
    fixed_time: Res<Time<Fixed>>,
    view: Res<DebugView>,
) {
    let (body, input, state) = player.into_inner();
    let (mut transform, mut projection) = camera.into_inner();

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
        let roll = -lean * tuning.lean.angle.to_radians();
        transform.translation = eye;
        transform.rotation = look * Quat::from_rotation_z(roll);
    }

    if let Projection::Perspective(perspective) = projection.as_mut() {
        let fov = tuning.look.fov.to_radians();
        if perspective.fov != fov {
            perspective.fov = fov;
        }
    }
}
