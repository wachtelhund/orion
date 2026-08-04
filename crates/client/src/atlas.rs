//! Procedural sprite atlas — the entire art of the game, painted at startup.
//!
//! Style target: SC1-era isometric pixel art. Dark outlines, 3-4 tone
//! material ramps, dithered texture, light from the upper-left, hard team
//! color accents. Every sprite is painted per team so no runtime palette
//! tricks are needed.

use crate::font;

pub const ATLAS: u32 = 4096;

/// Supersample factor for world sprites: canvas px per screen unit at
/// zoom 1. At the default zoom (2 x DPI scale) a 4x sprite is texel-perfect
/// on retina displays instead of showing 4x4 screen px per canvas px.
pub const SS: f32 = 4.0;

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
        self.outline_t(dark, 1);
    }

    /// Outline with thickness `t` canvas px (supersampled sprites need ~2 so
    /// the silhouette still reads at screen scale).
    pub fn outline_t(&mut self, dark: Color, t: i32) {
        let orig = self.px.clone();
        for y in 0..self.h {
            for x in 0..self.w {
                let p = orig[(y * self.w + x) as usize];
                if p[3] < 40 {
                    continue;
                }
                let mut edge = false;
                'scan: for dy in -t..=t {
                    for dx in -t..=t {
                        let (nx, ny) = (x + dx, y + dy);
                        if nx < 0 || ny < 0 || nx >= self.w || ny >= self.h {
                            edge = true;
                            break 'scan;
                        } else if orig[(ny * self.w + nx) as usize][3] < 40 {
                            edge = true;
                            break 'scan;
                        }
                    }
                }
                if edge {
                    self.set(x, y, dark);
                }
            }
        }
    }

    /// Top-left rim light: brighten body pixels that sit just inside the
    /// outline on the lit side. Call AFTER outline_t with the same color.
    pub fn rim(&mut self, outline: Color, f: f32) {
        let orig = self.px.clone();
        let is_edge = |p: Color| p == outline || p[3] < 40;
        for y in 0..self.h {
            for x in 0..self.w {
                let p = orig[(y * self.w + x) as usize];
                if p[3] < 40 || p == outline {
                    continue;
                }
                let up = if y > 0 { orig[((y - 1) * self.w + x) as usize] } else { outline };
                let left = if x > 0 { orig[(y * self.w + x - 1) as usize] } else { outline };
                if is_edge(up) || is_edge(left) {
                    self.set(x, y, scale([p[0], p[1], p[2]], f));
                }
            }
        }
    }

    /// Soft radial glow: quadratic falloff alpha blend. The emissive
    /// workhorse — windows, engines, plasma, bio-lights.
    pub fn glow(&mut self, cx: f32, cy: f32, r: f32, c: [u8; 3], strength: f32) {
        for y in (cy - r) as i32..=(cy + r) as i32 {
            for x in (cx - r) as i32..=(cx + r) as i32 {
                let dx = x as f32 + 0.5 - cx;
                let dy = y as f32 + 0.5 - cy;
                let d = (dx * dx + dy * dy).sqrt() / r;
                if d < 1.0 {
                    let a = ((1.0 - d) * (1.0 - d) * strength * 255.0).min(255.0) as u8;
                    if a > 3 {
                        self.blend(x, y, [c[0], c[1], c[2], a]);
                    }
                }
            }
        }
    }

    /// Filled polygon (even-odd scanline). Angular armor beats ellipses.
    pub fn poly(&mut self, pts: &[(f32, f32)], c: Color) {
        let y0 = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min) as i32;
        let y1 = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max) as i32 + 1;
        for y in y0..=y1 {
            let sy = y as f32 + 0.5;
            let mut xs: Vec<f32> = Vec::new();
            for i in 0..pts.len() {
                let (x0, py0) = pts[i];
                let (x1, py1) = pts[(i + 1) % pts.len()];
                if (py0 <= sy) != (py1 <= sy) {
                    xs.push(x0 + (sy - py0) / (py1 - py0) * (x1 - x0));
                }
            }
            xs.sort_by(|a, b| a.total_cmp(b));
            for pair in xs.chunks(2) {
                if let [a, b] = pair {
                    for x in *a as i32..=(*b - 0.01) as i32 {
                        self.blend(x, y, c);
                    }
                }
            }
        }
    }

    /// Ellipse shaded as a lit dome: fake sphere normal, light from the
    /// upper-left, quantized to 5 tones with ordered dithering.
    pub fn dome(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, base: [u8; 3]) {
        for y in (cy - ry - 1.0) as i32..=(cy + ry + 1.0) as i32 {
            for x in (cx - rx - 1.0) as i32..=(cx + rx + 1.0) as i32 {
                let dx = (x as f32 + 0.5 - cx) / rx;
                let dy = (y as f32 + 0.5 - cy) / ry;
                let d = dx * dx + dy * dy;
                if d > 1.0 {
                    continue;
                }
                let nz = (1.0 - d).sqrt();
                let lit = (-dx * 0.40 - dy * 0.55 + nz * 0.75).clamp(0.0, 1.3) / 1.3;
                self.blend(x, y, scale(base, tone(lit, x, y)));
            }
        }
    }

    /// Downward fading streak — rust runs, grime, soot.
    pub fn streak(&mut self, x: i32, y: i32, len: i32, c: [u8; 3], a0: f32) {
        for k in 0..len {
            let a = (a0 * (1.0 - k as f32 / len as f32) * 255.0) as u8;
            self.blend(x, y + k, [c[0], c[1], c[2], a]);
        }
    }
}

/// Ordered-dither tone quantizer: continuous light 0..1 to one of five
/// material tones with a 2x2 Bayer boundary dither. Shared by every painter
/// so all materials posterize the same way.
pub fn tone(lit: f32, x: i32, y: i32) -> f32 {
    const LEVELS: [f32; 5] = [0.50, 0.72, 0.94, 1.16, 1.38];
    let t = lit.clamp(0.0, 1.0) * (LEVELS.len() - 1) as f32;
    let i = t as usize;
    let frac = t - i as f32;
    let bayer = [[0.25f32, 0.75], [1.0, 0.5]][(y & 1) as usize][(x & 1) as usize];
    let up = frac > bayer;
    LEVELS[(i + up as usize).min(LEVELS.len() - 1)]
}

/// Smooth bilinear value noise on a `cell`-px lattice, 0..1. Macro-scale
/// ground variation without per-pixel salt-and-pepper.
pub fn vnoise(x: f32, y: f32, cell: f32, salt: u32) -> f32 {
    let gx = (x / cell).floor();
    let gy = (y / cell).floor();
    let fx = x / cell - gx;
    let fy = y / cell - gy;
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sy = fy * fy * (3.0 - 2.0 * fy);
    let corner = |ix: f32, iy: f32| (hash2(ix as i32, iy as i32, salt) % 1000) as f32 / 1000.0;
    let a = corner(gx, gy);
    let b = corner(gx + 1.0, gy);
    let c = corner(gx, gy + 1.0);
    let d = corner(gx + 1.0, gy + 1.0);
    a + (b - a) * sx + (c - a) * sy + (a - b - c + d) * sx * sy
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

#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
    pub w: u32,
    pub h: u32,
    /// Canvas px per screen unit at zoom 1. Draw code divides pixel sizes
    /// by this, so sprites painted at different densities coexist.
    pub scale: f32,
}

impl Default for Region {
    fn default() -> Region {
        Region { u0: 0.0, v0: 0.0, u1: 0.0, v1: 0.0, w: 0, h: 0, scale: 1.0 }
    }
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
        self.place_s(c, 1.0)
    }

    /// Place a canvas painted at `scale` canvas px per screen unit.
    fn place_s(&mut self, c: &Canvas, scale: f32) -> Region {
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
            scale,
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
    /// Destructible trees (two variants) + the rock wall boulder.
    pub trees: [Region; 2],
    pub rock_wall: Region,
    /// [unit_type][team][facing][frame]. 0-6: Vanguard Combine (worker,
    /// trooper, vanguard, breaker, skywing, stormcaller, breaker-sieged).
    /// 7-12: Kyth Assembly (drone, skitter, spitter, ravager, wisp, weaver).
    pub units: Vec<Region>,
    /// [building_type][team]. 0-6 Vanguard, 7-13 Kyth (hive, spire, sapwell,
    /// warren, incubator, roost, cortex).
    pub buildings: Vec<Region>,
    /// Building sprite heights in SCREEN units at zoom 1 (canvas px / scale).
    pub building_px_h: [f32; 14],
    // effects
    pub flash: Region,
    pub spark: Region,
    pub blast_ring: Region,
    pub corpse: Region,
    pub rubble: Region,
    // console chrome
    pub chrome_panel: Region,
    pub chrome_dark: Region,
    pub gold_h: Region,
    pub gold_v: Region,
    pub gold_corner: Region,
    pub rivet: Region,
    pub btn_plate: Region,
    pub btn_plate_hi: Region,
    pub menu_plate: Region,
    pub menu_plate_hi: Region,
    pub title_plate: Region,
    pub shoulder: Region,
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

/// Cliff wall height in canvas px = SS * iso::ELEV_PX.
pub const CLIFF_H: i32 = 48;

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

    // Terrain (painted at SS; tiles are drawn at explicit world sizes so
    // the region scale is informational here).
    let ground_low =
        std::array::from_fn(|i| p.place_s(&ground_tile(i as u32, GROUND_LOW, false), SS));
    let ground_high =
        std::array::from_fn(|i| p.place_s(&ground_tile(i as u32 + 8, GROUND_HIGH, true), SS));
    let ramp = std::array::from_fn(|i| p.place_s(&ramp_tile(i as u32), SS));
    let rock = std::array::from_fn(|i| p.place_s(&rock_tile(i as u32), SS));
    let cliff_left = p.place_s(&cliff_face(true), SS);
    let cliff_right = p.place_s(&cliff_face(false), SS);

    // Minerals + geyser.
    let minerals = std::array::from_fn(|i| p.place_s(&mineral_cluster(i as u32), SS));
    let geyser = p.place_s(&paint_geyser(), SS);

    // Units: [type][team][facing][frame].
    let mut units = Vec::new();
    for unit_type in 0..13 {
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
                        6 => paint_breaker_sieged(facing, frame, TEAMS[team]),
                        7 => paint_kdrone(facing, frame, TEAMS[team]),
                        8 => paint_skitter(facing, frame, TEAMS[team]),
                        9 => paint_spitter(facing, frame, TEAMS[team]),
                        10 => paint_ravager(facing, frame, TEAMS[team]),
                        11 => paint_wisp(facing, frame, TEAMS[team]),
                        _ => paint_weaver(facing, frame, TEAMS[team]),
                    };
                    units.push(p.place_s(&c, SS));
                }
            }
        }
    }

    // Buildings: [type][team].
    let mut buildings = Vec::new();
    let mut building_px_h = [0f32; 14];
    for b_type in 0..14 {
        for team in 0..2 {
            let c = match b_type {
                0 => paint_hq(TEAMS[team]),
                1 => paint_pylon(TEAMS[team]),
                2 => paint_barracks(TEAMS[team]),
                3 => paint_condenser(TEAMS[team]),
                4 => paint_forge(TEAMS[team]),
                5 => paint_aerie(TEAMS[team]),
                6 => paint_archive(TEAMS[team]),
                7 => paint_hive(TEAMS[team]),
                8 => paint_spire(TEAMS[team]),
                9 => paint_sapwell(TEAMS[team]),
                10 => paint_warren(TEAMS[team]),
                11 => paint_incubator(TEAMS[team]),
                12 => paint_roost(TEAMS[team]),
                _ => paint_cortex(TEAMS[team]),
            };
            building_px_h[b_type] = c.h as f32;
            buildings.push(p.place(&c));
        }
    }

    // Destructible flora + rocks.
    let trees = [p.place_s(&tree_canvas(0), SS), p.place_s(&tree_canvas(1), SS)];
    let rock_wall = {
        let mut c = Canvas::new(88, 72);
        // Angular basalt slabs: dark mass, lit top facets, cracks.
        c.poly(&[(6.0, 52.0), (30.0, 34.0), (62.0, 36.0), (82.0, 52.0), (60.0, 64.0), (22.0, 64.0)], rgba([74, 70, 74]));
        c.poly(&[(14.0, 46.0), (34.0, 30.0), (52.0, 34.0), (36.0, 46.0)], rgba([96, 92, 96]));
        c.poly(&[(40.0, 44.0), (58.0, 32.0), (74.0, 46.0), (56.0, 52.0)], rgba([88, 84, 88]));
        c.poly(&[(6.0, 52.0), (22.0, 64.0), (24.0, 52.0)], rgba([52, 49, 52]));
        c.line(34.0, 34.0, 40.0, 58.0, 1.6, rgba([44, 41, 44]));
        c.line(56.0, 36.0, 52.0, 60.0, 1.3, rgba([48, 45, 48]));
        c.outline_t([22, 20, 22, 255], 2);
        c.rim([22, 20, 22, 255], 1.25);
        p.place_s(&c, SS)
    };

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

    // Console chrome (SC:R-style): navy tech panels, gold piping, beveled
    // button plates. Painted once, stretched/tiled by the HUD.
    let chrome_panel = p.place(&chrome_panel_canvas(384, 160, 1.0));
    let chrome_dark = p.place(&chrome_panel_canvas(192, 96, 0.55));
    let gold_h = p.place(&gold_strip_canvas(64, 6, false));
    let gold_v = p.place(&gold_strip_canvas(6, 64, true));
    let gold_corner = {
        let mut c = Canvas::new(14, 14);
        // Rounded gold corner (top-left orientation; HUD flips via scale).
        for y in 0..14 {
            for x in 0..14 {
                let dx = (x as f32 + 0.5 - 14.0) / 14.0;
                let dy = (y as f32 + 0.5 - 14.0) / 14.0;
                let d = (dx * dx + dy * dy).sqrt();
                if (0.62..=1.0).contains(&d) {
                    let f = if d > 0.93 {
                        0.55
                    } else if d < 0.74 {
                        1.35
                    } else {
                        1.0
                    };
                    c.set(x, y, scale(GOLD, f));
                }
            }
        }
        p.place(&c)
    };
    let rivet = {
        let mut c = Canvas::new(7, 7);
        c.ellipse_shaded(3.5, 3.5, 3.0, 3.0, [116, 122, 140]);
        c.set(2, 2, [210, 218, 235, 255]);
        p.place(&c)
    };
    let btn_plate = p.place(&button_plate_canvas(52, 48, false));
    let btn_plate_hi = p.place(&button_plate_canvas(52, 48, true));
    let menu_plate = p.place(&menu_plate_canvas(300, 44, false));
    let menu_plate_hi = p.place(&menu_plate_canvas(300, 44, true));
    let title_plate = p.place(&title_plate_canvas(320, 40));
    let shoulder = {
        // Angled console shoulder: navy triangle with a gold hypotenuse.
        // Painted as the LEFT-rising variant; mirrored by negative width.
        let (w, h) = (48, 48);
        let mut c = Canvas::new(w, h);
        for y in 0..h {
            for x in 0..w {
                // Solid below the diagonal from bottom-left to top-right.
                if y >= h - 1 - x {
                    let f = tech_noise(x, y, 31);
                    c.set(x, y, scale(NAVY, f));
                }
            }
        }
        for x in 0..w {
            let y = h - 1 - x;
            c.set(x, y, rgba(GOLD));
            c.set(x, (y + 1).min(h - 1), scale(GOLD, 0.6));
            if y + 2 < h {
                c.set(x, y + 2, scale(NAVY, 1.5));
            }
        }
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
        trees,
        rock_wall,
        units,
        buildings,
        building_px_h,
        flash,
        spark,
        blast_ring,
        corpse,
        rubble,
        chrome_panel,
        chrome_dark,
        gold_h,
        gold_v,
        gold_corner,
        rivet,
        btn_plate,
        btn_plate_hi,
        menu_plate,
        menu_plate_hi,
        title_plate,
        shoulder,
        glyphs,
    };
    (p.px, book)
}

