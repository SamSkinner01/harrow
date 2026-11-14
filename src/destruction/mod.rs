//! Destruction (M4): soft walls, floors, hatches and barricades that open up
//! cell by cell; melee; reinforcement; the breach charge; hard breaches.
//!
//! How it fits together (see DESIGN.md, "Destruction" and "M4 implementation
//! notes"):
//!
//! - **Surfaces** (`surface.rs`, `spawn.rs`): when a soft kit piece spawns
//!   (plaster walls, wooden floors, the hatch tile and cover, barricades),
//!   its glTF content is replaced by a [`SoftSurface`]: a grid of ~10 cm
//!   cells with timber studs/joists inside. The mesh (`build.rs`) and a
//!   compound box collider are rebuilt from the cells, for that one surface,
//!   only when it changes.
//! - **Impacts** (`impacts.rs`, fixed tick): every
//!   [`SurfaceImpact`](crate::combat::SurfaceImpact) (bullets and pellets
//!   from weapons, melee and explosions from here) knocks out cells. The
//!   result depends only on the impacts, so it's deterministic.
//!   [`Explosion`] opens surfaces in range and hurts people; [`HardBreach`]
//!   cuts through anything, reinforced or not.
//! - **Actions** (fixed tick, from [`DestructionIntent`]): melee (V),
//!   reinforcing and barricading (hold F), the breach charge (hold 4 to
//!   place, press 4 to detonate).
//! - **Round loop** (`round_link.rs`): the [`DestructionGate`] follows the
//!   round phase, each new round restores the map and refills counts,
//!   loadouts decide who carries breach charges, destruction makes `Noise`.
//! - **Presentation** (every frame): mesh rebuilds, debris and dust, a small
//!   HUD, a camera nudge on melee.
//!
//! Debug and test flags are in `debug.rs` (and the README).

mod breach;
mod build;
mod debris;
mod debug;
mod grid;
mod hud;
mod impacts;
mod input;
mod interact;
mod melee;
mod round_link;
mod spawn;
mod surface;

use bevy::prelude::*;

pub use breach::BreachCharge;
// Public API for weapons (M3), the round loop and gadgets (M6), bots (M7).
#[allow(unused_imports)]
pub use impacts::surface_root;
#[allow(unused_imports)]
pub use surface::{NotReinforceable, SoftSurface, SurfaceMaterial, SurfaceMount, SurfaceRole};

use crate::{
    combat::Team,
    config::Tuning,
    round::RoundSystems,
    player::{CharacterState, PlayerInput},
};

pub struct DestructionPlugin;

impl Plugin for DestructionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DestructionGate>()
            .add_message::<Explosion>()
            .add_message::<HardBreach>()
            .add_message::<ReinforceRequest>()
            .add_message::<Reinforced>()
            .add_message::<ReinforceStarted>()
            .add_message::<ReinforceCancelled>()
            .add_message::<SurfaceChanged>()
            .add_message::<SurfaceBroken>()
            .add_message::<MeleeSwing>()
            .configure_sets(
                FixedUpdate,
                (
                    DestructionSystems::Reset,
                    DestructionSystems::Actions,
                    DestructionSystems::Explosions,
                    DestructionSystems::ApplyImpacts,
                    DestructionSystems::Rebuild,
                )
                    .chain()
                    // After the round flow: phase changes, resets and the
                    // defuser interaction are settled for this tick.
                    .after(RoundSystems::Flow),
            )
            .add_observer(equip_character)
            .add_plugins((
                spawn::SurfaceSpawnPlugin,
                impacts::ImpactPlugin,
                input::DestructionInputPlugin,
                melee::MeleePlugin,
                interact::InteractPlugin,
                breach::BreachChargePlugin,
                debris::DebrisPlugin,
                hud::DestructionHudPlugin,
                debug::DestructionDebugPlugin,
                round_link::RoundLinkPlugin,
            ));
    }
}

