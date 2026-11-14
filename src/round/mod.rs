//! Round loop (M6): operator selection, prep and action phases, the defuser
//! objective, win conditions, score and side swaps.
//!
//! Data flow, one way, like the player module:
//!
//! 1. **Input** (`input.rs`, PreUpdate): keys -> [`RoundIntent`] on the local
//!    player. Bots and network clients will write the same component.
//! 2. **Simulation** (fixed tick): [`Round`] is the whole match state as
//!    plain data (phase, timer, score, sides), so a server can own and
//!    replicate it later. `flow.rs` runs the timers and win conditions,
//!    `defuser.rs` the plant/disable objective, `operators.rs` operator
//!    selection and stats.
//! 3. **Presentation** (`hud.rs`, `sound.rs`, Update): reads the simulation,
//!    never changes it.
//!
//! [`RoundPhase`] is also a Bevy `State` that mirrors `Round::phase` (it
//! changes at the next frame's state transition), so other modules can gate
//! systems with `run_if(in_state(RoundPhase::Prep))` or
//! `run_if(RoundPhase::allows_setup)`.
//!
//! Hooks for other modules:
//! - [`RoundReset`] at the start of every round: refill ammo, rebuild walls.
//! - [`MovementLock`]: movement should ignore move input while present.
//! - [`operators::OperatorSpeed`]: movement should multiply its speeds by it.
//! - [`ObjectiveInteraction`]: the player is planting/disabling; F belongs
//!   to the round module (e.g. don't start reinforcing).
//!
//! The objective data (site pairs, spawns) is per map (`sites.rs`). On maps
//! without sites (`--map greybox`), or with `--no-round` (free play for
//! testing other features), the round loop stays [`RoundPhase::Inactive`].

pub mod defuser;
mod flow;
mod hud;
mod input;
pub mod operators;
mod script;
pub mod sites;
// Synthesised sounds; not in the dedicated-server build (no audio).
#[cfg(feature = "client")]
pub mod sound;

use bevy::prelude::*;

use crate::{
    combat::Team,
    config::{RoundTimers, RoundTuning},
    player::{AutomatedRun, LocalPlayer, Player},
};

pub struct RoundPlugin;

impl Plugin for RoundPlugin {
    fn build(&self, app: &mut App) {
        let settings = RoundSettings::from_args();
        if settings.scripted {
            // Scripted runs don't grab the mouse and play no sounds.
            app.insert_resource(AutomatedRun);
        }
        app.insert_resource(settings)
            .init_state::<RoundPhase>()
            .init_resource::<Round>()
            .add_message::<RoundReset>()
            .add_message::<RoundEvent>()
            .configure_sets(
                FixedUpdate,
                (
                    RoundSystems::Script,
                    RoundSystems::Objective,
                    RoundSystems::Flow,
                    RoundSystems::Reactions,
                    RoundSystems::Locks,
                )
                    .chain(),
            )
            .add_observer(add_round_components)
            .add_observer(join_local_squad)
            .add_systems(Update, (mirror_phase_state, log_round_events))
            .add_plugins((
                sites::SitesPlugin,
                flow::FlowPlugin,
                operators::OperatorsPlugin,
                defuser::DefuserPlugin,
                input::RoundInputPlugin,
                hud::RoundHudPlugin,
                script::RoundScriptPlugin,
            ));
        #[cfg(feature = "client")]
        app.add_plugins(sound::RoundSoundPlugin);
    }
}

/// Fixed-tick order inside the round module. Gadgets run after `Flow`.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum RoundSystems {
    /// Test scripts (`--round-script`) act like a player here.
    Script,
    /// Plant, disable, defuser pick-up.
    Objective,
    /// Timers, phase changes, win conditions, score.
    Flow,
    /// Reactions to phase changes: respawn, operator stats, defuser hand-out.
    Reactions,
    /// Decide who can't move this tick.
    Locks,
}

/// Where the round is. Mirrors [`Round::phase`] as a Bevy state.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub enum RoundPhase {
    /// No round loop (not on a map with bomb sites, or not started yet).
    #[default]
    Inactive,
    /// Everyone picks an operator.
    OperatorSelect,
    /// Defenders set up; attackers wait at their spawn.
    Prep,
    /// Attackers push; the defuser can be planted.
    Action,
    /// The defuser is running; defenders must disable it.
    Planted,
    /// Result banner.
    RoundEnd,
    /// Match result banner, then a new match.
    MatchEnd,
}

