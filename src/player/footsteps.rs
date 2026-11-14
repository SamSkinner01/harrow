//! Footstep sounds for every character, by surface.
//!
//! Each [`Footstep`] (written by the movement code for every character)
//! casts a short ray down from just above the step against world geometry
//! and looks up what was hit ([`SurfaceLookup`]: destructible surfaces know
//! their material, everything else goes by piece/collider name, see
//! `audio.surfaces` in the tuning file). Stone, wood, sand and metal each
//! have their own recordings. Volume follows the step's loudness (sqrt, so
//! a crouch-walk is quiet but audible). The local player's steps play
//! straight to the ears; everyone else's are 3D at their feet.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::{Footstep, LocalPlayer};
use crate::{
    audio::{Category, Cue, Sound, Sounds, SurfaceLookup},
    physics::GameLayer,
};

pub struct FootstepAudioPlugin;

impl Plugin for FootstepAudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, footstep_sounds);
    }
}

/// The ray starts this far above the feet and looks this far down.
const RAY_START: f32 = 0.3;
const RAY_LENGTH: f32 = 1.0;

fn footstep_sounds(
    mut steps: MessageReader<Footstep>,
    local: Query<(), With<LocalPlayer>>,
    spatial: SpatialQuery,
    surfaces: SurfaceLookup,
    mut sounds: Sounds,
) {
    if !sounds.enabled() {
        steps.clear();
        return;
    }
    let filter = SpatialQueryFilter::from_mask(GameLayer::World);
    for step in steps.read() {
        if step.loudness <= 0.0 {
            continue;
        }
        let origin = step.position + Vec3::Y * RAY_START;
        let surface = match spatial.cast_ray(origin, Dir3::NEG_Y, RAY_LENGTH, true, &filter) {
            Some(hit) => surfaces.material(hit.entity, origin - Vec3::Y * hit.distance, origin),
            None => surfaces.fallback(),
        };
        let mut sound = Sound::new("footstep", Cue::Step(surface.material), Category::Footsteps)
            .gain(step.loudness.sqrt())
            .detail(format!(
                "{}, loudness {:.2}",
                surface.describe(),
                step.loudness
            ));
        if !local.contains(step.entity) {
            sound = sound.at(step.position + Vec3::Y * 0.05);
        }
        sounds.play(sound);
    }
}