/// Order of the destruction systems in `FixedUpdate`. Weapons should write
/// their [`SurfaceImpact`](crate::combat::SurfaceImpact)s before `ApplyImpacts` so holes open on the same
/// tick (otherwise they open one tick later).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum DestructionSystems {
    /// A new round restores every surface.
    Reset,
    /// Melee, reinforcing, barricading, breach charges.
    Actions,
    /// [`Explosion`]s turn into surface impacts and damage.
    Explosions,
    /// Impacts, hard breaches and reinforcement change the cell grids.
    ApplyImpacts,
    /// Colliders of changed surfaces are rebuilt.
    Rebuild,
}

/// What may happen right now. Follows the round phase (`round_link.rs`);
/// everything is allowed in free play. Other code may also write it.
#[derive(Resource, Debug, Clone)]
pub struct DestructionGate {
    /// Defenders may reinforce (prep and action phases).
    pub reinforce: bool,
    /// Defenders may put up barricades.
    pub barricade: bool,
    /// Attackers may place and detonate breach charges.
    pub breach_charge: bool,
}

impl Default for DestructionGate {
    fn default() -> Self {
        Self {
            reinforce: true,
            barricade: true,
            breach_charge: true,
        }
    }
}

/// What a character wants to do this tick (held buttons). Written by the
/// local input (`input.rs`), later by bots and the network; read only by
/// the simulation. Aim comes from [`PlayerInput`] yaw/pitch.
#[derive(Component, Default, Debug, Clone, Copy)]
pub struct DestructionIntent {
    /// Melee button (V) held.
    pub melee: bool,
    /// Interact (F) held: reinforce, barricade.
    pub interact: bool,
    /// Secondary gadget (4) held: place / detonate a breach charge.
    pub gadget: bool,
}

/// Reinforcements a defender has left this round.
#[derive(Component, Debug, Clone, Copy)]
pub struct Reinforcements(pub u32);

/// Breach charges a character has left (0 = doesn't carry them). The
/// round loop / loadouts set this; attackers start with the tuning count.
#[derive(Component, Debug, Clone, Copy)]
pub struct BreachCharges(pub u32);

/// What a character is busy with (for HUDs and bots). Simulation state.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq)]
pub enum Activity {
    #[default]
    Idle,
    /// Holding F on a wall or hatch; `progress` 0..1.
    Reinforcing { surface: Entity, progress: f32 },
    /// Holding F at a doorway or window (`slot` is the wall piece);
    /// `progress` 0..1.
    Barricading { slot: Entity, progress: f32 },
    /// Holding 4 on a surface; `progress` 0..1.
    PlacingCharge { surface: Entity, progress: f32 },
}

/// A blast. Opens soft surfaces within `breach_radius` (walls get a
/// walk-through hole if the blast is low enough, floors a drop-through
/// hole), breaks hatches and barricades, does nothing to reinforced or
/// stone surfaces, and hurts people within `damage_radius` (full damage
/// close in, falling to zero at the edge; stone and reinforced walls block
/// it, soft walls halve it). Gadgets (frags, impact charges) write this too.
#[derive(Message, Clone, Debug)]
pub struct Explosion {
    pub centre: Vec3,
    pub breach_radius: f32,
    pub damage_radius: f32,
    pub max_damage: f32,
    pub source: Option<Entity>,
    /// Surface a shaped charge was stuck to, if any: only that surface is
    /// opened, and it doesn't count as cover. `None` (grenades) opens every
    /// soft surface in range.
    pub attached_to: Option<Entity>,
}

/// Cut a circular plug through a soft surface, reinforced or not: plaster,
/// studs, frame and plate all go. For the ring cutter (`radius` ~0.5) and
/// the backplate charge (a fist-sized hole). `surface` can be the surface
/// root or its collider; `centre` is in world space.
#[derive(Message, Clone, Debug)]
pub struct HardBreach {
    pub surface: Entity,
    pub centre: Vec3,
    pub radius: f32,
    pub source: Option<Entity>,
}

