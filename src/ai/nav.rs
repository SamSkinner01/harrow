//! Navigation grid, generated from the level's colliders once the map has
//! loaded, and patched locally when destruction changes the level.
//!
//! **Grid.** Columns every `nav.cell` metres (0.4 by default) over the map's
//! bounds (every finite world collider plus the spawns, with a margin).
//! Each column holds up to [`LAYERS`] nodes, one per walkable floor level,
//! found by sweeping a small sphere straight down through the level (it
//! steps through floors, so every storey, stair ramp and roof is found; a
//! sphere rather than a ray so bullet holes don't punch gaps in floors).
//!
//! **Clearance.** A node is kept if an upright capsule of `agent_radius`
//! fits on it standing ([`STAND`]) or at least crouching ([`CROUCH`]),
//! lifted a little so small lips and stair ramps don't count. A spot that
//! is blocked only by an intact barricade is kept and flagged
//! [`BARRICADE`] (attackers can path through it and break it).
//!
//! **Links.** Each node links to the node in each of its 8 neighbouring
//! columns that it can walk to: the height difference is a step of at most
//! `max_step` on flat ground or follows the slope on ramps, and two
//! horizontal rays (knee and chest height) between them are clear (so thin
//! walls and railings split the grid). Where the neighbour column has no
//! node at this height but a floor up to `max_drop` below, and a sphere of
//! `drop_radius` falls through to it, a one-way **drop** link is added
//! (open hatches and blown floors). Soft walls get **breach** links across
//! them (both ways) for bots carrying breach charges.
//!
//! **Updates.** Destruction messages (big holes, broken hatches and
//! barricades, new or removed surfaces, a new round restoring everything)
//! mark the affected area dirty; it is re-sampled and re-linked a little at
//! a time (`update_budget_ms` per frame) and the grid's `revision` goes up,
//! so bots re-plan paths that crossed it.
//!
//! Generation only uses spatial queries against `GameLayer::World`, so it
//! works for any map built from colliders (no hand-placed nav data).

use std::time::{Duration, Instant};

use avian3d::prelude::*;
use bevy::{
    platform::collections::HashMap,
    prelude::*,
    world_serialization::{WorldAssetRoot, WorldInstance},
};

use super::AiSettings;
use crate::{
    config::Tuning,
    destruction::{SoftSurface, SurfaceBroken, SurfaceChanged, SurfaceRole},
    physics::GameLayer,
    round::{RoundReset, sites::KeepLayout},
};

/// Most floor levels in one column.
pub const LAYERS: usize = 4;
/// Index of a node: `column * LAYERS + layer`.
pub type NodeId = u32;

/// Node flags.
pub const STAND: u8 = 1;
pub const CROUCH: u8 = 2;
pub const SLOPE: u8 = 4;
pub const BARRICADE: u8 = 8;
/// Some of the 8 neighbours are not walk-linked (next to a wall or edge).
pub const EDGE: u8 = 16;
/// Next to a drop (the side of a stair ramp, a floor's edge): paths keep
/// off these where they can, so nobody gets nudged over.
pub const LEDGE: u8 = 32;

const NO_LINK: u8 = 0xFF;
const KIND_WALK: u8 = 0;
const KIND_DROP: u8 = 1;

/// Neighbour offsets (x, z), in link order. Opposite of `d` is `(d + 4) % 8`.
pub const DIRS: [IVec2; 8] = [
    IVec2::new(1, 0),
    IVec2::new(1, 1),
    IVec2::new(0, 1),
    IVec2::new(-1, 1),
    IVec2::new(-1, 0),
    IVec2::new(-1, -1),
    IVec2::new(0, -1),
    IVec2::new(1, -1),
];

/// Lift of the clearance capsule above the floor (lips, ramps).
const LIFT: f32 = 0.12;
/// Radius of the sphere swept down to find floors.
const PROBE_RADIUS: f32 = 0.1;
/// Heights of the two rays that check a link (stand / crouch-only).
const LINK_RAYS_STAND: [f32; 3] = [0.3, 1.0, 1.7];
const LINK_RAYS_CROUCH: [f32; 3] = [0.25, 0.6, 1.1];

/// One floor found in a column.
#[derive(Clone, Copy, Debug)]
struct Sample {
    y: f32,
    flags: u8,
    blocker: Option<Entity>,
    grad: Vec2,
}

#[derive(Clone, Copy, Debug)]
pub struct NavNode {
    /// Floor height.
    pub y: f32,
    pub flags: u8,
    /// Floor slope: height change per metre along x and z.
    pub grad: Vec2,
    /// Per direction: `NO_LINK` or `kind << 4 | layer` of the neighbour.
    links: [u8; 8],
    /// The barricade in the way, for [`BARRICADE`] nodes.
    pub blocker: Option<Entity>,
}

impl Default for NavNode {
    fn default() -> Self {
        Self {
            y: 0.0,
            flags: 0,
            grad: Vec2::ZERO,
            links: [NO_LINK; 8],
            blocker: None,
        }
    }
}

impl NavNode {
    pub fn standable(&self) -> bool {
        self.flags & STAND != 0
    }
}

/// How a path gets from one node to the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    Walk,
    /// Fall through a hole or open hatch (one way).
    Drop,
    /// Blow a hole in a soft wall with a breach charge first.
    Breach,
}

/// A way through a soft wall for someone with a breach charge.
#[derive(Clone, Copy, Debug)]
pub struct BreachLink {
    pub from: NodeId,
    pub to: NodeId,
    pub surface: Entity,
    /// Where to aim to stick the charge (on the `from` side's face).
    pub aim: Vec3,
}

