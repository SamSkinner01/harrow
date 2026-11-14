//! Tuning values from `assets/config/tuning.ron`.
//!
//! The file is the single source of truth. A copy is compiled into the binary
//! so the game always starts with valid values; the file on disk is then loaded
//! as an asset and hot-reloaded whenever it is saved. A file with errors is
//! reported in the log and ignored, keeping the previous values.

use bevy::{
    asset::{AssetLoader, LoadContext, io::Reader},
    prelude::*,
};
use serde::Deserialize;
use std::collections::HashMap;

const TUNING_PATH: &str = "config/tuning.ron";
const EMBEDDED_TUNING: &str = include_str!("../assets/config/tuning.ron");

pub struct TuningPlugin;

impl Plugin for TuningPlugin {
    fn build(&self, app: &mut App) {
        let tuning: Tuning = ron::from_str(EMBEDDED_TUNING)
            .unwrap_or_else(|err| panic!("assets/{TUNING_PATH} is invalid: {err}"));
        app.insert_resource(tuning)
            .init_asset::<Tuning>()
            .register_asset_loader(TuningLoader)
            .add_systems(Startup, load_tuning)
            .add_systems(PreUpdate, apply_tuning_changes.in_set(TuningSystems));
    }
}

/// Where a changed tuning file is applied (`PreUpdate`). Systems that
/// override tuning values (a joined server's movement values, a dedicated
/// server's round settings) run after it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct TuningSystems;

#[derive(Asset, Resource, TypePath, Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Tuning {
    pub look: LookTuning,
    pub controls: ControlTuning,
    pub movement: MovementTuning,
    pub body: BodyTuning,
    pub lean: LeanTuning,
    pub view: ViewTuning,
    pub lighting: LightingTuning,
    pub surfaces: HashMap<String, SurfaceTuning>,
    pub footsteps: FootstepTuning,
    #[cfg_attr(not(feature = "client"), allow(dead_code))] // sounds only
    pub audio: AudioTuning,
    // Placeholder sections, filled in by M3 / M4 / M6.
    pub weapons: WeaponTuning,
    pub destruction: DestructionTuning,
    pub round: RoundTuning,
}

/// Weapons (M3). Owned by `src/weapons`; the tuning file documents each value.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct WeaponTuning {
    /// Aim-down-sights button behaviour.
    pub ads_mode: ButtonMode,
    /// Seconds after a sprint ends before you can fire or aim.
    pub sprint_recovery: f32,
    /// Seconds to lower the current weapon when switching.
    pub holster_time: f32,
    /// Pulling the trigger on an empty magazine starts a reload.
    pub auto_reload: bool,
    pub noise: WeaponNoiseTuning,
    pub damage: ZoneDamageTuning,
    pub penetration: PenetrationTuning,
    pub primary: WeaponDef,
    pub sidearm: WeaponDef,
    pub view: WeaponViewTuning,
    pub effects: WeaponEffectsTuning,
    pub dummies: DummyTuning,
}

/// Damage multipliers by body region.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ZoneDamageTuning {
    pub headshot_kills: bool,
    pub head: f32,
    pub torso: f32,
    pub legs: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct PenetrationTuning {
    /// Fraction of damage kept after each soft surface.
    pub soft_damage_kept: f32,
    /// Soft surfaces a bullet passes through before stopping.
    pub max_soft_layers: u32,
    /// Bullet range, metres.
    pub range: f32,
}

/// One weapon.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct WeaponDef {
    pub name: String,
    pub model: WeaponModelTuning,
    pub damage: f32,
    pub falloff_start: f32,
    pub falloff_end: f32,
    pub falloff_min: f32,
    pub rpm: f32,
    pub full_auto: bool,
    pub magazine: u32,
    /// A tactical reload keeps one round in the chamber (magazine + 1).
    pub chamber: bool,
    pub reserve: u32,
    pub reload_tactical: f32,
    pub reload_empty: f32,
    pub draw_time: f32,
    pub ads_time: f32,
    pub ads_zoom: f32,
    pub ads_move_speed: f32,
    pub recoil: RecoilTuning,
    pub spread: SpreadTuning,
    pub pose: ViewmodelPose,
}

