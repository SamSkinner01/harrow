//! Hooks into the round loop (M6) and the shared combat contracts:
//!
//! - [`DestructionGate`] follows the round phase: reinforcing and
//!   barricading in prep, action and planted (and free play); breach
//!   charges whenever attackers may use gadgets.
//! - Every [`RoundReset`] restores the map: soft surfaces intact and
//!   unreinforced, the map's barricades back up, defenders' barricades and
//!   placed charges gone, reinforcement and charge counts refilled.
//! - The operator's [`Loadout`] decides who carries breach charges (the
//!   soft-breach charge secondary); without one (free play) attackers do.
//! - Destruction makes gameplay [`Noise`]: explosions, things breaking,
//!   melee hits, reinforcement.
//!
//! The defuser interaction owning F (`ObjectiveInteraction`) is handled in
//! `interact.rs`.

use bevy::prelude::*;

use super::{
    Activity, BreachCharge, BreachCharges, DestructionGate, DestructionSystems, Explosion,
    MeleeSwing, Reinforced, Reinforcements, SurfaceBroken,
    interact::PlacedBarricade,
    surface::{SoftSurface, layout_for},
};
use crate::{
    combat::{Noise, NoiseKind, SurfaceKind, Team},
    config::{DestructionTuning, Tuning},
    gadgets::{SecondaryGadget, gadgets_allowed},
    round::{RoundPhase, RoundReset, operators::Loadout},
};

pub struct RoundLinkPlugin;

impl Plugin for RoundLinkPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(apply_loadout)
            .add_systems(Update, follow_round_phase.run_if(state_changed::<RoundPhase>))
            .add_systems(
                FixedUpdate,
                (
                    reset_for_new_round.in_set(DestructionSystems::Reset),
                    make_noise.after(DestructionSystems::Rebuild),
                ),
            );
    }
}

/// Breach charges a character carries with this loadout.
fn charges_for(team: Team, loadout: Option<&Loadout>, tuning: &DestructionTuning) -> u32 {
    let carries = match loadout {
        Some(loadout) => loadout.secondary == SecondaryGadget::SoftBreachCharge,
        None => team == Team::Attackers,
    };
    if carries { tuning.breach_charge.count } else { 0 }
}

fn follow_round_phase(phase: Res<State<RoundPhase>>, mut gate: ResMut<DestructionGate>) {
    let phase = *phase.get();
    let setup = phase.is_live() || phase == RoundPhase::Inactive;
    gate.reinforce = setup;
    gate.barricade = setup;
    gate.breach_charge = gadgets_allowed(phase, Team::Attackers);
}

/// Operators are equipped when prep starts: set breach charges from the
/// chosen secondary gadget.
fn apply_loadout(
    insert: On<Insert<Loadout>>,
    characters: Query<(&Team, &Loadout)>,
    tuning: Res<Tuning>,
    mut commands: Commands,
) {
    if let Ok((team, loadout)) = characters.get(insert.entity) {
        let charges = charges_for(*team, Some(loadout), &tuning.destruction);
        commands.entity(insert.entity).insert(BreachCharges(charges));
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn reset_for_new_round(
    mut resets: MessageReader<RoundReset>,
    mut surfaces: Query<(Entity, &Name, &mut SoftSurface)>,
    placed: Query<(), With<PlacedBarricade>>,
    charges: Query<Entity, With<BreachCharge>>,
    mut characters: Query<(
        &Team,
        Option<&Loadout>,
        &mut Reinforcements,
        &mut BreachCharges,
        &mut Activity,
    )>,
    tuning: Res<Tuning>,
    mut commands: Commands,
) {
    let Some(reset) = resets.read().last() else {
        return;
    };
    let t = &tuning.destruction;
    let mut restored = 0;
    for (entity, name, mut surface) in &mut surfaces {
        if placed.contains(entity) {
            commands.entity(entity).despawn();
            continue;
        }
        let Some((mut spec, grid)) = layout_for(name.as_str(), t) else {
            continue;
        };
        // Keep the map's "not reinforceable" decision.
        spec.reinforceable = surface.spec.reinforceable;
        let revision = surface.revision + 1;
        let mut fresh = SoftSurface::new(spec, grid, surface.seed);
        fresh.revision = revision;
        *surface = fresh;
        commands.entity(entity).insert(SurfaceKind::Soft);
        restored += 1;
    }
    for charge in &charges {
        commands.entity(charge).despawn();
    }
    for (team, loadout, mut reinforcements, mut breach, mut activity) in &mut characters {
        reinforcements.0 = t.reinforce.per_defender;
        breach.0 = charges_for(*team, loadout, t);
        *activity = Activity::Idle;
    }
    info!("destruction: restored {restored} surfaces for round {}", reset.round);
}

#[allow(clippy::too_many_arguments)]
fn make_noise(
    mut explosions: MessageReader<Explosion>,
    mut broken: MessageReader<SurfaceBroken>,
    mut swings: MessageReader<MeleeSwing>,
    mut reinforced: MessageReader<Reinforced>,
    places: Query<&GlobalTransform>,
    tuning: Res<Tuning>,
    mut noise: MessageWriter<Noise>,
) {
    let loud = &tuning.destruction.noise;
    let at = |entity: Entity| places.get(entity).map(|gt| gt.translation()).ok();
    for blast in explosions.read() {
        noise.write(Noise {
            source: blast.source,
            position: blast.centre,
            loudness: loud.explosion,
            kind: NoiseKind::Explosion,
        });
    }
    for b in broken.read() {
        if let Some(position) = at(b.surface) {
            noise.write(Noise {
                source: None,
                position,
                loudness: loud.breaking,
                kind: NoiseKind::Other,
            });
        }
    }
    for swing in swings.read().filter(|s| s.hit) {
        if let Some(position) = at(swing.attacker) {
            noise.write(Noise {
                source: Some(swing.attacker),
                position,
                loudness: loud.melee,
                kind: NoiseKind::Other,
            });
        }
    }
    for r in reinforced.read() {
        if let Some(position) = at(r.surface) {
            noise.write(Noise {
                source: r.by,
                position,
                loudness: loud.reinforce,
                kind: NoiseKind::Gadget,
            });
        }
    }
}