/// Reinforce a surface immediately (map setup, bots, tests). The plate goes
/// on the side facing `from`. Ignored if the surface can't be reinforced.
/// Players reinforce by holding F instead; that ends in the same place.
#[derive(Message, Clone, Debug)]
pub struct ReinforceRequest {
    pub surface: Entity,
    pub from: Vec3,
    pub by: Option<Entity>,
}

/// A surface was reinforced (it is now [`SurfaceKind::Hard`]).
///
/// [`SurfaceKind::Hard`]: crate::combat::SurfaceKind::Hard
#[derive(Message, Clone, Debug)]
pub struct Reinforced {
    pub surface: Entity,
    pub by: Option<Entity>,
}

/// A defender started holding F on a surface.
#[derive(Message, Clone, Debug)]
pub struct ReinforceStarted {
    pub player: Entity,
    pub surface: Entity,
}

/// A reinforcement in progress was interrupted (moved, looked away, let go,
/// or the gate closed).
#[derive(Message, Clone, Debug)]
pub struct ReinforceCancelled {
    pub player: Entity,
    pub surface: Entity,
}

/// Cells were knocked out of a surface (debris, sound, AI hooks).
#[derive(Message, Clone, Debug)]
pub struct SurfaceChanged {
    pub surface: Entity,
    /// World-space centres of the removed cells.
    pub removed: Vec<Vec3>,
    /// World-space size of a typical removed cell (debris size).
    pub cell_size: f32,
    pub material: SurfaceMaterial,
    /// Where the impact (or blast centre) was.
    pub origin: Vec3,
    /// Direction the impact travelled (debris flies this way); zero for
    /// blasts (debris flies out from `origin`).
    pub direction: Vec3,
    /// Big event (explosion, breaking open): more debris and dust.
    pub violent: bool,
}

/// A hatch broke open or a barricade was cleared.
#[derive(Message, Clone, Debug)]
pub struct SurfaceBroken {
    pub surface: Entity,
    pub role: SurfaceRole,
}

/// A melee swing happened (feedback, sound).
#[derive(Message, Clone, Debug)]
pub struct MeleeSwing {
    pub attacker: Entity,
    /// Something was hit (a person or a surface).
    pub hit: bool,
}

/// Eye position and look direction of a character, from simulation state
/// only (body position, stance height, lean, intended look angles).
pub fn eye_and_aim(
    transform: &Transform,
    state: &CharacterState,
    input: &PlayerInput,
    tuning: &Tuning,
) -> (Vec3, Dir3) {
    let right = Quat::from_rotation_y(input.yaw) * Vec3::X;
    let eye = transform.translation
        + Vec3::Y * (state.height - tuning.body.eye_from_top)
        + right * (state.lean * tuning.lean.offset);
    let look = Quat::from_euler(EulerRot::YXZ, input.yaw, input.pitch, 0.0);
    (eye, Dir3::new(look * Vec3::NEG_Z).unwrap_or(Dir3::NEG_Z))
}

/// Every character (anything with a [`Team`] and [`PlayerInput`]) gets
/// intent, activity and gadget counts. Loadouts (M6) may overwrite counts.
fn equip_character(
    add: On<Add<Team>>,
    characters: Query<&Team, With<PlayerInput>>,
    tuning: Res<Tuning>,
    mut commands: Commands,
) {
    let Ok(team) = characters.get(add.entity) else {
        return;
    };
    let t = &tuning.destruction;
    let charges = match team {
        Team::Attackers => t.breach_charge.count,
        Team::Defenders => 0,
    };
    commands.entity(add.entity).insert_if_new((
        DestructionIntent::default(),
        Activity::default(),
        melee::MeleeState::default(),
        Reinforcements(t.reinforce.per_defender),
        BreachCharges(charges),
    ));
}