/// The navigation grid. `ready` is false until the first build finishes.
#[derive(Resource, Default)]
pub struct NavGrid {
    pub ready: bool,
    /// World (x, z) of column (0, 0)'s centre.
    pub origin: Vec2,
    pub cell: f32,
    pub nx: i32,
    pub nz: i32,
    pub y_top: f32,
    pub y_bottom: f32,
    nodes: Vec<NavNode>,
    counts: Vec<u8>,
    pub breaches: Vec<BreachLink>,
    breach_index: HashMap<NodeId, Vec<usize>>,
    /// Goes up on every change.
    pub revision: u32,
    /// Recently changed areas: (revision, min, max) in world x/z.
    pub changes: Vec<(u32, Vec2, Vec2)>,
}

impl NavGrid {
    fn in_bounds(&self, c: IVec2) -> bool {
        c.x >= 0 && c.y >= 0 && c.x < self.nx && c.y < self.nz
    }

    fn column_index(&self, c: IVec2) -> usize {
        (c.y * self.nx + c.x) as usize
    }

    fn column_of(&self, id: NodeId) -> IVec2 {
        let col = id as i32 / LAYERS as i32;
        IVec2::new(col % self.nx, col / self.nx)
    }

    pub fn column_centre(&self, c: IVec2) -> Vec2 {
        self.origin + c.as_vec2() * self.cell
    }

    fn id(&self, c: IVec2, layer: usize) -> NodeId {
        (self.column_index(c) * LAYERS + layer) as NodeId
    }

    pub fn node(&self, id: NodeId) -> &NavNode {
        &self.nodes[id as usize]
    }

    pub fn exists(&self, id: NodeId) -> bool {
        let layer = id as usize % LAYERS;
        (id as usize) < self.nodes.len() && layer < self.counts[id as usize / LAYERS] as usize
    }

    /// World position of a node (on the floor).
    pub fn pos(&self, id: NodeId) -> Vec3 {
        let c = self.column_centre(self.column_of(id));
        Vec3::new(c.x, self.node(id).y, c.y)
    }

    /// Nodes of a column.
    fn layers(&self, c: IVec2) -> std::ops::Range<usize> {
        0..self.counts[self.column_index(c)] as usize
    }

