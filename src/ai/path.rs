//! A* over the navigation grid, and path smoothing.
//!
//! Costs are metres walked, plus extras: crouch-only nodes cost more (slow),
//! a barricade costs the time to break it, a drop or a breach costs extra.
//! Callers can add their own per-node penalty (e.g. a second attack route
//! avoiding the first, or known enemy positions).
//!
//! Smoothing ("string pulling") skips grid nodes where a straight walk on
//! the grid is possible, but never across edge nodes (next to walls), so
//! paths go through the middle of doorways and keep off walls. Drops,
//! breaches and barricades always stay as waypoints.

use std::{cmp::Reverse, collections::BinaryHeap};

use bevy::prelude::*;

use super::nav::{BARRICADE, BreachLink, EDGE, LEDGE, LinkKind, NavGrid, NodeId, STAND};

/// Reused search buffers (one search at a time, which may be spread over
/// several calls: [`PathScratch::begin`], [`PathScratch::resume`]).
#[derive(Resource, Default)]
pub struct PathScratch {
    g: Vec<f32>,
    parent: Vec<u32>,
    via: Vec<LinkKind>,
    stamp: Vec<u32>,
    generation: u32,
    open: BinaryHeap<(Reverse<u32>, NodeId)>,
    pending: Option<Pending>,
    /// Searches started and nodes expanded so far (stats).
    pub searches: u32,
    pub expansions: u64,
}

/// What a path may use.
#[derive(Clone, Copy)]
pub struct PathOptions {
    /// Extra cost (metres) of breaking through a barricade; `None` = never.
    /// Attackers break them routinely; defenders only as a last resort
    /// (their own barricades shouldn't lock them out of a site).
    pub barricades: Option<f32>,
    /// Extra cost (metres) of blowing through a soft wall with a breach
    /// charge; `None` = can't (carries none).
    pub breach: Option<f32>,
    pub max_expansions: u32,
    /// Weight of the distance-to-goal estimate: 1 finds the cheapest path;
    /// above 1 usually searches less of a big map, for a path at most that
    /// many times as costly (in practice the same or a few percent more).
    /// Much above 1 makes the worst cases (dead ends towards the goal)
    /// worse, not better.
    pub greed: f32,
}

/// One waypoint.
#[derive(Clone, Debug)]
pub struct Waypoint {
    pub pos: Vec3,
    /// How the path arrives here.
    pub kind: LinkKind,
    /// Only crouching fits here.
    pub crouch: bool,
    /// Barricade to break before stepping here.
    pub blocker: Option<Entity>,
    /// Breach to make before stepping here.
    pub breach: Option<BreachLink>,
}

#[derive(Clone, Debug, Default)]
pub struct Path {
    pub points: Vec<Waypoint>,
    /// Grid revision it was planned on.
    pub revision: u32,
    /// Total cost (metres + extras).
    pub cost: f32,
}

const COST_CROUCH: f32 = 1.8;
/// Breaking a barricade, for attackers (metres of walking it's worth).
pub const COST_BARRICADE: f32 = 6.0;
/// The same for defenders: only when there's no other way.
pub const COST_BARRICADE_DEFENDER: f32 = 40.0;
const COST_EDGE: f32 = 0.25;
const COST_LEDGE: f32 = 2.0;
const COST_DROP: f32 = 1.5;
/// Breaching a soft wall: placing, backing off and waiting is worth this
/// much walking.
pub const COST_BREACH: f32 = 14.0;
/// For a stack that is meant to open a second way in.
pub const COST_BREACH_PREFERRED: f32 = 2.0;

/// Shortest path from `from` to `to`, searched in one go. `penalty` adds a
/// cost per entered node.
pub fn find_path(
    grid: &NavGrid,
    scratch: &mut PathScratch,
    from: NodeId,
    to: NodeId,
    options: PathOptions,
    penalty: &dyn Fn(NodeId) -> f32,
) -> Option<Path> {
    if !scratch.begin(grid, from, to, options) {
        return None;
    }
    let mut budget = u32::MAX;
    match scratch.resume(grid, &mut budget, penalty) {
        Search::Found(path) => Some(path),
        _ => None,
    }
}

/// How a search spread over several calls is going.
pub enum Search {
    /// Out of budget for now; call `resume` again.
    Running,
    Found(Path),
    Failed,
}

/// A search in progress.
#[derive(Clone, Copy)]
struct Pending {
    from: NodeId,
    to: NodeId,
    options: PathOptions,
    expansions: u32,
    /// Grid revision it started on (a change restarts it).
    revision: u32,
}

impl PathScratch {
    /// Start a search from `from` to `to` (dropping any in progress); false
    /// if either node doesn't exist. Continue it with [`Self::resume`].
    pub fn begin(&mut self, grid: &NavGrid, from: NodeId, to: NodeId, options: PathOptions) -> bool {
        self.pending = None;
        self.open.clear();
        if !grid.ready || !grid.exists(from) || !grid.exists(to) {
            return false;
        }
        let n = grid.node_capacity();
        if self.g.len() != n {
            self.g = vec![0.0; n];
            self.parent = vec![u32::MAX; n];
            self.via = vec![LinkKind::Walk; n];
            self.stamp = vec![0; n];
        }
        self.generation = self.generation.wrapping_add(1).max(1);
        self.searches += 1;
        self.visit(from, 0.0, u32::MAX, LinkKind::Walk);
        let h = grid.pos(from).distance(grid.pos(to)) * options.greed.max(1.0);
        self.open.push((Reverse(h.to_bits()), from));
        self.pending = Some(Pending { from, to, options, expansions: 0, revision: grid.revision });
        true
    }

