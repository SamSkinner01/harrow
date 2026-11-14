//! Game audio: recorded sound effects, played straight to the ears (the
//! local player's own sounds) or in 3D at a world position (everything
//! else).
//!
//! - [`SoundLibrary`]: every bank of variants under `assets/audio`, loaded
//!   at startup. The files are built by `tools/audio/build_audio.py`;
//!   [`BANKS`] must list the same banks and variant counts.
//! - [`Sounds`]: system param that plays a [`Sound`]: picks a variant (never
//!   the same one twice in a row), varies pitch and volume slightly, applies
//!   the mix (`audio` in the tuning file) and, for world sounds, our own
//!   distance fall-off. `--log-sounds` logs every decision.
//! - Spatial setup: the player camera gets a `SpatialListener` (see
//!   [`listener`] for why the ears are swapped and positions are scaled).
//! - `world.rs`: bullet impacts, breaking surfaces, explosions.
//! - `surface.rs`: what a collider sounds like ([`SurfaceLookup`]).
//!
//! Weapon sounds live in `weapons/audio.rs` and footsteps in
//! `player/footsteps.rs`; both play through [`Sounds`].
//!
//! Automated runs (`AutomatedRun`) are muted. With `--log-sounds` they
//! still log what would have played, which is how audio is checked in
//! scripted runs; `--play-sounds` plays them anyway (to listen to a
//! scripted scene).

mod surface;
mod world;

use std::collections::HashMap;

use bevy::{
    audio::{SpatialScale, Volume},
    ecs::system::SystemParam,
    prelude::*,
};

pub use surface::SurfaceLookup;

use crate::{
    config::{SoundCategory, SoundMaterial, Tuning},
    player::{AutomatedRun, PlayerCamera},
};

pub struct GameAudioPlugin;

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SoundState>()
            .add_observer(add_listener)
            .add_systems(PreStartup, load_library)
            .add_systems(First, start_frame)
            .add_systems(
                PreUpdate,
                apply_audio_tuning.run_if(resource_changed::<Tuning>),
            )
            .add_plugins(world::WorldSoundsPlugin);
    }
}

/// A kind of sound. Material cues without recordings of their own borrow
/// another bank (see [`Cue::resolve`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cue {
    /// Your own shot (stereo, first person).
    RifleShot,
    PistolShot,
    /// Someone else's shot: close recording (mono) and the distant layer.
    RifleShotWorld,
    PistolShotWorld,
    RifleFar,
    PistolFar,
    DryFire,
    RifleMagOut,
    RifleMagIn,
    RifleCharge,
    PistolMagOut,
    PistolMagIn,
    PistolSlide,
    Draw,
    Holster,
    Step(SoundMaterial),
    Impact(SoundMaterial),
    Break(SoundMaterial),
    Explosion,
    ExplosionFar,
}

impl Cue {
    /// The bank that actually plays.
    fn resolve(self) -> Cue {
        use SoundMaterial::*;
        match self {
            Cue::Step(Plaster | Body) => Cue::Step(Stone),
            Cue::Break(Stone | Sand | Body) => Cue::Break(Plaster),
            other => other,
        }
    }
}

/// Banks under `assets/audio`: (cue, path prefix, variants). The files are
/// `<prefix>_<n>.ogg` for n = 1..=variants. Keep in step with
/// `tools/audio/build_audio.py` (it prints these counts).
pub const BANKS: &[(Cue, &str, usize)] = {
    use Cue::*;
    use SoundMaterial::*;
    &[
        (RifleShot, "weapons/rifle_shot", 4),
        (RifleShotWorld, "weapons/rifle_shot_mono", 4),
        (RifleFar, "weapons/rifle_far", 4),
        (PistolShot, "weapons/pistol_shot", 4),
        (PistolShotWorld, "weapons/pistol_shot_mono", 4),
        (PistolFar, "weapons/pistol_far", 2),
        (DryFire, "weapons/dry_fire", 1),
        (RifleMagOut, "weapons/rifle_mag_out", 1),
        (RifleMagIn, "weapons/rifle_mag_in", 1),
        (RifleCharge, "weapons/rifle_charge", 2),
        (PistolMagOut, "weapons/pistol_mag_out", 1),
        (PistolMagIn, "weapons/pistol_mag_in", 1),
        (PistolSlide, "weapons/pistol_slide", 1),
        (Draw, "weapons/draw", 2),
        (Holster, "weapons/holster", 2),
        (Step(Stone), "footsteps/stone", 6),
        (Step(Wood), "footsteps/wood", 8),
        (Step(Sand), "footsteps/sand", 6),
        (Step(Metal), "footsteps/metal", 8),
        (Impact(Stone), "impacts/stone", 5),
        (Impact(Plaster), "impacts/plaster", 4),
        (Impact(Wood), "impacts/wood", 5),
        (Impact(Metal), "impacts/metal", 5),
        (Impact(Body), "impacts/body", 4),
        (Impact(Sand), "impacts/sand", 4),
        (Break(Wood), "destruction/wood", 4),
        (Break(Plaster), "destruction/plaster", 4),
        (Break(Metal), "destruction/metal", 3),
        (Explosion, "explosions/near", 3),
        (ExplosionFar, "explosions/far", 3),
    ]
};

