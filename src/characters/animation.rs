//! Drives each character's animation graph from [`CharacterAnim`].
//!
//! Every clip plays all the time (so walk and run stay in step); only the
//! weights change. Per frame:
//!
//! - **Stance** (stand / crouch / prone) is smoothed into three weights.
//! - **Speed** picks the clips within a stance: idle -> walk -> run,
//!   crouch idle -> crouch walk, prone idle -> crawl, keyed to the game's
//!   movement speeds ([`Clip::game_speed`]). Moving clips play at movement
//!   speed / the speed their stance feet move at (`harrow_clips`), so the
//!   feet don't slide.
//! - **Aiming / firing** blends the `fire` clip over the arms, chest and
//!   head (aim pose, cheek on the stock; firing plays its recoil kick on a
//!   loop at [`FIRE_RATE`]).
//! - **Reloading** restarts `reload` on the arms, chest and head when it
//!   turns on.
//! - **Lean** blends `lean_left` / `lean_right` over the lower spine.
//!
//! Arm and spine overrides are off while prone (there are no prone aiming
//! or reload clips yet).
//!
//! Blending maths: Bevy averages each bone over the clips that animate it,
//! weighted. The locomotion weights sum to 1, so an override clip with
//! weight `w / (1 - w)` ends up with exactly fraction `w` of its bones.

use bevy::{animation::RepeatAnimation, prelude::*};

use super::{
    Character, CharacterParts,
    library::{CharacterLibrary, Clip, OperatorRig},
};
use crate::player::Stance;

/// Shots per second the `fire` clip's recoil loop is played at.
pub const FIRE_RATE: f32 = 10.0;

pub(super) struct AnimationPlugin;

impl Plugin for AnimationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (bind_graphs, drive_animation).chain());
    }
}

/// What a character is doing, for its animation. Written every frame by
/// whoever controls the character (bot, network, third-person view).
#[derive(Component, Clone, Debug, Default)]
pub struct CharacterAnim {
    /// Ground speed, m/s (forward; strafing and backpedalling use the same
    /// clips for now).
    pub speed: f32,
    pub stance: Stance,
    /// -1 full left .. 1 full right (same convention as the player's lean).
    pub lean: f32,
    /// Weapon at the shoulder (aim-down-sights).
    pub aiming: bool,
    /// Shooting: aims and plays the recoil kick at [`FIRE_RATE`].
    pub firing: bool,
    /// Reloading: the reload clip plays from the start when this turns on.
    pub reloading: bool,
}

/// Smoothed blend state.
#[derive(Component, Default)]
pub(super) struct AnimBlend {
    /// Stand, crouch, prone.
    stance: [f32; 3],
    speed: f32,
    lean: f32,
    aim: f32,
    reload: f32,
    reloading: bool,
}

/// Seconds to blend between stances / for the arms / for leaning.
const STANCE_TIME: [f32; 3] = [0.15, 0.15, 0.35];
const ARMS_TIME: f32 = 0.1;
const LEAN_TIME: f32 = 0.08;
const SPEED_TIME: f32 = 0.12;

fn bind_graphs(
    characters: Query<(Entity, &Character, &CharacterParts), Without<AnimBlend>>,
    mut players: Query<&mut AnimationPlayer>,
    library: Res<CharacterLibrary>,
    mut commands: Commands,
) {
    for (entity, character, parts) in &characters {
        let Some(rig) = library.rigs.get(&character.operator) else {
            continue;
        };
        let Ok(mut player) = players.get_mut(parts.player) else {
            continue;
        };
        for clip in Clip::ALL {
            let active = player.play(rig.node(clip));
            active.set_weight(0.0);
            if clip != Clip::Reload {
                active.repeat();
            }
        }
        if let Some(fire) = player.animation_mut(rig.node(Clip::Fire)) {
            fire.pause();
        }
        if let Some(reload) = player.animation_mut(rig.node(Clip::Reload)) {
            reload.set_repeat(RepeatAnimation::Never).pause();
        }
        commands
            .entity(parts.player)
            .insert(AnimationGraphHandle(rig.graph.clone()));
        commands.entity(entity).insert(AnimBlend {
            stance: [1.0, 0.0, 0.0],
            ..default()
        });
    }
}

