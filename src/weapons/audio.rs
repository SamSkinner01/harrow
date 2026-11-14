//! Weapon sounds: shots, the empty click, reloads (magazine out, magazine
//! in, charging handle / slide), draw and holster. Recorded sounds from
//! `assets/audio/weapons`, played through [`crate::audio::Sounds`].
//!
//! The local player's weapon plays straight to the ears (the shot is a
//! stereo first-person recording). Everyone else's is 3D at their position;
//! their shots layer the close recording with a distant one, crossfaded by
//! distance.

use bevy::prelude::*;

use super::{ShotFired, Slot, WeaponEvent, WeaponEventKind};
use crate::{
    audio::{Category, Cue, Sound, Sounds},
    config::Tuning,
    player::LocalPlayer,
};

pub struct WeaponAudioPlugin;

impl Plugin for WeaponAudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (shot_sounds, weapon_event_sounds).chain());
    }
}

/// When each reload sound plays, as a fraction of the reload's duration
/// (`reload_tactical` / `reload_empty` in the tuning file). The first-person
/// reload animation should keep in step with these.
#[derive(Clone, Copy, Debug)]
pub struct ReloadTiming {
    /// The old magazine comes out.
    pub mag_out: f32,
    /// The new magazine seats.
    pub mag_in: f32,
    /// Charging handle / slide (empty reloads only).
    pub rack: f32,
}

pub const RELOAD_TIMING: ReloadTiming = ReloadTiming {
    mag_out: 0.2,
    mag_in: 0.7,
    rack: 0.86,
};

/// Height of the hands above a character's feet, for 3D foley.
const HANDS_HEIGHT: f32 = 1.2;

fn shot_sounds(
    mut shots: MessageReader<ShotFired>,
    local: Query<(), With<LocalPlayer>>,
    mut sounds: Sounds,
) {
    if !sounds.enabled() {
        shots.clear();
        return;
    }
    for shot in shots.read() {
        let (own, world, far, name, gain) = match shot.slot {
            Slot::Primary => (
                Cue::RifleShot,
                Cue::RifleShotWorld,
                Cue::RifleFar,
                "rifle",
                1.0,
            ),
            Slot::Sidearm => (
                Cue::PistolShot,
                Cue::PistolShotWorld,
                Cue::PistolFar,
                "pistol",
                0.85,
            ),
        };
        if local.contains(shot.shooter) {
            sounds.play(
                Sound::new("shot", own, Category::Shots)
                    .gain(gain)
                    .important()
                    .detail(name),
            );
            continue;
        }
        let blend = sounds.distant_blend(shot.origin);
        sounds.play(
            Sound::new("shot", world, Category::Shots)
                .at(shot.origin)
                .gain(gain * (1.0 - 0.7 * blend))
                .detail(name),
        );
        sounds.play(
            Sound::new("shot (distant layer)", far, Category::ShotsDistant)
                .at(shot.origin)
                .gain(gain * (0.25 + 0.75 * blend))
                .detail(name),
        );
    }
}

/// A reload sound waiting for its moment.
struct Pending {
    due: f32,
    shooter: Entity,
    slot: Slot,
    cue: Cue,
    gain: f32,
    what: &'static str,
}

fn weapon_event_sounds(
    mut events: MessageReader<WeaponEvent>,
    local: Query<(), With<LocalPlayer>>,
    characters: Query<&GlobalTransform>,
    tuning: Res<Tuning>,
    time: Res<Time>,
    mut pending: Local<Vec<Pending>>,
    mut sounds: Sounds,
) {
    if !sounds.enabled() {
        events.clear();
        pending.clear();
        return;
    }
    let now = time.elapsed_secs();
    let play = |sounds: &mut Sounds,
                shooter: Entity,
                slot: Slot,
                what: &'static str,
                cue: Cue,
                gain: f32| {
        let mut sound = Sound::new(what, cue, Category::Foley)
            .gain(gain)
            .detail(slot_name(slot));
        if !local.contains(shooter) {
            let Ok(body) = characters.get(shooter) else {
                return;
            };
            sound = sound.at(body.translation() + Vec3::Y * HANDS_HEIGHT);
        }
        sounds.play(sound);
    };
    for event in events.read() {
        let (shooter, slot) = (event.shooter, event.slot);
        match event.kind {
            WeaponEventKind::DryFire => {
                play(&mut sounds, shooter, slot, "dry fire", Cue::DryFire, 0.8)
            }
            WeaponEventKind::ReloadStart { empty } => {
                let def = slot.def(&tuning);
                let duration = if empty {
                    def.reload_empty
                } else {
                    def.reload_tactical
                };
                let (out, inn, rack) = match slot {
                    Slot::Primary => (Cue::RifleMagOut, Cue::RifleMagIn, Cue::RifleCharge),
                    Slot::Sidearm => (Cue::PistolMagOut, Cue::PistolMagIn, Cue::PistolSlide),
                };
                let t = RELOAD_TIMING;
                let mut push = |at: f32, cue: Cue, gain: f32, what: &'static str| {
                    pending.push(Pending {
                        due: now + duration * at,
                        shooter,
                        slot,
                        cue,
                        gain,
                        what,
                    });
                };
                push(t.mag_out, out, 0.75, "reload: magazine out");
                push(t.mag_in, inn, 0.9, "reload: magazine in");
                if empty {
                    push(t.rack, rack, 0.9, "reload: charging handle / slide");
                }
            }
            WeaponEventKind::Holster => {
                // Switching cancels a reload in progress.
                pending.retain(|p| p.shooter != shooter);
                play(&mut sounds, shooter, slot, "holster", Cue::Holster, 0.45);
            }
            WeaponEventKind::Draw => play(&mut sounds, shooter, slot, "draw", Cue::Draw, 0.55),
            WeaponEventKind::ReloadDone => {}
        }
    }
    let mut i = 0;
    while i < pending.len() {
        if pending[i].due <= now {
            let p = pending.swap_remove(i);
            play(&mut sounds, p.shooter, p.slot, p.what, p.cue, p.gain);
        } else {
            i += 1;
        }
    }
}

fn slot_name(slot: Slot) -> &'static str {
    match slot {
        Slot::Primary => "rifle",
        Slot::Sidearm => "pistol",
    }
}
