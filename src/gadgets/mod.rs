//! Operator and shared gadgets (M6), recon drones and CCTV.
//!
//! - Unique gadgets (key 3): Lark's **sensor darts** (`darts.rs`), Fathom's
//!   **seismic stakes** (`stakes.rs`), Auger's **ring cutter**
//!   (`cutter.rs`) and Mortise's **backplate charges** (`backplate.rs`).
//!   Darts and stakes react to a press; the cutter and backplates use
//!   tap/hold: hold 3 to clamp/bolt/re-seat, tap 3 to start the cutter or
//!   fire the latest backplate.
//! - Shared secondaries (key 4, `secondary.rs`): the one in the
//!   character's [`Loadout`](crate::round::operators::Loadout) responds.
//!   Frag, flashbang, smoke and impact charge are thrown
//!   (`throwables.rs`); razor wire, ballistic cover and the proximity alarm
//!   are placed (`deployables.rs`). The soft-breach charge belongs to
//!   destruction (M4); it only acts for characters with breach charges.
//! - Recon (key G): attackers' drones (`drone.rs`), defenders' CCTV
//!   (`cctv.rs`), seen through one remote camera (`remote_view.rs`).
//! - Intel (`intel.rs`): [`Reveal`]s drawn for the owning team.
//!
//! Same data flow as the rest of the game: keys -> intents
//! ([`GadgetIntent`], [`DroneIntent`]) in PreUpdate -> fixed-tick
//! simulation on plain components -> presentation (Update). Bots and the
//! network can drive everything by writing the intents.
//!
//! Deployed gadgets that can be shot carry `Health` and a sensor collider on
//! `GameLayer::Hitbox` (like character hit zones), so bullets can find and
//! break them: M3 should send a `DamageMessage` to the hit entity itself
//! when it has `Health` and no `HitZone`. Broken gadgets (`Dead`) are
//! removed here.

mod backplate;
mod cctv;
mod cutter;
mod darts;
mod deployables;
mod drone;
mod effects;
mod hud;
mod intel;
mod particles;
mod remote_view;
mod secondary;
mod sfx;
mod stakes;
mod test_script;
mod throwables;
pub mod tuning;

pub use cctv::CctvCamera;
pub use darts::{DartLight, DartState, SensorDart};
pub(crate) use darts::{attach_dart_model, spawn_listening_dart};
pub use drone::ReconStock;
pub use remote_view::{RemoteView, RemoteViewCamera};
pub use secondary::SecondaryStock;
pub use stakes::SeismicStake;
pub(crate) use stakes::{attach_stake_model, spawn_stake};
// Gadget state for bots (M7), networking (M8) and audio: not all used in
// this crate yet.
#[allow(unused_imports)]
pub use backplate::BackplateCharge;
#[allow(unused_imports)]
pub use cctv::CctvLayout;
#[allow(unused_imports)]
pub use cutter::{CutterBlade, CutterState, RingCutter};
#[allow(unused_imports)]
pub use deployables::{BallisticCover, GadgetSpeedScale, ProximityAlarm, RazorWire};
#[allow(unused_imports)]
pub use drone::Drone;
#[allow(unused_imports)]
pub use effects::{Flashed, HearingMuffled};
#[allow(unused_imports)]
pub use throwables::{Grenade, SmokeCloud, smoke_blocks};

use bevy::prelude::*;
use bevy_enhanced_input::prelude::*;

use crate::{
    combat::{Dead, Team},
    config::BodyTuning,
    destruction::DestructionSystems,
    player::{CharacterState, HitZone, LocalPlayer, Player, PlayerInput, input_captured},
    round::{Round, RoundPhase, RoundReset, RoundSystems},
};

pub struct GadgetsPlugin;