fn drive_animation(
    mut characters: Query<(&Character, &CharacterParts, &CharacterAnim, &mut AnimBlend)>,
    mut players: Query<&mut AnimationPlayer>,
    library: Res<CharacterLibrary>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    let approach = |current: &mut f32, target: f32, seconds: f32| {
        *current += (target - *current) * (1.0 - (-dt / seconds.max(1e-3)).exp());
    };
    for (character, parts, anim, mut blend) in &mut characters {
        let Some(rig) = library.rigs.get(&character.operator) else {
            continue;
        };
        let Ok(mut player) = players.get_mut(parts.player) else {
            continue;
        };
        let target = match anim.stance {
            Stance::Stand => 0,
            Stance::Crouch => 1,
            Stance::Prone => 2,
        };
        for i in 0..3 {
            let goal = if i == target { 1.0 } else { 0.0 };
            approach(&mut blend.stance[i], goal, STANCE_TIME[i.max(target)]);
        }
        approach(&mut blend.speed, anim.speed.max(0.0), SPEED_TIME);
        approach(&mut blend.lean, anim.lean.clamp(-1.0, 1.0), LEAN_TIME);
        approach(
            &mut blend.aim,
            f32::from(anim.aiming || anim.firing),
            ARMS_TIME,
        );
        approach(&mut blend.reload, f32::from(anim.reloading), ARMS_TIME);

        let weights = clip_weights(&blend);
        for clip in Clip::ALL {
            if let Some(active) = player.animation_mut(rig.node(clip)) {
                active.set_weight(weights[clip as usize]);
            }
        }
        set_playback_speeds(&mut player, rig, blend.speed);

        // Fire: run the recoil loop while firing; otherwise finish the
        // current kick and hold the aim pose at the start of the clip.
        if let Some(fire) = player.animation_mut(rig.node(Clip::Fire)) {
            if anim.firing {
                fire.set_speed(FIRE_RATE * FIRE_CLIP_LENGTH).resume();
            } else if !fire.is_paused() && (fire.just_completed() || fire.seek_time() < 0.01) {
                fire.pause().seek_to(0.0);
            }
        }
        if anim.reloading && !blend.reloading {
            player
                .start(rig.node(Clip::Reload))
                .set_repeat(RepeatAnimation::Never)
                .resume();
        }
        blend.reloading = anim.reloading;
    }
}

/// Length of the `fire` clip (one shot), seconds.
const FIRE_CLIP_LENGTH: f32 = 0.2;

fn clip_weights(blend: &AnimBlend) -> [f32; Clip::COUNT] {
    let mut w = [0.0; Clip::COUNT];
    let total = blend.stance.iter().sum::<f32>().max(1e-3);
    let [stand, crouch, prone] = blend.stance.map(|x| x / total);
    let s = blend.speed;

    // Stand: idle until 60% of walking speed is reached, walk, then run.
    let walk = Clip::Walk.game_speed();
    let run = Clip::Run.game_speed();
    let moving = (s / (0.6 * walk)).clamp(0.0, 1.0);
    let running = ((s - walk) / (run - walk)).clamp(0.0, 1.0);
    w[Clip::Idle as usize] = stand * (1.0 - moving);
    w[Clip::Walk as usize] = stand * moving * (1.0 - running);
    w[Clip::Run as usize] = stand * moving * running;

    let crouch_moving = (s / (0.6 * Clip::CrouchWalk.game_speed())).clamp(0.0, 1.0);
    w[Clip::CrouchIdle as usize] = crouch * (1.0 - crouch_moving);
    w[Clip::CrouchWalk as usize] = crouch * crouch_moving;

    let crawling = (s / (0.6 * Clip::ProneCrawl.game_speed())).clamp(0.0, 1.0);
    w[Clip::ProneIdle as usize] = prone * (1.0 - crawling);
    w[Clip::ProneCrawl as usize] = prone * crawling;

    // Overrides; the locomotion weights above sum to 1.
    let upright = 1.0 - prone;
    let reload = (blend.reload * upright).min(0.98);
    let aim = (blend.aim * upright * (1.0 - reload)).min(0.98 - reload);
    let arms = aim + reload;
    w[Clip::Fire as usize] = aim / (1.0 - arms);
    w[Clip::Reload as usize] = reload / (1.0 - arms);

    let lean = (blend.lean.abs() * upright).min(0.98);
    let lean_weight = lean / (1.0 - lean);
    if blend.lean < 0.0 {
        w[Clip::LeanLeft as usize] = lean_weight;
    } else {
        w[Clip::LeanRight as usize] = lean_weight;
    }
    w
}

fn set_playback_speeds(player: &mut AnimationPlayer, rig: &OperatorRig, speed: f32) {
    for (clip, min, max) in [
        (Clip::Walk, 0.55, 1.5),
        (Clip::Run, 0.7, 1.4),
        (Clip::CrouchWalk, 0.5, 1.5),
        (Clip::ProneCrawl, 0.5, 1.5),
    ] {
        let authored = rig.speed(clip);
        if authored > 0.0
            && let Some(active) = player.animation_mut(rig.node(clip))
        {
            active.set_speed((speed / authored).clamp(min, max));
        }
    }
}