/// A gnarled alien conifer: dark trunk, layered teal-green canopy. Two
/// variants via `v` so forests don't tile visibly.
fn tree_canvas(v: i32) -> Canvas {
    let mut c = Canvas::new(80, 104);
    let cx = 40.0 + if v == 0 { 0.0 } else { 3.0 };
    // Trunk + root flare.
    c.poly(&[(cx - 4.0, 98.0), (cx + 4.0, 98.0), (cx + 2.5, 62.0), (cx - 2.5, 62.0)], rgba([50, 38, 28]));
    c.poly(&[(cx - 10.0, 100.0), (cx + 10.0, 100.0), (cx + 5.0, 93.0), (cx - 5.0, 93.0)], rgba([42, 32, 24]));
    c.line(cx - 1.5, 94.0, cx - 2.5, 68.0, 1.5, rgba([64, 48, 36]));
    // Three serrated conifer tiers — sharp silhouette, drooping spiked
    // skirts, apex skewed per variant so the forest isn't cloned.
    let tiers: [(f32, f32, f32, [u8; 3]); 3] = [
        (86.0, 27.0, 30.0, [28, 56, 44]),
        (64.0, 20.0, 26.0, [36, 70, 52]),
        (44.0, 13.0, 24.0, [46, 86, 62]),
    ];
    for (ti, &(by, hw, ah, col)) in tiers.iter().enumerate() {
        let apex_x = cx + if v == 0 { [-2.0, 2.0, -1.0][ti] } else { [3.0, -2.0, 2.0][ti] };
        let mut pts: Vec<(f32, f32)> = vec![(apex_x, by - ah), (cx + hw, by)];
        let n = 6;
        for k in 1..n {
            let f = k as f32 / n as f32;
            let x = cx + hw - f * hw * 2.0;
            let droop = if k % 2 == 1 { 6.0 } else { 1.0 };
            let jitter = (hash2(k, ti as i32 * 9 + v, 551) % 4) as f32;
            pts.push((x, by + droop + jitter));
        }
        pts.push((cx - hw, by));
        c.poly(&pts, rgba(col));
        // Lit left slope, shadowed underside.
        c.line(apex_x, by - ah, cx - hw + 2.0, by - 1.0, 1.6, rgba(scale_rgb(col, 1.35)));
        c.line(cx - hw * 0.5, by + 3.0, cx + hw * 0.6, by + 2.0, 2.0, rgba(scale_rgb(col, 0.6)));
    }
    // Needle spire tip.
    c.line(cx, 22.0, cx + if v == 0 { -1.0 } else { 2.0 }, 8.0, 2.0, rgba([54, 100, 70]));
    // Bioluminescent specks in the foliage.
    for k in 0..6 {
        let h = hash2(k, v, 421);
        let x = 22.0 + (h % 36) as f32;
        let y = 34.0 + ((h >> 8) % 44) as f32;
        c.glow(x, y, 3.0, [110, 235, 185], 0.7);
        c.set(x as i32, y as i32, [180, 250, 215, 255]);
    }
    c.outline_t([10, 18, 14, 255], 2);
    c.rim([10, 18, 14, 255], 1.3);
    c
}


// -------------------------------------------------------- console chrome ----

/// Chrome palette: deep navy tech panels with gold piping.
pub const NAVY: [u8; 3] = [22, 34, 58];
pub const GOLD: [u8; 3] = [201, 162, 39];

/// Brightness factor for the brushed tech-panel texture at (x, y).
fn tech_noise(x: i32, y: i32, salt: u32) -> f32 {
    let n = (hash2(x, y, salt) % 100) as f32 / 100.0;
    // Horizontal brushing + sparse darker seams.
    let brush = if y % 4 == 0 { 1.08 } else { 1.0 };
    let seam = if hash2(0, y / 9, salt ^ 5) % 7 == 0 && y % 9 == 8 { 0.82 } else { 1.0 };
    (0.92 + n * 0.16) * brush * seam
}

/// Navy tech panel with circuit detailing; `level` scales overall brightness
/// (1.0 = raised panel, ~0.55 = dark inset).
fn chrome_panel_canvas(w: i32, h: i32, level: f32) -> Canvas {
    let mut c = Canvas::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let mut f = tech_noise(x, y, 77) * level;
            // Vignette toward edges sells depth.
            let ex = (x.min(w - 1 - x) as f32 / 22.0).min(1.0);
            let ey = (y.min(h - 1 - y) as f32 / 22.0).min(1.0);
            f *= 0.82 + 0.18 * ex.min(ey);
            c.set(x, y, scale(NAVY, f));
        }
    }
    // Faint circuit traces — barely-there detailing. Bright marks at this
    // scale read as broken pixels, not texture (user report), so: few,
    // dim, and no glowing endpoint nodes.
    let traces = (w * h / 9000).max(2);
    for k in 0..traces {
        let hsh = hash2(k, w + h, 913);
        let x0 = 10 + (hsh % (w as u32 - 20)) as i32;
        let y0 = 10 + ((hsh >> 9) % (h as u32 - 20)) as i32;
        let len = 10 + ((hsh >> 18) % 18) as i32;
        let vertical = hsh & 1 == 0;
        for s in 0..len {
            let (x, y) = if vertical { (x0, y0 + s) } else { (x0 + s, y0) };
            if x < w - 8 && y < h - 8 {
                c.blend(x, y, [58, 82, 124, 55]);
            }
        }
    }
    c
}

/// Gold piping strip: bright core, dark rim, subtle segment ticks.
fn gold_strip_canvas(w: i32, h: i32, vertical: bool) -> Canvas {
    let mut c = Canvas::new(w, h);
    let thick = if vertical { w } else { h };
    for y in 0..h {
        for x in 0..w {
            let t = if vertical { x } else { y };
            let f = match t {
                0 => 0.5,
                1 => 1.45,
                _ if t == thick - 1 => 0.42,
                _ if t == thick - 2 => 0.8,
                _ => 1.05,
            };
            let along = if vertical { y } else { x };
            let seg = if along % 16 == 15 { 0.75 } else { 1.0 };
            c.set(x, y, scale(GOLD, f * seg));
        }
    }
    c
}

/// Command-card button plate: navy face, chunky steel bevel, gold rim on
/// hover.
fn button_plate_canvas(w: i32, h: i32, hover: bool) -> Canvas {
    let mut c = Canvas::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let b = x.min(y).min(w - 1 - x).min(h - 1 - y);
            let px = match b {
                0 => scale([8, 10, 16], 1.0),
                1 | 2 => {
                    // Bevel: lit top-left, shadowed bottom-right.
                    let tl = x <= 2 || y <= 2;
                    let f = if tl { 1.7 } else { 0.55 };
                    if hover {
                        scale(GOLD, if tl { 1.1 } else { 0.5 })
                    } else {
                        scale([88, 98, 122], f)
                    }
                }
                _ => {
                    let f = tech_noise(x, y, 55) * if hover { 1.25 } else { 1.0 };
                    scale(NAVY, f)
                }
            };
            c.set(x, y, px);
        }
    }
    c
}

/// Menu row plate: wide navy plate with a thin steel bevel and gold ends.
fn menu_plate_canvas(w: i32, h: i32, hover: bool) -> Canvas {
    let mut c = Canvas::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let b = x.min(y).min(w - 1 - x).min(h - 1 - y);
            let px = match b {
                0 => scale([6, 8, 14], 1.0),
                1 => {
                    let tl = y <= 1;
                    if hover {
                        scale(GOLD, if tl { 1.2 } else { 0.6 })
                    } else {
                        scale([80, 90, 114], if tl { 1.5 } else { 0.6 })
                    }
                }
                _ => {
                    let mut f = tech_noise(x, y, 21) * if hover { 1.3 } else { 0.92 };
                    // Slight center glow.
                    let cx = (x as f32 / w as f32 - 0.5).abs();
                    f *= 1.1 - cx * 0.3;
                    scale(NAVY, f)
                }
            };
            c.set(x, y, px);
        }
    }
    // Gold end caps.
    for y in 2..h - 2 {
        for x in [2, 3, w - 4, w - 3] {
            c.set(x, y, scale(GOLD, if x % 2 == 0 { 1.0 } else { 0.65 }));
        }
    }
    c
}

