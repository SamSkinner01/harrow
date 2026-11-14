//! World sounds: bullet impacts (by surface material, rate-limited), body
//! hits, surfaces breaking, explosions. All 3D at the event's position.

use bevy::prelude::*;

use super::{
    Category, Cue, Sound, Sounds,
    surface::{SurfaceLookup, from_surface_material},
};
use crate::{
    combat::{ImpactKind, SurfaceImpact},
    config::{AudioTuning, SoundMaterial},
    destruction::{Explosion, SoftSurface, SurfaceBroken, SurfaceChanged},
    weapons::BulletHit,
};

pub(super) struct WorldSoundsPlugin;

impl Plugin for WorldSoundsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (impact_sounds, break_sounds, explosion_sounds));
    }
}

/// Keeps full auto and shotgun pellets from stacking dozens of impact
/// sounds: a per-second budget, and impacts close to a recent one merge.
#[derive(Default)]
struct ImpactLimiter {
    tokens: f32,
    last: f32,
    recent: Vec<(f32, Vec3)>,
}

impl ImpactLimiter {
    fn allow(&mut self, now: f32, at: Vec3, audio: &AudioTuning) -> Result<(), &'static str> {
        let rate = audio.impacts_per_second.max(0.0);
        // Up to a fifth of a second's budget at once.
        let burst = (rate * 0.2).max(1.0);
        self.tokens = (self.tokens + (now - self.last).max(0.0) * rate).min(burst);
        self.last = now;
        let (radius, window) = audio.impact_merge;
        self.recent.retain(|(t, _)| now - t < window);
        if self.recent.iter().any(|(_, p)| p.distance(at) < radius) {
            return Err("merged with a nearby impact");
        }
        if self.tokens < 1.0 {
            return Err("impact rate limit");
        }
        self.tokens -= 1.0;
        self.recent.push((now, at));
        Ok(())
    }
}

fn impact_sounds(
    mut impacts: MessageReader<SurfaceImpact>,
    mut hits: MessageReader<BulletHit>,
    surfaces: SurfaceLookup,
    mut sounds: Sounds,
    time: Res<Time>,
    mut limiter: Local<ImpactLimiter>,
) {
    if !sounds.enabled() {
        impacts.clear();
        hits.clear();
        return;
    }
    let now = time.elapsed_secs();
    for impact in impacts.read() {
        // Melee hits are heavier and lower; explosions have their own sound.
        let (event, gain, speed) = match impact.kind {
            ImpactKind::Bullet => ("bullet impact", 1.0, 1.0),
            ImpactKind::Pellet => ("pellet impact", 0.8, 1.05),
            ImpactKind::Melee => ("melee impact", 1.4, 0.82),
            ImpactKind::Explosion { .. } => continue,
        };
        if let Err(reason) = limiter.allow(now, impact.point, &sounds.tuning().audio) {
            sounds.log_skip(event, reason);
            continue;
        }
        let from = impact.point - impact.direction * 0.05;
        let surface = surfaces.material(impact.collider, impact.point, from);
        sounds.play(
            Sound::new(event, Cue::Impact(surface.material), Category::Impacts)
                .at(impact.point)
                .gain(gain)
                .speed(speed)
                .detail(surface.describe()),
        );
    }
    for hit in hits.read() {
        // Hit zones are flesh; other hit volumes are gadgets.
        let material = if hit.zone.is_some() {
            SoundMaterial::Body
        } else {
            SoundMaterial::Metal
        };
        if let Err(reason) = limiter.allow(now, hit.point, &sounds.tuning().audio) {
            sounds.log_skip("bullet hit", reason);
            continue;
        }
        let detail = match hit.zone {
            Some(zone) => format!("{material:?} ({zone:?})"),
            None => format!("{material:?} (gadget)"),
        };
        sounds.play(
            Sound::new("bullet hit", Cue::Impact(material), Category::Impacts)
                .at(hit.point)
                .gain(1.2)
                .detail(detail),
        );
    }
}

/// Breaking: violent surface changes (blasts, breaking open), surfaces that
/// broke (hatches, barricades) and big non-violent changes (melee holes).
/// An explosion can change several surfaces on one tick, so only the
/// biggest few play.
fn break_sounds(
    mut changed: MessageReader<SurfaceChanged>,
    mut broken: MessageReader<SurfaceBroken>,
    surfaces: Query<(&SoftSurface, &GlobalTransform)>,
    mut sounds: Sounds,
) {
    if !sounds.enabled() {
        changed.clear();
        broken.clear();
        return;
    }
    let min_cells = sounds.tuning().audio.break_min_cells;
    let mut breaks: Vec<Break> = Vec::new();
    for change in changed.read() {
        let cells = change.removed.len();
        if cells == 0 || (!change.violent && cells < min_cells) {
            continue;
        }
        let centre = change.removed.iter().copied().sum::<Vec3>() / cells as f32;
        let (event, base) = if change.violent {
            ("surface blown open", 1.0)
        } else {
            ("surface broken into", 0.6)
        };
        breaks.push(Break {
            surface: change.surface,
            at: centre,
            material: from_surface_material(change.material),
            gain: base * (cells as f32 / 40.0).sqrt().clamp(0.5, 1.2),
            event,
            detail: format!("{cells} cells"),
        });
    }
    for b in broken.read() {
        let Ok((soft, transform)) = surfaces.get(b.surface) else {
            continue;
        };
        let c = (soft.grid.min() + soft.grid.max()) / 2.0;
        let (w0, w1) = soft.spec.skin_depth;
        let centre = transform.transform_point(soft.spec.plane.to_local(c.x, c.y, (w0 + w1) / 2.0));
        // Breaking open also knocks cells out: one sound per surface.
        breaks.retain(|other| other.surface != b.surface);
        breaks.push(Break {
            surface: b.surface,
            at: centre,
            material: from_surface_material(soft.spec.skin_material),
            gain: 1.2,
            event: "surface broke",
            detail: format!("{:?}", b.role),
        });
    }
    breaks.sort_by(|a, b| b.gain.total_cmp(&a.gain));
    for b in breaks.into_iter().take(3) {
        let material = b.material;
        sounds.play(
            Sound::new(b.event, Cue::Break(material), Category::Destruction)
                .at(b.at)
                .gain(b.gain)
                .detail(format!("{material:?}, {}", b.detail)),
        );
    }
}

/// One breaking sound to play.
struct Break {
    surface: Entity,
    at: Vec3,
    material: SoundMaterial,
    gain: f32,
    event: &'static str,
    detail: String,
}

/// Explosions: the close recording and the distant layer, crossfaded by
/// distance (the distant one always adds a little tail).
fn explosion_sounds(mut explosions: MessageReader<Explosion>, mut sounds: Sounds) {
    if !sounds.enabled() {
        explosions.clear();
        return;
    }
    for blast in explosions.read() {
        let blend = sounds.distant_blend(blast.centre);
        let detail = format!("breach radius {:.1} m", blast.breach_radius);
        sounds.play(
            Sound::new("explosion", Cue::Explosion, Category::Explosions)
                .at(blast.centre)
                .gain(1.0 - 0.6 * blend)
                .important()
                .detail(detail.clone()),
        );
        sounds.play(
            Sound::new(
                "explosion (distant layer)",
                Cue::ExplosionFar,
                Category::ExplosionsDistant,
            )
            .at(blast.centre)
            .gain(0.3 + 0.7 * blend)
            .important()
            .detail(detail),
        );
    }
}
