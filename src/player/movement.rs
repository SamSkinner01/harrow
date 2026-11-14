//! Fixed-tick character simulation: stances, lean, walking, sprinting,
//! gravity and collision.
//!
//! Uses Avian's move-and-slide on a kinematic capsule. The body's transform
//! origin is at the feet; the capsule centre is half the current height above.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::{CharacterState, Footstep, Lean, Player, PlayerInput, Stance};
use crate::{config::Tuning, net::Replay, physics::GameLayer, round::MovementLock, weapons::MoveSpeedScale};

pub struct MovementPlugin;

impl Plugin for MovementPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, move_characters);
    }
}

/// Fastest a character can fall, m/s.
const TERMINAL_VELOCITY: f32 = 50.0;
/// Radius of the sphere used to check that a lean doesn't push the head
/// through a wall.
const HEAD_CLEARANCE: f32 = 0.14;

/// Everything this reads must be the same on the server and on a client
/// predicting its own character: input, `CharacterState`, `Transform`,
/// `LinearVelocity` (all networked, see `src/net/`), `MovementLock`
/// (replicated) and the movement tuning (sent by the server).
#[allow(clippy::too_many_arguments)]
pub(super) fn move_characters(
    mut characters: Query<
        (
            Entity,
            &PlayerInput,
            &mut CharacterState,
            &mut Transform,
            &mut LinearVelocity,
            Has<MovementLock>,
            Option<&MoveSpeedScale>,
        ),
        With<Player>,
    >,
    move_and_slide: MoveAndSlide,
    mut footsteps: MessageWriter<Footstep>,
    replay: Replay,
    tuning: Res<Tuning>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    let (m, body) = (&tuning.movement, &tuning.body);
    let max_slope = m.max_slope.to_radians();
    let spatial = &move_and_slide.spatial_query;

    for (entity, input, mut state, mut transform, mut velocity, locked, speed_scale) in &mut characters {
        state.prev_height = state.height;
        state.prev_lean = state.lean;
        // Locked (planting, dead, choosing an operator...): look and stance
        // still work, walking doesn't.
        let (move_axis, sprint) = if locked {
            (Vec2::ZERO, false)
        } else {
            (input.move_axis, input.sprint)
        };

        let filter = SpatialQueryFilter::from_mask([GameLayer::World, GameLayer::Character])
            .with_excluded_entities([entity]);
        let feet = transform.translation;
        let facing = Quat::from_rotation_y(input.yaw);

        // --- Stance ---------------------------------------------------------
        // Only start rising if the full target height fits; otherwise stay
        // put (no half-standing under a low ceiling).
        let target_height = body.height_for(input.stance);
        let rising = target_height > state.height;
        let fits = !rising || {
            // Slightly thinner so standing next to a wall isn't "blocked".
            let probe = capsule(body.radius - 0.02, target_height);
            spatial
                .shape_intersections(&probe, feet + Vec3::Y * target_height / 2.0, Quat::IDENTITY, &filter)
                .is_empty()
        };
        if fits {
            let involves_prone = input.stance == Stance::Prone || state.stance == Stance::Prone;
            let rate = if involves_prone {
                (body.stand_height - body.prone_height) / body.prone_time
            } else {
                (body.stand_height - body.crouch_height) / body.crouch_time
            };
            state.height = state.height.move_towards(target_height, rate * dt);
            state.stance = input.stance;
        }

        // --- Lean -------------------------------------------------------------
        let lean_target = match (state.stance, input.lean) {
            (Stance::Prone, _) | (_, Lean::Center) => 0.0,
            (_, Lean::Left) => -1.0,
            (_, Lean::Right) => 1.0,
        };
        let mut lean = state.lean.move_towards(lean_target, dt / tuning.lean.time);
        if lean != 0.0 {
            // Stop the head at walls: sweep a head-sized sphere sideways from
            // the upright eye position.
            let side = facing * Vec3::X * lean.signum();
            let eye = feet + Vec3::Y * (state.height - body.eye_from_top);
            let wanted = lean.abs() * tuning.lean.offset;
            if let Some(hit) = spatial.cast_shape(
                &Collider::sphere(HEAD_CLEARANCE),
                eye,
                Quat::IDENTITY,
                Dir3::new(side).unwrap_or(Dir3::X),
                &ShapeCastConfig::from_max_distance(wanted),
                &filter,
            ) {
                lean = lean.signum() * hit.distance / tuning.lean.offset;
            }
        }
        state.lean = lean;

        // --- Horizontal: accelerate toward the wished-for velocity ---------
        state.sprinting = sprint
            && move_axis.y > 0.5
            && input.lean == Lean::Center
            && state.stance == Stance::Stand
            && state.height >= body.stand_height - 0.01;
        let speed = if state.sprinting {
            m.sprint_speed
        } else {
            speed_for_height(state.height, &tuning)
        } * speed_scale.map_or(1.0, |s| s.0); // e.g. slower while aiming (weapons)
        let wish = facing * Vec3::new(move_axis.x, 0.0, -move_axis.y);
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
        let shape = capsule(body.radius, state.height);
        let to_center = Vec3::Y * state.height / 2.0;
        let mut hit_ground = false;
        let output = move_and_slide.move_and_slide(
            &shape,
            feet + to_center,
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
            if let Some(hit) = spatial.cast_shape(
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

        // --- Footsteps: one per stride of ground covered ---------------------
        if hit_ground {
            let (stride, loudness) = stride_and_loudness(&state, &tuning);
            state.stride += (position - feet).with_y(0.0).length();
            if state.stride >= stride {
                state.stride -= stride;
                // A networking rollback re-simulates ticks that already
                // made their footsteps.
                if !replay.active() {
                    footsteps.write(Footstep {
                        entity,
                        position,
                        loudness,
                    });
                }
            }
        } else {
            state.stride = 0.0;
        }

        transform.translation = position;
        transform.rotation = facing;
        velocity.0 = new_velocity;
        state.grounded = hit_ground;
    }
}

/// Step length and loudness for the current gait.
pub(super) fn stride_and_loudness(state: &CharacterState, tuning: &Tuning) -> (f32, f32) {
    let f = &tuning.footsteps;
    if state.sprinting {
        return (f.sprint_stride, f.sprint_loudness);
    }
    match state.stance {
        Stance::Stand => (f.walk_stride, f.walk_loudness),
        Stance::Crouch => (f.crouch_stride, f.crouch_loudness),
        Stance::Prone => (f.prone_stride, f.prone_loudness),
    }
}

/// Walking speed for a body height, blending smoothly between stances.
fn speed_for_height(height: f32, tuning: &Tuning) -> f32 {
    let (m, b) = (&tuning.movement, &tuning.body);
    if height >= b.crouch_height {
        let t = (height - b.crouch_height) / (b.stand_height - b.crouch_height);
        m.crouch_speed.lerp(m.walk_speed, t.clamp(0.0, 1.0))
    } else {
        let t = (height - b.prone_height) / (b.crouch_height - b.prone_height);
        m.prone_speed.lerp(m.crouch_speed, t.clamp(0.0, 1.0))
    }
}

/// Upright capsule of total height `height`.
fn capsule(radius: f32, height: f32) -> Collider {
    Collider::capsule(radius, (height - 2.0 * radius).max(0.0))
}
