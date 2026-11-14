//! Fixed tick: impacts, explosions, hard breaches and reinforcement change
//! the cell grids. Everything here is deterministic: the same impacts in the
//! same order give the same holes on every machine (randomness comes from a
//! hash of the surface's placement, its impact count and the cell).

use avian3d::prelude::*;
use bevy::prelude::*;

use super::{
    DestructionSystems, Explosion, HardBreach, ReinforceRequest, Reinforced, SurfaceBroken,
    SurfaceChanged,
    grid::{BRACE, CORE, CellRect, FRAME, PLATE, SKIN, hash, unit},
    spawn::SurfaceParts,
    surface::{SoftSurface, SurfaceMaterial, SurfaceRole},
};
use crate::{
    combat::{DamageKind, DamageMessage, Dead, Health, ImpactKind, SurfaceImpact, SurfaceKind},
    config::{DestructionTuning, Tuning},
    physics::GameLayer,
    player::HitZone,
};

pub struct ImpactPlugin;

impl Plugin for ImpactPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            explode.in_set(DestructionSystems::Explosions),
        )
        .add_systems(
            FixedUpdate,
            (apply_reinforce_requests, apply_impacts, apply_hard_breaches)
                .chain()
                .in_set(DestructionSystems::ApplyImpacts),
        );
    }
}

/// Every flag: a hard breach takes it all.
const ALL: u8 = SKIN | CORE | FRAME | PLATE | BRACE;

/// The surface root for a collider (or the root itself).
pub fn surface_root(entity: Entity, colliders: &Query<&ColliderOf>) -> Entity {
    colliders.get(entity).map(|c| c.body).unwrap_or(entity)
}

/// Removal helpers for one impact on one surface.
struct Cutter<'a> {
    surface: &'a mut SoftSurface,
    tuning: &'a DestructionTuning,
    removed: Vec<usize>,
}

impl Cutter<'_> {
    fn chance(&self, cell: usize, salt: u32) -> f32 {
        unit(hash(self.surface.seed ^ salt, self.surface.impacts, cell as u32))
    }

    /// Clears `clear` from cells whose centre is within `radius` (ragged).
    fn disc(&mut self, centre: Vec2, radius: f32, clear: u8, protect: u8) {
        if radius <= 0.0 {
            return;
        }
        let jitter = self.tuning.hole_jitter;
        let pad = radius + jitter;
        let rect = self.surface.grid.cells_overlapping(centre - pad, centre + pad);
        let nu = self.surface.grid.nu();
        let (seed, n) = (self.surface.seed, self.surface.impacts);
        self.surface.grid.clear_where(
            rect,
            clear,
            protect,
            |u, v, c| {
                let d = (unit(hash(seed, n, (v * nu + u) as u32)) * 2.0 - 1.0) * jitter;
                c.distance(centre) <= radius + d
            },
            &mut self.removed,
        );
    }

    /// Clears `clear` from cells whose centre is inside `min..max` (ragged).
    fn rect(&mut self, min: Vec2, max: Vec2, clear: u8, protect: u8) {
        let j = self.tuning.hole_jitter;
        let rect = self.surface.grid.cells_overlapping(min - j, max + j);
        let nu = self.surface.grid.nu();
        let seed = self.surface.seed;
        let n = self.surface.impacts;
        let jitter = self.tuning.hole_jitter;
        self.surface.grid.clear_where(
            rect,
            clear,
            protect,
            |u, v, c| {
                let d = (unit(hash(seed, n, (v * nu + u) as u32)) * 2.0 - 1.0) * jitter;
                c.cmpge(min - d).all() && c.cmple(max + d).all()
            },
            &mut self.removed,
        );
    }

    /// One bullet: the cell it hit (or, on a hole edge, the nearest
    /// breakable cell), plus a chance for each neighbour.
    fn bullet(&mut self, p: Vec2) {
        let grid = &self.surface.grid;
        let (cu, cv) = grid.cell_at(p);
        let breakable = |c: u8| c & SKIN != 0 && c & FRAME == 0;
        let hit = grid.get(cu as isize, cv as isize);
        let target = if breakable(hit) {
            Some((cu, cv))
        } else if hit & (SKIN | CORE) != 0 {
            // Frame or a bare stud: the bullet doesn't break it.
            None
        } else {
            // Hit the side of an existing hole: chip its edge.
            let mut best: Option<((usize, usize), f32)> = None;
            for dv in -1..=1 {
                for du in -1..=1 {
                    let (u, v) = (cu as isize + du, cv as isize + dv);
                    if breakable(grid.get(u, v)) {
                        let d = grid.centre(u as usize, v as usize).distance(p);
                        if best.is_none_or(|(_, b)| d < b) {
                            best = Some(((u as usize, v as usize), d));
                        }
                    }
                }
            }
            best.map(|(cell, _)| cell)
        };
        let Some((u, v)) = target else { return };
        let mut cells = vec![(u as isize, v as isize)];
        for (du, dv) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (nu, nv) = (u as isize + du, v as isize + dv);
            if nu >= 0 && nv >= 0 && (nu as usize) < grid.nu() && (nv as usize) < grid.nv() {
                let i = grid.index(nu as usize, nv as usize);
                if self.chance(i, 0xB0_11E7) < self.tuning.bullet_extra_chance {
                    cells.push((nu, nv));
                }
            }
        }
        for (u, v) in cells {
            let grid = &mut self.surface.grid;
            let i = grid.index(u as usize, v as usize);
            if breakable(grid.cells[i]) {
                grid.cells[i] &= !SKIN;
                self.removed.push(i);
            }
        }
    }

    /// Breaks a hatch or barricade completely.
    fn break_all(&mut self) {
        let grid = &mut self.surface.grid;
        let all = CellRect { u0: 0, v0: 0, u1: grid.nu(), v1: grid.nv() };
        grid.clear_where(all, SKIN | CORE, 0, |_, _, _| true, &mut self.removed);
        for fixture in &mut self.surface.spec.fixtures {
            if fixture.breakable {
                fixture.present = false;
            }
        }
        self.surface.broken = true;
    }
}

