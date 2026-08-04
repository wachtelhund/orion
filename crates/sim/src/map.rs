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
    /// Suggested expansion HQ origins (3x3), nearest-to-start first per
    /// player. Empty on maps without expansions.
    pub expansions: Vec<TilePos>,
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

/// All shipping maps, in menu order. Names are the replay/net identifiers —
/// never rename one that shipped.
pub const MAP_NAMES: &[&str] = &["meridian", "caverns"];

pub fn by_name(name: &str) -> Option<Map> {
    match name {
        "meridian" => Some(meridian()),
        "caverns" => Some(caverns()),
        _ => None,
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
        expansions: Vec::new(),
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
    // 1000/patch mines out a single-base economy by ~minute 15 — turtle
    // standoffs then resolve on remaining army instead of the game clock.
    const PATCH: i32 = 1000;
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

/// Second 1v1 map: "Caverns", an homage to SC2's Xel'Naga Caverns. NE/SW
/// high-ground mains, a NATURAL expansion below each ramp (first map with
/// expansions), an open low-ground center split by cavern rock formations
/// into a short middle path and two long edge routes. 180°-rotation
/// symmetric; every feature is pushed together with its mirrored twin.
pub fn caverns() -> Map {
    let w = 80;
    let h = 80;
    let mut map = Map {
        width: w,
        height: h,
        kind: vec![TileKind::Ground; (w * h) as usize],
        elev: vec![0u8; (w * h) as usize],
        // P0 in the SW main, P1 in the NE (mirror of P0).
        starts: vec![TilePos::new(11, 67), TilePos::new(w - 12, h - 68)],
        minerals: Vec::new(),
        geysers: Vec::new(),
        expansions: Vec::new(),
    };

    for x in 0..w {
        for y in 0..h {
            if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
                map.kind[(y * w + x) as usize] = TileKind::Blocked;
            }
        }
    }

    // Main plateaus: 22x22, SW and NE corners, elev 1 with a cliff ring.
    let plateau = |map: &mut Map, ox: i32, oy: i32| {
        for x in ox..ox + 22 {
            for y in oy..oy + 22 {
                if map.in_bounds(x, y) {
                    let i = map.idx(x, y);
                    map.elev[i] = 1;
                }
            }
        }
        for x in ox - 1..=ox + 22 {
            for y in oy - 1..=oy + 22 {
                let inside = x >= ox && x < ox + 22 && y >= oy && y < oy + 22;
                if !inside && map.in_bounds(x, y) {
                    let i = map.idx(x, y);
                    map.kind[i] = TileKind::Blocked;
                    map.elev[i] = 1;
                }
            }
        }
    };
    plateau(&mut map, 1, h - 23); // SW: (1..22, 57..78)
    plateau(&mut map, w - 23, 1); // NE: (57..78, 1..22)

    let ramp = |map: &mut Map, tiles: &[(i32, i32)]| {
        for &(x, y) in tiles {
            let i = map.idx(x, y);
            map.kind[i] = TileKind::Ramp;
            map.elev[i] = 0;
            // Mirrored twin.
            let (mx, my) = (map.width - 1 - x, map.height - 1 - y);
            let j = map.idx(mx, my);
            map.kind[j] = TileKind::Ramp;
            map.elev[j] = 0;
        }
    };
    // SW main ramp: opening on its north edge, toward the natural.
    ramp(&mut map, &[(8, 56), (9, 56), (10, 56), (11, 56), (8, 55), (9, 55), (10, 55), (11, 55)]);

    // Natural expansions on the low ground past each ramp: an HQ slot with
    // its own mineral arc + geyser. SW natural sits at (10..12, 44..46).
    map.expansions.push(TilePos::new(10, 44));
    map.expansions.push(TilePos::new(w - 3 - 10, h - 3 - 44));

    const PATCH: i32 = 1000;
    let patches = |map: &mut Map, line: &[(i32, i32)], amount: i32| {
        for &(x, y) in line {
            map.minerals.push((TilePos::new(x, y), amount));
            map.minerals.push((TilePos::new(map.width - 1 - x, map.height - 1 - y), amount));
        }
    };
    // Main mineral line: arced into the SW corner behind the HQ.
    patches(
        &mut map,
        &[(4, 61), (4, 63), (4, 65), (4, 67), (4, 69), (5, 71), (6, 73), (8, 74)],
        PATCH,
    );
    // Natural mineral line: hugging the map edge west of the natural HQ.
    patches(&mut map, &[(4, 41), (4, 43), (4, 45), (4, 47), (5, 49), (6, 51)], PATCH);

    const GAS: i32 = 2500;
    let geysers = |map: &mut Map, origins: &[(i32, i32)]| {
        for &(x, y) in origins {
            map.geysers.push((TilePos::new(x, y), GAS));
            map.geysers.push((TilePos::new(map.width - 2 - x, map.height - 2 - y), GAS));
        }
    };
    geysers(&mut map, &[(16, 74), (15, 44)]); // main + natural

    // Cavern rock formations: split the center into a tight middle lane
    // and two wide edge routes (NW and SE corridors stay open).
    let rocks = [
        (34, 34, 12, 3), // upper jaw of the central cavern
        (34, 43, 12, 3), // lower jaw (mirrored twin lands symmetric)
        (24, 24, 6, 6),  // inner-corner boulders shaping the diagonals
        (56, 40, 4, 8),  // east flank pinch
    ];
    for &(rx, ry, rw, rh) in &rocks {
        for x in rx..rx + rw {
            for y in ry..ry + rh {
                if map.in_bounds(x, y) && map.elev_at(x, y) == 0 {
                    let i = map.idx(x, y);
                    map.kind[i] = TileKind::Blocked;
                }
                let (mx, my) = (w - 1 - x, h - 1 - y);
                if map.in_bounds(mx, my) && map.elev_at(mx, my) == 0 {
                    let i = map.idx(mx, my);
                    map.kind[i] = TileKind::Blocked;
                }
            }
        }
    }

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