/// Menu title plate: darker, gold underline, winged ends.
fn title_plate_canvas(w: i32, h: i32) -> Canvas {
    let mut c = Canvas::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let f = tech_noise(x, y, 8) * 0.6;
            let cx = (x as f32 / w as f32 - 0.5).abs();
            c.set(x, y, scale(NAVY, f * (1.25 - cx * 0.5)));
        }
    }
    for x in 0..w {
        let cx = (x as f32 / w as f32 - 0.5).abs();
        if cx < 0.46 {
            c.set(x, h - 3, scale(GOLD, 1.1 - cx));
            c.set(x, h - 2, scale(GOLD, 0.55));
        }
    }
    c
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
//
// Dystopian wasteland rules: ground stays DARK (values ~40-95) and
// low-contrast so units and buildings pop against it. Detail comes from
// macro value-noise patches, hairline cracks, sparse debris and rust —
// never from bright or busy texture.

const GROUND_LOW: [[u8; 3]; 3] = [[57, 51, 45], [48, 43, 38], [65, 58, 51]];
const GROUND_HIGH: [[u8; 3]; 3] = [[84, 81, 75], [73, 70, 65], [94, 91, 84]];

/// Ashen wasteland diamond at 4x. Variants differ by salt; some carry
/// cracks, scorch rings, debris or rust flecks. `high` adds slab seams.
fn ground_tile(salt: u32, ramp_colors: [[u8; 3]; 3], high: bool) -> Canvas {
    let (w, hh) = (128, 64);
    let mut c = Canvas::new(w, hh);
    let inside = |px: f32, py: f32| -> bool {
        let dx = (px + 0.5 - w as f32 / 2.0) / (w as f32 / 2.0);
        let dy = (py + 0.5 - hh as f32 / 2.0) / (hh as f32 / 2.0);
        dx.abs() + dy.abs() <= 1.0
    };
    // Scorch patch position for the variants that get one.
    let sc = hash2(salt as i32, 3, 902);
    let (scx, scy) = (34.0 + (sc % 60) as f32, 18.0 + ((sc >> 8) % 28) as f32);
    let scorched = salt % 4 == 3;
    for y in 0..hh {
        for x in 0..w {
            if !inside(x as f32, y as f32) {
                continue;
            }
            // Macro patches so the ground isn't uniform static.
            let macro_f = 0.90 + vnoise(x as f32, y as f32, 26.0, salt ^ 71) * 0.20;
            let h = hash2(x, y, salt);
            let t = match h % 16 {
                0..=8 => ramp_colors[0],
                9..=13 => ramp_colors[1],
                _ => ramp_colors[2],
            };
            let mut f = macro_f;
            if scorched {
                let d = ((x as f32 - scx).powi(2) / 4.0 + (y as f32 - scy).powi(2)).sqrt();
                if d < 14.0 {
                    f *= 0.80 + 0.20 * (d / 14.0);
                }
            }
            c.set(x, y, rgba(scale_rgb(t, f)));
            // Sparse rust flecks bleed through the ash.
            if h % 149 == 0 {
                c.blend(x, y, [104, 58, 34, 70]);
            }
        }
    }
    // Hairline cracks: dark random walks with a faint lit lip below. Some
    // variants stay clean so the pattern doesn't tile into a scribble.
    let n_cracks = match salt % 4 {
        0 => 2,
        1 | 2 => 1,
        _ => 0,
    };
    for k in 0..n_cracks {
        let h = hash2(k, salt as i32, 411);
        let mut px = 24.0 + (h % 80) as f32;
        let mut py = 12.0 + ((h >> 8) % 40) as f32;
        let mut dir = ((h >> 16) % 628) as f32 / 100.0;
        for s in 0..9 {
            let step = 6.0 + (hash2(s, k, salt ^ 9) % 5) as f32;
            let nx = px + dir.cos() * step;
            let ny = py + dir.sin() * step * 0.5; // iso squash
            if inside(px, py) && inside(nx, ny) {
                c.line(px, py, nx, ny, 1.0, [30, 27, 24, 200]);
                c.line(px, py + 1.0, nx, ny + 1.0, 1.0, [255, 255, 255, 14]);
            }
            px = nx;
            py = ny;
            dir += ((hash2(s, k + 7, salt) % 100) as f32 / 100.0 - 0.5) * 1.1;
        }
    }
    // Debris: a few dark pebbles / plate shards.
    for k in 0..3 {
        let h = hash2(k as i32 + 40, salt as i32, 5);
        let px = 20.0 + (h % 88) as f32;
        let py = 12.0 + ((h >> 8) % 40) as f32;
        if inside(px - 4.0, py - 2.0) && inside(px + 4.0, py + 2.0) {
            if h % 3 == 0 && high {
                // Broken plate fragment on the slabs.
                c.poly(
                    &[(px - 4.0, py), (px + 1.0, py - 2.5), (px + 5.0, py + 0.5), (px, py + 2.5)],
                    rgba(scale_rgb(ramp_colors[1], 0.78)),
                );
                c.line(px - 3.0, py, px + 3.0, py - 1.0, 1.0, rgba(scale_rgb(ramp_colors[2], 1.05)));
            } else {
                c.dome(px, py, 2.4, 1.4, scale_rgb(ramp_colors[1], 0.82));
            }
        }
    }
    // High ground: faint slab seams parallel to the diamond edges.
    if high {
        for k in 0..2 {
            let h = hash2(k + 9, salt as i32, 33);
            let off = -16.0 + (h % 32) as f32;
            let up = h % 2 == 0;
            // Seam through (64+off, 32): slope +-0.5 in canvas space.
            let (x0, y0) = (16.0f32, 32.0 + off + if up { -8.0 } else { 8.0 });
            let sl = if up { 0.5 } else { -0.5 };
            for s in 0..96 {
                let px = x0 + s as f32;
                let py = y0 + s as f32 * sl;
                if inside(px, py) && inside(px, py + 1.0) {
                    c.blend(px as i32, py as i32, [30, 28, 26, 90]);
                    c.blend(px as i32, py as i32 + 1, [255, 255, 255, 12]);
                }
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

/// Ramp: reinforced causeway — dark plating with worn cross-treads.
fn ramp_tile(salt: u32) -> Canvas {
    let (w, hh) = (128, 64);
    let mut c = Canvas::new(w, hh);
    let base = [78, 73, 64];
    for y in 0..hh {
        for x in 0..w {
            let dx = (x as f32 + 0.5 - 64.0) / 64.0;
            let dy = (y as f32 + 0.5 - 32.0) / 32.0;
            if dx.abs() + dy.abs() > 1.0 {
                continue;
            }
            let h = hash2(x, y, salt + 40);
            let macro_f = 0.92 + vnoise(x as f32, y as f32, 24.0, salt ^ 55) * 0.16;
            // Cross-treads perpendicular to the slope.
            let stripe = ((x + y * 2) / 12) % 2 == 0;
            let mut f = if stripe { 0.88 } else { 1.0 };
            // Tread lip highlight.
            if (x + y * 2) % 24 == 0 {
                f = 1.10;
            }
            if h % 31 == 0 {
                f *= 0.82; // dents
            }
            c.set(x, y, rgba(scale_rgb(base, f * macro_f)));
            if h % 173 == 0 {
                c.blend(x, y, [104, 58, 34, 60]); // rust
            }
        }
    }
    c
}

/// Impassable rock tile: near-black basalt rubble.
fn rock_tile(salt: u32) -> Canvas {
    let (w, hh) = (128, 64);
    let mut c = Canvas::new(w, hh);
    let inside = |px: f32, py: f32| -> bool {
        let dx = (px + 0.5 - 64.0) / 64.0;
        let dy = (py + 0.5 - 32.0) / 32.0;
        dx.abs() + dy.abs() <= 1.0
    };
    for y in 0..hh {
        for x in 0..w {
            if !inside(x as f32, y as f32) {
                continue;
            }
            let h = hash2(x, y, salt + 80);
            let macro_f = 0.88 + vnoise(x as f32, y as f32, 18.0, salt ^ 91) * 0.24;
            let t = match h % 16 {
                0..=7 => [50, 47, 50],
                8..=12 => [42, 40, 43],
                _ => [60, 56, 60],
            };
            c.set(x, y, rgba(scale_rgb(t, macro_f)));
        }
    }
    // Angular basalt chunks.
    for k in 0..4 {
        let h = hash2(k, salt as i32, 31);
        let px = 30.0 + (h % 68) as f32;
        let py = 16.0 + ((h >> 8) % 30) as f32;
        let r = 5.0 + ((h >> 16) % 6) as f32;
        if inside(px - r, py) && inside(px + r, py) {
            c.poly(
                &[
                    (px - r, py + r * 0.3),
                    (px - r * 0.4, py - r * 0.55),
                    (px + r * 0.6, py - r * 0.4),
                    (px + r, py + r * 0.35),
                    (px, py + r * 0.6),
                ],
                rgba([66, 62, 66]),
            );
            c.line(px - r * 0.4, py - r * 0.55, px + r * 0.6, py - r * 0.4, 1.2, rgba([88, 84, 88]));
            c.line(px - r * 0.4, py - r * 0.5, px - r * 0.2, py + r * 0.5, 1.0, rgba([44, 41, 44]));
        }
    }
    c
}

/// Cliff wall face hanging under a tile's lower-left or lower-right edge.
/// Canvas is 128x(64+CLIFF_H); the wall occupies one half, following the
/// diamond's bottom edge, extruded down by CLIFF_H. Sediment strata warped
/// by noise, occasional rusted support beams, bright top rim fading to
/// near-black at the base.
fn cliff_face(left: bool) -> Canvas {
    let mut c = Canvas::new(128, 64 + CLIFF_H);
    for x in 0..128i32 {
        let on_half = if left { x < 64 } else { x >= 64 };
        if !on_half {
            continue;
        }
        let dx = (x as f32 + 0.5 - 64.0) / 64.0;
        let edge_y = 32.0 + 32.0 * (1.0 - dx.abs());
        // Rusted support beams punctuate long walls.
        let beam = hash2(x / 14, 0, if left { 61 } else { 62 }) % 5 == 0 && x % 14 < 3;
        for wy in 0..CLIFF_H {
            let y = edge_y as i32 + wy;
            let h = hash2(x, wy, if left { 7 } else { 8 });
            let base: [u8; 3] = if left { [58, 52, 46] } else { [74, 66, 57] };
            // Strata bands displaced by noise so they undulate.
            let warp = (vnoise(x as f32, 0.0, 20.0, 313) * 8.0) as i32;
            let strata = ((wy + warp) / 8) % 2 == 0;
            let mut f = if strata { 1.0 } else { 0.86 };
            if h % 19 == 0 {
                f *= 1.14;
            }
            if h % 23 == 0 {
                f *= 0.74;
            }
            if beam {
                f *= 0.62; // dark girder inset
            }
            // Depth: fade toward the base; bright sunlit rim at the top.
            let depth = wy as f32 / CLIFF_H as f32;
            f *= 1.0 - depth * 0.45;
            if wy <= 1 {
                f = 1.55;
            }
            c.set(x, y, rgba(scale_rgb(base, f)));
            if beam && h % 7 == 0 {
                c.blend(x, y, [110, 60, 34, 90]); // rust bleeding off the girder
            }
        }
    }
    c
}

// -------------------------------------------------------------- minerals ----

fn mineral_cluster(salt: u32) -> Canvas {
    let mut c = Canvas::new(120, 104);
    let body = [58, 180, 215];
    let lite = [132, 232, 255];
    let dark = [24, 95, 140];
    let glint = [225, 252, 255];
    // Faint energy pool where the cluster meets the ground.
    c.glow(60.0, 84.0, 36.0, [40, 160, 200], 0.30);
    // (base_x, base_y, width, height); tallest shard in the middle.
    let shards: [(f32, f32, f32, f32); 4] = [
        (36.0, 76.0, 17.0, 38.0),
        (62.0, 70.0, 21.0, 52.0),
        (88.0, 78.0, 14.0, 32.0),
        (50.0, 86.0, 12.0, 24.0),
    ];
    for (i, &(x, y, w, h)) in shards.iter().enumerate() {
        if (salt == 1 && i == 3) || (salt == 2 && i >= 2) {
            continue; // depleted variants
        }
        let tip = (x + 5.0, y - h);
        // Three hard facets — lit left, body center, deep right — plus a
        // small base diamond so the crystal sits IN the ground.
        c.poly(&[(x - w * 0.5, y - 2.0), (x, y + w * 0.16), (x + w * 0.5, y - 3.0), (x, y - w * 0.2)], rgba(scale_rgb(body, 0.7)));
        c.poly(&[(x - w * 0.5, y - 2.0), (x - w * 0.12, y + w * 0.10), (tip.0 - 1.5, tip.1)], rgba(lite));
        c.poly(&[(x - w * 0.12, y + w * 0.10), (x + w * 0.22, y + w * 0.04), (tip.0, tip.1)], rgba(body));
        c.poly(&[(x + w * 0.22, y + w * 0.04), (x + w * 0.5, y - 3.0), (tip.0 + 1.5, tip.1)], rgba(dark));
        // Glint running up the lit edge.
        c.line(x - w * 0.34, y - h * 0.16, tip.0 - 1.5, tip.1 + 2.0, 1.6, rgba(glint));
        // Internal light concentrated low in the shard.
        c.glow(x + 1.0, y - h * 0.18, w * 0.7, [96, 224, 252], 0.4);
    }
    // Chips scattered around the base.
    for k in 0..5 {
        let h = hash2(k, salt as i32, 87);
        let px = 18.0 + (h % 84) as f32;
        let py = 82.0 + ((h >> 8) % 14) as f32;
        c.poly(&[(px - 3.0, py), (px - 0.5, py - 4.5), (px + 3.0, py), (px, py + 2.0)], rgba(body));
        c.line(px - 2.0, py - 1.0, px - 0.5, py - 3.5, 1.0, rgba(lite));
    }
    c.outline_t([10, 34, 52, 255], 2);
    c.rim([10, 34, 52, 255], 1.22);
    c
}

// ----------------------------------------------------------------- units ----

fn facing_vec(f: usize) -> (f32, f32) {
    let a = f as f32 * std::f32::consts::FRAC_PI_4;
    (a.cos(), a.sin())
}

const GUNMETAL: [u8; 3] = [64, 70, 80];
const GUNMETAL_DARK: [u8; 3] = [42, 46, 54];
const STEEL_LIT: [u8; 3] = [126, 134, 148];
const AMBER: [u8; 3] = [255, 186, 84];
const VISOR: [u8; 3] = [120, 235, 255];
const OUTLINE: Color = [10, 11, 15, 255];

/// Angular armor plate: quad with a lit top-left edge and shadowed bottom.
fn plate(c: &mut Canvas, pts: &[(f32, f32)], base: [u8; 3], f: f32) {
    c.poly(pts, rgba(scale_rgb(base, f)));
    // Lit edge along the first segment.
    c.line(pts[0].0, pts[0].1, pts[1].0, pts[1].1, 1.6, rgba(scale_rgb(base, f * 1.45)));
    let n = pts.len();
    c.line(pts[n - 2].0, pts[n - 2].1, pts[n - 1].0, pts[n - 1].1, 1.4, rgba(scale_rgb(base, f * 0.6)));
}

/// Trooper: hard-shell infantry. 104x112.
fn paint_trooper(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(104, 112);
    let (dx, dy) = facing_vec(f);
    let cx = 52.0;
    let cy = 60.0;
    let gun_behind = dy < -0.3;

    let gun = |c: &mut Canvas| {
        let gx0 = cx + dx * 12.0;
        let gy0 = cy - 4.0 + dy * 6.0;
        let gx1 = gx0 + dx * 38.0;
        let gy1 = gy0 + dy * 20.0;
        // Receiver + barrel, chunky, with a muzzle block and a glow cell.
        c.line(gx0, gy0, gx1, gy1, 7.0, rgba(GUNMETAL_DARK));
        c.line(gx0 + dx * 6.0, gy0 + dy * 3.0, gx1, gy1, 3.5, rgba([30, 33, 40]));
        c.line(gx1 - dx * 5.0, gy1 - dy * 2.5, gx1, gy1, 5.5, rgba(STEEL_LIT));
        // Underslung grip.
        c.line(gx0 + dx * 12.0, gy0 + dy * 6.0 + 3.0, gx0 + dx * 12.0, gy0 + dy * 6.0 + 9.0, 3.0, rgba(GUNMETAL_DARK));
        // Power cell glow.
        c.glow(gx0 + dx * 18.0, gy0 + dy * 9.0 - 1.0, 4.5, team, 0.9);
        c.set((gx0 + dx * 18.0) as i32, (gy0 + dy * 9.0 - 1.0) as i32, rgba(scale_rgb(team, 1.5)));
    };
    if gun_behind {
        gun(&mut c);
    }

    // Legs: angular greaves with knee plates; walk cycle alternates lift.
    let lift = if frame == 0 { 0.0 } else { 4.0 };
    plate(&mut c, &[(34.0, 80.0 + lift), (46.0, 80.0 + lift), (44.0, 102.0 - lift * 0.5), (36.0, 102.0 - lift * 0.5)], GUNMETAL_DARK, 1.0);
    plate(&mut c, &[(56.0, 84.0 - lift), (68.0, 84.0 - lift), (70.0, 102.0 + lift * 0.3), (58.0, 102.0 + lift * 0.3)], GUNMETAL_DARK, 0.9);
    // Knee plates.
    c.poly(&[(35.0, 82.0 + lift), (45.0, 82.0 + lift), (40.0, 89.0 + lift)], rgba(GUNMETAL));
    c.poly(&[(57.0, 86.0 - lift), (67.0, 86.0 - lift), (62.0, 93.0 - lift)], rgba(GUNMETAL));
    // Boots.
    c.rect(32, 100, 16, 8, rgba([32, 35, 42]));
    c.rect(56, 102, 16, 7, rgba([28, 30, 36]));

    // Torso: faceted carapace — broad shoulders, tapered waist.
    plate(&mut c, &[(30.0, 48.0), (74.0, 48.0), (68.0, 76.0), (38.0, 76.0)], GUNMETAL, 1.0);
    // Chest angle facet toward facing.
    c.poly(&[(34.0 + dx * 4.0, 50.0), (52.0 + dx * 8.0, 46.0 + dy * 3.0), (52.0 + dx * 8.0, 66.0), (38.0 + dx * 4.0, 70.0)], rgba(scale_rgb(GUNMETAL, 1.16)));
    // Waist band.
    c.rect(38, 72, 30, 5, rgba(GUNMETAL_DARK));
    // Team power core + conduit.
    let corex = cx + dx * 7.0;
    let corey = cy - 2.0 + dy * 4.0;
    c.glow(corex, corey, 8.0, team, 0.85);
    c.poly(&[(corex - 3.0, corey - 4.0), (corex + 3.0, corey - 4.0), (corex + 2.0, corey + 4.0), (corex - 2.0, corey + 4.0)], rgba(scale_rgb(team, 1.35)));
    c.line(corex, corey - 4.0, cx - 6.0, 40.0, 1.6, rgba(scale_rgb(team, 0.8)));

    // Pauldrons: heavy angular slabs with a team chevron.
    plate(&mut c, &[(16.0, 42.0), (36.0, 38.0), (38.0, 52.0), (20.0, 56.0)], STEEL_LIT, 0.94);
    plate(&mut c, &[(68.0, 38.0), (88.0, 42.0), (84.0, 56.0), (66.0, 52.0)], GUNMETAL, 0.9);
    c.line(20.0, 46.0, 32.0, 43.0, 2.4, rgba(team));
    c.line(72.0, 43.0, 84.0, 46.0, 2.4, rgba(team));

    // Helmet: angular dome with a glowing visor slit (or backpack when
    // facing away).
    plate(&mut c, &[(42.0, 22.0), (62.0, 22.0), (64.0, 36.0), (40.0, 36.0)], scale_rgb(GUNMETAL, 1.12), 1.0);
    c.poly(&[(42.0, 22.0), (52.0, 17.0), (62.0, 22.0)], rgba(STEEL_LIT));
    if dy > -0.5 {
        let vx = cx + dx * 6.0;
        let vy = 29.0 + dy * 3.0;
        c.line(vx - 6.0, vy, vx + 6.0, vy, 2.6, rgba(VISOR));
        c.glow(vx, vy, 7.0, VISOR, 0.7);
    } else {
        c.rect(44, 24, 16, 12, rgba(GUNMETAL_DARK));
        c.rect(46, 26, 5, 8, rgba([56, 62, 72]));
        c.rect(53, 26, 5, 8, rgba([56, 62, 72]));
        c.glow(50.0, 38.0, 4.0, AMBER, 0.5);
    }

    if !gun_behind {
        gun(&mut c);
    }
    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.3);
    c
}

/// Fabricator: tracked engineering rig. 96x96.
fn paint_worker(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(96, 96);
    let (dx, dy) = facing_vec(f);
    let cx = 48.0;
    let cy = 52.0;
    let bob = if frame == 0 { 0.0 } else { -2.0 };

    // Tracked chassis: two angular track pods with moving tread notches.
    plate(&mut c, &[(20.0, 70.0), (76.0, 70.0), (80.0, 84.0), (16.0, 84.0)], [38, 41, 48], 1.0);
    for k in 0..7 {
        let notch = 20 + k * 8 + (frame as i32) * 4;
        if notch < 76 {
            c.rect(notch, 74, 2, 8, rgba([70, 76, 88]));
        }
    }
    c.rect(14, 82, 68, 4, rgba([26, 28, 34]));

    // Cab: mustard industrial box with grime and a hazard band.
    let mustard = [164, 136, 62];
    plate(&mut c, &[(26.0, 40.0 + bob), (70.0, 40.0 + bob), (74.0, 68.0), (22.0, 68.0)], mustard, 1.0);
    c.poly(&[(26.0, 40.0 + bob), (34.0, 32.0 + bob), (64.0, 32.0 + bob), (70.0, 40.0 + bob)], rgba(scale_rgb(mustard, 1.2)));
    // Hazard chevrons on the skirt.
    for k in 0..6 {
        let x = 26 + k * 8;
        let col = if k % 2 == 0 { [30, 30, 34] } else { [190, 158, 60] };
        c.poly(&[(x as f32, 64.0), (x as f32 + 4.0, 64.0), (x as f32 + 8.0, 68.0), (x as f32 + 4.0, 68.0)], rgba(col));
    }
    // Grime streaks.
    for k in 0..4 {
        let h = hash2(k, 5, 313);
        c.streak(30 + (h % 36) as i32, 46, 8 + (h % 8) as i32, [60, 50, 30], 0.35);
    }
    // Glazed cab window toward facing.
    if dy > -0.5 {
        let wx = cx + dx * 12.0;
        let wy = 42.0 + bob + dy * 5.0;
        c.poly(&[(wx - 8.0, wy - 4.0), (wx + 8.0, wy - 4.0), (wx + 6.0, wy + 5.0), (wx - 6.0, wy + 5.0)], rgba([110, 210, 235]));
        c.line(wx - 7.0, wy - 3.0, wx + 7.0, wy - 3.0, 1.2, rgba([200, 245, 255]));
    } else {
        // Engine grill + exhaust when facing away.
        c.rect(38, (44.0 + bob) as i32, 20, 10, rgba(GUNMETAL_DARK));
        c.glow(58.0, 40.0 + bob, 4.0, AMBER, 0.5);
    }
    // Team panel on the roof.
    c.rect(30, (34.0 + bob) as i32, 10, 5, rgba(team));
    // Manipulator arm toward facing with hydraulic elbow + claw.
    let ax = cx + dx * 26.0;
    let ay = cy + bob + dy * 14.0;
    c.line(cx + dx * 10.0, cy + bob - 2.0, cx + dx * 20.0, cy + bob - 8.0, 4.5, rgba(GUNMETAL_DARK));
    c.line(cx + dx * 20.0, cy + bob - 8.0, ax, ay, 3.5, rgba(GUNMETAL));
    c.line(ax, ay, ax + dx * 5.0, ay + dy * 3.0 - 3.0, 2.5, rgba(STEEL_LIT));
    c.line(ax, ay, ax + dx * 5.0, ay + dy * 3.0 + 3.0, 2.5, rgba(STEEL_LIT));
    // Beacon (blinks with frame).
    if frame == 1 {
        c.glow(cx - 14.0, 30.0 + bob, 5.0, AMBER, 0.9);
    }
    c.set((cx - 14.0) as i32, (30.0 + bob) as i32, rgba(AMBER));

    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.25);
    c
}

/// Vanguard: heavy shock trooper, twin energy blades. 128x128.
fn paint_vanguard(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(128, 128);
    let (dx, dy) = facing_vec(f);
    let cx = 64.0;
    let cy = 68.0;

    let lift = if frame == 0 { 0.0 } else { 4.0 };
    // Wide-stance legs: massive greaves.
    plate(&mut c, &[(38.0, 92.0 + lift), (54.0, 92.0 + lift), (52.0, 118.0 - lift * 0.5), (40.0, 118.0 - lift * 0.5)], GUNMETAL_DARK, 1.0);
    plate(&mut c, &[(74.0, 96.0 - lift), (90.0, 96.0 - lift), (92.0, 120.0 + lift * 0.3), (78.0, 120.0 + lift * 0.3)], GUNMETAL_DARK, 0.9);
    c.poly(&[(39.0, 94.0 + lift), (53.0, 94.0 + lift), (46.0, 104.0 + lift)], rgba(GUNMETAL));
    c.poly(&[(75.0, 98.0 - lift), (89.0, 98.0 - lift), (82.0, 108.0 - lift)], rgba(GUNMETAL));
    c.rect(36, 114, 20, 10, rgba([30, 32, 38]));
    c.rect(76, 118, 20, 9, rgba([28, 30, 36]));

    // Energy blades on both forearms.
    let blades = |c: &mut Canvas| {
        for side in [-1.0f32, 1.0] {
            let ox = -dy * side * 28.0;
            let oy = dx * side * 18.0;
            let px = cx + dx * 12.0 + ox;
            let py = cy + dy * 8.0 + oy;
            // Forearm housing.
            plate(c, &[(px - 6.0, py - 5.0), (px + 6.0, py - 5.0), (px + 5.0, py + 5.0), (px - 5.0, py + 5.0)], scale_rgb(GUNMETAL, 1.1), 1.0);
            // Blade: team edge, white-hot core, halo.
            let bx = px + dx * 34.0;
            let by = py + dy * 20.0;
            c.line(px + dx * 6.0, py + dy * 4.0, bx, by, 8.0, [team[0], team[1], team[2], 110]);
            c.line(px + dx * 6.0, py + dy * 4.0, bx, by, 4.5, rgba(team));
            c.line(px + dx * 9.0, py + dy * 6.0, bx - dx * 1.5, by - dy * 1.0, 2.0, rgba([255, 255, 255]));
            c.glow(bx, by, 7.0, team, 0.8);
        }
    };
    let behind = dy < -0.3;
    if behind {
        blades(&mut c);
    }

    // Torso: broad faceted carapace with a vertical power core.
    plate(&mut c, &[(32.0, 46.0), (96.0, 46.0), (88.0, 88.0), (40.0, 88.0)], GUNMETAL, 0.96);
    c.poly(&[(38.0 + dx * 5.0, 50.0), (64.0 + dx * 10.0, 44.0 + dy * 4.0), (64.0 + dx * 10.0, 74.0), (44.0 + dx * 5.0, 82.0)], rgba(scale_rgb(GUNMETAL, 1.18)));
    c.rect(42, 84, 44, 6, rgba(GUNMETAL_DARK));
    // Core slit.
    let corex = cx + dx * 9.0;
    c.glow(corex, 64.0, 11.0, team, 0.8);
    c.rect((corex - 2.5) as i32, 54, 5, 20, rgba(scale_rgb(team, 1.3)));

    // Massive pauldrons.
    plate(&mut c, &[(8.0, 36.0), (40.0, 30.0), (44.0, 52.0), (14.0, 58.0)], STEEL_LIT, 0.95);
    plate(&mut c, &[(88.0, 30.0), (120.0, 36.0), (114.0, 58.0), (84.0, 52.0)], GUNMETAL, 0.88);
    c.line(14.0, 42.0, 36.0, 37.0, 3.5, rgba(team));
    c.line(92.0, 37.0, 114.0, 42.0, 3.5, rgba(team));
    // Pauldron kill-spikes.
    c.poly(&[(10.0, 38.0), (16.0, 26.0), (20.0, 37.0)], rgba(STEEL_LIT));
    c.poly(&[(108.0, 37.0), (112.0, 26.0), (118.0, 38.0)], rgba(GUNMETAL));

    // Helm with crest fin.
    plate(&mut c, &[(52.0, 26.0), (76.0, 26.0), (78.0, 42.0), (50.0, 42.0)], scale_rgb(GUNMETAL, 1.14), 1.0);
    c.poly(&[(62.0, 8.0), (66.0, 8.0), (68.0, 28.0), (60.0, 28.0)], rgba(team));
    c.glow(64.0, 10.0, 5.0, team, 0.6);
    if dy > -0.5 {
        let vx = cx + dx * 7.0;
        let vy = 34.0 + dy * 3.0;
        c.line(vx - 7.0, vy, vx + 7.0, vy, 3.0, rgba([255, 150, 70]));
        c.glow(vx, vy, 8.0, [255, 150, 70], 0.7);
    } else {
        c.rect(56, 30, 16, 12, rgba(GUNMETAL_DARK));
        c.glow(64.0, 44.0, 6.0, AMBER, 0.5);
    }

    if !behind {
        blades(&mut c);
    }
    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.3);
    c
}

/// Breaker: siege tank, mobile mode. 136x120.
fn paint_breaker(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(136, 120);
    let (dx, dy) = facing_vec(f);
    let cx = 68.0;
    let cy = 68.0;
    // Twin treads with drive wheels and moving links.
    for side in [-1.0f32, 1.0] {
        let ty = cy + 20.0 + side * 10.0;
        plate(&mut c, &[(20.0, ty - 8.0), (116.0, ty - 8.0), (120.0, ty + 8.0), (16.0, ty + 8.0)], [36, 39, 46], 1.0);
        for k in 0..9 {
            let notch = 22 + k * 11 + (frame as i32) * 5;
            if notch < 114 {
                c.rect(notch, ty as i32 - 4, 3, 10, rgba([66, 72, 84]));
            }
        }
        for k in 0..4 {
            c.dome(34.0 + k as f32 * 22.0, ty + 1.0, 6.0, 5.0, [52, 56, 66]);
        }
    }
    // Hull: sloped glacis + upper deck with panel seams.
    plate(&mut c, &[(24.0, 62.0), (112.0, 62.0), (104.0, 84.0), (32.0, 84.0)], GUNMETAL_DARK, 1.0);
    plate(&mut c, &[(30.0, 46.0), (106.0, 46.0), (112.0, 62.0), (24.0, 62.0)], GUNMETAL, 1.05);
    c.line(48.0, 48.0, 44.0, 62.0, 1.4, rgba(GUNMETAL_DARK));
    c.line(88.0, 48.0, 92.0, 62.0, 1.4, rgba(GUNMETAL_DARK));
    // Team stripe along the hull side.
    c.rect(30, 64, 76, 4, rgba(team));
    // Engine vents at the rear with amber heat glow.
    c.rect(30, 52, 12, 8, rgba([30, 33, 40]));
    c.glow(36.0, 56.0, 6.0, AMBER, 0.5);
    // Turret: angular, rotates with facing; long barrel + muzzle brake.
    let tx = cx + dx * 4.0;
    let ty = cy - 22.0 + dy * 3.0;
    plate(&mut c, &[(tx - 20.0, ty - 4.0), (tx + 14.0, ty - 8.0), (tx + 20.0, ty + 8.0), (tx - 16.0, ty + 12.0)], scale_rgb(GUNMETAL, 1.12), 1.0);
    c.glow(tx - dx * 6.0, ty + 2.0, 5.0, team, 0.8);
    c.rect((tx - dx * 6.0 - 2.0) as i32, (ty) as i32, 4, 4, rgba(scale_rgb(team, 1.3)));
    let bx = tx + dx * 52.0;
    let by = ty + dy * 26.0;
    c.line(tx + dx * 12.0, ty + dy * 6.0, bx, by, 6.0, rgba(GUNMETAL_DARK));
    c.line(tx + dx * 16.0, ty + dy * 8.0, bx - dx * 3.0, by - dy * 1.5, 3.0, rgba([32, 35, 42]));
    c.line(bx - dx * 6.0, by - dy * 3.0, bx, by, 8.0, rgba(STEEL_LIT));
    // Antenna.
    c.line(tx - 14.0, ty - 6.0, tx - 18.0, ty - 22.0, 1.3, rgba([90, 96, 108]));
    c.set((tx - 18.0) as i32, (ty - 23.0) as i32, rgba([255, 90, 80]));
    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.28);
    c
}

