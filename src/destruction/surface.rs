//! Soft surfaces: the [`SoftSurface`] component and the layouts that turn
//! fortress-kit pieces into cell grids.
//!
//! Kit dimensions come from `tools/blender/kit/pieces.py` (walls 3.0 m high
//! and 0.15 m thick between posts at x = 0.25 .. length - 0.25; floor tiles
//! 2 x 2 m with their top at y = 0 and 0.2 m deep; the hatch opening is
//! 0.55 .. 1.45 with a 4 cm metal frame; barricade planks 18 cm with 8 cm
//! gaps). If those change, change them here too.

use bevy::prelude::*;

use super::grid::{BRACE, CORE, CellGrid, FRAME, PLATE, SKIN, edges, hash};
use crate::config::DestructionTuning;

/// Kit constants (see `pieces.py`).
const WALL_HEIGHT: f32 = 3.0;
const SOFT_WALL_THICKNESS: f32 = 0.15;
const POST_HALF: f32 = 0.25;
const SLAB: f32 = 0.2;
const HATCH_OPEN: (f32, f32) = (0.55, 1.45);
const HATCH_FRAME: f32 = 0.04;
/// Timber ring around the hatch opening that only a hard breach removes.
const HATCH_RING: f32 = 0.1;
const HATCH_COVER_DEPTH: f32 = 0.06;
const PLANK: f32 = 0.18;
const PLANK_GAP: f32 = 0.08;
const PLANK_DEPTH: f32 = 0.05;

/// What kind of soft surface this is; decides how impacts affect it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceRole {
    /// Plaster-on-timber partition wall.
    Wall,
    /// Wooden floor / ceiling.
    Floor,
    /// Hatch cover: holes from bullets, breaks open from repeated melee from
    /// above or any explosion.
    Hatch,
    /// Planks across a door or window: breaks progressively, clears on two
    /// melee hits or an explosion.
    Barricade,
}

/// Library material a part of a surface is drawn with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SurfaceMaterial {
    Plaster,
    Wood,
    Metal,
}

impl SurfaceMaterial {
    pub const ALL: [SurfaceMaterial; 3] = [Self::Plaster, Self::Wood, Self::Metal];

    pub fn library_name(self) -> &'static str {
        match self {
            Self::Plaster => "plaster",
            Self::Wood => "wood",
            Self::Metal => "metal",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// How the grid's (u, v, w) map onto the piece's local axes. `w` is depth
/// (through the surface); +w is the "front".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plane {
    /// u = x, v = y, w = z.
    Wall,
    /// u = x, v = z, w = y (front = top).
    Floor,
}

impl Plane {
    pub fn to_local(self, u: f32, v: f32, w: f32) -> Vec3 {
        match self {
            Plane::Wall => Vec3::new(u, v, w),
            Plane::Floor => Vec3::new(u, w, v),
        }
    }

    /// Local point -> ((u, v), w).
    pub fn from_local(self, p: Vec3) -> (Vec2, f32) {
        match self {
            Plane::Wall => (Vec2::new(p.x, p.y), p.z),
            Plane::Floor => (Vec2::new(p.x, p.z), p.y),
        }
    }

    /// Local direction of +w.
    pub fn normal(self) -> Vec3 {
        self.to_local(0.0, 0.0, 1.0)
    }

