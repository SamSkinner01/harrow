//! Round flow on the fixed tick: timers, phase changes, win conditions,
//! score, side swaps, and the reset at the start of every round.
//!
//! Rules (numbers in `round.timers`):
//! - Each round: operator select -> prep -> action -> (planted) -> round end.
//! - Action timer runs out without a plant: defenders win.
//! - Defuser runs out: attackers win. Defuser disabled: defenders win.
//! - A team whose members are all dead loses, except that a wiped attacking
//!   team doesn't lose once the defuser is planted (defenders must still
//!   disable it). A team with no members at all (solo play) is never
//!   "wiped".
//! - First squad to `rounds_to_win` wins the match; sides swap every
//!   `swap_sides_every` rounds; a new match starts after the match banner.

use avian3d::prelude::LinearVelocity;
use bevy::prelude::*;

use super::{
    MovementLock, ObjectiveInteraction, Round, RoundEvent, RoundPhase, RoundReset, RoundSettings,
    RoundSystems, Squad, WinReason, other_team, sites::KeepLayout,
};
use crate::{
    combat::{Dead, Health, Team},
    config::Tuning,
    player::{LocalPlayer, Player, PlayerInput},
};

pub struct FlowPlugin;

impl Plugin for FlowPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (
                (tick_round, assign_sides, super::sites::choose_site_pair)
                    .chain()
                    .in_set(RoundSystems::Flow),
                reset_characters.in_set(RoundSystems::Reactions),
                update_movement_locks.in_set(RoundSystems::Locks),
            ),
        );
    }
}

/// Members of a team and how many are alive. Anything with a `Team` and
/// `Health` counts (players, bots, M3's training dummies).
#[derive(Default, Clone, Copy, Debug)]
struct Headcount {
    members: u32,
    alive: u32,
}

impl Headcount {
    fn wiped(self) -> bool {
        self.members > 0 && self.alive == 0
    }
}

#[allow(clippy::too_many_arguments)]
fn tick_round(
    mut round: ResMut<Round>,
    settings: Res<RoundSettings>,
    tuning: Res<Tuning>,
    time: Res<Time>,
    characters: Query<(&Team, Has<Dead>), With<Health>>,
    choices: Query<&super::operators::OperatorChoice, With<Player>>,
    mut events: MessageWriter<RoundEvent>,
) {
    if !settings.enabled {
        return;
    }
    let timers = settings.timers(&tuning.round).clone();
    let dt = time.delta_secs();
    round.elapsed += dt;
    round.time_left -= dt;
    let time_up = round.time_left <= 0.0;

    let mut attackers = Headcount::default();
    let mut defenders = Headcount::default();
    for (team, dead) in &characters {
        let count = match team {
            Team::Attackers => &mut attackers,
            Team::Defenders => &mut defenders,
        };
        count.members += 1;
        count.alive += u32::from(!dead);
    }

    match round.phase {
        RoundPhase::Inactive => start_match(&mut round, settings.start_side, timers.select, &mut events),
        RoundPhase::OperatorSelect => {
            let everyone_locked = !choices.is_empty() && choices.iter().all(|c| c.locked.is_some());
            if time_up || everyone_locked {
                round.enter(RoundPhase::Prep, timers.prep, &mut events);
            }
        }
        RoundPhase::Prep | RoundPhase::Action => {
            if defenders.wiped() {
                round.finish(Team::Attackers, WinReason::Wipe, &timers, &mut events);
            } else if attackers.wiped() {
                round.finish(Team::Defenders, WinReason::Wipe, &timers, &mut events);
            } else if time_up && round.phase == RoundPhase::Prep {
                round.enter(RoundPhase::Action, timers.action, &mut events);
            } else if time_up {
                round.finish(Team::Defenders, WinReason::TimeUp, &timers, &mut events);
            }
        }
        RoundPhase::Planted => {
            if defenders.wiped() {
                round.finish(Team::Attackers, WinReason::Wipe, &timers, &mut events);
            } else if time_up {
                round.finish(Team::Attackers, WinReason::DefuserCompleted, &timers, &mut events);
            }
        }
        RoundPhase::RoundEnd => {
            if time_up {
                let to_win = settings.rounds_to_win(&tuning.round);
                let winner = if round.score_blue >= to_win {
                    Some(Squad::Blue)
                } else if round.score_red >= to_win {
                    Some(Squad::Red)
                } else {
                    None
                };
                if let Some(squad) = winner {
                    round.match_winner = Some(squad);
                    events.write(RoundEvent::MatchWon {
                        squad,
                        score_blue: round.score_blue,
                        score_red: round.score_red,
                    });
                    round.enter(RoundPhase::MatchEnd, timers.match_end, &mut events);
                } else {
                    let every = tuning.round.swap_sides_every.max(1);
                    if round.number.is_multiple_of(every) {
                        round.blue_side = other_team(round.blue_side);
                    }
                    round.number += 1;
                    round.result = None;
                    round.enter(RoundPhase::OperatorSelect, timers.select, &mut events);
                }
            }
        }
        RoundPhase::MatchEnd => {
            if time_up {
                start_match(&mut round, settings.start_side, timers.select, &mut events);
            }
        }
    }
}

