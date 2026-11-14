//! Bot tuning (`ai` section at the end of `assets/config/tuning.ron`).
//! Every value is explained in the tuning file; this is just the shape.

use serde::Deserialize;

use crate::config::RoundTimers;

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct AiTuning {
    /// Difficulty when `--difficulty` isn't given.
    pub difficulty: DifficultyLevel,
    pub easy: Difficulty,
    pub normal: Difficulty,
    pub hard: Difficulty,
    pub nav: NavTuning,
    pub senses: SensesTuning,
    pub behaviour: BehaviourTuning,
    /// Round timers used by `--ai-test` (instead of `round.test_timers`).
    pub test_timers: RoundTimers,
    /// Round wins for a match in `--ai-test`.
    pub test_rounds_to_win: u32,
}

impl AiTuning {
    pub fn preset(&self, level: DifficultyLevel) -> &Difficulty {
        match level {
            DifficultyLevel::Easy => &self.easy,
            DifficultyLevel::Normal => &self.normal,
            DifficultyLevel::Hard => &self.hard,
        }
    }
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum DifficultyLevel {
    Easy,
    Normal,
    Hard,
}

impl DifficultyLevel {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "easy" => Some(Self::Easy),
            "normal" => Some(Self::Normal),
            "hard" => Some(Self::Hard),
            _ => None,
        }
    }
}

/// How well a bot sees, aims and shoots.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Difficulty {
    pub reaction_time: f32,
    pub reaction_jitter: f32,
    pub aim_error: f32,
    pub aim_settle: f32,
    pub tracking_error: f32,
    pub turn_speed: f32,
    pub recoil_control: f32,
    pub head_aim: f32,
    pub fov: f32,
    pub sight_range: f32,
    pub peripheral_delay: f32,
    pub hearing: f32,
    pub wallbang_chance: f32,
    pub burst_close: u32,
    pub burst_far: u32,
    pub burst_pause: f32,
    pub crouch_chance: f32,
    pub lean_chance: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct NavTuning {
    pub cell: f32,
    pub agent_radius: f32,
    pub max_step: f32,
    pub max_drop: f32,
    pub drop_radius: f32,
    pub margin: f32,
    pub build_budget_ms: f32,
    pub update_budget_ms: f32,
    pub debug_range: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct SensesTuning {
    pub sight_interval: f32,
    pub hearing_range: f32,
    pub other_floor_hearing: f32,
    pub remember_seen: f32,
    pub remember_heard: f32,
    pub callout_repeat: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct BehaviourTuning {
    pub think_interval: f32,
    pub retreat_health: f32,
    pub reload_below: f32,
    pub stack_spacing: f32,
    pub careful_range: f32,
    pub avoid_radius: f32,
    pub stuck_time: f32,
    pub roamers: u32,
    pub dart_range: f32,
    pub stake_spread: f32,
}
