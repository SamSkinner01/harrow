//! Weapons (M3): firing, recoil, spread, aim-down-sights, reloads, hit
//! detection and penetration, plus the first-person weapon view, HUD,
//! placeholder sounds and training dummies.
//!
//! Data flow, one way (same rules as the player module):
//!
//! 1. **Input** (`input.rs`, PreUpdate): mouse/keys -> [`WeaponInput`] on the
//!    local player. Bots and network clients will write the same component.
//! 2. **Simulation** (`firing.rs`, `ballistics.rs`, FixedUpdate): reads
//!    [`WeaponInput`] and the look angles in `PlayerInput`, updates
//!    [`WeaponState`], fires hitscan bullets from the true eye position, and
//!    writes `DamageMessage` / `SurfaceImpact` (shared combat contracts) plus
//!    [`ShotFired`], [`WeaponEvent`] and [`BulletHit`] for presentation.
//! 3. **Presentation** (`view.rs`, `viewmodel.rs`, `effects.rs`, `hud.rs`,
//!    `audio.rs`, every frame): reads state and messages, never changes
//!    gameplay state. [`AimView`] tells the player camera about ADS zoom and
//!    recoil.
//!
//! `dummies.rs` adds training targets (`--dummies`); `testing.rs` has the
//! automated checks (`--weapon-test`) and screenshot helpers.

// Synthesised sounds; not in the dedicated-server build (no audio).
// First-person arms; client only (rendering assets).
#[cfg(feature = "client")]
mod arms;
#[cfg(feature = "client")]
mod audio;
mod ballistics;
mod dummies;
mod effects;
mod firing;
mod hud;
mod input;
mod materials;
mod testing;
mod third_person;
mod view;
mod viewmodel;

use bevy::prelude::*;

/// Shared weapon materials (also used for third-person weapon models).
pub use materials::WeaponMaterials;

use crate::{
    config::{Tuning, WeaponDef},
    player::{HitZone, Player},
};

pub struct WeaponsPlugin;

impl Plugin for WeaponsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ShotFired>()
            .add_message::<WeaponEvent>()
            .add_message::<BulletHit>()
            .add_observer(arm_character)
            .add_plugins((
                input::WeaponInputPlugin,
                firing::FiringPlugin,
                dummies::DummiesPlugin,
                materials::WeaponMaterialsPlugin,
                view::AimViewPlugin,
                viewmodel::ViewmodelPlugin,
                effects::ImpactEffectsPlugin,
                hud::WeaponHudPlugin,
                testing::WeaponTestPlugin,
                third_person::ThirdPersonCheckPlugin,
            ));
        #[cfg(feature = "client")]
        app.add_plugins((
            crate::audio::GameAudioPlugin,
            audio::WeaponAudioPlugin,
            arms::ArmsPlugin,
        ));
    }
}

/// Ordering for other modules: the fixed-tick weapon simulation.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum WeaponSystems {
    Simulate,
}

/// A weapon slot in the loadout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Reflect, serde::Serialize, serde::Deserialize)]
pub enum Slot {
    #[default]
    Primary,
    Sidearm,
}

impl Slot {
    pub const ALL: [Slot; 2] = [Slot::Primary, Slot::Sidearm];

    pub fn index(self) -> usize {
        match self {
            Slot::Primary => 0,
            Slot::Sidearm => 1,
        }
    }

    pub fn other(self) -> Slot {
        match self {
            Slot::Primary => Slot::Sidearm,
            Slot::Sidearm => Slot::Primary,
        }
    }

    /// This slot's weapon in the tuning file.
    pub fn def(self, tuning: &Tuning) -> &WeaponDef {
        match self {
            Slot::Primary => &tuning.weapons.primary,
            Slot::Sidearm => &tuning.weapons.sidearm,
        }
    }
}