/// What happened to a surface.
struct Outcome {
    removed: Vec<usize>,
    broke: bool,
    violent: bool,
}

/// Applies one impact to a surface. `local` is the impact point in the
/// piece's local space, `down` whether it travelled downwards (from above).
fn cut(
    surface: &mut SoftSurface,
    kind: ImpactKind,
    local: Vec3,
    down: bool,
    tuning: &DestructionTuning,
) -> Outcome {
    let was_broken = surface.broken;
    let (uv, w) = surface.spec.plane.from_local(local);
    let role = surface.spec.role;
    let lip = surface.spec.lip;
    let floor_v = surface.grid.min().y;
    let mut c = Cutter {
        surface,
        tuning,
        removed: Vec::new(),
    };
    let mut violent = false;
    match (role, kind) {
        (SurfaceRole::Floor | SurfaceRole::Wall, ImpactKind::Bullet) => c.bullet(uv),
        (SurfaceRole::Hatch | SurfaceRole::Barricade, ImpactKind::Bullet) => c.bullet(uv),
        (_, ImpactKind::Pellet) => c.disc(uv, tuning.pellet_radius, SKIN, FRAME),
        (SurfaceRole::Wall, ImpactKind::Melee) => {
            let m = &tuning.melee;
            let marks = &mut c.surface.melee_marks;
            let mark = match marks
                .iter_mut()
                .find(|mark| mark.centre.distance(uv) < m.merge_distance)
            {
                Some(mark) => {
                    mark.hits += 1;
                    *mark
                }
                None => {
                    let mark = super::surface::MeleeMark { centre: uv, hits: 1 };
                    marks.push(mark);
                    mark
                }
            };
            let radius = m
                .hole_radii
                .get(mark.hits as usize - 1)
                .or(m.hole_radii.last())
                .copied()
                .unwrap_or(0.15);
            c.disc(mark.centre, radius, SKIN, FRAME);
            if mark.hits >= m.crouch_hole_hits {
                let half = m.crouch_hole_width / 2.0;
                c.rect(
                    Vec2::new(mark.centre.x - half, floor_v),
                    Vec2::new(mark.centre.x + half, floor_v + lip + m.crouch_hole_height),
                    SKIN,
                    FRAME,
                );
            }
        }
        (SurfaceRole::Floor, ImpactKind::Melee) => {}
        (SurfaceRole::Hatch, ImpactKind::Melee) => {
            if down && !c.surface.broken {
                c.surface.melee_hits += 1;
                if c.surface.melee_hits >= tuning.hatch_melee_hits {
                    c.break_all();
                } else {
                    c.disc(uv, 0.12, SKIN, 0);
                }
            }
        }
        (SurfaceRole::Barricade, ImpactKind::Melee) => {
            c.surface.melee_hits += 1;
            if c.surface.melee_hits >= tuning.barricade.melee_hits {
                c.break_all();
            } else {
                c.disc(uv, tuning.barricade.melee_radius, SKIN, 0);
            }
        }
        (_, ImpactKind::Explosion { radius }) => {
            violent = true;
            // The blast reaches the surface's plane with a smaller radius
            // the further it is from it.
            let (w0, w1) = c.surface.spec.skin_depth;
            let d = if w < w0 { w0 - w } else if w > w1 { w - w1 } else { 0.0 };
            if d < radius {
                let r = (radius * radius - d * d).sqrt();
                let (min, max) = (c.surface.grid.min(), c.surface.grid.max());
                let reaches = uv.clamp(min, max).distance(uv) <= r;
                match role {
                    SurfaceRole::Hatch | SurfaceRole::Barricade => {
                        if reaches {
                            c.break_all();
                        }
                    }
                    SurfaceRole::Wall => {
                        c.disc(uv, r, SKIN | CORE, FRAME);
                        let e = &tuning.explosion;
                        if uv.y - r <= floor_v + lip + e.blowout_reach {
                            let half = r * e.blowout_width;
                            c.rect(
                                Vec2::new(uv.x - half, floor_v),
                                Vec2::new(uv.x + half, uv.y),
                                SKIN | CORE,
                                FRAME,
                            );
                        }
                    }
                    SurfaceRole::Floor => c.disc(uv, r, SKIN | CORE, FRAME),
                }
            }
        }
    }
    // Barricades fall apart once most planks are gone.
    if role == SurfaceRole::Barricade
        && !c.surface.broken
        && !c.removed.is_empty()
        && c.surface.intact_fraction() < tuning.barricade.clear_below
    {
        c.break_all();
    }
    let broke = c.surface.broken && !was_broken;
    Outcome {
        removed: c.removed,
        broke,
        violent: violent || broke,
    }
}

