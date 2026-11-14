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
    pub weapons: WeaponTuning,
    #[allow(dead_code)]
    pub destruction: DestructionTuning,
    #[allow(dead_code)]
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
    /// `assets/models/weapons/<model>.glb`
    pub model: String,
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

/// Where the weapon sits in front of the camera, metres.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ViewmodelPose {
    /// Grip position at the hip: (right, up, forward).
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
    pub volume: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct DummyTuning {
    pub health: f32,
    pub respawn_time: f32,
}

/// Destruction (M4). Owned by `src/destruction`.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct DestructionTuning {}

/// Round loop and gadgets (M6). Owned by `src/round` and `src/gadgets`.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct RoundTuning {}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct LightingTuning {
    /// Camera exposure, EV100.
    pub exposure: f32,
    /// Sun illuminance, lux.
    pub sun: f32,
    /// Ambient (sky fill) brightness, cd/m2.
    pub ambient: f32,
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
