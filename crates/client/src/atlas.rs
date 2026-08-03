//! Procedural sprite atlas — the entire art of the game, painted at startup.
//!
//! Style target: SC1-era isometric pixel art. Dark outlines, 3-4 tone
//! material ramps, dithered texture, light from the upper-left, hard team
//! color accents. Every sprite is painted per team so no runtime palette
//! tricks are needed.

use crate::font;

pub const ATLAS: u32 = 1024;

pub type Color = [u8; 4];

pub const TEAMS: [[u8; 3]; 2] = [[64, 128, 255], [255, 84, 64]];

// ---------------------------------------------------------------- canvas ----

pub struct Canvas {
    pub w: i32,
    pub h: i32,
    pub px: Vec<[u8; 4]>,
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Canvas {
        Canvas { w, h, px: vec![[0, 0, 0, 0]; (w * h) as usize] }
    }

    #[inline]
    pub fn set(&mut self, x: i32, y: i32, c: Color) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            self.px[(y * self.w + x) as usize] = c;
        }
    }

    #[inline]
    pub fn get(&self, x: i32, y: i32) -> Color {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            self.px[(y * self.w + x) as usize]
        } else {
            [0, 0, 0, 0]
        }
    }

    pub fn blend(&mut self, x: i32, y: i32, c: Color) {
        if c[3] == 255 {
            self.set(x, y, c);
            return;
        }
        let dst = self.get(x, y);
        let a = c[3] as u32;
        let na = 255 - a;
        let out = [
            ((c[0] as u32 * a + dst[0] as u32 * na) / 255) as u8,
            ((c[1] as u32 * a + dst[1] as u32 * na) / 255) as u8,
            ((c[2] as u32 * a + dst[2] as u32 * na) / 255) as u8,
            (a + dst[3] as u32 * na / 255).min(255) as u8,
        ];
        self.set(x, y, out);
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Color) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.blend(xx, yy, c);
            }
        }
    }

    pub fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, c: Color) {
        for y in (cy - ry - 1.0) as i32..=(cy + ry + 1.0) as i32 {
            for x in (cx - rx - 1.0) as i32..=(cx + rx + 1.0) as i32 {
                let dx = (x as f32 + 0.5 - cx) / rx;
                let dy = (y as f32 + 0.5 - cy) / ry;
                if dx * dx + dy * dy <= 1.0 {
                    self.blend(x, y, c);
                }
            }
        }
    }

    /// Ellipse with baked top-left lighting: three-tone ramp.
    pub fn ellipse_shaded(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, base: [u8; 3]) {
        for y in (cy - ry - 1.0) as i32..=(cy + ry + 1.0) as i32 {
            for x in (cx - rx - 1.0) as i32..=(cx + rx + 1.0) as i32 {
                let dx = (x as f32 + 0.5 - cx) / rx;
                let dy = (y as f32 + 0.5 - cy) / ry;
                let d = dx * dx + dy * dy;
                if d <= 1.0 {
                    let lit = -dx * 0.5 - dy * 0.8; // light from upper-left
                    let f = if lit > 0.25 && d < 0.8 {
                        1.25
                    } else if lit < -0.3 || d > 0.85 {
                        0.72
                    } else {
                        1.0
                    };
                    self.blend(x, y, scale(base, f));
                }
            }
        }
    }

    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, thick: f32, c: Color) {
        let dx = x1 - x0;
        let dy = y1 - y0;
        let len = (dx * dx + dy * dy).sqrt().max(0.001);
        let steps = (len * 2.0) as i32 + 1;
        for s in 0..=steps {
            let f = s as f32 / steps as f32;
            let px = x0 + dx * f;
            let py = y0 + dy * f;
            let r = thick * 0.5;
            for yy in (py - r) as i32..=(py + r) as i32 {
                for xx in (px - r) as i32..=(px + r) as i32 {
                    let ddx = xx as f32 + 0.5 - px;
                    let ddy = yy as f32 + 0.5 - py;
                    if ddx * ddx + ddy * ddy <= r * r + 0.3 {
                        self.blend(xx, yy, c);
                    }
                }
            }
        }
    }

    /// Darken opaque pixels that touch transparency — SC-style sprite outline.
    pub fn outline(&mut self, dark: Color) {
        let orig = self.px.clone();
        for y in 0..self.h {
            for x in 0..self.w {
                let p = orig[(y * self.w + x) as usize];
                if p[3] < 40 {
                    continue;
                }
                let mut edge = false;
                for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= self.w || ny >= self.h {
                        edge = true;
                    } else if orig[(ny * self.w + nx) as usize][3] < 40 {
                        edge = true;
                    }
                }
                if edge {
                    self.set(x, y, dark);
                }
            }
        }
    }
}

pub fn scale(c: [u8; 3], f: f32) -> Color {
    [
        (c[0] as f32 * f).min(255.0) as u8,
        (c[1] as f32 * f).min(255.0) as u8,
        (c[2] as f32 * f).min(255.0) as u8,
        255,
    ]
}

pub fn rgba(c: [u8; 3]) -> Color {
    [c[0], c[1], c[2], 255]
}

/// Deterministic pixel hash for texture noise.
pub fn hash2(x: i32, y: i32, salt: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(374761393) ^ (y as u32).wrapping_mul(668265263) ^ salt;
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    h ^ (h >> 16)
}

// ---------------------------------------------------------- atlas layout ----

#[derive(Clone, Copy, Debug, Default)]
pub struct Region {
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
    pub w: u32,
    pub h: u32,
}

/// Shelf packer writing canvases into the atlas pixel buffer.
struct Packer {
    px: Vec<u8>,
    cx: u32,
    cy: u32,
    row_h: u32,
}

impl Packer {
    fn new() -> Packer {
        Packer { px: vec![0; (ATLAS * ATLAS * 4) as usize], cx: 0, cy: 0, row_h: 0 }
    }

    fn place(&mut self, c: &Canvas) -> Region {
        let (w, h) = (c.w as u32, c.h as u32);
        if self.cx + w + 2 > ATLAS {
            self.cx = 0;
            self.cy += self.row_h + 2;
            self.row_h = 0;
        }
        assert!(self.cy + h + 2 <= ATLAS, "atlas full");
        let (ox, oy) = (self.cx, self.cy);
        for y in 0..h {
            for x in 0..w {
                let p = c.px[(y * w + x) as usize];
                let i = (((oy + y) * ATLAS + ox + x) * 4) as usize;
                self.px[i..i + 4].copy_from_slice(&p);
            }
        }
        self.cx += w + 2;
        self.row_h = self.row_h.max(h);
        let a = ATLAS as f32;
        Region {
            u0: (ox as f32 + 0.01) / a,
            v0: (oy as f32 + 0.01) / a,
            u1: (ox as f32 + w as f32 - 0.01) / a,
            v1: (oy as f32 + h as f32 - 0.01) / a,
            w,
            h,
        }
    }
}

