//! The cell grid behind every soft surface: pure data, no ECS.
//!
//! A surface is a flat panel cut into roughly 10 cm cells. Each cell holds a
//! few flags saying which layers of material are still there. Holes are made
//! by clearing flags; the mesh and collider are rebuilt from the flags.
//!
//! Grid lines don't have to be evenly spaced: openings, studs and the frame
//! get lines exactly at their edges, and the rest are filled in at the cell
//! size. Coordinates are in the surface's own (u, v) plane, in metres.

use bevy::math::Vec2;

/// Plaster (walls) or boards (floors, hatches, barricades): the full
/// thickness of the surface. Bullets, pellets and melee remove it.
pub const SKIN: u8 = 1 << 0;
/// Timber inside the surface (studs, joists, battens). Survives bullets and
/// melee; only explosions (and hard breaches) remove it.
pub const CORE: u8 = 1 << 1;
/// Never removed except by a hard breach (outer frame strips, bottom lip).
pub const FRAME: u8 = 1 << 2;
/// Metal reinforcement plate on one side.
pub const PLATE: u8 = 1 << 3;
/// Thicker metal brace on top of the plate.
pub const BRACE: u8 = 1 << 4;

#[derive(Clone, Debug)]
pub struct CellGrid {
    /// Column edges (u), increasing; `nu() + 1` values.
    pub us: Vec<f32>,
    /// Row edges (v), increasing; `nv() + 1` values.
    pub vs: Vec<f32>,
    /// Flags per cell, row by row: index `v * nu + u`.
    pub cells: Vec<u8>,
}

/// Cell rectangle, `u0..u1` x `v0..v1` (end-exclusive).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellRect {
    pub u0: usize,
    pub v0: usize,
    pub u1: usize,
    pub v1: usize,
}

impl CellGrid {
    pub fn new(us: Vec<f32>, vs: Vec<f32>, mut init: impl FnMut(Vec2, usize, usize) -> u8) -> Self {
        let (nu, nv) = (us.len() - 1, vs.len() - 1);
        let mut cells = Vec::with_capacity(nu * nv);
        for v in 0..nv {
            for u in 0..nu {
                let centre = Vec2::new((us[u] + us[u + 1]) / 2.0, (vs[v] + vs[v + 1]) / 2.0);
                cells.push(init(centre, u, v));
            }
        }
        Self { us, vs, cells }
    }

    pub fn nu(&self) -> usize {
        self.us.len() - 1
    }

    pub fn nv(&self) -> usize {
        self.vs.len() - 1
    }

    pub fn index(&self, u: usize, v: usize) -> usize {
        v * self.nu() + u
    }

    /// Flags of a cell; 0 outside the grid.
    pub fn get(&self, u: isize, v: isize) -> u8 {
        if u < 0 || v < 0 || u as usize >= self.nu() || v as usize >= self.nv() {
            return 0;
        }
        self.cells[self.index(u as usize, v as usize)]
    }

    pub fn centre(&self, u: usize, v: usize) -> Vec2 {
        Vec2::new(
            (self.us[u] + self.us[u + 1]) / 2.0,
            (self.vs[v] + self.vs[v + 1]) / 2.0,
        )
    }

    pub fn size(&self) -> Vec2 {
        Vec2::new(
            self.us[self.nu()] - self.us[0],
            self.vs[self.nv()] - self.vs[0],
        )
    }

    pub fn min(&self) -> Vec2 {
        Vec2::new(self.us[0], self.vs[0])
    }

    pub fn max(&self) -> Vec2 {
        Vec2::new(self.us[self.nu()], self.vs[self.nv()])
    }

    /// The cell containing `p`, clamped onto the grid.
    pub fn cell_at(&self, p: Vec2) -> (usize, usize) {
        (column_of(&self.us, p.x), column_of(&self.vs, p.y))
    }

    /// Range of cells whose extent overlaps `min..max` (end-exclusive).
    pub fn cells_overlapping(&self, min: Vec2, max: Vec2) -> CellRect {
        CellRect {
            u0: column_of(&self.us, min.x),
            v0: column_of(&self.vs, min.y),
            u1: column_of(&self.us, max.x) + 1,
            v1: column_of(&self.vs, max.y) + 1,
        }
    }

    pub fn count(&self, mask: u8) -> usize {
        self.cells.iter().filter(|&&c| c & mask != 0).count()
    }

    /// Clears `clear` from every cell in `rect` for which `pick` is true,
    /// skipping cells with any `protect` flag. Returns the changed cells.
    pub fn clear_where(
        &mut self,
        rect: CellRect,
        clear: u8,
        protect: u8,
        mut pick: impl FnMut(usize, usize, Vec2) -> bool,
        changed: &mut Vec<usize>,
    ) {
        for v in rect.v0..rect.v1.min(self.nv()) {
            for u in rect.u0..rect.u1.min(self.nu()) {
                let i = self.index(u, v);
                let flags = self.cells[i];
                if flags & clear == 0 || flags & protect != 0 {
                    continue;
                }
                if pick(u, v, self.centre(u, v)) {
                    self.cells[i] &= !clear;
                    changed.push(i);
                }
            }
        }
    }

