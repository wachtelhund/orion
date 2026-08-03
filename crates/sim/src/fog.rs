//! Per-player fog of war. Three states: unexplored, explored (seen before,
//! not currently), visible. Visibility is recomputed every tick by stamping
//! precomputed sight circles; the high-ground rule is: a viewer sees a tile
//! only if the tile's elevation is <= the viewer's elevation.

use crate::map::{Map, TilePos};
use crate::state::{EntityKind, State};

pub const MAX_SIGHT: u32 = 12;

#[derive(Clone)]
pub struct FogGrid {
    pub explored: Vec<bool>,
    pub vis: Vec<bool>,
}

impl FogGrid {
    pub fn new(w: i32, h: i32) -> FogGrid {
        let n = (w * h) as usize;
        FogGrid { explored: vec![false; n], vis: vec![false; n] }
    }

    #[inline]
    pub fn visible(&self, map: &Map, t: TilePos) -> bool {
        map.in_bounds(t.x, t.y) && self.vis[map.idx(t.x, t.y)]
    }

    #[inline]
    pub fn explored(&self, map: &Map, t: TilePos) -> bool {
        map.in_bounds(t.x, t.y) && self.explored[map.idx(t.x, t.y)]
    }
}

/// Offsets within a circle of radius r, precomputed once (pure integers).
pub fn circle_offsets(r: u32) -> Vec<(i32, i32)> {
    let r = r as i32;
    let mut out = Vec::new();
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy <= r * r {
                out.push((dx, dy));
            }
        }
    }
    out
}

pub struct SightMasks {
    masks: Vec<Vec<(i32, i32)>>,
}

impl SightMasks {
    pub fn build() -> SightMasks {
        SightMasks { masks: (0..=MAX_SIGHT).map(circle_offsets).collect() }
    }

    pub fn get(&self, r: u32) -> &[(i32, i32)] {
        &self.masks[(r.min(MAX_SIGHT)) as usize]
    }
}

impl State {
    pub fn update_fog(&mut self) {
        // Lazily built once; deterministic pure function of radius.
        thread_local! {
            static MASKS: SightMasks = SightMasks::build();
        }
        for f in &mut self.fog {
            f.vis.iter_mut().for_each(|v| *v = false);
        }
        MASKS.with(|masks| {
            for i in 0..self.entities.len() {
                let e = &self.entities[i];
                if !e.alive || e.owner == crate::state::NEUTRAL {
                    continue;
                }
                if e.kind == EntityKind::Resource {
                    continue;
                }
                let sight = self.sight_of(i as u32);
                let t = TilePos::of(e.pos);
                let flying = e.kind == EntityKind::Unit
                    && self.data.units[e.def as usize].fly;
                let viewer_elev =
                    if flying { u8::MAX } else { self.map.elev_at(t.x, t.y) };
                let fog = &mut self.fog[e.owner as usize];
                for &(dx, dy) in masks.get(sight) {
                    let (x, y) = (t.x + dx, t.y + dy);
                    if !self.map.in_bounds(x, y) {
                        continue;
                    }
                    if self.map.elev_at(x, y) > viewer_elev {
                        continue; // low ground cannot see up
                    }
                    let idx = self.map.idx(x, y);
                    fog.vis[idx] = true;
                    fog.explored[idx] = true;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_radius_zero_is_self() {
        assert_eq!(circle_offsets(0), vec![(0, 0)]);
    }

    #[test]
    fn circle_grows() {
        assert!(circle_offsets(9).len() > circle_offsets(5).len());
    }
}
