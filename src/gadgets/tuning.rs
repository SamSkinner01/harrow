//! Gadget tuning (M6 part two): the `round.gadgets` section of
//! `assets/config/tuning.ron`. Every value is explained in the tuning file.

use serde::Deserialize;

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct GadgetTuning {
    /// A press of 3, 4 or G shorter than this is a tap (trigger, connect);
    /// longer is a hold (place, clamp, throw a new drone).
    pub tap_time: f32,
    pub ring_cutter: RingCutterTuning,
    pub backplate: BackplateTuning,
    pub counts: SecondaryCounts,
    pub throw: ThrowTuning,
    pub frag: FragTuning,
    pub flashbang: FlashTuning,
    pub smoke: SmokeTuning,
    pub impact: ImpactTuning,
    pub razor_wire: RazorWireTuning,
    pub cover: CoverTuning,
    pub alarm: AlarmTuning,
    pub drone: DroneTuning,
    pub cctv: CctvTuning,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct RingCutterTuning {
    pub reach: f32,
    pub clamp_time: f32,
    pub cut_time: f32,
    pub cut_radius: f32,
    pub cut_height: f32,
    pub expose_at: f32,
    pub reseat_time: f32,
    pub noise: f32,
    pub noise_interval: f32,
    pub health: f32,
    pub blade_health: f32,
    pub revolutions: f32,
    pub knockdown_depth: f32,
    pub knockdown_damage: f32,
    pub shake_radius: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct BackplateTuning {
    pub reach: f32,
    pub bolt_time: f32,
    pub gadget_radius: f32,
    pub hole_radius: f32,
    pub blast_radius: f32,
    pub blast_damage: f32,
    pub health: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct SecondaryCounts {
    pub frag: u8,
    pub flashbang: u8,
    pub smoke: u8,
    pub razor_wire: u8,
    pub ballistic_cover: u8,
    pub impact_charge: u8,
    pub proximity_alarm: u8,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ThrowTuning {
    pub speed: f32,
    pub lift: f32,
    pub radius: f32,
    pub restitution: f32,
    pub friction: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct FragTuning {
    pub fuse: f32,
    pub max_damage: f32,
    pub damage_radius: f32,
    pub breach_radius: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct FlashTuning {
    pub fuse: f32,
    pub radius: f32,
    pub blind_time: f32,
    pub deaf_time: f32,
    pub look_away: f32,
    pub loudness: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct SmokeTuning {
    pub fuse: f32,
    pub radius: f32,
    pub grow_time: f32,
    pub duration: f32,
    pub fade_time: f32,
    pub particles: usize,
    pub max_particles: usize,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ImpactTuning {
    pub speed: f32,
    pub breach_radius: f32,
    pub damage_radius: f32,
    pub max_damage: f32,
    pub max_flight: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct RazorWireTuning {
    pub place_time: f32,
    pub distance: f32,
    pub length: f32,
    pub depth: f32,
    pub height: f32,
    pub slow: f32,
    pub noise: f32,
    pub health: f32,
    pub melee_damage: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct CoverTuning {
    pub place_time: f32,
    pub distance: f32,
    pub width: f32,
    pub height: f32,
    pub health: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct AlarmTuning {
    pub place_time: f32,
    pub reach: f32,
    pub radius: f32,
    pub vertical_reach: f32,
    pub ping_interval: f32,
    pub health: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct DroneTuning {
    pub count: u8,
    pub throw_speed: f32,
    pub speed: f32,
    pub reverse_speed: f32,
    pub turn_rate: f32,
    pub acceleration: f32,
    pub gravity: f32,
    pub radius: f32,
    pub step_height: f32,
    pub max_slope: f32,
    pub health: f32,
    pub battery: f32,
    pub camera_height: f32,
    pub fov: f32,
    pub signal_range: f32,
    pub noise: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct CctvTuning {
    pub health: f32,
    pub fov: f32,
    pub pan_limit: f32,
    pub tilt_limit: f32,
}
