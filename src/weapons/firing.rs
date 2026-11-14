//! Fixed-tick weapon simulation: switching, reloading, aiming, fire rate,
//! recoil, spread, and turning hitscan results into damage.
//!
//! Rules are summarised in DESIGN.md ("M3 weapons"); every number comes from
//! the `weapons` section of the tuning file.

use std::f32::consts::TAU;

use avian3d::prelude::*;
use bevy::prelude::*;

use super::{
    Ammo, BulletHit, MoveSpeedScale, ShotFired, Slot, WeaponAction, WeaponEvent, WeaponEventKind,
    WeaponInput, WeaponState, WeaponSystems,
    ballistics::{self, TraceWorld},
};
use crate::{
    combat::{DamageKind, DamageMessage, Dead, Health, SurfaceImpact, SurfaceKind},
    config::{Tuning, WeaponDef},
    player::{CharacterState, HitZone, PlayerInput},
};

pub struct FiringPlugin;

impl Plugin for FiringPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            simulate_weapons.in_set(WeaponSystems::Simulate),
        );
    }
}

/// Matches the player input's pitch limit.
const PITCH_LIMIT: f32 = 89.0_f32.to_radians();

/// The true eye position, computed exactly like the player camera does:
/// top of the body minus `eye_from_top`, shifted sideways by the lean.
pub fn eye_position(feet: Vec3, yaw: f32, height: f32, lean: f32, tuning: &Tuning) -> Vec3 {
    let right = Quat::from_rotation_y(yaw) * Vec3::X;
    feet + Vec3::Y * (height - tuning.body.eye_from_top) + right * (lean * tuning.lean.offset)
}

/// Aim rotation: look angles plus recoil (degrees, right/up).
pub fn aim_rotation(yaw: f32, pitch: f32, recoil: Vec2) -> Quat {
    let yaw = yaw - recoil.x.to_radians();
    let pitch = (pitch + recoil.y.to_radians()).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0)
}

/// Kick of shot number `index` (0-based) in a burst, degrees (right, up).
pub fn pattern_kick(pattern: &[(f32, f32)], repeat_last: usize, index: usize) -> Vec2 {
    if pattern.is_empty() {
        return Vec2::ZERO;
    }
    let len = pattern.len();
    let i = if index < len {
        index
    } else {
        let tail = repeat_last.clamp(1, len);
        len - tail + (index - len) % tail
    };
    Vec2::new(pattern[i].0, pattern[i].1)
}

/// Two uniform numbers in 0..1 for shot `n`: deterministic, so a server or
/// replay gets the same spread from the same shot count.
fn shot_random(n: u32, slot: Slot) -> (f32, f32) {
    let mut x = (n as u64) << 1 | slot.index() as u64;
    let mut next = || {
        // splitmix64
        x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 40) as f32 / (1u64 << 24) as f32
    };
    (next(), next())
}

/// Spread cone radius in degrees for the next shot.
pub fn current_spread(
    def: &WeaponDef,
    state: &WeaponState,
    body: &CharacterState,
    horizontal_speed: f32,
    tuning: &Tuning,
    first_shot: bool,
) -> f32 {
    let s = &def.spread;
    let b = &tuning.body;
    let aim = state.aim;
    let base = s.hip.lerp(s.ads, aim);
    let speed = (horizontal_speed / tuning.movement.walk_speed).clamp(0.0, 1.5);
    let moving = s.moving_hip.lerp(s.moving_ads, aim) * speed;
    // Blend by body height, like the hit zones: 0 standing .. 1 crouched,
    // 0 crouched .. 1 prone.
    let crouch = ((b.stand_height - body.height) / (b.stand_height - b.crouch_height)).clamp(0.0, 1.0);
    let prone = ((b.crouch_height - body.height) / (b.crouch_height - b.prone_height)).clamp(0.0, 1.0);
    let stance = 1.0_f32.lerp(s.crouch_scale, crouch).lerp(s.prone_scale, prone);
    let bloom = state.bloom * 1.0_f32.lerp(s.ads_bloom_scale, aim);
    let mut spread = (base + moving + bloom) * stance;
    if first_shot {
        spread *= s.first_shot_scale;
    }
    spread.max(0.0)
}