/// World-space centres of cells (for debris).
fn cell_positions(surface: &SoftSurface, cells: &[usize], transform: &GlobalTransform) -> Vec<Vec3> {
    let grid = &surface.grid;
    let (w0, w1) = surface.spec.skin_depth;
    cells
        .iter()
        .map(|&i| {
            let (u, v) = (i % grid.nu(), i / grid.nu());
            let c = grid.centre(u, v);
            transform.transform_point(surface.spec.plane.to_local(c.x, c.y, (w0 + w1) / 2.0))
        })
        .collect()
}

fn apply_impacts(
    mut impacts: MessageReader<SurfaceImpact>,
    colliders: Query<&ColliderOf>,
    mut surfaces: Query<(&mut SoftSurface, &GlobalTransform)>,
    tuning: Res<Tuning>,
    mut changed: MessageWriter<SurfaceChanged>,
    mut broken: MessageWriter<SurfaceBroken>,
) {
    let t = &tuning.destruction;
    for impact in impacts.read() {
        let root = surface_root(impact.collider, &colliders);
        let Ok((mut surface, transform)) = surfaces.get_mut(root) else {
            continue;
        };
        // Reinforced surfaces ignore everything but hard breaches.
        if surface.is_reinforced() {
            continue;
        }
        let local = transform.affine().inverse().transform_point3(impact.point);
        let down = impact.direction.y < -0.3;
        let outcome = cut(&mut surface, impact.kind, local, down, t);
        surface.impacts = surface.impacts.wrapping_add(1);
        if outcome.removed.is_empty() && !outcome.broke {
            continue;
        }
        surface.revision += 1;
        let material = surface.spec.skin_material;
        changed.write(SurfaceChanged {
            surface: root,
            removed: cell_positions(&surface, &outcome.removed, transform),
            cell_size: t.cell_size,
            material,
            origin: impact.point,
            direction: impact.direction,
            violent: outcome.violent,
        });
        if outcome.broke {
            broken.write(SurfaceBroken {
                surface: root,
                role: surface.spec.role,
            });
        }
    }
}