/// Everything the renderer can draw.
pub struct SpriteBook {
    pub white: Region,
    pub circle: Region,
    pub ring: Region,
    pub diamond: Region,
    pub diamond_outline: Region,
    // terrain
    pub ground_low: [Region; 4],
    pub ground_high: [Region; 4],
    pub ramp: [Region; 2],
    pub rock: [Region; 4],
    pub cliff_left: Region,
    pub cliff_right: Region,
    // world objects
    pub minerals: [Region; 3],
    pub geyser: Region,
    /// [unit_type][team][facing][frame]. Types: worker, trooper, vanguard,
    /// breaker, skywing, stormcaller, breaker-sieged.
    pub units: Vec<Region>,
    /// [building_type][team]: hq, depot, barracks, condenser, forge, aerie,
    /// archive.
    pub buildings: Vec<Region>,
    pub building_px_h: [u32; 7],
    // effects
    pub flash: Region,
    pub spark: Region,
    pub blast_ring: Region,
    pub corpse: Region,
    pub rubble: Region,
    // font
    glyphs: Vec<(char, Region)>,
}

pub const N_FACINGS: usize = 8;
pub const N_FRAMES: usize = 2;

impl SpriteBook {
    pub fn unit(&self, unit_type: usize, team: usize, facing: usize, frame: usize) -> Region {
        self.units[((unit_type * 2 + team) * N_FACINGS + facing) * N_FRAMES + frame]
    }

    pub fn building(&self, b_type: usize, team: usize) -> Region {
        self.buildings[b_type * 2 + team]
    }

    pub fn glyph(&self, c: char) -> Option<Region> {
        let up = c.to_ascii_uppercase();
        self.glyphs.iter().find(|(g, _)| *g == up).map(|(_, r)| *r)
    }
}

/// Cliff wall height in canvas px = 2 * iso::ELEV_PX (canvas is 2x screen
/// resolution at zoom 1).
pub const CLIFF_H: i32 = 24;

pub fn build() -> (Vec<u8>, SpriteBook) {
    let mut p = Packer::new();

    // Basics.
    let mut c = Canvas::new(8, 8);
    c.rect(0, 0, 8, 8, [255, 255, 255, 255]);
    let white = p.place(&c);

    let mut c = Canvas::new(32, 32);
    c.ellipse(16.0, 16.0, 15.0, 15.0, [255, 255, 255, 255]);
    let circle = p.place(&c);

    let mut c = Canvas::new(32, 32);
    for y in 0..32 {
        for x in 0..32 {
            let dx = x as f32 + 0.5 - 16.0;
            let dy = y as f32 + 0.5 - 16.0;
            let d = (dx * dx + dy * dy).sqrt();
            if d <= 15.5 && d >= 12.5 {
                c.set(x, y, [255, 255, 255, 255]);
            }
        }
    }
    let ring = p.place(&c);

    let diamond = p.place(&diamond_canvas(64, 32, [255, 255, 255, 255], false));
    let diamond_outline = p.place(&diamond_canvas(64, 32, [255, 255, 255, 255], true));

    // Terrain.
    let ground_low = std::array::from_fn(|i| p.place(&ground_tile(i as u32, GROUND_LOW)));
    let ground_high = std::array::from_fn(|i| p.place(&ground_tile(i as u32 + 8, GROUND_HIGH)));
    let ramp = std::array::from_fn(|i| p.place(&ramp_tile(i as u32)));
    let rock = std::array::from_fn(|i| p.place(&rock_tile(i as u32)));
    let cliff_left = p.place(&cliff_face(true));
    let cliff_right = p.place(&cliff_face(false));

    // Minerals + geyser.
    let minerals = std::array::from_fn(|i| p.place(&mineral_cluster(i as u32)));
    let geyser = p.place(&paint_geyser());

    // Units: [type][team][facing][frame].
    let mut units = Vec::new();
    for unit_type in 0..7 {
        for team in 0..2 {
            for facing in 0..N_FACINGS {
                for frame in 0..N_FRAMES {
                    let c = match unit_type {
                        0 => paint_worker(facing, frame, TEAMS[team]),
                        1 => paint_trooper(facing, frame, TEAMS[team]),
                        2 => paint_vanguard(facing, frame, TEAMS[team]),
                        3 => paint_breaker(facing, frame, TEAMS[team]),
                        4 => paint_skywing(facing, frame, TEAMS[team]),
                        5 => paint_stormcaller(facing, frame, TEAMS[team]),
                        _ => paint_breaker_sieged(facing, frame, TEAMS[team]),
                    };
                    units.push(p.place(&c));
                }
            }
        }
    }

    // Buildings: [type][team].
    let mut buildings = Vec::new();
    let mut building_px_h = [0u32; 7];
    for b_type in 0..7 {
        for team in 0..2 {
            let c = match b_type {
                0 => paint_hq(TEAMS[team]),
                1 => paint_pylon(TEAMS[team]),
                2 => paint_barracks(TEAMS[team]),
                3 => paint_condenser(TEAMS[team]),
                4 => paint_forge(TEAMS[team]),
                5 => paint_aerie(TEAMS[team]),
                _ => paint_archive(TEAMS[team]),
            };
            building_px_h[b_type] = c.h as u32;
            buildings.push(p.place(&c));
        }
    }

    // Effects.
    let flash = p.place(&star_flash());
    let spark = {
        let mut c = Canvas::new(7, 7);
        c.ellipse(3.5, 3.5, 3.0, 3.0, [255, 240, 160, 255]);
        c.ellipse(3.5, 3.5, 1.6, 1.6, [255, 255, 255, 255]);
        p.place(&c)
    };
    let blast_ring = {
        let mut c = Canvas::new(24, 24);
        for y in 0..24 {
            for x in 0..24 {
                let dx = x as f32 + 0.5 - 12.0;
                let dy = (y as f32 + 0.5 - 12.0) * 2.0;
                let d = (dx * dx + dy * dy).sqrt();
                if d <= 11.5 && d >= 8.5 {
                    c.set(x, y, [255, 200, 120, 255]);
                }
            }
        }
        p.place(&c)
    };
    let corpse = {
        let mut c = Canvas::new(20, 10);
        c.ellipse_shaded(10.0, 5.0, 9.0, 4.0, [52, 46, 44]);
        c.outline([20, 18, 18, 255]);
        p.place(&c)
    };
    let rubble = {
        let mut c = Canvas::new(48, 28);
        for k in 0..14 {
            let h = hash2(k, 7, 99);
            let x = 6.0 + (h % 36) as f32;
            let y = 6.0 + ((h >> 8) % 16) as f32;
            let r = 2.0 + ((h >> 16) % 4) as f32;
            c.ellipse_shaded(x, y, r, r * 0.6, [70, 64, 60]);
        }
        c.outline([24, 22, 20, 255]);
        p.place(&c)
    };

    // Font.
    let mut glyphs = Vec::new();
    for ch in " ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789:/-.%+!(),".chars() {
        if let Some(rows) = font::glyph(ch) {
            let mut c = Canvas::new(font::GLYPH_W as i32, font::GLYPH_H as i32);
            for (ry, row) in rows.iter().enumerate() {
                for (rx, bit) in row.chars().enumerate() {
                    if bit == '1' {
                        c.set(rx as i32, ry as i32, [255, 255, 255, 255]);
                    }
                }
            }
            glyphs.push((ch, p.place(&c)));
        }
    }

    let book = SpriteBook {
        white,
        circle,
        ring,
        diamond,
        diamond_outline,
        ground_low,
        ground_high,
        ramp,
        rock,
        cliff_left,
        cliff_right,
        minerals,
        geyser,
        units,
        buildings,
        building_px_h,
        flash,
        spark,
        blast_ring,
        corpse,
        rubble,
        glyphs,
    };
    (p.px, book)
}