/// What the controlling player wants from their weapons. Pure intent: the
/// simulation decides what is possible.
///
/// Button presses are counters rather than flags, so a click shorter than a
/// simulation tick is never lost and the simulation never has to write back.
/// Networked as part of `net::input::NetInput`.
#[derive(Component, Default, Debug, Clone, PartialEq, Reflect, serde::Serialize, serde::Deserialize)]
pub struct WeaponInput {
    /// Trigger held.
    pub trigger: bool,
    /// Total trigger presses so far (wraps).
    pub trigger_presses: u32,
    /// Wants to aim down sights (hold/toggle already resolved).
    pub aim: bool,
    /// Total reload presses so far (wraps).
    pub reload_requests: u32,
    /// Weapon the player wants in hand.
    pub slot: Slot,
}

/// Multiplies a character's walking speed (read by the movement code).
/// Weapons set it from aiming; 1.0 = normal.
/// Replicated: a client's movement prediction reads it.
#[derive(Component, Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MoveSpeedScale(pub f32);

impl Default for MoveSpeedScale {
    fn default() -> Self {
        Self(1.0)
    }
}

/// Rounds for one weapon.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Ammo {
    pub magazine: u32,
    pub reserve: u32,
}

/// What the hands are busy with.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum WeaponAction {
    Ready,
    Reloading { elapsed: f32, duration: f32, empty: bool },
    /// Lowering the active weapon before switching to `to`.
    Holstering { elapsed: f32, duration: f32, to: Slot },
    /// Raising the (new) active weapon.
    Drawing { elapsed: f32, duration: f32 },
}

impl WeaponAction {
    /// 0..1 through the current action (1 when ready).
    pub fn progress(&self) -> f32 {
        match *self {
            WeaponAction::Ready => 1.0,
            WeaponAction::Reloading { elapsed, duration, .. }
            | WeaponAction::Holstering { elapsed, duration, .. }
            | WeaponAction::Drawing { elapsed, duration } => {
                (elapsed / duration.max(1e-3)).clamp(0.0, 1.0)
            }
        }
    }
}

/// Weapon simulation state of one character. Owned by the fixed tick (on
/// the server when networked; clients get it replicated).
#[derive(Component, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WeaponState {
    pub active: Slot,
    pub ammo: [Ammo; 2],
    pub action: WeaponAction,
    /// 0 = hip .. 1 = fully aimed, and its value at the previous tick.
    pub aim: f32,
    pub prev_aim: f32,
    /// View kick from recoil in degrees (right, up), and at the previous tick.
    pub recoil: Vec2,
    pub prev_recoil: Vec2,
    /// Extra spread from sustained fire, degrees.
    pub bloom: f32,
    /// Seconds until the next shot may fire (can go negative while the
    /// trigger is held, to keep the exact fire rate between ticks).
    pub cooldown: f32,
    /// Seconds since the last shot.
    pub since_shot: f32,
    /// Shots in the current burst (indexes the recoil pattern).
    pub burst: u32,
    /// Every shot ever fired; seeds the deterministic spread.
    pub shots_fired: u32,
    /// Seconds left before firing/aiming is allowed after a sprint.
    pub sprint_lock: f32,
    /// A semi-auto click waiting for the fire-rate limit.
    pub queued_shot: bool,
    /// Hands busy with something else (e.g. the defuser): no firing or
    /// aiming, weapon lowered.
    pub hands_busy: bool,
    /// The empty click already played for the current trigger hold.
    pub dry_fired: bool,
    /// Input counters already handled.
    pub seen_trigger: u32,
    pub seen_reload: u32,
}

impl WeaponState {
    pub fn new(tuning: &Tuning) -> Self {
        let full = |slot: Slot| {
            let def = slot.def(tuning);
            Ammo {
                // Spawn with a full magazine and one in the chamber.
                magazine: def.magazine + u32::from(def.chamber),
                reserve: def.reserve,
            }
        };
        Self {
            active: Slot::Primary,
            ammo: [full(Slot::Primary), full(Slot::Sidearm)],
            action: WeaponAction::Ready,
            aim: 0.0,
            prev_aim: 0.0,
            recoil: Vec2::ZERO,
            prev_recoil: Vec2::ZERO,
            bloom: 0.0,
            cooldown: 0.0,
            since_shot: 10.0,
            burst: 0,
            shots_fired: 0,
            sprint_lock: 0.0,
            queued_shot: false,
            hands_busy: false,
            dry_fired: false,
            seen_trigger: 0,
            seen_reload: 0,
        }
    }