/// Damage multiplier for distance travelled.
pub fn falloff(def: &WeaponDef, distance: f32) -> f32 {
    if distance <= def.falloff_start {
        return 1.0;
    }
    let span = (def.falloff_end - def.falloff_start).max(1e-3);
    let t = ((distance - def.falloff_start) / span).clamp(0.0, 1.0);
    1.0_f32.lerp(def.falloff_min, t)
}

#[derive(bevy::ecs::system::SystemParam)]
struct Outputs<'w> {
    damage: MessageWriter<'w, DamageMessage>,
    impacts: MessageWriter<'w, SurfaceImpact>,
    shots: MessageWriter<'w, ShotFired>,
    events: MessageWriter<'w, WeaponEvent>,
    hits: MessageWriter<'w, BulletHit>,
}

#[derive(bevy::ecs::system::SystemParam)]
struct Targets<'w, 's> {
    spatial: SpatialQuery<'w, 's>,
    colliders: Query<'w, 's, &'static ColliderOf>,
    zones: Query<'w, 's, &'static HitZone>,
    surfaces: Query<'w, 's, &'static SurfaceKind>,
    health: Query<'w, 's, &'static Health, Without<Dead>>,
}

#[allow(clippy::type_complexity)]
fn simulate_weapons(
    mut characters: Query<(
        Entity,
        &WeaponInput,
        &PlayerInput,
        &CharacterState,
        &Transform,
        &LinearVelocity,
        &mut WeaponState,
        &mut MoveSpeedScale,
        Has<Dead>,
    )>,
    world: Targets,
    mut out: Outputs,
    tuning: Res<Tuning>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    let w = &tuning.weapons;
    let trace_world = TraceWorld {
        spatial: &world.spatial,
        colliders: &world.colliders,
        zones: &world.zones,
        surfaces: &world.surfaces,
    };

    for (entity, input, look, body, transform, velocity, mut state, mut speed_scale, dead) in
        &mut characters
    {
        // Edge-triggered requests: compare the input's counters with what
        // was already handled.
        let presses = input.trigger_presses.wrapping_sub(state.seen_trigger);
        state.seen_trigger = input.trigger_presses;
        let reload_requested = input.reload_requests != state.seen_reload;
        state.seen_reload = input.reload_requests;

        state.prev_aim = state.aim;
        state.prev_recoil = state.recoil;

        if dead {
            state.aim = 0.0;
            state.queued_shot = false;
            speed_scale.0 = 1.0;
            continue;
        }

        // --- Recovery from earlier shots ----------------------------------
        let def = state.active.def(&tuning).clone();
        state.since_shot += dt;
        state.cooldown -= dt;
        // Recoil and bloom only recover once you stop firing.
        if state.since_shot > def.recoil.recovery_delay {
            state.recoil = state
                .recoil
                .move_towards(Vec2::ZERO, def.recoil.recovery_speed * dt);
            state.bloom = (state.bloom - def.spread.bloom_recovery * dt).max(0.0);
        }

        if body.sprinting {
            state.sprint_lock = w.sprint_recovery;
        } else {
            state.sprint_lock = (state.sprint_lock - dt).max(0.0);
        }
        let sprint_blocked = body.sprinting || state.sprint_lock > 0.0;

        // --- Switching ------------------------------------------------------
        match state.action {
            WeaponAction::Holstering { to, .. } if to == input.slot => {}
            WeaponAction::Holstering { .. } if input.slot == state.active => {
                // Changed their mind before the old weapon was down.
                state.action = WeaponAction::Ready;
            }
            _ if input.slot != state.active => {
                state.action = WeaponAction::Holstering {
                    elapsed: 0.0,
                    duration: w.holster_time,
                    to: input.slot,
                };
                state.queued_shot = false;
                out.events.write(WeaponEvent {
                    shooter: entity,
                    slot: state.active,
                    kind: WeaponEventKind::Holster,
                });
            }
            _ => {}
        }

        // --- Timed actions ----------------------------------------------------
        let mut finished = None;
        match &mut state.action {
            WeaponAction::Ready => {}
            WeaponAction::Reloading {
                elapsed, duration, ..
            }
            | WeaponAction::Holstering {
                elapsed, duration, ..
            }
            | WeaponAction::Drawing { elapsed, duration } => {
                *elapsed += dt;
                if *elapsed >= *duration {
                    finished = Some(state.action);
                }
            }
        }
        match finished {
            Some(WeaponAction::Reloading { empty, .. }) => {
                let index = state.active.index();
                let ammo = &mut state.ammo[index];
                let capacity = def.magazine + u32::from(def.chamber && !empty);
                let taken = capacity.saturating_sub(ammo.magazine).min(ammo.reserve);
                ammo.magazine += taken;
                ammo.reserve -= taken;
                state.action = WeaponAction::Ready;
                out.events.write(WeaponEvent {
                    shooter: entity,
                    slot: state.active,
                    kind: WeaponEventKind::ReloadDone,
                });
            }
            Some(WeaponAction::Holstering { to, .. }) => {
                state.active = to;
                state.burst = 0;
                state.bloom = 0.0;
                state.action = WeaponAction::Drawing {
                    elapsed: 0.0,
                    duration: to.def(&tuning).draw_time,
                };
                out.events.write(WeaponEvent {
                    shooter: entity,
                    slot: to,
                    kind: WeaponEventKind::Draw,
                });
            }
            Some(WeaponAction::Drawing { .. }) => state.action = WeaponAction::Ready,
            _ => {}
        }
        // The active weapon may have changed.
        let def = state.active.def(&tuning).clone();
        let slot = state.active;

        // --- Reload -----------------------------------------------------------
        let ammo = state.ammo();
        let capacity = def.magazine + u32::from(def.chamber && ammo.magazine > 0);
        if reload_requested
            && state.action == WeaponAction::Ready
            && ammo.magazine < capacity
            && ammo.reserve > 0
        {
            start_reload(&mut state, &def, entity, &mut out.events);
        }

        // --- Aim down sights --------------------------------------------------
        let can_aim = !sprint_blocked
            && matches!(state.action, WeaponAction::Ready | WeaponAction::Drawing { .. });
        let aim_target = if input.aim && can_aim { 1.0 } else { 0.0 };
        state.aim = state
            .aim
            .move_towards(aim_target, dt / def.ads_time.max(1e-3));
        speed_scale.0 = 1.0_f32.lerp(def.ads_move_speed, state.aim);

        // --- Fire ---------------------------------------------------------------
        if !input.trigger {
            state.dry_fired = false;
        }
        if presses > 0 {
            state.queued_shot = true;
        }
        let wants_fire = if def.full_auto {
            input.trigger || state.queued_shot
        } else {
            state.queued_shot
        };
        let can_fire = state.action == WeaponAction::Ready && !sprint_blocked;
        if !wants_fire || !can_fire {
            // No banking of fire-rate time while not shooting.
            state.cooldown = state.cooldown.max(0.0);
        }
        if wants_fire && can_fire && state.cooldown <= 0.0 {
            state.queued_shot = false;
            if state.ammo().magazine == 0 {
                if !state.dry_fired || presses > 0 {
                    state.dry_fired = true;
                    out.events.write(WeaponEvent {
                        shooter: entity,
                        slot,
                        kind: WeaponEventKind::DryFire,
                    });
                    if w.auto_reload && state.ammo().reserve > 0 {
                        start_reload(&mut state, &def, entity, &mut out.events);
                    }
                }
                state.cooldown = state.cooldown.max(0.0);
            } else {
                let shot = Shot {
                    shooter: entity,
                    slot,
                    feet: transform.translation,
                    look,
                    body,
                    horizontal_speed: velocity.0.with_y(0.0).length(),
                };
                fire_one(&shot, &def, &mut state, &trace_world, &world.health, &mut out, &tuning);
                state.cooldown += 60.0 / def.rpm.max(1.0);
            }
        }
    }
}

