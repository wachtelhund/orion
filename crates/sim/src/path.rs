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
    let n = (map.width * map.height) as usize;
    let mut cost = vec![u32::MAX; n];
    let mut dir = vec![NO_DIR; n];

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
                    let (x, y) = (target.x + dx, target.y + dy);
                    if open(map, blocked, x, y) {
                        target = TilePos::new(x, y);
                        break 'search;
                    }
                }
            }
        }
    }
    if !open(map, blocked, target.x, target.y) {
        return FlowField { target, dir, cost };
    }

    // Dijkstra outward from target. heap entries: (cost, tile_idx).
    let mut heap: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();
    let ti = map.idx(target.x, target.y);
    cost[ti] = 0;
    heap.push(Reverse((0, ti as u32)));

    while let Some(Reverse((c, i))) = heap.pop() {
        if c > cost[i as usize] {
            continue;
        }
        let x = i as i32 % map.width;
        let y = i as i32 / map.width;
        for (di, &(dx, dy)) in DIRS.iter().enumerate() {
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
                heap.push(Reverse((nc, ni as u32)));
            }
        }
    }

    FlowField { target, dir, cost }
}

#[inline]
fn open(map: &Map, blocked: &[bool], x: i32, y: i32) -> bool {
    map.walkable(x, y) && !blocked[map.idx(x, y)]
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
