//! Placeholder gadget sounds, synthesised in code (no external audio), and
//! when they play. Presentation only; muted during automated runs.
//!
//! Volume falls off with distance from the listener (the local player, or
//! the drone / camera they are looking through) and is muffled while the
//! local player is deafened by a flashbang.

use bevy::{audio::Volume, prelude::*};

use super::{
    GadgetEvent, SecondaryGadget,
    cutter::{CutterState, RingCutter},
    drone::Drone,
    effects::HearingMuffled,
    remote_view::Listener,
};
use crate::{
    config::Tuning,
    player::{AutomatedRun, LocalPlayer},
};

pub struct GadgetSfxPlugin;

impl Plugin for GadgetSfxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, create_sounds).add_systems(
            Update,
            (event_sounds, cutter_grind, drone_whir).run_if(not(resource_exists::<AutomatedRun>)),
        );
    }
}

#[derive(Resource)]
pub struct GadgetSfx {
    pub grind: Handle<AudioSource>,
    pub clunk: Handle<AudioSource>,
    pub thud: Handle<AudioSource>,
    pub bang: Handle<AudioSource>,
    pub flash: Handle<AudioSource>,
    pub ring: Handle<AudioSource>,
    pub hiss: Handle<AudioSource>,
    pub rattle: Handle<AudioSource>,
    pub beep: Handle<AudioSource>,
    pub whir: Handle<AudioSource>,
    pub click: Handle<AudioSource>,
    pub pop: Handle<AudioSource>,
    pub ratchet: Handle<AudioSource>,
}

/// Play `sound` at `position` (None = right here) for the listener.
pub fn play_at(
    commands: &mut Commands,
    sound: &Handle<AudioSource>,
    volume: f32,
    position: Option<Vec3>,
    listener: &Listener,
    hearing: &HearingMuffled,
    tuning: &Tuning,
) {
    let distance = position.map_or(0.0, |p| p.distance(listener.position));
    let falloff = 1.0 / (1.0 + distance / 5.0);
    let v = volume * falloff * tuning.round.volume * (1.0 - 0.85 * hearing.amount);
    if v < 0.01 {
        return;
    }
    commands.spawn((
        AudioPlayer(sound.clone()),
        PlaybackSettings::DESPAWN.with_volume(Volume::Linear(v)),
    ));
}

const RATE: u32 = 22_050;

struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

fn samples(dur: f32, mut f: impl FnMut(f32) -> f32) -> Vec<f32> {
    let n = (RATE as f32 * dur) as usize;
    (0..n).map(|i| f(i as f32 / RATE as f32)).collect()
}