/// Degrees of view kick per shot, (right, up).
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct RecoilTuning {
    pub pattern: Vec<(f32, f32)>,
    pub repeat_last: usize,
    pub ads_scale: f32,
    pub recovery_delay: f32,
    pub recovery_speed: f32,
}

/// Bullet spread, degrees (cone radius).
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct SpreadTuning {
    pub hip: f32,
    pub ads: f32,
    pub moving_hip: f32,
    pub moving_ads: f32,
    pub crouch_scale: f32,
    pub prone_scale: f32,
    pub per_shot: f32,
    pub max_bloom: f32,
    pub ads_bloom_scale: f32,
    pub bloom_recovery: f32,
    pub first_shot_scale: f32,
    pub first_shot_reset: f32,
}

/// The 3D model in your hands. Everything the viewmodel needs to know about
/// a model is here, so a different `.glb` can be dropped in without code.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct WeaponModelTuning {
    /// glTF file under `assets/`.
    pub path: String,
    /// Uniform scale (e.g. 0.01 for a model made in centimetres).
    pub scale: f32,
    /// Turn the model so it points down -Z with +Y up: (yaw, pitch, roll),
    /// degrees.
    pub rotation: (f32, f32, f32),
    /// Sight line point (ADS eye alignment) and barrel tip, in model units.
    /// `None` = read them from the nodes named below.
    pub sight: Option<(f32, f32, f32)>,
    pub muzzle: Option<(f32, f32, f32)>,
    pub sight_node: String,
    pub muzzle_node: String,
    /// Nodes animated by reloads and shots (optional).
    pub magazine_node: Option<String>,
    pub slide_node: Option<String>,
    /// How the model sits on a third-person character (see
    /// [`ThirdPersonAttach`]).
    pub third_person: ThirdPersonAttach,
    /// First-person arms holding this model: the clip prefix in
    /// `assets/models/characters/arms_*.glb` (`<arms>_hold`,
    /// `<arms>_reload`, `<arms>_reload_empty`; built for the model's grip,
    /// so use the model they were posed on). None = no arms.
    #[cfg_attr(not(feature = "client"), allow(dead_code))] // arms are client only
    pub arms: Option<String>,
}

/// The weapon model's transform relative to a character's `weapon_r` bone
/// (child of `hand_r`). The bone's frame: origin where the web of the right
/// hand sits on the grip, +Y along the barrel, +Z up, +X to the right. Our
/// weapon models use the same origin but point the barrel down -Z with +Y
/// up, so `rotation` is (0, 90, 0) for them. Used by the third-person
/// bodies (bots, remote players) and the `--lineup` check.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ThirdPersonAttach {
    /// Metres, in the bone's frame (x right, y forward along the barrel, z up).
    pub offset: (f32, f32, f32),
    /// (yaw, pitch, roll) in degrees, applied to the model like
    /// `model.rotation` (Y, then X, then Z).
    pub rotation: (f32, f32, f32),
}

