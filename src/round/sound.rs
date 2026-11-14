//! Placeholder round and gadget sounds, synthesised in code (no external
//! audio): beeps for the defuser, chords for round results, a hiss for
//! landing sensor darts, a tick for seismic-stake pings.
//!
//! Presentation only. Muted during automated runs (`AutomatedRun`).

use bevy::{audio::Volume, prelude::*};

use super::{
    Round, RoundEvent, RoundPhase, Squad, defuser::DefuserDevice,
};
use crate::{
    config::Tuning,
    player::{AutomatedRun, LocalPlayer},
};

pub struct RoundSoundPlugin;

impl Plugin for RoundSoundPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, create_sounds).add_systems(
            Update,
            (round_event_sounds, defuser_beeps).run_if(not(resource_exists::<AutomatedRun>)),
        );
    }
}

/// Handles to every synthesised sound.
#[derive(Resource)]
pub struct Sfx {
    pub plant_start: Handle<AudioSource>,
    pub planted: Handle<AudioSource>,
    pub beep: Handle<AudioSource>,
    pub disabled: Handle<AudioSource>,
    pub win: Handle<AudioSource>,
    pub lose: Handle<AudioSource>,
    pub phase: Handle<AudioSource>,
    pub dart_fire: Handle<AudioSource>,
    pub hiss: Handle<AudioSource>,
    pub tick: Handle<AudioSource>,
    pub stake_place: Handle<AudioSource>,
}

/// Play `sound` once at `volume` (multiplied by the round volume).
pub fn play(commands: &mut Commands, sound: &Handle<AudioSource>, volume: f32, tuning: &Tuning) {
    commands.spawn((
        AudioPlayer(sound.clone()),
        PlaybackSettings::DESPAWN.with_volume(Volume::Linear(volume * tuning.round.volume)),
    ));
}

fn create_sounds(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let mut add = |samples: Vec<f32>| {
        sources.add(AudioSource {
            bytes: wav(&samples).into(),
        })
    };
    commands.insert_resource(Sfx {
        plant_start: add(tones(&[(880.0, 0.06), (0.0, 0.03), (1175.0, 0.08)])),
        planted: add(tones(&[(660.0, 0.12), (990.0, 0.12), (1320.0, 0.25)])),
        beep: add(tones(&[(1760.0, 0.07)])),
        disabled: add(tones(&[(990.0, 0.12), (660.0, 0.12), (440.0, 0.35)])),
        win: add(chord(&[523.0, 659.0, 784.0], 0.9)),
        lose: add(chord(&[311.0, 370.0, 466.0], 0.9)),
        phase: add(tones(&[(587.0, 0.1), (0.0, 0.05), (587.0, 0.1)])),
        dart_fire: add(noise_burst(0.08, 0.5, 60.0)),
        hiss: add(noise_burst(0.45, 0.85, 7.0)),
        tick: add(tones(&[(2400.0, 0.025)])),
        stake_place: add(thunk()),
    });
}

fn round_event_sounds(
    mut commands: Commands,
    mut events: MessageReader<RoundEvent>,
    local: Query<Entity, With<LocalPlayer>>,
    round: Res<Round>,
    sfx: Res<Sfx>,
    tuning: Res<Tuning>,
) {
    let me = local.single().ok();
    for event in events.read() {
        let sound = match event {
            RoundEvent::PlantStarted { entity, .. } if Some(*entity) == me => Some(&sfx.plant_start),
            RoundEvent::DisableStarted { entity } if Some(*entity) == me => Some(&sfx.plant_start),
            RoundEvent::Planted { .. } => Some(&sfx.planted),
            RoundEvent::Disabled { .. } => Some(&sfx.disabled),
            RoundEvent::PhaseStarted {
                phase: RoundPhase::Prep | RoundPhase::Action,
                ..
            } => Some(&sfx.phase),
            RoundEvent::RoundWon { result, .. } => {
                if round.squad_on(result.winner) == Squad::Blue {
                    Some(&sfx.win)
                } else {
                    Some(&sfx.lose)
                }
            }
            _ => None,
        };
        if let Some(sound) = sound {
            play(&mut commands, sound, 0.8, &tuning);
        }
    }
}

/// The planted defuser beeps every second, four times a second near the
/// end; quieter with distance.
fn defuser_beeps(
    mut commands: Commands,
    round: Res<Round>,
    devices: Query<(&DefuserDevice, &GlobalTransform)>,
    listener: Query<&Transform, With<LocalPlayer>>,
    sfx: Res<Sfx>,
    tuning: Res<Tuning>,
    mut last_beat: Local<i32>,
) {
    if round.phase != RoundPhase::Planted {
        *last_beat = -1;
        return;
    }
    let rate = if round.time_left < 10.0 { 4.0 } else { 1.0 };
    let beat = (round.elapsed * rate) as i32;
    if beat == *last_beat {
        return;
    }
    *last_beat = beat;
    let Some((_, at)) = devices.iter().find(|(d, _)| !d.disabled) else {
        return;
    };
    let distance = listener
        .single()
        .map(|t| t.translation.distance(at.translation()))
        .unwrap_or(0.0);
    let volume = (1.0 / (1.0 + distance / 6.0)).clamp(0.08, 1.0);
    play(&mut commands, &sfx.beep, volume, &tuning);
}

const RATE: u32 = 22_050;

/// A sequence of sine tones (`freq 0` = silence), each with a soft attack
/// and release.
fn tones(parts: &[(f32, f32)]) -> Vec<f32> {
    let mut out = Vec::new();
    for &(freq, dur) in parts {
        let n = (RATE as f32 * dur) as usize;
        for i in 0..n {
            let t = i as f32 / RATE as f32;
            let env = (t / 0.005).min(1.0) * ((dur - t) / 0.02).clamp(0.0, 1.0);
            let s = if freq > 0.0 {
                (t * freq * std::f32::consts::TAU).sin() * 0.6
            } else {
                0.0
            };
            out.push(s * env);
        }
    }
    out
}

/// Three notes together, decaying.
fn chord(freqs: &[f32], dur: f32) -> Vec<f32> {
    let n = (RATE as f32 * dur) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let env = (t / 0.01).min(1.0) * (-t * 3.0).exp();
            let s: f32 = freqs
                .iter()
                .map(|f| (t * f * std::f32::consts::TAU).sin())
                .sum();
            s / freqs.len() as f32 * 0.7 * env
        })
        .collect()
}

/// Filtered noise. `brightness` 0..1 (higher = hissier), `decay` per second.
fn noise_burst(dur: f32, brightness: f32, decay: f32) -> Vec<f32> {
    let n = (RATE as f32 * dur) as usize;
    let mut seed: u32 = 0x1234_5678;
    let mut low = 0.0f32;
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let white = seed as f32 / u32::MAX as f32 * 2.0 - 1.0;
            low += (white - low) * 0.2;
            // High-pass mix: white minus its low-passed part.
            let s = white * brightness + low * (1.0 - brightness) - low * brightness * 0.8;
            let env = (t / 0.004).min(1.0) * (-t * decay).exp();
            s * env * 0.6
        })
        .collect()
}

/// A dull metal knock for hammering a stake in.
fn thunk() -> Vec<f32> {
    let n = (RATE as f32 * 0.18) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let body = (t * 140.0 * std::f32::consts::TAU).sin() * (-t * 30.0).exp();
            let ring = (t * 1900.0 * std::f32::consts::TAU).sin() * (-t * 45.0).exp() * 0.3;
            (body + ring) * 0.7
        })
        .collect()
}

/// 16-bit mono WAV from samples in -1..1.
fn wav(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    out
}