    /// Size along local axes of a box spanning du, dv, dw.
    pub fn size(self, du: f32, dv: f32, dw: f32) -> Vec3 {
        self.to_local(du, dv, dw)
    }
}

/// A fixed box that belongs to a surface (hatch frame, barricade battens).
#[derive(Clone, Debug)]
pub struct Fixture {
    /// Local-space corners.
    pub min: Vec3,
    pub max: Vec3,
    pub material: SurfaceMaterial,
    /// Removed when the surface breaks (barricade battens).
    pub breakable: bool,
    pub present: bool,
}

/// Static description of a surface: its shape, materials and rules.
#[derive(Clone, Debug)]
pub struct SurfaceSpec {
    pub role: SurfaceRole,
    pub plane: Plane,
    /// Depth range (w) of the skin.
    pub skin_depth: (f32, f32),
    /// Depth range (w) of the timber core.
    pub core_depth: (f32, f32),
    pub skin_material: SurfaceMaterial,
    /// Draw the faces around the outside of the grid (for free-standing
    /// surfaces like barricades; walls and floors abut other pieces).
    pub closed_borders: bool,
    /// Defenders can reinforce it.
    pub reinforceable: bool,
    /// Height of the bottom lip (top of the first row), walls only.
    pub lip: f32,
    pub fixtures: Vec<Fixture>,
}

/// Reinforcement installed on a surface.
#[derive(Clone, Copy, Debug)]
pub struct Reinforcement {
    /// Side the plate is on: +1 = +w (front), -1 = -w.
    pub side: f32,
    /// Who installed it.
    pub by: Option<Entity>,
}

/// A spot on a surface's face where a gadget can sit (see
/// [`SoftSurface::mount`]).
#[derive(Clone, Copy, Debug)]
pub struct SurfaceMount {
    /// On the face, world space.
    pub point: Vec3,
    /// Out of the face, towards whoever placed it.
    pub normal: Vec3,
    pub reinforced: bool,
}

/// Put this on a soft kit piece's root (when spawning it) to stop
/// defenders reinforcing it. Door and window walls are never reinforceable.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct NotReinforceable;

/// A melee hole in progress: hits that land close together grow one hole.
#[derive(Clone, Copy, Debug)]
pub struct MeleeMark {
    pub centre: Vec2,
    pub hits: u32,
}

/// Destruction state of one soft kit piece. Lives on the piece's root entity
/// (the static rigid body, which also carries [`SurfaceKind`]).
///
/// Everything that changes during a round is in here, and it only changes in
/// response to impacts, explosions, hard breaches and reinforcement, so it
/// can be replicated or replayed.
///
/// [`SurfaceKind`]: crate::combat::SurfaceKind
#[derive(Component, Clone, Debug)]
pub struct SoftSurface {
    pub spec: SurfaceSpec,
    pub grid: CellGrid,
    pub reinforced: Option<Reinforcement>,
    /// Hatch broken open / barricade cleared.
    pub broken: bool,
    /// Melee holes in progress (walls).
    pub melee_marks: Vec<MeleeMark>,
    /// Melee hits taken (hatches from above, barricades).
    pub melee_hits: u32,
    /// Impacts applied so far; feeds the deterministic randomness.
    pub impacts: u32,
    /// Per-piece random seed, from its placement.
    pub seed: u32,
    /// Bumped on every change; meshes and colliders rebuild when it moves.
    pub revision: u32,
    /// Cells in the intact surface (for "how damaged is it").
    pub initial_cells: usize,
}

impl SoftSurface {
    pub fn new(spec: SurfaceSpec, grid: CellGrid, seed: u32) -> Self {
        let initial_cells = grid.count(SKIN);
        Self {
            spec,
            grid,
            reinforced: None,
            broken: false,
            melee_marks: Vec::new(),
            melee_hits: 0,
            impacts: 0,
            seed,
            revision: 1,
            initial_cells,
        }
    }

    pub fn role(&self) -> SurfaceRole {
        self.spec.role
    }

    pub fn is_reinforced(&self) -> bool {
        self.reinforced.is_some()
    }

    /// Can be reinforced right now (reinforceable kind, not already).
    pub fn can_reinforce(&self) -> bool {
        self.spec.reinforceable && self.reinforced.is_none()
    }

    /// Fraction of the original skin still present.
    pub fn intact_fraction(&self) -> f32 {
        if self.initial_cells == 0 {
            return 0.0;
        }
        self.grid.count(SKIN) as f32 / self.initial_cells as f32
    }