/// Breaker deployed: stabilizers out, elevated long-barrel. 152x136.
fn paint_breaker_sieged(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(152, 136);
    let (dx, dy) = facing_vec(f);
    let cx = 76.0;
    let cy = 78.0;
    // Splayed stabilizer legs with piston shine + feet.
    for (lx, ly) in [(-46.0f32, 26.0f32), (46.0, 26.0), (-34.0, 42.0), (34.0, 42.0)] {
        c.line(cx, cy + 6.0, cx + lx, cy + ly, 7.0, rgba(GUNMETAL_DARK));
        c.line(cx + lx * 0.4, cy + 6.0 + ly * 0.4, cx + lx * 0.8, cy + 6.0 + ly * 0.75, 2.5, rgba(STEEL_LIT));
        plate(&mut c, &[(cx + lx - 9.0, cy + ly - 2.0), (cx + lx + 9.0, cy + ly - 2.0), (cx + lx + 11.0, cy + ly + 6.0), (cx + lx - 11.0, cy + ly + 6.0)], [34, 37, 44], 1.0);
    }
    // Raised hull.
    plate(&mut c, &[(36.0, 56.0), (116.0, 56.0), (108.0, 82.0), (44.0, 82.0)], GUNMETAL, 1.0);
    c.rect(44, 74, 64, 5, rgba(team));
    c.line(60.0, 58.0, 56.0, 74.0, 1.4, rgba(GUNMETAL_DARK));
    c.line(92.0, 58.0, 96.0, 74.0, 1.4, rgba(GUNMETAL_DARK));
    // Recoil shield: big angular wedge behind the gun.
    let sx = cx - dx * 10.0;
    let sy = cy - 34.0;
    plate(&mut c, &[(sx - 18.0, sy - 6.0), (sx + 10.0, sy - 12.0), (sx + 16.0, sy + 10.0), (sx - 14.0, sy + 14.0)], scale_rgb(GUNMETAL, 1.15), 1.0);
    // Elevated siege barrel: two segments, muzzle brake, charge glow.
    let bx = cx + dx * 66.0;
    let by = sy - 12.0 + dy * 30.0;
    c.line(sx + dx * 10.0, sy + dy * 4.0, bx, by, 7.0, rgba(GUNMETAL_DARK));
    c.line(sx + dx * 16.0, sy + dy * 7.0, bx - dx * 4.0, by - dy * 2.0, 3.5, rgba([32, 35, 42]));
    c.line(bx - dx * 8.0, by - dy * 4.0, bx, by, 9.0, rgba(STEEL_LIT));
    let chg = if frame == 0 { 0.5 } else { 0.9 };
    c.glow(bx, by, 7.0, team, chg);
    // Warning strobes while deployed.
    if frame == 1 {
        c.glow(44.0, 52.0, 6.0, AMBER, 0.9);
        c.glow(108.0, 52.0, 6.0, AMBER, 0.9);
    }
    c.set(44, 52, rgba(AMBER));
    c.set(108, 52, rgba(AMBER));
    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.28);
    c
}