    /// Outgoing links of a node: (neighbour, kind, length in metres).
    pub fn links(&self, id: NodeId) -> impl Iterator<Item = (NodeId, LinkKind, f32)> + '_ {
        let c = self.column_of(id);
        let node = *self.node(id);
        let grid = (0..8).filter_map(move |d| {
            let link = node.links[d];
            if link == NO_LINK {
                return None;
            }
            let n = c + DIRS[d];
            let to = self.id(n, (link & 0x0F) as usize);
            let kind = if link >> 4 == KIND_DROP { LinkKind::Drop } else { LinkKind::Walk };
            let length = if d % 2 == 1 { self.cell * std::f32::consts::SQRT_2 } else { self.cell };
            Some((to, kind, length))
        });
        let breaches = self
            .breach_index
            .get(&id)
            .into_iter()
            .flatten()
            .map(move |&i| {
                let b = &self.breaches[i];
                (b.to, LinkKind::Breach, self.pos(b.from).distance(self.pos(b.to)))
            });
        grid.chain(breaches)
    }

    /// Walk link from `id` in direction `d`, if any.
    pub fn walk_link(&self, id: NodeId, d: usize) -> Option<NodeId> {
        let link = self.node(id).links[d];
        (link != NO_LINK && link >> 4 == KIND_WALK)
            .then(|| self.id(self.column_of(id) + DIRS[d], (link & 0x0F) as usize))
    }

    pub fn breach_from(&self, from: NodeId, to: NodeId) -> Option<&BreachLink> {
        self.breach_index
            .get(&from)?
            .iter()
            .map(|&i| &self.breaches[i])
            .find(|b| b.to == to)
    }

    /// The node a character standing at `feet` is on: the highest floor
    /// at most a little above the feet in that column, else the closest
    /// node nearby (within `search` columns).
    pub fn locate(&self, feet: Vec3, search: i32) -> Option<NodeId> {
        if !self.ready {
            return None;
        }
        let c = ((Vec2::new(feet.x, feet.z) - self.origin) / self.cell).round().as_ivec2();
        if self.in_bounds(c) {
            let best = self
                .layers(c)
                .map(|l| (l, self.nodes[self.column_index(c) * LAYERS + l].y))
                .filter(|&(_, y)| y <= feet.y + 0.6 && y >= feet.y - 1.5)
                .max_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((layer, _)) = best {
                return Some(self.id(c, layer));
            }
        }
        let mut best: Option<(NodeId, f32)> = None;
        for dz in -search..=search {
            for dx in -search..=search {
                let n = c + IVec2::new(dx, dz);
                if !self.in_bounds(n) {
                    continue;
                }
                for layer in self.layers(n) {
                    let id = self.id(n, layer);
                    let p = self.pos(id);
                    if (p.y - feet.y).abs() > 1.2 {
                        continue;
                    }
                    let d = p.distance_squared(feet);
                    if best.is_none_or(|(_, bd)| d < bd) {
                        best = Some((id, d));
                    }
                }
            }
        }
        best.map(|(id, _)| id)
    }

    /// Every node in a column range (for searches around a point).
    pub fn nodes_near(&self, centre: Vec3, radius: f32, max_dy: f32) -> Vec<NodeId> {
        let mut out = Vec::new();
        if !self.ready {
            return out;
        }
        let r = (radius / self.cell).ceil() as i32;
        let c = ((Vec2::new(centre.x, centre.z) - self.origin) / self.cell).round().as_ivec2();
        for dz in -r..=r {
            for dx in -r..=r {
                let n = c + IVec2::new(dx, dz);
                if !self.in_bounds(n) {
                    continue;
                }
                for layer in self.layers(n) {
                    let id = self.id(n, layer);
                    let p = self.pos(id);
                    if (p.y - centre.y).abs() <= max_dy
                        && p.with_y(0.0).distance(centre.with_y(0.0)) <= radius
                    {
                        out.push(id);
                    }
                }
            }
        }
        out
    }

    /// One line per node of the column at `p`: height, flags, slope and
    /// links (for `--ai-navtest` dumps).
    pub fn describe(&self, p: Vec3) -> String {
        let c = ((Vec2::new(p.x, p.z) - self.origin) / self.cell).round().as_ivec2();
        if !self.in_bounds(c) {
            return "out of bounds".into();
        }
        let centre = self.column_centre(c);
        let mut out = format!("column ({:.1}, {:.1}):", centre.x, centre.y);
        for layer in self.layers(c) {
            let n = self.nodes[self.column_index(c) * LAYERS + layer];
            let links: Vec<String> = (0..8)
                .map(|d| match n.links[d] {
                    NO_LINK => "-".to_string(),
                    l if l >> 4 == KIND_DROP => format!("v{}", l & 0x0F),
                    l => format!("{}", l & 0x0F),
                })
                .collect();
            out += &format!(
                " [y {:.2} flags {:05b} grad ({:.2},{:.2}) links {}]",
                n.y,
                n.flags,
                n.grad.x,
                n.grad.y,
                links.join("")
            );
        }
        out
    }

    /// Every node.
    pub fn all_nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        (0..self.counts.len()).flat_map(move |col| {
            (0..self.counts[col] as usize).map(move |layer| (col * LAYERS + layer) as NodeId)
        })
    }

    /// Nodes in the biggest area connected by walk links (barricades count
    /// as passable): where people actually go. Indexed by node id.
    pub fn main_area(&self) -> Vec<bool> {
        let mut component = vec![u32::MAX; self.nodes.len()];
        let mut sizes: Vec<usize> = Vec::new();
        let mut stack = Vec::new();
        for start in self.all_nodes() {
            if component[start as usize] != u32::MAX {
                continue;
            }
            let id = sizes.len() as u32;
            let mut size = 0;
            component[start as usize] = id;
            stack.push(start);
            while let Some(n) = stack.pop() {
                size += 1;
                for d in 0..8 {
                    if let Some(next) = self.walk_link(n, d)
                        && component[next as usize] == u32::MAX
                    {
                        component[next as usize] = id;
                        stack.push(next);
                    }
                }
            }
            sizes.push(size);
        }
        let Some((biggest, _)) = sizes.iter().enumerate().max_by_key(|(_, s)| **s) else {
            return vec![false; self.nodes.len()];
        };
        component.iter().map(|&c| c == biggest as u32).collect()
    }

    /// Size of the node array (for search buffers).
    pub fn node_capacity(&self) -> usize {
        self.nodes.len()
    }

    /// Number of nodes (for logs).
    pub fn node_count(&self) -> usize {
        self.counts.iter().map(|&c| c as usize).sum()
    }

    /// Walk from `from` towards `to` (same floor) column by column, only
    /// over walk links between nodes with all flags in `need` and not
    /// `avoid` (except at the start and end). True if it gets there.
    pub fn straight_walk(&self, from: NodeId, to: NodeId, need: u8, avoid: u8) -> bool {
        let a = self.pos(from);
        let b = self.pos(to);
        let flat = (b - a).with_y(0.0);
        let steps = (flat.length() / (self.cell * 0.5)).ceil() as i32;
        let mut current = from;
        let target_col = self.column_of(to);
        for i in 1..=steps {
            let p = a + flat * (i as f32 / steps as f32);
            let col = ((Vec2::new(p.x, p.z) - self.origin) / self.cell).round().as_ivec2();
            let cur_col = self.column_of(current);
            if col == cur_col {
                continue;
            }
            let delta = col - cur_col;
            let Some(d) = DIRS.iter().position(|&dir| dir == delta) else {
                return false;
            };
            // Diagonal steps must not cut a corner.
            if d % 2 == 1 {
                let side_a = self.walk_link(current, (d + 7) % 8);
                let side_b = self.walk_link(current, (d + 1) % 8);
                if side_a.is_none() || side_b.is_none() {
                    return false;
                }
            }
            let Some(next) = self.walk_link(current, d) else {
                return false;
            };
            let flags = self.node(next).flags;
            let endpoint = col == target_col;
            if flags & need != need || (!endpoint && flags & avoid != 0) {
                return false;
            }
            current = next;
        }
        current == to
    }
}

/// Region updates waiting to be applied, and the first build's progress.
#[derive(Resource, Default)]
pub struct NavBuilder {
    stage: Stage,
    stable_frames: u32,
    last_colliders: usize,
    cursor: usize,
    started: Option<Instant>,
    sample_time: Duration,
    /// Areas to re-sample (world x/z min, max), with the time they were
    /// marked (applied after a short delay so colliders are rebuilt).
    dirty: Vec<(Vec2, Vec2, f32)>,
    /// Region being re-sampled: columns (min, max) and progress.
    region: Option<RegionUpdate>,
    /// Soft surfaces and where they are, to find removed ones.
    known: HashMap<Entity, (Vec2, Vec2)>,
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    #[default]
    WaitForMap,
    Sample,
    Link,
    Done,
}

#[derive(Clone, Copy, Debug)]
struct RegionUpdate {
    min: IVec2,
    max: IVec2,
    /// Columns sampled so far, then columns linked.
    sampled: i32,
    linked: i32,
}

/// Debug drawing (`--ai-debug`).
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct NavGizmos;

pub struct NavPlugin;

impl Plugin for NavPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NavGrid>()
            .init_resource::<NavBuilder>()
            .init_gizmo_group::<NavGizmos>()
            .add_systems(
                Update,
                (track_surfaces, build_nav, update_regions).chain(),
            );
    }
}

