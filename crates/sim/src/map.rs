//! Tile map: heights, cliffs, ramps. Terrain is static at runtime, so all
//! derived navigation data can be precomputed at load. Buildings add dynamic
//! blockers on top via `State`, not here.

use serde::{Deserialize, Serialize};

use crate::fixed::{Fx, FxVec2};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum TileKind {
    Ground,
    /// Impassable terrain (cliff face, rocks).
    Blocked,
    /// Connects different heights: traversal onto/off a ramp ignores the
    /// height-equality rule.
    Ramp,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct TilePos {
    pub x: i32,
    pub y: i32,
}

impl TilePos {
    pub fn new(x: i32, y: i32) -> TilePos {
        TilePos { x, y }
    }

    pub fn of(pos: FxVec2) -> TilePos {
        TilePos { x: pos.x.floor_int(), y: pos.y.floor_int() }
    }

    pub fn center(self) -> FxVec2 {
        FxVec2::tile_center(self.x, self.y)
    }
}

#[derive(Clone)]
pub struct Map {
    pub width: i32,
    pub height: i32,
    pub kind: Vec<TileKind>,
    pub elev: Vec<u8>,
    /// Player start locations (HQ centers), index = player id.
    pub starts: Vec<TilePos>,
    /// Mineral patch tiles with starting amounts.
    pub minerals: Vec<(TilePos, i32)>,
    /// Plasma geyser footprint origins (2x2 tiles) with gas amounts.
    pub geysers: Vec<(TilePos, i32)>,
}

impl Map {
    #[inline]
    pub fn idx(&self, x: i32, y: i32) -> usize {
        debug_assert!(self.in_bounds(x, y));
        (y * self.width + x) as usize
    }

    #[inline]
    pub fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.width && y < self.height
    }

    #[inline]
    pub fn kind_at(&self, x: i32, y: i32) -> TileKind {
        if !self.in_bounds(x, y) {
            return TileKind::Blocked;
        }
        self.kind[self.idx(x, y)]
    }

    #[inline]
    pub fn elev_at(&self, x: i32, y: i32) -> u8 {
        if !self.in_bounds(x, y) {
            return 0;
        }
        self.elev[self.idx(x, y)]
    }

    /// Terrain-only walkability (ignores buildings/resources).
    #[inline]
    pub fn walkable(&self, x: i32, y: i32) -> bool {
        matches!(self.kind_at(x, y), TileKind::Ground | TileKind::Ramp)
    }

    /// Can a ground unit step between two adjacent tiles? Equal height, or
    /// via a ramp tile on either side.
    #[inline]
    pub fn traversable(&self, ax: i32, ay: i32, bx: i32, by: i32) -> bool {
        if !self.walkable(ax, ay) || !self.walkable(bx, by) {
            return false;
        }
        if self.elev_at(ax, ay) == self.elev_at(bx, by) {
            return true;
        }
        self.kind_at(ax, ay) == TileKind::Ramp || self.kind_at(bx, by) == TileKind::Ramp
    }

    pub fn clamp_pos(&self, p: FxVec2) -> FxVec2 {
        FxVec2 {
            x: p.x.clamp(Fx::from_ratio(1, 4), Fx::from_int(self.width) - Fx::from_ratio(1, 4)),
            y: p.y.clamp(Fx::from_ratio(1, 4), Fx::from_int(self.height) - Fx::from_ratio(1, 4)),
        }
    }
}

