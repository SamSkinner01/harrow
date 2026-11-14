//! Hit zones: sensor colliders that bullets will test against (M3).
//!
//! They are separate from the movement capsule so that stance and lean move
//! what can be shot: leaning exposes your head and part of your torso past a
//! corner, crouching folds the legs forward, and prone lays everything flat.
//!
//! Zone sizes are fixed; only their placement changes. Placement is computed
//! every tick from [`CharacterState`] in the body's local space (origin at
//! the feet, -Z forward, +X right).
//!
//! Known limitation: the movement capsule stays upright when prone, so the
//! prone legs (behind) and head (in front) can poke through walls. A proper
//! prone collider comes with character models (M5).

use std::f32::consts::FRAC_PI_2;

use avian3d::prelude::*;
use bevy::prelude::*;

use super::{CharacterState, Player};
use crate::{config::Tuning, physics::GameLayer};

pub struct HitboxPlugin;

impl Plugin for HitboxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            update_hitboxes.after(super::movement::move_characters),
        );
    }
}

/// Body region a hit zone belongs to; decides damage in M3.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitZone {
    Head,
    Torso,
    Legs,
}

impl HitZone {
    /// Capsule radius and cylinder length (length 0 = sphere).
    pub fn shape(self) -> (f32, f32) {
        match self {
            HitZone::Head => (0.12, 0.0),
            HitZone::Torso => (0.2, 0.3),
            HitZone::Legs => (0.15, 0.5),
        }
    }
}

pub(super) fn spawn_hitboxes(parent: &mut ChildSpawnerCommands) {
    for zone in [HitZone::Head, HitZone::Torso, HitZone::Legs] {
        let (radius, length) = zone.shape();
        parent.spawn((
            Name::new(format!("{zone:?} hit zone")),
            zone,
            Collider::capsule(radius, length),
            Sensor,
            // Only spatial queries (bullets) find these; they never collide.
            CollisionLayers::new(GameLayer::Hitbox, LayerMask::NONE),
            Transform::default(),
        ));
    }
}

fn update_hitboxes(
    players: Query<&CharacterState, With<Player>>,
    mut zones: Query<(&HitZone, &ChildOf, &mut Transform)>,
    tuning: Res<Tuning>,
) {
    for (zone, child_of, mut transform) in &mut zones {
        if let Ok(state) = players.get(child_of.parent()) {
            *transform = zone_pose(*zone, state.height, state.lean, &tuning);
        }
    }
}

/// Where a zone sits for a given body height and lean.
pub fn zone_pose(zone: HitZone, height: f32, lean: f32, tuning: &Tuning) -> Transform {
    let body = &tuning.body;
    // 0 standing .. 1 fully crouched; 0 crouched or taller .. 1 fully prone.
    let crouch = ((body.stand_height - height) / (body.stand_height - body.crouch_height))
        .clamp(0.0, 1.0);
    let prone = ((body.crouch_height - height) / (body.crouch_height - body.prone_height))
        .clamp(0.0, 1.0);

    let lean_shift = lean * tuning.lean.offset;
    let lean_roll = Quat::from_rotation_z(-lean * tuning.lean.angle.to_radians());
    let lie_flat = Quat::from_rotation_x(FRAC_PI_2);

    let (upright, flat) = match zone {
        // Top of the head 2 cm below the top of the body; moves the full
        // lean distance.
        HitZone::Head => (
            Transform::from_xyz(lean_shift, height - 0.14, 0.0),
            Transform::from_xyz(0.0, 0.18, -0.45),
        ),
        // Shoulders ~0.28 m below the top; tilts with the lean, pivoting
        // near the hips, so it shifts about half as far as the head.
        HitZone::Torso => (
            Transform::from_xyz(lean_shift * 0.5, height - 0.63, 0.0).with_rotation(lean_roll),
            Transform::from_xyz(0.0, 0.2, 0.05).with_rotation(lie_flat),
        ),
        // Standing: upright under the torso. Crouched: thighs folded forward.
        HitZone::Legs => {
            let standing = Transform::from_xyz(0.0, 0.4, 0.0);
            let crouched = Transform::from_xyz(0.0, 0.3, -0.15).with_rotation(lie_flat);
            (
                blend(standing, crouched, crouch),
                Transform::from_xyz(0.0, 0.15, 0.75).with_rotation(lie_flat),
            )
        }
    };
    blend(upright, flat, prone)
}

fn blend(a: Transform, b: Transform, t: f32) -> Transform {
    Transform {
        translation: a.translation.lerp(b.translation, t),
        rotation: a.rotation.slerp(b.rotation, t),
        scale: Vec3::ONE,
    }
}
