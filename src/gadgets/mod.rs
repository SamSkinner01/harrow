//! Operator gadgets (M6).
//!
//! - Lark's **sensor darts** (`darts.rs`) and Fathom's **seismic stakes**
//!   (`stakes.rs`) are built. Both produce [`Reveal`] intel that `intel.rs`
//!   shows to the owning team through walls.
//! - Auger's **ring cutter** and Mortise's **backplate charges** are data
//!   only: they need M4's hard-breach API and reinforced walls.
//!
//! Same data flow as the rest of the game: key 3 -> [`GadgetIntent`]
//! (input, PreUpdate) -> fixed-tick simulation -> presentation (Update).
//!
//! Deployed gadgets carry `Health` and a sensor collider on
//! `GameLayer::Hitbox` (like character hit zones), so bullets can find and
//! break them: M3 should send a `DamageMessage` to the hit entity itself
//! when it has `Health` and no `HitZone`. Broken gadgets (`Dead`) are
//! removed here.

mod darts;
mod intel;
mod stakes;

pub use darts::{DartLight, DartState, SensorDart};
pub(crate) use darts::{attach_dart_model, spawn_listening_dart};
pub use stakes::SeismicStake;
pub(crate) use stakes::{attach_stake_model, spawn_stake};

use bevy::prelude::*;
use bevy_enhanced_input::prelude::*;

use crate::{
    combat::{Dead, Team},
    config::BodyTuning,
    player::{CharacterState, LocalPlayer, Player, PlayerInput, input_captured},
    round::{Round, RoundPhase, RoundReset, RoundSystems},
};

pub struct GadgetsPlugin;

impl Plugin for GadgetsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Reveal>()
            .add_message::<GadgetEvent>()
            .add_input_context::<GadgetControls>()
            .configure_sets(FixedUpdate, GadgetSystems.after(RoundSystems::Flow))
            .add_observer(add_gadget_intent)
            .add_systems(Startup, (spawn_controls, create_gadget_assets))
            .add_systems(
                PreUpdate,
                gather_gadget_input
                    .run_if(input_captured)
                    .after(EnhancedInputSystems::Update),
            )
            .add_systems(
                FixedUpdate,
                (refuse_unbuilt_gadgets, remove_broken_gadgets, clear_on_round_reset)
                    .in_set(GadgetSystems),
            )
            .add_systems(Update, log_gadget_events)
            .add_plugins((darts::DartsPlugin, stakes::StakesPlugin, intel::IntelPlugin));
    }
}

/// Fixed-tick gadget simulation; runs after the round flow.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct GadgetSystems;

/// Unique (operator) gadgets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GadgetKind {
    /// Auger: slow, loud hard-breach machine. Waits for M4.
    RingCutter,
    /// Lark.
    SensorDart,
    /// Mortise: anti-breach charge on reinforced walls. Waits for M4.
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

    /// Implemented in this build.
    pub fn is_built(self) -> bool {
        matches!(self, GadgetKind::SensorDart | GadgetKind::SeismicStake)
    }
}

/// Shared secondary gadgets (DESIGN.md "Operator structure"). Data only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// Pressed the unique-gadget key (3). Cleared once handled.
    pub use_unique: bool,
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

/// A deployed gadget that can be shot (has `Health`).
#[derive(Component, Debug)]
pub struct DeployedGadget;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RevealSource {
    SensorDart,
    SeismicStake,
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
}

/// Whether `team` may use gadgets in `phase`: defenders from prep on,
/// attackers once the action phase starts (during prep they would be
/// driving recon drones). Always in free play.
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

/// Shared meshes and materials for deployed gadgets.
#[derive(Resource)]
pub(crate) struct GadgetAssets {
    dart_body: Handle<Mesh>,
    dart_fin: Handle<Mesh>,
    dart_light: Handle<Mesh>,
    stake_spike: Handle<Mesh>,
    stake_head: Handle<Mesh>,
    stake_light: Handle<Mesh>,
    dark_metal: Handle<StandardMaterial>,
    steel: Handle<StandardMaterial>,
    dart_glow: Handle<StandardMaterial>,
    stake_glow: Handle<StandardMaterial>,
    stake_saturated: Handle<StandardMaterial>,
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
    commands.insert_resource(GadgetAssets {
        dart_body: meshes.add(Cylinder::new(0.012, 0.13)),
        dart_fin: meshes.add(Cuboid::new(0.05, 0.035, 0.002)),
        dart_light: meshes.add(Sphere::new(0.016)),
        stake_spike: meshes.add(Cylinder::new(0.016, 0.30)),
        stake_head: meshes.add(Cylinder::new(0.045, 0.035)),
        stake_light: meshes.add(Sphere::new(0.014)),
        dark_metal: mat(Color::srgb(0.12, 0.12, 0.13), 0.6, LinearRgba::BLACK),
        steel: mat(Color::srgb(0.55, 0.55, 0.52), 0.8, LinearRgba::BLACK),
        dart_glow: mat(Color::srgb(0.3, 0.9, 1.0), 0.0, LinearRgba::rgb(0.6, 3.0, 4.0)),
        stake_glow: mat(Color::srgb(0.4, 1.0, 0.4), 0.0, LinearRgba::rgb(0.5, 4.0, 0.5)),
        stake_saturated: mat(Color::srgb(1.0, 0.6, 0.1), 0.0, LinearRgba::rgb(4.0, 1.6, 0.1)),
    });
}

#[derive(Component, Default, Clone)]
struct GadgetControls;

#[derive(InputAction)]
#[action_output(bool)]
struct UseUniqueGadget;

fn spawn_controls(mut commands: Commands) {
    commands.spawn_scene(bsn! {
        GadgetControls
        Actions<GadgetControls> [
            Action::<UseUniqueGadget>::new()
            Bindings [
                Binding(KeyCode::Digit3)
            ]
        ]
    });
}

fn gather_gadget_input(
    unique: Single<&ActionEvents, With<Action<UseUniqueGadget>>>,
    mut intent: Single<&mut GadgetIntent, With<LocalPlayer>>,
) {
    // Latched until the fixed tick handles it.
    if unique.contains(ActionEvents::START) {
        intent.use_unique = true;
    }
}

fn add_gadget_intent(add: On<Add<Player>>, mut commands: Commands) {
    commands.entity(add.entity).insert(GadgetIntent::default());
}

/// Ring cutter and backplate charges wait for M4.
fn refuse_unbuilt_gadgets(
    mut users: Query<(Entity, &mut GadgetIntent, &UniqueGadget)>,
    mut events: MessageWriter<GadgetEvent>,
) {
    for (entity, mut intent, gadget) in &mut users {
        if intent.use_unique && !gadget.kind.is_built() {
            intent.use_unique = false;
            events.write(GadgetEvent::Refused {
                by: entity,
                reason: "not built yet (needs destruction, M4)".into(),
            });
        }
    }
}

fn remove_broken_gadgets(
    broken: Query<(Entity, &Transform), (With<DeployedGadget>, With<Dead>)>,
    mut events: MessageWriter<GadgetEvent>,
    mut commands: Commands,
) {
    for (entity, transform) in &broken {
        events.write(GadgetEvent::Destroyed {
            entity,
            position: transform.translation,
        });
        commands.entity(entity).despawn();
    }
}

/// Every round starts with no gadgets deployed.
fn clear_on_round_reset(
    mut resets: MessageReader<RoundReset>,
    deployed: Query<Entity, Or<(With<SensorDart>, With<SeismicStake>)>>,
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