/// Skywing: delta-wing gunship. 144x120.
fn paint_skywing(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(144, 120);
    let (dx, dy) = facing_vec(f);
    let (dy_i, px, py) = (dy * 0.6, 72.0, 60.0);
    let nose = (px + dx * 40.0, py + dy_i * 40.0);
    let tail = (px - dx * 30.0, py - dy_i * 30.0);
    let (wx, wy) = (-dy_i, dx * 0.6);
    // Swept delta wings: angular panels with team edge stripes.
    for side in [-1.0f32, 1.0] {
        let root_f = (px + dx * 12.0, py + dy_i * 12.0);
        let root_b = (px - dx * 16.0, py - dy_i * 16.0);
        let tip = (px - dx * 22.0 + wx * 44.0 * side, py - dy_i * 22.0 + wy * 44.0 * side);
        let shade = if side < 0.0 { 1.05 } else { 0.85 };
        c.poly(&[root_f, tip, root_b], rgba(scale_rgb(GUNMETAL, shade)));
        // Leading-edge team stripe.
        c.line(root_f.0, root_f.1, tip.0, tip.1, 2.6, rgba(team));
        // Wingtip nav light.
        c.glow(tip.0, tip.1, 4.0, if side < 0.0 { [255, 80, 70] } else { [90, 255, 120] }, 0.8);
    }
    // Fuselage: dart-shaped hull with a raised spine.
    c.poly(&[
        (nose.0, nose.1),
        (px + dx * 4.0 + wx * 9.0, py + dy_i * 4.0 + wy * 9.0),
        (tail.0 + wx * 6.0, tail.1 + wy * 6.0),
        (tail.0 - wx * 6.0, tail.1 - wy * 6.0),
        (px + dx * 4.0 - wx * 9.0, py + dy_i * 4.0 - wy * 9.0),
    ], rgba(GUNMETAL));
    c.line(nose.0, nose.1, tail.0, tail.1, 3.0, rgba(scale_rgb(GUNMETAL, 1.25)));
    // Chin gun pod.
    c.line(px + dx * 16.0, py + dy_i * 16.0 + 4.0, px + dx * 30.0, py + dy_i * 30.0 + 4.0, 2.5, rgba([32, 35, 42]));
    // Canopy glow.
    c.poly(&[
        (px + dx * 18.0 + wx * 4.0, py + dy_i * 18.0 + wy * 4.0),
        (px + dx * 28.0, py + dy_i * 28.0),
        (px + dx * 18.0 - wx * 4.0, py + dy_i * 18.0 - wy * 4.0),
        (px + dx * 12.0, py + dy_i * 12.0),
    ], rgba([110, 220, 245]));
    c.glow(px + dx * 20.0, py + dy_i * 20.0, 6.0, VISOR, 0.6);
    // Twin engines: bright cores + flame trail flicker.
    let g = if frame == 0 { 1.0 } else { 1.45 };
    for side in [-1.0f32, 1.0] {
        let ex = tail.0 + wx * 8.0 * side;
        let ey = tail.1 + wy * 8.0 * side;
        c.glow(ex, ey, 9.0 * g, [90, 180, 255], 0.8);
        c.ellipse(ex, ey, 3.5, 3.0, rgba([210, 240, 255]));
        c.line(ex, ey, ex - dx * 10.0 * g, ey - dy_i * 10.0 * g, 2.0, [150, 210, 255, 160]);
    }
    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.25);
    c
}

