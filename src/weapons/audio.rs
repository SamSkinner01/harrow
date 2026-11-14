//! Placeholder weapon sounds for the local player, synthesised in code like
//! the footsteps (no external assets): rifle and pistol shots, the empty
//! click, and reload foley (magazine out, magazine in, bolt/slide rack).
//! Muted during automated runs.

use std::f32::consts::TAU;

use bevy::{audio::Volume, prelude::*};

use super::{ShotFired, Slot, WeaponEvent, WeaponEventKind};
use crate::{
    config::Tuning,
    player::{AutomatedRun, LocalPlayer},
};

pub struct WeaponAudioPlugin;

impl Plugin for WeaponAudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, create_sounds).add_systems(
            Update,
            play_weapon_sounds.run_if(not(resource_exists::<AutomatedRun>)),
        );
    }
}

#[derive(Resource)]
struct WeaponSounds {
    rifle: Handle<AudioSource>,
    pistol: Handle<AudioSource>,
    click: Handle<AudioSource>,
    mag_out: Handle<AudioSource>,
    mag_in: Handle<AudioSource>,
    rack: Handle<AudioSource>,
}

fn create_sounds(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    // `--dump-weapon-sounds <dir>`: also write each sound as a .wav file,
    // to listen to them outside the game.
    let dump = {
        let args: Vec<String> = std::env::args().collect();
        args.iter()
            .position(|a| a == "--dump-weapon-sounds")
            .and_then(|i| args.get(i + 1))
            .map(std::path::PathBuf::from)
    };
    let mut add = |name: &str, samples: Vec<f32>| {
        let bytes = wav(&samples);
        if let Some(dir) = &dump {
            let path = dir.join(format!("{name}.wav"));
            match std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&path, &bytes)) {
                Ok(()) => info!("wrote {}", path.display()),
                Err(err) => warn!("could not write {}: {err}", path.display()),
            }
        }
        sources.add(AudioSource {
            bytes: bytes.into(),
        })
    };
    commands.insert_resource(WeaponSounds {
        rifle: add("rifle_shot", gunshot(0.42, 70.0, 0.55, 11)),
        pistol: add("pistol_shot", gunshot(0.3, 110.0, 0.35, 23)),
        click: add("dry_click", click(1800.0, 0.025, 31)),
        mag_out: add("magazine_out", mag_out()),
        mag_in: add("magazine_in", mag_in()),
        rack: add("rack", rack()),
    });
}

/// Reload sounds waiting for their moment in the animation.
#[derive(Default)]
struct Pending(Vec<(f32, Handle<AudioSource>, f32)>);

fn play_weapon_sounds(
    mut commands: Commands,
    mut shots: MessageReader<ShotFired>,
    mut events: MessageReader<WeaponEvent>,
    local: Query<(), With<LocalPlayer>>,
    sounds: Res<WeaponSounds>,
    tuning: Res<Tuning>,
    time: Res<Time>,
    mut pending: Local<Pending>,
    mut alternate: Local<bool>,
) {
    let volume = tuning.weapons.effects.volume;
    let mut play = |handle: &Handle<AudioSource>, gain: f32, speed: f32| {
        commands.spawn((
            AudioPlayer(handle.clone()),
            PlaybackSettings::DESPAWN
                .with_volume(Volume::Linear(volume * gain))
                .with_speed(speed),
        ));
    };
    for shot in shots.read().filter(|s| local.contains(s.shooter)) {
        // Slight variation so automatic fire isn't a machine-gun loop.
        *alternate = !*alternate;
        let speed = if *alternate { 1.0 } else { 0.96 };
        match shot.slot {
            Slot::Primary => play(&sounds.rifle, 1.0, speed),
            Slot::Sidearm => play(&sounds.pistol, 0.9, speed),
        }
    }
    let now = time.elapsed_secs();
    for event in events.read().filter(|e| local.contains(e.shooter)) {
        match event.kind {
            WeaponEventKind::DryFire => play(&sounds.click, 0.8, 1.0),
            WeaponEventKind::ReloadStart { empty } => {
                let def = event.slot.def(&tuning);
                let duration = if empty { def.reload_empty } else { def.reload_tactical };
                // Matches the viewmodel: out at ~20%, in at ~70%, rack at ~85%.
                pending.0.push((now + duration * 0.2, sounds.mag_out.clone(), 0.7));
                pending.0.push((now + duration * 0.7, sounds.mag_in.clone(), 0.8));
                if empty {
                    pending.0.push((now + duration * 0.86, sounds.rack.clone(), 0.8));
                }
            }
            WeaponEventKind::Holster => pending.0.clear(),
            WeaponEventKind::Draw => play(&sounds.mag_in, 0.35, 0.8),
            WeaponEventKind::ReloadDone => {}
        }
    }
    let mut i = 0;
    while i < pending.0.len() {
        if pending.0[i].0 <= now {
            let (_, handle, gain) = pending.0.swap_remove(i);
            play(&handle, gain, 1.0);
        } else {
            i += 1;
        }
    }
}

