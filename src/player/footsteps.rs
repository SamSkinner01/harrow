//! Placeholder footstep audio for the local player.
//!
//! The sound is synthesised in code (a short gritty scuff with a low thump),
//! so there is no external asset yet. Real recorded footsteps per surface
//! come later; the [`Footstep`] message stays the same.

use bevy::{audio::Volume, prelude::*};

use super::{AutomatedRun, Footstep, LocalPlayer};
use crate::config::Tuning;

pub struct FootstepAudioPlugin;

impl Plugin for FootstepAudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, create_sound).add_systems(
            Update,
            play_local_footsteps.run_if(not(resource_exists::<AutomatedRun>)),
        );
    }
}

#[derive(Resource)]
struct FootstepSound(Handle<AudioSource>);

fn create_sound(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let source = AudioSource {
        bytes: footstep_wav().into(),
    };
    commands.insert_resource(FootstepSound(sources.add(source)));
}

fn play_local_footsteps(
    mut commands: Commands,
    mut steps: MessageReader<Footstep>,
    local: Query<(), With<LocalPlayer>>,
    sound: Res<FootstepSound>,
    tuning: Res<Tuning>,
    mut left_foot: Local<bool>,
) {
    for step in steps.read() {
        if !local.contains(step.entity) || step.loudness <= 0.0 {
            continue;
        }
        // Alternate feet with a slight pitch difference so it isn't robotic.
        *left_foot = !*left_foot;
        let speed = if *left_foot { 1.0 } else { 0.92 };
        let volume = tuning.footsteps.volume * step.loudness.sqrt();
        commands.spawn((
            AudioPlayer(sound.0.clone()),
            PlaybackSettings::DESPAWN
                .with_volume(Volume::Linear(volume))
                .with_speed(speed),
        ));
    }
}

/// 16-bit mono WAV, ~90 ms.
fn footstep_wav() -> Vec<u8> {
    const RATE: u32 = 22_050;
    let count = (RATE as f32 * 0.09) as usize;
    let mut seed: u32 = 0x9E37_79B9;
    let mut filtered = 0.0f32;
    let mut samples = Vec::with_capacity(count);
    for i in 0..count {
        let t = i as f32 / RATE as f32;
        // xorshift noise, low-passed into a grit/scuff texture.
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let noise = seed as f32 / u32::MAX as f32 * 2.0 - 1.0;
        filtered += (noise - filtered) * 0.3;
        let scuff = filtered * (-t * 45.0).exp();
        let thump = (t * std::f32::consts::TAU * 80.0).sin() * (-t * 35.0).exp();
        let s = (scuff * 0.9 + thump * 0.5).clamp(-1.0, 1.0);
        samples.push((s * i16::MAX as f32 * 0.8) as i16);
    }

    let data_len = (samples.len() * 2) as u32;
    let mut wav = Vec::with_capacity(44 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        wav.extend_from_slice(&s.to_le_bytes());
    }
    wav
}