fn apply_hard_breaches(
    mut breaches: MessageReader<HardBreach>,
    colliders: Query<&ColliderOf>,
    mut surfaces: Query<(&mut SoftSurface, &GlobalTransform)>,
    tuning: Res<Tuning>,
    mut changed: MessageWriter<SurfaceChanged>,
) {
    let t = &tuning.destruction;
    for breach in breaches.read() {
        let root = surface_root(breach.surface, &colliders);
        let Ok((mut surface, transform)) = surfaces.get_mut(root) else {
            warn!("hard breach on {root:?}, which is not a soft or reinforced surface");
            continue;
        };
        debug!("hard breach on {root:?} by {:?}", breach.source);
        let local = transform.affine().inverse().transform_point3(breach.centre);
        let (uv, _) = surface.spec.plane.from_local(local);
        let mut c = Cutter {
            surface: &mut surface,
            tuning: t,
            removed: Vec::new(),
        };
        c.disc(uv, breach.radius, ALL, 0);
        let removed = c.removed;
        surface.impacts = surface.impacts.wrapping_add(1);
        if removed.is_empty() {
            continue;
        }
        surface.revision += 1;
        let material = if surface.is_reinforced() {
            SurfaceMaterial::Metal
        } else {
            surface.spec.skin_material
        };
        let direction = transform.rotation() * surface.spec.plane.normal();
        changed.write(SurfaceChanged {
            surface: root,
            removed: cell_positions(&surface, &removed, transform),
            cell_size: t.cell_size,
            material,
            origin: breach.centre,
            direction,
            violent: true,
        });
    }
}

fn apply_reinforce_requests(
    mut requests: MessageReader<ReinforceRequest>,
    colliders: Query<&ColliderOf>,
    mut surfaces: Query<(&mut SoftSurface, &GlobalTransform)>,
    mut reinforced: MessageWriter<Reinforced>,
    mut commands: Commands,
) {
    for request in requests.read() {
        let root = surface_root(request.surface, &colliders);
        let Ok((mut surface, transform)) = surfaces.get_mut(root) else {
            continue;
        };
        if !surface.can_reinforce() {
            continue;
        }
        let local = transform.affine().inverse().transform_point3(request.from);
        let (_, w) = surface.spec.plane.from_local(local);
        surface.reinforce(w, request.by);
        commands.entity(root).insert(SurfaceKind::Hard);
        reinforced.write(Reinforced {
            surface: root,
            by: request.by,
        });
    }
}

