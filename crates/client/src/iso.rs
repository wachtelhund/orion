//! The fixed 2:1 isometric projection. World coordinates are sim tiles
//! (floats here — renderer only); iso coordinates are pixels at zoom 1.

pub const TILE_HALF_W: f32 = 16.0;
pub const TILE_HALF_H: f32 = 8.0;
/// Vertical pixel offset per elevation level (screen units at zoom 1).
/// The cliff-face sprites in atlas.rs are painted 2x this tall (canvas px
/// are half screen units at the texel-perfect zoom of 2).
pub const ELEV_PX: f32 = 12.0;

#[inline]
pub fn world_to_iso(wx: f32, wy: f32) -> (f32, f32) {
    ((wx - wy) * TILE_HALF_W, (wx + wy) * TILE_HALF_H)
}

#[inline]
pub fn iso_to_world(ix: f32, iy: f32) -> (f32, f32) {
    (
        ix / (2.0 * TILE_HALF_W) + iy / (2.0 * TILE_HALF_H),
        iy / (2.0 * TILE_HALF_H) - ix / (2.0 * TILE_HALF_W),
    )
}

#[derive(Clone, Copy)]
pub struct Camera {
    /// Center of the view in iso pixels (zoom 1).
    pub cx: f32,
    pub cy: f32,
    pub zoom: f32,
    pub screen_w: f32,
    pub screen_h: f32,
}

impl Camera {
    pub fn world_to_screen(&self, wx: f32, wy: f32) -> (f32, f32) {
        let (ix, iy) = world_to_iso(wx, wy);
        (
            (ix - self.cx) * self.zoom + self.screen_w * 0.5,
            (iy - self.cy) * self.zoom + self.screen_h * 0.5,
        )
    }

    pub fn screen_to_world(&self, sx: f32, sy: f32) -> (f32, f32) {
        let ix = (sx - self.screen_w * 0.5) / self.zoom + self.cx;
        let iy = (sy - self.screen_h * 0.5) / self.zoom + self.cy;
        iso_to_world(ix, iy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_roundtrip() {
        for &(x, y) in &[(0.0, 0.0), (12.5, 3.25), (79.0, 79.0), (3.0, 40.0)] {
            let (ix, iy) = world_to_iso(x, y);
            let (bx, by) = iso_to_world(ix, iy);
            assert!((bx - x).abs() < 1e-4 && (by - y).abs() < 1e-4);
        }
    }

    #[test]
    fn camera_roundtrip() {
        let cam = Camera { cx: 100.0, cy: 50.0, zoom: 1.7, screen_w: 1440.0, screen_h: 900.0 };
        let (sx, sy) = cam.world_to_screen(20.0, 30.0);
        let (wx, wy) = cam.screen_to_world(sx, sy);
        assert!((wx - 20.0).abs() < 1e-3 && (wy - 30.0).abs() < 1e-3);
    }
}