fn start_match(round: &mut Round, side: Team, select: f32, events: &mut MessageWriter<RoundEvent>) {
    *round = Round {
        number: 1,
        blue_side: side,
        ..default()
    };
    round.enter(RoundPhase::OperatorSelect, select, events);
}

/// At the start of each round, every squad member takes its squad's side.
fn assign_sides(
    round: Res<Round>,
    mut events: MessageReader<RoundEvent>,
    mut members: Query<(&Squad, &mut Team)>,
) {
    let new_round = events.read().any(|e| {
        matches!(e, RoundEvent::PhaseStarted { phase: RoundPhase::OperatorSelect, .. })
    });
    if !new_round {
        return;
    }
    for (squad, mut team) in &mut members {
        team.set_if_neq(round.side_of(*squad));
    }
}

/// At the start of each round: heal and revive everyone, move players to
/// their team's spawn, clear objective state, and tell other modules.
#[allow(clippy::too_many_arguments)]
fn reset_characters(
    round: Res<Round>,
    settings: Res<RoundSettings>,
    layout: Res<KeepLayout>,
    mut events: MessageReader<RoundEvent>,
    mut reset: MessageWriter<RoundReset>,
    mut characters: Query<(Entity, &mut Health, Has<Dead>)>,
    mut players: Query<
        (
            Entity,
            &Team,
            &mut Transform,
            &mut PlayerInput,
            &mut LinearVelocity,
            Has<LocalPlayer>,
        ),
        With<Player>,
    >,
    mut commands: Commands,
) {
    let new_round = events.read().any(|e| {
        matches!(e, RoundEvent::PhaseStarted { phase: RoundPhase::OperatorSelect, .. })
    });
    if !new_round {
        return;
    }
    for (entity, mut health, dead) in &mut characters {
        health.current = health.max;
        if dead {
            commands.entity(entity).remove::<Dead>();
        }
    }
    let mut next_spawn = [0usize; 2];
    // Local player first, so it always gets the first spawn of its side.
    let mut order: Vec<_> = players.iter().map(|p| (p.0, p.5)).collect();
    order.sort_by_key(|&(e, local)| (!local, e));
    for (entity, _) in order {
        let Ok((_, team, mut transform, mut input, mut velocity, local)) = players.get_mut(entity) else {
            continue;
        };
        commands
            .entity(entity)
            .remove::<(ObjectiveInteraction, super::defuser::DefuserCarrier)>();
        if local && round.number == 1 && settings.keep_debug_spawn {
            continue;
        }
        let side = usize::from(*team == Team::Defenders);
        let spawns = layout.spawns(*team);
        if spawns.is_empty() {
            continue;
        }
        let spawn = spawns[next_spawn[side] % spawns.len()];
        next_spawn[side] += 1;
        transform.translation = spawn.position;
        transform.rotation = Quat::from_rotation_y(spawn.yaw);
        input.yaw = spawn.yaw;
        input.pitch = 0.0;
        velocity.0 = Vec3::ZERO;
    }
    reset.write(RoundReset {
        round: round.number,
    });
}

/// Who can't walk this tick: planting/disabling, the dead, choosing an
/// operator, attackers during prep (if tuned so), and everyone after the
/// match.
fn update_movement_locks(
    round: Res<Round>,
    tuning: Res<Tuning>,
    players: Query<
        (Entity, &Team, Has<ObjectiveInteraction>, Has<Dead>, Has<MovementLock>),
        With<Player>,
    >,
    mut commands: Commands,
) {
    for (entity, team, interacting, dead, locked) in &players {
        let should = interacting
            || dead
            || match round.phase {
                RoundPhase::OperatorSelect | RoundPhase::MatchEnd => true,
                RoundPhase::Prep => *team == Team::Attackers && tuning.round.attackers_hold_in_prep,
                _ => false,
            };
        if should && !locked {
            commands.entity(entity).insert(MovementLock);
        } else if !should && locked {
            commands.entity(entity).remove::<MovementLock>();
        }
    }
}
