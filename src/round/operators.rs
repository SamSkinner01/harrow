//! Operators: the four starter operators (DESIGN.md "Starter operators"),
//! selection before each round, and applying their stats.
//!
//! Names, sides, roles and gadgets are fixed design data here; armour/speed
//! ratings and gadget counts are tuning numbers (`round.operators`).
//!
//! When prep starts, each character gets:
//! - [`Operator`]: who they are this round,
//! - [`Armour`] (combat): damage scale from the armour rating,
//! - [`OperatorSpeed`]: movement multiplier from the speed rating,
//! - [`UniqueGadget`] (gadgets): the unique gadget and its count,
//! - [`Loadout`]: weapon slots and the shared secondary gadget (chosen with
//!   4 on the selection screen, see [`OperatorChoice::secondary_for`]).

use bevy::prelude::*;

use super::{Round, RoundEvent, RoundIntent, RoundPhase, RoundSettings, RoundSystems};
use crate::{
    combat::{Armour, Team},
    config::{OperatorNumbers, RoundTuning, Tuning},
    gadgets::{GadgetKind, SecondaryGadget, UniqueGadget},
    player::{AutomatedRun, LocalPlayer, Player},
};

pub struct OperatorsPlugin;

impl Plugin for OperatorsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (
                (pick_operators, free_play_operator).in_set(RoundSystems::Objective),
                (open_selection, apply_operators)
                    .chain()
                    .in_set(RoundSystems::Reactions),
            ),
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OperatorId {
    Auger,
    Lark,
    Mortise,
    Fathom,
}

/// Fixed design data for one operator.
#[derive(Debug)]
pub struct OperatorInfo {
    pub id: OperatorId,
    pub callsign: &'static str,
    pub side: Team,
    pub role: &'static str,
    pub gadget: GadgetKind,
    /// Shared secondary gadgets this operator may choose from.
    pub secondaries: &'static [SecondaryGadget],
    pub default_secondary: SecondaryGadget,
}

const ATTACKER_SECONDARIES: &[SecondaryGadget] = &[
    SecondaryGadget::SoftBreachCharge,
    SecondaryGadget::Frag,
    SecondaryGadget::Flashbang,
    SecondaryGadget::Smoke,
];
const DEFENDER_SECONDARIES: &[SecondaryGadget] = &[
    SecondaryGadget::RazorWire,
    SecondaryGadget::BallisticCover,
    SecondaryGadget::ImpactCharge,
    SecondaryGadget::ProximityAlarm,
];

pub const ROSTER: [OperatorInfo; 4] = [
    OperatorInfo {
        id: OperatorId::Auger,
        callsign: "AUGER",
        side: Team::Attackers,
        role: "Hard breacher",
        gadget: GadgetKind::RingCutter,
        secondaries: ATTACKER_SECONDARIES,
        default_secondary: SecondaryGadget::SoftBreachCharge,
    },
    OperatorInfo {
        id: OperatorId::Lark,
        callsign: "LARK",
        side: Team::Attackers,
        role: "Scout",
        gadget: GadgetKind::SensorDart,
        secondaries: ATTACKER_SECONDARIES,
        default_secondary: SecondaryGadget::Smoke,
    },
    OperatorInfo {
        id: OperatorId::Mortise,
        callsign: "MORTISE",
        side: Team::Defenders,
        role: "Anchor / anti-breach",
        gadget: GadgetKind::BackplateCharge,
        secondaries: DEFENDER_SECONDARIES,
        default_secondary: SecondaryGadget::RazorWire,
    },
    OperatorInfo {
        id: OperatorId::Fathom,
        callsign: "FATHOM",
        side: Team::Defenders,
        role: "Roamer",
        gadget: GadgetKind::SeismicStake,
        secondaries: DEFENDER_SECONDARIES,
        default_secondary: SecondaryGadget::ProximityAlarm,
    },
];

