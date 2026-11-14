//! First-person camera. Presentation only: reads the (interpolated) player
//! state every frame and never changes gameplay state.

use bevy::prelude::*;

use super::{CharacterState, LocalPlayer, PlayerInput};
use crate::config::Tuning;

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

fn follow_local_player(
    player: Single<(&Transform, &PlayerInput, &CharacterState), With<LocalPlayer>>,
    camera: Single<(&mut Transform, &mut Projection), (With<PlayerCamera>, Without<LocalPlayer>)>,
    tuning: Res<Tuning>,
) {
    let (body, input, state) = player.into_inner();
    let (mut transform, mut projection) = camera.into_inner();

    let eye_height = state.height - tuning.body.eye_from_top;
    transform.translation = body.translation + Vec3::Y * eye_height;
    // Look angles come straight from input (not the fixed tick), so turning
    // is never delayed or stepped.
    transform.rotation = Quat::from_euler(EulerRot::YXZ, input.yaw, input.pitch, 0.0);

    if let Projection::Perspective(perspective) = projection.as_mut() {
        let fov = tuning.look.fov.to_radians();
        if perspective.fov != fov {
            perspective.fov = fov;
        }
    }
}
