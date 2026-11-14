//! Fixed-tick character simulation: walking, sprinting, gravity, collision.
//!
//! Uses Avian's move-and-slide on a kinematic capsule. The body's transform
//! origin is at the feet; the capsule centre is half the current height above.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::{CharacterState, Player, PlayerInput};
use crate::{config::Tuning, physics::GameLayer};

pub struct MovementPlugin;

impl Plugin for MovementPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, move_characters);
    }
}

/// Fastest a character can fall, m/s.
const TERMINAL_VELOCITY: f32 = 50.0;

fn move_characters(
    mut characters: Query<
        (
            Entity,
            &PlayerInput,
            &mut CharacterState,
            &mut Transform,
            &mut LinearVelocity,
        ),
        With<Player>,
    >,
    move_and_slide: MoveAndSlide,
    tuning: Res<Tuning>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    let m = &tuning.movement;
    let max_slope = m.max_slope.to_radians();

    for (entity, input, mut state, mut transform, mut velocity) in &mut characters {
        let radius = tuning.body.radius;
        let shape = capsule(radius, state.height);
        let to_center = Vec3::Y * state.height / 2.0;
        let filter = SpatialQueryFilter::from_mask([GameLayer::World, GameLayer::Character])
            .with_excluded_entities([entity]);

        // --- Horizontal: accelerate toward the wished-for velocity ---------
        let facing = Quat::from_rotation_y(input.yaw);
        let wish = facing * Vec3::new(input.move_axis.x, 0.0, -input.move_axis.y);
        let sprinting = input.sprint && input.move_axis.y > 0.5;
        let speed = if sprinting { m.sprint_speed } else { m.walk_speed };
        let target = wish * speed;
        let horizontal = Vec3::new(velocity.x, 0.0, velocity.z);
        let rate = if target.length_squared() >= horizontal.length_squared() {
            m.acceleration
        } else {
            m.deceleration
        };
        let horizontal = horizontal.move_towards(target, rate * dt);

        // --- Vertical: gravity ----------------------------------------------
        let vertical = (velocity.y - m.gravity * dt).max(-TERMINAL_VELOCITY);
        let desired = Vec3::new(horizontal.x, vertical, horizontal.z);

        // --- Collide and slide ------------------------------------------------
        let mut hit_ground = false;
        let output = move_and_slide.move_and_slide(
            &shape,
            transform.translation + to_center,
            Quat::IDENTITY,
            desired,
            time.delta(),
            &MoveAndSlideConfig::default(),
            &filter,
            |hit| {
                let normal = **hit.normal;
                if normal.angle_between(Vec3::Y) <= max_slope {
                    hit_ground = true;
                    // On walkable ground: keep the full horizontal speed and
                    // tilt it to follow the slope. Gravity never makes you
                    // slide, and ramps don't slow you down.
                    let h = Vec3::new(hit.velocity.x, 0.0, hit.velocity.z);
                    let rise = -(normal.x * h.x + normal.z * h.z) / normal.y;
                    *hit.velocity = h + Vec3::Y * rise;
                }
                MoveAndSlideHitResponse::Accept
            },
        );
        let mut position = output.position - to_center;
        let mut new_velocity = output.projected_velocity;

        // --- Ground snap: stay glued to ramps when walking down them ---------
        if !hit_ground && state.grounded && new_velocity.y <= 0.0 {
            if let Some(hit) = move_and_slide.spatial_query.cast_shape(
                &shape,
                position + to_center,
                Quat::IDENTITY,
                Dir3::NEG_Y,
                &ShapeCastConfig::from_max_distance(m.ground_snap),
                &filter,
            ) && hit.normal1.angle_between(Vec3::Y) <= max_slope
            {
                position.y -= hit.distance;
                hit_ground = true;
            }
        }
        if hit_ground {
            new_velocity.y = 0.0;
        }

        transform.translation = position;
        transform.rotation = facing;
        velocity.0 = new_velocity;
        state.grounded = hit_ground;
    }
}

/// Upright capsule of total height `height`.
fn capsule(radius: f32, height: f32) -> Collider {
    Collider::capsule(radius, (height - 2.0 * radius).max(0.0))
}
