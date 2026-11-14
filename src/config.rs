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
