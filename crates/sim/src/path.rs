//! Pathfinding: flow fields shared per move order.
//!
//! One Dijkstra from the target over the walkability grid produces a
//! direction per tile; every unit in the order follows the same field.
//! Cost is per-order, not per-unit — that's what makes 200-unit move
//! orders affordable. Fields also incorporate building blockers at the
//! moment the order is issued (terrain itself never changes).
//!
//! Determinism: BinaryHeap entries are tie-broken by tile index, and the
//! whole computation is pure integer arithmetic.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::map::{Map, TilePos};

/// 8 directions, index -> (dx, dy). Order matters: it is part of determinism.
pub const DIRS: [(i32, i32); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

const NO_DIR: u8 = u8::MAX;
const STRAIGHT: u32 = 2;
const DIAGONAL: u32 = 3; // ~sqrt(2) * 2

#[derive(Clone)]
pub struct FlowField {
    pub target: TilePos,
    /// Direction index per tile (into DIRS), NO_DIR if unreachable/at target.
    pub dir: Vec<u8>,
    /// Integer cost from each tile to target; u32::MAX if unreachable.
    pub cost: Vec<u32>,
}

impl FlowField {
    pub fn dir_at(&self, map: &Map, t: TilePos) -> Option<(i32, i32)> {
        if !map.in_bounds(t.x, t.y) {
            return None;
        }
        let d = self.dir[map.idx(t.x, t.y)];
        if d == NO_DIR {
            None
        } else {
            Some(DIRS[d as usize])
        }
    }

    pub fn cost_at(&self, map: &Map, t: TilePos) -> u32 {
        if !map.in_bounds(t.x, t.y) {
            return u32::MAX;
        }
        self.cost[map.idx(t.x, t.y)]
    }
}

/// `blocked` marks tiles occupied by buildings/resources (dynamic blockers on
/// top of terrain). Same length as the map grid.
pub fn compute_flow_field(map: &Map, blocked: &[bool], target: TilePos) -> FlowField {
    // Tie-break mirroring (see the Dijkstra comment below): targets in the
    // SE half scan in point-mirrored order everywhere, ring search included
    // — a gather field toward a blocked mineral tile must resolve to the
    // mirrored adjacent tile on the mirrored side of the map.
    let n_tiles = (map.width * map.height) as u32;
    let flip = 2 * (map.idx(target.x, target.y) as u32) > n_tiles - 1;
    let m = |v: i32| if flip { -v } else { v };

    let mut target = target;
    // If the target tile itself is blocked (e.g. right-click on a building),
    // find the nearest open tile by ring search so the field still resolves.
    if !open(map, blocked, target.x, target.y) {
        'search: for r in 1i32..12 {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    let (x, y) = (target.x + m(dx), target.y + m(dy));
                    if open(map, blocked, x, y) {
                        target = TilePos::new(x, y);
                        break 'search;
                    }
                }
            }
        }
    }
    compute_flow_field_multi(map, blocked, &[target])
}

/// Flow field seeded from EVERY open tile in `seeds` at cost 0. Used for
/// destinations with a footprint (depots, resources, build sites): units
/// then approach whichever side of the structure is actually reachable.
/// A single-tile seed picked by ring search can land in a sealed pocket
/// beside a walled-in building — every worker outside gets an unreachable
/// field and the economy freezes (found by soak).
pub fn compute_flow_field_multi(map: &Map, blocked: &[bool], seeds: &[TilePos]) -> FlowField {
    let n = (map.width * map.height) as usize;
    let mut cost = vec![u32::MAX; n];
    let mut dir = vec![NO_DIR; n];
    let n_tiles = n as u32;

    let open_seeds: Vec<usize> =
        seeds.iter().filter(|t| open(map, blocked, t.x, t.y)).map(|t| map.idx(t.x, t.y)).collect();
    let target = seeds.first().copied().unwrap_or(TilePos::new(0, 0));
    if open_seeds.is_empty() {
        return FlowField { target, dir, cost };
    }
    // Mirror-consistent flip for a seed SET: min+max index straddles the
    // map midpoint antisymmetrically under 180-degree rotation.
    let lo = *open_seeds.iter().min().unwrap() as u32;
    let hi = *open_seeds.iter().max().unwrap() as u32;
    let flip = lo + hi > n_tiles - 1;

    // Dijkstra outward from target. Tie-breaks (heap order, DIRS scan
    // order) decide which of several equal-cost paths a field prefers.
    // Left as-is they always prefer the NW, which on a 180-degree-mirrored
    // map gives one spawn systematically better path shapes (the Kyth
    // mirror SE bias). Fields whose target lies in the SE half therefore
    // run with point-mirrored tie-breaks, making mirrored orders produce
    // mirrored fields.
    let heap_key = |i: u32| if flip { n_tiles - 1 - i } else { i };
    // heap entries: (cost, tie_key, tile_idx).
    let mut heap: BinaryHeap<Reverse<(u32, u32, u32)>> = BinaryHeap::new();
    for &si in &open_seeds {
        cost[si] = 0;
        heap.push(Reverse((0, heap_key(si as u32), si as u32)));
    }

    while let Some(Reverse((c, _, i))) = heap.pop() {
        if c > cost[i as usize] {
            continue;
        }
        let x = i as i32 % map.width;
        let y = i as i32 / map.width;
        for k in 0..8 {
            // Mirrored scan: DIRS[d] -> DIRS[d+4] is exactly (dx,dy) -> (-dx,-dy).
            let di = if flip { (k + 4) % 8 } else { k };
            let (dx, dy) = DIRS[di];
            let (nx, ny) = (x + dx, y + dy);
            if !map.in_bounds(nx, ny) || !open(map, blocked, nx, ny) {
                continue;
            }
            // Expanding from `i` to neighbor `n` means units will walk n -> i,
            // so traversability is checked in that direction.
            if !map.traversable(nx, ny, x, y) {
                continue;
            }
            let diagonal = dx != 0 && dy != 0;
            if diagonal {
                // No corner cutting: both orthogonal neighbors must be open.
                if !open(map, blocked, x + dx, y) || !open(map, blocked, x, y + dy) {
                    continue;
                }
                if !map.traversable(nx, ny, x + dx, y) || !map.traversable(nx, ny, x, y + dy) {
                    continue;
                }
            }
            let step = if diagonal { DIAGONAL } else { STRAIGHT };
            let nc = c + step;
            let ni = map.idx(nx, ny);
            if nc < cost[ni] {
                cost[ni] = nc;
                // Unit at (nx,ny) should move opposite the expansion dir.
                dir[ni] = ((di + 4) % 8) as u8;
                heap.push(Reverse((nc, heap_key(ni as u32), ni as u32)));
            }
        }
    }

    FlowField { target, dir, cost }
}