/// The first 1v1 map: "Meridian". 80x80, two high-ground mains connected to
/// a low-ground center, one ramp each, mineral line per main, blocked rock
/// clusters shaping the middle. 180°-rotation symmetric for fairness.
/// Deterministic by construction (pure function, no RNG).
pub fn meridian() -> Map {
    let w = 80;
    let h = 80;
    let mut map = Map {
        width: w,
        height: h,
        kind: vec![TileKind::Ground; (w * h) as usize],
        elev: vec![0u8; (w * h) as usize],
        starts: vec![TilePos::new(14, 14), TilePos::new(w - 15, h - 15)],
        minerals: Vec::new(),
        geysers: Vec::new(),
    };

    // Border wall.
    for x in 0..w {
        for y in 0..h {
            if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
                map.kind[(y * w + x) as usize] = TileKind::Blocked;
            }
        }
    }

    // Main plateaus: 24x24 in NW and SE corners at elevation 1, ringed by
    // cliff except where the ramp cuts through.
    let plateau = |map: &mut Map, ox: i32, oy: i32| {
        for x in ox..ox + 24 {
            for y in oy..oy + 24 {
                if map.in_bounds(x, y) {
                    let i = map.idx(x, y);
                    map.elev[i] = 1;
                }
            }
        }
        // Cliff ring just outside the plateau edge. The ring sits at plateau
        // elevation so it renders as the cliff band itself (impassable rock
        // on top, wall faces dropping to the low ground beyond).
        for x in ox - 1..=ox + 24 {
            for y in oy - 1..=oy + 24 {
                let inside = x >= ox && x < ox + 24 && y >= oy && y < oy + 24;
                if !inside && map.in_bounds(x, y) {
                    let i = map.idx(x, y);
                    map.kind[i] = TileKind::Blocked;
                    map.elev[i] = 1;
                }
            }
        }
    };
    plateau(&mut map, 1, 1);
    plateau(&mut map, w - 25, h - 25);

    // Ramps: cut a 4-wide opening in each plateau's inner corner cliff and
    // mark it as ramp, connecting elevation 1 to 0.
    let ramp = |map: &mut Map, tiles: &[(i32, i32)]| {
        for &(x, y) in tiles {
            let i = map.idx(x, y);
            map.kind[i] = TileKind::Ramp;
            map.elev[i] = 0;
        }
    };
    // NW ramp: opening on the SE corner of the NW plateau.
    ramp(&mut map, &[(24, 20), (24, 21), (24, 22), (24, 23), (25, 20), (25, 21), (25, 22), (25, 23)]);
    // SE ramp, mirrored (180° rotation: (x,y) -> (w-1-x, h-1-y)).
    ramp(
        &mut map,
        &[
            (w - 25, h - 21),
            (w - 25, h - 22),
            (w - 25, h - 23),
            (w - 25, h - 24),
            (w - 26, h - 21),
            (w - 26, h - 22),
            (w - 26, h - 23),
            (w - 26, h - 24),
        ],
    );

    // Center rock clusters: create lanes through the middle. Symmetric.
    let rocks = [
        (38, 26, 6, 4),
        (26, 44, 4, 6),
        (48, 38, 5, 5),
    ];
    for &(rx, ry, rw, rh) in &rocks {
        for x in rx..rx + rw {
            for y in ry..ry + rh {
                if map.in_bounds(x, y) {
                    let i = map.idx(x, y);
                    map.kind[i] = TileKind::Blocked;
                }
                // Mirrored twin.
                let (mx, my) = (w - 1 - x, h - 1 - y);
                if map.in_bounds(mx, my) {
                    let i = map.idx(mx, my);
                    map.kind[i] = TileKind::Blocked;
                }
            }
        }
    }

    // Mineral lines: 8 patches per main, arced around the HQ site.
    const PATCH: i32 = 1500;
    let line_nw = [(6, 4), (5, 6), (4, 8), (4, 10), (4, 12), (4, 14), (5, 16), (6, 18)];
    for &(x, y) in &line_nw {
        map.minerals.push((TilePos::new(x, y), PATCH));
        map.minerals.push((TilePos::new(w - 1 - x, h - 1 - y), PATCH));
    }
    // Mineral tiles stay Ground terrain; State adds them as dynamic blockers
    // so the tile opens up when a patch mines out.

    // One plasma geyser per main (2x2), on the far side of the HQ from the
    // mineral line. Mirrored for the SE main.
    const GAS: i32 = 2500;
    map.geysers.push((TilePos::new(17, 4), GAS));
    map.geysers.push((TilePos::new(w - 1 - 18, h - 1 - 5), GAS));

    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meridian_is_sane() {
        let m = meridian();
        assert_eq!(m.starts.len(), 2);
        assert_eq!(m.minerals.len(), 16);
        // Starts are walkable, on high ground.
        for s in &m.starts {
            assert!(m.walkable(s.x, s.y));
            assert_eq!(m.elev_at(s.x, s.y), 1);
        }
        // Ramp connects heights.
        assert!(m.traversable(23, 21, 24, 21)); // plateau -> ramp
        assert!(m.traversable(24, 21, 26, 21) || m.traversable(25, 21, 26, 21)); // ramp -> low ground
    }

    #[test]
    fn meridian_is_symmetric() {
        let m = meridian();
        // 180° rotational symmetry of walkability + elevation.
        for x in 0..m.width {
            for y in 0..m.height {
                let (mx, my) = (m.width - 1 - x, m.height - 1 - y);
                assert_eq!(m.walkable(x, y), m.walkable(mx, my), "walk asym at {},{}", x, y);
                assert_eq!(m.elev_at(x, y), m.elev_at(mx, my), "elev asym at {},{}", x, y);
            }
        }
    }
}