    pub fn ammo(&self) -> Ammo {
        self.ammo[self.active.index()]
    }
}

/// Presentation output for the player camera: ADS zoom and recoil kick,
/// interpolated between ticks. Lives on the local player; the camera reads it.
#[derive(Component, Debug, Clone, Copy)]
pub struct AimView {
    /// Multiplies the camera's field of view (below 1 = zoomed in).
    pub fov_scale: f32,
    /// Recoil added to the look angles, radians (same sign as `PlayerInput`).
    pub yaw: f32,
    pub pitch: f32,
    /// 0 = hip .. 1 = fully aimed (eased). For HUD and crosshair.
    pub aim: f32,
}

impl Default for AimView {
    fn default() -> Self {
        Self {
            fov_scale: 1.0,
            yaw: 0.0,
            pitch: 0.0,
            aim: 0.0,
        }
    }
}

/// A shot was fired (one per bullet). Forwarded to clients.
#[derive(Message, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ShotFired {
    pub shooter: Entity,
    pub slot: Slot,
    pub origin: Vec3,
    pub direction: Vec3,
    /// Where the bullet stopped (or its range ran out).
    pub end: Vec3,
    /// Spread cone used for this shot, degrees.
    pub spread: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WeaponEventKind {
    DryFire,
    ReloadStart { empty: bool },
    ReloadDone,
    Holster,
    Draw,
}

/// Something happened to a character's weapon (for sounds and animation).
/// Forwarded to clients.
#[derive(Message, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WeaponEvent {
    pub shooter: Entity,
    pub slot: Slot,
    pub kind: WeaponEventKind,
}

/// A bullet hit something with health: a character's hit zone or a hit
/// volume such as a gadget (hit markers, dummies, tests). Forwarded to
/// clients.
#[derive(Message, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BulletHit {
    pub shooter: Entity,
    pub target: Entity,
    /// The hit zone, or None for other hit volumes (gadgets).
    pub zone: Option<HitZone>,
    pub point: Vec3,
    /// Damage sent with the `DamageMessage`.
    pub damage: f32,
    /// Distance travelled, metres.
    pub distance: f32,
    /// Soft surfaces the bullet passed through first.
    pub layers: u32,
    /// The hit takes the target to zero health.
    pub lethal: bool,
}

/// Every character gets a loadout and weapon intent when it appears.
/// (`if_new`: on a client, weapon state may already have been replicated.)
fn arm_character(add: On<Add<Player>>, mut commands: Commands, tuning: Res<Tuning>) {
    commands.entity(add.entity).insert_if_new((
        WeaponInput::default(),
        WeaponState::new(&tuning),
        MoveSpeedScale::default(),
        AimView::default(),
    ));
}

/// Where a weapon's model sits on a third-person character: the model's
/// transform relative to the `weapon_r` bone (tuning: `model.third_person`
/// and `model.scale`). Spawn the weapon's glTF scene as a child of the bone
/// with this transform, e.g.
/// `(WorldAssetRoot(..def.model.path..), third_person_transform(def), ChildOf(weapon_r))`.
pub fn third_person_transform(def: &WeaponDef) -> Transform {
    let m = &def.model;
    let (yaw, pitch, roll) = m.third_person.rotation;
    Transform {
        translation: Vec3::from(m.third_person.offset),
        rotation: Quat::from_euler(
            EulerRot::YXZ,
            yaw.to_radians(),
            pitch.to_radians(),
            roll.to_radians(),
        ),
        scale: Vec3::splat(m.scale.max(1e-4)),
    }
}
