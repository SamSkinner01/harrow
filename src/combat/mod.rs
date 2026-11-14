//! Shared combat contracts: the types that weapons (M3), destruction (M4),
//! the round loop and gadgets (M6) and bots (M7) all talk through.
//!
//! - [`Health`] on anything that can be hurt; [`DamageMessage`] to hurt it;
//!   [`Died`] (and the [`Dead`] marker) when it reaches zero.
//! - [`Team`] on every character.
//! - [`SurfaceKind`] on world pieces: hard (indestructible, stops bullets)
//!   or soft (destructible, bullets penetrate). It sits on the piece's rigid
//!   body, so for a hit collider look it up through `ColliderOf::body`.
//! - [`SurfaceImpact`] when something strikes the world (bullet, melee,
//!   explosion). Destruction consumes these to open holes.
//! - [`Armour`] scales damage taken (operators' armour rating, M6).
//! - [`Noise`] for sounds other than footsteps that gameplay can hear
//!   (detection gadgets, bots).
//!
//! All of this runs on the fixed simulation tick. Write messages in
//! `FixedUpdate`; damage is applied in `FixedPostUpdate`.

// Shared contract written ahead of its users (M3, M4, M6). Remove this once
// every type is used.
#![allow(dead_code)]

use bevy::prelude::*;

use crate::player::HitZone;

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<DamageMessage>()
            .add_message::<Died>()
            .add_message::<SurfaceImpact>()
            .add_message::<Noise>()
            .add_systems(FixedPostUpdate, apply_damage.in_set(CombatSystems));
    }
}

/// Damage application (`FixedPostUpdate`). Only the authority runs it
/// (networking gates the set).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CombatSystems;

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Hash, serde::Serialize, serde::Deserialize)]
pub enum Team {
    Attackers,
    Defenders,
}

#[derive(Component, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

impl Health {
    pub fn new(max: f32) -> Self {
        Self { current: max, max }
    }
}

/// Body armour. Rule: every [`DamageMessage`] aimed at this entity is
/// multiplied by `damage_scale` (1.0 = no protection) when it is applied,
/// after the sender's zone multipliers, for every damage kind. The round
/// module sets it from the operator's armour rating (M6).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Armour {
    pub damage_scale: f32,
}

/// Added when health reaches zero.
#[derive(Component, Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Dead;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageKind {
    Bullet,
    Melee,
    Explosion,
    Gadget,
}

/// Hurt `target`. `amount` is final (zone multipliers already applied by the
/// sender); `zone` is for feedback and stats.
#[derive(Message, Clone, Debug)]
pub struct DamageMessage {
    pub target: Entity,
    pub amount: f32,
    pub kind: DamageKind,
    pub zone: Option<HitZone>,
    pub source: Option<Entity>,
}

#[derive(Message, Clone, Debug)]
pub struct Died {
    pub entity: Entity,
    pub killer: Option<Entity>,
}

/// What a world piece is made of.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SurfaceKind {
    /// Indestructible; stops bullets.
    Hard,
    /// Destructible; bullets penetrate with reduced damage.
    Soft,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ImpactKind {
    Bullet,
    /// One shotgun pellet; destruction groups nearby pellets into a bigger hole.
    Pellet,
    Melee,
    Explosion { radius: f32 },
}

/// Something struck the world at `point`.
#[derive(Message, Clone, Debug)]
pub struct SurfaceImpact {
    /// The collider entity that was hit.
    pub collider: Entity,
    pub point: Vec3,
    pub normal: Vec3,
    /// Direction of travel (unit), e.g. the bullet's direction.
    pub direction: Vec3,
    pub kind: ImpactKind,
    pub source: Option<Entity>,
}

/// What kind of sound a [`Noise`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoiseKind {
    Reload,
    /// Deploying, throwing or firing a gadget.
    Gadget,
    Door,
    Gunfire,
    Explosion,
    /// Breaching tools (e.g. a ring cutter at work).
    Breach,
    Other,
}

/// A sound that gameplay can hear: detection gadgets (Lark's darts listen
/// for these, loud ones saturate Fathom's stakes) and, later, bots.
/// Footsteps have their own message (`player::Footstep`).
///
/// `source` is the character responsible (its `Team` decides who can be
/// revealed by it), or `None` for world sounds.
///
/// Loudness uses the footstep scale: 1.0 = a sprinting footstep (walk 0.5,
/// crouch-walk 0.12). Suggested values: gadget deploy 0.5, reload 0.6,
/// door 0.6, gunshot 3.0, ring cutter 5.0 (repeat while cutting),
/// explosion 6.0.
#[derive(Message, Clone, Debug)]
pub struct Noise {
    pub source: Option<Entity>,
    pub position: Vec3,
    pub loudness: f32,
    pub kind: NoiseKind,
}

fn apply_damage(
    mut damage: MessageReader<DamageMessage>,
    mut targets: Query<(&mut Health, Option<&Armour>), Without<Dead>>,
    mut died: MessageWriter<Died>,
    mut commands: Commands,
) {
    for hit in damage.read() {
        let Ok((mut health, armour)) = targets.get_mut(hit.target) else {
            continue;
        };
        if health.current <= 0.0 {
            continue;
        }
        let amount = hit.amount * armour.map_or(1.0, |a| a.damage_scale);
        health.current = (health.current - amount).max(0.0);
        if health.current <= 0.0 {
            commands.entity(hit.target).insert(Dead);
            died.write(Died {
                entity: hit.target,
                killer: hit.source,
            });
        }
    }
}