impl RoundPhase {
    pub fn label(self) -> &'static str {
        match self {
            RoundPhase::Inactive => "FREE PLAY",
            RoundPhase::OperatorSelect => "OPERATOR SELECT",
            RoundPhase::Prep => "PREP",
            RoundPhase::Action => "ACTION",
            RoundPhase::Planted => "DEFUSER PLANTED",
            RoundPhase::RoundEnd => "ROUND OVER",
            RoundPhase::MatchEnd => "MATCH OVER",
        }
    }

    /// Characters are in the fight (prep, action or planted).
    pub fn is_live(self) -> bool {
        matches!(self, RoundPhase::Prep | RoundPhase::Action | RoundPhase::Planted)
    }

    /// Run condition for defender set-up actions such as reinforcing walls:
    /// true in prep, action and planted phases, and when there is no round
    /// loop at all (free play on test maps).
    #[allow(dead_code)] // for M4 (reinforcement)
    pub fn allows_setup(phase: Res<State<RoundPhase>>) -> bool {
        let phase = *phase.get();
        phase.is_live() || phase == RoundPhase::Inactive
    }
}

/// A persistent team for the whole match. Squads swap between attack and
/// defence; each character's [`Team`] says which side it plays this round.
#[derive(Component, Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
pub enum Squad {
    /// The local player's squad.
    Blue,
    Red,
}

impl Squad {
    pub fn other(self) -> Squad {
        match self {
            Squad::Blue => Squad::Red,
            Squad::Red => Squad::Blue,
        }
    }
}

pub fn other_team(team: Team) -> Team {
    match team {
        Team::Attackers => Team::Defenders,
        Team::Defenders => Team::Attackers,
    }
}