    /// Where an aim ray from `eye` along `dir` crosses this surface's middle
    /// plane within `max` metres, if that's on the panel (holes included).
    pub fn aim_point(&self, transform: &GlobalTransform, eye: Vec3, dir: Vec3, max: f32) -> Option<Vec3> {
        let plane = self.spec.plane;
        let (w0, w1) = self.spec.skin_depth;
        let normal = (transform.rotation() * plane.normal()).normalize();
        let origin = transform.transform_point(plane.to_local(0.0, 0.0, (w0 + w1) / 2.0));
        let denom = dir.dot(normal);
        if denom.abs() < 0.15 {
            return None;
        }
        let t = (origin - eye).dot(normal) / denom;
        if !(0.0..=max).contains(&t) {
            return None;
        }
        let point = eye + dir * t;
        let local = transform.affine().inverse().transform_point3(point);
        let (uv, _) = plane.from_local(local);
        let (min, max) = (self.grid.min(), self.grid.max());
        (uv.cmpge(min).all() && uv.cmple(max).all()).then_some(point)
    }

    /// Where a gadget aimed from `eye` along `dir` would sit on this
    /// surface (on the face towards `eye`), within `reach`. Holes count as
    /// part of the surface, so this works on damaged walls too.
    pub fn mount(&self, transform: &GlobalTransform, eye: Vec3, dir: Vec3, reach: f32) -> Option<SurfaceMount> {
        let point = self.aim_point(transform, eye, dir, reach)?;
        let side = self.side_of(transform, eye);
        let normal = (transform.rotation() * self.spec.plane.normal() * side).normalize();
        let (w0, w1) = self.spec.skin_depth;
        let face = if side > 0.0 { w1 } else { w0 };
        let local = transform.affine().inverse().transform_point3(point);
        let (uv, _) = self.spec.plane.from_local(local);
        let on_face = transform.transform_point(self.spec.plane.to_local(uv.x, uv.y, face));
        Some(SurfaceMount {
            point: on_face,
            normal,
            reinforced: self.is_reinforced(),
        })
    }

    /// Which side of the surface `point` (world space) is on: +1 or -1.
    pub fn side_of(&self, transform: &GlobalTransform, point: Vec3) -> f32 {
        let local = transform.affine().inverse().transform_point3(point);
        let (_, w) = self.spec.plane.from_local(local);
        if w >= 0.0 { 1.0 } else { -1.0 }
    }

    /// Depth range of the reinforcement plate (and brace) on `side`.
    pub fn plate_depth(&self, side: f32, tuning: &DestructionTuning) -> ((f32, f32), (f32, f32)) {
        let (w0, w1) = self.spec.skin_depth;
        let gap = 0.004;
        let (t, b) = (tuning.reinforce.plate_thickness, tuning.reinforce.brace_thickness);
        if side >= 0.0 {
            ((w1 + gap, w1 + gap + t), (w1 + gap + t, w1 + gap + t + b))
        } else {
            ((w0 - gap - t, w0 - gap), (w0 - gap - t - b, w0 - gap - t))
        }
    }

