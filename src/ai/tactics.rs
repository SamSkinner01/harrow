//! What the map means to a bot, worked out from the nav grid, the bomb
//! sites and the destructible surfaces (no hand-placed hints), and kept
//! fresh as the level changes:
//!
//! - **Entrances** of each site: where nav links cross the site's boundary
//!   (doorways, holes, drops), clustered into one point per opening.
//! - **Hold spots**: places inside a site with a line of sight to an
//!   entrance from a few metres away, preferring cover next to a wall and
//!   an off-angle (not straight down the doorway).
//! - **Walls to reinforce**: reinforceable soft walls and hatches around a
//!   site, with where to stand (on the site side) and where to look.
//! - **Doorways to barricade**: open doorways at a site's entrances.
//! - **Stake spots**: a few metres outside each entrance, on the approach.
//! - **Roam spots**: indoor places away from the sites, for roamers.
//!
//! [`find_cover`] searches near a bot for a spot out of a threat's sight.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::{
    AiSettings,
    nav::{EDGE, LinkKind, NavGrid, NodeId, STAND},
};
use crate::{
    config::Tuning,
    destruction::{BarricadeSlot, SoftSurface, SurfaceRole},
    physics::GameLayer,
    round::{
        RoundReset,
        sites::{KeepLayout, SiteId},
    },
};

pub struct TacticsPlugin;

impl Plugin for TacticsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Tactics>()
            .add_systems(Update, analyse_map);
    }
}

#[derive(Clone, Debug)]
pub struct Entrance {
    /// Just inside the opening (floor).
    pub inside: Vec3,
    /// Just outside.
    pub outside: Vec3,
    /// Out of the site, horizontal.
    pub out_dir: Vec3,
    pub kind: LinkKind,
}

#[derive(Clone, Debug)]
pub struct HoldSpot {
    pub pos: Vec3,
    /// Point to watch (the entrance, chest height).
    pub watch: Vec3,
    pub entrance: usize,
    pub cover: bool,
}

#[derive(Clone, Debug)]
pub struct WallJob {
    pub surface: Entity,
    pub stand: Vec3,
    pub aim: Vec3,
    /// Crouch to reach (hatches from above).
    pub crouch: bool,
    pub hatch: bool,
}

#[derive(Clone, Debug)]
pub struct DoorJob {
    pub slot: Entity,
    pub stand: Vec3,
    pub aim: Vec3,
    /// Where a barricade would stand (to see if one is up).
    pub at: Vec3,
}

#[derive(Clone, Debug)]
pub struct SiteTactics {
    pub id: SiteId,
    pub centre: Vec3,
    pub min: Vec3,
    pub max: Vec3,
    /// Standable nodes inside.
    pub inside: Vec<NodeId>,
    pub entrances: Vec<Entrance>,
    pub holds: Vec<HoldSpot>,
    pub walls: Vec<WallJob>,
    pub doors: Vec<DoorJob>,
    pub stakes: Vec<(Vec3, Vec3)>,
}

impl SiteTactics {
    pub fn contains(&self, p: Vec3) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }

    /// A standable node inside, spread by `pick`.
    pub fn inside_point(&self, grid: &NavGrid, pick: usize) -> Vec3 {
        if self.inside.is_empty() {
            return self.centre;
        }
        grid.pos(self.inside[pick % self.inside.len()])
    }
}

/// The map's tactical layout. `revision` follows the nav grid's.
#[derive(Resource, Default)]
pub struct Tactics {
    pub ready: bool,
    pub revision: u32,
    pub sites: Vec<SiteTactics>,
    pub roam: Vec<Vec3>,
    /// Middle of the sites (for "outside north" style callouts).
    pub centre: Vec3,
}

impl Tactics {
    pub fn site(&self, id: SiteId) -> Option<&SiteTactics> {
        self.sites.iter().find(|s| s.id == id)
    }
}