// ---------------------------------------------------------------- shapes ----

fn diamond_canvas(w: i32, h: i32, c: Color, outline_only: bool) -> Canvas {
    let mut cv = Canvas::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let dx = (x as f32 + 0.5 - w as f32 / 2.0) / (w as f32 / 2.0);
            let dy = (y as f32 + 0.5 - h as f32 / 2.0) / (h as f32 / 2.0);
            let d = dx.abs() + dy.abs();
            let inside = if outline_only { d <= 1.0 && d >= 0.82 } else { d <= 1.0 };
            if inside {
                cv.set(x, y, c);
            }
        }
    }
    cv
}

// --------------------------------------------------------------- terrain ----

const GROUND_LOW: [[u8; 3]; 3] = [[96, 84, 66], [83, 72, 57], [108, 96, 76]];
const GROUND_HIGH: [[u8; 3]; 3] = [[125, 114, 92], [110, 100, 80], [140, 130, 106]];

/// Dithered dirt diamond. Variants differ by noise salt; a few get pebbles.
fn ground_tile(salt: u32, ramp_colors: [[u8; 3]; 3]) -> Canvas {
    let mut c = Canvas::new(64, 32);
    for y in 0..32 {
        for x in 0..64 {
            let dx = (x as f32 + 0.5 - 32.0) / 32.0;
            let dy = (y as f32 + 0.5 - 16.0) / 16.0;
            if dx.abs() + dy.abs() > 1.0 {
                continue;
            }
            let h = hash2(x, y, salt);
            let t = match h % 16 {
                0..=8 => ramp_colors[0],
                9..=13 => ramp_colors[1],
                _ => ramp_colors[2],
            };
            c.set(x, y, rgba(t));
        }
    }
    // Pebble clusters on some variants.
    if salt % 3 == 0 {
        for k in 0..3 {
            let h = hash2(k as i32, salt as i32, 5);
            let px = 12.0 + (h % 40) as f32;
            let py = 8.0 + ((h >> 8) % 16) as f32;
            let dx = (px - 32.0) / 32.0;
            let dy = (py - 16.0) / 16.0;
            if dx.abs() + dy.abs() < 0.7 {
                c.ellipse(px, py, 2.0, 1.2, rgba(scale_rgb(ramp_colors[1], 0.7)));
                c.ellipse(px - 0.5, py - 0.5, 1.0, 0.6, rgba(scale_rgb(ramp_colors[2], 1.1)));
            }
        }
    }
    c
}

fn scale_rgb(c: [u8; 3], f: f32) -> [u8; 3] {
    [
        (c[0] as f32 * f).min(255.0) as u8,
        (c[1] as f32 * f).min(255.0) as u8,
        (c[2] as f32 * f).min(255.0) as u8,
    ]
}

/// Ramp: mid-tone with chevron striations.
fn ramp_tile(salt: u32) -> Canvas {
    let mut c = Canvas::new(64, 32);
    let base = [112, 100, 78];
    for y in 0..32 {
        for x in 0..64 {
            let dx = (x as f32 + 0.5 - 32.0) / 32.0;
            let dy = (y as f32 + 0.5 - 16.0) / 16.0;
            if dx.abs() + dy.abs() > 1.0 {
                continue;
            }
            let h = hash2(x, y, salt + 40);
            let stripe = ((x + y * 2) / 6) % 2 == 0;
            let mut t = if stripe { scale_rgb(base, 0.88) } else { base };
            if h % 13 == 0 {
                t = scale_rgb(base, 1.12);
            }
            c.set(x, y, rgba(t));
        }
    }
    c
}

/// Impassable rock tile: dark cracked stone.
fn rock_tile(salt: u32) -> Canvas {
    let mut c = Canvas::new(64, 32);
    for y in 0..32 {
        for x in 0..64 {
            let dx = (x as f32 + 0.5 - 32.0) / 32.0;
            let dy = (y as f32 + 0.5 - 16.0) / 16.0;
            if dx.abs() + dy.abs() > 1.0 {
                continue;
            }
            let h = hash2(x, y, salt + 80);
            let t = match h % 16 {
                0..=7 => [58, 54, 56],
                8..=12 => [48, 45, 47],
                _ => [70, 66, 68],
            };
            c.set(x, y, rgba(t));
        }
    }
    // A boulder or two.
    for k in 0..2 {
        let h = hash2(k, salt as i32, 31);
        let px = 18.0 + (h % 28) as f32;
        let py = 10.0 + ((h >> 8) % 12) as f32;
        let dx = (px - 32.0) / 32.0;
        let dy = (py - 16.0) / 16.0;
        if dx.abs() + dy.abs() < 0.65 {
            c.ellipse_shaded(px, py, 5.0, 3.0, [86, 80, 82]);
        }
    }
    c
}