impl OperatorId {
    pub fn info(self) -> &'static OperatorInfo {
        ROSTER
            .iter()
            .find(|op| op.id == self)
            .expect("every operator is in the roster")
    }

    pub fn from_name(name: &str) -> Option<Self> {
        ROSTER
            .iter()
            .find(|op| op.callsign.eq_ignore_ascii_case(name))
            .map(|op| op.id)
    }

    pub fn numbers(self, tuning: &RoundTuning) -> &OperatorNumbers {
        let roster = &tuning.operators;
        match self {
            OperatorId::Auger => &roster.auger,
            OperatorId::Lark => &roster.lark,
            OperatorId::Mortise => &roster.mortise,
            OperatorId::Fathom => &roster.fathom,
        }
    }

    /// Operators playable on `side`, in selection order (key 1, 2, ...).
    pub fn for_side(side: Team) -> impl Iterator<Item = OperatorId> {
        ROSTER.iter().filter(move |op| op.side == side).map(|op| op.id)
    }

    pub fn default_for(side: Team) -> OperatorId {
        Self::for_side(side)
            .next()
            .expect("each side has an operator")
    }
}

/// The operator a character plays this round.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Operator(pub OperatorId);

/// Movement speed multiplier from the operator's speed rating (1.0 =
/// normal). Movement should multiply its walk/crouch/prone/sprint speeds by
/// this; it doesn't yet (see the M6 report).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct OperatorSpeed(pub f32);

/// Loadout slots. Weapon ids refer to M3's weapons (one rifle, one sidearm
/// for now). The secondary gadget answers key 4 (destruction's breach
/// charge, or `gadgets` for the rest).
#[derive(Component, Clone, Debug)]
pub struct Loadout {
    pub primary: &'static str,
    pub sidearm: &'static str,
    pub secondary: SecondaryGadget,
}

/// Operator selection state for one character.
#[derive(Component, Default, Debug, Clone)]
pub struct OperatorChoice {
    /// Last pick on each side, reused as the default next time.
    pub attacker: Option<OperatorId>,
    pub defender: Option<OperatorId>,
    /// Locked in for the coming round.
    pub locked: Option<OperatorId>,
    /// Secondary gadget picked per operator (4 on the selection screen);
    /// operators not listed carry their default.
    pub secondaries: Vec<(OperatorId, SecondaryGadget)>,
}

impl OperatorChoice {
    /// The secondary gadget `op` will carry.
    pub fn secondary_for(&self, op: OperatorId) -> SecondaryGadget {
        self.secondaries
            .iter()
            .find(|(o, g)| *o == op && op.info().secondaries.contains(g))
            .map_or(op.info().default_secondary, |(_, g)| *g)
    }

    pub fn set_secondary(&mut self, op: OperatorId, gadget: SecondaryGadget) {
        self.secondaries.retain(|(o, _)| *o != op);
        self.secondaries.push((op, gadget));
    }

    pub fn preferred(&self, side: Team) -> Option<OperatorId> {
        match side {
            Team::Attackers => self.attacker,
            Team::Defenders => self.defender,
        }
    }

    fn lock(&mut self, op: OperatorId) {
        match op.info().side {
            Team::Attackers => self.attacker = Some(op),
            Team::Defenders => self.defender = Some(op),
        }
        self.locked = Some(op);
    }
}

/// Number keys on the selection screen.
fn pick_operators(
    round: Res<Round>,
    mut pickers: Query<(Entity, &mut RoundIntent, &mut OperatorChoice, &Team), With<Player>>,
    mut events: MessageWriter<RoundEvent>,
) {
    for (entity, mut intent, mut choice, team) in &mut pickers {
        let Some(index) = intent.pick_operator.take() else {
            continue;
        };
        if round.phase != RoundPhase::OperatorSelect {
            continue;
        }
        if let Some(op) = OperatorId::for_side(*team).nth(index as usize) {
            choice.lock(op);
            events.write(RoundEvent::OperatorLocked { entity, operator: op });
        }
    }
}