#[inline]
fn open(map: &Map, blocked: &[bool], x: i32, y: i32) -> bool {
    map.walkable(x, y) && !blocked[map.idx(x, y)]
}

/// The tiles ringing a footprint at `origin` (fw x fh) — seed set for
/// `compute_flow_field_multi` so units can approach from any open side.
pub fn ring_seeds(origin: TilePos, fw: i32, fh: i32) -> Vec<TilePos> {
    let mut v = Vec::with_capacity(((fw + 2) * (fh + 2) - fw * fh) as usize);
    for y in origin.y - 1..=origin.y + fh {
        for x in origin.x - 1..=origin.x + fw {
            let inside =
                x >= origin.x && x < origin.x + fw && y >= origin.y && y < origin.y + fh;
            if !inside {
                v.push(TilePos::new(x, y));
            }
        }
    }
    v
}

/// Pool of flow fields referenced by unit orders. Freed by periodic mark-and-
/// sweep from `step` (deterministic: runs at fixed ticks, scans in order).
#[derive(Default)]
pub struct FieldPool {
    pub fields: Vec<Option<FlowField>>,
}

impl FieldPool {
    pub fn insert(&mut self, f: FlowField) -> u32 {
        // Reuse the first free slot (deterministic scan order).
        for (i, slot) in self.fields.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(f);
                return i as u32;
            }
        }
        self.fields.push(Some(f));
        (self.fields.len() - 1) as u32
    }

    pub fn get(&self, id: u32) -> Option<&FlowField> {
        self.fields.get(id as usize).and_then(|s| s.as_ref())
    }

    pub fn sweep(&mut self, live: &[bool]) {
        for (i, slot) in self.fields.iter_mut().enumerate() {
            if !live.get(i).copied().unwrap_or(false) {
                *slot = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::meridian;

    #[test]
    fn caverns_mains_connect() {
        let m = crate::map::by_name("caverns").unwrap();
        let blocked = vec![false; (m.width * m.height) as usize];
        let f = compute_flow_field(&m, &blocked, m.starts[0]);
        assert_ne!(f.cost_at(&m, m.starts[1]), u32::MAX, "caverns mains not connected");
        // Expansion slots must be reachable too.
        for e in &m.expansions {
            assert_ne!(f.cost_at(&m, *e), u32::MAX, "expansion unreachable");
        }
    }

    #[test]
    fn field_reaches_across_map() {
        let m = meridian();
        let blocked = vec![false; (m.width * m.height) as usize];
        // From SE main to NW main: must route through both ramps.
        let f = compute_flow_field(&m, &blocked, m.starts[0]);
        let start = m.starts[1];
        assert_ne!(f.cost_at(&m, start), u32::MAX, "SE main cannot reach NW main");

        // Walk the field from SE start; it must arrive at the target.
        let mut t = start;
        for _ in 0..2000 {
            if t == f.target {
                return;
            }
            let Some((dx, dy)) = f.dir_at(&m, t) else {
                panic!("field dead-ends at {:?}", t)
            };
            t = TilePos::new(t.x + dx, t.y + dy);
        }
        panic!("field walk did not terminate");
    }

    #[test]
    fn blocked_target_resolves_to_nearby_tile() {
        let m = meridian();
        let blocked = vec![false; (m.width * m.height) as usize];
        // A mineral tile is Blocked terrain; the field should retarget nearby.
        let patch = m.minerals[0].0;
        let f = compute_flow_field(&m, &blocked, patch);
        assert!(m.walkable(f.target.x, f.target.y));
    }
}