    /// Sets `flags` on every cell for which `pick` is true.
    pub fn set_where(&mut self, flags: u8, mut pick: impl FnMut(usize, usize, Vec2) -> bool) {
        for v in 0..self.nv() {
            for u in 0..self.nu() {
                if pick(u, v, self.centre(u, v)) {
                    let i = self.index(u, v);
                    self.cells[i] |= flags;
                }
            }
        }
    }

    /// Merges cells with the same non-zero `key` into as few rectangles as
    /// possible (greedy: grow along u, then along v).
    pub fn greedy(&self, key: impl Fn(u8) -> u8) -> Vec<(u8, CellRect)> {
        let (nu, nv) = (self.nu(), self.nv());
        let mut done = vec![false; nu * nv];
        let mut out = Vec::new();
        for v in 0..nv {
            let mut u = 0;
            while u < nu {
                let i = v * nu + u;
                let k = key(self.cells[i]);
                if done[i] || k == 0 {
                    u += 1;
                    continue;
                }
                let same = |x: usize, y: usize, done: &[bool]| {
                    let j = y * nu + x;
                    !done[j] && key(self.cells[j]) == k
                };
                let mut u1 = u + 1;
                while u1 < nu && same(u1, v, &done) {
                    u1 += 1;
                }
                let mut v1 = v + 1;
                'grow: while v1 < nv {
                    for x in u..u1 {
                        if !same(x, v1, &done) {
                            break 'grow;
                        }
                    }
                    v1 += 1;
                }
                for y in v..v1 {
                    for x in u..u1 {
                        done[y * nu + x] = true;
                    }
                }
                out.push((k, CellRect { u0: u, v0: v, u1, v1 }));
                u = u1;
            }
        }
        out
    }
}

/// Index of the interval of `edges` containing `x`, clamped.
fn column_of(edges: &[f32], x: f32) -> usize {
    let n = edges.len() - 1;
    // First edge strictly greater than x, minus one.
    let i = edges.partition_point(|&e| e <= x);
    i.saturating_sub(1).min(n - 1)
}

/// Grid lines from `min` to `max`: the ends, every `required` line inside,
/// and regular lines every `cell` metres (aligned to multiples of `cell`)
/// unless they would make a sliver next to a required line.
pub fn edges(min: f32, max: f32, cell: f32, required: &[f32]) -> Vec<f32> {
    const EPS: f32 = 1e-4;
    let mut fixed: Vec<f32> = required
        .iter()
        .copied()
        .filter(|&x| x > min + EPS && x < max - EPS)
        .chain([min, max])
        .collect();
    fixed.sort_by(f32::total_cmp);
    fixed.dedup_by(|a, b| (*a - *b).abs() < EPS);
    let mut out = fixed.clone();
    let mut x = (min / cell).ceil() * cell;
    while x < max - EPS {
        if fixed.iter().all(|&f| (f - x).abs() >= cell * 0.45) {
            out.push(x);
        }
        x += cell;
    }
    out.sort_by(f32::total_cmp);
    out
}

/// Small deterministic hash (for per-impact randomness that every machine
/// computes identically).
pub fn hash(a: u32, b: u32, c: u32) -> u32 {
    let mut h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77) ^ c.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    h
}

/// Hash -> 0..1.
pub fn unit(h: u32) -> f32 {
    (h >> 8) as f32 / (1u32 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_keep_required_lines_and_avoid_slivers() {
        let e = edges(0.25, 1.75, 0.1, &[0.3, 0.97, 1.03, 1.7]);
        assert_eq!(e.first(), Some(&0.25));
        assert_eq!(e.last(), Some(&1.75));
        for r in [0.3, 0.97, 1.03, 1.7] {
            assert!(e.iter().any(|&x| (x - r).abs() < 1e-4), "missing {r}");
        }
        for w in e.windows(2) {
            assert!(w[1] - w[0] > 0.04, "sliver {w:?}");
        }
    }

    #[test]
    fn greedy_covers_every_filled_cell_once() {
        let us = edges(0.0, 1.0, 0.1, &[]);
        let vs = edges(0.0, 1.0, 0.1, &[]);
        let mut grid = CellGrid::new(us, vs, |_, _, _| SKIN);
        let mut changed = Vec::new();
        let all = CellRect { u0: 0, v0: 0, u1: 10, v1: 10 };
        grid.clear_where(all, SKIN, 0, |_, _, c| c.distance(Vec2::splat(0.5)) < 0.25, &mut changed);
        let rects = grid.greedy(|c| c & SKIN);
        let covered: usize = rects.iter().map(|(_, r)| (r.u1 - r.u0) * (r.v1 - r.v0)).sum();
        assert_eq!(covered, grid.count(SKIN));
        assert_eq!(covered + changed.len(), 100);
    }

    #[test]
    fn cell_at_clamps() {
        let grid = CellGrid::new(vec![0.0, 0.5, 1.0], vec![0.0, 1.0], |_, _, _| SKIN);
        assert_eq!(grid.cell_at(Vec2::new(-1.0, 5.0)), (0, 0));
        assert_eq!(grid.cell_at(Vec2::new(0.7, 0.2)), (1, 0));
        assert_eq!(grid.cell_at(Vec2::new(2.0, 0.2)), (1, 0));
    }
}