impl Plugin for GadgetsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Reveal>()
            .add_message::<GadgetEvent>()
            .add_input_context::<GadgetControls>()
            .configure_sets(
                FixedUpdate,
                (
                    GadgetSystems
                        .after(RoundSystems::Flow)
                        // Grenades and charges write explosions and hard
                        // breaches that destruction applies on the same tick.
                        .before(DestructionSystems::Explosions),
                    (
                        GadgetStage::Input,
                        GadgetStage::Use,
                        GadgetStage::Simulate,
                        GadgetStage::Effects,
                    )
                        .chain()
                        .in_set(GadgetSystems),
                ),
            )
            .add_observer(add_gadget_components)
            .add_systems(Startup, (spawn_controls, create_gadget_assets))
            .add_systems(
                PreUpdate,
                (
                    gather_gadget_input.run_if(input_captured),
                    release_gadget_input.run_if(not(input_captured)),
                )
                    .after(EnhancedInputSystems::Update),
            )
            .add_systems(
                FixedUpdate,
                (
                    track_buttons.in_set(GadgetStage::Input),
                    (remove_broken_gadgets, clear_on_round_reset).in_set(GadgetSystems),
                ),
            )
            .add_systems(Update, log_gadget_events)
            .add_plugins((
                darts::DartsPlugin,
                stakes::StakesPlugin,
                intel::IntelPlugin,
                cutter::CutterPlugin,
                backplate::BackplatePlugin,
                secondary::SecondaryPlugin,
                throwables::ThrowablesPlugin,
                deployables::DeployablesPlugin,
            ))
            .add_plugins((
                drone::DronePlugin,
                cctv::CctvPlugin,
                remote_view::RemoteViewPlugin,
                effects::EffectsPlugin,
                particles::ParticlesPlugin,
                sfx::GadgetSfxPlugin,
                hud::GadgetHudPlugin,
                test_script::GadgetTestPlugin,
            ));
    }
}

/// Fixed-tick gadget simulation; runs after the round flow and before
/// destruction applies explosions.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct GadgetSystems;

/// Order inside [`GadgetSystems`].
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum GadgetStage {
    /// Button edges (tap / hold) from the intents.
    Input,
    /// Characters use gadgets: place, throw, trigger, drive.
    Use,
    /// Deployed gadgets work: fuses, cutting, flight, detection.
    Simulate,
    /// Consequences: slow, flash, jams, damage to gadgets.
    Effects,
}

/// Unique (operator) gadgets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GadgetKind {
    /// Auger: slow, loud hard-breach machine.
    RingCutter,
    /// Lark.
    SensorDart,
    /// Mortise: anti-breach charge on reinforced walls.
    BackplateCharge,
    /// Fathom.
    SeismicStake,
}

impl GadgetKind {
    pub fn name(self) -> &'static str {
        match self {
            GadgetKind::RingCutter => "Ring Cutter",
            GadgetKind::SensorDart => "Sensor Darts",
            GadgetKind::BackplateCharge => "Backplate Charges",
            GadgetKind::SeismicStake => "Seismic Stakes",
        }
    }

    /// How to use it (HUD hint).
    pub fn hint(self) -> &'static str {
        match self {
            GadgetKind::RingCutter => "hold 3 on a wall or hatch to clamp, tap 3 to start",
            GadgetKind::SensorDart => "press 3 to fire",
            GadgetKind::BackplateCharge => "hold 3 on your reinforcement, tap 3 to fire",
            GadgetKind::SeismicStake => "press 3 to hammer in",
        }
    }
}

/// Shared secondary gadgets (DESIGN.md "Operator structure").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SecondaryGadget {
    SoftBreachCharge,
    Frag,
    Flashbang,
    Smoke,
    RazorWire,
    BallisticCover,
    ImpactCharge,
    ProximityAlarm,
}

impl SecondaryGadget {
    pub const ALL: [SecondaryGadget; 8] = [
        SecondaryGadget::SoftBreachCharge,
        SecondaryGadget::Frag,
        SecondaryGadget::Flashbang,
        SecondaryGadget::Smoke,
        SecondaryGadget::RazorWire,
        SecondaryGadget::BallisticCover,
        SecondaryGadget::ImpactCharge,
        SecondaryGadget::ProximityAlarm,
    ];