/// Stormcaller: robed psionic with a crackling orb. 104x128.
fn paint_stormcaller(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(104, 128);
    let (dx, dy) = facing_vec(f);
    let cx = 52.0;
    let cy = 72.0;
    let robe = [46, 48, 68];
    // Robe: tall silhouette with a jagged, wind-torn hem.
    c.poly(&[
        (cx - 16.0, cy + 34.0), (cx - 10.0, cy + 28.0), (cx - 5.0, cy + 35.0),
        (cx + 2.0, cy + 27.0), (cx + 8.0, cy + 34.0), (cx + 14.0, cy + 29.0),
        (cx + 18.0, cy + 20.0), (cx + 12.0, cy - 24.0), (cx - 12.0, cy - 24.0), (cx - 18.0, cy + 20.0),
    ], rgba(robe));
    // Lit fold + deep shadow folds.
    c.line(cx - 10.0, cy - 20.0, cx - 14.0, cy + 28.0, 2.5, rgba(scale_rgb(robe, 1.35)));
    c.line(cx + 2.0, cy - 18.0, cx + 4.0, cy + 24.0, 2.0, rgba(scale_rgb(robe, 0.62)));
    c.line(cx + 10.0, cy - 16.0, cx + 13.0, cy + 22.0, 1.6, rgba(scale_rgb(robe, 0.62)));
    // Team sash + belt sigil.
    c.line(cx - 10.0, cy - 14.0, cx + 10.0, cy + 12.0, 4.0, rgba(team));
    c.glow(cx, cy + 2.0, 6.0, team, 0.6);
    // Shoulder cowl: angular.
    plate(&mut c, &[(cx - 16.0, cy - 28.0), (cx + 16.0, cy - 28.0), (cx + 12.0, cy - 16.0), (cx - 12.0, cy - 16.0)], scale_rgb(robe, 1.15), 1.0);
    // Hood: peaked, face in shadow with glowing eyes.
    c.poly(&[(cx - 9.0, cy - 30.0), (cx, cy - 48.0), (cx + 9.0, cy - 30.0), (cx + 7.0, cy - 24.0), (cx - 7.0, cy - 24.0)], rgba(scale_rgb(robe, 1.2)));
    c.poly(&[(cx - 5.0, cy - 32.0), (cx + 5.0, cy - 32.0), (cx + 4.0, cy - 25.0), (cx - 4.0, cy - 25.0)], rgba([14, 14, 22]));
    if dy > -0.4 {
        let ex = cx + dx * 2.5;
        c.set((ex - 2.0) as i32, (cy - 29.0) as i32, rgba([150, 240, 255]));
        c.set((ex + 2.0) as i32, (cy - 29.0) as i32, rgba([150, 240, 255]));
        c.glow(ex, cy - 29.0, 5.0, VISOR, 0.6);
    }
    // Storm orb held toward facing: layered glow + crackling arcs.
    let ox = cx + dx * 28.0;
    let oy = cy - 16.0 + dy * 12.0;
    c.glow(ox, oy, 16.0, [64, 210, 230], 0.55);
    c.ellipse(ox, oy, 7.0, 7.0, rgba([64, 210, 230]));
    c.ellipse(ox - 2.0, oy - 2.0, 3.0, 3.0, rgba([230, 255, 255]));
    let a0 = if frame == 0 { 0.6f32 } else { 2.2 };
    for k in 0..4 {
        let a = a0 + k as f32 * 1.65;
        let r1 = 12.0 + (hash2(k, f as i32, 89) % 6) as f32;
        // Jagged two-segment arc.
        let mx = ox + (a.cos() * r1 * 0.6) + 2.0;
        let my = oy + (a.sin() * r1 * 0.6) - 2.0;
        c.line(ox, oy, mx, my, 1.4, [170, 245, 255, 220]);
        c.line(mx, my, ox + a.cos() * r1, oy + a.sin() * r1, 1.2, [170, 245, 255, 180]);
    }
    // Arm reaching to the orb.
    c.line(cx + dx * 8.0, cy - 14.0, ox - dx * 6.0, oy + 2.0, 3.5, rgba(scale_rgb(robe, 1.1)));
    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.3);
    c
}

// ----------------------------------------------------- Kyth Assembly ----

const CHITIN: [u8; 3] = [58, 46, 70];
const CHITIN_LIGHT: [u8; 3] = [110, 90, 128];
const MEMBRANE: [u8; 3] = [142, 88, 70];
const KYTH_GLOW: [u8; 3] = [158, 255, 96];
const LEG: Color = [38, 30, 44, 255];

/// Jointed insect leg: coxa -> knee -> sharp tip.
fn kleg(c: &mut Canvas, x0: f32, y0: f32, kx: f32, ky: f32, tx: f32, ty: f32, t: f32) {
    c.line(x0, y0, kx, ky, t, LEG);
    c.line(kx, ky, tx, ty, t * 0.7, LEG);
    c.set(kx as i32, ky as i32, rgba(CHITIN_LIGHT));
}

/// Scalloped plate edge: bright crescent along a carapace segment.
fn scallop(c: &mut Canvas, cx: f32, cy: f32, r: f32, w: f32) {
    for k in 0..(w as i32) {
        let a = std::f32::consts::PI * (0.15 + 0.7 * k as f32 / w);
        let x = cx + a.cos() * r;
        let y = cy + a.sin() * r * 0.6;
        c.set(x as i32, y as i32, rgba(CHITIN_LIGHT));
        c.set(x as i32, y as i32 + 1, rgba(scale_rgb(CHITIN_LIGHT, 0.7)));
    }
}

/// Drone: segmented hover-bug worker. 96x88.
fn paint_kdrone(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(96, 88);
    let (dx, dy) = facing_vec(f);
    let cx = 48.0;
    let cy = 46.0;
    let bob = if frame == 0 { 0.0 } else { -3.0 };
    // Dangling jointed legs.
    for (k, side) in [-1.0f32, 1.0].iter().enumerate() {
        let lo = if (frame + k) % 2 == 0 { 3.0 } else { 0.0 };
        kleg(&mut c, cx - side * 6.0, cy + bob + 10.0, cx - side * 18.0, cy + 18.0 + lo, cx - side * 22.0, cy + 30.0 - lo, 3.0);
        kleg(&mut c, cx + side * 2.0, cy + bob + 12.0, cx + side * 10.0, cy + 22.0 - lo, cx + side * 12.0, cy + 32.0 + lo, 2.6);
    }
    // Abdomen: two chitin segments with scalloped plate edges.
    c.dome(cx - dx * 10.0, cy + bob + 2.0, 20.0, 15.0, CHITIN);
    scallop(&mut c, cx - dx * 10.0, cy + bob + 2.0, 17.0, 22.0);
    c.dome(cx + dx * 8.0, cy + bob - 2.0 + dy * 5.0, 14.0, 11.0, scale_rgb(CHITIN, 1.18));
    // Dorsal glow slit.
    c.line(cx - dx * 18.0 - 4.0, cy + bob - 6.0, cx - dx * 2.0 + 4.0, cy + bob - 8.0, 2.0, rgba(KYTH_GLOW));
    c.glow(cx - dx * 8.0, cy + bob - 7.0, 8.0, KYTH_GLOW, 0.5);
    // Team plate on the back segment.
    c.poly(&[(cx - dx * 14.0 - 5.0, cy + bob - 2.0), (cx - dx * 14.0 + 3.0, cy + bob - 5.0), (cx - dx * 14.0 + 6.0, cy + bob + 3.0), (cx - dx * 14.0 - 2.0, cy + bob + 5.0)], rgba(team));
    // Head: mandibles + paired glowing eyes toward facing.
    if dy > -0.5 {
        let hx = cx + dx * 18.0;
        let hy = cy + bob + dy * 7.0;
        c.line(hx, hy + 2.0, hx + dx * 8.0, hy + dy * 4.0 + 4.0, 2.2, LEG);
        c.line(hx + 2.0, hy + 2.0, hx + dx * 8.0 + 2.0, hy + dy * 4.0 - 2.0, 2.2, LEG);
        c.set((hx + dx * 2.0 - 2.0) as i32, (hy - 2.0) as i32, rgba(KYTH_GLOW));
        c.set((hx + dx * 2.0 + 2.0) as i32, (hy - 2.0) as i32, rgba(KYTH_GLOW));
        c.glow(hx + dx * 2.0, hy - 2.0, 6.0, KYTH_GLOW, 0.6);
    }
    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.3);
    c
}

