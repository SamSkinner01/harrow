//! [`AimView`]: what the weapon does to the player's view (ADS zoom and
//! recoil kick), interpolated between fixed ticks. Computed right after the
//! fixed-tick loop each frame, so the camera (in `Update`) always sees this
//! frame's value.

use bevy::prelude::*;

use super::{AimView, WeaponState};
use crate::{config::Tuning, player::LocalPlayer};

pub struct AimViewPlugin;

impl Plugin for AimViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            RunFixedMainLoop,
            update_aim_view.in_set(RunFixedMainLoopSystems::AfterFixedMainLoop),
        );
    }
}

/// Smooth start and stop for the ADS blend.
pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn update_aim_view(
    mut players: Query<(&WeaponState, &mut AimView), With<LocalPlayer>>,
    tuning: Res<Tuning>,
    fixed_time: Res<Time<Fixed>>,
) {
    let t = fixed_time.overstep_fraction();
    for (state, mut view) in &mut players {
        let aim = ease(state.prev_aim.lerp(state.aim, t));
        let recoil = state.prev_recoil.lerp(state.recoil, t);
        let def = state.active.def(&tuning);
        // A real zoom: narrow the field of view so the centre is magnified
        // by `ads_zoom`.
        let fov = tuning.look.fov.to_radians();
        let zoomed = 2.0 * ((fov * 0.5).tan() / def.ads_zoom.max(0.1)).atan();
        *view = AimView {
            fov_scale: 1.0_f32.lerp(zoomed / fov, aim),
            yaw: -recoil.x.to_radians(),
            pitch: recoil.y.to_radians(),
            aim,
        };
    }
}