/// Cliff wall face hanging under a tile's lower-left or lower-right edge.
/// Canvas is 64x(32+CLIFF_H); the wall occupies one half, following the
/// diamond's bottom edge, extruded down by CLIFF_H. Rock strata + bright rim.
fn cliff_face(left: bool) -> Canvas {
    let mut c = Canvas::new(64, 32 + CLIFF_H);
    for x in 0..64i32 {
        let on_half = if left { x < 32 } else { x >= 32 };
        if !on_half {
            continue;
        }
        // Bottom edge of the diamond at this column.
        let dx = (x as f32 + 0.5 - 32.0) / 32.0;
        let edge_y = 16.0 + 16.0 * (1.0 - dx.abs());
        for wy in 0..CLIFF_H {
            let y = edge_y as i32 + wy;
            let h = hash2(x, wy, if left { 7 } else { 8 });
            let strata = (wy / 4) % 2 == 0;
            let base: [u8; 3] = if left { [70, 62, 54] } else { [88, 78, 66] };
            let mut t = if strata { base } else { scale_rgb(base, 0.85) };
            if h % 11 == 0 {
                t = scale_rgb(base, 1.15);
            }
            if h % 17 == 0 {
                t = scale_rgb(base, 0.7);
            }
            // Bright rim where the wall meets the top surface.
            if wy == 0 {
                t = scale_rgb(base, 1.45);
            }
            if wy == CLIFF_H - 1 {
                t = scale_rgb(base, 0.55);
            }
            c.set(x, y, rgba(t));
        }
    }
    c
}

// -------------------------------------------------------------- minerals ----

fn mineral_cluster(salt: u32) -> Canvas {
    let mut c = Canvas::new(30, 26);
    let body = [64, 190, 220];
    let lite = [160, 240, 255];
    let dark = [30, 110, 150];
    let shards: [(f32, f32, f32, f32); 4] = [
        (9.0, 16.0, 4.0, 9.0),
        (16.0, 14.0, 5.0, 12.0),
        (22.0, 17.0, 3.5, 8.0),
        (13.0, 19.0, 3.0, 6.0),
    ];
    for (i, &(x, y, w, h)) in shards.iter().enumerate() {
        if (salt == 1 && i == 3) || (salt == 2 && i >= 2) {
            continue; // depleted variants
        }
        // Crystal: slanted quad + tip.
        let tip = (x + 1.5, y - h);
        for t in 0..=(h as i32 * 2) {
            let f = t as f32 / (h * 2.0);
            let cx = x + (tip.0 - x) * f;
            let cy = y + (tip.1 - y) * f;
            let ww = w * (1.0 - f * 0.7);
            for px in (cx - ww / 2.0) as i32..=(cx + ww / 2.0) as i32 {
                let facet = px as f32 > cx;
                let col = if facet { dark } else { body };
                c.blend(px, cy as i32, rgba(col));
            }
        }
        // Highlight edge.
        c.line(x - w * 0.3, y - 1.0, tip.0 - 0.5, tip.1 + 1.0, 1.0, rgba(lite));
    }
    c.outline([16, 40, 60, 255]);
    c
}

// ----------------------------------------------------------------- units ----

fn facing_vec(f: usize) -> (f32, f32) {
    let a = f as f32 * std::f32::consts::FRAC_PI_4;
    (a.cos(), a.sin())
}

const GUNMETAL: [u8; 3] = [104, 110, 122];
const GUNMETAL_DARK: [u8; 3] = [66, 70, 80];
const VISOR: [u8; 3] = [120, 235, 255];
const OUTLINE: Color = [16, 17, 22, 255];

/// Trooper: marine-ish infantry. 26x28.
fn paint_trooper(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(26, 28);
    let (dx, dy) = facing_vec(f);
    let cx = 13.0;
    let cy = 15.0;
    let gun_behind = dy < -0.3;

    let gun = |c: &mut Canvas| {
        let gx0 = cx + dx * 3.0;
        let gy0 = cy - 1.0 + dy * 1.5;
        c.line(gx0, gy0, gx0 + dx * 9.0, gy0 + dy * 5.0, 2.0, rgba(GUNMETAL_DARK));
        c.line(gx0 + dx * 7.0, gy0 + dy * 3.6, gx0 + dx * 9.5, gy0 + dy * 5.2, 1.5, rgba([140, 145, 155]));
    };
    if gun_behind {
        gun(&mut c);
    }

    // Legs (walk cycle: alternate 1px lift).
    let lift = if frame == 0 { 0 } else { 1 };
    c.rect(9, 20 + lift, 3, 6 - lift, rgba(GUNMETAL_DARK));
    c.rect(14, 21 - lift, 3, 6 + lift - 1, rgba(GUNMETAL_DARK));
    // Feet.
    c.rect(8, 25, 4, 2, rgba([50, 52, 60]));
    c.rect(14, 25, 4, 2, rgba([50, 52, 60]));

    // Torso armor.
    c.ellipse_shaded(cx, cy, 5.0, 6.0, GUNMETAL);
    // Team chest plate, pushed toward the facing.
    c.ellipse(cx + dx * 2.0, cy + dy * 1.5, 2.6, 2.4, rgba(team));
    // Shoulder pauldrons.
    c.ellipse_shaded(cx - 6.0, cy - 2.0, 2.6, 2.4, scale_rgb(GUNMETAL, 1.05));
    c.ellipse_shaded(cx + 6.0, cy - 2.0, 2.6, 2.4, scale_rgb(GUNMETAL, 0.9));
    c.ellipse(cx - 6.0, cy - 3.0, 1.4, 0.9, rgba(team));
    c.ellipse(cx + 6.0, cy - 3.0, 1.4, 0.9, rgba(team));

    // Helmet + visor.
    c.ellipse_shaded(cx, cy - 7.5, 3.4, 3.2, scale_rgb(GUNMETAL, 1.1));
    if dy > -0.5 {
        c.ellipse(cx + dx * 1.8, cy - 7.2 + dy * 0.8, 1.8, 1.0, rgba(VISOR));
    } else {
        // Backpack when facing away.
        c.rect(11, 6, 4, 3, rgba(GUNMETAL_DARK));
    }

    if !gun_behind {
        gun(&mut c);
    }
    c.outline(OUTLINE);
    c
}