/// Every recorded sound, by cue.
#[derive(Resource)]
pub struct SoundLibrary {
    banks: HashMap<Cue, Bank>,
}

struct Bank {
    prefix: &'static str,
    variants: Vec<Handle<AudioSource>>,
}

fn load_library(mut commands: Commands, assets: Res<AssetServer>) {
    let banks = BANKS
        .iter()
        .map(|&(cue, prefix, count)| {
            let variants = (1..=count)
                .map(|n| assets.load(format!("audio/{prefix}_{n}.ogg")))
                .collect();
            (cue, Bank { prefix, variants })
        })
        .collect();
    commands.insert_resource(SoundLibrary { banks });
}

/// Which mix bus and distance range a sound uses (`audio.categories`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Shots,
    ShotsDistant,
    /// Reloads, dry fire, draw/holster.
    Foley,
    Footsteps,
    Impacts,
    Destruction,
    Explosions,
    ExplosionsDistant,
}

impl Category {
    fn range(self, tuning: &Tuning) -> SoundCategory {
        let c = &tuning.audio.categories;
        match self {
            Category::Shots => c.shots,
            Category::ShotsDistant => c.shots_distant,
            Category::Foley => c.foley,
            Category::Footsteps => c.footsteps,
            Category::Impacts => c.impacts,
            Category::Destruction => c.destruction,
            Category::Explosions => c.explosions,
            Category::ExplosionsDistant => c.explosions_distant,
        }
    }

    /// Bus volume: footsteps or effects (and the older per-module volumes,
    /// which still multiply).
    fn bus(self, tuning: &Tuning) -> f32 {
        let a = &tuning.audio;
        match self {
            Category::Footsteps => a.footsteps * tuning.footsteps.volume,
            Category::Shots | Category::ShotsDistant | Category::Foley => {
                a.effects * tuning.weapons.effects.volume
            }
            _ => a.effects,
        }
    }
}

/// One sound to play. Built with [`Sound::new`] and the helpers below.
#[derive(Debug, Clone)]
pub struct Sound {
    pub cue: Cue,
    pub category: Category,
    /// World position (3D), or `None` for the local player's own sounds.
    pub at: Option<Vec3>,
    /// Extra linear gain (footstep loudness, layer crossfades).
    pub gain: f32,
    /// Base playback speed (pitch); random variation is added on top.
    pub speed: f32,
    /// Always plays, even over the voice limit.
    pub important: bool,
    /// For `--log-sounds`: what triggered it, and details (surface...).
    pub event: &'static str,
    pub detail: String,
}

impl Sound {
    pub fn new(event: &'static str, cue: Cue, category: Category) -> Self {
        Self {
            cue,
            category,
            at: None,
            gain: 1.0,
            speed: 1.0,
            important: false,
            event,
            detail: String::new(),
        }
    }

    pub fn at(mut self, position: Vec3) -> Self {
        self.at = Some(position);
        self
    }

    pub fn gain(mut self, gain: f32) -> Self {
        self.gain = gain;
        self
    }

    pub fn speed(mut self, speed: f32) -> Self {
        self.speed = speed;
        self
    }

    pub fn important(mut self) -> Self {
        self.important = true;
        self
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }
}

/// Marks a playing world (3D) sound, for the voice limit.
#[derive(Component)]
struct WorldVoice;