/// When selection opens: clear last round's lock; pre-lock where there is
/// nothing to choose interactively (the `--operator` flag for its side, or
/// an automated screenshot run). Test scripts pick themselves or let the
/// timer run out.
fn open_selection(
    mut phase_events: MessageReader<RoundEvent>,
    settings: Res<RoundSettings>,
    automated: Option<Res<AutomatedRun>>,
    mut pickers: Query<(Entity, &mut OperatorChoice, &Team, Has<LocalPlayer>), With<Player>>,
    mut commands: Commands,
) {
    let opened = phase_events.read().any(|e| {
        matches!(e, RoundEvent::PhaseStarted { phase: RoundPhase::OperatorSelect, .. })
    });
    if !opened {
        return;
    }
    for (entity, mut choice, team, local) in &mut pickers {
        choice.locked = None;
        if local
            && let Some(op) = settings.operator
            && op.info().side == *team
        {
            choice.lock(op);
        } else if automated.is_some() && !settings.scripted {
            let op = choice
                .preferred(*team)
                .unwrap_or_else(|| OperatorId::default_for(*team));
            choice.lock(op);
        } else {
            continue;
        }
        if let Some(op) = choice.locked {
            // Deferred: this system also reads round events.
            commands.write_message(RoundEvent::OperatorLocked { entity, operator: op });
        }
    }
}

/// When prep starts: everyone becomes their locked (or default) operator.
fn apply_operators(
    mut phase_events: MessageReader<RoundEvent>,
    tuning: Res<Tuning>,
    mut players: Query<(Entity, &mut OperatorChoice, &Team), With<Player>>,
    mut commands: Commands,
) {
    let prep = phase_events
        .read()
        .any(|e| matches!(e, RoundEvent::PhaseStarted { phase: RoundPhase::Prep, .. }));
    if !prep {
        return;
    }
    let round = &tuning.round;
    for (entity, mut choice, team) in &mut players {
        let op = match choice.locked {
            Some(op) if op.info().side == *team => op,
            _ => choice
                .preferred(*team)
                .unwrap_or_else(|| OperatorId::default_for(*team)),
        };
        choice.lock(op);
        equip_operator(&mut commands, entity, op, round, choice.secondary_for(op));
    }
}

/// Give `entity` everything that comes with playing `op`, carrying the
/// `secondary` gadget (falls back to the operator's default if `op` can't
/// carry it).
pub(crate) fn equip_operator(
    commands: &mut Commands,
    entity: Entity,
    op: OperatorId,
    round: &RoundTuning,
    secondary: SecondaryGadget,
) {
    let info = op.info();
    let numbers = op.numbers(round);
    commands.entity(entity).insert((
        Operator(op),
        Armour {
            damage_scale: round.armour_damage.get(numbers.armour),
        },
        OperatorSpeed(round.speed_multiplier.get(numbers.speed)),
        UniqueGadget {
            kind: info.gadget,
            remaining: numbers.gadgets,
            max: numbers.gadgets,
        },
        Loadout {
            primary: "rifle",
            sidearm: "sidearm",
            secondary: if info.secondaries.contains(&secondary) {
                secondary
            } else {
                info.default_secondary
            },
        },
    ));
    info!(
        "round: {entity} plays {} (armour {}, speed {}, {} x{})",
        info.callsign,
        numbers.armour,
        numbers.speed,
        info.gadget.name(),
        numbers.gadgets
    );
}

/// Free play (no round loop): `--operator` still makes the local player
/// that operator, on that operator's side, so gadgets can be tried out.
fn free_play_operator(
    settings: Res<RoundSettings>,
    tuning: Res<Tuning>,
    mut local: Query<(Entity, &mut Team, &OperatorChoice), With<LocalPlayer>>,
    mut done: Local<bool>,
    mut commands: Commands,
) {
    if *done || settings.enabled {
        return;
    }
    let (Some(op), Ok((entity, mut team, choice))) = (settings.operator, local.single_mut()) else {
        return;
    };
    *done = true;
    *team = op.info().side;
    equip_operator(&mut commands, entity, op, &tuning.round, choice.secondary_for(op));
}