/// Fabricator: worker drone with cab + arms. 24x24.
fn paint_worker(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(24, 24);
    let (dx, dy) = facing_vec(f);
    let cx = 12.0;
    let cy = 13.0;
    let bob = if frame == 0 { 0.0 } else { -0.7 };

    // Tracks/base.
    c.rect(7, 18, 10, 4, rgba([58, 60, 66]));
    c.rect(6, 19, 12, 2, rgba([44, 46, 52]));

    // Body pod.
    c.ellipse_shaded(cx, cy + bob, 6.0, 5.0, [176, 160, 84]); // industrial yellow
    c.ellipse(cx - dx * 2.0, cy + bob - dy * 1.5, 2.2, 1.8, rgba(team));
    // Cab window toward facing.
    if dy > -0.5 {
        c.ellipse(cx + dx * 2.6, cy + bob + dy * 1.8 - 1.0, 2.2, 1.6, rgba([150, 230, 250]));
    }
    // Arms toward facing.
    let ax = cx + dx * 5.0;
    let ay = cy + bob + dy * 3.0;
    c.line(cx + dx * 2.0, cy + bob + 1.0, ax + dx * 2.5, ay, 1.8, rgba(GUNMETAL_DARK));
    c.ellipse(ax + dx * 3.0, ay + dy * 1.0, 1.5, 1.3, rgba([120, 124, 132]));
    // Beacon.
    c.set(11, (cy + bob - 6.0) as i32, [255, 200, 80, 255]);

    c.outline(OUTLINE);
    c
}

/// Vanguard: heavy melee shock trooper. 32x32. Broad armored silhouette,
/// energy blades on both forearms, small crested helm.
fn paint_vanguard(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(32, 32);
    let (dx, dy) = facing_vec(f);
    let cx = 16.0;
    let cy = 17.0;
    let armor = scale_rgb(GUNMETAL, 0.92);

    let lift = if frame == 0 { 0 } else { 1 };
    // Wide-stance legs with armored greaves.
    c.rect(10, 23 + lift, 4, 6 - lift, rgba(GUNMETAL_DARK));
    c.rect(18, 24 - lift, 4, 6 + lift - 1, rgba(GUNMETAL_DARK));
    c.rect(9, 27, 6, 3, rgba([52, 54, 62]));
    c.rect(17, 28, 6, 3, rgba([52, 54, 62]));

    // Energy blades on both forearms (drawn behind when facing away).
    let blades = |c: &mut Canvas| {
        for side in [-1.0f32, 1.0] {
            // Perpendicular offset places one arm each side of the torso.
            let ox = -dy * side * 7.0;
            let oy = dx * side * 4.5;
            let px = cx + dx * 3.0 + ox;
            let py = cy + dy * 2.0 + oy;
            // Forearm block.
            c.ellipse_shaded(px, py, 2.4, 2.2, scale_rgb(GUNMETAL, 1.05));
            // Blade: thick team-colored edge with white-hot core.
            c.line(px + dx * 1.5, py + dy * 1.0, px + dx * 9.0, py + dy * 5.5, 2.6, rgba(team));
            c.line(px + dx * 2.5, py + dy * 1.6, px + dx * 8.5, py + dy * 5.2, 1.1, rgba([255, 255, 255]));
        }
    };
    let behind = dy < -0.3;
    if behind {
        blades(&mut c);
    }

    // Torso: broad armored dome with a belt line.
    c.ellipse_shaded(cx, cy, 7.5, 6.5, armor);
    c.rect((cx - 6.0) as i32, (cy + 2.0) as i32, 12, 2, rgba(scale_rgb(armor, 0.72)));
    // Team chest emblem, modest.
    c.ellipse(cx + dx * 2.2, cy - 1.0 + dy * 1.5, 2.4, 2.0, rgba(team));

    // Massive pauldrons with rim light + team stripe.
    c.ellipse_shaded(cx - 8.5, cy - 4.0, 4.0, 3.4, scale_rgb(GUNMETAL, 1.12));
    c.ellipse_shaded(cx + 8.5, cy - 4.0, 4.0, 3.4, scale_rgb(GUNMETAL, 0.88));
    c.rect((cx - 11.0) as i32, (cy - 5.5) as i32, 5, 2, rgba(team));
    c.rect((cx + 6.0) as i32, (cy - 5.5) as i32, 5, 2, rgba(team));

    // Compact helm with crest fin.
    c.ellipse_shaded(cx, cy - 9.0, 3.0, 2.8, GUNMETAL);
    c.rect((cx - 1.0) as i32, (cy - 14.0) as i32, 2, 5, rgba(team));
    if dy > -0.5 {
        c.ellipse(cx + dx * 1.6, cy - 8.8 + dy * 0.6, 1.7, 0.9, rgba([255, 170, 90]));
    } else {
        // Thruster pack when facing away.
        c.rect((cx - 2.0) as i32, (cy - 6.0) as i32, 4, 4, rgba(GUNMETAL_DARK));
    }

    if !behind {
        blades(&mut c);
    }
    c.outline(OUTLINE);
    c
}

/// Breaker: siege tank, mobile mode. Turret tracks facing. 34x30.
fn paint_breaker(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(34, 30);
    let (dx, dy) = facing_vec(f);
    let cx = 17.0;
    let cy = 17.0;
    // Treads: two dark tracks with moving notches.
    for side in [-1.0f32, 1.0] {
        let ty = cy + 5.0 + side * 2.5;
        c.rect(5, ty as i32, 24, 4, rgba([52, 54, 60]));
        for k in 0..6 {
            let notch = 6 + k * 4 + (frame as i32);
            c.rect(notch, ty as i32 + 1, 1, 2, rgba([80, 84, 92]));
        }
    }
    // Hull.
    c.ellipse_shaded(cx, cy + 1.0, 10.0, 5.0, scale_rgb(GUNMETAL, 0.95));
    c.rect((cx - 7.0) as i32, (cy + 3.0) as i32, 14, 2, rgba(team));
    // Turret + barrel toward facing.
    c.ellipse_shaded(cx, cy - 3.0, 5.0, 3.5, scale_rgb(GUNMETAL, 1.1));
    c.line(cx + dx * 3.0, cy - 3.5 + dy * 1.5, cx + dx * 13.0, cy - 3.5 + dy * 6.5, 2.4, rgba(GUNMETAL_DARK));
    c.line(cx + dx * 11.0, cy - 3.5 + dy * 5.5, cx + dx * 13.5, cy - 3.5 + dy * 6.7, 1.4, rgba([150, 155, 165]));
    c.ellipse(cx - dx * 1.0, cy - 4.5, 1.6, 1.1, rgba(team));
    c.outline(OUTLINE);
    c
}