fn create_sounds(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let mut add = |s: Vec<f32>| sources.add(AudioSource { bytes: wav(&s).into() });
    let tau = std::f32::consts::TAU;

    // Cutter: a gritty saw buzz with a metallic whine, half a second
    // (repeated while cutting).
    let mut n = Noise(0xC0FF_EE01);
    let grind = samples(0.5, |t| {
        let saw = ((t * 118.0).fract() * 2.0 - 1.0) * 0.35;
        let whine = (t * 1460.0 * tau).sin() * 0.12 * (1.0 + 0.3 * (t * 23.0 * tau).sin());
        let grit = n.next() * 0.35 * (0.6 + 0.4 * (t * 37.0 * tau).sin().abs());
        let env = (t / 0.02).min(1.0) * ((0.5 - t) / 0.02).clamp(0.0, 1.0);
        (saw + whine + grit) * env * 0.8
    });
    // Jam: a hard metallic clunk.
    let clunk = samples(0.3, |t| {
        let body = (t * 95.0 * tau).sin() * (-t * 18.0).exp();
        let ring = (t * 2350.0 * tau).sin() * (-t * 25.0).exp() * 0.4;
        (body + ring) * 0.8
    });
    // The plug falling: a deep thud with rubble.
    let mut n = Noise(0x0BAD_F00D);
    let thud = samples(0.9, |t| {
        let body = (t * 55.0 * tau).sin() * (-t * 6.0).exp();
        let rubble = n.next() * (-t * 5.0).exp() * 0.4;
        (body + rubble) * 0.9
    });
    // Explosion: a boom with a noisy tail.
    let mut n = Noise(0x1234_ABCD);
    let mut low = 0.0f32;
    let bang = samples(1.4, |t| {
        low += (n.next() - low) * 0.06;
        let thump = (t * 48.0 * tau).sin() * (-t * 7.0).exp();
        (low * 3.0 * (-t * 3.0).exp() + thump * 0.8).clamp(-1.0, 1.0)
    });
    // Flashbang: a sharp crack.
    let mut n = Noise(0x5EED_0001);
    let flash = samples(0.6, |t| {
        let crack = n.next() * (-t * 14.0).exp();
        let body = (t * 70.0 * tau).sin() * (-t * 9.0).exp() * 0.6;
        (crack + body).clamp(-1.0, 1.0)
    });
    // Ringing ears after a flash.
    let ring = samples(4.0, |t| {
        let env = (t / 0.05).min(1.0) * (1.0 - t / 4.0).max(0.0).powi(2);
        ((t * 3900.0 * tau).sin() * 0.5 + (t * 3950.0 * tau).sin() * 0.3) * env * 0.5
    });
    // Smoke pouring out.
    let mut n = Noise(0x0050_0E00);
    let hiss = samples(2.5, |t| {
        let env = (t / 0.1).min(1.0) * (1.0 - t / 2.5).max(0.0);
        n.next() * 0.45 * env
    });
    // Razor wire: a few metallic ticks.
    let mut n = Noise(0x00A1_2E00);
    let rattle = samples(0.35, |t| {
        let clicks = [0.0, 0.07, 0.15, 0.22];
        let mut s = 0.0;
        for c in clicks {
            let dt = t - c;
            if dt >= 0.0 {
                s += (dt * 3100.0 * tau).sin() * (-dt * 70.0).exp() * 0.5 + n.next() * (-dt * 120.0).exp() * 0.2;
            }
        }
        s
    });
    // Proximity alarm: two quick high beeps.
    let beep = samples(0.28, |t| {
        let on = t < 0.09 || (0.15..0.24).contains(&t);
        if on { (t * 2600.0 * tau).sin() * 0.55 } else { 0.0 }
    });
    // Drone motor.
    let whir = samples(0.4, |t| {
        let env = (t / 0.03).min(1.0) * ((0.4 - t) / 0.05).clamp(0.0, 1.0);
        ((t * 310.0 * tau).sin() * 0.3 + (t * 620.0 * tau).sin() * 0.15) * env
    });
    // Switching views: a burst of static.
    let mut n = Noise(0x57A7_1C00);
    let click = samples(0.12, |t| n.next() * (1.0 - t / 0.12) * 0.4);
    // Impact charge / backplate: a short sharp pop.
    let mut n = Noise(0x0000_9090);
    let pop = samples(0.5, |t| {
        let crack = n.next() * (-t * 22.0).exp();
        let body = (t * 80.0 * tau).sin() * (-t * 12.0).exp();
        (crack * 0.8 + body * 0.7).clamp(-1.0, 1.0)
    });
    // Clamping / bolting: a ratchet.
    let ratchet = samples(0.4, |t| {
        let k = (t * 18.0).fract();
        (t * 1700.0 * tau).sin() * (-k * 40.0).exp() * 0.5
    });

    commands.insert_resource(GadgetSfx {
        grind: add(grind),
        clunk: add(clunk),
        thud: add(thud),
        bang: add(bang),
        flash: add(flash),
        ring: add(ring),
        hiss: add(hiss),
        rattle: add(rattle),
        beep: add(beep),
        whir: add(whir),
        click: add(click),
        pop: add(pop),
        ratchet: add(ratchet),
    });
}