/// Random numbers, the last variant per cue, the debug flags.
#[derive(Resource)]
pub struct SoundState {
    rng: u64,
    last: HashMap<Cue, usize>,
    /// `--log-sounds`: log every sound (and every skipped one).
    log: bool,
    /// `--play-sounds`: play sounds in automated runs too (to listen to a
    /// scripted scene).
    play_automated: bool,
    /// World sounds started this frame (not yet visible to queries).
    started: usize,
}

impl Default for SoundState {
    fn default() -> Self {
        let has_flag = |flag: &str| std::env::args().any(|a| a == flag);
        let (log, play_automated) = (has_flag("--log-sounds"), has_flag("--play-sounds"));
        if log {
            info!("--log-sounds: logging every sound (automated runs log without playing)");
        }
        Self {
            rng: 0x9E37_79B9_7F4A_7C15,
            last: HashMap::new(),
            log,
            play_automated,
            started: 0,
        }
    }
}

impl SoundState {
    fn next(&mut self) -> u32 {
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        (self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as u32
    }

    /// Uniform in -1..1.
    fn signed(&mut self) -> f32 {
        self.next() as f32 / u32::MAX as f32 * 2.0 - 1.0
    }

    /// A variant index, never the previous one for this cue.
    fn pick(&mut self, cue: Cue, count: usize) -> usize {
        if count <= 1 {
            return 0;
        }
        let mut i = self.next() as usize % count;
        if self.last.get(&cue) == Some(&i) {
            i = (i + 1 + self.next() as usize % (count - 1)) % count;
        }
        self.last.insert(cue, i);
        i
    }
}

fn start_frame(mut state: ResMut<SoundState>) {
    state.started = 0;
}

/// Plays sounds. See the module docs.
#[derive(SystemParam)]
pub struct Sounds<'w, 's> {
    commands: Commands<'w, 's>,
    library: Res<'w, SoundLibrary>,
    state: ResMut<'w, SoundState>,
    tuning: Res<'w, Tuning>,
    automated: Option<Res<'w, AutomatedRun>>,
    listener: Query<'w, 's, &'static GlobalTransform, With<SpatialListener>>,
    voices: Query<'w, 's, (), With<WorldVoice>>,
}

impl Sounds<'_, '_> {
    /// Whether anything is played or logged. When false, callers can skip
    /// their work (ray casts) and just drain their messages.
    pub fn enabled(&self) -> bool {
        !self.muted() || self.state.log
    }

    /// Automated runs are silent unless `--play-sounds` is given.
    fn muted(&self) -> bool {
        self.automated.is_some() && !self.state.play_automated
    }

    pub fn tuning(&self) -> &Tuning {
        &self.tuning
    }

    /// Distance from the listener, metres (0 without a listener).
    pub fn distance(&self, position: Vec3) -> f32 {
        self.listener
            .iter()
            .next()
            .map_or(0.0, |ear| ear.translation().distance(position))
    }

