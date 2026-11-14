//! Presentation of gadget intel and gadget sounds.
//!
//! [`Reveal`]s for the local player's team become world markers drawn on top
//! of everything (visible through walls and floors):
//! - sensor-dart reveals: cyan ripples spreading on the floor,
//! - seismic-stake pings: an orange diamond over a pulsing ring.
//!
//! `--show-all-intel` shows both teams' markers (for testing).

use std::f32::consts::FRAC_PI_2;

use bevy::{color::palettes::css, prelude::*};

use super::{GadgetEvent, Reveal, RevealSource};
use crate::{
    combat::Team,
    config::Tuning,
    player::{AutomatedRun, LocalPlayer},
    round::{RoundSettings, sound::{Sfx, play}},
};

pub struct IntelPlugin;

impl Plugin for IntelPlugin {
    fn build(&self, app: &mut App) {
        app.init_gizmo_group::<IntelGizmos>()
            .init_resource::<IntelMarkers>()
            .add_systems(Startup, configure_gizmos)
            .add_systems(Update, (collect_reveals, draw_markers).chain())
            .add_systems(
                Update,
                gadget_sounds.run_if(not(resource_exists::<AutomatedRun>)),
            );
    }
}

/// Gizmos drawn in front of all geometry.
#[derive(Default, Reflect, GizmoConfigGroup)]
struct IntelGizmos;

fn configure_gizmos(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<IntelGizmos>();
    config.depth_bias = -1.0;
    config.line.width = 3.0;
}

/// Intel currently on screen for the local player.
#[derive(Resource, Default, Debug)]
pub struct IntelMarkers {
    pub items: Vec<IntelMarker>,
}

#[derive(Debug, Clone)]
pub struct IntelMarker {
    pub position: Vec3,
    pub age: f32,
    pub source: RevealSource,
}

fn collect_reveals(
    mut reveals: MessageReader<Reveal>,
    mut markers: ResMut<IntelMarkers>,
    local: Query<&Team, With<LocalPlayer>>,
    settings: Res<RoundSettings>,
    tuning: Res<Tuning>,
    time: Res<Time>,
) {
    let my_team = local.single().ok().copied();
    for reveal in reveals.read() {
        if settings.show_all_intel || Some(reveal.for_team) == my_team {
            markers.items.push(IntelMarker {
                position: reveal.position,
                age: 0.0,
                source: reveal.source,
            });
        }
    }
    let dt = time.delta_secs();
    let intel = &tuning.round.intel;
    markers.items.retain_mut(|m| {
        m.age += dt;
        m.age
            < match m.source {
                RevealSource::SensorDart => intel.ripple_time,
                RevealSource::SeismicStake => intel.ping_time,
            }
    });
}

fn draw_markers(markers: Res<IntelMarkers>, tuning: Res<Tuning>, mut gizmos: Gizmos<IntelGizmos>) {
    let flat = Quat::from_rotation_x(FRAC_PI_2);
    let intel = &tuning.round.intel;
    for m in &markers.items {
        let ground = m.position + Vec3::Y * 0.05;
        match m.source {
            RevealSource::SensorDart => {
                let life = (m.age / intel.ripple_time).clamp(0.0, 1.0);
                for ring in 0..3 {
                    let t = (life + ring as f32 / 3.0).fract();
                    let color = Color::from(css::AQUA).with_alpha((1.0 - t) * (1.0 - life * 0.6));
                    gizmos.circle(Isometry3d::new(ground, flat), 0.15 + t * 1.1, color);
                }
            }
            RevealSource::SeismicStake => {
                let life = (m.age / intel.ping_time).clamp(0.0, 1.0);
                let color = Color::from(css::ORANGE_RED).with_alpha(1.0 - life * 0.8);
                let top = m.position + Vec3::Y * 1.1;
                let (h, w) = (0.22, 0.16);
                let points = [
                    top + Vec3::Y * h,
                    top + Vec3::X * w,
                    top - Vec3::Y * h,
                    top - Vec3::X * w,
                    top + Vec3::Y * h,
                ];
                gizmos.linestrip(points, color);
                gizmos.line(top - Vec3::Y * h, ground, color.with_alpha(0.4 * (1.0 - life)));
                gizmos.circle(Isometry3d::new(ground, flat), 0.2 + life * 0.6, color);
            }
        }
    }
}

/// Launcher pop for the shooter, hiss for defenders near a landing dart,
/// a thunk when placing a stake, a tick for every stake ping.
fn gadget_sounds(
    mut commands: Commands,
    mut events: MessageReader<GadgetEvent>,
    local: Query<(Entity, &Transform, &Team), With<LocalPlayer>>,
    sfx: Res<Sfx>,
    tuning: Res<Tuning>,
) {
    let Ok((me, at, my_team)) = local.single() else {
        return;
    };
    for event in events.read() {
        let sound = match event {
            GadgetEvent::DartFired { by } if *by == me => Some((&sfx.dart_fire, 0.7)),
            GadgetEvent::DartStuck { position, team }
                if team != my_team
                    && position.distance(at.translation) <= tuning.round.darts.hiss_radius =>
            {
                Some((&sfx.hiss, 0.9))
            }
            GadgetEvent::StakePlaced { by, .. } if *by == me => Some((&sfx.stake_place, 0.9)),
            GadgetEvent::StakePing { team, .. } if team == my_team => Some((&sfx.tick, 0.6)),
            _ => None,
        };
        if let Some((sound, volume)) = sound {
            play(&mut commands, sound, volume, &tuning);
        }
    }
}