    pub fn name(self) -> &'static str {
        match self {
            SecondaryGadget::SoftBreachCharge => "Soft-breach charge",
            SecondaryGadget::Frag => "Frag grenade",
            SecondaryGadget::Flashbang => "Flashbang",
            SecondaryGadget::Smoke => "Smoke grenade",
            SecondaryGadget::RazorWire => "Razor wire",
            SecondaryGadget::BallisticCover => "Ballistic cover",
            SecondaryGadget::ImpactCharge => "Impact charge",
            SecondaryGadget::ProximityAlarm => "Proximity alarm",
        }
    }

    /// Command-line / script name (`--secondary frag`).
    pub fn from_name(name: &str) -> Option<Self> {
        let name = name.to_ascii_lowercase().replace(['-', '_', ' '], "");
        Self::ALL.into_iter().find(|g| {
            let full = g.name().to_ascii_lowercase().replace(['-', ' '], "");
            let short = match g {
                SecondaryGadget::SoftBreachCharge => "breach",
                SecondaryGadget::Frag => "frag",
                SecondaryGadget::Flashbang => "flash",
                SecondaryGadget::Smoke => "smoke",
                SecondaryGadget::RazorWire => "wire",
                SecondaryGadget::BallisticCover => "cover",
                SecondaryGadget::ImpactCharge => "impact",
                SecondaryGadget::ProximityAlarm => "alarm",
            };
            name == full || name == short
        })
    }

    /// Thrown with 4 (released to throw) rather than placed (held).
    pub fn is_thrown(self) -> bool {
        matches!(
            self,
            SecondaryGadget::Frag
                | SecondaryGadget::Flashbang
                | SecondaryGadget::Smoke
                | SecondaryGadget::ImpactCharge
        )
    }
}

/// The operator's unique gadget and how many are left this round. Set by
/// the round module when prep starts.
#[derive(Component, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UniqueGadget {
    pub kind: GadgetKind,
    pub remaining: u8,
    pub max: u8,
}

/// What a character wants to do with gadgets. Written by input, bots or
/// the network; read by the fixed tick.
#[derive(Component, Default, Debug, Clone)]
pub struct GadgetIntent {
    /// Pressed the unique-gadget key (3). Cleared once handled (darts,
    /// stakes).
    pub use_unique: bool,
    /// The unique-gadget key (3) is held (ring cutter, backplate: tap/hold).
    pub unique_held: bool,
    /// The secondary-gadget key (4) is held.
    pub secondary_held: bool,
}

/// What a character wants from recon: drones (attackers) and CCTV
/// (defenders). Written by input, bots or the network.
#[derive(Component, Default, Debug, Clone)]
pub struct DroneIntent {
    /// The recon key (G) is held: tap = connect / disconnect, hold = throw
    /// another drone.
    pub recon_held: bool,
    /// Pressed previous (-1) / next (+1) drone or camera. Cleared once
    /// handled.
    pub cycle: i32,
    /// Drive axis while connected: x = turn right, y = forward.
    pub drive: Vec2,
    /// Absolute look angles while connected (radians): the drone's heading
    /// and camera pitch, or a CCTV camera's pan and tilt. The simulation
    /// resets them when a view is entered.
    pub yaw: f32,
    pub pitch: f32,
}

/// Who deployed a gadget.
#[derive(Component, Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct GadgetOwner {
    /// The deploying character (`None` for test-script gadgets). For kill
    /// credit and bots later.
    #[allow(dead_code)]
    #[entities]
    pub entity: Option<Entity>,
    pub team: Team,
}

/// A deployed gadget that can be shot (has `Health`); removed when it
/// breaks.
#[derive(Component, Debug)]
pub struct DeployedGadget;

/// Despawned at the start of every round (thrown, placed and cosmetic
/// gadget entities).
#[derive(Component, Debug)]
pub struct RoundScoped;

/// A key's state on the fixed tick, derived from a held flag in an intent.
#[derive(Clone, Copy, Debug, Default)]
pub struct ButtonTrack {
    pub held: bool,
    /// Seconds held so far (0 on the press tick).
    pub held_for: f32,
    pub pressed: bool,
    pub released: bool,
    /// Released after less than `tap_time`.
    pub tapped: bool,
}

impl ButtonTrack {
    fn update(&mut self, held: bool, dt: f32, tap_time: f32) {
        self.pressed = held && !self.held;
        self.released = !held && self.held;
        self.tapped = self.released && self.held_for < tap_time;
        if self.pressed {
            self.held_for = 0.0;
        } else if held {
            self.held_for += dt;
        }
        self.held = held;
    }
}

