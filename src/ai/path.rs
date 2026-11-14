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

/// Reused search buffers (one search at a time).
#[derive(Resource, Default)]
pub struct PathScratch {
    g: Vec<f32>,
    parent: Vec<u32>,
    via: Vec<LinkKind>,
    stamp: Vec<u32>,
    generation: u32,
    /// Searches run this frame (for budgets and stats).
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

/// Shortest path from `from` to `to`. `penalty` adds a cost per entered node.
pub fn find_path(
    grid: &NavGrid,
    scratch: &mut PathScratch,
    from: NodeId,
    to: NodeId,
    options: PathOptions,
    penalty: &dyn Fn(NodeId) -> f32,
) -> Option<Path> {
    if !grid.ready || !grid.exists(from) || !grid.exists(to) {
        return None;
    }
    let n = grid.node_capacity();
    if scratch.g.len() != n {
        scratch.g = vec![0.0; n];
        scratch.parent = vec![u32::MAX; n];
        scratch.via = vec![LinkKind::Walk; n];
        scratch.stamp = vec![0; n];
    }
    scratch.generation = scratch.generation.wrapping_add(1).max(1);
    scratch.searches += 1;
    let generation = scratch.generation;
    let goal = grid.pos(to);
    let mut open = BinaryHeap::new();
    let visit = |s: &mut PathScratch, id: NodeId, g: f32, parent: u32, via: LinkKind| {
        let i = id as usize;
        s.stamp[i] = generation;
        s.g[i] = g;
        s.parent[i] = parent;
        s.via[i] = via;
    };
    visit(scratch, from, 0.0, u32::MAX, LinkKind::Walk);
    open.push((Reverse(grid.pos(from).distance(goal).to_bits()), from));
    let mut expansions = 0;
    let mut found = false;
    while let Some((Reverse(f_bits), id)) = open.pop() {
        if id == to {
            found = true;
            break;
        }
        let g = scratch.g[id as usize];
        let h = grid.pos(id).distance(goal);
        // Stale heap entry (a cheaper route was found since).
        if f32::from_bits(f_bits) > g + h + 1e-3 {
            continue;
        }
        expansions += 1;
        if expansions > options.max_expansions {
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
            if scratch.stamp[i] == generation && scratch.g[i] <= g_next {
                continue;
            }
            visit(scratch, next, g_next, id, kind);
            let f = g_next + grid.pos(next).distance(goal);
            open.push((Reverse(f.to_bits()), next));
        }
    }
    scratch.expansions += expansions as u64;
    if !found {
        return None;
    }
    // Walk back.
    let mut chain = Vec::new();
    let mut id = to;
    while id != u32::MAX {
        chain.push((id, scratch.via[id as usize]));
        id = scratch.parent[id as usize];
    }
    chain.reverse();
    Some(smooth(grid, &chain, scratch.g[to as usize]))
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