/// Everything the samplers need.
struct Sampler<'a, 'w, 's> {
    spatial: &'a SpatialQuery<'w, 's>,
    colliders: &'a Query<'w, 's, &'static ColliderOf>,
    surfaces: &'a Query<'w, 's, (Entity, &'static SoftSurface, &'static GlobalTransform, &'static Name)>,
    filter: SpatialQueryFilter,
    probe: Collider,
    stand: Collider,
    crouch: Collider,
    stand_centre: f32,
    crouch_centre: f32,
    cos_slope: f32,
    tan_slope: f32,
    max_step: f32,
    max_drop: f32,
    drop_probe: Collider,
    drop_radius: f32,
}

impl Sampler<'_, '_, '_> {
    fn new<'a, 'w, 's>(
        spatial: &'a SpatialQuery<'w, 's>,
        colliders: &'a Query<'w, 's, &'static ColliderOf>,
        surfaces: &'a Query<'w, 's, (Entity, &'static SoftSurface, &'static GlobalTransform, &'static Name)>,
        tuning: &Tuning,
    ) -> Sampler<'a, 'w, 's> {
        let nav = &tuning.ai.nav;
        let body = &tuning.body;
        let r = nav.agent_radius;
        let capsule = |top: f32| {
            let height = top - LIFT;
            Collider::capsule(r, (height - 2.0 * r).max(0.01))
        };
        let slope = tuning.movement.max_slope.to_radians();
        Sampler {
            spatial,
            colliders,
            surfaces,
            filter: SpatialQueryFilter::from_mask(GameLayer::World),
            probe: Collider::sphere(PROBE_RADIUS),
            stand: capsule(body.stand_height),
            crouch: capsule(body.crouch_height),
            stand_centre: LIFT + (body.stand_height - LIFT) / 2.0,
            crouch_centre: LIFT + (body.crouch_height - LIFT) / 2.0,
            cos_slope: slope.cos(),
            tan_slope: slope.tan(),
            max_step: nav.max_step,
            max_drop: nav.max_drop,
            drop_probe: Collider::sphere(nav.drop_radius),
            drop_radius: nav.drop_radius,
        }
    }

    /// Floors in the column at (x, z), top down: (y, flags, blocker).
    fn sample(&self, x: f32, z: f32, y_top: f32, y_bottom: f32) -> Vec<Sample> {
        let mut found = Vec::new();
        let mut excluded: Vec<Entity> = Vec::new();
        let mut y = y_top;
        for _ in 0..48 {
            if y <= y_bottom || found.len() >= LAYERS {
                break;
            }
            let origin = Vec3::new(x, y, z);
            let config = ShapeCastConfig::from_max_distance(y - y_bottom);
            let predicate = |e: Entity| !excluded.contains(&e);
            let Some(hit) = self.spatial.cast_shape_predicate(
                &self.probe,
                origin,
                Quat::IDENTITY,
                Dir3::NEG_Y,
                &config,
                &self.filter,
                &predicate,
            ) else {
                break;
            };
            if hit.distance <= 1e-4 {
                // Started inside this collider: look past it.
                excluded.push(hit.entity);
                continue;
            }
            // The floor's slope from a ray at the column centre (the
            // sphere's contact normal is skewed at edges and corners).
            let sphere_floor = (y - hit.distance) - PROBE_RADIUS / hit.normal1.y.max(0.2);
            let centre_ray = self
                .spatial
                .cast_ray(Vec3::new(x, sphere_floor + 0.25, z), Dir3::NEG_Y, 0.5, true, &self.filter)
                .filter(|r| r.distance > 1e-4);
            let (n, floor) = match centre_ray {
                Some(r) => (r.normal, sphere_floor + 0.25 - r.distance),
                // Over a small hole (or just past an edge): flat at the
                // sphere's height.
                None => (Vec3::Y, hit.point1.y),
            };
            if n.y >= self.cos_slope && hit.normal1.y >= 0.2 {
                let (flags, blocker) = self.clearance(Vec3::new(x, floor, z));
                if flags != 0 {
                    let sloped = n.y < 0.995;
                    let grad = if sloped { Vec2::new(-n.x / n.y, -n.z / n.y) } else { Vec2::ZERO };
                    found.push(Sample {
                        y: floor,
                        flags: flags | if sloped { SLOPE } else { 0 },
                        blocker,
                        grad,
                    });
                }
            }
            // Carry on just under this surface (inside its collider, which
            // the next cast then skips).
            y = (y - hit.distance - PROBE_RADIUS) - 0.02;
        }
        found
    }

    /// What fits standing on the floor at `p`.
    fn clearance(&self, p: Vec3) -> (u8, Option<Entity>) {
        let stand = self.spatial.shape_intersections(
            &self.stand,
            p + Vec3::Y * self.stand_centre,
            Quat::IDENTITY,
            &self.filter,
        );
        if stand.is_empty() {
            return (STAND | CROUCH, None);
        }
        let crouch = self.spatial.shape_intersections(
            &self.crouch,
            p + Vec3::Y * self.crouch_centre,
            Quat::IDENTITY,
            &self.filter,
        );
        if crouch.is_empty() {
            return (CROUCH, None);
        }
        // Only an intact barricade in the way?
        let mut blocker = None;
        for e in &stand {
            let root = self.colliders.get(*e).map(|c| c.body).unwrap_or(*e);
            match self.surfaces.get(root) {
                Ok((_, s, _, _)) if s.role() == SurfaceRole::Barricade && !s.broken => blocker = Some(root),
                _ => return (0, None),
            }
        }
        (STAND | CROUCH | BARRICADE, blocker)
    }

    /// A clear horizontal line between two floor points at body heights.
    fn clear_between(&self, a: Vec3, b: Vec3, heights: [f32; 3]) -> bool {
        heights.iter().all(|&h| {
            let (pa, pb) = (a + Vec3::Y * h, b + Vec3::Y * h);
            let delta = pb - pa;
            let Ok(dir) = Dir3::new(delta) else { return true };
            self.spatial
                .cast_ray(pa, dir, delta.length(), true, &self.filter)
                .is_none()
        })
    }

    /// A body-sized hole from `upper` down to the floor at `lower`.
    fn can_drop(&self, upper: Vec3, lower_y: f32) -> bool {
        let start = upper + Vec3::Y * 0.6;
        let fall = start.y - lower_y;
        let config = ShapeCastConfig::from_max_distance(fall);
        match self.spatial.cast_shape(
            &self.drop_probe,
            start,
            Quat::IDENTITY,
            Dir3::NEG_Y,
            &config,
            &self.filter,
        ) {
            None => true,
            Some(hit) => hit.distance >= fall - self.drop_radius - 0.2,
        }
    }
}

