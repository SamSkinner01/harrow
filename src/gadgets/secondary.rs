//! Shared secondary gadgets (key 4): stock per round, which gadget answers
//! the key, and choosing one on the operator screen.
//!
//! - The character's `Loadout.secondary` decides. The soft-breach charge
//!   is destruction's (it uses `BreachCharges`); every other secondary is
//!   handled here with a [`SecondaryStock`].
//! - Thrown gadgets (frag, flashbang, smoke, impact charge): press 4 to
//!   ready one (a frag's fuse starts burning: cooking), release to throw.
//! - Placed gadgets (razor wire, ballistic cover, proximity alarm): hold 4
//!   for the placing time, standing still.
//! - On the operator screen, 4 cycles the secondary for the operator you
//!   picked (`OperatorChoice`). `--secondary <name>` picks it from the
//!   command line (free play and tests).

use bevy::prelude::*;

use super::{
    GadgetActivity, GadgetButtons, GadgetEvent, GadgetStage, SecondaryGadget, gadget_block,
    deployables::{self, PlaceTarget},
    remote_view::RemoteView,
    throwables,
};
use crate::{
    combat::{Dead, Team},
    config::Tuning,
    gadgets::tuning::GadgetTuning,
    player::{CharacterState, LocalPlayer, Player, PlayerInput},
    round::{
        Round, RoundPhase, RoundReset,
        operators::{Loadout, OperatorChoice, OperatorId},
    },
};

pub struct SecondaryPlugin;

impl Plugin for SecondaryPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SecondaryFlag(flag_value("--secondary").and_then(|name| {
            let gadget = SecondaryGadget::from_name(&name);
            if gadget.is_none() {
                warn!("--secondary expects breach|frag|flash|smoke|wire|cover|impact|alarm, got {name:?}");
            }
            gadget
        })))
        .add_observer(stock_from_loadout)
        .add_systems(PostStartup, apply_secondary_flag)
        .add_systems(
            FixedUpdate,
            (
                (refill_on_reset, choose_on_select_screen).in_set(GadgetStage::Input),
                use_secondaries.in_set(GadgetStage::Use),
            ),
        );
    }
}

/// Shared secondary gadgets a character has left this round (not the
/// soft-breach charge, which is destruction's `BreachCharges`).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct SecondaryStock {
    pub kind: SecondaryGadget,
    pub remaining: u8,
    pub max: u8,
}

/// `--secondary <name>` for the local player.
#[derive(Resource)]
struct SecondaryFlag(Option<SecondaryGadget>);