pub fn team_label(team: Team) -> &'static str {
    match team {
        Team::Attackers => "ATTACKERS",
        Team::Defenders => "DEFENDERS",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WinReason {
    DefuserCompleted,
    DefuserDisabled,
    TimeUp,
    Wipe,
}

impl WinReason {
    pub fn label(self) -> &'static str {
        match self {
            WinReason::DefuserCompleted => "Defuser completed",
            WinReason::DefuserDisabled => "Defuser disabled",
            WinReason::TimeUp => "Time ran out",
            WinReason::Wipe => "Team eliminated",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RoundResult {
    pub winner: Team,
    pub reason: WinReason,
}

/// The whole match state as plain data. Owned by the fixed-tick simulation
/// (later: the server); everything else only reads it.
/// Networked as `net::RoundSync` (the server's copy wins on clients).
#[derive(Resource, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Round {
    pub phase: RoundPhase,
    /// 1-based. 0 before the first round starts.
    pub number: u32,
    /// Seconds left in this phase. While planted: the defuser's time left.
    pub time_left: f32,
    /// Seconds since this phase began.
    pub elapsed: f32,
    pub score_blue: u32,
    pub score_red: u32,
    /// The side squad Blue plays this round; Red plays the other.
    pub blue_side: Team,
    /// Set when a round ends; cleared when the next one starts.
    pub result: Option<RoundResult>,
    /// Set when the match is decided.
    pub match_winner: Option<Squad>,
}

impl Default for Round {
    fn default() -> Self {
        Self {
            phase: RoundPhase::Inactive,
            number: 0,
            time_left: 0.0,
            elapsed: 0.0,
            score_blue: 0,
            score_red: 0,
            blue_side: Team::Attackers,
            result: None,
            match_winner: None,
        }
    }
}

impl Round {
    pub fn side_of(&self, squad: Squad) -> Team {
        match squad {
            Squad::Blue => self.blue_side,
            Squad::Red => other_team(self.blue_side),
        }
    }

    pub fn squad_on(&self, team: Team) -> Squad {
        if self.blue_side == team { Squad::Blue } else { Squad::Red }
    }

    pub fn score_of(&self, squad: Squad) -> u32 {
        match squad {
            Squad::Blue => self.score_blue,
            Squad::Red => self.score_red,
        }
    }

    /// Switch phase, restarting the phase clock.
    pub(crate) fn enter(&mut self, phase: RoundPhase, duration: f32, events: &mut MessageWriter<RoundEvent>) {
        self.phase = phase;
        self.time_left = duration;
        self.elapsed = 0.0;
        events.write(RoundEvent::PhaseStarted {
            round: self.number,
            phase,
        });
    }

    /// End the round with `winner` and score it.
    pub(crate) fn finish(
        &mut self,
        winner: Team,
        reason: WinReason,
        timers: &RoundTimers,
        events: &mut MessageWriter<RoundEvent>,
    ) {
        if !matches!(self.phase, RoundPhase::Prep | RoundPhase::Action | RoundPhase::Planted) {
            return;
        }
        match self.squad_on(winner) {
            Squad::Blue => self.score_blue += 1,
            Squad::Red => self.score_red += 1,
        }
        let result = RoundResult { winner, reason };
        self.result = Some(result);
        events.write(RoundEvent::RoundWon {
            round: self.number,
            result,
            score_blue: self.score_blue,
            score_red: self.score_red,
        });
        self.enter(RoundPhase::RoundEnd, timers.round_end, events);
    }
}

/// Sent once at the start of every round, after players are respawned and
/// before operator selection. Listen to it to reset per-round state (ammo,
/// destroyed walls, deployed gadgets).
#[derive(Message, Clone, Debug)]
pub struct RoundReset {
    pub round: u32,
}

/// Things that happened in the round, for presentation (HUD, sounds) and the
/// log. Written by the simulation.
/// Forwarded to clients by the server (`net/events.rs`).
#[derive(Message, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum RoundEvent {
    PhaseStarted {
        round: u32,
        phase: RoundPhase,
    },
    OperatorLocked {
        entity: Entity,
        operator: operators::OperatorId,
    },
    DefuserAssigned {
        entity: Entity,
    },
    DefuserEquipped {
        entity: Entity,
        equipped: bool,
    },
    PlantRefused {
        entity: Entity,
        /// A `Cow` so the event can be sent over the network.
        reason: std::borrow::Cow<'static, str>,
    },
    PlantStarted {
        entity: Entity,
        site: sites::SiteId,
    },
    PlantInterrupted {
        entity: Entity,
        reason: InterruptReason,
        progress: f32,
    },
    Planted {
        entity: Option<Entity>,
        site: sites::SiteId,
        position: Vec3,
    },
    DisableStarted {
        entity: Entity,
    },
    DisableInterrupted {
        entity: Entity,
        reason: InterruptReason,
        progress: f32,
    },
    Disabled {
        entity: Entity,
    },
    DefuserDropped {
        position: Vec3,
    },
    DefuserPickedUp {
        entity: Entity,
    },
    RoundWon {
        round: u32,
        result: RoundResult,
        score_blue: u32,
        score_red: u32,
    },
    MatchWon {
        squad: Squad,
        score_blue: u32,
        score_red: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum InterruptReason {
    /// Let go of F.
    Released,
    /// Walked out of the site / out of reach.
    LeftArea,
    Died,
    /// The phase ended (time up, round over).
    PhaseEnded,
}

/// What a character wants from the round module. Written by input (local
/// player), bots or the network; read by the fixed-tick simulation.
#[derive(Component, Default, Debug, Clone)]
pub struct RoundIntent {
    /// Holding interact (F): plant or disable the defuser.
    pub interact: bool,
    /// Pressed equip defuser (5). Cleared by the simulation once handled.
    pub equip_defuser: bool,
    /// Pressed an operator number on the selection screen (0-based).
    /// Cleared by the simulation once handled.
    pub pick_operator: Option<u8>,
}

/// Present while the character must not walk: planting, disabling, waiting
/// at the attacker spawn during prep, choosing an operator. Movement should
/// treat move input as zero (look still works).
///
/// Movement reads it (`player/movement.rs`); replicated, so a client
/// predicts its own character with the server's lock.
#[derive(Component, Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MovementLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum InteractionKind {
    Plant,
    Disable,
}

/// On a character that is planting or disabling the defuser.
#[derive(Component, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ObjectiveInteraction {
    pub kind: InteractionKind,
    /// 0..1.
    pub progress: f32,
}

/// Command-line options for the round loop. See README "Developer flags".
#[derive(Resource, Debug, Clone)]
pub struct RoundSettings {
    /// Round loop runs (maps with bomb sites, and not with `--no-round`).
    pub enabled: bool,
    /// `--round-test`: short timers and a short match.
    pub test: bool,
    /// `--team`: side the local squad starts on.
    pub start_side: Team,
    /// `--operator`.
    pub operator: Option<operators::OperatorId>,
    /// `--spawn` was given: keep it for the first round.
    pub keep_debug_spawn: bool,
    /// `--round-script` was given.
    pub scripted: bool,
    /// `--show-all-intel`: show both teams' gadget reveals.
    pub show_all_intel: bool,
}

impl RoundSettings {
    fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let has = |flag: &str| args.iter().any(|a| a == flag);
        let value = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
                .cloned()
        };
        let operator = value("--operator").and_then(|name| {
            let id = operators::OperatorId::from_name(&name);
            if id.is_none() {
                warn!("--operator expects auger|lark|mortise|fathom, got {name:?}");
            }
            id
        });
        let team = value("--team").and_then(|t| match t.as_str() {
            "attackers" | "attack" => Some(Team::Attackers),
            "defenders" | "defend" => Some(Team::Defenders),
            other => {
                warn!("--team expects attackers|defenders, got {other:?}");
                None
            }
        });
        let start_side = team
            .or_else(|| operator.map(|op| op.info().side))
            .unwrap_or(Team::Attackers);
        let enabled = crate::map::MapChoice::from_args().has_objectives() && !has("--no-round");
        Self {
            enabled,
            test: has("--round-test"),
            start_side,
            operator,
            keep_debug_spawn: has("--spawn"),
            scripted: has("--round-script"),
            show_all_intel: has("--show-all-intel"),
        }
    }

    pub fn timers<'a>(&self, tuning: &'a RoundTuning) -> &'a RoundTimers {
        if self.test { &tuning.test_timers } else { &tuning.timers }
    }

    pub fn rounds_to_win(&self, tuning: &RoundTuning) -> u32 {
        if self.test { tuning.test_rounds_to_win } else { tuning.rounds_to_win }
    }
}