/// Button edges for the gadget keys (simulation state, per character).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct GadgetButtons {
    pub unique: ButtonTrack,
    pub secondary: ButtonTrack,
    pub recon: ButtonTrack,
}

/// What a character is busy doing with a gadget (for HUDs and bots).
#[derive(Component, Default, Debug, Clone, Copy, PartialEq)]
pub enum GadgetActivity {
    #[default]
    Idle,
    /// Holding 3 on a wall or hatch: clamping a ring cutter.
    Clamping { surface: Entity, progress: f32 },
    /// Holding 3 at a jammed ring cutter.
    Reseating { cutter: Entity, progress: f32 },
    /// Holding 3 on a reinforced plate: bolting a backplate charge.
    Bolting { surface: Entity, progress: f32 },
    /// Holding 4: setting down a deployable.
    Placing { kind: SecondaryGadget, progress: f32 },
    /// Holding 4 with a grenade in hand. `fuse`: seconds left if cooking.
    /// `total`: the full fuse (for the HUD bar).
    Cooking { kind: SecondaryGadget, fuse: Option<f32>, total: f32 },
}

impl GadgetActivity {
    /// Label and 0..1 progress for the HUD.
    pub fn progress(&self) -> Option<(String, f32)> {
        match *self {
            GadgetActivity::Idle => None,
            GadgetActivity::Clamping { progress, .. } => Some(("Clamping ring cutter".into(), progress)),
            GadgetActivity::Reseating { progress, .. } => Some(("Re-seating ring cutter".into(), progress)),
            GadgetActivity::Bolting { progress, .. } => Some(("Bolting backplate charge".into(), progress)),
            GadgetActivity::Placing { kind, progress } => Some((format!("Placing {}", kind.name().to_lowercase()), progress)),
            GadgetActivity::Cooking { kind, fuse, total } => Some(match fuse {
                Some(left) => (format!("{} cooking  {left:.1} s", kind.name()), 1.0 - left / total.max(0.01)),
                None => (format!("{}: release 4 to throw", kind.name()), 1.0),
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RevealSource {
    SensorDart,
    SeismicStake,
    ProximityAlarm,
}

/// Intel: an enemy position revealed to `for_team`.
/// Forwarded to the matching team's clients (`net/events.rs`).
#[derive(Message, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Reveal {
    pub for_team: Team,
    pub position: Vec3,
    pub source: RevealSource,
    /// The character that gave itself away.
    pub revealed: Option<Entity>,
}

/// Gadget happenings, for presentation and the log.
/// Forwarded to clients by the server (`net/events.rs`).
// Every field is logged (Debug) and meant for presentation and bots;
// not all are read in code yet.
#[allow(dead_code)]
#[derive(Message, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum GadgetEvent {
    DartFired { by: Entity },
    DartStuck { position: Vec3, team: Team },
    DartLost,
    DartExpired { position: Vec3 },
    StakePlaced { by: Entity, position: Vec3 },
    StakePing { stake: Entity, position: Vec3, team: Team },
    StakeSaturated { stake: Entity },
    Refused {
        by: Entity,
        /// A `Cow` so the event can be sent over the network.
        reason: std::borrow::Cow<'static, str>,
    },
    Destroyed { entity: Entity, position: Vec3 },
    CutterClamped { by: Entity, cutter: Entity, position: Vec3, reinforced: bool },
    CutterStarted { cutter: Entity },
    CutterExposed { cutter: Entity, position: Vec3 },
    CutterJammed { cutter: Entity, position: Vec3 },
    CutterReseated { cutter: Entity },
    CutterBreached { cutter: Entity, position: Vec3, knocked_down: u32 },
    BackplateSet { by: Entity, position: Vec3 },
    BackplateFired { position: Vec3, destroyed: u32 },
    Thrown { by: Entity, kind: SecondaryGadget },
    Detonated { kind: SecondaryGadget, position: Vec3 },
    Flashed { entity: Entity, strength: f32 },
    Placed { by: Entity, kind: SecondaryGadget, position: Vec3 },
    /// Someone stepped in razor wire.
    WireRattle { by: Entity, position: Vec3 },
    AlarmTriggered { alarm: Entity, position: Vec3 },
    DroneThrown { by: Entity, drone: Entity },
    ViewEntered { by: Entity, view: RemoteView },
    /// Back in the body; `lost`: the feed was cut (drone or camera broke).
    ViewLeft { by: Entity, lost: bool },
}

/// Whether `team` may use gadgets in `phase`: defenders from prep on,
/// attackers once the action phase starts (during prep they drive recon
/// drones). Always in free play.
pub fn gadgets_allowed(phase: RoundPhase, team: Team) -> bool {
    match phase {
        RoundPhase::Inactive | RoundPhase::Action | RoundPhase::Planted => true,
        RoundPhase::Prep => team == Team::Defenders,
        _ => false,
    }
}

/// Eye position and look direction of a character.
pub fn eye_and_look(
    transform: &Transform,
    state: &CharacterState,
    input: &PlayerInput,
    body: &BodyTuning,
    lean_offset: f32,
) -> (Vec3, Dir3) {
    let right = Quat::from_rotation_y(input.yaw) * Vec3::X;
    let eye = transform.translation
        + Vec3::Y * (state.height - body.eye_from_top)
        + right * (state.lean * lean_offset);
    let look = Quat::from_euler(EulerRot::YXZ, input.yaw, input.pitch, 0.0) * Vec3::NEG_Z;
    (eye, Dir3::new(look).unwrap_or(Dir3::NEG_Z))
}

/// Take a pending unique-gadget press of `kind` if this character may use
/// it now. Returns false (and reports why) otherwise.
#[allow(clippy::too_many_arguments)]
pub(crate) fn take_gadget_use(
    entity: Entity,
    intent: &mut GadgetIntent,
    gadget: &UniqueGadget,
    kind: GadgetKind,
    team: Team,
    dead: bool,
    round: &Round,
    events: &mut MessageWriter<GadgetEvent>,
) -> bool {
    if gadget.kind != kind || !intent.use_unique {
        return false;
    }
    intent.use_unique = false;
    let reason = if dead {
        Some("dead")
    } else if !gadgets_allowed(round.phase, team) {
        Some("gadgets can't be used right now")
    } else if gadget.remaining == 0 {
        Some("none left")
    } else {
        None
    };
    if let Some(reason) = reason {
        events.write(GadgetEvent::Refused {
            by: entity,
            reason: reason.into(),
        });
        return false;
    }
    true
}

/// Why a character can't use gadgets right now, if it can't.
pub(crate) fn gadget_block(dead: bool, phase: RoundPhase, team: Team, viewing: bool) -> Option<&'static str> {
    if dead {
        Some("dead")
    } else if viewing {
        Some("in a remote view")
    } else if !gadgets_allowed(phase, team) {
        Some("gadgets can't be used right now")
    } else {
        None
    }
}

/// Shared meshes and materials for deployed gadgets.
#[derive(Resource)]
pub(crate) struct GadgetAssets {
    dart_body: Handle<Mesh>,
    dart_fin: Handle<Mesh>,
    dart_light: Handle<Mesh>,
    stake_spike: Handle<Mesh>,
    stake_head: Handle<Mesh>,
    stake_light: Handle<Mesh>,
    pub(crate) dark_metal: Handle<StandardMaterial>,
    pub(crate) steel: Handle<StandardMaterial>,
    dart_glow: Handle<StandardMaterial>,
    stake_glow: Handle<StandardMaterial>,
    stake_saturated: Handle<StandardMaterial>,
    /// Unlit red indicator light.
    pub(crate) red_light: Handle<StandardMaterial>,
    /// Unlit green indicator light.
    pub(crate) green_light: Handle<StandardMaterial>,
    /// Glowing orange (hot metal, sparks).
    pub(crate) hot: Handle<StandardMaterial>,
    /// Olive-drab painted steel.
    pub(crate) olive: Handle<StandardMaterial>,
    /// Black rubber / plastic.
    pub(crate) rubber: Handle<StandardMaterial>,
    /// Dark glass (lenses, vision blocks).
    pub(crate) glass: Handle<StandardMaterial>,
    pub(crate) unit_cube: Handle<Mesh>,
    pub(crate) unit_sphere: Handle<Mesh>,
    pub(crate) unit_cylinder: Handle<Mesh>,
}

fn create_gadget_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut mat = |color: Color, metallic: f32, emissive: LinearRgba| {
        materials.add(StandardMaterial {
            base_color: color,
            metallic,
            perceptual_roughness: 0.45,
            emissive,
            ..default()
        })
    };
    let dark_metal = mat(Color::srgb(0.12, 0.12, 0.13), 0.6, LinearRgba::BLACK);
    let steel = mat(Color::srgb(0.55, 0.55, 0.52), 0.8, LinearRgba::BLACK);
    let dart_glow = mat(Color::srgb(0.3, 0.9, 1.0), 0.0, LinearRgba::rgb(0.6, 3.0, 4.0));
    let stake_glow = mat(Color::srgb(0.4, 1.0, 0.4), 0.0, LinearRgba::rgb(0.5, 4.0, 0.5));
    let stake_saturated = mat(Color::srgb(1.0, 0.6, 0.1), 0.0, LinearRgba::rgb(4.0, 1.6, 0.1));
    let hot = mat(Color::srgb(1.0, 0.55, 0.15), 0.0, LinearRgba::rgb(14.0, 5.0, 0.8));
    let olive = materials.add(StandardMaterial {
        base_color: Color::srgb(0.27, 0.29, 0.20),
        metallic: 0.5,
        perceptual_roughness: 0.7,
        ..default()
    });
    let rubber = materials.add(StandardMaterial {
        base_color: Color::srgb(0.05, 0.05, 0.05),
        perceptual_roughness: 0.9,
        ..default()
    });
    let glass = materials.add(StandardMaterial {
        base_color: Color::srgb(0.02, 0.03, 0.04),
        metallic: 0.2,
        perceptual_roughness: 0.08,
        reflectance: 0.8,
        ..default()
    });
    let mut unlit = |color: Color| {
        materials.add(StandardMaterial {
            base_color: color,
            unlit: true,
            ..default()
        })
    };
    commands.insert_resource(GadgetAssets {
        dart_body: meshes.add(Cylinder::new(0.012, 0.13)),
        dart_fin: meshes.add(Cuboid::new(0.05, 0.035, 0.002)),
        dart_light: meshes.add(Sphere::new(0.016)),
        stake_spike: meshes.add(Cylinder::new(0.016, 0.30)),
        stake_head: meshes.add(Cylinder::new(0.045, 0.035)),
        stake_light: meshes.add(Sphere::new(0.014)),
        dark_metal,
        steel,
        dart_glow,
        stake_glow,
        stake_saturated,
        red_light: unlit(Color::srgb(1.0, 0.08, 0.04)),
        green_light: unlit(Color::srgb(0.2, 1.0, 0.3)),
        hot,
        olive,
        rubber,
        glass,
        unit_cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        unit_sphere: meshes.add(Sphere::new(0.5)),
        unit_cylinder: meshes.add(Cylinder::new(0.5, 1.0)),
    });
}

/// Child marking where blasts measure their distance to a gadget.
/// Destruction's explosion damage uses a target's hit zones, or else a
/// point 1 m above it (inside the ceiling for a wall camera). This zone
/// has no collider, so bullets and melee never see it.
pub(crate) fn blast_anchor(at: Vec3) -> impl Bundle {
    (Name::new("Blast anchor"), HitZone::Torso, Transform::from_translation(at))
}

/// A box of `size` at `at` (local), using the shared unit cube.
pub(crate) fn part(
    assets: &GadgetAssets,
    material: &Handle<StandardMaterial>,
    at: Vec3,
    size: Vec3,
) -> (Mesh3d, MeshMaterial3d<StandardMaterial>, Transform) {
    (
        Mesh3d(assets.unit_cube.clone()),
        MeshMaterial3d(material.clone()),
        Transform::from_translation(at).with_scale(size),
    )
}

#[derive(Component, Default, Clone)]
struct GadgetControls;

#[derive(InputAction)]
#[action_output(bool)]
struct UseUniqueGadget;

#[derive(InputAction)]
#[action_output(bool)]
struct UseSecondaryGadget;

#[derive(InputAction)]
#[action_output(bool)]
struct Recon;

#[derive(InputAction)]
#[action_output(bool)]
struct ReconPrevious;

#[derive(InputAction)]
#[action_output(bool)]
struct ReconNext;

#[derive(InputAction)]
#[action_output(Vec2)]
struct DroneDrive;

#[derive(InputAction)]
#[action_output(Vec2)]
struct DroneLook;

fn spawn_controls(mut commands: Commands) {
    // Key 4 is also bound by destruction (breach charge); neither binding
    // consumes the key, and only the loadout's gadget acts on it.
    commands.spawn_scene(bsn! {
        GadgetControls
        Actions<GadgetControls> [
            Action::<UseUniqueGadget>::new()
            Bindings [
                Binding(KeyCode::Digit3)
            ]
            --
            Action::<UseSecondaryGadget>::new()
            Bindings [
                Binding(KeyCode::Digit4)
            ]
            --
            Action::<Recon>::new()
            Bindings [
                Binding(KeyCode::KeyG)
            ]
            --
            Action::<ReconPrevious>::new()
            Bindings [
                Binding(KeyCode::KeyQ)
            ]
            --
            Action::<ReconNext>::new()
            Bindings [
                Binding(KeyCode::KeyE)
            ]
            --
            Action::<DroneDrive>::new()
            DeadZone
            Bindings [
                {preset::cardinal::wasd_keys()}
            ]
            --
            Action::<DroneLook>::new()
            Bindings [
                Binding::mouse_motion()
            ]
        ]
    });
}

const DRONE_PITCH_LIMIT: f32 = 1.2;

#[allow(clippy::too_many_arguments)]
fn gather_gadget_input(
    unique: Single<(&Action<UseUniqueGadget>, &ActionEvents)>,
    secondary: Single<&Action<UseSecondaryGadget>>,
    recon: Single<&Action<Recon>>,
    previous: Single<&ActionEvents, With<Action<ReconPrevious>>>,
    next: Single<&ActionEvents, With<Action<ReconNext>>>,
    drive: Single<&Action<DroneDrive>>,
    look: Single<&Action<DroneLook>>,
    tuning: Res<crate::config::Tuning>,
    time: Res<Time>,
    mut intents: Query<(&mut GadgetIntent, &mut DroneIntent, Option<&RemoteView>), With<LocalPlayer>>,
) {
    let Ok((mut intent, mut drone, view)) = intents.single_mut() else {
        return;
    };
    let (unique_held, unique_events) = unique.into_inner();
    // Latched until the fixed tick handles it.
    if unique_events.contains(ActionEvents::START) {
        intent.use_unique = true;
    }
    intent.unique_held = **unique_held;
    intent.secondary_held = **secondary.into_inner();
    drone.recon_held = **recon.into_inner();
    let Some(view) = view else {
        drone.drive = Vec2::ZERO;
        return;
    };
    if previous.contains(ActionEvents::START) {
        drone.cycle -= 1;
    }
    if next.contains(ActionEvents::START) {
        drone.cycle += 1;
    }
    let g = &tuning.round.gadgets;
    let axis = drive.into_inner().clamp_length_max(1.0);
    drone.drive = axis;
    let delta = **look.into_inner();
    let sensitivity = tuning.look.sensitivity.to_radians();
    let invert = if tuning.look.invert_y { -1.0 } else { 1.0 };
    // A/D turn the drone (or pan the camera) too.
    let turn = axis.x * g.drone.turn_rate.to_radians() * time.delta_secs();
    let yaw = drone.yaw - delta.x * sensitivity - turn;
    let pitch = drone.pitch - delta.y * sensitivity * invert;
    match view {
        RemoteView::Drone(_) => {
            drone.yaw = yaw.rem_euclid(std::f32::consts::TAU);
            drone.pitch = pitch.clamp(-DRONE_PITCH_LIMIT, DRONE_PITCH_LIMIT);
        }
        RemoteView::Camera(_) => {
            // Pan and tilt within the mount's limits.
            let (pan, tilt) = (g.cctv.pan_limit.to_radians(), g.cctv.tilt_limit.to_radians());
            drone.yaw = yaw.clamp(-pan, pan);
            drone.pitch = pitch.clamp(-tilt, tilt);
        }
    }
}

/// Mouse released (menus, alt-tab): let go of every gadget key.
fn release_gadget_input(mut intents: Query<(&mut GadgetIntent, &mut DroneIntent), With<LocalPlayer>>) {
    for (mut intent, mut drone) in &mut intents {
        intent.unique_held = false;
        intent.secondary_held = false;
        drone.recon_held = false;
        drone.drive = Vec2::ZERO;
    }
}

fn add_gadget_components(add: On<Add<Player>>, tuning: Res<crate::config::Tuning>, mut commands: Commands) {
    let drones = tuning.round.gadgets.drone.count;
    commands.entity(add.entity).insert((
        GadgetIntent::default(),
        DroneIntent::default(),
        GadgetButtons::default(),
        GadgetActivity::default(),
        ReconStock {
            drones,
            max: drones,
        },
    ));
}

fn track_buttons(
    tuning: Res<crate::config::Tuning>,
    time: Res<Time>,
    mut characters: Query<(&GadgetIntent, &DroneIntent, &mut GadgetButtons)>,
) {
    let dt = time.delta_secs();
    let tap = tuning.round.gadgets.tap_time;
    for (intent, drone, mut buttons) in &mut characters {
        buttons.unique.update(intent.unique_held, dt, tap);
        buttons.secondary.update(intent.secondary_held, dt, tap);
        buttons.recon.update(drone.recon_held, dt, tap);
    }
}

fn remove_broken_gadgets(
    broken: Query<(Entity, &GlobalTransform), (With<DeployedGadget>, With<Dead>)>,
    mut events: MessageWriter<GadgetEvent>,
    mut commands: Commands,
) {
    for (entity, transform) in &broken {
        events.write(GadgetEvent::Destroyed {
            entity,
            position: transform.translation(),
        });
        commands.entity(entity).despawn();
    }
}

/// Every round starts with no gadgets deployed and nobody busy.
fn clear_on_round_reset(
    mut resets: MessageReader<RoundReset>,
    deployed: Query<Entity, Or<(With<SensorDart>, With<SeismicStake>, With<RoundScoped>)>>,
    mut characters: Query<&mut GadgetActivity>,
    mut commands: Commands,
) {
    let Some(reset) = resets.read().last() else {
        return;
    };
    let mut count = 0;
    for entity in &deployed {
        commands.entity(entity).despawn();
        count += 1;
    }
    for mut activity in &mut characters {
        *activity = GadgetActivity::Idle;
    }
    if count > 0 {
        info!("gadget: cleared {count} deployed gadgets for round {}", reset.round);
    }
}

fn log_gadget_events(mut events: MessageReader<GadgetEvent>, mut reveals: MessageReader<Reveal>) {
    for event in events.read() {
        match event {
            GadgetEvent::DartFired { by } => info!("gadget: {by} fired a sensor dart"),
            GadgetEvent::DartStuck { position, team } => {
                info!("gadget: {team:?} sensor dart stuck at {position:.2}, listening")
            }
            GadgetEvent::DartLost => info!("gadget: sensor dart hit nothing"),
            GadgetEvent::DartExpired { position } => {
                info!("gadget: sensor dart at {position:.2} stopped listening")
            }
            GadgetEvent::StakePlaced { by, position } => {
                info!("gadget: {by} placed a seismic stake at {position:.2}")
            }
            GadgetEvent::StakePing { stake, position, team } => {
                info!("gadget: stake {stake} PING for {team:?}: footstep at {position:.2}")
            }
            GadgetEvent::StakeSaturated { stake } => {
                info!("gadget: stake {stake} saturated by a loud noise")
            }
            GadgetEvent::Refused { by, reason } => info!("gadget: {by} can't use gadget: {reason}"),
            GadgetEvent::Destroyed { entity, position } => {
                info!("gadget: {entity} destroyed at {position:.2}")
            }
            other => info!("gadget: {other:?}"),
        }
    }
    for reveal in reveals.read() {
        info!(
            "intel: {:?} revealed {} at {:.2} to {:?}",
            reveal.source,
            reveal.revealed.map_or("?".into(), |e| e.to_string()),
            reveal.position,
            reveal.for_team
        );
    }
}