fn flag_value(flag: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

/// How many of `kind` a loadout carries (0 for the breach charge: that
/// count is destruction's).
pub fn count_for(kind: SecondaryGadget, tuning: &GadgetTuning) -> u8 {
    let c = &tuning.counts;
    match kind {
        SecondaryGadget::SoftBreachCharge => 0,
        SecondaryGadget::Frag => c.frag,
        SecondaryGadget::Flashbang => c.flashbang,
        SecondaryGadget::Smoke => c.smoke,
        SecondaryGadget::RazorWire => c.razor_wire,
        SecondaryGadget::BallisticCover => c.ballistic_cover,
        SecondaryGadget::ImpactCharge => c.impact_charge,
        SecondaryGadget::ProximityAlarm => c.proximity_alarm,
    }
}

fn stock(kind: SecondaryGadget, tuning: &Tuning) -> SecondaryStock {
    let n = count_for(kind, &tuning.round.gadgets);
    SecondaryStock {
        kind,
        remaining: n,
        max: n,
    }
}

/// Operators are equipped when prep starts (and in free play): stock up the
/// chosen secondary.
fn stock_from_loadout(
    insert: On<Insert<Loadout>>,
    loadouts: Query<&Loadout>,
    tuning: Res<Tuning>,
    mut commands: Commands,
) {
    if let Ok(loadout) = loadouts.get(insert.entity) {
        commands.entity(insert.entity).insert(stock(loadout.secondary, &tuning));
    }
}

/// Every new round refills the stock (the loadout may change again at prep).
fn refill_on_reset(
    mut resets: MessageReader<RoundReset>,
    tuning: Res<Tuning>,
    mut characters: Query<(&Loadout, &mut SecondaryStock)>,
) {
    if resets.read().last().is_none() {
        return;
    }
    for (loadout, mut s) in &mut characters {
        *s = stock(loadout.secondary, &tuning);
    }
}

/// `--secondary`: the local player's choice for every operator on both
/// sides (where the operator can carry it).
fn apply_secondary_flag(
    flag: Res<SecondaryFlag>,
    mut choices: Query<&mut OperatorChoice, With<LocalPlayer>>,
) {
    let Some(gadget) = flag.0 else { return };
    for mut choice in &mut choices {
        for op in [OperatorId::Auger, OperatorId::Lark, OperatorId::Mortise, OperatorId::Fathom] {
            if op.info().secondaries.contains(&gadget) {
                choice.set_secondary(op, gadget);
            }
        }
    }
}

/// On the operator screen, 4 cycles the secondary of the picked (or
/// default) operator.
fn choose_on_select_screen(
    round: Res<Round>,
    mut pickers: Query<(Entity, &GadgetButtons, &Team, &mut OperatorChoice), With<Player>>,
) {
    if round.phase != RoundPhase::OperatorSelect {
        return;
    }
    for (entity, buttons, team, mut choice) in &mut pickers {
        if !buttons.secondary.pressed {
            continue;
        }
        let op = choice
            .locked
            .filter(|op| op.info().side == *team)
            .or_else(|| choice.preferred(*team))
            .unwrap_or_else(|| OperatorId::default_for(*team));
        let options = op.info().secondaries;
        let current = choice.secondary_for(op);
        let i = options.iter().position(|g| *g == current).unwrap_or(0);
        let next = options[(i + 1) % options.len()];
        choice.set_secondary(op, next);
        info!("gadget: {entity} picks {} for {}", next.name(), op.info().callsign);
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn use_secondaries(
    round: Res<Round>,
    tuning: Res<Tuning>,
    time: Res<Time>,
    targets: PlaceTarget,
    mut users: Query<
        (
            Entity,
            &Transform,
            &CharacterState,
            &PlayerInput,
            &Team,
            &GadgetButtons,
            &mut GadgetActivity,
            &mut SecondaryStock,
            Has<Dead>,
            Has<RemoteView>,
        ),
        With<Player>,
    >,
    mut events: MessageWriter<GadgetEvent>,
    mut commands: Commands,
) {
    let g = &tuning.round.gadgets;
    let dt = time.delta_secs();
    if round.phase == RoundPhase::OperatorSelect {
        return;
    }
    for (entity, transform, state, input, team, buttons, mut activity, mut stock, dead, viewing) in &mut users {
        if stock.kind == SecondaryGadget::SoftBreachCharge {
            continue;
        }
        let block = gadget_block(dead, round.phase, *team, viewing);
        let button = buttons.secondary;
        let kind = stock.kind;
        match *activity {
            GadgetActivity::Cooking { kind: held_kind, fuse } => {
                if block.is_some() {
                    // Dropped: a cooking frag still goes off where you stand.
                    if fuse.is_some() {
                        throwables::spawn_grenade(&mut commands, entity, *team, held_kind, transform, state, input, &tuning, Some(0.0));
                    }
                    *activity = GadgetActivity::Idle;
                    continue;
                }
                let fuse = fuse.map(|f| f - dt);
                if fuse.is_some_and(|f| f <= 0.0) {
                    // Held too long: it goes off in your hand.
                    throwables::spawn_grenade(&mut commands, entity, *team, held_kind, transform, state, input, &tuning, Some(0.0));
                    *activity = GadgetActivity::Idle;
                    continue;
                }
                if button.held {
                    *activity = GadgetActivity::Cooking { kind: held_kind, fuse };
                } else {
                    throwables::spawn_grenade(&mut commands, entity, *team, held_kind, transform, state, input, &tuning, fuse);
                    events.write(GadgetEvent::Thrown { by: entity, kind: held_kind });
                    *activity = GadgetActivity::Idle;
                }
            }
            GadgetActivity::Placing { kind: placing, progress } => {
                let still = input.move_axis.length_squared() < 0.01;
                let target = deployables::place_target(placing, transform, state, input, &tuning, &targets);
                match target {
                    Some(at) if button.held && still && block.is_none() && stock.remaining > 0 => {
                        let progress = progress + dt / deployables::place_time(placing, g).max(0.01);
                        if progress < 1.0 {
                            *activity = GadgetActivity::Placing { kind: placing, progress };
                        } else {
                            *activity = GadgetActivity::Idle;
                            stock.remaining -= 1;
                            deployables::spawn_deployable(&mut commands, placing, entity, *team, at, &tuning);
                            events.write(GadgetEvent::Placed {
                                by: entity,
                                kind: placing,
                                position: at.translation,
                            });
                        }
                    }
                    _ => *activity = GadgetActivity::Idle,
                }
            }
            GadgetActivity::Idle if button.pressed => {
                if let Some(reason) = block {
                    events.write(GadgetEvent::Refused { by: entity, reason });
                    continue;
                }
                if stock.remaining == 0 {
                    events.write(GadgetEvent::Refused { by: entity, reason: "none left" });
                    continue;
                }
                if kind.is_thrown() {
                    stock.remaining -= 1;
                    let fuse = (kind == SecondaryGadget::Frag).then_some(g.frag.fuse);
                    *activity = GadgetActivity::Cooking { kind, fuse };
                } else if deployables::place_target(kind, transform, state, input, &tuning, &targets).is_some() {
                    *activity = GadgetActivity::Placing { kind, progress: 0.0 };
                } else {
                    events.write(GadgetEvent::Refused {
                        by: entity,
                        reason: deployables::place_hint(kind),
                    });
                }
            }
            _ => {}
        }
    }
}