const RATE: u32 = 22_050;

/// Deterministic white noise.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

/// A shot: sharp crack, low boom, then a noisy tail (room/outdoor echo).
fn gunshot(length: f32, boom_hz: f32, tail: f32, seed: u32) -> Vec<f32> {
    let count = (RATE as f32 * length) as usize;
    let mut noise = Noise(0x1234_5677 ^ seed);
    let (mut low, mut band) = (0.0f32, 0.0f32);
    (0..count)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let n = noise.next();
            low += (n - low) * 0.08;
            band += (n - band) * 0.45;
            let crack = n * (-t * 160.0).exp();
            let body = band * (-t * 28.0).exp();
            let boom = (t * TAU * boom_hz * (1.0 - t)).sin() * (-t * 18.0).exp();
            let echo = low * tail * (-t * 7.0).exp() * (1.0 - (-t * 60.0).exp());
            ((crack * 0.9 + body * 0.6 + boom * 0.9 + echo * 1.4) * 0.7).clamp(-1.0, 1.0)
        })
        .collect()
}

/// A short metallic click.
fn click(freq: f32, length: f32, seed: u32) -> Vec<f32> {
    let count = (RATE as f32 * length) as usize;
    let mut noise = Noise(0x0BAD_5EED ^ seed);
    (0..count)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let ring = (t * TAU * freq).sin() * (-t * 220.0).exp();
            let tick = noise.next() * (-t * 900.0).exp();
            (ring * 0.6 + tick * 0.8).clamp(-1.0, 1.0)
        })
        .collect()
}

/// Mix `sound` into `into` starting at `at` seconds.
fn overlay(into: &mut Vec<f32>, sound: &[f32], at: f32, gain: f32) {
    let start = (at * RATE as f32) as usize;
    if into.len() < start + sound.len() {
        into.resize(start + sound.len(), 0.0);
    }
    for (i, s) in sound.iter().enumerate() {
        into[start + i] = (into[start + i] + s * gain).clamp(-1.0, 1.0);
    }
}

/// Scrape of plastic on metal.
fn scrape(length: f32, seed: u32) -> Vec<f32> {
    let count = (RATE as f32 * length) as usize;
    let mut noise = Noise(0x5CA4_E000 ^ seed);
    let mut band = 0.0f32;
    (0..count)
        .map(|i| {
            let t = i as f32 / length / RATE as f32;
            band += (noise.next() - band) * 0.3;
            band * (t * std::f32::consts::PI).sin() * 0.5
        })
        .collect()
}

fn mag_out() -> Vec<f32> {
    let mut s = click(1300.0, 0.03, 1);
    overlay(&mut s, &scrape(0.12, 2), 0.02, 1.0);
    s
}

fn mag_in() -> Vec<f32> {
    let mut s = scrape(0.08, 3);
    overlay(&mut s, &click(900.0, 0.04, 4), 0.07, 1.0);
    overlay(&mut s, &click(1500.0, 0.03, 5), 0.1, 0.7);
    s
}

fn rack() -> Vec<f32> {
    let mut s = click(1100.0, 0.04, 6);
    overlay(&mut s, &scrape(0.06, 7), 0.02, 0.8);
    overlay(&mut s, &click(1700.0, 0.04, 8), 0.13, 1.0);
    s
}

/// 16-bit mono WAV.
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
        out.extend_from_slice(&((s * 0.85 * i16::MAX as f32) as i16).to_le_bytes());
    }
    out
}