impl NavGrid {
    /// Write one column's samples.
    fn set_column(&mut self, c: IVec2, samples: &[Sample]) {
        let col = self.column_index(c);
        let count = samples.len().min(LAYERS);
        self.counts[col] = count as u8;
        for layer in 0..LAYERS {
            let node = &mut self.nodes[col * LAYERS + layer];
            *node = NavNode::default();
            if let Some(sample) = samples.get(layer) {
                node.y = sample.y;
                node.flags = sample.flags;
                node.blocker = sample.blocker;
                node.grad = sample.grad;
            }
        }
    }

    /// Link every node of column `c` to its neighbours (both directions for
    /// walk links; drop links one way, from `c`).
    fn link_column(&mut self, c: IVec2, s: &Sampler) {
        let cell = self.cell;
        for layer in self.layers(c) {
            let id = self.id(c, layer) as usize;
            self.nodes[id].links = [NO_LINK; 8];
        }
        for layer in self.layers(c) {
            let id = self.id(c, layer);
            let a = *self.node(id);
            let pa = self.pos(id);
            for d in 0..8 {
                let n = c + DIRS[d];
                if !self.in_bounds(n) {
                    continue;
                }
                let dist = if d % 2 == 1 { cell * std::f32::consts::SQRT_2 } else { cell };
                let h = DIRS[d].as_vec2() * cell;
                let max_rise = dist * s.tan_slope + s.max_step;
                // Walk link to the closest node within reach: the height
                // change must follow the floors' slopes (so the side of a
                // stair ramp is a ledge, not a way up).
                let mut best: Option<(usize, f32)> = None;
                let mut below: Option<(usize, f32)> = None;
                for nl in self.layers(n) {
                    let b = self.nodes[self.column_index(n) * LAYERS + nl];
                    let dy = b.y - a.y;
                    // Between the two slopes' predictions (a kink between the
                    // columns, e.g. the top or foot of a ramp), with a step's
                    // tolerance.
                    let (pa_dy, pb_dy) = (a.grad.dot(h), b.grad.dot(h));
                    let tol = s.max_step + 0.04;
                    let walkable = dy.abs() <= max_rise
                        && dy >= pa_dy.min(pb_dy) - tol
                        && dy <= pa_dy.max(pb_dy) + tol;
                    if walkable && best.is_none_or(|(_, bd)| dy.abs() < bd) {
                        best = Some((nl, dy.abs()));
                    }
                    let drop = -dy;
                    if !walkable && drop > s.max_step && drop <= s.max_drop && below.is_none_or(|(_, by)| b.y > by) {
                        below = Some((nl, b.y));
                    }
                }
                if let Some((nl, _)) = best {
                    let b_id = self.id(n, nl);
                    let b = *self.node(b_id);
                    let pb = self.pos(b_id);
                    let heights = if a.flags & STAND != 0 && b.flags & STAND != 0 {
                        LINK_RAYS_STAND
                    } else {
                        LINK_RAYS_CROUCH
                    };
                    // Barricade nodes: the planks are what's in the way;
                    // the rays would hit them, so only check the floor gap.
                    let barricade = (a.flags | b.flags) & BARRICADE != 0;
                    if barricade || s.clear_between(pa, pb, heights) {
                        self.nodes[id as usize].links[d] = KIND_WALK << 4 | nl as u8;
                        let back = self.id(n, nl) as usize;
                        self.nodes[back].links[(d + 4) % 8] = KIND_WALK << 4 | layer as u8;
                    }
                    continue;
                }
                // Nothing at this height next door: a hole to drop through?
                if let Some((nl, y)) = below {
                    let above_hole = Vec3::new(pa.x, a.y, pa.z)
                        + Vec3::new(DIRS[d].x as f32, 0.0, DIRS[d].y as f32) * cell;
                    let heights = LINK_RAYS_STAND;
                    if s.clear_between(pa, above_hole, heights) && s.can_drop(above_hole, y) {
                        self.nodes[id as usize].links[d] = KIND_DROP << 4 | nl as u8;
                    }
                }
            }
        }
    }

    /// Clear back-links into column `c` from its neighbours (before it is
    /// re-linked), so stale links don't survive a re-sample.
    fn unlink_into(&mut self, c: IVec2) {
        for d in 0..8 {
            let n = c + DIRS[d];
            if !self.in_bounds(n) {
                continue;
            }
            for nl in self.layers(n) {
                let id = self.id(n, nl) as usize;
                self.nodes[id].links[(d + 4) % 8] = NO_LINK;
            }
        }
    }

    fn update_edge_flags(&mut self, c: IVec2) {
        for layer in self.layers(c) {
            let id = self.id(c, layer) as usize;
            let links = self.nodes[id].links;
            let all = links.iter().all(|&l| l != NO_LINK && l >> 4 == KIND_WALK);
            let ledge = links.iter().any(|&l| l != NO_LINK && l >> 4 == KIND_DROP);
            let flags = &mut self.nodes[id].flags;
            *flags &= !(EDGE | LEDGE);
            if !all {
                *flags |= EDGE;
            }
            if ledge {
                *flags |= LEDGE;
            }
        }
    }