    /// Installs the reinforcement plate on `side`. Plates cover the whole
    /// panel (existing holes included); braces run along the frame and the
    /// studs, and across at two heights.
    pub fn reinforce(&mut self, side: f32, by: Option<Entity>) {
        let side = if side >= 0.0 { 1.0 } else { -1.0 };
        self.reinforced = Some(Reinforcement { side, by });
        let grid = &mut self.grid;
        let (min, max) = (grid.min(), grid.max());
        let size = max - min;
        // Braces: along the studs / battens (columns that are mostly core),
        // around the edges, and two horizontal bars.
        let core_columns: Vec<bool> = (0..grid.nu())
            .map(|u| {
                let core = (0..grid.nv())
                    .filter(|&v| grid.cells[grid.index(u, v)] & CORE != 0)
                    .count();
                core * 2 > grid.nv()
            })
            .collect();
        let bars = [min.y + size.y * 0.3, min.y + size.y * 0.72];
        let bar_half = 0.04;
        grid.set_where(PLATE, |_, _, _| true);
        grid.set_where(BRACE, |u, _, c| {
            core_columns[u]
                || c.x - min.x < 0.08
                || max.x - c.x < 0.08
                || bars.iter().any(|&b| (c.y - b).abs() < bar_half + 0.01)
        });
        self.revision += 1;
    }
}

/// Builds the surface for kit piece `name`, or `None` if it isn't one we
/// make destructible. `seed` is per placement.
pub fn layout_for(name: &str, t: &DestructionTuning) -> Option<(SurfaceSpec, CellGrid)> {
    Some(match name {
        "soft_wall_1m" => wall(1.0, &[], t),
        "soft_wall_2m" => wall(2.0, &[], t),
        "soft_wall_door_2m" => wall(2.0, &[(Vec2::new(0.5, 0.0), Vec2::new(1.5, 2.1))], t),
        "soft_wall_window_2m" => wall(2.0, &[(Vec2::new(0.5, 1.0), Vec2::new(1.5, 2.1))], t),
        "floor_wood_2m" => floor(false, t),
        "floor_hatch_2m" => floor(true, t),
        "hatch_cover" => hatch_cover(t),
        "barricade_door" => barricade(1.1, 0.0, 2.2, t),
        "barricade_window" => barricade(1.0, 1.0, 1.1, t),
        _ => return None,
    })
}

/// Seed from where a piece is placed (centimetres), so it's the same on
/// every machine.
pub fn placement_seed(transform: &Transform) -> u32 {
    let p = (transform.translation * 100.0).round();
    let (_, yaw, _) = transform.rotation.to_euler(EulerRot::YXZ);
    hash(p.x as i32 as u32, p.y as i32 as u32, (p.z as i32 as u32) ^ ((yaw * 100.0) as i32 as u32))
}

/// Centres of studs across `lo..hi`: one just outside each opening edge,
/// then evenly spaced so no bay is wider than `max_bay`.
fn stud_centres(lo: f32, hi: f32, forced: &[f32], width: f32, max_bay: f32) -> Vec<f32> {
    let mut barriers: Vec<f32> = forced.to_vec();
    barriers.sort_by(f32::total_cmp);
    let mut studs = barriers.clone();
    // Spans between barriers (stud edges or the frame).
    let mut points = vec![(lo, lo)];
    points.extend(barriers.iter().map(|&s| (s - width / 2.0, s + width / 2.0)));
    points.push((hi, hi));
    for pair in points.windows(2) {
        let (start, end) = (pair[0].1, pair[1].0);
        let span = end - start;
        if span <= max_bay {
            continue;
        }
        // n bays need n - 1 studs; each stud also takes its width.
        let n = ((span + width) / (max_bay + width)).ceil() as usize;
        let step = (span + width) / n as f32;
        for k in 1..n {
            studs.push(start - width / 2.0 + step * k as f32);
        }
    }
    studs.sort_by(f32::total_cmp);
    studs
}

/// Plaster wall `length` m long with rectangular `openings` (min, max) in
/// (u, v). Openings are cells that are simply never there.
fn wall(length: f32, openings: &[(Vec2, Vec2)], t: &DestructionTuning) -> (SurfaceSpec, CellGrid) {
    let w = &t.walls;
    let (u0, u1) = (POST_HALF, length - POST_HALF);
    let (in0, in1) = (u0 + w.frame, u1 - w.frame);
    let half = SOFT_WALL_THICKNESS / 2.0;

    let forced: Vec<f32> = openings
        .iter()
        .flat_map(|(min, max)| [min.x - w.stud_width / 2.0, max.x + w.stud_width / 2.0])
        .filter(|&s| s > in0 + w.stud_width / 2.0 && s < in1 - w.stud_width / 2.0)
        .collect();
    let studs = stud_centres(in0, in1, &forced, w.stud_width, w.stud_max_bay);

    let mut u_lines = vec![in0, in1];
    let mut v_lines = vec![w.lip, WALL_HEIGHT - w.top_plate];
    for s in &studs {
        u_lines.extend([s - w.stud_width / 2.0, s + w.stud_width / 2.0]);
    }
    for (min, max) in openings {
        u_lines.extend([min.x, max.x]);
        v_lines.extend([min.y, max.y]);
    }
    let us = edges(u0, u1, t.cell_size, &u_lines);
    let vs = edges(0.0, WALL_HEIGHT, t.cell_size, &v_lines);

    let inside = |c: Vec2, (min, max): &(Vec2, Vec2)| c.cmpgt(*min).all() && c.cmplt(*max).all();
    let grid = CellGrid::new(us, vs, |c, _, _| {
        if openings.iter().any(|o| inside(c, o)) {
            return 0;
        }
        let mut flags = SKIN;
        if studs.iter().any(|s| (c.x - s).abs() < w.stud_width / 2.0) {
            flags |= CORE;
        }
        if c.x < in0 || c.x > in1 || c.y < w.lip || c.y > WALL_HEIGHT - w.top_plate {
            flags |= FRAME;
        }
        flags
    });
    let spec = SurfaceSpec {
        role: SurfaceRole::Wall,
        plane: Plane::Wall,
        skin_depth: (-half, half),
        core_depth: (-half + 0.02, half - 0.02),
        skin_material: SurfaceMaterial::Plaster,
        closed_borders: false,
        reinforceable: openings.is_empty(),
        lip: w.lip,
        fixtures: Vec::new(),
    };
    (spec, grid)
}

/// 2 x 2 m wooden floor tile (top at y = 0), optionally with the hatch
/// opening and its metal frame.
fn floor(hatch: bool, t: &DestructionTuning) -> (SurfaceSpec, CellGrid) {
    let f = &t.floors;
    let jw = f.joist_width;
    let (a, b) = HATCH_OPEN;
    // Joists run along z; edge joists so holes can't join across tiles.
    let mut forced = vec![];
    if hatch {
        forced.extend([a - jw / 2.0, b + jw / 2.0]);
    }
    let mut joists = stud_centres(jw, 2.0 - jw, &forced, jw, f.joist_max_bay);
    joists.extend([jw / 2.0, 2.0 - jw / 2.0]);

    // Around a hatch, a timber ring and the two trimmer joists beside it
    // are frame: blasts can't leave the hatch hanging in mid-air.
    let ring = HATCH_RING;
    let trimmers = [a - jw / 2.0, b + jw / 2.0];
    let mut u_lines: Vec<f32> = joists.iter().flat_map(|j| [j - jw / 2.0, j + jw / 2.0]).collect();
    let mut v_lines = vec![];
    if hatch {
        u_lines.extend([a - ring, a, b, b + ring]);
        v_lines.extend([a - ring, a, b, b + ring]);
    }
    let us = edges(0.0, 2.0, t.cell_size, &u_lines);
    let vs = edges(0.0, 2.0, t.cell_size, &v_lines);
    let inside = |c: Vec2, lo: f32, hi: f32| c.x > lo && c.x < hi && c.y > lo && c.y < hi;
    let grid = CellGrid::new(us, vs, |c, _, _| {
        if hatch && inside(c, a, b) {
            return 0;
        }
        let mut flags = SKIN;
        if joists.iter().any(|j| (c.x - j).abs() < jw / 2.0) {
            flags |= CORE;
        }
        if hatch
            && (inside(c, a - ring, b + ring) || trimmers.iter().any(|j| (c.x - j).abs() < jw / 2.0))
        {
            flags |= CORE | FRAME;
        }
        flags
    });
    let mut fixtures = Vec::new();
    if hatch {
        let fr = HATCH_FRAME;
        let y0 = -SLAB;
        for (min, max) in [
            (Vec3::new(a, y0, a), Vec3::new(b, 0.0, a + fr)),
            (Vec3::new(a, y0, b - fr), Vec3::new(b, 0.0, b)),
            (Vec3::new(a, y0, a + fr), Vec3::new(a + fr, 0.0, b - fr)),
            (Vec3::new(b - fr, y0, a + fr), Vec3::new(b, 0.0, b - fr)),
        ] {
            fixtures.push(Fixture {
                min,
                max,
                material: SurfaceMaterial::Metal,
                breakable: false,
                present: true,
            });
        }
    }
    let spec = SurfaceSpec {
        role: SurfaceRole::Floor,
        plane: Plane::Floor,
        skin_depth: (-SLAB, 0.0),
        core_depth: (-SLAB + 0.01, -0.01),
        skin_material: SurfaceMaterial::Wood,
        closed_borders: false,
        reinforceable: false,
        lip: 0.0,
        fixtures,
    };
    (spec, grid)
}

/// The boards closing a hatch, with two battens underneath in a cross.
fn hatch_cover(t: &DestructionTuning) -> (SurfaceSpec, CellGrid) {
    let (a, b) = (HATCH_OPEN.0 + HATCH_FRAME, HATCH_OPEN.1 - HATCH_FRAME);
    let mid = (a + b) / 2.0;
    let bw = t.floors.joist_width;
    let lines = [mid - bw / 2.0, mid + bw / 2.0];
    let us = edges(a, b, t.cell_size, &lines);
    let vs = edges(a, b, t.cell_size, &lines);
    let grid = CellGrid::new(us, vs, |c, _, _| {
        if (c.x - mid).abs() < bw / 2.0 || (c.y - mid).abs() < bw / 2.0 {
            SKIN | CORE
        } else {
            SKIN
        }
    });
    let spec = SurfaceSpec {
        role: SurfaceRole::Hatch,
        plane: Plane::Floor,
        skin_depth: (-HATCH_COVER_DEPTH, 0.0),
        core_depth: (-HATCH_COVER_DEPTH + 0.005, -0.005),
        skin_material: SurfaceMaterial::Wood,
        closed_borders: true,
        reinforceable: true,
        lip: 0.0,
        fixtures: Vec::new(),
    };
    (spec, grid)
}

/// Planks across an opening `width` wide from `bottom` to `bottom + height`,
/// centred on x = 0 (same layout as the kit's barricade pieces).
fn barricade(width: f32, bottom: f32, height: f32, t: &DestructionTuning) -> (SurfaceSpec, CellGrid) {
    let half = width / 2.0 + 0.05;
    let columns = ((2.0 * half) / t.barricade.segment).round().max(1.0) as usize;
    let us: Vec<f32> = (0..=columns)
        .map(|i| -half + 2.0 * half * i as f32 / columns as f32)
        .collect();
    let mut vs = vec![];
    let mut y = bottom + 0.05;
    while y + PLANK <= bottom + height + 1e-4 {
        vs.extend([y, y + PLANK]);
        y += PLANK + PLANK_GAP;
    }
    // Plank rows are even rows; gaps (odd rows) are empty.
    let grid = CellGrid::new(dedup(us), vs, |_, _, v| if v % 2 == 0 { SKIN } else { 0 });
    let fixtures = [-width / 4.0, width / 4.0]
        .into_iter()
        .map(|x| Fixture {
            min: Vec3::new(x - 0.05, bottom, -0.065),
            max: Vec3::new(x + 0.05, bottom + height, -PLANK_DEPTH / 2.0),
            material: SurfaceMaterial::Wood,
            breakable: true,
            present: true,
        })
        .collect();
    let spec = SurfaceSpec {
        role: SurfaceRole::Barricade,
        plane: Plane::Wall,
        skin_depth: (-PLANK_DEPTH / 2.0, PLANK_DEPTH / 2.0),
        core_depth: (0.0, 0.0),
        skin_material: SurfaceMaterial::Wood,
        closed_borders: true,
        reinforceable: false,
        lip: 0.0,
        fixtures,
    };
    (spec, grid)
}

fn dedup(mut v: Vec<f32>) -> Vec<f32> {
    v.dedup_by(|a, b| (*a - *b).abs() < 1e-5);
    v
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::config::Tuning;

    pub(crate) fn tuning() -> DestructionTuning {
        let tuning: Tuning = ron::from_str(include_str!("../../assets/config/tuning.ron")).unwrap();
        tuning.destruction
    }

    /// Widest gap between core (or frame/opening) columns along u in row `v`.
    fn widest_bay(grid: &CellGrid, v: usize) -> f32 {
        let mut widest: f32 = 0.0;
        let mut start = grid.us[0];
        for u in 0..grid.nu() {
            let c = grid.cells[grid.index(u, v)];
            if c & (CORE | FRAME) != 0 || c == 0 {
                widest = widest.max(grid.us[u] - start);
                start = grid.us[u + 1];
            }
        }
        widest.max(grid.us[grid.nu()] - start)
    }

    #[test]
    fn plaster_wall_has_frame_lip_and_a_middle_stud() {
        let t = tuning();
        let (spec, grid) = layout_for("soft_wall_2m", &t).unwrap();
        assert!(spec.reinforceable);
        assert_eq!(grid.min(), Vec2::new(0.25, 0.0));
        assert_eq!(grid.max(), Vec2::new(1.75, 3.0));
        // Bottom row is the lip (frame), at the configured height.
        assert!((grid.vs[1] - t.walls.lip).abs() < 1e-4);
        assert!((0..grid.nu()).all(|u| grid.cells[grid.index(u, 0)] & FRAME != 0));
        // A stud in the middle at x = 1.0, and bays under the configured max.
        let (su, sv) = grid.cell_at(Vec2::new(1.0, 1.5));
        assert_ne!(grid.cells[grid.index(su, sv)] & CORE, 0);
        assert!(widest_bay(&grid, sv) <= t.walls.stud_max_bay + 1e-3);
        // A person (0.6 m) fits between the studs.
        assert!(widest_bay(&grid, sv) >= 0.6);
    }

    #[test]
    fn door_wall_has_an_opening_and_cannot_be_reinforced() {
        let t = tuning();
        let (spec, grid) = layout_for("soft_wall_door_2m", &t).unwrap();
        assert!(!spec.reinforceable);
        let (u, v) = grid.cell_at(Vec2::new(1.0, 1.0));
        assert_eq!(grid.cells[grid.index(u, v)], 0);
        let (u, v) = grid.cell_at(Vec2::new(1.0, 2.5));
        assert_ne!(grid.cells[grid.index(u, v)] & SKIN, 0);
    }

    #[test]
    fn floor_joists_are_too_close_to_drop_between() {
        let t = tuning();
        for name in ["floor_wood_2m", "floor_hatch_2m"] {
            let (_, grid) = layout_for(name, &t).unwrap();
            for v in 0..grid.nv() {
                assert!(widest_bay(&grid, v) < 0.6, "{name} row {v}: {}", widest_bay(&grid, v));
            }
        }
    }

    #[test]
    fn reinforcing_covers_every_cell_with_plate() {
        let t = tuning();
        let (spec, grid) = layout_for("soft_wall_2m", &t).unwrap();
        let mut surface = SoftSurface::new(spec, grid, 1);
        surface.reinforce(-1.0, None);
        assert!(surface.is_reinforced() && !surface.can_reinforce());
        assert!(surface.grid.cells.iter().all(|c| c & PLATE != 0));
        assert!(surface.grid.count(BRACE) > 0);
    }
}