/// Where the weapon sits in front of the camera, metres.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ViewmodelPose {
    /// Model origin at the hip: (right, up, forward).
    pub hip: (f32, f32, f32),
    /// Eye to sight distance when aiming.
    pub ads_distance: f32,
    pub kick_back: f32,
    /// Degrees.
    pub kick_up: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct WeaponViewTuning {
    /// Weapon camera vertical field of view, degrees.
    pub fov: f32,
    pub sway: f32,
    pub sway_max: f32,
    pub bob_height: f32,
    pub bob_side: f32,
    pub ads_steadiness: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct WeaponEffectsTuning {
    pub muzzle_flash_time: f32,
    /// Lumens.
    pub muzzle_light: f32,
    pub impact_time: f32,
    pub max_impacts: usize,
    pub hit_marker_time: f32,
    #[cfg_attr(not(feature = "client"), allow(dead_code))] // sounds only
    pub volume: f32,
}

/// Loudness of weapon sounds to gameplay (`combat::Noise`), footstep scale.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct WeaponNoiseTuning {
    pub gunshot: f32,
    pub reload: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct DummyTuning {
    pub health: f32,
    pub respawn_time: f32,
}

/// Destruction (M4). Owned by `src/destruction`. Explanations of every
/// value are in the tuning file.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct DestructionTuning {
    pub cell_size: f32,
    pub walls: WallTuning,
    pub floors: FloorTuning,
    pub bullet_extra_chance: f32,
    pub pellet_radius: f32,
    pub hole_jitter: f32,
    pub melee: MeleeTuning,
    pub explosion: ExplosionTuning,
    pub hatch_melee_hits: u32,
    pub barricade: BarricadeTuning,
    pub reinforce: ReinforceTuning,
    pub breach_charge: BreachChargeTuning,
    pub debris: DebrisTuning,
    pub noise: DestructionNoiseTuning,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct WallTuning {
    pub frame: f32,
    pub lip: f32,
    pub top_plate: f32,
    pub stud_width: f32,
    pub stud_max_bay: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct FloorTuning {
    pub joist_width: f32,
    pub joist_max_bay: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct MeleeTuning {
    pub range: f32,
    pub sweep_radius: f32,
    pub edge_radius: f32,
    pub cooldown: f32,
    pub damage: f32,
    pub hole_radii: Vec<f32>,
    pub merge_distance: f32,
    pub crouch_hole_hits: u32,
    pub crouch_hole_width: f32,
    pub crouch_hole_height: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ExplosionTuning {
    pub blowout_reach: f32,
    pub blowout_width: f32,
    pub full_damage_fraction: f32,
    pub soft_cover_factor: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct BarricadeTuning {
    pub segment: f32,
    pub melee_hits: u32,
    pub melee_radius: f32,
    pub clear_below: f32,
    pub build_time: f32,
    pub reach: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ReinforceTuning {
    pub time: f32,
    pub reach: f32,
    pub per_defender: u32,
    pub plate_thickness: f32,
    pub brace_thickness: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct BreachChargeTuning {
    pub count: u32,
    pub place_time: f32,
    pub reach: f32,
    pub wall_height: f32,
    pub breach_radius: f32,
    pub damage_radius: f32,
    pub max_damage: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct DestructionNoiseTuning {
    pub explosion: f32,
    pub breaking: f32,
    pub melee: f32,
    pub reinforce: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct DebrisTuning {
    pub max_chunks: usize,
    pub max_dust: usize,
    pub chunks_per_impact: usize,
    pub lifetime: f32,
}

/// Round loop, operators and gadgets (M6). Owned by `src/round` and
/// `src/gadgets`. Every value is explained in the tuning file.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct RoundTuning {
    pub timers: RoundTimers,
    /// Used instead of `timers` with `--round-test`.
    pub test_timers: RoundTimers,
    pub rounds_to_win: u32,
    pub test_rounds_to_win: u32,
    pub swap_sides_every: u32,
    pub attackers_hold_in_prep: bool,
    pub disable_reach: f32,
    pub pickup_reach: f32,
    pub armour_damage: RatingScale,
    pub speed_multiplier: RatingScale,
    pub operators: OperatorRoster,
    pub darts: DartTuning,
    pub stakes: StakeTuning,
    pub intel: IntelTuning,
    #[cfg_attr(not(feature = "client"), allow(dead_code))] // sounds only
    pub volume: f32,
    /// Ring cutter, backplate charges, shared secondaries, drones, CCTV
    /// (M6 part two). Defined in `src/gadgets/tuning.rs`.
    pub gadgets: crate::gadgets::tuning::GadgetTuning,
    /// Which site pair each round uses (maps with several pairs).
    pub site_pairs: Rotation,
    /// Which of the pair's attacker spawn areas each round uses.
    pub attacker_spawns: Rotation,
}

/// How each round picks from a list (site pairs, spawn areas): `Cycle` (in
/// order, one step per round), `Random`, or `Fixed(n)` (1-based).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotation {
    Cycle,
    Random,
    Fixed(u32),
}

/// Seconds for each part of a round.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct RoundTimers {
    pub select: f32,
    pub prep: f32,
    pub action: f32,
    pub plant: f32,
    pub defuser: f32,
    pub disable: f32,
    pub round_end: f32,
    pub match_end: f32,
}

/// A value for each armour or speed rating (1 to 3).
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct RatingScale {
    pub rating_1: f32,
    pub rating_2: f32,
    pub rating_3: f32,
}

impl RatingScale {
    pub fn get(&self, rating: u8) -> f32 {
        match rating {
            0 | 1 => self.rating_1,
            2 => self.rating_2,
            _ => self.rating_3,
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct OperatorRoster {
    pub auger: OperatorNumbers,
    pub lark: OperatorNumbers,
    pub mortise: OperatorNumbers,
    pub fathom: OperatorNumbers,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct OperatorNumbers {
    pub armour: u8,
    pub speed: u8,
    /// How many of the unique gadget the operator carries.
    pub gadgets: u8,
}

/// Lark's sensor darts.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct DartTuning {
    pub speed: f32,
    pub drop: f32,
    pub max_flight: f32,
    pub listen_time: f32,
    pub radius: f32,
    pub min_loudness: f32,
    pub health: f32,
    pub hiss_radius: f32,
}

/// Fathom's seismic stakes.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct StakeTuning {
    pub place_distance: f32,
    pub radius: f32,
    pub vertical_reach: f32,
    pub min_loudness: f32,
    pub ping_interval: f32,
    pub saturate_loudness: f32,
    pub saturate_radius: f32,
    pub saturate_time: f32,
    pub health: f32,
}

/// How long revealed positions stay on screen.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct IntelTuning {
    pub ripple_time: f32,
    pub ping_time: f32,
}

/// Sun, sky, ambient, eye adaptation, post-processing and lanterns. Every
/// value is explained in the tuning file. Read by `src/map/environment.rs`
/// (sun, ambient), `src/map/look.rs` (cameras, sky, fill lights; client
/// only) and `src/map/lamps.rs`.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
#[cfg_attr(not(feature = "client"), allow(dead_code))] // look: client only
pub struct LightingTuning {
    /// Camera exposure, EV100.
    pub exposure: f32,
    /// Sun illuminance, lux.
    pub sun: f32,
    /// Sun colour (linear RGB).
    pub sun_colour: (f32, f32, f32),
    /// Sun height above the horizon, degrees.
    pub sun_elevation: f32,
    /// Sun compass bearing, degrees (0 north, 90 east, 180 south).
    pub sun_azimuth: f32,
    /// Uniform ambient floor everywhere, cd/m2.
    pub ambient: f32,
    /// Sky cubemap (KTX2, under `assets/`).
    pub sky: String,
    /// Sky brightness: the sky's mean luminance, cd/m2.
    pub sky_brightness: f32,
    /// Outdoor fill light (hemisphere), cd/m2, and its colours facing up,
    /// sideways and down (linear RGB).
    pub sky_fill: f32,
    pub sky_fill_colours: ((f32, f32, f32), (f32, f32, f32), (f32, f32, f32)),
    /// Multiplier on the sky fill for the weapon in your hands.
    pub weapon_fill: f32,
    /// Fill light inside buildings, cd/m2, and its colours (up, side, down).
    pub interior_fill: f32,
    pub interior_fill_colours: ((f32, f32, f32), (f32, f32, f32), (f32, f32, f32)),
    /// Haze colour (linear, in the sky image's units: the horizon) and the
    /// distance, metres, at which it hides everything.
    pub fog_colour: (f32, f32, f32),
    pub fog_visibility: f32,
    /// Sun glow in the haze when looking towards the sun (0..1).
    pub fog_sun_glow: f32,
    /// Max eye-adaptation boost in dark scenes, stops (0 = off).
    pub adaptation_max: f32,
    /// Average scene log2 luminance below which adaptation kicks in.
    pub adaptation_threshold: f32,
    /// Eye adaptation speed, stops per second: (brighten, darken).
    pub adaptation_speed: (f32, f32),
    pub tonemapper: Tonemapper,
    pub grading: GradingTuning,
    /// Bloom strength (0 = off) and the brightness it starts at.
    pub bloom: f32,
    pub bloom_threshold: f32,
    /// SSAO sample radius and assumed object thickness, metres.
    pub ssao_radius: f32,
    pub ssao_thickness: f32,
    /// Interior lantern spot-light intensity (game units).
    pub lamp_intensity: f32,
    /// Lantern colour (sRGB).
    pub lamp_colour: (f32, f32, f32),
    /// Lantern light range, metres.
    pub lamp_range: f32,
    /// Lantern glass glow (emissive, cd/m2-ish).
    pub lamp_glow: f32,
    /// Lanterns without shadows (small rooms on the full map): intensity
    /// and range multiplier, so less light leaks through walls.
    pub unshadowed_lamp_scale: f32,
    /// Shadow-casting lanterns only render shadows within this many metres
    /// of the camera (each shadow map costs a render pass).
    pub lamp_shadow_distance: f32,
}

/// Display transform (see Bevy's `Tonemapping`).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(feature = "client"), allow(dead_code))]
pub enum Tonemapper {
    AgX,
    TonyMcMapface,
    AcesFitted,
    BlenderFilmic,
    KhronosPbrNeutral,
}

/// Colour grading before (and saturation after) tonemapping.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
#[cfg_attr(not(feature = "client"), allow(dead_code))]
pub struct GradingTuning {
    /// White balance: + warmer / - cooler (CIE x shift, small values).
    pub temperature: f32,
    /// + magenta / - green.
    pub tint: f32,
    /// Overall saturation (1 = unchanged), and multipliers for the shadows
    /// and the highlights.
    pub saturation: f32,
    pub shadow_saturation: f32,
    pub highlight_saturation: f32,
    /// Highlight brightness multiplier.
    pub highlight_gain: f32,
    /// Saturation after tonemapping.
    pub post_saturation: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct SurfaceTuning {
    /// Metres covered by one texture repeat.
    pub scale: f32,
    /// Linear multiplier on the texture colour.
    pub tint: (f32, f32, f32),
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ViewTuning {
    pub bob_height: f32,
    pub bob_side: f32,
    /// Degrees.
    pub bob_roll: f32,
}

// Serialize: the server sends it to clients (`net/tuning_sync.rs`).
#[derive(Deserialize, serde::Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct FootstepTuning {
    pub walk_stride: f32,
    pub sprint_stride: f32,
    pub crouch_stride: f32,
    pub prone_stride: f32,
    pub walk_loudness: f32,
    pub sprint_loudness: f32,
    pub crouch_loudness: f32,
    pub prone_loudness: f32,
    pub volume: f32,
}

/// Sound mix, distance falloff and surface sounds (`src/audio`). Every value
/// is explained in the tuning file.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
#[cfg_attr(not(feature = "client"), allow(dead_code))] // sounds only
pub struct AudioTuning {
    /// Overall volume (menu: master).
    pub master: f32,
    /// Weapons, impacts, destruction and explosions (menu: effects).
    pub effects: f32,
    /// Everyone's footsteps.
    pub footsteps: f32,
    /// Falloff beyond a category's `near` distance: gain = (near / d) ^ rolloff.
    pub rolloff: f32,
    /// Metres between the listener's ears (stereo panning of world sounds).
    pub ear_gap: f32,
    /// Random pitch per play, +- fraction.
    pub pitch_jitter: f32,
    /// Random volume per play, +- dB.
    pub volume_jitter_db: f32,
    /// Most world sounds playing at once (your own sounds and explosions
    /// always play).
    pub max_voices: usize,
    /// Most bullet-impact sounds per second.
    pub impacts_per_second: f32,
    /// Impacts closer than (metres, seconds) to one already playing are
    /// skipped (pellets, penetration).
    pub impact_merge: (f32, f32),
    /// Distances (metres) over which other people's gunshots and explosions
    /// cross over to the distant layer: (start, fully distant).
    pub distant_blend: (f32, f32),
    /// Non-violent surface damage removing at least this many cells (melee)
    /// also plays a break sound.
    pub break_min_cells: usize,
    pub categories: SoundCategories,
    /// Name pattern -> material, first match wins (see the tuning file).
    pub surfaces: Vec<(String, SoundMaterial)>,
    /// Material for surfaces no pattern matches.
    pub default_surface: SoundMaterial,
}

/// Volume and distance range of one kind of sound.
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(deny_unknown_fields)]
#[cfg_attr(not(feature = "client"), allow(dead_code))] // sounds only
pub struct SoundCategory {
    pub volume: f32,
    /// Full volume up to here, metres.
    pub near: f32,
    /// Silent from here, metres.
    pub far: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
#[cfg_attr(not(feature = "client"), allow(dead_code))] // sounds only
pub struct SoundCategories {
    pub shots: SoundCategory,
    pub shots_distant: SoundCategory,
    pub foley: SoundCategory,
    pub footsteps: SoundCategory,
    pub impacts: SoundCategory,
    pub destruction: SoundCategory,
    pub explosions: SoundCategory,
    pub explosions_distant: SoundCategory,
}

/// What a surface sounds like (footsteps, bullet impacts, breaking).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SoundMaterial {
    Stone,
    Plaster,
    Wood,
    Metal,
    Sand,
    Body,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ControlTuning {
    pub sprint: ButtonMode,
    pub crouch: ButtonMode,
    pub lean: ButtonMode,
}

/// Whether an input acts while held or flips on/off with each press.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonMode {
    Hold,
    Toggle,
}

// Serialize: the server sends it to clients (`net/tuning_sync.rs`).
#[derive(Deserialize, serde::Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct LeanTuning {
    /// Camera roll at full lean, degrees.
    pub angle: f32,
    /// Sideways head/camera shift at full lean, metres.
    pub offset: f32,
    /// Seconds from upright to full lean.
    pub time: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct LookTuning {
    /// Degrees of turn per mouse count.
    pub sensitivity: f32,
    pub invert_y: bool,
    /// Vertical field of view in degrees.
    pub fov: f32,
}

// Serialize: the server sends it to clients (`net/tuning_sync.rs`).
#[derive(Deserialize, serde::Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct MovementTuning {
    pub walk_speed: f32,
    pub sprint_speed: f32,
    pub crouch_speed: f32,
    pub prone_speed: f32,
    pub acceleration: f32,
    pub deceleration: f32,
    pub gravity: f32,
    /// Degrees.
    pub max_slope: f32,
    pub ground_snap: f32,
}

// Serialize: the server sends it to clients (`net/tuning_sync.rs`).
#[derive(Deserialize, serde::Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct BodyTuning {
    pub radius: f32,
    pub stand_height: f32,
    pub crouch_height: f32,
    pub prone_height: f32,
    pub eye_from_top: f32,
    pub crouch_time: f32,
    pub prone_time: f32,
}

#[derive(Resource)]
struct TuningHandle(Handle<Tuning>);

#[derive(Default, TypePath)]
struct TuningLoader;

impl AssetLoader for TuningLoader {
    type Asset = Tuning;
    type Settings = ();
    type Error = BevyError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Tuning, BevyError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(ron::de::from_bytes(&bytes)?)
    }

    fn extensions(&self) -> &[&str] {
        &["ron"]
    }
}

fn load_tuning(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(TuningHandle(assets.load(TUNING_PATH)));
}

fn apply_tuning_changes(
    mut events: MessageReader<AssetEvent<Tuning>>,
    handle: Option<Res<TuningHandle>>,
    assets: Res<Assets<Tuning>>,
    mut tuning: ResMut<Tuning>,
) {
    let Some(handle) = handle else { return };
    for event in events.read() {
        let (AssetEvent::LoadedWithDependencies { id } | AssetEvent::Modified { id }) = event
        else {
            continue;
        };
        if *id != handle.0.id() {
            continue;
        }
        if let Some(new) = assets.get(*id) {
            *tuning = new.clone();
            info!("tuning applied from assets/{TUNING_PATH}");
        }
    }
}