    fn rebuild_breach_index(&mut self) {
        self.breach_index.clear();
        for (i, b) in self.breaches.iter().enumerate() {
            self.breach_index.entry(b.from).or_default().push(i);
        }
    }

    fn mark_changed(&mut self, min: Vec2, max: Vec2) {
        self.revision += 1;
        self.changes.push((self.revision, min, max));
        if self.changes.len() > 64 {
            self.changes.remove(0);
        }
    }
}

/// Breach links across one soft wall, if it can take a breach charge.
fn breach_links_for(
    grid: &NavGrid,
    entity: Entity,
    surface: &SoftSurface,
    transform: &GlobalTransform,
    name: &str,
    charge_height: f32,
) -> Vec<BreachLink> {
    let mut out = Vec::new();
    // Plain walls only: door and window walls already have a way through.
    if surface.role() != SurfaceRole::Wall
        || surface.is_reinforced()
        || name.contains("door")
        || name.contains("window")
    {
        return out;
    }
    let (min, max) = (surface.grid.min(), surface.grid.max());
    let length = max.x - min.x;
    let count = (length / 1.0).floor().max(1.0) as i32;
    let normal = (transform.rotation() * Vec3::Z).with_y(0.0).normalize_or_zero();
    if normal == Vec3::ZERO {
        return out;
    }
    let (w0, w1) = surface.spec.skin_depth;
    for i in 0..count {
        let u = min.x + length * (i as f32 + 0.5) / count as f32;
        let base = transform.transform_point(Vec3::new(u, 0.05, 0.0));
        let sides = [
            (base + normal * 0.55, transform.transform_point(Vec3::new(u, charge_height, w1))),
            (base - normal * 0.55, transform.transform_point(Vec3::new(u, charge_height, w0))),
        ];
        let nodes: Vec<Option<NodeId>> = sides
            .iter()
            .map(|(p, _)| {
                let id = grid.locate(*p + Vec3::Y * 0.3, 1)?;
                let q = grid.pos(id);
                (grid.node(id).standable()
                    && (q.y - p.y).abs() < 0.4
                    && q.with_y(0.0).distance(p.with_y(0.0)) < 0.5)
                    .then_some(id)
            })
            .collect();
        if let [Some(a), Some(b)] = nodes[..] {
            out.push(BreachLink { from: a, to: b, surface: entity, aim: sides[0].1 });
            out.push(BreachLink { from: b, to: a, surface: entity, aim: sides[1].1 });
        }
    }
    out
}

/// World-space x/z bounds of a soft surface.
fn surface_bounds(surface: &SoftSurface, transform: &GlobalTransform) -> (Vec2, Vec2) {
    let (min, max) = (surface.grid.min(), surface.grid.max());
    let plane = surface.spec.plane;
    let mut lo = Vec2::splat(f32::MAX);
    let mut hi = Vec2::splat(f32::MIN);
    for u in [min.x, max.x] {
        for v in [min.y, max.y] {
            for w in [-0.3, 0.3] {
                let p = transform.transform_point(plane.to_local(u, v, w));
                lo = lo.min(Vec2::new(p.x, p.z));
                hi = hi.max(Vec2::new(p.x, p.z));
            }
        }
    }
    (lo, hi)
}

/// Destruction and round resets mark areas dirty.
#[allow(clippy::too_many_arguments)]
fn track_surfaces(
    mut builder: ResMut<NavBuilder>,
    grid: Res<NavGrid>,
    surfaces: Query<(Entity, &SoftSurface, &GlobalTransform)>,
    added: Query<Entity, Added<SoftSurface>>,
    mut removed: RemovedComponents<SoftSurface>,
    mut changed: MessageReader<SurfaceChanged>,
    mut broken: MessageReader<SurfaceBroken>,
    mut resets: MessageReader<RoundReset>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs();
    let mut marks: Vec<(Vec2, Vec2)> = Vec::new();
    // Remember where every surface is (they don't move).
    for entity in &added {
        if let Ok((_, s, gt)) = surfaces.get(entity) {
            let bounds = surface_bounds(s, gt);
            builder.known.insert(entity, bounds);
            marks.push(bounds);
        }
    }
    for entity in removed.read() {
        if let Some(bounds) = builder.known.remove(&entity) {
            marks.push(bounds);
        }
    }
    for change in changed.read() {
        // Bullet holes don't change where people can walk; big holes do.
        if change.violent || change.removed.len() >= 40 {
            if let Some(&b) = builder.known.get(&change.surface) {
                marks.push(b);
            }
        }
    }
    for b in broken.read() {
        if let Some(&bounds) = builder.known.get(&b.surface) {
            marks.push(bounds);
        }
    }
    if resets.read().last().is_some() {
        marks.extend(builder.known.values().copied());
    }
    if !grid.ready && builder.stage != Stage::Done {
        // The first build will see everything anyway.
        return;
    }
    for (min, max) in marks {
        builder.dirty.push((min, max, now));
    }
}