/// Explosions: open every soft surface in range (by writing explosion
/// impacts, so they go through the same path as everything else) and hurt
/// people in range.
#[allow(clippy::too_many_arguments)]
fn explode(
    mut explosions: MessageReader<Explosion>,
    spatial: SpatialQuery,
    colliders: Query<&ColliderOf>,
    surfaces: Query<&SurfaceParts, With<SoftSurface>>,
    kinds: Query<&SurfaceKind>,
    characters: Query<(Entity, &GlobalTransform), (With<Health>, Without<Dead>)>,
    zones: Query<(&HitZone, &ChildOf, &GlobalTransform)>,
    tuning: Res<Tuning>,
    mut impacts: MessageWriter<SurfaceImpact>,
    mut damage: MessageWriter<DamageMessage>,
) {
    let t = &tuning.destruction.explosion;
    let world = SpatialQueryFilter::from_mask(GameLayer::World);
    for blast in explosions.read() {
        // Surfaces: one impact each, at the blast centre.
        let mut hit: Vec<(Entity, Entity)> = Vec::new();
        if let Some(root) = blast.attached_to
            && let Ok(parts) = surfaces.get(root)
        {
            hit.push((root, parts.collider));
        }
        // A charge stuck to a surface is shaped: it only opens that one.
        if blast.breach_radius > 0.0 && blast.attached_to.is_none() {
            let sphere = Collider::sphere(blast.breach_radius);
            for collider in spatial.shape_intersections(&sphere, blast.centre, Quat::IDENTITY, &world) {
                let root = surface_root(collider, &colliders);
                if surfaces.contains(root) && !hit.iter().any(|(r, _)| *r == root) {
                    hit.push((root, collider));
                }
            }
        }
        for (_, collider) in &hit {
            impacts.write(SurfaceImpact {
                collider: *collider,
                point: blast.centre,
                normal: Vec3::Y,
                // No single direction: debris flies out from the centre.
                direction: Vec3::ZERO,
                kind: ImpactKind::Explosion {
                    radius: blast.breach_radius,
                },
                source: blast.source,
            });
        }

        // People: the most exposed hit zone decides the damage.
        if blast.max_damage <= 0.0 || blast.damage_radius <= 0.0 {
            continue;
        }
        for (character, body) in &characters {
            let mut points: Vec<(Option<HitZone>, Vec3)> = zones
                .iter()
                .filter(|(_, parent, _)| parent.parent() == character)
                .map(|(zone, _, gt)| (Some(*zone), gt.translation()))
                .collect();
            if points.is_empty() {
                points.push((None, body.translation() + Vec3::Y));
            }
            let mut best: Option<(f32, Option<HitZone>)> = None;
            for (zone, point) in points {
                let offset = point - blast.centre;
                let distance = offset.length();
                if distance > blast.damage_radius {
                    continue;
                }
                let x = distance / blast.damage_radius;
                // Full damage close in, then dropping off steeply.
                let falloff = if x <= t.full_damage_fraction {
                    1.0
                } else {
                    (1.0 - (x - t.full_damage_fraction) / (1.0 - t.full_damage_fraction)).powi(2)
                };
                let mut amount = blast.max_damage * falloff;
                if let Ok(dir) = Dir3::new(offset) {
                    for ray in spatial.ray_hits(blast.centre, dir, distance, 8, true, &world) {
                        let root = surface_root(ray.entity, &colliders);
                        if Some(root) == blast.attached_to {
                            continue;
                        }
                        match kinds.get(root) {
                            Ok(SurfaceKind::Soft) => amount *= t.soft_cover_factor,
                            _ => {
                                amount = 0.0;
                                break;
                            }
                        }
                    }
                }
                if best.is_none_or(|(b, _)| amount > b) {
                    best = Some((amount, zone));
                }
            }
            if let Some((amount, zone)) = best
                && amount >= 0.5
            {
                damage.write(DamageMessage {
                    target: character,
                    amount,
                    kind: DamageKind::Explosion,
                    zone,
                    source: blast.source,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::destruction::surface::{layout_for, tests::tuning};

    fn wall() -> SoftSurface {
        let (spec, grid) = layout_for("soft_wall_2m", &tuning()).unwrap();
        SoftSurface::new(spec, grid, 42)
    }

    fn hit(surface: &mut SoftSurface, kind: ImpactKind, at: Vec3) -> Outcome {
        let outcome = cut(surface, kind, at, false, &tuning());
        surface.impacts += 1;
        outcome
    }

    fn cell(surface: &SoftSurface, u: f32, v: f32) -> u8 {
        let (cu, cv) = surface.grid.cell_at(Vec2::new(u, v));
        surface.grid.cells[surface.grid.index(cu, cv)]
    }

    #[test]
    fn same_impacts_give_the_same_holes() {
        let (mut a, mut b) = (wall(), wall());
        let shots = [
            (ImpactKind::Bullet, Vec3::new(0.6, 1.2, 0.07)),
            (ImpactKind::Bullet, Vec3::new(0.62, 1.25, 0.07)),
            (ImpactKind::Pellet, Vec3::new(1.3, 2.0, -0.07)),
            (ImpactKind::Melee, Vec3::new(0.6, 1.5, 0.07)),
            (ImpactKind::Explosion { radius: 0.5 }, Vec3::new(1.4, 0.6, 0.1)),
        ];
        for (kind, at) in shots {
            hit(&mut a, kind, at);
            hit(&mut b, kind, at);
        }
        assert_eq!(a.grid.cells, b.grid.cells);
        assert!(a.intact_fraction() < 1.0);
    }

    #[test]
    fn bullets_leave_studs_and_frame() {
        let mut s = wall();
        // On the middle stud: the plaster goes, the stud stays.
        hit(&mut s, ImpactKind::Bullet, Vec3::new(1.0, 1.5, 0.07));
        assert_eq!(cell(&s, 1.0, 1.5) & (SKIN | CORE), CORE);
        // On the lip: nothing happens.
        let before = cell(&s, 0.8, 0.03);
        hit(&mut s, ImpactKind::Bullet, Vec3::new(0.8, 0.03, 0.07));
        assert_eq!(cell(&s, 0.8, 0.03), before);
    }

    #[test]
    fn melee_makes_a_crouch_hole_on_the_third_hit() {
        let mut s = wall();
        let spot = Vec3::new(0.64, 1.3, 0.07);
        hit(&mut s, ImpactKind::Melee, spot);
        assert_ne!(cell(&s, 0.64, 0.4) & SKIN, 0, "one hit doesn't reach the floor");
        hit(&mut s, ImpactKind::Melee, spot);
        hit(&mut s, ImpactKind::Melee, spot);
        for v in [0.2, 0.6, 1.0] {
            assert_eq!(cell(&s, 0.64, v) & SKIN, 0, "open at {v}");
        }
        // The stud survives melee.
        assert_ne!(cell(&s, 1.0, 1.0) & CORE, 0);
    }

    #[test]
    fn low_blast_opens_a_walk_through_hole() {
        let mut s = wall();
        let t = tuning();
        let r = t.breach_charge.breach_radius;
        let h = t.breach_charge.wall_height;
        hit(&mut s, ImpactKind::Explosion { radius: r }, Vec3::new(1.0, h, 0.1));
        // Clear from just above the lip to head height, studs included.
        for v in [0.1, 0.5, 1.0, 1.5, 1.85] {
            assert_eq!(cell(&s, 1.0, v) & (SKIN | CORE), 0, "open at {v}");
            assert_eq!(cell(&s, 0.7, v) & (SKIN | CORE), 0, "open at {v}");
        }
        assert_ne!(cell(&s, 1.0, 0.03) & FRAME, 0, "lip stays");
    }

    #[test]
    fn hatch_breaks_on_the_third_melee_from_above() {
        let t = tuning();
        let (spec, grid) = layout_for("hatch_cover", &t).unwrap();
        let mut s = SoftSurface::new(spec, grid, 7);
        let top = Vec3::new(1.0, 0.0, 1.0);
        for i in 1..=3 {
            let o = cut(&mut s, ImpactKind::Melee, top, true, &t);
            s.impacts += 1;
            assert_eq!(o.broke, i == t.hatch_melee_hits);
        }
        assert!(s.broken && s.grid.count(SKIN | CORE) == 0);
        // From below, melee does nothing.
        let (spec, grid) = layout_for("hatch_cover", &t).unwrap();
        let mut s = SoftSurface::new(spec, grid, 7);
        for _ in 0..5 {
            cut(&mut s, ImpactKind::Melee, top, false, &t);
        }
        assert!(!s.broken);
    }

    #[test]
    fn barricade_clears_on_the_second_melee() {
        let t = tuning();
        let (spec, grid) = layout_for("barricade_door", &t).unwrap();
        let mut s = SoftSurface::new(spec, grid, 3);
        let o = cut(&mut s, ImpactKind::Melee, Vec3::new(0.0, 1.2, 0.03), false, &t);
        assert!(!o.broke && !o.removed.is_empty());
        let o = cut(&mut s, ImpactKind::Melee, Vec3::new(0.0, 1.2, 0.03), false, &t);
        assert!(o.broke && s.broken);
        assert!(s.spec.fixtures.iter().all(|f| !f.present));
    }
}