    /// Drop the search in progress.
    pub fn cancel(&mut self) {
        self.pending = None;
        self.open.clear();
    }

    fn visit(&mut self, id: NodeId, g: f32, parent: u32, via: LinkKind) {
        let i = id as usize;
        self.stamp[i] = self.generation;
        self.g[i] = g;
        self.parent[i] = parent;
        self.via[i] = via;
    }

    /// Continue the search in progress, expanding at most `budget` nodes
    /// (taken off `budget`). A changed grid restarts it.
    pub fn resume(&mut self, grid: &NavGrid, budget: &mut u32, penalty: &dyn Fn(NodeId) -> f32) -> Search {
        let Some(mut pending) = self.pending else {
            return Search::Failed;
        };
        if grid.revision != pending.revision {
            if !self.begin(grid, pending.from, pending.to, pending.options) {
                return Search::Failed;
            }
            pending = self.pending.expect("just begun");
        }
        let (to, options) = (pending.to, pending.options);
        let goal = grid.pos(to);
        let greed = options.greed.max(1.0);
        let generation = self.generation;
        let mut found = false;
        let mut out_of_budget = false;
        while let Some((Reverse(f_bits), id)) = self.open.pop() {
            if id == to {
                found = true;
                break;
            }
            let g = self.g[id as usize];
            let h = grid.pos(id).distance(goal) * greed;
            // Stale heap entry (a cheaper route was found since).
            if f32::from_bits(f_bits) > g + h + 1e-3 {
                continue;
            }
            if *budget == 0 {
                self.open.push((Reverse(f_bits), id));
                out_of_budget = true;
                break;
            }
            *budget -= 1;
            pending.expansions += 1;
            self.expansions += 1;
            if pending.expansions > options.max_expansions {
                break;
            }
            for (next, kind, length) in grid.links(id) {
                let node = grid.node(next);
                let mut cost = length;
                match kind {
                    LinkKind::Walk => {}
                    LinkKind::Drop => cost += COST_DROP,
                    LinkKind::Breach => match options.breach {
                        Some(extra) => cost += extra,
                        None => continue,
                    },
                }
                if node.flags & BARRICADE != 0 {
                    let Some(extra) = options.barricades else { continue };
                    cost += extra;
                }
                // Keep off walls a little, and off ledges a lot.
                if node.flags & LEDGE != 0 {
                    cost += COST_LEDGE;
                } else if node.flags & EDGE != 0 {
                    cost += COST_EDGE;
                }
                if node.flags & STAND == 0 {
                    cost *= COST_CROUCH;
                }
                cost += penalty(next);
                let g_next = g + cost;
                let i = next as usize;
                if self.stamp[i] == generation && self.g[i] <= g_next {
                    continue;
                }
                self.visit(next, g_next, id, kind);
                let f = g_next + grid.pos(next).distance(goal) * greed;
                self.open.push((Reverse(f.to_bits()), next));
            }
        }
        if out_of_budget {
            self.pending = Some(pending);
            return Search::Running;
        }
        self.cancel();
        if !found {
            return Search::Failed;
        }
        // Walk back.
        let mut chain = Vec::new();
        let mut id = to;
        while id != u32::MAX {
            chain.push((id, self.via[id as usize]));
            id = self.parent[id as usize];
        }
        chain.reverse();
        Search::Found(smooth(grid, &chain, self.g[to as usize]))
    }
}

/// Turn a node chain into sparse waypoints.
fn smooth(grid: &NavGrid, chain: &[(NodeId, LinkKind)], cost: f32) -> Path {
    let waypoint = |i: usize| {
        let (node, kind) = chain[i];
        let n = grid.node(node);
        let breach = if kind == LinkKind::Breach && i > 0 {
            grid.breach_from(chain[i - 1].0, node).copied()
        } else {
            None
        };
        Waypoint {
            pos: grid.pos(node),
            kind,
            crouch: n.flags & STAND == 0,
            blocker: if n.flags & BARRICADE != 0 { n.blocker } else { None },
            breach,
        }
    };
    let special = |i: usize| {
        let (node, kind) = chain[i];
        kind != LinkKind::Walk || grid.node(node).flags & BARRICADE != 0
    };
    let mut points = Vec::new();
    let mut i = 0;
    while i + 1 < chain.len() {
        // Furthest node reachable in a straight walk, stopping at specials.
        let limit = (i + 28).min(chain.len() - 1);
        let mut next = i + 1;
        if !special(i + 1) {
            let need = if grid.node(chain[i].0).flags & STAND != 0 { STAND } else { 0 };
            let mut j = limit;
            while j > i + 1 {
                let blocked = (i + 1..=j).any(special);
                if !blocked && grid.straight_walk(chain[i].0, chain[j].0, need, BARRICADE | EDGE) {
                    break;
                }
                j -= 1;
            }
            next = j;
        }
        points.push(waypoint(next));
        i = next;
    }
    if points.is_empty() && !chain.is_empty() {
        points.push(waypoint(chain.len() - 1));
    }
    Path {
        points,
        revision: grid.revision,
        cost,
    }
}