/// First build: wait for the map, then sample and link, a slice per frame.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn build_nav(
    mut builder: ResMut<NavBuilder>,
    mut grid: ResMut<NavGrid>,
    settings: Res<AiSettings>,
    tuning: Res<Tuning>,
    layout: Option<Res<KeepLayout>>,
    spatial: SpatialQuery,
    colliders: Query<&'static ColliderOf>,
    surfaces: Query<(Entity, &'static SoftSurface, &'static GlobalTransform, &'static Name)>,
    bounds: Query<(&ColliderAabb, Option<&CollisionLayers>, Has<Sensor>)>,
    pending_pieces: Query<(), (With<WorldAssetRoot>, With<RigidBody>, Without<WorldInstance>)>,
    pending_colliders: Query<(), Or<(With<ColliderConstructor>, With<ColliderConstructorHierarchy>)>>,
    time: Res<Time<Real>>,
) {
    if !settings.nav_wanted() || builder.stage == Stage::Done {
        return;
    }
    let nav = &tuning.ai.nav;
    match builder.stage {
        Stage::WaitForMap => {
            let world_colliders = bounds
                .iter()
                .filter(|(_, layers, sensor)| {
                    !sensor && layers.is_none_or(|l| l.memberships.has_all(GameLayer::World))
                })
                .count();
            let settled = pending_pieces.is_empty()
                && pending_colliders.is_empty()
                && world_colliders > 1
                && world_colliders == builder.last_colliders;
            builder.last_colliders = world_colliders;
            builder.stable_frames = if settled { builder.stable_frames + 1 } else { 0 };
            if builder.stable_frames < 20 || time.elapsed_secs() < 1.0 {
                return;
            }
            // Bounds: every finite world collider, plus spawns and sites.
            let mut lo = Vec3::splat(f32::MAX);
            let mut hi = Vec3::splat(f32::MIN);
            for (aabb, layers, sensor) in &bounds {
                if sensor || !layers.is_none_or(|l| l.memberships.has_all(GameLayer::World)) {
                    continue;
                }
                let size = aabb.max - aabb.min;
                if !size.is_finite() || size.x > 1000.0 || size.z > 1000.0 || size.y > 1000.0 {
                    continue;
                }
                lo = lo.min(aabb.min);
                hi = hi.max(aabb.max);
            }
            if let Some(layout) = &layout {
                for spawn in layout.attacker_spawns.iter().chain(&layout.defender_spawns) {
                    lo = lo.min(spawn.position);
                    hi = hi.max(spawn.position);
                }
            }
            if !lo.is_finite() || !hi.is_finite() {
                return;
            }
            let margin = nav.margin;
            let cell = nav.cell.max(0.1);
            let min = Vec2::new(lo.x - margin, lo.z - margin);
            let size = Vec2::new(hi.x - lo.x, hi.z - lo.z) + Vec2::splat(2.0 * margin);
            let nx = (size.x / cell).ceil() as i32 + 1;
            let nz = (size.y / cell).ceil() as i32 + 1;
            *grid = NavGrid {
                ready: false,
                origin: min,
                cell,
                nx,
                nz,
                y_top: hi.y + 2.0,
                y_bottom: lo.y - 2.0,
                nodes: vec![NavNode::default(); (nx * nz) as usize * LAYERS],
                counts: vec![0; (nx * nz) as usize],
                ..default()
            };
            info!(
                "nav: map settled ({world_colliders} colliders); building a {nx} x {nz} grid ({cell} m) over x {:.1}..{:.1}, z {:.1}..{:.1}",
                min.x,
                min.x + size.x,
                min.y,
                min.y + size.y
            );
            builder.stage = Stage::Sample;
            builder.cursor = 0;
            builder.started = Some(Instant::now());
            // Remember the surfaces so later changes find their bounds.
            for (entity, s, gt, _) in &surfaces {
                let b = surface_bounds(s, gt);
                builder.known.insert(entity, b);
            }
        }
        Stage::Sample | Stage::Link => {
            let sampler = Sampler::new(&spatial, &colliders, &surfaces, &tuning);
            let budget = Duration::from_secs_f32(nav.build_budget_ms.max(0.5) / 1000.0);
            let start = Instant::now();
            let total = (grid.nx * grid.nz) as usize;
            while start.elapsed() < budget && builder.cursor < total {
                let i = builder.cursor as i32;
                let c = IVec2::new(i % grid.nx, i / grid.nx);
                if builder.stage == Stage::Sample {
                    let p = grid.column_centre(c);
                    let samples = sampler.sample(p.x, p.y, grid.y_top, grid.y_bottom);
                    grid.set_column(c, &samples);
                } else {
                    grid.link_column(c, &sampler);
                }
                builder.cursor += 1;
            }
            if builder.cursor < total {
                return;
            }
            builder.cursor = 0;
            if builder.stage == Stage::Sample {
                builder.sample_time = builder.started.map(|s| s.elapsed()).unwrap_or_default();
                builder.stage = Stage::Link;
                return;
            }
            for i in 0..total as i32 {
                let c = IVec2::new(i % grid.nx, i / grid.nx);
                grid.update_edge_flags(c);
            }
            let charge_height = tuning.destruction.breach_charge.wall_height;
            let mut breaches = Vec::new();
            grid.ready = true;
            for (entity, s, gt, name) in &surfaces {
                breaches.extend(breach_links_for(&grid, entity, s, gt, name, charge_height));
            }
            grid.breaches = breaches;
            grid.rebuild_breach_index();
            grid.revision += 1;
            builder.stage = Stage::Done;
            let elapsed = builder.started.map(|s| s.elapsed()).unwrap_or_default();
            info!(
                "nav: ready: {} nodes, {} breach links; sampled in {:.0} ms, linked in {:.0} ms (spread over frames)",
                grid.node_count(),
                grid.breaches.len(),
                builder.sample_time.as_secs_f32() * 1000.0,
                (elapsed - builder.sample_time).as_secs_f32() * 1000.0
            );
        }
        Stage::Done => {}
    }
}