/// Breaker deployed: stabilizer legs, elevated long barrel. 38x34.
fn paint_breaker_sieged(f: usize, _frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(38, 34);
    let (dx, dy) = facing_vec(f);
    let cx = 19.0;
    let cy = 19.0;
    // Stabilizer legs splayed to four corners.
    for (lx, ly) in [(-11.0f32, 6.0f32), (11.0, 6.0), (-8.0, 10.0), (8.0, 10.0)] {
        c.line(cx, cy + 2.0, cx + lx, cy + ly, 2.0, rgba(GUNMETAL_DARK));
        c.rect((cx + lx - 2.0) as i32, (cy + ly) as i32, 4, 2, rgba([50, 52, 60]));
    }
    // Raised hull.
    c.ellipse_shaded(cx, cy - 1.0, 9.0, 5.0, GUNMETAL);
    c.rect((cx - 6.0) as i32, (cy + 1.0) as i32, 12, 2, rgba(team));
    // Recoil shield + long barrel, slightly elevated.
    c.ellipse_shaded(cx - dx * 2.0, cy - 6.0, 4.5, 3.0, scale_rgb(GUNMETAL, 1.15));
    c.line(cx + dx * 2.0, cy - 7.0 + dy * 1.0, cx + dx * 17.0, cy - 9.5 + dy * 7.0, 2.6, rgba(GUNMETAL_DARK));
    c.line(cx + dx * 14.0, cy - 9.0 + dy * 5.8, cx + dx * 17.5, cy - 9.7 + dy * 7.1, 1.5, rgba([160, 165, 175]));
    c.ellipse(cx - dx * 3.0, cy - 7.5, 1.6, 1.1, rgba(team));
    c.outline(OUTLINE);
    c
}

/// Skywing: delta-wing gunship. Drawn along its facing. 36x30.
fn paint_skywing(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(36, 30);
    let (dx, dy) = facing_vec(f);
    // Screen-space squash for the iso look.
    let (dy_i, px, py) = (dy * 0.6, 18.0, 15.0);
    let nose = (px + dx * 9.0, py + dy_i * 9.0);
    let tail = (px - dx * 8.0, py - dy_i * 8.0);
    // Perpendicular for wings.
    let (wx, wy) = (-dy_i, dx * 0.6);
    // Wings: swept back from mid-fuselage toward tail, team colored.
    for side in [-1.0f32, 1.0] {
        let root = (px + dx * 2.0, py + dy_i * 2.0);
        let tip = (
            px - dx * 4.0 + wx * 11.0 * side,
            py - dy_i * 4.0 + wy * 11.0 * side,
        );
        c.line(root.0, root.1, tip.0, tip.1, 3.4, rgba(scale_rgb(GUNMETAL, 0.9)));
        c.line(
            root.0 - dx * 2.0 + wx * 4.0 * side,
            root.1 - dy_i * 2.0 + wy * 4.0 * side,
            tip.0,
            tip.1,
            1.4,
            rgba(team),
        );
    }
    // Fuselage.
    c.line(nose.0, nose.1, tail.0, tail.1, 4.2, rgba(GUNMETAL));
    c.line(nose.0, nose.1, px + dx * 4.0, py + dy_i * 4.0, 2.4, rgba(scale_rgb(GUNMETAL, 1.2)));
    // Cockpit.
    c.ellipse(px + dx * 4.5, py + dy_i * 4.5, 1.9, 1.4, rgba([140, 235, 255]));
    // Engine glow (flickers with frame).
    let g = if frame == 0 { 1.6 } else { 2.3 };
    c.ellipse(tail.0, tail.1, g, g * 0.8, rgba([255, 170, 90]));
    c.ellipse(tail.0, tail.1, g * 0.5, g * 0.4, rgba([255, 240, 200]));
    c.outline(OUTLINE);
    c
}

/// Stormcaller: robed psionic with a crackling orb. 26x32.
fn paint_stormcaller(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(26, 32);
    let (dx, dy) = facing_vec(f);
    let cx = 13.0;
    let cy = 18.0;
    let robe = [58, 60, 84];
    // Robe: tall tapered body.
    c.ellipse_shaded(cx, cy + 4.0, 5.5, 4.0, scale_rgb(robe, 0.9));
    c.ellipse_shaded(cx, cy - 1.0, 4.5, 7.0, robe);
    // Team sash.
    c.line(cx - 3.0, cy - 4.0, cx + 3.0, cy + 4.0, 1.6, rgba(team));
    // Hood + face glow.
    c.ellipse_shaded(cx, cy - 9.0, 3.2, 3.4, scale_rgb(robe, 1.15));
    if dy > -0.4 {
        c.ellipse(cx + dx * 1.4, cy - 9.0 + dy * 0.8, 1.6, 1.1, rgba([150, 240, 255]));
    }
    // Floating orb toward facing, crackling.
    let ox = cx + dx * 7.0;
    let oy = cy - 4.0 + dy * 3.0;
    c.ellipse(ox, oy, 2.6, 2.6, rgba([64, 210, 230]));
    c.ellipse(ox - 0.8, oy - 0.8, 1.0, 1.0, rgba([230, 255, 255]));
    let a0 = if frame == 0 { 0.6f32 } else { 2.2 };
    for k in 0..3 {
        let a = a0 + k as f32 * 2.1;
        c.line(ox, oy, ox + a.cos() * 4.5, oy + a.sin() * 4.5, 1.0, [170, 245, 255, 200]);
    }
    c.outline(OUTLINE);
    c
}

// ------------------------------------------------------------- buildings ----