/// Every character takes part in rounds.
fn add_round_components(add: On<Add<Player>>, mut commands: Commands) {
    // `if_new`: on a client the choice may already have been replicated.
    commands
        .entity(add.entity)
        .insert(RoundIntent::default())
        .insert_if_new(operators::OperatorChoice::default());
}

/// The local player is in squad Blue. (Bots get their squad from M7.)
fn join_local_squad(add: On<Add<LocalPlayer>>, mut commands: Commands) {
    // `if_new`: a networked character already has its squad.
    commands.entity(add.entity).insert_if_new(Squad::Blue);
}

/// Keep the Bevy state in step with the simulation's phase.
fn mirror_phase_state(
    round: Res<Round>,
    state: Res<State<RoundPhase>>,
    mut next: ResMut<NextState<RoundPhase>>,
) {
    if *state.get() != round.phase {
        next.set_if_different(round.phase);
    }
}

fn log_round_events(mut events: MessageReader<RoundEvent>, names: Query<&Name>) {
    let name = |e: Entity| {
        names
            .get(e)
            .map(|n| n.as_str().to_string())
            .unwrap_or_else(|_| format!("{e}"))
    };
    for event in events.read() {
        match event {
            RoundEvent::PhaseStarted { round, phase } => {
                info!("round {round}: phase {phase:?}")
            }
            RoundEvent::OperatorLocked { entity, operator } => {
                info!("round: {} locked in {}", name(*entity), operator.info().callsign)
            }
            RoundEvent::DefuserAssigned { entity } => {
                info!("round: {} carries the defuser", name(*entity))
            }
            RoundEvent::DefuserEquipped { entity, equipped } => {
                info!("round: {} defuser equipped = {equipped}", name(*entity))
            }
            RoundEvent::PlantRefused { entity, reason } => {
                info!("round: {} can't plant: {reason}", name(*entity))
            }
            RoundEvent::PlantStarted { entity, site } => {
                info!("round: {} started planting at site {site:?}", name(*entity))
            }
            RoundEvent::PlantInterrupted { entity, reason, progress } => info!(
                "round: {} plant interrupted ({reason:?}) at {:.0}%",
                name(*entity),
                progress * 100.0
            ),
            RoundEvent::Planted { entity, site, position } => info!(
                "round: defuser PLANTED at site {site:?} {position:.2} by {}",
                entity.map(name).unwrap_or_else(|| "script".into())
            ),
            RoundEvent::DisableStarted { entity } => {
                info!("round: {} started disabling the defuser", name(*entity))
            }
            RoundEvent::DisableInterrupted { entity, reason, progress } => info!(
                "round: {} disable interrupted ({reason:?}) at {:.0}%",
                name(*entity),
                progress * 100.0
            ),
            RoundEvent::Disabled { entity } => {
                info!("round: defuser DISABLED by {}", name(*entity))
            }
            RoundEvent::DefuserDropped { position } => {
                info!("round: defuser dropped at {position:.2}")
            }
            RoundEvent::DefuserPickedUp { entity } => {
                info!("round: {} picked up the defuser", name(*entity))
            }
            RoundEvent::RoundWon { round, result, score_blue, score_red } => info!(
                "round {round}: {} WIN ({}). Score blue {score_blue} - red {score_red}",
                team_label(result.winner),
                result.reason.label()
            ),
            RoundEvent::MatchWon { squad, score_blue, score_red } => info!(
                "match: squad {squad:?} WINS {score_blue}-{score_red}"
            ),
        }
    }
}