/// Skitter: fast blade-bug. 88x80.
fn paint_skitter(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(88, 80);
    let (dx, dy) = facing_vec(f);
    let cx = 44.0;
    let cy = 42.0;
    let step = if frame == 0 { 5.0 } else { -5.0 };
    // Four spread legs, alternating stride.
    kleg(&mut c, cx - 6.0, cy + 4.0, cx - 18.0 + step, cy + 10.0, cx - 24.0 + step, cy + 24.0, 2.6);
    kleg(&mut c, cx + 6.0, cy + 4.0, cx + 18.0 - step, cy + 10.0, cx + 24.0 - step, cy + 24.0, 2.6);
    kleg(&mut c, cx - 4.0, cy + 6.0, cx - 10.0 - step, cy + 14.0, cx - 12.0 - step, cy + 26.0, 2.2);
    kleg(&mut c, cx + 4.0, cy + 6.0, cx + 10.0 + step, cy + 14.0, cx + 12.0 + step, cy + 26.0, 2.2);
    // Low angular dart body.
    c.poly(&[
        (cx - dx * 20.0 - dy * 8.0, cy - dy * 10.0 + dx * 5.0),
        (cx + dx * 14.0, cy + dy * 8.0 - 8.0),
        (cx + dx * 20.0, cy + dy * 10.0),
        (cx + dx * 14.0, cy + dy * 8.0 + 6.0),
        (cx - dx * 20.0 + dy * 8.0, cy - dy * 10.0 - dx * 5.0),
    ], rgba(CHITIN));
    c.dome(cx - dx * 4.0, cy - 2.0, 11.0, 7.0, scale_rgb(CHITIN, 1.15));
    scallop(&mut c, cx - dx * 4.0, cy - 2.0, 9.0, 14.0);
    // Twin scythe mandibles: hard tapered blades.
    for side in [-1.0f32, 1.0] {
        let bx = cx + dx * 16.0 - dy * side * 7.0;
        let by = cy + dy * 9.0 + dx * side * 4.0;
        c.poly(&[
            (bx, by - 2.0),
            (bx + dx * 18.0 + dy * side * 3.0, by + dy * 9.0 - 1.0),
            (bx + dx * 10.0, by + dy * 5.0 + 3.0),
        ], rgba(CHITIN_LIGHT));
        c.line(bx, by - 1.0, bx + dx * 16.0 + dy * side * 3.0, by + dy * 8.0 - 1.0, 1.4, rgba(team));
    }
    if dy > -0.5 {
        c.set((cx + dx * 10.0 - 2.0) as i32, (cy + dy * 5.0 - 3.0) as i32, rgba(KYTH_GLOW));
        c.set((cx + dx * 10.0 + 2.0) as i32, (cy + dy * 5.0 - 3.0) as i32, rgba(KYTH_GLOW));
        c.glow(cx + dx * 10.0, cy + dy * 5.0 - 3.0, 5.0, KYTH_GLOW, 0.7);
    }
    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.3);
    c
}

/// Spitter: slug artillery with an acid cannon. 104x96.
fn paint_spitter(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(104, 96);
    let (dx, dy) = facing_vec(f);
    let cx = 52.0;
    let cy = 54.0;
    let squish = if frame == 0 { 0.0 } else { 2.0 };
    // Slug body: three descending segments, scalloped.
    c.dome(cx - dx * 16.0, cy + squish, 16.0, 12.0 - squish * 0.5, MEMBRANE);
    c.dome(cx - dx * 4.0, cy - 2.0 + squish * 0.5, 18.0, 13.0, CHITIN);
    scallop(&mut c, cx - dx * 4.0, cy - 2.0, 15.0, 20.0);
    c.dome(cx + dx * 10.0, cy - 5.0, 12.0, 9.0, scale_rgb(CHITIN, 1.15));
    // Team ridge spines along the back.
    for k in 0..3 {
        let sx = cx - 12.0 + k as f32 * 10.0;
        c.poly(&[(sx - 2.0, cy - 12.0), (sx + 1.0, cy - 22.0 - k as f32), (sx + 4.0, cy - 12.0)], rgba(team));
    }
    // Acid cannon: chitin tube angled up toward facing, glowing chamber.
    let tx = cx + dx * 26.0;
    let ty = cy - 16.0 + dy * 10.0;
    c.line(cx + dx * 8.0, cy - 4.0, tx, ty, 8.0, rgba(CHITIN_LIGHT));
    c.line(cx + dx * 12.0, cy - 6.0, tx, ty, 4.0, rgba(scale_rgb(CHITIN_LIGHT, 0.7)));
    // Muzzle maw + acid glow (charges with frame).
    let chg = if frame == 0 { 0.55 } else { 0.95 };
    c.dome(tx + dx * 3.0, ty + dy * 1.5, 5.0, 4.5, [46, 36, 54]);
    c.glow(tx + dx * 3.0, ty + dy * 1.5, 9.0, KYTH_GLOW, chg);
    c.ellipse(tx + dx * 3.0, ty + dy * 1.5, 2.5, 2.0, rgba(scale_rgb(KYTH_GLOW, 1.2)));
    // Acid drip.
    c.set((tx + dx * 4.0) as i32, (ty + dy * 2.0 + 5.0) as i32, rgba(KYTH_GLOW));
    // Glow chamber on the body.
    c.glow(cx - dx * 2.0, cy + 2.0, 7.0, KYTH_GLOW, 0.4);
    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.28);
    c
}

/// Ravager: towering siege beetle. 136x128.
fn paint_ravager(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(136, 128);
    let (dx, dy) = facing_vec(f);
    let cx = 68.0;
    let cy = 72.0;
    let lift = if frame == 0 { 0.0 } else { 3.0 };
    // Six thick jointed legs.
    for k in 0..3 {
        let off = -20.0 + k as f32 * 20.0;
        let l = if k % 2 == 0 { lift } else { -lift };
        kleg(&mut c, cx + off, cy + 8.0, cx + off - 14.0, cy + 22.0 - l, cx + off - 18.0, cy + 40.0 + l, 4.0);
        kleg(&mut c, cx + off, cy + 8.0, cx + off + 14.0, cy + 24.0 + l, cx + off + 18.0, cy + 42.0 - l, 4.0);
    }
    // Massive tiered carapace with scalloped plate rows.
    c.dome(cx - dx * 8.0, cy - 8.0, 40.0, 30.0, CHITIN);
    scallop(&mut c, cx - dx * 8.0, cy - 4.0, 34.0, 40.0);
    c.dome(cx - dx * 16.0, cy - 20.0, 26.0, 17.0, scale_rgb(CHITIN, 1.15));
    scallop(&mut c, cx - dx * 16.0, cy - 16.0, 22.0, 28.0);
    c.dome(cx - dx * 20.0, cy - 30.0, 15.0, 9.0, CHITIN_LIGHT);
    // Team spine row on the crest.
    for k in 0..4 {
        let sx = cx - 22.0 + k as f32 * 12.0;
        c.poly(&[(sx - 3.0, cy - 34.0), (sx, cy - 48.0 - (k % 2) as f32 * 4.0), (sx + 3.0, cy - 34.0)], rgba(team));
        c.glow(sx, cy - 46.0, 4.0, team, 0.4);
    }
    // Crusher claws: huge scythe polys toward facing.
    for side in [-1.0f32, 1.0] {
        let px = cx + dx * 24.0 - dy * side * 20.0;
        let py = cy + dy * 18.0 + dx * side * 12.0;
        c.poly(&[
            (px - 4.0, py - 6.0),
            (px + dx * 24.0 + 2.0, py + dy * 14.0 - 4.0),
            (px + dx * 28.0, py + dy * 17.0 + 4.0),
            (px + dx * 12.0, py + dy * 9.0 + 8.0),
        ], rgba(CHITIN_LIGHT));
        c.line(px + dx * 6.0, py + dy * 3.0, px + dx * 26.0, py + dy * 15.0, 2.0, rgba(team));
        // Serration teeth on the inner edge.
        for t in 0..3 {
            let f2 = 0.35 + t as f32 * 0.22;
            c.poly(&[
                (px + dx * 28.0 * f2 - 2.0, py + dy * 17.0 * f2 + 6.0),
                (px + dx * 28.0 * f2 + 2.0, py + dy * 17.0 * f2 + 6.0),
                (px + dx * 28.0 * f2, py + dy * 17.0 * f2 + 11.0),
            ], rgba(CHITIN_LIGHT));
        }
    }
    // Glowing eye cluster + underglow.
    if dy > -0.4 {
        let ex = cx + dx * 16.0;
        let ey = cy - 4.0 + dy * 8.0;
        for (ox, oy) in [(-3.0f32, -1.0f32), (0.0, -3.0), (3.0, -1.0), (0.0, 1.0)] {
            c.set((ex + ox) as i32, (ey + oy) as i32, rgba(KYTH_GLOW));
        }
        c.glow(ex, ey, 8.0, KYTH_GLOW, 0.7);
    }
    c.glow(cx, cy + 12.0, 14.0, KYTH_GLOW, 0.25);
    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.3);
    c
}

/// Wisp: floating jelly flyer. 104x104.
fn paint_wisp(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(104, 104);
    let (dx, dy) = facing_vec(f);
    let cx = 52.0;
    let cy = 42.0;
    let pulse = if frame == 0 { 0.0 } else { 3.0 };
    // Bell: membrane with a translucent skirt rim.
    c.dome(cx, cy, 24.0 + pulse, 19.0 - pulse * 0.5, MEMBRANE);
    c.ellipse(cx, cy + 12.0 - pulse * 0.5, 26.0 + pulse, 7.0, [142, 88, 70, 120]);
    // Chitin cap plates on the crown.
    c.dome(cx - 6.0, cy - 12.0, 12.0, 7.0, CHITIN_LIGHT);
    scallop(&mut c, cx - 6.0, cy - 10.0, 10.0, 12.0);
    // Inner glow core showing through the membrane.
    c.glow(cx, cy + 2.0, 16.0, KYTH_GLOW, 0.5);
    c.ellipse(cx - dx * 2.0, cy + 2.0, 5.0, 5.5, rgba(scale_rgb(KYTH_GLOW, 1.1)));
    // Team organ sac toward facing.
    c.dome(cx + dx * 10.0, cy + dy * 6.0 + 2.0, 6.0, 5.0, team);
    c.glow(cx + dx * 10.0, cy + dy * 6.0 + 2.0, 8.0, team, 0.5);
    // Trailing tentacles: two-segment sway.
    for k in 0..5 {
        let off = -18.0 + k as f32 * 9.0;
        let sway = if (k + frame) % 2 == 0 { 5.0 } else { -4.0 };
        let x0 = cx + off;
        let y0 = cy + 15.0;
        let x1 = x0 - dx * 8.0 + sway * 0.5;
        let y1 = y0 + 16.0 - dy * 4.0;
        c.line(x0, y0, x1, y1, 2.4, rgba(scale_rgb(MEMBRANE, 0.8)));
        c.line(x1, y1, x1 + sway, y1 + 18.0, 1.8, rgba(scale_rgb(MEMBRANE, 0.62)));
        c.set((x1 + sway) as i32, (y1 + 18.0) as i32, rgba(KYTH_GLOW));
    }
    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.25);
    c
}