    /// 0 close by .. 1 far away, over `audio.distant_blend`: how much of a
    /// gunshot or explosion is its distant layer.
    pub fn distant_blend(&self, position: Vec3) -> f32 {
        let (a, b) = self.tuning.audio.distant_blend;
        let t = ((self.distance(position) - a) / (b - a).max(1e-3)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    /// Logs a sound that was deliberately not played (rate limits).
    pub fn log_skip(&self, event: &'static str, reason: &str) {
        if self.state.log {
            info!("sound: {event} skipped ({reason})");
        }
    }

    /// Plays (or, in automated runs, only logs) `sound`. Returns whether it
    /// was played or logged.
    pub fn play(&mut self, sound: Sound) -> bool {
        if !self.enabled() {
            return false;
        }
        let cue = sound.cue.resolve();
        let Some(bank) = self.library.banks.get(&cue) else {
            warn!("sound: no recordings for {cue:?}");
            return false;
        };
        let t = &self.tuning;
        let a = &t.audio;
        let range = sound.category.range(t);
        let distance = sound.at.map(|p| self.distance(p));
        let falloff = distance.map_or(1.0, |d| falloff(d, range, a.rolloff));
        let jitter_db = self.state.signed() * a.volume_jitter_db;
        let speed = sound.speed * (1.0 + self.state.signed() * a.pitch_jitter);
        let volume = sound.category.bus(t)
            * range.volume
            * sound.gain
            * falloff
            * 10f32.powf(jitter_db / 20.0);
        let index = self.state.pick(cue, bank.variants.len());
        let muted = self.muted();

        let skip = if volume < 1e-3 {
            Some("too far to hear")
        } else if sound.at.is_some()
            && !sound.important
            && self.voices.iter().len() + self.state.started >= a.max_voices
        {
            Some("voice limit")
        } else {
            None
        };
        if self.state.log {
            let place = match (sound.at, distance) {
                (Some(p), Some(d)) => {
                    format!("3D at ({:.1}, {:.1}, {:.1}) {d:.1} m", p.x, p.y, p.z)
                }
                _ => "2D (own)".to_string(),
            };
            let detail = if sound.detail.is_empty() {
                String::new()
            } else {
                format!(" {}", sound.detail)
            };
            let outcome = match (skip, muted) {
                (Some(reason), _) => format!(" -- skipped: {reason}"),
                (None, true) => " (muted)".to_string(),
                (None, false) => String::new(),
            };
            info!(
                "sound: {}{detail} -> {}_{}.ogg, {place}, volume {volume:.3}, speed {speed:.2}{outcome}",
                sound.event,
                bank.prefix,
                index + 1,
            );
        }
        if skip.is_some() {
            return false;
        }
        if muted {
            return true;
        }
        let handle = bank.variants[index].clone();
        let settings = PlaybackSettings::DESPAWN
            .with_volume(Volume::Linear(volume))
            .with_speed(speed);
        match sound.at {
            Some(position) => {
                self.state.started += 1;
                self.commands.spawn((
                    AudioPlayer(handle),
                    settings
                        .with_spatial(true)
                        .with_spatial_scale(SpatialScale::new(SPATIAL_SCALE)),
                    Transform::from_translation(position),
                    WorldVoice,
                ));
            }
            None => {
                self.commands.spawn((AudioPlayer(handle), settings));
            }
        }
        true
    }
}

/// Volume at `distance` metres: full up to `near`, then (near / d)^rolloff,
/// faded to silence over the last quarter before `far`.
pub fn falloff(distance: f32, range: SoundCategory, rolloff: f32) -> f32 {
    let near = range.near.max(0.01);
    let base = if distance <= near {
        1.0
    } else {
        (near / distance).powf(rolloff.max(0.0))
    };
    let far = range.far.max(near + 0.01);
    let fade_from = near.max(far * 0.75);
    let edge = if distance >= far {
        0.0
    } else if distance <= fade_from {
        1.0
    } else {
        let t = (distance - fade_from) / (far - fade_from);
        1.0 - t * t * (3.0 - 2.0 * t)
    };
    base * edge
}

/// Positions of world sounds and the listener are multiplied by this before
/// they reach the audio backend. See [`listener`].
const SPATIAL_SCALE: f32 = 1.0 / 4000.0;

/// The listener on the player camera.
///
/// Bevy's spatial audio is rodio's `Spatial` source, which gives each ear
/// `pan * min(1, 1 / distance^2)` (distances in scaled units). Its `pan`
/// favours the ear *farther* from the source and relies on the 1/distance^2
/// term to flip that round, which only works within a few ear gaps: past
/// that, distant sounds come from the wrong side, and the 1/d^2 fall-off is
/// far too steep in metres anyway. So: every position is scaled by
/// [`SPATIAL_SCALE`] (anything within 4 km stays inside one scaled unit, so
/// the distance term is always 1), and the ears are swapped, which turns
/// `pan` the right way round: 0.75 / 0.75 straight ahead or behind, 1.0 /
/// 0.5 fully to one side. Distance fall-off is ours ([`falloff`]).
pub fn listener(ear_gap: f32) -> SpatialListener {
    let gap = ear_gap.max(0.01);
    SpatialListener {
        left_ear_offset: Vec3::X * gap / 2.0,
        right_ear_offset: Vec3::X * gap / -2.0,
    }
}

fn add_listener(add: On<Add<PlayerCamera>>, mut commands: Commands, tuning: Res<Tuning>) {
    commands
        .entity(add.entity)
        .insert(listener(tuning.audio.ear_gap));
}

/// Master volume and ear gap from the tuning file.
fn apply_audio_tuning(
    tuning: Res<Tuning>,
    mut global: ResMut<GlobalVolume>,
    mut listeners: Query<&mut SpatialListener>,
) {
    global.volume = Volume::Linear(tuning.audio.master.max(0.0));
    for mut ears in &mut listeners {
        *ears = listener(tuning.audio.ear_gap);
    }
}