fn start_reload(
    state: &mut WeaponState,
    def: &WeaponDef,
    shooter: Entity,
    events: &mut MessageWriter<WeaponEvent>,
) {
    let empty = state.ammo().magazine == 0;
    state.action = WeaponAction::Reloading {
        elapsed: 0.0,
        duration: if empty {
            def.reload_empty
        } else {
            def.reload_tactical
        },
        empty,
    };
    state.queued_shot = false;
    events.write(WeaponEvent {
        shooter,
        slot: state.active,
        kind: WeaponEventKind::ReloadStart { empty },
    });
}

struct Shot<'a> {
    shooter: Entity,
    slot: Slot,
    feet: Vec3,
    look: &'a PlayerInput,
    body: &'a CharacterState,
    horizontal_speed: f32,
}

fn fire_one(
    shot: &Shot,
    def: &WeaponDef,
    state: &mut WeaponState,
    world: &TraceWorld,
    health: &Query<&Health, Without<Dead>>,
    out: &mut Outputs,
    tuning: &Tuning,
) {
    let first_shot = state.since_shot >= def.spread.first_shot_reset;
    if first_shot {
        state.burst = 0;
    }

    // Direction: look angles + recoil + a random point in the spread cone.
    let spread = current_spread(def, state, shot.body, shot.horizontal_speed, tuning, first_shot);
    let (u1, u2) = shot_random(state.shots_fired, shot.slot);
    let cone = spread.to_radians() * u1.sqrt(); // uniform over the cone's disc
    let around = TAU * u2;
    let local = Vec3::new(
        cone.sin() * around.cos(),
        cone.sin() * around.sin(),
        -cone.cos(),
    );
    let rotation = aim_rotation(shot.look.yaw, shot.look.pitch, state.recoil);
    let direction = Dir3::new(rotation * local).unwrap_or(Dir3::NEG_Z);
    let origin = eye_position(
        shot.feet,
        shot.look.yaw,
        shot.body.height,
        shot.body.lean,
        tuning,
    );

    let trace = ballistics::trace(world, shot.shooter, origin, direction, &tuning.weapons.penetration);
    for (impact, _) in &trace.impacts {
        out.impacts.write(impact.clone());
    }
    if let Some(hit) = trace.hit
        && let Ok(target_health) = health.get(hit.target)
    {
        let zones = &tuning.weapons.damage;
        let base = def.damage * hit.damage_scale * falloff(def, hit.distance);
        let amount = match hit.zone {
            HitZone::Head if zones.headshot_kills => target_health.current,
            HitZone::Head => base * zones.head,
            HitZone::Torso => base * zones.torso,
            HitZone::Legs => base * zones.legs,
        };
        out.damage.write(DamageMessage {
            target: hit.target,
            amount,
            kind: DamageKind::Bullet,
            zone: Some(hit.zone),
            source: Some(shot.shooter),
        });
        out.hits.write(BulletHit {
            shooter: shot.shooter,
            target: hit.target,
            zone: hit.zone,
            point: hit.point,
            damage: amount,
            distance: hit.distance,
            layers: hit.layers,
            lethal: amount >= target_health.current,
        });
    }

    let ammo: &mut Ammo = &mut state.ammo[shot.slot.index()];
    ammo.magazine -= 1;
    out.shots.write(ShotFired {
        shooter: shot.shooter,
        slot: shot.slot,
        origin,
        direction: *direction,
        end: trace.end,
        spread,
    });

    // Recoil applies after the shot: the first bullet goes where you aim.
    let kick = pattern_kick(&def.recoil.pattern, def.recoil.repeat_last, state.burst as usize)
        * 1.0_f32.lerp(def.recoil.ads_scale, state.aim);
    state.recoil += kick;
    state.burst += 1;
    state.bloom = (state.bloom + def.spread.per_shot).min(def.spread.max_bloom);
    state.since_shot = 0.0;
    state.shots_fired = state.shots_fired.wrapping_add(1);
}