#[allow(clippy::too_many_arguments)]
fn event_sounds(
    mut commands: Commands,
    mut events: MessageReader<GadgetEvent>,
    local: Query<Entity, With<LocalPlayer>>,
    places: Query<&GlobalTransform>,
    sfx: Res<GadgetSfx>,
    listener: Res<Listener>,
    hearing: Res<HearingMuffled>,
    tuning: Res<Tuning>,
) {
    let me = local.single().ok();
    let at = |e: Entity| places.get(e).ok().map(|t| t.translation());
    for event in events.read() {
        let (sound, volume, position) = match event {
            GadgetEvent::CutterClamped { position, .. } => (&sfx.ratchet, 0.7, Some(*position)),
            GadgetEvent::CutterJammed { position, .. } => (&sfx.clunk, 1.0, Some(*position)),
            GadgetEvent::CutterReseated { cutter } => (&sfx.ratchet, 0.8, at(*cutter)),
            GadgetEvent::CutterBreached { position, .. } => (&sfx.thud, 1.2, Some(*position)),
            GadgetEvent::BackplateSet { position, .. } => (&sfx.ratchet, 0.6, Some(*position)),
            GadgetEvent::BackplateFired { position, .. } => (&sfx.pop, 1.3, Some(*position)),
            GadgetEvent::Detonated { kind, position } => match kind {
                SecondaryGadget::Frag => (&sfx.bang, 1.4, Some(*position)),
                SecondaryGadget::Flashbang => (&sfx.flash, 1.4, Some(*position)),
                SecondaryGadget::Smoke => (&sfx.hiss, 0.8, Some(*position)),
                SecondaryGadget::ImpactCharge => (&sfx.pop, 1.2, Some(*position)),
                _ => continue,
            },
            GadgetEvent::Flashed { entity, strength } if Some(*entity) == me => {
                // Not muffled: this is the ringing itself.
                play_at(&mut commands, &sfx.ring, 0.6 * strength, None, &listener, &HearingMuffled::default(), &tuning);
                continue;
            }
            GadgetEvent::Placed { position, kind, .. } => match kind {
                SecondaryGadget::RazorWire => (&sfx.rattle, 0.8, Some(*position)),
                _ => (&sfx.ratchet, 0.6, Some(*position)),
            },
            GadgetEvent::WireRattle { position, .. } => (&sfx.rattle, 1.0, Some(*position)),
            GadgetEvent::AlarmTriggered { alarm, .. } => (&sfx.beep, 0.9, at(*alarm)),
            GadgetEvent::ViewEntered { by, .. } | GadgetEvent::ViewLeft { by, .. } if Some(*by) == me => {
                (&sfx.click, 0.5, None)
            }
            GadgetEvent::Destroyed { position, .. } => (&sfx.clunk, 0.5, Some(*position)),
            _ => continue,
        };
        play_at(&mut commands, sound, volume, position, &listener, &hearing, &tuning);
    }
}

/// A working cutter grinds every half second.
#[allow(clippy::too_many_arguments)]
fn cutter_grind(
    mut commands: Commands,
    cutters: Query<(&RingCutter, &GlobalTransform)>,
    sfx: Res<GadgetSfx>,
    listener: Res<Listener>,
    hearing: Res<HearingMuffled>,
    tuning: Res<Tuning>,
    time: Res<Time>,
    mut timer: Local<f32>,
) {
    *timer -= time.delta_secs();
    if *timer > 0.0 {
        return;
    }
    *timer = 0.45;
    for (cutter, at) in &cutters {
        if cutter.state == CutterState::Cutting {
            play_at(&mut commands, &sfx.grind, 1.0, Some(at.translation()), &listener, &hearing, &tuning);
        }
    }
}

/// Moving drones whir (quietly).
#[allow(clippy::too_many_arguments)]
fn drone_whir(
    mut commands: Commands,
    drones: Query<(&Drone, &GlobalTransform)>,
    sfx: Res<GadgetSfx>,
    listener: Res<Listener>,
    hearing: Res<HearingMuffled>,
    tuning: Res<Tuning>,
    time: Res<Time>,
    mut timer: Local<f32>,
) {
    *timer -= time.delta_secs();
    if *timer > 0.0 {
        return;
    }
    *timer = 0.38;
    for (drone, at) in &drones {
        if drone.velocity.with_y(0.0).length() > 0.3 {
            play_at(&mut commands, &sfx.whir, 0.35, Some(at.translation()), &listener, &hearing, &tuning);
        }
    }
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