/// Isometric box: top diamond + two shaded walls. The bread and butter of
/// every structure.
fn iso_box(
    c: &mut Canvas,
    cx: f32,
    top_cy: f32,
    half_w: f32,
    wall_h: f32,
    top: [u8; 3],
    accent: Option<[u8; 3]>,
) {
    let half_h = half_w * 0.5;
    // Walls first.
    for x in (cx - half_w) as i32..=(cx + half_w) as i32 {
        let dxf = (x as f32 + 0.5 - cx) / half_w;
        if dxf.abs() > 1.0 {
            continue;
        }
        let edge_y = top_cy + half_h * (1.0 - dxf.abs());
        let left = dxf < 0.0;
        let base: [u8; 3] = if left { scale_rgb(top, 0.55) } else { scale_rgb(top, 0.75) };
        for wy in 0..wall_h as i32 {
            let mut t = base;
            if wy == 0 {
                t = scale_rgb(base, 1.3);
            }
            // Accent stripe near the bottom.
            if let Some(a) = accent {
                if wy == wall_h as i32 - 3 || wy == wall_h as i32 - 4 {
                    t = a;
                }
            }
            c.set(x, (edge_y + wy as f32) as i32, rgba(t));
        }
    }
    // Top surface.
    for y in (top_cy - half_h) as i32..=(top_cy + half_h) as i32 {
        for x in (cx - half_w) as i32..=(cx + half_w) as i32 {
            let dxf = (x as f32 + 0.5 - cx) / half_w;
            let dyf = (y as f32 + 0.5 - top_cy) / half_h;
            if dxf.abs() + dyf.abs() <= 1.0 {
                let lit = if dxf - dyf < -0.2 { 1.12 } else if dxf - dyf > 0.5 { 0.92 } else { 1.0 };
                c.set(x, y, rgba(scale_rgb(top, lit)));
            }
        }
    }
}

fn windows(c: &mut Canvas, cx: f32, edge_cy: f32, half_w: f32, wall_h: f32, lit: [u8; 3]) {
    let half_h = half_w * 0.5;
    let mut k = 0;
    let mut x = cx - half_w + 4.0;
    while x < cx + half_w - 3.0 {
        let dxf = (x - cx) / half_w;
        let edge_y = edge_cy + half_h * (1.0 - dxf.abs());
        if k % 2 == 0 {
            c.rect(x as i32, (edge_y + wall_h * 0.35) as i32, 2, 2, rgba(lit));
        }
        x += 4.0;
        k += 1;
    }
}

/// Headquarters: 96px-wide command center. Base + main hall + tower + pad.
fn paint_hq(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(100, 78);
    let cx = 50.0;
    let concrete = [120, 118, 112];
    let hull = [138, 136, 130];

    // Ground plate.
    iso_box(&mut c, cx, 54.0, 48.0, 6.0, concrete, None);
    // Main hall.
    iso_box(&mut c, cx, 40.0, 36.0, 16.0, hull, Some(team));
    windows(&mut c, cx, 40.0, 36.0, 16.0, [255, 220, 130]);
    // Command tower.
    iso_box(&mut c, cx - 10.0, 26.0, 16.0, 15.0, scale_rgb(hull, 1.06), Some(team));
    // Landing pad.
    c.ellipse(cx + 18.0, 38.0, 12.0, 6.0, rgba(scale_rgb(concrete, 0.9)));
    c.ellipse(cx + 18.0, 38.0, 8.0, 4.0, rgba(scale_rgb(concrete, 1.1)));
    c.ellipse(cx + 18.0, 38.0, 3.0, 1.5, rgba(team));
    // Antenna.
    c.line(cx - 14.0, 24.0, cx - 14.0, 10.0, 1.2, rgba([90, 92, 100]));
    c.set((cx - 14.0) as i32, 9, [255, 80, 70, 255]);
    c.set((cx - 15.0) as i32, 10, [255, 160, 150, 255]);
    c.outline(OUTLINE);
    c
}

/// Supply Pylon: base plate + glowing team-colored obelisk.
fn paint_pylon(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(68, 62);
    let cx = 34.0;
    iso_box(&mut c, cx, 46.0, 30.0, 6.0, [118, 116, 110], None);
    // Obelisk: tapering vertical crystal.
    for y in 0..34 {
        let f = y as f32 / 34.0; // 0 top -> 1 bottom
        let w = 3.0 + f * 8.0;
        let yy = 12.0 + y as f32;
        for x in (cx - w) as i32..=(cx + w) as i32 {
            let side = (x as f32 - cx) / w;
            let base = if side < -0.2 {
                scale_rgb(team, 1.25)
            } else if side > 0.4 {
                scale_rgb(team, 0.6)
            } else {
                team
            };
            c.set(x, yy as i32, rgba(base));
        }
    }
    // Inner glow + floating tip.
    c.ellipse(cx, 22.0, 2.0, 6.0, rgba([255, 255, 255]));
    c.ellipse(cx, 7.0, 2.5, 3.0, rgba(scale_rgb(team, 1.3)));
    c.outline(OUTLINE);
    c
}

/// Muster Hall: wide barracks with door + roof vents.
fn paint_barracks(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(100, 70);
    let cx = 50.0;
    let hull = [128, 122, 112];
    iso_box(&mut c, cx, 50.0, 48.0, 6.0, [116, 114, 108], None);
    iso_box(&mut c, cx, 34.0, 38.0, 18.0, hull, Some(team));
    windows(&mut c, cx, 34.0, 38.0, 18.0, [255, 220, 130]);
    // Door on the lower-left wall.
    let dx = -14.0f32;
    let edge_y = 34.0 + 19.0 * (1.0 - (dx / 38.0f32).abs());
    c.rect((cx + dx - 4.0) as i32, (edge_y + 2.0) as i32, 9, 12, rgba([60, 58, 62]));
    c.rect((cx + dx - 4.0) as i32, (edge_y + 2.0) as i32, 9, 2, rgba(team));
    // Roof vents.
    for k in 0..3 {
        let vx = cx - 12.0 + k as f32 * 12.0;
        iso_box(&mut c, vx, 26.0, 5.0, 4.0, scale_rgb(hull, 1.1), None);
    }
    c.outline(OUTLINE);
    c
}

/// Plasma geyser: rocky mound with a glowing teal vent. 2x2 tiles (64px).
fn paint_geyser() -> Canvas {
    let mut c = Canvas::new(64, 44);
    // Mound.
    c.ellipse_shaded(32.0, 30.0, 28.0, 12.0, [92, 84, 72]);
    c.ellipse_shaded(32.0, 26.0, 20.0, 8.0, [104, 95, 80]);
    // Vent crater.
    c.ellipse(32.0, 24.0, 11.0, 5.0, rgba([56, 52, 48]));
    // Plasma glow.
    c.ellipse(32.0, 24.0, 8.0, 3.6, rgba([40, 200, 190]));
    c.ellipse(32.0, 24.0, 4.5, 2.0, rgba([170, 255, 245]));
    // Wisps.
    for k in 0..3 {
        let h = hash2(k, 3, 77);
        let x = 26.0 + (h % 14) as f32;
        let y = 12.0 - (k as f32 * 3.0);
        c.ellipse(x, y, 2.0, 1.2, [120, 240, 230, 120]);
    }
    // Scattered rocks.
    c.ellipse_shaded(10.0, 36.0, 4.0, 2.2, [80, 74, 66]);
    c.ellipse_shaded(54.0, 34.0, 3.4, 2.0, [80, 74, 66]);
    c.outline([20, 26, 26, 255]);
    c
}