/// Recompute when the grid first becomes ready, after a round reset, and
/// (at most every few seconds) after the grid changes.
#[allow(clippy::too_many_arguments)]
fn analyse_map(
    mut tactics: ResMut<Tactics>,
    grid: Res<NavGrid>,
    settings: Res<AiSettings>,
    layout: Option<Res<KeepLayout>>,
    surfaces: Query<(Entity, &SoftSurface, &GlobalTransform, &Name)>,
    slots: Query<(Entity, &BarricadeSlot, &GlobalTransform)>,
    spatial: SpatialQuery,
    tuning: Res<Tuning>,
    mut resets: MessageReader<RoundReset>,
    time: Res<Time<Real>>,
    mut last: Local<f32>,
    mut pending_reset: Local<bool>,
) {
    if resets.read().last().is_some() {
        *pending_reset = true;
    }
    if !settings.nav_wanted() || !grid.ready {
        return;
    }
    let Some(layout) = layout else { return };
    let now = time.elapsed_secs();
    let stale = grid.revision != tactics.revision && now - *last > 3.0;
    // After a reset the grid is patched over a few frames; wait for it.
    let reset_due = *pending_reset && grid.revision != tactics.revision && now - *last > 0.5;
    if tactics.ready && !stale && !reset_due {
        return;
    }
    *pending_reset = false;
    *last = now;
    let started = std::time::Instant::now();
    let filter = SpatialQueryFilter::from_mask(GameLayer::World);
    let eye = tuning.body.stand_height - tuning.body.eye_from_top;
    let clear = |a: Vec3, b: Vec3| {
        let d = b - a;
        let Ok(dir) = Dir3::new(d) else { return true };
        spatial.cast_ray(a, dir, d.length(), true, &filter).is_none_or(|h| h.distance >= d.length() - 0.1)
    };
    // Only places people can walk to (not crate tops or roofs).
    let main = grid.main_area();
    let mut sites = Vec::new();
    for site in &layout.sites {
        let min = site.min;
        let max = site.max;
        let centre = site.centre();
        let inside_box = |p: Vec3| p.cmpge(min).all() && p.cmple(max).all();
        let pad = 2.0;
        let around = grid.nodes_near(centre, (max - min).with_y(0.0).length() / 2.0 + pad, 4.5);
        let inside: Vec<NodeId> = around
            .iter()
            .copied()
            .filter(|&id| main[id as usize] && grid.node(id).standable() && inside_box(grid.pos(id) + Vec3::Y * 0.1))
            .collect();

        // Boundary crossings, both ways (drops come in from above).
        let mut crossings: Vec<(Vec3, Vec3, LinkKind)> = Vec::new();
        for &id in &around {
            let p = grid.pos(id);
            let a_in = inside_box(p + Vec3::Y * 0.1);
            for (to, kind, _) in grid.links(id) {
                let q = grid.pos(to);
                let b_in = inside_box(q + Vec3::Y * 0.1);
                if a_in && !b_in && kind != LinkKind::Drop {
                    crossings.push((p, q, kind));
                } else if !a_in && b_in {
                    crossings.push((q, p, kind));
                }
            }
        }
        let mut entrances: Vec<(Vec<(Vec3, Vec3)>, LinkKind)> = Vec::new();
        for (inside_p, outside_p, kind) in crossings {
            match entrances
                .iter_mut()
                .find(|(pts, k)| *k == kind && pts.iter().any(|(i, _)| i.distance(inside_p) < 1.3))
            {
                Some((pts, _)) => pts.push((inside_p, outside_p)),
                None => entrances.push((vec![(inside_p, outside_p)], kind)),
            }
        }
        let entrances: Vec<Entrance> = entrances
            .into_iter()
            .map(|(pts, kind)| {
                let n = pts.len() as f32;
                let inside_p = pts.iter().map(|(i, _)| *i).sum::<Vec3>() / n;
                let outside_p = pts.iter().map(|(_, o)| *o).sum::<Vec3>() / n;
                let out_dir = (outside_p - inside_p).with_y(0.0).normalize_or((inside_p - centre).with_y(0.0).normalize_or(Vec3::Z));
                Entrance { inside: inside_p, outside: outside_p, out_dir, kind }
            })
            .collect();

        // Hold spots: inside, seeing an entrance from 2.5..10 m.
        let mut holds = Vec::new();
        // Watch real openings; intact soft walls only if there are none.
        let openings = entrances.iter().any(|e| e.kind != LinkKind::Breach);
        for (e_index, e) in entrances.iter().enumerate() {
            if openings && e.kind == LinkKind::Breach {
                continue;
            }
            let watch = e.inside + Vec3::Y * 1.2;
            let mut candidates: Vec<(f32, HoldSpot)> = Vec::new();
            for (k, &id) in inside.iter().enumerate() {
                if k % 2 == 1 {
                    continue;
                }
                let p = grid.pos(id);
                let d = p.distance(e.inside);
                if !(2.5..=10.0).contains(&d) || (p.y - e.inside.y).abs() > 1.0 && e.kind != LinkKind::Drop {
                    continue;
                }
                if !clear(p + Vec3::Y * eye, watch) {
                    continue;
                }
                let cover = grid.node(id).flags & EDGE != 0;
                // Off-angle: not straight down the opening.
                let along = (p - e.inside).with_y(0.0).normalize_or_zero().dot(-e.out_dir);
                let score = (d - 5.0).abs() * 0.4 + along.max(0.0) * 1.5 - if cover { 1.0 } else { 0.0 };
                candidates.push((score, HoldSpot { pos: p, watch, entrance: e_index, cover }));
            }
            candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
            // Keep a few, spread apart.
            let mut kept: Vec<HoldSpot> = Vec::new();
            for (_, spot) in candidates {
                if kept.iter().all(|k| k.pos.distance(spot.pos) > 1.5) {
                    kept.push(spot);
                }
                if kept.len() >= 3 {
                    break;
                }
            }
            holds.extend(kept);
        }

        // Walls and hatches to reinforce.
        let mut walls = Vec::new();
        for (entity, surface, gt, _) in &surfaces {
            if !surface.can_reinforce() {
                continue;
            }
            let (gmin, gmax) = (surface.grid.min(), surface.grid.max());
            let mid_u = (gmin.x + gmax.x) / 2.0;
            match surface.role() {
                SurfaceRole::Wall => {
                    let base = gt.transform_point(Vec3::new(mid_u, 0.0, 0.0));
                    let half = gt.affine().transform_vector3(Vec3::X * (gmax.x - gmin.x) / 2.0);
                    // Near the site box, on its floor.
                    let pad = Vec3::new(0.4, 0.0, 0.4);
                    let ends = [base - half, base + half, base];
                    let near = ends.iter().any(|p| {
                        let q = p.clamp(min - pad, max + pad);
                        q.with_y(0.0).distance(p.with_y(0.0)) < 0.05
                    }) && (base.y - site.floor).abs() < 0.6;
                    if !near {
                        continue;
                    }
                    let normal = gt.affine().transform_vector3(Vec3::Z).with_y(0.0).normalize_or_zero();
                    let side = if (centre - base).dot(normal) >= 0.0 { 1.0 } else { -1.0 };
                    let want = base + normal * side * 0.8;
                    let Some(node) = grid.locate(want + Vec3::Y * 0.2, 2).filter(|n| main[*n as usize]) else { continue };
                    let stand = grid.pos(node);
                    if stand.distance(want) > 0.8 || !grid.node(node).standable() {
                        continue;
                    }
                    walls.push(WallJob {
                        surface: entity,
                        stand,
                        aim: gt.transform_point(Vec3::new(mid_u, 1.3, 0.0)),
                        crouch: false,
                        hatch: false,
                    });
                }
                SurfaceRole::Hatch => {
                    let mid_v = (gmin.y + gmax.y) / 2.0;
                    let hatch = gt.transform_point(Vec3::new(mid_u, 0.0, mid_v));
                    let in_xz = hatch.x >= min.x - 0.5 && hatch.x <= max.x + 0.5 && hatch.z >= min.z - 0.5 && hatch.z <= max.z + 0.5;
                    if !in_xz || hatch.y < site.floor - 0.5 || hatch.y > site.floor + 3.6 {
                        continue;
                    }
                    // From below if this site is under it, else crouched beside it.
                    let below = hatch.y - site.floor > 1.0;
                    let job = if below {
                        grid.locate(Vec3::new(hatch.x, site.floor + 0.2, hatch.z), 1).filter(|n| main[*n as usize]).and_then(|n| {
                            let p = grid.pos(n);
                            (grid.node(n).standable() && hatch.y - p.y < 3.4 && p.with_y(0.0).distance(hatch.with_y(0.0)) < 0.5)
                                .then_some(WallJob { surface: entity, stand: p, aim: hatch - Vec3::Y * 0.03, crouch: false, hatch: true })
                        })
                    } else {
                        let side = hatch + Vec3::new(0.0, 0.2, 0.9);
                        grid.locate(side, 2).filter(|n| main[*n as usize]).map(|n| WallJob {
                            surface: entity,
                            stand: grid.pos(n),
                            aim: hatch,
                            crouch: true,
                            hatch: true,
                        })
                    };
                    walls.extend(job);
                }
                _ => {}
            }
        }

        // Open doorways at entrances, to barricade.
        let mut doors = Vec::new();
        for (slot_entity, slot, gt) in &slots {
            if slot.window {
                continue;
            }
            let opening = gt.transform_point(Vec3::new((slot.min.x + slot.max.x) / 2.0, 1.1, 0.0));
            let at = gt.transform_point(Vec3::new((slot.min.x + slot.max.x) / 2.0, 0.0, 0.0));
            let Some(e) = entrances
                .iter()
                .filter(|e| e.kind == LinkKind::Walk)
                .find(|e| e.inside.with_y(0.0).distance(at.with_y(0.0)) < 1.6 && (e.inside.y - at.y).abs() < 0.8)
            else {
                continue;
            };
            let want = at - e.out_dir * 1.0;
            let Some(node) = grid.locate(want + Vec3::Y * 0.2, 2) else { continue };
            if !grid.node(node).standable() || !main[node as usize] {
                continue;
            }
            doors.push(DoorJob { slot: slot_entity, stand: grid.pos(node), aim: opening, at });
        }

        // Stake spots: out along each entrance's approach.
        let spread = tuning.ai.behaviour.stake_spread;
        let mut stakes = Vec::new();
        for e in entrances.iter().filter(|e| e.kind == LinkKind::Walk) {
            let want = e.outside + e.out_dir * spread;
            if let Some(node) = grid.locate(want + Vec3::Y * 0.3, 3).filter(|n| main[*n as usize]) {
                let p = grid.pos(node);
                if !inside_box(p + Vec3::Y * 0.1) && grid.node(node).standable() {
                    stakes.push((p, e.out_dir));
                }
            }
        }

        sites.push(SiteTactics {
            id: site.id,
            centre,
            min,
            max,
            inside,
            entrances,
            holds,
            walls,
            doors,
            stakes,
        });
    }

    // Roam spots: indoor nodes away from the sites, every few metres.
    let mut roam = Vec::new();
    let centre: Vec3 = layout.sites.iter().map(|s| s.centre()).sum::<Vec3>() / layout.sites.len().max(1) as f32;
    for id in grid.nodes_near(centre, 40.0, 12.0) {
        let p = grid.pos(id);
        let column = ((Vec2::new(p.x, p.z) - grid.origin) / grid.cell).round().as_ivec2();
        if column.x % 6 != 0 || column.y % 6 != 0 || !grid.node(id).standable() || !main[id as usize] {
            continue;
        }
        if layout.site_at(p + Vec3::Y * 0.1).is_some() {
            continue;
        }
        let covered = spatial.cast_ray(p + Vec3::Y, Dir3::Y, 6.0, true, &filter).is_some();
        if covered {
            roam.push(p);
        }
    }

    let count = |f: fn(&SiteTactics) -> usize| sites.iter().map(f).sum::<usize>();
    info!(
        "ai: map analysed in {:.1} ms: {} entrances, {} hold spots, {} walls to reinforce, {} doorways, {} stake spots, {} roam spots",
        started.elapsed().as_secs_f32() * 1000.0,
        count(|s| s.entrances.len()),
        count(|s| s.holds.len()),
        count(|s| s.walls.len()),
        count(|s| s.doors.len()),
        count(|s| s.stakes.len()),
        roam.len()
    );
    *tactics = Tactics {
        ready: true,
        revision: grid.revision,
        sites,
        roam,
        centre,
    };
}

/// A nearby standable spot out of sight of `threat` (an eye position),
/// not closer to it than the bot. Up to ~24 line checks.
pub fn find_cover(
    grid: &NavGrid,
    spatial: &SpatialQuery,
    from: Vec3,
    threat: Vec3,
    eye_height: f32,
    radius: f32,
) -> Option<Vec3> {
    let filter = SpatialQueryFilter::from_mask(GameLayer::World);
    let mut candidates: Vec<(f32, Vec3)> = grid
        .nodes_near(from, radius, 1.0)
        .into_iter()
        .filter(|&id| grid.node(id).flags & STAND != 0)
        .map(|id| grid.pos(id))
        .filter(|p| p.distance(threat) >= from.distance(threat) - 1.0 && p.distance(from) > 1.0)
        .map(|p| (p.distance(from), p))
        .collect();
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
    candidates
        .into_iter()
        .step_by(2)
        .take(24)
        .map(|(_, p)| p)
        .find(|p| {
            let eye = *p + Vec3::Y * eye_height;
            let d = threat - eye;
            let Ok(dir) = Dir3::new(d) else { return false };
            spatial
                .cast_ray(eye, dir, d.length(), true, &filter)
                .is_some_and(|h| h.distance < d.length() - 0.3)
        })
}
