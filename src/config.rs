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
            .add_systems(PreUpdate, apply_tuning_changes);
    }
}

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
    // Placeholder sections, filled in by M3 / M4 / M6.
    #[allow(dead_code)]
    pub weapons: WeaponTuning,
    pub destruction: DestructionTuning,
    pub round: RoundTuning,
}

/// Weapons (M3). Owned by `src/weapons`.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct WeaponTuning {}

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
    pub volume: f32,
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

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct LightingTuning {
    /// Camera exposure, EV100.
    pub exposure: f32,
    /// Sun illuminance, lux.
    pub sun: f32,
    /// Ambient (sky fill) brightness, cd/m2.
    pub ambient: f32,
    /// Max eye-adaptation boost in dark scenes, stops (0 = off).
    pub adaptation_max: f32,
    /// Average scene log2 luminance below which adaptation kicks in.
    pub adaptation_threshold: f32,
    /// Interior lantern spot-light intensity (game units).
    pub lamp_intensity: f32,
    /// Lantern colour (sRGB).
    pub lamp_colour: (f32, f32, f32),
    /// Lantern light range, metres.
    pub lamp_range: f32,
    /// Lanterns without shadows (small rooms on the full map): intensity
    /// and range multiplier, so less light leaks through walls.
    pub unshadowed_lamp_scale: f32,
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

#[derive(Deserialize, Clone, Debug)]
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

#[derive(Deserialize, Clone, Debug)]
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

#[derive(Deserialize, Clone, Debug)]
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

#[derive(Deserialize, Clone, Debug)]
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