/// Re-sample and re-link dirty areas, a slice per frame.
#[allow(clippy::too_many_arguments)]
fn update_regions(
    mut builder: ResMut<NavBuilder>,
    mut grid: ResMut<NavGrid>,
    tuning: Res<Tuning>,
    spatial: SpatialQuery,
    colliders: Query<&'static ColliderOf>,
    surfaces: Query<(Entity, &'static SoftSurface, &'static GlobalTransform, &'static Name)>,
    time: Res<Time>,
) {
    if !grid.ready {
        return;
    }
    let now = time.elapsed_secs();
    if builder.region.is_none() {
        // Merge everything that has waited long enough (colliders are
        // rebuilt on the fixed tick, so give it a moment).
        let ready: Vec<(Vec2, Vec2)> = builder
            .dirty
            .iter()
            .filter(|(_, _, at)| now - at >= 0.1)
            .map(|(a, b, _)| (*a, *b))
            .collect();
        if ready.is_empty() {
            return;
        }
        builder.dirty.retain(|(_, _, at)| now - at < 0.1);
        let mut lo = Vec2::splat(f32::MAX);
        let mut hi = Vec2::splat(f32::MIN);
        for (a, b) in ready {
            lo = lo.min(a);
            hi = hi.max(b);
        }
        let pad = 0.6;
        let min = ((lo - Vec2::splat(pad) - grid.origin) / grid.cell).floor().as_ivec2().max(IVec2::ZERO);
        let max = ((hi + Vec2::splat(pad) - grid.origin) / grid.cell)
            .ceil()
            .as_ivec2()
            .min(IVec2::new(grid.nx - 1, grid.nz - 1));
        if min.x > max.x || min.y > max.y {
            return;
        }
        builder.region = Some(RegionUpdate { min, max, sampled: 0, linked: 0 });
    }
    let Some(mut region) = builder.region else { return };
    let sampler = Sampler::new(&spatial, &colliders, &surfaces, &tuning);
    let budget = Duration::from_secs_f32(tuning.ai.nav.update_budget_ms.max(0.2) / 1000.0);
    let start = Instant::now();
    let width = region.max.x - region.min.x + 1;
    let height = region.max.y - region.min.y + 1;
    let columns = width * height;
    // Linking covers a one-column ring around the region too.
    let ring_min = (region.min - IVec2::ONE).max(IVec2::ZERO);
    let ring_max = (region.max + IVec2::ONE).min(IVec2::new(grid.nx - 1, grid.nz - 1));
    let ring_w = ring_max.x - ring_min.x + 1;
    let ring_columns = ring_w * (ring_max.y - ring_min.y + 1);
    while start.elapsed() < budget {
        if region.sampled < columns {
            let c = region.min + IVec2::new(region.sampled % width, region.sampled / width);
            grid.unlink_into(c);
            let p = grid.column_centre(c);
            let samples = sampler.sample(p.x, p.y, grid.y_top, grid.y_bottom);
            grid.set_column(c, &samples);
            region.sampled += 1;
        } else if region.linked < ring_columns {
            let c = ring_min + IVec2::new(region.linked % ring_w, region.linked / ring_w);
            grid.link_column(c, &sampler);
            region.linked += 1;
        } else {
            break;
        }
    }
    if region.sampled < columns || region.linked < ring_columns {
        builder.region = Some(region);
        return;
    }
    builder.region = None;
    // Edge flags around the region (links into it changed too).
    let edge_min = (ring_min - IVec2::ONE).max(IVec2::ZERO);
    let edge_max = (ring_max + IVec2::ONE).min(IVec2::new(grid.nx - 1, grid.nz - 1));
    for z in edge_min.y..=edge_max.y {
        for x in edge_min.x..=edge_max.x {
            grid.update_edge_flags(IVec2::new(x, z));
        }
    }
    // Breach links: recompute every one (cheap, a few per wall).
    let charge_height = tuning.destruction.breach_charge.wall_height;
    let mut breaches = Vec::new();
    for (entity, s, gt, name) in &surfaces {
        breaches.extend(breach_links_for(&grid, entity, s, gt, name, charge_height));
    }
    grid.breaches = breaches;
    grid.rebuild_breach_index();
    let lo = grid.column_centre(region.min) - Vec2::splat(grid.cell);
    let hi = grid.column_centre(region.max) + Vec2::splat(grid.cell);
    grid.mark_changed(lo, hi);
    debug!("nav: updated columns {:?}..{:?} (revision {})", region.min, region.max, grid.revision);
}

/// `--ai-debug`: draw the grid near the camera (green standing, yellow
/// crouch-only, red barricaded; blue drop links, magenta breach links).
pub fn draw_nav(
    grid: Res<NavGrid>,
    tuning: Res<Tuning>,
    camera: Query<&GlobalTransform, With<crate::player::PlayerCamera>>,
    mut gizmos: Gizmos<NavGizmos>,
) {
    let Ok(eye) = camera.single() else { return };
    if !grid.ready {
        return;
    }
    let eye = eye.translation();
    let range = tuning.ai.nav.debug_range;
    for id in grid.nodes_near(eye, range, 6.0) {
        let node = grid.node(id);
        let p = grid.pos(id) + Vec3::Y * 0.05;
        let color = if node.flags & BARRICADE != 0 {
            Color::srgb(1.0, 0.2, 0.2)
        } else if node.flags & STAND == 0 {
            Color::srgb(1.0, 0.85, 0.2)
        } else if node.flags & EDGE != 0 {
            Color::srgb(0.2, 0.6, 0.3)
        } else {
            Color::srgb(0.3, 0.95, 0.4)
        };
        for (to, kind, _) in grid.links(id) {
            let q = grid.pos(to) + Vec3::Y * 0.05;
            match kind {
                LinkKind::Walk if to > id => gizmos.line(p, p.lerp(q, 0.5), color),
                LinkKind::Walk => gizmos.line(p, p.lerp(q, 0.5), color),
                LinkKind::Drop => gizmos.line(p, q, Color::srgb(0.3, 0.5, 1.0)),
                LinkKind::Breach => gizmos.line(p + Vec3::Y * 0.3, q + Vec3::Y * 0.3, Color::srgb(1.0, 0.2, 1.0)),
            }
        }
    }
}