/// Plasma Condenser: squat industrial extractor with tank + intake pipe.
fn paint_condenser(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(68, 58);
    let cx = 34.0;
    let hull = [122, 118, 108];
    iso_box(&mut c, cx, 42.0, 30.0, 6.0, [112, 110, 104], None);
    // Main intake housing.
    iso_box(&mut c, cx, 32.0, 22.0, 12.0, hull, Some(team));
    // Condensation tank on top.
    c.ellipse_shaded(cx - 6.0, 22.0, 9.0, 6.5, scale_rgb(hull, 1.08));
    c.ellipse(cx - 6.0, 19.0, 5.0, 3.0, rgba([64, 210, 200]));
    c.ellipse(cx - 7.5, 18.2, 2.0, 1.2, rgba([190, 255, 248]));
    // Intake pipe to the vent side.
    c.line(cx + 8.0, 26.0, cx + 20.0, 34.0, 3.0, rgba(scale_rgb(hull, 0.8)));
    c.line(cx + 9.0, 25.0, cx + 20.0, 32.5, 1.2, rgba(scale_rgb(hull, 1.2)));
    // Gauge light.
    c.set((cx + 10.0) as i32, 24, [64, 220, 210, 255]);
    c.outline(OUTLINE);
    c
}

/// Forge: heavy factory with furnace glow and chimney. 100x74.
fn paint_forge(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(100, 74);
    let cx = 50.0;
    let hull = [124, 116, 106];
    iso_box(&mut c, cx, 50.0, 48.0, 6.0, [112, 110, 104], None);
    iso_box(&mut c, cx, 32.0, 38.0, 20.0, hull, Some(team));
    windows(&mut c, cx, 32.0, 38.0, 20.0, [255, 190, 110]);
    // Furnace mouth on the lower-left wall.
    c.rect((cx - 24.0) as i32, 46, 10, 8, rgba([255, 140, 60]));
    c.rect((cx - 22.0) as i32, 48, 6, 4, rgba([255, 220, 140]));
    // Chimney with smoke.
    iso_box(&mut c, cx + 22.0, 16.0, 7.0, 14.0, scale_rgb(hull, 0.9), None);
    for k in 0..3 {
        let h = hash2(k, 5, 60);
        c.ellipse(cx + 22.0 + (h % 5) as f32 - 2.0, 8.0 - k as f32 * 3.0, 2.2, 1.4, [130, 130, 135, 140]);
    }
    // Crane arm.
    c.line(cx - 30.0, 26.0, cx - 14.0, 18.0, 1.6, rgba([90, 92, 100]));
    c.outline(OUTLINE);
    c
}

/// Aerie: landing pad + control tower. 100x70.
fn paint_aerie(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(100, 70);
    let cx = 50.0;
    iso_box(&mut c, cx, 48.0, 48.0, 6.0, [112, 110, 104], None);
    // Raised landing pad.
    iso_box(&mut c, cx + 8.0, 38.0, 34.0, 8.0, [96, 100, 108], None);
    // Pad markings.
    c.ellipse(cx + 8.0, 38.0, 13.0, 6.5, rgba([120, 126, 136]));
    c.ellipse(cx + 8.0, 38.0, 9.0, 4.5, rgba([96, 100, 108]));
    c.rect((cx + 6.0) as i32, 35, 4, 7, rgba(team));
    // Hazard stripes on the pad's front edge.
    for k in 0..8 {
        let x = (cx - 20.0 + k as f32 * 7.0) as i32;
        c.rect(x, 49, 4, 2, rgba(if k % 2 == 0 { [220, 180, 60] } else { [40, 40, 44] }));
    }
    // Control tower.
    iso_box(&mut c, cx - 28.0, 22.0, 12.0, 18.0, [132, 130, 124], Some(team));
    c.rect((cx - 34.0) as i32, 20, 12, 4, rgba([150, 230, 250]));
    c.line(cx - 28.0, 12.0, cx - 28.0, 4.0, 1.2, rgba([90, 92, 100]));
    c.set((cx - 28.0) as i32, 3, [255, 80, 70, 255]);
    c.outline(OUTLINE);
    c
}

/// Archive: research dome with antenna array. 70x62.
fn paint_archive(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(70, 62);
    let cx = 35.0;
    iso_box(&mut c, cx, 46.0, 30.0, 6.0, [114, 112, 108], None);
    // Dome.
    c.ellipse_shaded(cx, 32.0, 20.0, 14.0, [136, 134, 130]);
    c.ellipse(cx - 6.0, 26.0, 6.0, 4.0, rgba([170, 170, 168]));
    // Data windows band.
    for k in 0..6 {
        let x = (cx - 14.0 + k as f32 * 5.0) as i32;
        c.rect(x, 38, 2, 3, rgba([120, 220, 255]));
    }
    c.rect((cx - 16.0) as i32, 42, 32, 2, rgba(team));
    // Antenna array.
    for (ax, ah) in [(-10.0f32, 12.0f32), (0.0, 16.0), (10.0, 10.0)] {
        c.line(cx + ax, 22.0, cx + ax, 22.0 - ah, 1.1, rgba([90, 92, 100]));
        c.set((cx + ax) as i32, (21.0 - ah) as i32, [140, 240, 255, 255]);
    }
    c.outline(OUTLINE);
    c
}

// --------------------------------------------------------------- effects ----

fn star_flash() -> Canvas {
    let mut c = Canvas::new(11, 11);
    for k in 0..4 {
        let a = k as f32 * std::f32::consts::FRAC_PI_4;
        c.line(
            5.5 - a.cos() * 5.0,
            5.5 - a.sin() * 5.0,
            5.5 + a.cos() * 5.0,
            5.5 + a.sin() * 5.0,
            1.4,
            [255, 230, 150, 255],
        );
    }
    c.ellipse(5.5, 5.5, 2.2, 2.2, [255, 255, 255, 255]);
    c
}