/// Weaver: psionic rift node. 104x120.
fn paint_weaver(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(104, 120);
    let (dx, dy) = facing_vec(f);
    let cx = 52.0;
    let cy = 66.0;
    // Root tentacles gripping the ground.
    for k in 0..5 {
        let off = -20.0 + k as f32 * 10.0;
        c.line(cx + off, cy + 16.0, cx + off * 1.5, cy + 34.0, 3.0, LEG);
        c.line(cx + off * 1.5, cy + 34.0, cx + off * 1.7, cy + 42.0, 2.0, LEG);
    }
    // Bulb body: chitin with a lighter crown, scalloped.
    c.dome(cx, cy, 22.0, 24.0, CHITIN);
    scallop(&mut c, cx, cy + 6.0, 19.0, 26.0);
    c.dome(cx, cy - 22.0, 16.0, 13.0, CHITIN_LIGHT);
    scallop(&mut c, cx, cy - 20.0, 13.0, 16.0);
    // Team band.
    c.line(cx - 14.0, cy + 8.0, cx + 14.0, cy + 11.0, 3.5, rgba(team));
    // Psionic core: bright slit in the crown.
    c.glow(cx, cy - 24.0, 14.0, KYTH_GLOW, 0.6);
    c.rect((cx - 2.0) as i32, (cy - 32.0) as i32, 4, 14, rgba(scale_rgb(KYTH_GLOW, 1.15)));
    // Orbiting rift shards toward facing.
    let a0 = if frame == 0 { 0.0f32 } else { 0.8 };
    for k in 0..4 {
        let a = a0 + k as f32 * 1.6;
        let px = cx + dx * 22.0 + a.cos() * 13.0;
        let py = cy - 30.0 + dy * 10.0 + a.sin() * 7.0;
        c.poly(&[(px - 1.5, py + 4.0), (px, py - 5.0), (px + 1.5, py + 4.0)], rgba(KYTH_GLOW));
        c.glow(px, py, 5.0, KYTH_GLOW, 0.5);
    }
    if dy > -0.4 {
        c.set((cx + dx * 6.0 - 2.0) as i32, (cy - 22.0) as i32, rgba([220, 255, 180]));
        c.set((cx + dx * 6.0 + 2.0) as i32, (cy - 22.0) as i32, rgba([220, 255, 180]));
    }
    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.3);
    c
}

/// Organic mound base shared by Kyth structures.
fn kyth_mound(c: &mut Canvas, cx: f32, cy: f32, rx: f32, ry: f32, team: [u8; 3]) {
    c.ellipse_shaded(cx, cy, rx, ry, MEMBRANE);
    c.ellipse_shaded(cx, cy - ry * 0.25, rx * 0.8, ry * 0.7, CHITIN);
    // Creep skirt.
    c.ellipse(cx, cy + ry * 0.45, rx * 1.15, ry * 0.5, [92, 70, 96, 160]);
    // Team veins.
    for k in 0..3 {
        let a = k as f32 * 2.1 + 0.6;
        c.line(
            cx,
            cy + ry * 0.3,
            cx + a.cos() * rx * 0.9,
            cy + ry * 0.35 + a.sin().abs() * ry * 0.5,
            1.3,
            rgba(team),
        );
    }
}

/// Hive: the swarm HQ — big mound with chimney spires. 100x78.
fn paint_hive(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(100, 78);
    kyth_mound(&mut c, 50.0, 52.0, 44.0, 22.0, team);
    // Spires.
    for (sx, sh) in [(30.0f32, 22.0f32), (52.0, 30.0), (70.0, 18.0)] {
        c.line(sx, 44.0, sx - 2.0, 44.0 - sh, 4.5, rgba(CHITIN_LIGHT));
        c.ellipse(sx - 2.5, 42.0 - sh, 2.0, 2.4, rgba(KYTH_GLOW));
    }
    // Mouth.
    c.ellipse(50.0, 60.0, 10.0, 5.0, rgba([44, 34, 48]));
    c.ellipse(50.0, 59.0, 7.0, 3.0, rgba(KYTH_GLOW));
    c.outline(OUTLINE);
    c
}

/// Spire: supply crystal spike. 64x60.
fn paint_spire(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(64, 60);
    kyth_mound(&mut c, 32.0, 46.0, 26.0, 12.0, team);
    // Twisting spike.
    c.line(32.0, 42.0, 28.0, 20.0, 6.0, rgba(CHITIN));
    c.line(28.0, 20.0, 33.0, 6.0, 4.0, rgba(CHITIN_LIGHT));
    c.ellipse(33.0, 5.0, 2.6, 3.2, rgba(team));
    c.ellipse(30.0, 24.0, 1.8, 1.8, rgba(KYTH_GLOW));
    c.outline(OUTLINE);
    c
}

/// Sap Well: membrane pool over a geyser. 68x56.
fn paint_sapwell(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(68, 56);
    kyth_mound(&mut c, 34.0, 40.0, 30.0, 14.0, team);
    // Sap pool.
    c.ellipse(34.0, 36.0, 14.0, 6.5, rgba([54, 42, 58]));
    c.ellipse(34.0, 36.0, 10.0, 4.5, rgba([120, 220, 120]));
    c.ellipse(31.0, 34.5, 3.0, 1.5, rgba([200, 255, 180]));
    // Siphon tube.
    c.line(46.0, 32.0, 54.0, 20.0, 3.5, rgba(CHITIN_LIGHT));
    c.ellipse(54.5, 18.5, 2.4, 2.6, rgba(KYTH_GLOW));
    c.outline(OUTLINE);
    c
}

/// Warren: infantry mound with a birthing maw. 96x66.
fn paint_warren(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(96, 66);
    kyth_mound(&mut c, 48.0, 44.0, 42.0, 20.0, team);
    // Maw.
    c.ellipse(48.0, 52.0, 13.0, 6.0, rgba([44, 34, 48]));
    for k in 0..5 {
        let x = 38.0 + k as f32 * 5.0;
        c.line(x, 48.0, x + 1.0, 52.0, 1.3, rgba(CHITIN_LIGHT)); // teeth
    }
    // Back plates.
    c.ellipse_shaded(34.0, 30.0, 9.0, 5.0, CHITIN_LIGHT);
    c.ellipse_shaded(60.0, 28.0, 11.0, 6.0, CHITIN_LIGHT);
    c.outline(OUTLINE);
    c
}

/// Incubator: egg cluster. 96x70.
fn paint_incubator(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(96, 70);
    kyth_mound(&mut c, 48.0, 50.0, 42.0, 19.0, team);
    // Eggs.
    for (ex, ey, r) in [(32.0f32, 36.0f32, 8.0f32), (52.0, 30.0, 10.0), (68.0, 38.0, 7.0)] {
        c.ellipse_shaded(ex, ey, r, r * 1.15, MEMBRANE);
        c.ellipse(ex - r * 0.3, ey - r * 0.4, r * 0.35, r * 0.4, rgba(scale_rgb(MEMBRANE, 1.3)));
        c.ellipse(ex, ey + r * 0.3, r * 0.4, r * 0.3, rgba(KYTH_GLOW));
    }
    c.outline(OUTLINE);
    c
}

/// Roost: flyer perch tower. 84x74.
fn paint_roost(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(84, 74);
    kyth_mound(&mut c, 42.0, 56.0, 36.0, 16.0, team);
    // Tall perch stalk with hanging pods.
    c.line(42.0, 52.0, 38.0, 14.0, 7.0, rgba(CHITIN));
    c.line(38.0, 16.0, 24.0, 24.0, 3.0, rgba(CHITIN_LIGHT));
    c.line(38.0, 14.0, 56.0, 20.0, 3.0, rgba(CHITIN_LIGHT));
    for (px, py) in [(24.0f32, 28.0f32), (56.0, 24.0)] {
        c.ellipse_shaded(px, py + 3.0, 4.0, 5.0, MEMBRANE);
        c.ellipse(px, py + 5.0, 1.8, 2.0, rgba(KYTH_GLOW));
    }
    c.ellipse(37.0, 12.0, 3.0, 2.4, rgba(team));
    c.outline(OUTLINE);
    c
}

/// Cortex: exposed brain dome. 66x56.
fn paint_cortex(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(66, 56);
    kyth_mound(&mut c, 33.0, 42.0, 28.0, 13.0, team);
    // Brain.
    c.ellipse_shaded(33.0, 26.0, 16.0, 12.0, [186, 142, 160]);
    for k in 0..4 {
        let y = 18.0 + k as f32 * 5.0;
        c.line(20.0 + k as f32 * 2.0, y, 46.0 - k as f32 * 2.0, y + 2.0, 1.2, rgba([150, 108, 128]));
    }
    // Psionic sparks.
    c.ellipse(33.0, 14.0, 2.0, 2.0, rgba(KYTH_GLOW));
    c.line(28.0, 18.0, 26.0, 12.0, 1.1, rgba(KYTH_GLOW));
    c.line(40.0, 17.0, 43.0, 11.0, 1.1, rgba(KYTH_GLOW));
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
    let mut c = Canvas::new(256, 176);
    // Slag mound: overlapping dark domes give a lumpy silhouette.
    c.dome(128.0, 120.0, 110.0, 44.0, [66, 58, 50]);
    c.dome(70.0, 128.0, 44.0, 22.0, [60, 53, 46]);
    c.dome(190.0, 126.0, 46.0, 22.0, [72, 63, 54]);
    c.dome(128.0, 102.0, 74.0, 30.0, [78, 68, 58]);
    // Crater bowl.
    c.ellipse(128.0, 96.0, 46.0, 20.0, rgba([40, 37, 34]));
    c.ellipse(128.0, 96.0, 40.0, 17.0, rgba([30, 28, 26]));
    // Cracked veins radiating from the vent, glowing toward the source.
    for k in 0..5 {
        let h = hash2(k, 9, 501);
        let a = 0.5 + k as f32 * 1.25 + (h % 40) as f32 * 0.01;
        let (vx, vy) = (a.cos(), a.sin() * 0.45);
        let r0 = 40.0;
        let r1 = 78.0 + (h % 30) as f32;
        c.line(128.0 + vx * r0, 100.0 + vy * r0, 128.0 + vx * r1, 100.0 + vy * r1, 2.0, rgba([30, 110, 100]));
        let rm = r0 + (r1 - r0) * 0.55;
        c.line(128.0 + vx * r0, 100.0 + vy * r0, 128.0 + vx * rm, 100.0 + vy * rm, 1.0, rgba([80, 215, 195]));
    }
    // Plasma pool: layered glow with a white-hot heart.
    c.glow(128.0, 96.0, 60.0, [40, 200, 185], 0.55);
    c.ellipse(128.0, 96.0, 34.0, 14.0, rgba([46, 205, 190]));
    c.ellipse(128.0, 95.0, 22.0, 9.0, rgba([150, 250, 235]));
    c.ellipse(126.0, 94.0, 10.0, 4.0, rgba([230, 255, 250]));
    // Rising vapor wisps.
    for k in 0..4 {
        let h = hash2(k, 3, 77);
        let x = 104.0 + (h % 48) as f32;
        let y = 62.0 - k as f32 * 14.0;
        c.glow(x, y, 10.0 - k as f32 * 1.5, [110, 235, 220], 0.35);
    }
    // Scattered slag rocks.
    c.dome(36.0, 146.0, 15.0, 8.0, [58, 52, 46]);
    c.dome(218.0, 140.0, 13.0, 7.0, [58, 52, 46]);
    c.outline_t([18, 24, 24, 255], 2);
    c.rim([18, 24, 24, 255], 1.2);
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
