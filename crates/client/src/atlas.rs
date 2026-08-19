//! Procedural sprite atlas — the entire art of the game, painted at startup.
//!
//! Style target: SC1-era isometric pixel art. Dark outlines, 3-4 tone
//! material ramps, dithered texture, light from the upper-left, hard team
//! color accents. Every sprite is painted per team so no runtime palette
//! tricks are needed.

use crate::font;

pub const ATLAS: u32 = 6144;

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
    pub trees: [Region; 4],
    /// Disturbed-earth mound marking a friendly burrowed unit.
    pub burrow_mound: Region,
    /// Soft radial gradient for the additive glow pass.
    pub glow_soft: Region,
    /// Translucent iso hemisphere shell for shield fields (white; tinted
    /// and alpha-scaled at draw time).
    pub shield_dome: Region,
    pub rock_wall: Region,
    /// [unit_type][team][facing][frame]. 0-6: Vanguard Combine (worker,
    /// trooper, vanguard, breaker, skywing, stormcaller, breaker-sieged).
    /// 7-12: Kyth Assembly (drone, skitter, spitter, ravager, wisp, weaver).
    pub units: Vec<Region>,
    /// [building_type][team]. 0-6 Vanguard, 7-13 Kyth (hive, spire, sapwell,
    /// warren, incubator, roost, cortex).
    pub buildings: Vec<Region>,
    /// Bust portraits [unit_type][team] for the console portrait panel.
    pub portraits: Vec<Region>,
    /// Building sprite heights in SCREEN units at zoom 1 (canvas px / scale).
    pub building_px_h: [f32; 21],
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

    pub fn portrait(&self, unit_type: usize, team: usize) -> Region {
        self.portraits[unit_type * 2 + team]
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

    let mut c = Canvas::new(128, 128);
    for y in 0..128 {
        for x in 0..128 {
            let d = ((x as f32 + 0.5 - 64.0).powi(2) + (y as f32 + 0.5 - 64.0).powi(2)).sqrt();
            let a = ((62.0 - d).clamp(0.0, 2.0) * 127.5) as u8;
            if a > 0 {
                c.set(x, y, [255, 255, 255, a]);
            }
        }
    }
    let circle = p.place(&c);

    let mut c = Canvas::new(128, 128);
    for y in 0..128 {
        for x in 0..128 {
            let d = ((x as f32 + 0.5 - 64.0).powi(2) + (y as f32 + 0.5 - 64.0).powi(2)).sqrt();
            let a = ((62.0 - d).clamp(0.0, 1.5) * (d - 50.0).clamp(0.0, 1.5) * 113.0) as u8;
            if a > 0 {
                c.set(x, y, [255, 255, 255, a]);
            }
        }
    }
    let ring = p.place(&c);

    let diamond = p.place(&diamond_canvas(256, 128, [255, 255, 255, 255], false));
    let diamond_outline = p.place(&diamond_canvas(256, 128, [255, 255, 255, 255], true));

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
    for unit_type in 0..26 {
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
                        12 => paint_weaver(facing, frame, TEAMS[team]),
                        13 => paint_bulwark(facing, frame, TEAMS[team]),
                        14 => paint_bulwark_deployed(facing, frame, TEAMS[team]),
                        15 => paint_burrower(facing, frame, TEAMS[team]),
                        16 => paint_scrapper(facing, frame, TEAMS[team]),
                        17 => paint_arclight(facing, frame, TEAMS[team]),
                        18 => paint_mauler(facing, frame, TEAMS[team]),
                        19 => paint_lodestone(facing, frame, TEAMS[team]),
                        20 => paint_kestrel(facing, frame, TEAMS[team]),
                        21 => paint_resonant(facing, frame, TEAMS[team]),
                        22 => paint_marshal(facing, frame, TEAMS[team]),
                        23 => paint_broodmother(facing, frame, TEAMS[team]),
                        24 => paint_magnus(facing, frame, TEAMS[team]),
                        _ => paint_broodling(facing, frame, TEAMS[team]),
                    };
                    units.push(p.place_s(&c, SS));
                }
            }
        }
    }

    // Bust portraits: [unit_type][team].
    let mut portraits = Vec::new();
    for unit_type in 0..26 {
        for team in 0..2 {
            portraits.push(p.place_s(&paint_portrait(unit_type, TEAMS[team]), 1.6));
        }
    }

    // Buildings: [type][team].
    let mut buildings = Vec::new();
    let mut building_px_h = [0f32; 21];
    for b_type in 0..21 {
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
                13 => paint_cortex(TEAMS[team]),
                14 => paint_bastion(TEAMS[team]),
                15 => paint_mast(TEAMS[team]),
                16 => paint_fumetap(TEAMS[team]),
                17 => paint_assembly(TEAMS[team]),
                18 => paint_refit(TEAMS[team]),
                19 => paint_skydock(TEAMS[team]),
                _ => paint_relay(TEAMS[team]),
            };
            building_px_h[b_type] = c.h as f32 / SS;
            buildings.push(p.place_s(&c, SS));
        }
    }

    // Destructible flora + rocks.
    let trees = std::array::from_fn(|i| p.place_s(&tree_canvas(i as i32), SS));
    let glow_soft = {
        let mut c = Canvas::new(128, 128);
        for y in 0..128 {
            for x in 0..128 {
                let dx = (x as f32 + 0.5 - 64.0) / 64.0;
                let dy = (y as f32 + 0.5 - 64.0) / 64.0;
                let d = (dx * dx + dy * dy).sqrt();
                if d < 1.0 {
                    let a = ((1.0 - d).powi(2) * 255.0) as u8;
                    if a > 2 {
                        c.set(x, y, [255, 255, 255, a]);
                    }
                }
            }
        }
        p.place(&c)
    };
    let shield_dome = {
        let (w, h) = (512, 352);
        let mut c = Canvas::new(w, h);
        let (cx, base, dome_h, skirt) = (256.0, 200.0, 184.0, 120.0);
        let rx = 250.0;
        for y in 0..h {
            for x in 0..w {
                let nx = (x as f32 + 0.5 - cx) / rx;
                let fy = y as f32 + 0.5;
                let (r, upper) = if fy <= base {
                    (((nx * nx) + ((base - fy) / dome_h).powi(2)).sqrt(), true)
                } else {
                    (((nx * nx) + ((fy - base) / skirt).powi(2)).sqrt(), false)
                };
                if r > 1.0 {
                    continue;
                }
                // Fresnel shell: bright silhouette, near-clear center.
                let fresnel = r.powi(3);
                let mut a = if upper { 16.0 + 120.0 * fresnel } else { 8.0 + 70.0 * fresnel };
                // Hard rim line.
                if r > 0.972 {
                    a = if upper { 215.0 } else { 150.0 };
                }
                // Faint latitude bands sell the curvature.
                if upper {
                    let band = ((base - fy) / dome_h * 5.0).fract();
                    if band < 0.12 {
                        a += 14.0;
                    }
                    // Specular blob upper-left.
                    let sd = ((nx + 0.42).powi(2) + (((base - fy) / dome_h) - 0.6).powi(2)).sqrt();
                    if sd < 0.26 {
                        a += (1.0 - sd / 0.26) * 70.0;
                    }
                }
                c.set(x, y, [255, 255, 255, a.min(235.0) as u8]);
            }
        }
        // Ground-contact ring: bright base ellipse edge.
        for x in 0..w {
            let nx = (x as f32 + 0.5 - cx) / rx;
            if nx.abs() > 1.0 {
                continue;
            }
            let dy = skirt * (1.0 - nx * nx).sqrt();
            for sgn in [-1.0f32, 1.0] {
                let yy = (base + dy * sgn) as i32;
                c.blend(x, yy, [255, 255, 255, 90]);
                c.blend(x, yy + 1, [255, 255, 255, 45]);
            }
        }
        p.place(&c)
    };
    let burrow_mound = {
        let mut c = Canvas::new(72, 40);
        c.ellipse(36.0, 26.0, 30.0, 10.0, [30, 24, 20, 150]);
        c.dome(36.0, 22.0, 24.0, 10.0, [58, 46, 36]);
        c.dome(28.0, 20.0, 9.0, 5.0, [66, 52, 40]);
        c.dome(46.0, 21.0, 8.0, 4.5, [52, 41, 32]);
        // Cracked earth lines radiating.
        for k in 0..4 {
            let a = 0.5 + k as f32 * 1.6;
            c.line(36.0, 22.0, 36.0 + a.cos() * 22.0, 24.0 + a.sin().abs() * 8.0, 1.4, rgba([34, 27, 22]));
        }
        c.outline_t([20, 16, 13, 255], 2);
        p.place_s(&c, SS)
    };
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

    // Effects (painted at SS, drawn at explicit screen sizes).
    let flash = p.place_s(&star_flash(), SS);
    let spark = {
        let mut c = Canvas::new(28, 28);
        c.glow(14.0, 14.0, 13.0, [255, 240, 160], 0.9);
        c.ellipse(14.0, 14.0, 6.0, 6.0, rgba([255, 240, 160]));
        c.ellipse(14.0, 14.0, 3.0, 3.0, rgba([255, 255, 255]));
        p.place_s(&c, SS)
    };
    let blast_ring = {
        let mut c = Canvas::new(96, 96);
        for y in 0..96 {
            for x in 0..96 {
                let dx = x as f32 + 0.5 - 48.0;
                let dy = (y as f32 + 0.5 - 48.0) * 2.0;
                let d = (dx * dx + dy * dy).sqrt();
                let a = ((46.0 - d).clamp(0.0, 2.0) * (d - 36.0).clamp(0.0, 2.0) * 63.0) as u8;
                if a > 0 {
                    c.set(x, y, [255, 200, 120, a]);
                }
            }
        }
        p.place_s(&c, SS)
    };
    let corpse = {
        let mut c = Canvas::new(80, 40);
        // Scorched stain with debris chunks.
        c.ellipse(40.0, 22.0, 36.0, 14.0, [26, 22, 22, 180]);
        c.ellipse(38.0, 21.0, 26.0, 10.0, [38, 32, 30, 220]);
        for k in 0..6 {
            let h = hash2(k, 3, 517);
            let px = 16.0 + (h % 48) as f32;
            let py = 14.0 + ((h >> 8) % 14) as f32;
            c.dome(px, py, 3.0 + (h % 3) as f32, 2.0, [52, 46, 44]);
        }
        c.line(30.0, 18.0, 46.0, 24.0, 1.6, rgba([70, 64, 58]));
        p.place_s(&c, SS)
    };
    let rubble = {
        let mut c = Canvas::new(192, 112);
        // Collapsed plates and girder stubs in a dust bed.
        c.ellipse(96.0, 60.0, 84.0, 34.0, [30, 27, 26, 160]);
        for k in 0..10 {
            let h = hash2(k, 7, 99);
            let x = 30.0 + (h % 130) as f32;
            let y = 32.0 + ((h >> 8) % 52) as f32;
            let r = 7.0 + ((h >> 16) % 10) as f32;
            c.poly(&[
                (x - r, y + r * 0.3),
                (x - r * 0.3, y - r * 0.5),
                (x + r * 0.7, y - r * 0.35),
                (x + r, y + r * 0.4),
                (x, y + r * 0.55),
            ], rgba([64, 60, 56]));
            c.line(x - r * 0.3, y - r * 0.5, x + r * 0.7, y - r * 0.35, 1.4, rgba([92, 86, 80]));
        }
        for k in 0..3 {
            let h = hash2(k, 11, 313);
            let x = 46.0 + (h % 100) as f32;
            let y = 40.0 + ((h >> 8) % 36) as f32;
            c.line(x, y, x + 10.0, y - 14.0, 2.5, rgba([46, 44, 48]));
        }
        c.outline_t([20, 18, 17, 255], 2);
        p.place_s(&c, SS)
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

    // Font: 5x7 source grids baked at 3x with a bolder dilated stroke,
    // chamfered corners, a top-lit bevel and a dark outline. Tinting
    // multiplies, so the bevel and outline survive any text color.
    let mut glyphs = Vec::new();
    const FS: i32 = 3;
    for ch in " ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789:/-.%+!(),".chars() {
        if let Some(rows) = font::glyph(ch) {
            let (gw, gh) = (font::GLYPH_W as i32 * FS + 2, font::GLYPH_H as i32 * FS + 2);
            let mut fill = vec![false; (gw * gh) as usize];
            for (ry, row) in rows.iter().enumerate() {
                for (rx, bit) in row.chars().enumerate() {
                    if bit != '1' {
                        continue;
                    }
                    for oy in 0..FS {
                        for ox in 0..=FS {
                            // <=FS dilates one px rightward: bolder stroke.
                            let x = 1 + rx as i32 * FS + ox;
                            let y = 1 + ry as i32 * FS + oy;
                            if x < gw - 1 && y < gh - 1 {
                                fill[(y * gw + x) as usize] = true;
                            }
                        }
                    }
                }
            }
            // Chamfer convex corners for a softer letterform.
            let at = |f: &Vec<bool>, x: i32, y: i32| -> bool {
                x >= 0 && y >= 0 && x < gw && y < gh && f[(y * gw + x) as usize]
            };
            let snap = fill.clone();
            for y in 0..gh {
                for x in 0..gw {
                    if !at(&snap, x, y) {
                        continue;
                    }
                    let (u, d) = (at(&snap, x, y - 1), at(&snap, x, y + 1));
                    let (l, r) = (at(&snap, x - 1, y), at(&snap, x + 1, y));
                    if (!u && !l) || (!u && !r) || (!d && !l) || (!d && !r) {
                        fill[(y * gw + x) as usize] = false;
                    }
                }
            }
            // Bake: bevel-lit strokes + outline.
            let mut c = Canvas::new(gw, gh);
            for y in 0..gh {
                for x in 0..gw {
                    if at(&fill, x, y) {
                        let v = if !at(&fill, x, y - 1) {
                            255
                        } else if !at(&fill, x, y + 1) {
                            150
                        } else {
                            215
                        };
                        c.set(x, y, [v, v, v, 255]);
                    } else {
                        let edge = at(&fill, x - 1, y)
                            || at(&fill, x + 1, y)
                            || at(&fill, x, y - 1)
                            || at(&fill, x, y + 1);
                        if edge {
                            c.set(x, y, [24, 26, 32, 235]);
                        }
                    }
                }
            }
            glyphs.push((ch, p.place_s(&c, FS as f32)));
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
        burrow_mound,
        glow_soft,
        shield_dome,
        rock_wall,
        units,
        buildings,
        portraits,
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
    let mut c = Canvas::new(96, 176);
    let tall = v >= 2;
    let cx = 48.0 + if v % 2 == 0 { -1.0 } else { 2.0 };
    let base = 162.0;
    // Root shadow pool + flare so the tree sits IN the ground.
    c.ellipse(cx, base + 4.0, 22.0, 7.0, [8, 12, 10, 110]);
    c.poly(&[(cx - 11.0, base + 6.0), (cx + 11.0, base + 6.0), (cx + 6.0, base - 4.0), (cx - 6.0, base - 4.0)], rgba([42, 32, 24]));
    let crown = if tall { 18.0 } else { 62.0 };
    // Trunk with a lit bark edge.
    c.poly(&[(cx - 4.5, base), (cx + 4.5, base), (cx + 2.5, crown + 30.0), (cx - 2.5, crown + 30.0)], rgba([50, 38, 28]));
    c.line(cx - 2.0, base - 4.0, cx - 3.0, crown + 34.0, 1.8, rgba([66, 50, 38]));
    // Serrated tiers, dark at the base brightening toward the crown.
    let n = if tall { 5 } else { 4 };
    for ti in 0..n {
        let f = ti as f32 / (n - 1) as f32;
        let by = base - 16.0 - f * (base - 16.0 - (crown + 26.0));
        let hw = 30.0 - f * 17.0;
        let ah = 26.0 - f * 6.0;
        let col = [
            (28.0 + f * 20.0) as u8,
            (56.0 + f * 36.0) as u8,
            (44.0 + f * 22.0) as u8,
        ];
        let skew = ((hash2(ti, v * 13, 907) % 9) as f32 - 4.0) * 0.9;
        let apex_x = cx + skew;
        let mut pts: Vec<(f32, f32)> = vec![(apex_x, by - ah), (cx + hw, by)];
        let teeth = 6;
        for k in 1..teeth {
            let tf = k as f32 / teeth as f32;
            let x = cx + hw - tf * hw * 2.0;
            let droop = if k % 2 == 1 { 7.0 } else { 1.0 };
            let jitter = (hash2(k, ti * 9 + v, 551) % 4) as f32;
            pts.push((x, by + droop + jitter));
        }
        pts.push((cx - hw, by));
        c.poly(&pts, rgba(col));
        c.line(apex_x, by - ah, cx - hw + 2.0, by - 1.0, 1.6, rgba(scale_rgb(col, 1.35)));
        c.line(cx - hw * 0.5, by + 3.0, cx + hw * 0.6, by + 2.0, 2.0, rgba(scale_rgb(col, 0.6)));
    }
    // Needle spire.
    c.line(cx, crown + 8.0, cx + skew_tip(v), crown - 6.0, 2.2, rgba([54, 100, 70]));
    // Bioluminescent specks, denser on the tall variants.
    let specks = if tall { 9 } else { 6 };
    for k in 0..specks {
        let h = hash2(k, v, 421);
        let x = cx - 26.0 + (h % 52) as f32;
        let y = crown + 14.0 + ((h >> 8) % ((base - crown - 30.0) as u32)) as f32;
        c.glow(x, y, 3.0, [110, 235, 185], 0.7);
        c.set(x as i32, y as i32, [180, 250, 215, 255]);
    }
    c.outline_t([10, 18, 14, 255], 2);
    c.rim([10, 18, 14, 255], 1.3);
    c
}

fn skew_tip(v: i32) -> f32 {
    if v % 2 == 0 {
        -1.5
    } else {
        2.0
    }
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
            let inside = if outline_only { d <= 1.0 && d >= 0.93 } else { d <= 1.0 };
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

// AoE2-inspired warm palette on an alien world: low ground is warm ochre
// regolith, high ground is sun-bleached sandstone. Kept earthy (not neon)
// so the sci-fi blue crystals/pylons still pop as the cool accent.
const GROUND_LOW: [[u8; 3]; 3] = [[123, 96, 62], [106, 81, 52], [140, 112, 76]];
const GROUND_HIGH: [[u8; 3]; 3] = [[164, 138, 100], [146, 121, 86], [182, 158, 118]];

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
    let base = [132, 106, 72];
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
            // Warm ironstone rubble instead of cold basalt.
            let t = match h % 16 {
                0..=7 => [86, 62, 50],
                8..=12 => [70, 50, 41],
                _ => [102, 76, 60],
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
                rgba([112, 84, 66]),
            );
            c.line(px - r * 0.4, py - r * 0.55, px + r * 0.6, py - r * 0.4, 1.2, rgba([146, 116, 92]));
            c.line(px - r * 0.4, py - r * 0.5, px - r * 0.2, py + r * 0.5, 1.0, rgba([64, 46, 38]));
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
            // Warm ironstone cliff strata (sunlit right face brighter).
            let base: [u8; 3] = if left { [96, 72, 52] } else { [124, 96, 66] };
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

/// Bulwark: shield projector rig, mobile. 112x104.
fn paint_bulwark(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(112, 104);
    let (dx, dy) = facing_vec(f);
    let cx = 56.0;
    let cy = 58.0;
    // Tracked base.
    plate(&mut c, &[(26.0, 74.0), (86.0, 74.0), (90.0, 88.0), (22.0, 88.0)], [38, 41, 48], 1.0);
    for k in 0..6 {
        let notch = 28 + k * 10 + (frame as i32) * 5;
        if notch < 84 {
            c.rect(notch, 78, 2, 8, rgba([70, 76, 88]));
        }
    }
    // Hull with a sloped front toward facing.
    plate(&mut c, &[(30.0, 52.0), (82.0, 52.0), (86.0, 74.0), (26.0, 74.0)], GUNMETAL, 1.0);
    c.poly(&[(38.0 + dx * 6.0, 54.0), (56.0 + dx * 10.0, 50.0 + dy * 3.0), (56.0 + dx * 10.0, 68.0), (42.0 + dx * 6.0, 72.0)], rgba(scale_rgb(GUNMETAL, 1.15)));
    c.rect(32, 68, 48, 4, rgba(team));
    // Folded emitter petals on the roof: three angular vanes.
    for (px, lean) in [(40.0f32, -6.0f32), (56.0, 0.0), (72.0, 6.0)] {
        c.poly(&[(px - 4.0, 52.0), (px + 4.0, 52.0), (px + lean + 2.0, 26.0), (px + lean - 2.0, 26.0)], rgba(STEEL_LIT));
        c.set((px + lean) as i32, 26, rgba(scale_rgb(team, 1.3)));
        c.glow(px + lean, 25.0, 4.0, team, 0.5);
    }
    // Core emitter eye.
    c.glow(cx + dx * 6.0, cy - 2.0 + dy * 4.0, 9.0, team, 0.8);
    c.ellipse(cx + dx * 6.0, cy - 2.0 + dy * 4.0, 3.5, 3.0, rgba(scale_rgb(team, 1.35)));
    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.28);
    c
}

/// Bulwark deployed: petals open into a crown, field humming. 128x116.
fn paint_bulwark_deployed(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(128, 116);
    let (dx, dy) = facing_vec(f);
    let _ = (dx, dy);
    let cx = 64.0;
    // Anchor feet splayed.
    for (lx, ly) in [(-34.0f32, 22.0f32), (34.0, 22.0), (-22.0, 32.0), (22.0, 32.0)] {
        c.line(cx, 70.0, cx + lx, 70.0 + ly, 6.0, rgba(GUNMETAL_DARK));
        plate(&mut c, &[(cx + lx - 7.0, 68.0 + ly), (cx + lx + 7.0, 68.0 + ly), (cx + lx + 9.0, 74.0 + ly), (cx + lx - 9.0, 74.0 + ly)], [34, 37, 44], 1.0);
    }
    // Lowered hull.
    plate(&mut c, &[(34.0, 58.0), (94.0, 58.0), (98.0, 78.0), (30.0, 78.0)], GUNMETAL, 1.0);
    c.rect(38, 72, 52, 4, rgba(team));
    // Open emitter crown: five vanes fanned out, tips alight.
    for k in 0..5 {
        let a = -1.35 + k as f32 * 0.675;
        let px = cx + a.sin() * 34.0;
        let py = 52.0 - a.cos() * 26.0;
        c.line(cx, 56.0, px, py, 4.5, rgba(STEEL_LIT));
        c.poly(&[(px - 3.0, py + 2.0), (px + 3.0, py + 2.0), (px, py - 6.0)], rgba(scale_rgb(team, 1.2)));
        c.glow(px, py - 2.0, 6.0, team, 0.7);
    }
    // Humming field arcs between vane tips (pulse with frame).
    let pulse = if frame == 0 { 0.35 } else { 0.6 };
    for k in 0..4 {
        let a0 = -1.35 + k as f32 * 0.675;
        let a1 = a0 + 0.675;
        let (x0, y0) = (cx + a0.sin() * 34.0, 50.0 - a0.cos() * 26.0);
        let (x1, y1) = (cx + a1.sin() * 34.0, 50.0 - a1.cos() * 26.0);
        c.line(x0, y0 - 3.0, (x0 + x1) * 0.5, (y0 + y1) * 0.5 - 8.0, 1.4, [team[0], team[1], team[2], (pulse * 255.0) as u8]);
        c.line((x0 + x1) * 0.5, (y0 + y1) * 0.5 - 8.0, x1, y1 - 3.0, 1.4, [team[0], team[1], team[2], (pulse * 200.0) as u8]);
    }
    // Core blazing.
    c.glow(cx, 62.0, 13.0, team, 0.9);
    c.ellipse(cx, 62.0, 4.5, 3.5, rgba([250, 252, 255]));
    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.25);
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

/// Burrower: low ambush predator with digging scythes. 104x88.
fn paint_burrower(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(104, 88);
    let (dx, dy) = facing_vec(f);
    let cx = 52.0;
    let cy = 46.0;
    let step = if frame == 0 { 4.0 } else { -4.0 };
    // Six digging legs, low and wide.
    for k in 0..3 {
        let off = -12.0 + k as f32 * 12.0;
        let l = if k % 2 == 0 { step } else { -step };
        kleg(&mut c, cx + off, cy + 4.0, cx + off - 12.0, cy + 12.0 - l * 0.4, cx + off - 15.0, cy + 24.0 + l * 0.4, 3.0);
        kleg(&mut c, cx + off, cy + 4.0, cx + off + 12.0, cy + 13.0 + l * 0.4, cx + off + 15.0, cy + 25.0 - l * 0.4, 3.0);
    }
    // Low wedge body: armored plates sloping to a spade head.
    c.poly(&[
        (cx - dx * 26.0 - dy * 10.0, cy - dy * 12.0 + dx * 6.0),
        (cx + dx * 12.0, cy + dy * 6.0 - 12.0),
        (cx + dx * 26.0, cy + dy * 13.0),
        (cx + dx * 12.0, cy + dy * 6.0 + 9.0),
        (cx - dx * 26.0 + dy * 10.0, cy - dy * 12.0 - dx * 6.0),
    ], rgba(CHITIN));
    c.dome(cx - dx * 8.0, cy - 4.0, 14.0, 9.0, scale_rgb(CHITIN, 1.12));
    scallop(&mut c, cx - dx * 8.0, cy - 2.0, 12.0, 16.0);
    // Dorsal team ridge.
    for k in 0..3 {
        let sx = cx - dx * 16.0 + k as f32 * 7.0 * dx - 7.0 + 7.0 * k as f32 * (1.0 - dx.abs());
        c.poly(&[(sx - 2.0, cy - 12.0), (sx, cy - 19.0), (sx + 2.0, cy - 12.0)], rgba(team));
    }
    // Digging scythes: broad spade claws toward facing.
    for side in [-1.0f32, 1.0] {
        let px = cx + dx * 20.0 - dy * side * 9.0;
        let py = cy + dy * 11.0 + dx * side * 5.0;
        c.poly(&[
            (px - 3.0, py - 4.0),
            (px + dx * 16.0 + 2.0, py + dy * 9.0 - 2.0),
            (px + dx * 18.0, py + dy * 10.0 + 4.0),
            (px + dx * 8.0, py + dy * 5.0 + 6.0),
        ], rgba(CHITIN_LIGHT));
        c.line(px, py, px + dx * 16.0, py + dy * 9.0, 1.6, rgba(team));
    }
    if dy > -0.5 {
        c.set((cx + dx * 10.0 - 2.0) as i32, (cy + dy * 5.0 - 4.0) as i32, rgba(KYTH_GLOW));
        c.set((cx + dx * 10.0 + 2.0) as i32, (cy + dy * 5.0 - 4.0) as i32, rgba(KYTH_GLOW));
        c.glow(cx + dx * 10.0, cy + dy * 5.0 - 4.0, 5.0, KYTH_GLOW, 0.7);
    }
    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.3);
    c
}

/// Organic mound base shared by Kyth structures: creep skirt, lumpy
/// overlapped domes, a scalloped chitin cap, jagged team vein network and
/// acid spiracle pores.
fn kyth_mound(c: &mut Canvas, cx: f32, cy: f32, rx: f32, ry: f32, team: [u8; 3]) {
    c.ellipse(cx, cy + ry * 0.45, rx * 1.3, ry * 0.6, [34, 26, 38, 170]);
    // Base membrane mass with a ring of rim lumps breaking the silhouette.
    c.dome(cx, cy, rx, ry, MEMBRANE);
    for k in 0..9 {
        let h = hash2(k, rx as i32, 355);
        let a = k as f32 * 0.7 + (h % 30) as f32 * 0.01;
        let lr = rx * (0.14 + ((h >> 8) % 10) as f32 * 0.008);
        let lx = cx + a.cos() * rx * 0.88;
        let ly = cy + a.sin() * ry * 0.72;
        let col = if (h >> 4) % 4 == 0 {
            scale_rgb(MEMBRANE, 0.95)
        } else {
            scale_rgb(CHITIN, 0.88 + ((h >> 6) % 24) as f32 * 0.01)
        };
        c.dome(lx, ly, lr, lr * 0.62, col);
    }
    // Chitin mass over the crown.
    c.dome(cx - rx * 0.24, cy - ry * 0.22, rx * 0.7, ry * 0.66, CHITIN);
    c.dome(cx + rx * 0.3, cy - ry * 0.08, rx * 0.52, ry * 0.56, scale_rgb(CHITIN, 1.08));
    c.dome(cx, cy - ry * 0.38, rx * 0.5, ry * 0.46, CHITIN_LIGHT);
    scallop(c, cx, cy - ry * 0.33, rx * 0.46, rx * 0.55);
    // Mottled pox texture: dark pores with a lit upper lip.
    let pores = (rx * ry / 90.0) as i32;
    for k in 0..pores {
        let h = hash2(k, (rx + ry) as i32, 641);
        let a = (h % 628) as f32 / 100.0;
        let rr = ((h >> 10) % 900) as f32 / 1000.0;
        let px = cx + a.cos() * rx * rr * 0.9;
        let py = cy - ry * 0.15 + a.sin() * ry * rr * 0.8;
        let sz = 1.5 + ((h >> 20) % 3) as f32;
        c.ellipse(px, py, sz, sz * 0.7, [30, 22, 36, 90]);
        c.blend((px - 1.0) as i32, (py - 1.0) as i32, [150, 128, 160, 70]);
    }
    // Embedded vein pair: thick trunk forking into pore-lit branches.
    for side in [-1.0f32, 1.0] {
        let vein = scale_rgb(team, 0.72);
        let x0 = cx + side * rx * 0.1;
        let y0 = cy - ry * 0.25;
        let mx = cx + side * rx * 0.5;
        let my = cy + ry * 0.12;
        c.line(x0, y0, mx, my, 4.0, rgba(vein));
        for (fx, fy) in [(rx * 0.85, ry * 0.42), (rx * 0.68, ry * 0.62)] {
            let ex = cx + side * fx;
            let ey = cy + fy;
            c.line(mx, my, ex, ey, 2.2, rgba(scale_rgb(vein, 0.85)));
            c.set(ex as i32, ey as i32, rgba(scale_rgb(team, 1.25)));
            c.glow(ex, ey, 4.5, team, 0.45);
        }
    }
    // Acid spiracles.
    for k in 0..3 {
        let h = hash2(k, rx as i32, 977);
        let px = cx - rx * 0.55 + (h % (rx as u32 * 11 / 10)) as f32;
        let py = cy + ((h >> 8) % (ry as u32 / 2)) as f32;
        c.set(px as i32, py as i32, rgba(KYTH_GLOW));
        c.glow(px, py, 4.0, KYTH_GLOW, 0.5);
    }
}


/// Hive: the swarm HQ — great mound, chimney spires, toothed maw. 400x312.
fn paint_hive(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(400, 312);
    kyth_mound(&mut c, 200.0, 208.0, 176.0, 88.0, team);
    // Twisted chimney spires venting spore glow.
    for (sx, sh, lean) in [(120.0f32, 88.0f32, -10.0f32), (208.0, 120.0, 6.0), (280.0, 72.0, 12.0)] {
        let base_y = 176.0;
        let mid = (sx + lean * 0.6, base_y - sh * 0.55);
        let tip = (sx + lean, base_y - sh);
        c.line(sx, base_y, mid.0, mid.1, 16.0, rgba(CHITIN));
        c.line(mid.0, mid.1, tip.0, tip.1, 10.0, rgba(CHITIN_LIGHT));
        scallop(&mut c, mid.0, mid.1 + 4.0, 9.0, 12.0);
        c.poly(&[(tip.0 - 9.0, tip.1 + 4.0), (tip.0 - 3.0, tip.1 - 6.0), (tip.0 + 3.0, tip.1 - 6.0), (tip.0 + 9.0, tip.1 + 4.0)], rgba(CHITIN_LIGHT));
        c.ellipse(tip.0, tip.1 - 5.0, 4.5, 2.5, rgba([30, 22, 36]));
        c.glow(tip.0, tip.1 - 5.0, 10.0, KYTH_GLOW, 0.7);
        c.glow(tip.0 + 3.0, tip.1 - 16.0, 7.0, KYTH_GLOW, 0.3);
    }
    // Toothed maw.
    c.ellipse(200.0, 240.0, 44.0, 20.0, rgba([30, 22, 36]));
    c.glow(200.0, 238.0, 30.0, KYTH_GLOW, 0.55);
    c.ellipse(200.0, 237.0, 26.0, 10.0, rgba(scale_rgb(KYTH_GLOW, 0.8)));
    for k in 0..6 {
        let x = 164.0 + k as f32 * 14.0;
        c.poly(&[(x - 4.0, 224.0), (x, 236.0), (x + 4.0, 224.0)], rgba(CHITIN_LIGHT));
        c.poly(&[(x + 3.0, 254.0), (x + 7.0, 242.0), (x + 11.0, 254.0)], rgba(CHITIN_LIGHT));
    }
    // Egg sacs at the base.
    c.dome(96.0, 244.0, 18.0, 13.0, MEMBRANE);
    c.glow(96.0, 246.0, 8.0, KYTH_GLOW, 0.4);
    c.dome(308.0, 238.0, 15.0, 11.0, MEMBRANE);
    c.outline_t(OUTLINE, 3);
    c.rim(OUTLINE, 1.22);
    c
}

/// Spire: twisted supply spike. 256x240.
fn paint_spire(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(256, 240);
    kyth_mound(&mut c, 128.0, 184.0, 104.0, 48.0, team);
    // Twisting spike in three tapered segments.
    c.line(128.0, 168.0, 112.0, 96.0, 24.0, rgba(CHITIN));
    c.line(112.0, 96.0, 124.0, 44.0, 15.0, rgba(CHITIN_LIGHT));
    c.line(124.0, 44.0, 134.0, 16.0, 8.0, rgba(scale_rgb(CHITIN_LIGHT, 1.12)));
    // Ridge crescents up the shaft.
    for (rx2, ry2, rr) in [(120.0f32, 140.0f32, 15.0f32), (114.0, 110.0, 12.0), (118.0, 78.0, 10.0), (126.0, 50.0, 7.0)] {
        scallop(&mut c, rx2, ry2, rr, rr * 1.2);
    }
    // Crystal shard crown.
    c.poly(&[(128.0, 30.0), (134.0, 2.0), (140.0, 30.0)], rgba(scale_rgb(team, 1.2)));
    c.poly(&[(120.0, 34.0), (124.0, 14.0), (130.0, 32.0)], rgba(team));
    c.poly(&[(138.0, 32.0), (144.0, 16.0), (148.0, 34.0)], rgba(scale_rgb(team, 0.8)));
    c.line(133.0, 26.0, 134.0, 6.0, 1.4, rgba([250, 252, 255]));
    c.glow(134.0, 14.0, 16.0, team, 0.7);
    c.glow(114.0, 96.0, 8.0, KYTH_GLOW, 0.6);
    c.set(114, 96, rgba(KYTH_GLOW));
    c.outline_t(OUTLINE, 3);
    c.rim(OUTLINE, 1.22);
    c
}

/// Sap Well: membrane pool over a geyser. 272x224.
fn paint_sapwell(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(272, 224);
    kyth_mound(&mut c, 136.0, 160.0, 120.0, 56.0, team);
    // Sap pool: dark rim, luminous sap, wet highlight.
    c.ellipse(136.0, 144.0, 56.0, 26.0, rgba([34, 26, 40]));
    c.glow(136.0, 144.0, 44.0, [120, 220, 120], 0.5);
    c.ellipse(136.0, 144.0, 40.0, 18.0, rgba([96, 200, 96]));
    c.ellipse(136.0, 142.0, 26.0, 11.0, rgba([150, 240, 140]));
    c.ellipse(124.0, 138.0, 12.0, 6.0, rgba([210, 255, 190]));
    // Rising sap bubbles.
    for k in 0..4 {
        let h = hash2(k, 11, 431);
        let bx = 108.0 + (h % 56) as f32;
        let by = 120.0 - (k as f32 * 9.0);
        c.glow(bx, by, 4.0, [150, 240, 140], 0.5);
    }
    // Siphon tube arcing out of the pool.
    c.line(184.0, 128.0, 216.0, 84.0, 14.0, rgba(CHITIN_LIGHT));
    c.line(216.0, 84.0, 222.0, 70.0, 9.0, rgba(scale_rgb(CHITIN_LIGHT, 1.1)));
    scallop(&mut c, 200.0, 106.0, 12.0, 14.0);
    c.dome(222.0, 66.0, 10.0, 8.0, [46, 36, 54]);
    c.glow(222.0, 64.0, 12.0, KYTH_GLOW, 0.8);
    c.outline_t(OUTLINE, 3);
    c.rim(OUTLINE, 1.22);
    c
}

/// Warren: infantry mound with a birthing maw. 384x264.
fn paint_warren(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(384, 264);
    kyth_mound(&mut c, 192.0, 176.0, 168.0, 80.0, team);
    // Birthing maw: deep, toothed top and bottom.
    c.ellipse(192.0, 208.0, 56.0, 26.0, rgba([26, 20, 32]));
    c.glow(192.0, 206.0, 38.0, KYTH_GLOW, 0.45);
    c.ellipse(192.0, 204.0, 34.0, 13.0, rgba([44, 60, 40]));
    for k in 0..7 {
        let x = 148.0 + k as f32 * 15.0;
        c.poly(&[(x - 5.0, 188.0), (x, 204.0), (x + 5.0, 188.0)], rgba(CHITIN_LIGHT));
        c.poly(&[(x + 3.0, 228.0), (x + 8.0, 212.0), (x + 13.0, 228.0)], rgba(scale_rgb(CHITIN_LIGHT, 0.85)));
    }
    // Armored back plates.
    c.dome(120.0, 116.0, 40.0, 22.0, CHITIN_LIGHT);
    scallop(&mut c, 120.0, 122.0, 34.0, 40.0);
    c.dome(248.0, 108.0, 48.0, 26.0, CHITIN_LIGHT);
    scallop(&mut c, 248.0, 114.0, 42.0, 48.0);
    c.outline_t(OUTLINE, 3);
    c.rim(OUTLINE, 1.22);
    c
}

/// Incubator: egg cluster, one hatching. 384x280.
fn paint_incubator(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(384, 280);
    kyth_mound(&mut c, 192.0, 200.0, 168.0, 76.0, team);
    // Eggs: membrane shells with subsurface glow.
    for (ex, ey, r) in [(128.0f32, 144.0f32, 32.0f32), (208.0, 120.0, 40.0), (272.0, 152.0, 28.0), (160.0, 180.0, 20.0), (248.0, 196.0, 16.0)] {
        c.dome(ex, ey, r, r * 1.15, MEMBRANE);
        c.ellipse(ex - r * 0.3, ey - r * 0.45, r * 0.32, r * 0.4, rgba(scale_rgb(MEMBRANE, 1.3)));
        c.glow(ex, ey + r * 0.3, r * 0.6, KYTH_GLOW, 0.45);
        // Mottling.
        for k in 0..5 {
            let h = hash2(k, ex as i32, 733);
            let mx = ex - r * 0.6 + (h % (r as u32 * 6 / 5)) as f32;
            let my = ey - r * 0.5 + ((h >> 8) % (r as u32)) as f32;
            c.blend(mx as i32, my as i32, [40, 30, 44, 120]);
        }
    }
    // The big egg is hatching: glowing crack.
    c.line(196.0, 96.0, 208.0, 116.0, 2.0, rgba(scale_rgb(KYTH_GLOW, 1.1)));
    c.line(208.0, 116.0, 202.0, 132.0, 1.6, rgba(KYTH_GLOW));
    c.glow(204.0, 116.0, 10.0, KYTH_GLOW, 0.7);
    c.outline_t(OUTLINE, 3);
    c.rim(OUTLINE, 1.22);
    c
}

/// Roost: flyer perch tower with hanging pods. 336x296.
fn paint_roost(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(336, 296);
    kyth_mound(&mut c, 168.0, 224.0, 144.0, 64.0, team);
    // Perch stalk: thick two-segment trunk.
    c.line(168.0, 208.0, 156.0, 120.0, 28.0, rgba(CHITIN));
    c.line(156.0, 120.0, 152.0, 56.0, 18.0, rgba(CHITIN_LIGHT));
    scallop(&mut c, 160.0, 160.0, 16.0, 18.0);
    scallop(&mut c, 154.0, 100.0, 12.0, 14.0);
    // Branch arms.
    c.line(154.0, 64.0, 96.0, 96.0, 10.0, rgba(CHITIN_LIGHT));
    c.line(152.0, 56.0, 224.0, 80.0, 10.0, rgba(CHITIN_LIGHT));
    // Hanging pods: membrane sacs with glowing undersides + tendrils.
    for (px, py) in [(96.0f32, 112.0f32), (224.0, 96.0)] {
        c.line(px, py - 14.0, px, py - 2.0, 3.0, rgba([58, 44, 68]));
        c.dome(px, py + 8.0, 16.0, 20.0, MEMBRANE);
        c.glow(px, py + 20.0, 12.0, KYTH_GLOW, 0.6);
        c.ellipse(px, py + 22.0, 7.0, 5.0, rgba(scale_rgb(KYTH_GLOW, 0.9)));
        c.line(px - 4.0, py + 26.0, px - 6.0, py + 38.0, 1.5, rgba([58, 44, 68]));
    }
    // Team bulb crown.
    c.dome(150.0, 48.0, 12.0, 10.0, team);
    c.glow(150.0, 46.0, 16.0, team, 0.7);
    c.outline_t(OUTLINE, 3);
    c.rim(OUTLINE, 1.22);
    c
}

/// Cortex: exposed psionic brain. 264x224.
fn paint_cortex(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(264, 224);
    kyth_mound(&mut c, 132.0, 168.0, 112.0, 52.0, team);
    // Brain: wet folds with deep sulci.
    c.dome(132.0, 104.0, 64.0, 48.0, [172, 130, 148]);
    for k in 0..5 {
        let y = 72.0 + k as f32 * 18.0;
        let amp = 6.0 - k as f32;
        let x0 = 132.0 - 56.0 + k as f32 * 6.0;
        let x1 = 132.0 + 56.0 - k as f32 * 6.0;
        let mid = ((x0 + x1) * 0.5, y + amp + 4.0);
        c.line(x0, y, mid.0, mid.1, 2.2, rgba([128, 90, 110]));
        c.line(mid.0, mid.1, x1, y + 2.0, 2.2, rgba([128, 90, 110]));
    }
    c.ellipse(108.0, 76.0, 16.0, 9.0, rgba([210, 168, 184]));
    // Membrane collar where brain meets mound.
    scallop(&mut c, 132.0, 140.0, 58.0, 64.0);
    // Psionic storm above: arcs + motes.
    c.glow(132.0, 52.0, 22.0, KYTH_GLOW, 0.5);
    c.line(116.0, 64.0, 106.0, 40.0, 1.8, rgba(KYTH_GLOW));
    c.line(150.0, 60.0, 162.0, 38.0, 1.8, rgba(KYTH_GLOW));
    c.line(132.0, 56.0, 130.0, 32.0, 1.4, rgba(scale_rgb(KYTH_GLOW, 1.15)));
    for (mx2, my2) in [(104.0f32, 36.0f32), (164.0, 34.0), (130.0, 26.0)] {
        c.set(mx2 as i32, my2 as i32, rgba([220, 255, 180]));
        c.glow(mx2, my2, 4.0, KYTH_GLOW, 0.5);
    }
    c.outline_t(OUTLINE, 3);
    c.rim(OUTLINE, 1.22);
    c
}

// ------------------------------------------------------------- buildings ----

/// Iso building block at 4x: top plate + two walls with panel seams, wear
/// speckle, a lit roof lip and an optional emissive trim line under it.
/// The bread and butter of every Vanguard structure.
fn iso_box4(
    c: &mut Canvas,
    cx: f32,
    top_cy: f32,
    half_w: f32,
    wall_h: f32,
    top: [u8; 3],
    trim: Option<[u8; 3]>,
) {
    let half_h = half_w * 0.5;
    for x in (cx - half_w) as i32..=(cx + half_w) as i32 {
        let dxf = (x as f32 + 0.5 - cx) / half_w;
        if dxf.abs() > 1.0 {
            continue;
        }
        let edge_y = top_cy + half_h * (1.0 - dxf.abs());
        let left = dxf < 0.0;
        let base = if left { scale_rgb(top, 0.50) } else { scale_rgb(top, 0.72) };
        let seam = (x - (cx - half_w) as i32) % 16;
        for wy in 0..wall_h as i32 {
            let mut t = base;
            let h = hash2(x, wy, 271);
            if h % 43 == 0 {
                t = scale_rgb(base, 0.86);
            }
            if h % 57 == 0 {
                t = scale_rgb(base, 1.10);
            }
            if seam == 0 {
                t = scale_rgb(base, 0.78);
            }
            if seam == 1 {
                t = scale_rgb(base, 1.08);
            }
            if wy == 0 {
                t = scale_rgb(base, 1.45);
            }
            if wy >= wall_h as i32 - 2 {
                t = scale_rgb(base, 0.55);
            }
            if let Some(a) = trim {
                if wy == 2 || wy == 3 {
                    t = a;
                }
            }
            c.set(x, (edge_y + wy as f32) as i32, rgba(t));
        }
    }
    for y in (top_cy - half_h) as i32..=(top_cy + half_h) as i32 {
        for x in (cx - half_w) as i32..=(cx + half_w) as i32 {
            let dxf = (x as f32 + 0.5 - cx) / half_w;
            let dyf = (y as f32 + 0.5 - top_cy) / half_h;
            if dxf.abs() + dyf.abs() > 1.0 {
                continue;
            }
            let h = hash2(x, y, 653);
            let mut f = if dxf - dyf < -0.2 {
                1.12
            } else if dxf - dyf > 0.5 {
                0.92
            } else {
                1.0
            };
            if h % 61 == 0 {
                f *= 0.88;
            }
            if h % 71 == 0 {
                f *= 1.10;
            }
            // Roof plating seams parallel to the diamond edges.
            let s1 = ((x as f32 - cx) * 0.5 + (y as f32 - top_cy)).rem_euclid(22.0);
            let s2 = ((x as f32 - cx) * 0.5 - (y as f32 - top_cy)).rem_euclid(26.0);
            if s1 < 1.0 || s2 < 1.0 {
                f *= 0.82;
            }
            c.set(x, y, rgba(scale_rgb(top, f)));
        }
    }
}

/// Emissive window strips along a wall, with glow halos.
fn windows4(c: &mut Canvas, cx: f32, edge_cy: f32, half_w: f32, wall_h: f32, lit: [u8; 3]) {
    let half_h = half_w * 0.5;
    let mut x = cx - half_w + 14.0;
    let mut k = 0;
    while x < cx + half_w - 12.0 {
        let dxf = (x - cx) / half_w;
        let edge_y = edge_cy + half_h * (1.0 - dxf.abs());
        if k % 2 == 0 {
            let wy = edge_y + wall_h * 0.38;
            c.rect(x as i32, wy as i32, 6, 8, rgba(lit));
            c.rect(x as i32 + 1, wy as i32 + 1, 2, 3, rgba(scale_rgb(lit, 1.35)));
            c.glow(x + 3.0, wy + 4.0, 8.0, lit, 0.35);
        }
        x += 14.0;
        k += 1;
    }
}

/// Worn hazard chevrons along an edge.
fn hazard(c: &mut Canvas, x0: f32, y0: f32, n: i32) {
    for k in 0..n {
        let x = x0 + k as f32 * 8.0;
        let col = if k % 2 == 0 { [196, 160, 58] } else { [28, 28, 32] };
        c.poly(&[(x, y0), (x + 4.0, y0), (x + 8.0, y0 + 5.0), (x + 4.0, y0 + 5.0)], rgba(col));
    }
}

/// Industrial pipe with a top highlight and end flanges.
fn pipe(c: &mut Canvas, x0: f32, y0: f32, x1: f32, y1: f32, t: f32) {
    c.line(x0, y0, x1, y1, t, rgba([58, 61, 70]));
    c.line(x0, y0 - t * 0.25, x1, y1 - t * 0.25, t * 0.3, rgba([104, 110, 122]));
    c.rect((x0 - 2.0) as i32, (y0 - t * 0.6) as i32, 4, (t * 1.2) as i32, rgba([76, 80, 92]));
    c.rect((x1 - 2.0) as i32, (y1 - t * 0.6) as i32, 4, (t * 1.2) as i32, rgba([76, 80, 92]));
}

/// Headquarters: fortified command complex. 400x312.
fn paint_hq(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(400, 312);
    let cx = 200.0;
    let concrete = [92, 90, 86];
    let hull = [104, 102, 98];
    iso_box4(&mut c, cx, 216.0, 192.0, 24.0, concrete, None);
    hazard(&mut c, cx - 64.0, 244.0, 16);
    // Main hall.
    iso_box4(&mut c, cx, 160.0, 144.0, 64.0, hull, Some(team));
    windows4(&mut c, cx, 160.0, 144.0, 64.0, [255, 200, 110]);
    // Command tower with its own trim + windows.
    iso_box4(&mut c, cx - 40.0, 104.0, 64.0, 60.0, scale_rgb(hull, 1.06), Some(team));
    windows4(&mut c, cx - 40.0, 104.0, 64.0, 60.0, [255, 200, 110]);
    // Roof greebles: vent block + comms dish.
    iso_box4(&mut c, cx + 56.0, 132.0, 26.0, 14.0, scale_rgb(hull, 1.1), None);
    c.dome(cx - 76.0, 78.0, 14.0, 8.0, [96, 100, 108]);
    c.line(cx - 76.0, 78.0, cx - 66.0, 66.0, 2.0, rgba(STEEL_LIT));
    // Landing pad with a glowing team ring.
    c.ellipse(cx + 72.0, 152.0, 48.0, 24.0, rgba(scale_rgb(concrete, 0.8)));
    c.ellipse(cx + 72.0, 152.0, 40.0, 20.0, rgba(scale_rgb(concrete, 1.05)));
    for k in 0..8 {
        let a = k as f32 * 0.785;
        let lx = cx + 72.0 + a.cos() * 34.0;
        let ly = 152.0 + a.sin() * 17.0;
        c.set(lx as i32, ly as i32, rgba(scale_rgb(team, 1.3)));
        c.glow(lx, ly, 4.5, team, 0.55);
    }
    c.rect((cx + 68.0) as i32, 146, 3, 12, rgba(scale_rgb(team, 1.2)));
    c.rect((cx + 74.0) as i32, 146, 3, 12, rgba(scale_rgb(team, 1.2)));
    // Storage tanks + feed pipe.
    c.dome(cx + 132.0, 200.0, 20.0, 14.0, [84, 88, 96]);
    c.dome(cx + 160.0, 208.0, 15.0, 11.0, [78, 82, 90]);
    pipe(&mut c, cx + 112.0, 180.0, cx + 132.0, 198.0, 5.0);
    // Antenna mast + red strobe.
    c.line(cx - 56.0, 96.0, cx - 56.0, 40.0, 2.5, rgba([80, 84, 94]));
    c.line(cx - 56.0, 50.0, cx - 44.0, 58.0, 1.5, rgba([80, 84, 94]));
    c.glow(cx - 56.0, 38.0, 6.0, [255, 90, 80], 0.85);
    c.set((cx - 56.0) as i32, 38, rgba([255, 130, 120]));
    c.outline_t(OUTLINE, 3);
    c.rim(OUTLINE, 1.22);
    c
}

/// Supply Pylon: fenced power obelisk. 272x248.
fn paint_pylon(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(272, 248);
    let cx = 136.0;
    iso_box4(&mut c, cx, 184.0, 120.0, 24.0, [88, 86, 82], None);
    hazard(&mut c, cx - 40.0, 212.0, 10);
    // Anchor collar.
    iso_box4(&mut c, cx, 172.0, 44.0, 20.0, [70, 73, 82], Some(team));
    // Cables to the plate corners.
    c.line(cx - 36.0, 182.0, cx - 92.0, 196.0, 2.0, rgba([48, 50, 58]));
    c.line(cx + 36.0, 182.0, cx + 92.0, 196.0, 2.0, rgba([48, 50, 58]));
    // Obelisk: tapered energy crystal with hard facets.
    for y in 48..172 {
        let f = (y - 48) as f32 / 124.0;
        let w = 10.0 + f * 34.0;
        for x in (cx - w) as i32..=(cx + w) as i32 {
            let side = (x as f32 - cx) / w;
            let base = if side < -0.25 {
                scale_rgb(team, 1.3)
            } else if side > 0.45 {
                scale_rgb(team, 0.55)
            } else {
                team
            };
            c.set(x, y, rgba(base));
        }
    }
    // White-hot core slit + halo.
    c.rect((cx - 3.0) as i32, 70, 6, 70, rgba([245, 250, 255]));
    c.glow(cx, 110.0, 44.0, team, 0.4);
    // Floating tip shard.
    c.poly(&[(cx - 7.0, 34.0), (cx, 12.0), (cx + 7.0, 34.0), (cx, 42.0)], rgba(scale_rgb(team, 1.35)));
    c.glow(cx, 26.0, 12.0, team, 0.8);
    c.outline_t(OUTLINE, 3);
    c.rim(OUTLINE, 1.2);
    c
}

/// Muster Hall: bunker barracks with a blast door. 400x280.
fn paint_barracks(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(400, 280);
    let cx = 200.0;
    let hull = [100, 96, 90];
    iso_box4(&mut c, cx, 200.0, 192.0, 24.0, [88, 87, 84], None);
    iso_box4(&mut c, cx, 136.0, 152.0, 72.0, hull, Some(team));
    windows4(&mut c, cx, 136.0, 152.0, 72.0, [255, 200, 110]);
    // Blast door on the lower-left wall: dark slab, glowing team seam,
    // hazard posts, entry steps.
    let dxf: f32 = -56.0;
    let edge_y = 136.0 + 76.0 * (1.0 - (dxf / 152.0f32).abs());
    let dx0 = cx + dxf;
    c.poly(&[(dx0 - 20.0, edge_y - 26.0), (dx0 + 20.0, edge_y - 18.0), (dx0 + 20.0, edge_y + 14.0), (dx0 - 20.0, edge_y + 10.0)], rgba([40, 42, 48]));
    c.line(dx0, edge_y - 22.0, dx0, edge_y + 12.0, 2.5, rgba(scale_rgb(team, 1.2)));
    c.glow(dx0, edge_y - 4.0, 12.0, team, 0.4);
    c.rect((dx0 - 24.0) as i32, (edge_y - 26.0) as i32, 4, 40, rgba([196, 160, 58]));
    c.rect((dx0 + 20.0) as i32, (edge_y - 18.0) as i32, 4, 34, rgba([196, 160, 58]));
    hazard(&mut c, dx0 - 24.0, edge_y + 16.0, 7);
    // Roof vents.
    for k in 0..3 {
        let vx = cx - 48.0 + k as f32 * 48.0;
        iso_box4(&mut c, vx, 104.0, 20.0, 14.0, scale_rgb(hull, 1.1), None);
    }
    // Supply crates on the apron.
    for (bx, by, bw) in [(cx + 120.0, 196.0, 14.0), (cx + 142.0, 204.0, 11.0), (cx + 106.0, 208.0, 9.0)] {
        c.poly(&[(bx - bw, by), (bx, by - bw * 0.5), (bx + bw, by), (bx, by + bw * 0.5)], rgba([86, 90, 82]));
        c.poly(&[(bx - bw, by), (bx, by + bw * 0.5), (bx, by + bw * 0.9), (bx - bw, by + bw * 0.4)], rgba([62, 65, 60]));
        c.poly(&[(bx + bw, by), (bx, by + bw * 0.5), (bx, by + bw * 0.9), (bx + bw, by + bw * 0.4)], rgba([74, 78, 71]));
    }
    // Comms antenna.
    c.line(cx + 96.0, 112.0, cx + 96.0, 76.0, 2.0, rgba([80, 84, 94]));
    c.glow(cx + 96.0, 74.0, 5.0, [255, 90, 80], 0.7);
    c.outline_t(OUTLINE, 3);
    c.rim(OUTLINE, 1.22);
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

/// Plasma Condenser: tanked extractor rig. 272x232.
fn paint_condenser(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(272, 232);
    let cx = 136.0;
    let hull = [96, 94, 88];
    iso_box4(&mut c, cx, 168.0, 120.0, 24.0, [86, 85, 82], None);
    iso_box4(&mut c, cx, 128.0, 88.0, 48.0, hull, Some(team));
    windows4(&mut c, cx, 128.0, 88.0, 48.0, [255, 200, 110]);
    // Condensation tank: steel dome with a glowing teal sight-glass.
    c.dome(cx - 24.0, 88.0, 36.0, 26.0, [108, 112, 118]);
    c.ellipse(cx - 24.0, 76.0, 20.0, 12.0, rgba([64, 210, 200]));
    c.ellipse(cx - 30.0, 72.0, 8.0, 5.0, rgba([190, 255, 248]));
    c.glow(cx - 24.0, 76.0, 26.0, [64, 210, 200], 0.4);
    // Secondary tank.
    c.dome(cx + 40.0, 96.0, 20.0, 14.0, [92, 96, 104]);
    // Intake pipework down to the vent side.
    pipe(&mut c, cx + 32.0, 104.0, cx + 80.0, 136.0, 7.0);
    pipe(&mut c, cx + 56.0, 120.0, cx + 56.0, 148.0, 5.0);
    // Gauge lights.
    c.set((cx + 40.0) as i32, 96, rgba([64, 220, 210]));
    c.glow(cx + 40.0, 96.0, 5.0, [64, 220, 210], 0.7);
    c.outline_t(OUTLINE, 3);
    c.rim(OUTLINE, 1.22);
    c
}

/// Forge: heavy foundry — furnace maw, chimney, crane. 400x296.
fn paint_forge(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(400, 296);
    let cx = 200.0;
    let hull = [98, 92, 84];
    iso_box4(&mut c, cx, 200.0, 192.0, 24.0, [86, 85, 82], None);
    iso_box4(&mut c, cx, 128.0, 152.0, 80.0, hull, Some(team));
    windows4(&mut c, cx, 128.0, 152.0, 80.0, [255, 190, 110]);
    // Furnace maw on the lower-left wall: layered heat glow.
    let mx = cx - 96.0;
    let my = 184.0;
    c.poly(&[(mx - 22.0, my + 14.0), (mx - 18.0, my - 12.0), (mx, my - 20.0), (mx + 18.0, my - 12.0), (mx + 22.0, my + 14.0)], rgba([34, 32, 36]));
    c.glow(mx, my, 26.0, [255, 130, 50], 0.75);
    c.poly(&[(mx - 14.0, my + 12.0), (mx - 10.0, my - 6.0), (mx, my - 12.0), (mx + 10.0, my - 6.0), (mx + 14.0, my + 12.0)], rgba([255, 150, 60]));
    c.poly(&[(mx - 7.0, my + 10.0), (mx, my - 4.0), (mx + 7.0, my + 10.0)], rgba([255, 230, 150]));
    hazard(&mut c, mx - 24.0, my + 18.0, 7);
    // Slag channel glowing across the apron.
    c.line(mx, my + 16.0, mx - 24.0, my + 34.0, 4.0, rgba([120, 52, 30]));
    c.line(mx, my + 16.0, mx - 24.0, my + 34.0, 1.6, rgba([255, 140, 60]));
    // Chimney with ember glow + smoke.
    iso_box4(&mut c, cx + 88.0, 64.0, 28.0, 56.0, scale_rgb(hull, 0.92), None);
    c.glow(cx + 88.0, 52.0, 12.0, [255, 120, 50], 0.5);
    for k in 0..3 {
        let h = hash2(k, 5, 60);
        c.glow(cx + 88.0 + (h % 9) as f32 - 4.0, 34.0 - k as f32 * 12.0, 8.0 + k as f32 * 2.0, [120, 120, 126], 0.25);
    }
    // Gantry crane.
    c.line(cx - 120.0, 104.0, cx - 56.0, 72.0, 3.0, rgba([70, 74, 84]));
    c.line(cx - 88.0, 88.0, cx - 88.0, 116.0, 1.5, rgba([60, 63, 72]));
    c.rect((cx - 92.0) as i32, 116, 8, 6, rgba(STEEL_LIT));
    c.outline_t(OUTLINE, 3);
    c.rim(OUTLINE, 1.22);
    c
}

/// Aerie: flight deck + control tower. 400x280.
fn paint_aerie(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(400, 280);
    let cx = 200.0;
    iso_box4(&mut c, cx, 192.0, 192.0, 24.0, [86, 85, 82], None);
    // Raised flight deck.
    iso_box4(&mut c, cx + 32.0, 152.0, 136.0, 32.0, [76, 80, 88], None);
    hazard(&mut c, cx - 48.0, 220.0, 12);
    // Deck markings: rings + team glow ring + approach chevrons.
    c.ellipse(cx + 32.0, 152.0, 52.0, 26.0, rgba([96, 102, 112]));
    c.ellipse(cx + 32.0, 152.0, 40.0, 20.0, rgba([76, 80, 88]));
    for k in 0..10 {
        let a = k as f32 * 0.628;
        let lx = cx + 32.0 + a.cos() * 46.0;
        let ly = 152.0 + a.sin() * 23.0;
        c.set(lx as i32, ly as i32, rgba(scale_rgb(team, 1.3)));
        c.glow(lx, ly, 4.0, team, 0.5);
    }
    for k in 0..3 {
        let x = cx - 32.0 + k as f32 * 12.0;
        c.poly(&[(x, 148.0), (x + 6.0, 152.0), (x, 156.0), (x - 6.0, 152.0)], rgba(scale_rgb(team, 0.9)));
    }
    // Control tower with a glass band.
    iso_box4(&mut c, cx - 112.0, 88.0, 48.0, 72.0, [104, 102, 98], Some(team));
    let gy = 96;
    c.rect((cx - 136.0) as i32, gy, 48, 10, rgba([110, 220, 245]));
    c.rect((cx - 132.0) as i32, gy + 2, 12, 4, rgba([210, 245, 255]));
    c.glow(cx - 112.0, gy as f32 + 5.0, 26.0, VISOR, 0.35);
    // Dish + mast + strobe.
    c.dome(cx - 88.0, 52.0, 12.0, 7.0, [96, 100, 108]);
    c.line(cx - 112.0, 52.0, cx - 112.0, 18.0, 2.5, rgba([80, 84, 94]));
    c.glow(cx - 112.0, 16.0, 6.0, [255, 90, 80], 0.85);
    c.set((cx - 112.0) as i32, 16, rgba([255, 130, 120]));
    c.outline_t(OUTLINE, 3);
    c.rim(OUTLINE, 1.22);
    c
}

/// Archive: faceted research dome + antenna array. 280x248.
fn paint_archive(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(280, 248);
    let cx = 140.0;
    iso_box4(&mut c, cx, 184.0, 120.0, 24.0, [86, 85, 82], None);
    iso_box4(&mut c, cx, 148.0, 92.0, 32.0, [100, 98, 94], Some(team));
    // Dome with horizontal band seams.
    c.dome(cx, 116.0, 76.0, 52.0, [112, 110, 106]);
    for band in 0..4 {
        let by = 84.0 + band as f32 * 18.0;
        for x in (cx - 74.0) as i32..=(cx + 74.0) as i32 {
            let dx = (x as f32 - cx) / 76.0;
            let dy = (by - 116.0) / 52.0;
            if dx * dx + dy * dy < 0.96 {
                c.blend(x, by as i32, [30, 30, 34, 90]);
            }
        }
    }
    // Data window band: teal cells around the equator.
    for k in 0..7 {
        let x = cx - 54.0 + k as f32 * 18.0;
        c.rect(x as i32, 132, 8, 10, rgba([90, 210, 245]));
        c.rect(x as i32 + 2, 134, 3, 4, rgba([200, 245, 255]));
        c.glow(x + 4.0, 137.0, 9.0, [90, 210, 245], 0.35);
    }
    // Antenna array with cyan tips.
    for (ax, ah) in [(-40.0f32, 44.0f32), (0.0, 60.0), (40.0, 36.0)] {
        c.line(cx + ax, 76.0, cx + ax, 76.0 - ah, 2.2, rgba([80, 84, 94]));
        c.line(cx + ax, 76.0 - ah * 0.6, cx + ax + 8.0, 76.0 - ah * 0.6 - 5.0, 1.3, rgba([80, 84, 94]));
        c.glow(cx + ax, 74.0 - ah, 5.0, [140, 240, 255], 0.8);
        c.set((cx + ax) as i32, (74.0 - ah) as i32, rgba([190, 250, 255]));
    }
    c.outline_t(OUTLINE, 3);
    c.rim(OUTLINE, 1.22);
    c
}

// --------------------------------------------------------------- effects ----

fn star_flash() -> Canvas {
    let mut c = Canvas::new(44, 44);
    c.glow(22.0, 22.0, 20.0, [255, 230, 150], 0.55);
    for k in 0..4 {
        let a = k as f32 * std::f32::consts::FRAC_PI_4;
        c.line(
            22.0 - a.cos() * 19.0,
            22.0 - a.sin() * 19.0,
            22.0 + a.cos() * 19.0,
            22.0 + a.sin() * 19.0,
            2.6,
            [255, 230, 150, 255],
        );
    }
    c.ellipse(22.0, 22.0, 8.0, 8.0, [255, 255, 255, 255]);
    c
}


// -------------------------------------------- Ferron Compact painters ----
//
// Salvager machine-cult: rusted iron plates, exposed frames, magnet-violet
// coils and amber furnace light.

const RUST: [u8; 3] = [104, 74, 52];
const RUST_DARK: [u8; 3] = [70, 52, 40];
const RUST_LIT: [u8; 3] = [152, 112, 78];
const SCRAP: [u8; 3] = [92, 86, 80];
const COIL: [u8; 3] = [168, 138, 255];

/// Rust wear pass: streaks + bright chips over a plate region.
fn rust_wear(c: &mut Canvas, x0: i32, y0: i32, w: i32, h: i32, salt: u32) {
    for k in 0..(w * h / 300).max(2) {
        let hh = hash2(k, salt as i32, 733);
        let x = x0 + (hh % w as u32) as i32;
        let y = y0 + ((hh >> 8) % (h as u32 / 2)) as i32;
        c.streak(x, y, 4 + (hh % 8) as i32, [58, 40, 30], 0.5);
        if hh % 5 == 0 {
            c.set(x, y - 1, rgba(RUST_LIT));
        }
    }
}

/// Heavy walker leg: armored thigh quad + shin plate + wide foot, with a
/// piston highlight. The cure for stick-figure syndrome.
fn fer_leg(c: &mut Canvas, hip_x: f32, hip_y: f32, knee_x: f32, knee_y: f32, foot_x: f32, foot_y: f32, t: f32) {
    // Thigh: broad armored quad hip->knee.
    let (dx, dy) = (knee_x - hip_x, knee_y - hip_y);
    let len = (dx * dx + dy * dy).sqrt().max(0.01);
    let (px, py) = (-dy / len * t, dx / len * t);
    c.poly(&[
        (hip_x - px, hip_y - py),
        (hip_x + px, hip_y + py),
        (knee_x + px * 0.7, knee_y + py * 0.7),
        (knee_x - px * 0.7, knee_y - py * 0.7),
    ], rgba(RUST));
    c.line(hip_x - px * 0.4, hip_y - py * 0.4, knee_x - px * 0.3, knee_y - py * 0.3, 1.6, rgba(RUST_LIT));
    // Shin: darker plate knee->foot.
    let (dx2, dy2) = (foot_x - knee_x, foot_y - knee_y);
    let len2 = (dx2 * dx2 + dy2 * dy2).sqrt().max(0.01);
    let (qx, qy) = (-dy2 / len2 * t * 0.75, dx2 / len2 * t * 0.75);
    c.poly(&[
        (knee_x - qx, knee_y - qy),
        (knee_x + qx, knee_y + qy),
        (foot_x + qx * 0.8, foot_y + qy * 0.8),
        (foot_x - qx * 0.8, foot_y - qy * 0.8),
    ], rgba([88, 66, 50]));
    c.line(knee_x - qx * 0.5, knee_y - qy * 0.5, foot_x - qx * 0.4, foot_y - qy * 0.4, 1.4, rgba(RUST_LIT));
    // Piston shine + knee cap.
    c.line(knee_x, knee_y, foot_x * 0.6 + knee_x * 0.4, foot_y * 0.6 + knee_y * 0.4, 1.4, rgba(STEEL_LIT));
    c.dome(knee_x, knee_y, t * 0.9, t * 0.8, SCRAP);
    // Wide stomper foot.
    c.poly(&[
        (foot_x - t * 1.6, foot_y + 2.0),
        (foot_x - t * 0.6, foot_y - 2.5),
        (foot_x + t * 1.4, foot_y - 1.5),
        (foot_x + t * 1.8, foot_y + 3.0),
        (foot_x - t * 1.2, foot_y + 4.0),
    ], rgba([46, 42, 44]));
    c.line(foot_x - t * 1.2, foot_y - 0.5, foot_x + t * 1.2, foot_y + 0.5, 1.3, rgba([80, 74, 70]));
}

/// Scrapper: squat tracked hauler with a magnet boom. 104x96.
fn paint_scrapper(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(104, 96);
    let (dx, dy) = facing_vec(f);
    let cx = 52.0;
    let cy = 52.0;
    let bob = if frame == 0 { 0.0 } else { -1.0 };
    // Dozer tracks with armored side skirts.
    plate(&mut c, &[(18.0, 66.0), (86.0, 66.0), (92.0, 84.0), (12.0, 84.0)], [44, 40, 42], 1.0);
    for k in 0..8 {
        let notch = 16 + k * 10 + (frame as i32) * 5;
        if notch < 88 {
            c.rect(notch, 72, 3, 10, rgba([74, 70, 74]));
        }
    }
    plate(&mut c, &[(14.0, 62.0), (90.0, 62.0), (94.0, 72.0), (10.0, 72.0)], RUST_DARK, 1.0);
    rust_wear(&mut c, 14, 62, 76, 10, 301);
    // Hull: layered rusted plates with a raised cab.
    plate(&mut c, &[(20.0, 40.0 + bob), (84.0, 40.0 + bob), (90.0, 64.0), (14.0, 64.0)], RUST, 1.0);
    plate(&mut c, &[(26.0, 28.0 + bob), (58.0, 28.0 + bob), (62.0, 42.0 + bob), (22.0, 42.0 + bob)], [116, 84, 58], 1.0);
    rust_wear(&mut c, 20, (42.0 + bob) as i32, 64, 20, 307);
    // Cab glass + team band.
    if dy > -0.5 {
        c.poly(&[(30.0 + dx * 4.0, 30.0 + bob), (52.0 + dx * 4.0, 30.0 + bob), (50.0 + dx * 4.0, 38.0 + bob), (32.0 + dx * 4.0, 38.0 + bob)], rgba([150, 210, 235]));
    }
    c.rect(18, 58, 68, 5, rgba(team));
    // Hopper with heaped scrap.
    for k in 0..5 {
        let h = hash2(k, 3, 517);
        c.dome(62.0 + (h % 18) as f32, 34.0 + bob + ((h >> 8) % 6) as f32, 5.0 + (h % 4) as f32, 3.5, SCRAP);
    }
    // Magnet boom: heavy A-frame toward facing + glowing disc.
    let ax = cx + dx * 30.0;
    let ay = cy + bob + dy * 16.0 - 12.0;
    c.line(cx + dx * 6.0 - 4.0, cy + bob - 10.0, cx + dx * 18.0, cy + bob - 22.0, 6.0, rgba(RUST_DARK));
    c.line(cx + dx * 6.0 + 4.0, cy + bob - 8.0, cx + dx * 18.0, cy + bob - 22.0, 4.5, rgba(RUST));
    c.line(cx + dx * 18.0, cy + bob - 22.0, ax, ay, 4.0, rgba(SCRAP));
    c.line(cx + dx * 18.0, cy + bob - 20.0, ax * 0.7 + (cx + dx * 18.0) * 0.3, ay * 0.7 + (cy + bob - 20.0) * 0.3, 1.6, rgba(STEEL_LIT));
    c.dome(ax, ay + 5.0, 10.0, 5.0, [58, 54, 62]);
    c.glow(ax, ay + 7.0, 10.0, COIL, 0.7);
    c.ellipse(ax, ay + 7.0, 4.0, 2.2, rgba(scale_rgb(COIL, 1.2)));
    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.28);
    c
}

/// Arclight: armored chicken-walker with a shoulder arc array. 112x116.
fn paint_arclight(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(112, 116);
    let (dx, dy) = facing_vec(f);
    let cx = 56.0;
    let cy = 56.0;
    let step = if frame == 0 { 5.0 } else { -5.0 };
    // Two heavy reverse-knee legs.
    fer_leg(&mut c, cx - 9.0, cy + 8.0, cx - 15.0 + step * 0.4, cy + 26.0, cx - 11.0 + step, cy + 48.0, 5.5);
    fer_leg(&mut c, cx + 9.0, cy + 8.0, cx + 15.0 - step * 0.4, cy + 26.0, cx + 11.0 - step, cy + 48.0, 5.5);
    // Hip block.
    plate(&mut c, &[(cx - 14.0, cy + 2.0), (cx + 14.0, cy + 2.0), (cx + 12.0, cy + 14.0), (cx - 12.0, cy + 14.0)], RUST_DARK, 1.0);
    // Torso pod: broad, layered, armored.
    plate(&mut c, &[(30.0, 18.0), (82.0, 18.0), (88.0, 48.0), (24.0, 48.0)], RUST, 1.0);
    plate(&mut c, &[(34.0 + dx * 5.0, 22.0), (58.0 + dx * 8.0, 16.0 + dy * 3.0), (58.0 + dx * 8.0, 36.0), (38.0 + dx * 5.0, 42.0)], [124, 90, 62], 1.0);
    rust_wear(&mut c, 30, 20, 52, 26, 311);
    c.rect(32, 44, 48, 5, rgba(team));
    // Optic visor band.
    if dy > -0.5 {
        let vx = cx + dx * 8.0;
        c.rect((vx - 8.0) as i32, 28, 16, 5, rgba([30, 28, 34]));
        c.ellipse(vx, 30.0, 3.0, 2.2, rgba(COIL));
        c.glow(vx, 30.0, 7.0, COIL, 0.8);
    }
    // Shoulder arc array: two heavy prong towers with a live arc between.
    for side in [-1.0f32, 1.0] {
        let px = cx + side * 26.0;
        plate(&mut c, &[(px - 5.0, 8.0), (px + 5.0, 8.0), (px + 7.0, 22.0), (px - 7.0, 22.0)], SCRAP, 1.0);
        c.poly(&[(px - 3.0, 8.0), (px, -0.0 + 2.0), (px + 3.0, 8.0)], rgba(RUST_LIT));
        c.set(px as i32, 3, rgba(scale_rgb(COIL, 1.3)));
    }
    let arc_y = 5.0 + if frame == 0 { 0.0 } else { 2.0 };
    c.line(cx - 26.0, 4.0, cx, arc_y, 1.4, rgba(scale_rgb(COIL, 1.2)));
    c.line(cx, arc_y, cx + 26.0, 4.0, 1.4, rgba(scale_rgb(COIL, 1.2)));
    c.glow(cx, arc_y, 9.0, COIL, 0.7);
    // Cable run.
    c.line(cx - 22.0, 22.0, cx - 12.0, 40.0, 1.6, rgba([60, 50, 46]));
    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.3);
    c
}

/// Mauler: crab-plated wrecking walker. 128x112.
fn paint_mauler(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(128, 112);
    let (dx, dy) = facing_vec(f);
    let cx = 64.0;
    let cy = 56.0;
    let step = if frame == 0 { 4.0 } else { -4.0 };
    // Four heavy legs.
    fer_leg(&mut c, cx - 20.0, cy + 6.0, cx - 34.0, cy + 20.0 - step * 0.3, cx - 30.0 + step, cy + 40.0, 4.0);
    fer_leg(&mut c, cx + 20.0, cy + 6.0, cx + 34.0, cy + 20.0 + step * 0.3, cx + 30.0 - step, cy + 40.0, 4.0);
    fer_leg(&mut c, cx - 10.0, cy + 12.0, cx - 16.0, cy + 26.0 + step * 0.3, cx - 12.0 - step, cy + 44.0, 3.6);
    fer_leg(&mut c, cx + 10.0, cy + 12.0, cx + 16.0, cy + 26.0 - step * 0.3, cx + 12.0 + step, cy + 44.0, 3.6);
    // Carapace: broad domed shell of overlapping plates.
    plate(&mut c, &[(20.0, 34.0), (108.0, 34.0), (100.0, 62.0), (28.0, 62.0)], RUST, 1.0);
    plate(&mut c, &[(30.0, 22.0), (98.0, 22.0), (104.0, 38.0), (24.0, 38.0)], [120, 88, 60], 1.0);
    plate(&mut c, &[(44.0, 12.0), (84.0, 12.0), (92.0, 24.0), (36.0, 24.0)], RUST_LIT, 0.95);
    rust_wear(&mut c, 24, 24, 80, 34, 331);
    c.rect(30, 58, 68, 5, rgba(team));
    // Furnace core glowing through the front seam.
    c.glow(cx + dx * 8.0, 42.0, 11.0, AMBER, 0.75);
    c.rect((cx + dx * 8.0 - 5.0) as i32, 38, 10, 8, rgba([255, 150, 60]));
    // Massive hydraulic claws.
    for side in [-1.0f32, 1.0] {
        let px = cx + dx * 34.0 - dy * side * 20.0;
        let py = cy + dy * 18.0 + dx * side * 12.0;
        c.line(cx + dx * 16.0, cy + 4.0, px, py, 7.0, rgba(RUST_DARK));
        c.line(cx + dx * 16.0, cy + 2.0, px * 0.7 + (cx + dx * 16.0) * 0.3, py * 0.7 + (cy + 2.0) * 0.3, 2.0, rgba(STEEL_LIT));
        // Two-finger crusher.
        c.poly(&[
            (px - 3.0, py - 6.0),
            (px + dx * 20.0 + 2.0, py + dy * 11.0 - 5.0),
            (px + dx * 16.0, py + dy * 9.0),
            (px + 1.0, py - 1.0),
        ], rgba(SCRAP));
        c.poly(&[
            (px - 2.0, py + 5.0),
            (px + dx * 18.0 + 1.0, py + dy * 10.0 + 6.0),
            (px + dx * 14.0, py + dy * 8.0 + 1.0),
            (px, py + 1.0),
        ], rgba([78, 72, 68]));
        c.line(px + dx * 4.0, py, px + dx * 16.0, py + dy * 8.0, 1.8, rgba(team));
    }
    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.28);
    c
}

/// Lodestone: skirted rail-coil artillery crawler. 136x124.
fn paint_lodestone(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(136, 124);
    let (dx, dy) = facing_vec(f);
    let cx = 68.0;
    let cy = 70.0;
    // Heavy armored skirt over the wheels.
    plate(&mut c, &[(16.0, 74.0), (120.0, 74.0), (126.0, 96.0), (10.0, 96.0)], RUST_DARK, 1.0);
    for k in 0..5 {
        c.dome(30.0 + k as f32 * 19.0 + (frame as f32) * 2.0, 96.0, 8.0, 6.5, [40, 38, 40]);
        c.dome(30.0 + k as f32 * 19.0 + (frame as f32) * 2.0, 96.0, 3.0, 2.5, SCRAP);
    }
    // Skirt armor slats.
    for k in 0..6 {
        let x = 20 + k * 18;
        c.rect(x, 78, 2, 14, rgba([70, 52, 40]));
    }
    // Hull: layered glacis.
    plate(&mut c, &[(22.0, 52.0), (114.0, 52.0), (120.0, 76.0), (16.0, 76.0)], RUST, 1.0);
    plate(&mut c, &[(32.0, 42.0), (104.0, 42.0), (110.0, 54.0), (26.0, 54.0)], [122, 88, 60], 1.0);
    rust_wear(&mut c, 22, 44, 90, 30, 351);
    c.rect(28, 70, 80, 5, rgba(team));
    // Turret mount + massive coil cannon.
    let tx = cx + dx * 4.0;
    let ty = cy - 26.0 + dy * 4.0;
    plate(&mut c, &[(tx - 20.0, ty - 8.0), (tx + 16.0, ty - 11.0), (tx + 20.0, ty + 9.0), (tx - 16.0, ty + 12.0)], SCRAP, 1.0);
    c.dome(tx - dx * 6.0, ty - 2.0, 8.0, 6.0, [104, 98, 92]);
    let bx = tx + dx * 56.0;
    let by = ty + dy * 28.0;
    c.line(tx + dx * 10.0, ty + dy * 5.0, bx, by, 7.5, rgba([58, 56, 60]));
    c.line(tx + dx * 12.0, ty + dy * 6.0 - 2.0, bx - dx * 2.0, by - dy * 1.0 - 2.0, 2.0, rgba([96, 94, 98]));
    for k in 1..=3 {
        let fr = k as f32 / 4.0;
        let rx = tx + dx * 10.0 + (bx - tx - dx * 10.0) * fr;
        let ry = ty + dy * 5.0 + (by - ty - dy * 5.0) * fr;
        c.ellipse(rx, ry, 5.5, 4.2, rgba(scale_rgb(COIL, 0.8)));
        c.glow(rx, ry, 7.0, COIL, 0.5);
    }
    // Muzzle brake + charge.
    c.line(bx - dx * 5.0, by - dy * 2.5, bx, by, 10.0, rgba([88, 84, 88]));
    let chg = if frame == 0 { 0.5 } else { 0.9 };
    c.glow(bx, by, 9.0, COIL, chg);
    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.26);
    c
}

/// Kestrel: fat-hulled ducted-fan gunship. 144x116.
fn paint_kestrel(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(144, 116);
    let (dx, dy) = facing_vec(f);
    let (dy_i, px, py) = (dy * 0.6, 72.0, 58.0);
    let (wx, wy) = (-dy_i, dx * 0.6);
    // Thick-shrouded fans on stub wings.
    for side in [-1.0f32, 1.0] {
        let fx = px - dx * 4.0 + wx * 36.0 * side;
        let fy = py - dy_i * 4.0 + wy * 36.0 * side;
        // Stub wing.
        c.poly(&[
            (px + dx * 4.0, py + dy_i * 4.0 - 3.0),
            (fx - wx * side * 6.0, fy - wy * side * 6.0 - 4.0),
            (fx - wx * side * 4.0, fy - wy * side * 4.0 + 4.0),
            (px + dx * 2.0, py + dy_i * 2.0 + 4.0),
        ], rgba(RUST));
        // Fat shroud ring.
        c.ellipse(fx, fy, 17.0, 11.0, rgba([52, 48, 50]));
        c.ellipse(fx, fy, 13.0, 8.0, rgba([26, 25, 28]));
        c.ellipse(fx - 3.0, fy - 3.0, 4.0, 2.5, rgba([84, 80, 84]));
        let a0 = if frame == 0 { 0.4f32 } else { 1.2 };
        for k in 0..3 {
            let a = a0 + k as f32 * 2.09;
            c.line(fx, fy, fx + a.cos() * 12.0, fy + a.sin() * 7.0, 1.6, rgba([120, 118, 122]));
        }
        c.glow(fx, fy, 7.0, COIL, 0.35);
        c.set((fx + wx * 6.0 * side) as i32, (fy + wy * 6.0 * side) as i32, rgba(team));
    }
    // Fat fuselage: layered plates, chin turret, canopy.
    let nose = (px + dx * 30.0, py + dy_i * 30.0);
    let tail = (px - dx * 26.0, py - dy_i * 26.0);
    c.poly(&[
        (nose.0, nose.1 - 6.0),
        (px + dx * 8.0 + wx * 13.0, py + dy_i * 8.0 + wy * 13.0),
        (tail.0 + wx * 9.0, tail.1 + wy * 9.0 - 2.0),
        (tail.0, tail.1 + 6.0),
        (tail.0 - wx * 9.0, tail.1 - wy * 9.0 - 2.0),
        (px + dx * 8.0 - wx * 13.0, py + dy_i * 8.0 - wy * 13.0),
    ], rgba(RUST));
    c.poly(&[
        (nose.0 - dx * 4.0, nose.1 - dy_i * 4.0 - 5.0),
        (px + dx * 6.0 + wx * 8.0, py + dy_i * 6.0 + wy * 8.0 - 3.0),
        (px + dx * 2.0 - wx * 8.0, py + dy_i * 2.0 - wy * 8.0 - 3.0),
    ], rgba([124, 90, 62]));
    rust_wear(&mut c, (px - 22.0) as i32, (py - 12.0) as i32, 44, 22, 361);
    // Tail boom + fin.
    c.line(tail.0, tail.1, tail.0 - dx * 12.0, tail.1 - dy_i * 12.0, 5.0, rgba(RUST_DARK));
    c.poly(&[
        (tail.0 - dx * 9.0, tail.1 - dy_i * 9.0),
        (tail.0 - dx * 17.0, tail.1 - dy_i * 17.0 - 12.0),
        (tail.0 - dx * 18.0, tail.1 - dy_i * 18.0),
    ], rgba(RUST_LIT));
    // Canopy + chin gun.
    c.ellipse(px + dx * 16.0, py + dy_i * 16.0 - 3.0, 7.0, 4.5, rgba([150, 210, 235]));
    c.ellipse(px + dx * 14.0, py + dy_i * 14.0 - 4.5, 2.5, 1.5, rgba([220, 245, 255]));
    c.line(px + dx * 20.0, py + dy_i * 20.0 + 5.0, px + dx * 28.0, py + dy_i * 28.0 + 7.0, 3.0, rgba([50, 48, 52]));
    c.glow(px + dx * 29.0, py + dy_i * 29.0 + 7.0, 5.0, COIL, 0.6);
    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.25);
    c
}

/// Resonant: layered coil-priest on a hover skirt. 108x128.
fn paint_resonant(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(108, 128);
    let (dx, dy) = facing_vec(f);
    let cx = 54.0;
    let cy = 74.0;
    c.glow(cx, cy + 28.0, 18.0, COIL, 0.5);
    // Bell body: three stacked armored tiers.
    plate(&mut c, &[(cx - 24.0, cy + 18.0), (cx + 24.0, cy + 18.0), (cx + 16.0, cy + 28.0), (cx - 16.0, cy + 28.0)], RUST_DARK, 1.0);
    plate(&mut c, &[(cx - 20.0, cy - 4.0), (cx + 20.0, cy - 4.0), (cx + 24.0, cy + 20.0), (cx - 24.0, cy + 20.0)], RUST, 1.0);
    plate(&mut c, &[(cx - 14.0, cy - 22.0), (cx + 14.0, cy - 22.0), (cx + 20.0, cy - 2.0), (cx - 20.0, cy - 2.0)], [122, 88, 60], 1.0);
    rust_wear(&mut c, (cx - 20.0) as i32, (cy - 18.0) as i32, 40, 42, 371);
    // Team sash.
    c.line(cx - 14.0, cy - 16.0, cx + 14.0, cy + 12.0, 4.5, rgba(team));
    // Hooded head with optic.
    c.dome(cx, cy - 32.0, 12.0, 11.0, SCRAP);
    plate(&mut c, &[(cx - 12.0, cy - 34.0), (cx + 12.0, cy - 34.0), (cx + 9.0, cy - 24.0), (cx - 9.0, cy - 24.0)], RUST_LIT, 0.95);
    if dy > -0.5 {
        c.ellipse(cx + dx * 4.0, cy - 30.0, 3.0, 3.0, rgba(COIL));
        c.glow(cx + dx * 4.0, cy - 30.0, 6.0, COIL, 0.7);
    }
    // Coil crown: rings on a mast.
    c.line(cx, cy - 42.0, cx, cy - 58.0, 2.5, rgba(SCRAP));
    for (k, ry) in [(0usize, -46.0f32), (1, -54.0), (2, -61.0)].iter() {
        let rr = 15.0 - *k as f32 * 4.0;
        c.ellipse(cx, cy + ry, rr, rr * 0.4, rgba(scale_rgb(COIL, 0.75)));
        c.glow(cx, cy + ry, rr, COIL, 0.3);
    }
    let a0 = if frame == 0 { 0.5f32 } else { 2.1 };
    for k in 0..3 {
        let a = a0 + k as f32 * 2.1;
        c.line(cx, cy - 54.0, cx + a.cos() * 13.0, cy - 54.0 + a.sin() * 6.0, 1.3, rgba(scale_rgb(COIL, 1.2)));
    }
    c.glow(cx, cy - 54.0, 10.0, COIL, 0.6);
    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.3);
    c
}

/// Bastion: the Ferron foundry-fortress HQ. 400x300.
fn paint_bastion(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(400, 300);
    let cx = 200.0;
    iso_box4(&mut c, cx, 216.0, 192.0, 24.0, [84, 70, 58], None);
    hazard(&mut c, cx - 60.0, 244.0, 15);
    // Main keep: rusted plate walls.
    iso_box4(&mut c, cx, 152.0, 140.0, 68.0, [112, 82, 58], Some(team));
    windows4(&mut c, cx, 152.0, 140.0, 68.0, [255, 180, 90]);
    rust_wear(&mut c, (cx - 120.0) as i32, 160, 240, 56, 91);
    // Smelter tower with a glowing crucible mouth.
    iso_box4(&mut c, cx + 64.0, 96.0, 52.0, 56.0, [96, 72, 54], Some(team));
    c.ellipse(cx + 64.0, 78.0, 26.0, 12.0, rgba([40, 30, 26]));
    c.glow(cx + 64.0, 78.0, 22.0, AMBER, 0.8);
    c.ellipse(cx + 64.0, 78.0, 14.0, 6.0, rgba([255, 170, 70]));
    // Crane gantry: posts, beam, hanging magnet.
    c.line(cx - 150.0, 120.0, cx - 150.0, 190.0, 4.0, rgba([58, 50, 44]));
    c.line(cx - 60.0, 100.0, cx - 60.0, 160.0, 4.0, rgba([58, 50, 44]));
    c.line(cx - 154.0, 120.0, cx - 56.0, 100.0, 4.5, rgba(SCRAP));
    c.line(cx - 105.0, 110.0, cx - 105.0, 138.0, 1.6, rgba([70, 66, 62]));
    c.dome(cx - 105.0, 142.0, 9.0, 5.0, [60, 56, 64]);
    c.glow(cx - 105.0, 145.0, 10.0, COIL, 0.6);
    // Scrap heaps on the apron.
    for k in 0..6 {
        let h = hash2(k, 9, 411);
        let sxp = cx + 90.0 + (h % 70) as f32;
        let syp = 196.0 + ((h >> 8) % 30) as f32;
        c.dome(sxp, syp, 6.0 + (h % 5) as f32, 4.0, SCRAP);
    }
    // Mast + strobe.
    c.line(cx - 20.0, 96.0, cx - 20.0, 44.0, 2.5, rgba([70, 66, 62]));
    c.glow(cx - 20.0, 42.0, 6.0, [255, 90, 80], 0.85);
    c.set((cx - 20.0) as i32, 42, rgba([255, 130, 120]));
    c.outline_t(OUTLINE, 3);
    c.rim(OUTLINE, 1.22);
    c
}

/// Capacitor Mast: lattice tower with charged rings. 272x260.
fn paint_mast(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(272, 260);
    let cx = 136.0;
    iso_box4(&mut c, cx, 184.0, 120.0, 24.0, [84, 70, 58], None);
    iso_box4(&mut c, cx, 172.0, 40.0, 18.0, [96, 74, 56], Some(team));
    // Guy wires.
    c.line(cx - 30.0, 168.0, cx - 92.0, 196.0, 1.8, rgba([48, 44, 42]));
    c.line(cx + 30.0, 168.0, cx + 92.0, 196.0, 1.8, rgba([48, 44, 42]));
    // Lattice tower: two rails + cross bracing.
    for t in 0..9 {
        let f = t as f32 / 9.0;
        let w = 16.0 - f * 10.0;
        let y0 = 168.0 - f * 128.0;
        let y1 = 168.0 - (t as f32 + 1.0) / 9.0 * 128.0;
        let w1 = 16.0 - (t as f32 + 1.0) / 9.0 * 10.0;
        c.line(cx - w, y0, cx - w1, y1, 2.2, rgba(RUST));
        c.line(cx + w, y0, cx + w1, y1, 2.2, rgba(RUST));
        let dir = t % 2 == 0;
        c.line(cx - w, y0, cx + if dir { w1 } else { -w1 }, y1, 1.3, rgba(RUST_DARK));
        c.line(cx + w, y0, cx + if dir { -w1 } else { w1 }, y1, 1.3, rgba(RUST_DARK));
    }
    // Capacitor rings.
    for (ry, rr) in [(120.0f32, 22.0f32), (80.0, 17.0), (48.0, 12.0)] {
        c.ellipse(cx, ry, rr, rr * 0.35, rgba(scale_rgb(COIL, 0.8)));
        c.glow(cx, ry, rr + 4.0, COIL, 0.35);
    }
    c.glow(cx, 34.0, 9.0, COIL, 0.8);
    c.set(cx as i32, 34, rgba(scale_rgb(COIL, 1.3)));
    c.outline_t(OUTLINE, 3);
    c.rim(OUTLINE, 1.22);
    c
}

/// Fume Tap: wellhead + burn-off flare over a geyser. 272x232.
fn paint_fumetap(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(272, 232);
    let cx = 136.0;
    iso_box4(&mut c, cx, 168.0, 120.0, 24.0, [84, 70, 58], None);
    iso_box4(&mut c, cx - 12.0, 132.0, 72.0, 44.0, [104, 78, 56], Some(team));
    rust_wear(&mut c, (cx - 72.0) as i32, 138, 130, 34, 101);
    // Wellhead pipe stack with valve wheels.
    c.rect((cx - 20.0) as i32, 84, 16, 50, rgba(SCRAP));
    c.rect((cx - 22.0) as i32, 96, 20, 6, rgba([70, 66, 62]));
    for wy in [92.0f32, 112.0] {
        c.ellipse(cx - 12.0, wy, 9.0, 3.5, rgba(RUST_LIT));
    }
    // Flare stack with burning tip.
    c.line(cx + 52.0, 128.0, cx + 60.0, 62.0, 4.0, rgba(SCRAP));
    c.glow(cx + 61.0, 54.0, 13.0, AMBER, 0.9);
    c.ellipse(cx + 61.0, 56.0, 5.0, 8.0, rgba([255, 190, 90]));
    c.ellipse(cx + 61.0, 60.0, 3.0, 4.0, rgba([255, 240, 180]));
    // Teal fume glow at the tap seam (it sits on a geyser).
    c.glow(cx - 12.0, 150.0, 20.0, [64, 210, 200], 0.4);
    pipe(&mut c, cx + 12.0, 120.0, cx + 50.0, 130.0, 6.0);
    c.outline_t(OUTLINE, 3);
    c.rim(OUTLINE, 1.22);
    c
}

/// Assembly Line: open-bay production shed. 400x280.
fn paint_assembly(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(400, 280);
    let cx = 200.0;
    iso_box4(&mut c, cx, 200.0, 192.0, 24.0, [84, 70, 58], None);
    iso_box4(&mut c, cx, 140.0, 152.0, 64.0, [108, 80, 58], Some(team));
    windows4(&mut c, cx, 140.0, 152.0, 64.0, [255, 180, 90]);
    rust_wear(&mut c, (cx - 130.0) as i32, 148, 260, 52, 121);
    // Open bay: glowing interior with a half-built walker frame.
    let bx = cx - 60.0;
    let by = 186.0;
    c.poly(&[(bx - 26.0, by + 12.0), (bx - 22.0, by - 22.0), (bx + 22.0, by - 26.0), (bx + 26.0, by + 8.0)], rgba([30, 24, 22]));
    c.glow(bx, by - 6.0, 26.0, AMBER, 0.5);
    c.line(bx - 8.0, by + 4.0, bx - 4.0, by - 14.0, 2.0, rgba([70, 62, 56]));
    c.line(bx + 8.0, by + 2.0, bx + 4.0, by - 14.0, 2.0, rgba([70, 62, 56]));
    c.line(bx - 10.0, by - 12.0, bx + 10.0, by - 12.0, 2.0, rgba(SCRAP));
    hazard(&mut c, bx - 28.0, by + 14.0, 7);
    // Roof conveyor rail with buckets.
    c.line(cx - 120.0, 112.0, cx + 120.0, 100.0, 3.0, rgba(SCRAP));
    for k in 0..5 {
        let f = k as f32 / 4.0;
        let rx = cx - 110.0 + 220.0 * f;
        let ry = 112.0 - 12.0 * f + 5.0;
        c.rect(rx as i32 - 4, ry as i32, 8, 7, rgba(RUST_DARK));
    }
    c.outline_t(OUTLINE, 3);
    c.rim(OUTLINE, 1.22);
    c
}

/// Refit Bay: crane hall with a hanging chassis. 400x296.
fn paint_refit(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(400, 296);
    let cx = 200.0;
    iso_box4(&mut c, cx, 200.0, 192.0, 24.0, [84, 70, 58], None);
    iso_box4(&mut c, cx - 40.0, 132.0, 120.0, 76.0, [110, 82, 58], Some(team));
    windows4(&mut c, cx - 40.0, 132.0, 120.0, 76.0, [255, 180, 90]);
    rust_wear(&mut c, (cx - 140.0) as i32, 140, 200, 62, 141);
    // Yard gantry: posts + beam + chain + chassis.
    let gx = cx + 108.0;
    c.line(gx - 44.0, 130.0, gx - 44.0, 196.0, 4.5, rgba([58, 50, 44]));
    c.line(gx + 44.0, 142.0, gx + 44.0, 208.0, 4.5, rgba([58, 50, 44]));
    c.line(gx - 48.0, 130.0, gx + 48.0, 142.0, 5.0, rgba(SCRAP));
    c.line(gx, 138.0, gx, 164.0, 1.6, rgba([70, 66, 62]));
    plate(&mut c, &[(gx - 22.0, 164.0), (gx + 22.0, 168.0), (gx + 18.0, 184.0), (gx - 18.0, 180.0)], RUST_DARK, 1.0);
    // Welding sparks.
    c.glow(gx - 10.0, 176.0, 7.0, COIL, 0.9);
    c.set(gx as i32 - 10, 176, rgba([240, 240, 255]));
    // Chimney.
    iso_box4(&mut c, cx - 130.0, 76.0, 24.0, 48.0, [90, 68, 52], None);
    c.glow(cx - 130.0, 66.0, 10.0, AMBER, 0.5);
    c.outline_t(OUTLINE, 3);
    c.rim(OUTLINE, 1.22);
    c
}

/// Skydock: elevated flight pad on trusses. 400x280.
fn paint_skydock(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(400, 280);
    let cx = 200.0;
    iso_box4(&mut c, cx, 192.0, 192.0, 24.0, [84, 70, 58], None);
    // Truss legs under the raised pad.
    for (lx, ly) in [(-72.0f32, 0.0f32), (72.0, 0.0), (-24.0, 26.0), (110.0, 26.0)] {
        c.line(cx + 24.0 + lx, 150.0, cx + 24.0 + lx + 8.0, 196.0 + ly * 0.3, 4.5, rgba([58, 50, 44]));
        c.line(cx + 24.0 + lx, 160.0, cx + 24.0 + lx - 14.0, 190.0 + ly * 0.3, 2.0, rgba(RUST_DARK));
    }
    // Raised pad.
    iso_box4(&mut c, cx + 24.0, 140.0, 128.0, 24.0, [96, 78, 62], None);
    c.ellipse(cx + 24.0, 140.0, 48.0, 24.0, rgba([112, 92, 72]));
    c.ellipse(cx + 24.0, 140.0, 36.0, 18.0, rgba([96, 78, 62]));
    for k in 0..8 {
        let a = k as f32 * 0.785;
        let lx = cx + 24.0 + a.cos() * 42.0;
        let ly = 140.0 + a.sin() * 21.0;
        c.set(lx as i32, ly as i32, rgba(scale_rgb(COIL, 1.2)));
        c.glow(lx, ly, 4.5, COIL, 0.55);
    }
    hazard(&mut c, cx - 72.0, 172.0, 10);
    // Control shack + beacon.
    iso_box4(&mut c, cx - 128.0, 120.0, 44.0, 52.0, [104, 80, 58], Some(team));
    c.rect((cx - 148.0) as i32, 126, 40, 8, rgba([150, 210, 235]));
    c.line(cx - 128.0, 94.0, cx - 128.0, 58.0, 2.2, rgba([70, 66, 62]));
    c.glow(cx - 128.0, 56.0, 6.0, [255, 90, 80], 0.85);
    c.outline_t(OUTLINE, 3);
    c.rim(OUTLINE, 1.22);
    c
}

/// Relay: coil spire + listening dish. 264x248.
fn paint_relay(team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(264, 248);
    let cx = 132.0;
    iso_box4(&mut c, cx, 184.0, 120.0, 24.0, [84, 70, 58], None);
    iso_box4(&mut c, cx, 156.0, 64.0, 28.0, [104, 78, 56], Some(team));
    // Coil spire: shrinking rusted discs with charged gaps.
    for (k, (ry, rr)) in [(0usize, (128.0f32, 34.0f32)), (1, (104.0, 27.0)), (2, (82.0, 20.0)), (3, (62.0, 13.0))].iter() {
        let _ = k;
        c.dome(cx, *ry, *rr, rr * 0.45, RUST);
        c.glow(cx, ry - 8.0, rr * 0.8, COIL, 0.3);
    }
    c.line(cx - 4.0, 54.0, cx + 2.0, 34.0, 2.0, rgba(SCRAP));
    c.glow(cx, 30.0, 10.0, COIL, 0.8);
    // Arcs off the tip.
    c.line(cx, 34.0, cx - 12.0, 22.0, 1.3, rgba(scale_rgb(COIL, 1.2)));
    c.line(cx, 34.0, cx + 10.0, 20.0, 1.3, rgba(scale_rgb(COIL, 1.2)));
    // Listening dish on a strut.
    c.line(cx + 52.0, 140.0, cx + 72.0, 116.0, 3.0, rgba([58, 50, 44]));
    c.dome(cx + 78.0, 110.0, 18.0, 11.0, SCRAP);
    c.ellipse(cx + 78.0, 108.0, 10.0, 6.0, rgba([120, 116, 112]));
    c.set((cx + 78.0) as i32, 106, rgba(scale_rgb(COIL, 1.3)));
    c.outline_t(OUTLINE, 3);
    c.rim(OUTLINE, 1.22);
    c
}

// ----------------------------------------------------------- hero units ----

/// Marshal Kade: caped VC commander with twin banners. 120x128.
fn paint_marshal(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(120, 128);
    let (dx, dy) = facing_vec(f);
    let cx = 60.0;
    let cy = 70.0;
    let lift = if frame == 0 { 0.0 } else { 3.0 };
    // Cape flowing opposite the facing.
    c.poly(&[
        (cx - 14.0, cy - 22.0),
        (cx + 14.0, cy - 22.0),
        (cx + 20.0 - dx * 10.0, cy + 34.0),
        (cx - 20.0 - dx * 10.0, cy + 34.0),
    ], rgba([30, 36, 58]));
    c.line(cx - 12.0 - dx * 8.0, cy + 30.0, cx + 16.0 - dx * 10.0, cy + 32.0, 2.0, rgba([48, 56, 86]));
    // Banner poles with team pennants.
    for side in [-1.0f32, 1.0] {
        let px = cx + side * 16.0;
        c.line(px, cy - 20.0, px + side * 4.0, cy - 58.0, 2.2, rgba([90, 96, 108]));
        c.poly(&[
            (px + side * 4.0, cy - 58.0),
            (px + side * 18.0, cy - 52.0),
            (px + side * 4.0, cy - 46.0),
        ], rgba(team));
        c.glow(px + side * 10.0, cy - 52.0, 6.0, team, 0.4);
    }
    // Legs: heavy greaves.
    plate(&mut c, &[(cx - 16.0, cy + 22.0 + lift), (cx - 4.0, cy + 22.0 + lift), (cx - 6.0, cy + 46.0 - lift * 0.5), (cx - 16.0, cy + 46.0 - lift * 0.5)], GUNMETAL_DARK, 1.0);
    plate(&mut c, &[(cx + 4.0, cy + 24.0 - lift), (cx + 16.0, cy + 24.0 - lift), (cx + 16.0, cy + 48.0 + lift * 0.3), (cx + 6.0, cy + 48.0 + lift * 0.3)], GUNMETAL_DARK, 0.9);
    c.rect((cx - 18.0) as i32, (cy + 44.0) as i32, 16, 8, rgba([30, 32, 38]));
    c.rect((cx + 4.0) as i32, (cy + 46.0) as i32, 16, 8, rgba([28, 30, 36]));
    // Torso: gilded command armor.
    plate(&mut c, &[(cx - 22.0, cy - 20.0), (cx + 22.0, cy - 20.0), (cx + 18.0, cy + 24.0), (cx - 18.0, cy + 24.0)], GUNMETAL, 1.05);
    c.poly(&[(cx - 16.0 + dx * 4.0, cy - 16.0), (cx + dx * 8.0, cy - 22.0 + dy * 3.0), (cx + dx * 8.0, cy + 8.0), (cx - 12.0 + dx * 4.0, cy + 14.0)], rgba(scale_rgb(GUNMETAL, 1.2)));
    // Gold trim + core.
    c.line(cx - 18.0, cy - 18.0, cx + 18.0, cy - 18.0, 2.0, rgba([201, 162, 39]));
    c.glow(cx + dx * 6.0, cy - 2.0, 9.0, team, 0.8);
    c.rect((cx + dx * 6.0 - 3.0) as i32, (cy - 8.0) as i32, 6, 12, rgba(scale_rgb(team, 1.3)));
    // Massive pauldrons with gold edge.
    plate(&mut c, &[(cx - 36.0, cy - 28.0), (cx - 10.0, cy - 32.0), (cx - 8.0, cy - 16.0), (cx - 32.0, cy - 12.0)], STEEL_LIT, 0.98);
    plate(&mut c, &[(cx + 10.0, cy - 32.0), (cx + 36.0, cy - 28.0), (cx + 32.0, cy - 12.0), (cx + 8.0, cy - 16.0)], GUNMETAL, 0.9);
    c.line(cx - 34.0, cy - 26.0, cx - 12.0, cy - 30.0, 2.5, rgba([201, 162, 39]));
    c.line(cx + 12.0, cy - 30.0, cx + 34.0, cy - 26.0, 2.5, rgba([201, 162, 39]));
    // Commander helm with crest.
    plate(&mut c, &[(cx - 8.0, cy - 44.0), (cx + 8.0, cy - 44.0), (cx + 10.0, cy - 30.0), (cx - 10.0, cy - 30.0)], scale_rgb(GUNMETAL, 1.15), 1.0);
    c.poly(&[(cx - 2.0, cy - 58.0), (cx + 2.0, cy - 58.0), (cx + 4.0, cy - 42.0), (cx - 4.0, cy - 42.0)], rgba([201, 162, 39]));
    if dy > -0.5 {
        let vx = cx + dx * 4.0;
        c.line(vx - 5.0, cy - 36.0, vx + 5.0, cy - 36.0, 2.4, rgba(VISOR));
        c.glow(vx, cy - 36.0, 7.0, VISOR, 0.7);
    }
    // Heavy gun arm.
    let gx = cx + dx * 30.0;
    let gy = cy + dy * 12.0 - 2.0;
    c.line(cx + dx * 14.0, cy - 4.0, gx, gy, 6.5, rgba(GUNMETAL_DARK));
    c.line(gx - dx * 4.0, gy - dy * 2.0, gx, gy, 8.0, rgba(STEEL_LIT));
    c.glow(gx, gy, 5.0, team, 0.6);
    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.3);
    c
}

/// Broodmother Sszrak: crowned brood queen. 144x128.
fn paint_broodmother(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(144, 128);
    let (dx, dy) = facing_vec(f);
    let cx = 72.0;
    let cy = 72.0;
    let step = if frame == 0 { 4.0 } else { -4.0 };
    // Eight legs.
    for k in 0..4 {
        let off = -27.0 + k as f32 * 18.0;
        let l = if k % 2 == 0 { step } else { -step };
        kleg(&mut c, cx + off, cy + 6.0, cx + off - 12.0, cy + 22.0 - l * 0.3, cx + off - 15.0 + l, cy + 42.0, 4.0);
        kleg(&mut c, cx + off, cy + 6.0, cx + off + 12.0, cy + 24.0 + l * 0.3, cx + off + 15.0 - l, cy + 44.0, 4.0);
    }
    // Swollen egg abdomen behind: membrane with glowing eggs showing.
    c.dome(cx - dx * 22.0, cy + 2.0, 32.0, 24.0, MEMBRANE);
    for k in 0..5 {
        let h = hash2(k, f as i32, 611);
        let ex = cx - dx * 22.0 - 20.0 + (h % 40) as f32;
        let ey = cy - 8.0 + ((h >> 8) % 22) as f32;
        c.dome(ex, ey, 5.0, 6.0, scale_rgb(MEMBRANE, 1.15));
        c.glow(ex, ey + 2.0, 5.0, KYTH_GLOW, 0.5);
    }
    // Main carapace, tiered + scalloped.
    c.dome(cx + dx * 4.0, cy - 8.0, 30.0, 22.0, CHITIN);
    scallop(&mut c, cx + dx * 4.0, cy - 4.0, 26.0, 32.0);
    c.dome(cx + dx * 8.0, cy - 20.0, 20.0, 13.0, CHITIN_LIGHT);
    scallop(&mut c, cx + dx * 8.0, cy - 17.0, 17.0, 22.0);
    // Spine crown: tall team-tinted rack.
    for k in 0..5 {
        let sx = cx - 16.0 + k as f32 * 8.0;
        let h = 14.0 + ((k as i32 - 2).abs() as f32) * -3.0 + 10.0;
        c.poly(&[(sx - 2.5, cy - 30.0), (sx, cy - 30.0 - h), (sx + 2.5, cy - 30.0)], rgba(team));
        c.glow(sx, cy - 32.0 - h, 4.0, team, 0.4);
    }
    // Head with crowned plates + eye cluster.
    c.dome(cx + dx * 22.0, cy - 6.0 + dy * 8.0, 12.0, 9.0, CHITIN_LIGHT);
    if dy > -0.5 {
        let ex = cx + dx * 26.0;
        let ey = cy - 8.0 + dy * 9.0;
        for (ox, oy) in [(-3.0f32, 0.0f32), (0.0, -2.0), (3.0, 0.0)] {
            c.set((ex + ox) as i32, (ey + oy) as i32, rgba(KYTH_GLOW));
        }
        c.glow(ex, ey, 7.0, KYTH_GLOW, 0.7);
    }
    // Scythe fore-claws.
    for side in [-1.0f32, 1.0] {
        let px = cx + dx * 30.0 - dy * side * 16.0;
        let py = cy + dy * 16.0 + dx * side * 10.0;
        c.poly(&[
            (px - 3.0, py - 5.0),
            (px + dx * 20.0 + 2.0, py + dy * 12.0 - 4.0),
            (px + dx * 16.0, py + dy * 10.0 + 3.0),
            (px, py + 2.0),
        ], rgba(CHITIN_LIGHT));
        c.line(px + dx * 4.0, py, px + dx * 18.0, py + dy * 10.0, 1.8, rgba(team));
    }
    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.3);
    c
}

/// Magnus Vex: coil titan with a magnet hammer. 132x136.
fn paint_magnus(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(132, 136);
    let (dx, dy) = facing_vec(f);
    let cx = 66.0;
    let cy = 76.0;
    let lift = if frame == 0 { 0.0 } else { 3.0 };
    // Massive legs.
    fer_leg(&mut c, cx - 12.0, cy + 12.0, cx - 20.0, cy + 30.0, cx - 14.0 + lift, cy + 52.0, 6.0);
    fer_leg(&mut c, cx + 12.0, cy + 12.0, cx + 20.0, cy + 30.0, cx + 14.0 - lift, cy + 52.0, 6.0);
    // Torso: tiered rusted plates with a gold-violet core.
    plate(&mut c, &[(cx - 26.0, cy - 18.0), (cx + 26.0, cy - 18.0), (cx + 22.0, cy + 16.0), (cx - 22.0, cy + 16.0)], RUST, 1.0);
    plate(&mut c, &[(cx - 20.0, cy - 34.0), (cx + 20.0, cy - 34.0), (cx + 24.0, cy - 16.0), (cx - 24.0, cy - 16.0)], [124, 90, 62], 1.0);
    rust_wear(&mut c, (cx - 22.0) as i32, (cy - 30.0) as i32, 44, 42, 881);
    c.glow(cx + dx * 6.0, cy - 6.0, 10.0, COIL, 0.85);
    c.rect((cx + dx * 6.0 - 4.0) as i32, (cy - 12.0) as i32, 8, 12, rgba(scale_rgb(COIL, 1.3)));
    // Shoulder coil stacks: rings that pulse.
    for side in [-1.0f32, 1.0] {
        let px = cx + side * 30.0;
        plate(&mut c, &[(px - 8.0, cy - 40.0), (px + 8.0, cy - 40.0), (px + 10.0, cy - 16.0), (px - 10.0, cy - 16.0)], SCRAP, 1.0);
        for k in 0..3 {
            let ry = cy - 38.0 + k as f32 * 7.0;
            c.ellipse(px, ry, 9.0, 3.0, rgba(scale_rgb(COIL, 0.8)));
        }
        let pu = if frame == 0 { 0.4 } else { 0.7 };
        c.glow(px, cy - 28.0, 12.0, COIL, pu);
    }
    // Helm: sealed mask with a coil halo.
    plate(&mut c, &[(cx - 9.0, cy - 52.0), (cx + 9.0, cy - 52.0), (cx + 11.0, cy - 36.0), (cx - 11.0, cy - 36.0)], SCRAP, 1.05);
    c.ellipse(cx, cy - 56.0, 14.0, 4.5, rgba(scale_rgb(COIL, 0.75)));
    c.glow(cx, cy - 56.0, 14.0, COIL, 0.5);
    if dy > -0.5 {
        c.line(cx + dx * 4.0 - 4.0, cy - 44.0, cx + dx * 4.0 + 4.0, cy - 44.0, 2.2, rgba(COIL));
    }
    // Magnet hammer: thick haft + massive disc head.
    let hx = cx + dx * 34.0;
    let hy = cy + dy * 14.0 + 2.0;
    c.line(cx + dx * 14.0, cy - 2.0, hx, hy, 5.5, rgba([58, 50, 44]));
    c.dome(hx + dx * 4.0, hy + dy * 2.0, 12.0, 8.0, [64, 60, 68]);
    c.ellipse(hx + dx * 4.0, hy + dy * 2.0, 7.0, 4.0, rgba(scale_rgb(COIL, 0.9)));
    c.glow(hx + dx * 4.0, hy + dy * 2.0, 11.0, COIL, 0.7);
    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.28);
    c
}

/// Broodling: disposable summoned swarmling. 64x56.
fn paint_broodling(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(64, 56);
    let (dx, dy) = facing_vec(f);
    let cx = 32.0;
    let cy = 30.0;
    let step = if frame == 0 { 3.0 } else { -3.0 };
    kleg(&mut c, cx - 4.0, cy + 2.0, cx - 12.0 + step, cy + 8.0, cx - 15.0 + step, cy + 18.0, 2.0);
    kleg(&mut c, cx + 4.0, cy + 2.0, cx + 12.0 - step, cy + 8.0, cx + 15.0 - step, cy + 18.0, 2.0);
    c.poly(&[
        (cx - dx * 12.0 - dy * 5.0, cy - dy * 6.0 + dx * 3.0),
        (cx + dx * 10.0, cy + dy * 5.0 - 5.0),
        (cx + dx * 14.0, cy + dy * 7.0),
        (cx + dx * 10.0, cy + dy * 5.0 + 4.0),
        (cx - dx * 12.0 + dy * 5.0, cy - dy * 6.0 - dx * 3.0),
    ], rgba(CHITIN));
    c.line(cx + dx * 10.0, cy + dy * 5.0, cx + dx * 16.0, cy + dy * 8.0 - 1.0, 1.4, rgba(team));
    if dy > -0.5 {
        c.set((cx + dx * 8.0) as i32, (cy + dy * 4.0 - 2.0) as i32, rgba(KYTH_GLOW));
        c.glow(cx + dx * 8.0, cy + dy * 4.0 - 2.0, 4.0, KYTH_GLOW, 0.6);
    }
    // Decay shimmer: they're temporary and look it.
    c.glow(cx, cy, 10.0, KYTH_GLOW, 0.25);
    c.outline_t(OUTLINE, 2);
    c.rim(OUTLINE, 1.25);
    c
}

// ------------------------------------------------------------- portraits ----
//
// Console bust portraits: head-and-shoulders closeups on a dark scanlined
// backdrop with a team glow. 176x176, fit-to-box by the HUD.

fn portrait_backdrop(c: &mut Canvas, team: [u8; 3]) {
    for y in 0..176 {
        for x in 0..176 {
            let f = 0.5 + 0.5 * (y as f32 / 176.0);
            let mut px = scale_rgb([18, 22, 32], f);
            if y % 4 == 0 {
                px = scale_rgb(px, 0.86);
            }
            let h = hash2(x, y, 71);
            if h % 97 == 0 {
                px = scale_rgb(px, 1.3);
            }
            c.set(x, y, rgba(px));
        }
    }
    // Team halo behind the head.
    c.glow(88.0, 74.0, 70.0, team, 0.25);
    // Vignette corners.
    for y in 0..176 {
        for x in 0..176 {
            let dx = (x as f32 - 88.0) / 88.0;
            let dy = (y as f32 - 88.0) / 88.0;
            let d = (dx * dx + dy * dy).sqrt();
            if d > 0.85 {
                let a = (((d - 0.85) / 0.35) * 160.0).min(160.0) as u8;
                c.blend(x, y, [6, 8, 12, a]);
            }
        }
    }
}

/// Armored VC shoulders across the frame bottom.
fn vc_shoulders(c: &mut Canvas, team: [u8; 3]) {
    plate(c, &[(8.0, 156.0), (54.0, 122.0), (122.0, 122.0), (168.0, 156.0), (168.0, 176.0), (8.0, 176.0)], GUNMETAL, 0.92);
    plate(c, &[(4.0, 130.0), (52.0, 118.0), (58.0, 142.0), (10.0, 156.0)], STEEL_LIT, 0.9);
    plate(c, &[(124.0, 118.0), (172.0, 130.0), (166.0, 156.0), (118.0, 142.0)], GUNMETAL, 0.82);
    c.line(14.0, 138.0, 48.0, 128.0, 3.5, rgba(team));
    c.line(128.0, 128.0, 162.0, 138.0, 3.5, rgba(team));
    // Chest power light.
    c.glow(88.0, 152.0, 10.0, team, 0.7);
    c.rect(85, 146, 6, 12, rgba(scale_rgb(team, 1.25)));
}

/// Rusted Ferron shoulders with a coil chest light.
fn fer_shoulders(c: &mut Canvas, team: [u8; 3]) {
    plate(c, &[(8.0, 158.0), (52.0, 124.0), (124.0, 124.0), (168.0, 158.0), (168.0, 176.0), (8.0, 176.0)], RUST, 0.95);
    plate(c, &[(4.0, 132.0), (50.0, 120.0), (56.0, 144.0), (10.0, 158.0)], RUST_LIT, 0.9);
    plate(c, &[(126.0, 120.0), (172.0, 132.0), (166.0, 158.0), (120.0, 144.0)], RUST_DARK, 1.0);
    c.line(14.0, 140.0, 48.0, 130.0, 3.5, rgba(team));
    c.line(128.0, 130.0, 162.0, 140.0, 3.5, rgba(team));
    c.glow(88.0, 152.0, 10.0, COIL, 0.7);
    c.rect(85, 146, 6, 12, rgba(scale_rgb(COIL, 1.2)));
    rust_wear(c, 10, 126, 156, 40, 161);
}

/// VC helmet dome + jaw guard; returns visor band y.
fn vc_helm(c: &mut Canvas, mat: [u8; 3]) -> f32 {
    c.dome(88.0, 66.0, 40.0, 44.0, mat);
    plate(c, &[(56.0, 92.0), (120.0, 92.0), (112.0, 118.0), (64.0, 118.0)], scale_rgb(mat, 0.85), 1.0);
    c.poly(&[(56.0, 60.0), (88.0, 46.0), (120.0, 60.0), (114.0, 50.0), (88.0, 40.0), (62.0, 50.0)], rgba(scale_rgb(mat, 1.2)));
    70.0
}

fn vc_visor(c: &mut Canvas, y: f32, color: [u8; 3], w: f32) {
    c.poly(&[(88.0 - w, y - 5.0), (88.0 + w, y - 5.0), (88.0 + w - 4.0, y + 6.0), (88.0 - w + 4.0, y + 6.0)], rgba(color));
    c.line(88.0 - w + 6.0, y - 2.0, 88.0 + w - 8.0, y - 2.0, 1.6, rgba(scale_rgb(color, 1.4)));
    c.glow(88.0, y, w * 0.9, color, 0.5);
}

/// Kyth chitin head: plated wedge with scallop crown.
fn kyth_head(c: &mut Canvas, wide: f32, tall: f32) {
    c.dome(88.0, 76.0, wide, tall, CHITIN);
    c.dome(88.0, 58.0, wide * 0.7, tall * 0.5, CHITIN_LIGHT);
    scallop(c, 88.0, 62.0, wide * 0.62, wide * 0.8);
    // Neck plates into the frame bottom.
    c.dome(88.0, 150.0, wide * 1.2, 34.0, scale_rgb(CHITIN, 0.85));
    scallop(c, 88.0, 140.0, wide * 0.9, wide);
}

fn kyth_eyes(c: &mut Canvas, n: i32, y: f32, spread: f32) {
    for k in 0..n {
        let x = 88.0 - spread * 0.5 + spread * k as f32 / (n - 1).max(1) as f32;
        let yy = y + ((k * 7) % 3) as f32 * 2.0;
        c.ellipse(x, yy, 3.5, 4.5, rgba(KYTH_GLOW));
        c.set(x as i32, (yy - 1.0) as i32, rgba([230, 255, 200]));
        c.glow(x, yy, 7.0, KYTH_GLOW, 0.5);
    }
}

fn paint_portrait(unit_type: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(176, 176);
    portrait_backdrop(&mut c, team);
    match unit_type {
        // Fabricator: hard-hat rig with goggles and a breather.
        0 => {
            vc_shoulders(&mut c, team);
            let y = vc_helm(&mut c, [150, 124, 58]);
            c.poly(&[(50.0, 42.0), (88.0, 28.0), (126.0, 42.0), (126.0, 52.0), (50.0, 52.0)], rgba([176, 146, 66]));
            for gx in [-16.0f32, 16.0] {
                c.ellipse(88.0 + gx, y, 11.0, 9.0, rgba([40, 44, 52]));
                c.ellipse(88.0 + gx, y, 7.0, 6.0, rgba([120, 220, 240]));
                c.set((86.0 + gx) as i32, (y - 2.0) as i32, rgba([220, 250, 255]));
            }
            c.rect(78, 96, 20, 12, rgba([70, 74, 84]));
            for k in 0..3 {
                c.rect(80 + k * 6, 98, 3, 8, rgba([44, 47, 55]));
            }
        }
        // Trooper: classic visor slit.
        1 => {
            vc_shoulders(&mut c, team);
            let y = vc_helm(&mut c, GUNMETAL);
            vc_visor(&mut c, y, VISOR, 28.0);
            c.rect(72, 100, 32, 6, rgba([50, 53, 62]));
        }
        // Vanguard: crest fin, burning orange visor.
        2 => {
            vc_shoulders(&mut c, team);
            let y = vc_helm(&mut c, scale_rgb(GUNMETAL, 1.05));
            c.poly(&[(84.0, 40.0), (92.0, 40.0), (96.0, 6.0), (80.0, 6.0)], rgba(team));
            c.glow(88.0, 10.0, 10.0, team, 0.5);
            vc_visor(&mut c, y, [255, 150, 70], 24.0);
        }
        // Breaker: commander with headset and raised goggles.
        3 => {
            vc_shoulders(&mut c, team);
            let y = vc_helm(&mut c, [88, 92, 86]);
            // Slim goggle band parked on the brow.
            c.rect(64, 46, 48, 5, rgba([40, 42, 40]));
            c.rect(70, 47, 10, 3, rgba([130, 140, 150]));
            c.rect(96, 47, 10, 3, rgba([130, 140, 150]));
            vc_visor(&mut c, y + 4.0, [150, 200, 230], 24.0);
            c.dome(126.0, 78.0, 8.0, 10.0, [60, 64, 72]);
            c.line(126.0, 88.0, 112.0, 102.0, 3.0, rgba([60, 64, 72]));
        }
        // Skywing: full canopy visor reflecting sky.
        4 => {
            vc_shoulders(&mut c, team);
            let _ = vc_helm(&mut c, [96, 100, 110]);
            c.dome(88.0, 68.0, 30.0, 32.0, [70, 160, 200]);
            c.ellipse(78.0, 56.0, 12.0, 8.0, rgba([180, 235, 255]));
            c.line(60.0, 80.0, 116.0, 80.0, 2.0, rgba([40, 90, 120]));
            c.rect(70, 100, 36, 8, rgba([56, 60, 70]));
        }
        // Stormcaller: shadowed hood, glowing eyes, orb glint.
        5 => {
            let robe = [46, 48, 68];
            plate(&mut c, &[(20.0, 176.0), (44.0, 116.0), (132.0, 116.0), (156.0, 176.0)], robe, 1.0);
            // Peaked hood with a hanging cowl edge.
            c.poly(&[(60.0, 118.0), (74.0, 44.0), (88.0, 26.0), (102.0, 44.0), (116.0, 118.0), (102.0, 126.0), (74.0, 126.0)], rgba(scale_rgb(robe, 1.25)));
            c.line(74.0, 44.0, 62.0, 116.0, 2.5, rgba(scale_rgb(robe, 1.5)));
            // Face void with burning eyes.
            c.poly(&[(70.0, 112.0), (80.0, 56.0), (96.0, 56.0), (106.0, 112.0), (94.0, 120.0), (82.0, 120.0)], rgba([10, 10, 18]));
            c.ellipse(81.0, 86.0, 2.5, 3.0, rgba([170, 245, 255]));
            c.ellipse(95.0, 86.0, 2.5, 3.0, rgba([170, 245, 255]));
            c.glow(88.0, 86.0, 18.0, VISOR, 0.75);
            // Storm orb held at the shoulder.
            c.glow(138.0, 138.0, 20.0, [64, 210, 230], 0.8);
            c.ellipse(138.0, 138.0, 9.0, 9.0, rgba([64, 210, 230]));
            c.ellipse(135.0, 135.0, 3.5, 3.5, rgba([230, 255, 255]));
        }
        // Weaver (12) handled below with Kyth.
        // Bulwark: tech-priest helm with emitter antennae.
        13 | 14 => {
            vc_shoulders(&mut c, team);
            let y = vc_helm(&mut c, [92, 98, 108]);
            for ax in [-26.0f32, 26.0] {
                c.line(88.0 + ax, 52.0, 88.0 + ax * 1.3, 20.0, 2.5, rgba(STEEL_LIT));
                c.set((88.0 + ax * 1.3) as i32, 19, rgba(scale_rgb(team, 1.3)));
                c.glow(88.0 + ax * 1.3, 18.0, 6.0, team, 0.7);
            }
            vc_visor(&mut c, y, scale_rgb(team, 1.1), 20.0);
        }
        // Kyth drone: friendly round eye cluster.
        7 => {
            kyth_head(&mut c, 42.0, 40.0);
            kyth_eyes(&mut c, 4, 74.0, 40.0);
            c.line(66.0, 100.0, 78.0, 112.0, 3.0, LEG);
            c.line(110.0, 100.0, 98.0, 112.0, 3.0, LEG);
        }
        // Skitter: narrow head, big mandible blades.
        8 => {
            kyth_head(&mut c, 34.0, 42.0);
            kyth_eyes(&mut c, 2, 70.0, 26.0);
            for side in [-1.0f32, 1.0] {
                let x0 = 88.0 + side * 26.0;
                c.poly(&[(x0, 92.0), (x0 + side * 34.0, 120.0), (x0 + side * 18.0, 128.0), (x0 - side * 4.0, 102.0)], rgba(CHITIN_LIGHT));
                c.line(x0 + side * 6.0, 98.0, x0 + side * 30.0, 120.0, 1.8, rgba(team));
            }
        }
        // Spitter: tube snout with acid drip.
        9 => {
            kyth_head(&mut c, 40.0, 36.0);
            kyth_eyes(&mut c, 2, 62.0, 34.0);
            c.line(88.0, 84.0, 88.0, 116.0, 14.0, rgba(CHITIN_LIGHT));
            c.dome(88.0, 120.0, 11.0, 8.0, [46, 36, 54]);
            c.glow(88.0, 122.0, 12.0, KYTH_GLOW, 0.8);
            c.ellipse(88.0, 122.0, 5.0, 4.0, rgba(scale_rgb(KYTH_GLOW, 1.2)));
            c.set(88, 132, rgba(KYTH_GLOW));
        }
        // Ravager: crowned plates, tusked jaw.
        10 => {
            kyth_head(&mut c, 46.0, 42.0);
            for k in 0..3 {
                let x = 64.0 + k as f32 * 24.0;
                c.poly(&[(x - 5.0, 42.0), (x, 16.0 + (k % 2) as f32 * 8.0), (x + 5.0, 42.0)], rgba(team));
                c.glow(x, 20.0, 6.0, team, 0.4);
            }
            kyth_eyes(&mut c, 4, 72.0, 44.0);
            for side in [-1.0f32, 1.0] {
                c.poly(&[(88.0 + side * 30.0, 104.0), (88.0 + side * 44.0, 126.0), (88.0 + side * 22.0, 112.0)], rgba(CHITIN_LIGHT));
            }
        }
        // Wisp: translucent bell, inner glow.
        11 => {
            c.dome(88.0, 78.0, 46.0, 42.0, MEMBRANE);
            c.ellipse(88.0, 108.0, 50.0, 14.0, [142, 88, 70, 130]);
            c.glow(88.0, 82.0, 38.0, KYTH_GLOW, 0.75);
            c.ellipse(84.0, 82.0, 12.0, 13.0, rgba(scale_rgb(KYTH_GLOW, 1.15)));
            c.ellipse(81.0, 78.0, 5.0, 5.5, rgba([235, 255, 210]));
            c.dome(80.0, 52.0, 20.0, 12.0, CHITIN_LIGHT);
            for k in 0..4 {
                let x = 62.0 + k as f32 * 17.0;
                c.line(x, 116.0, x - 6.0, 152.0, 2.5, rgba(scale_rgb(MEMBRANE, 0.75)));
            }
        }
        // Weaver: crown slit blazing.
        12 => {
            kyth_head(&mut c, 40.0, 44.0);
            c.rect(84, 34, 8, 26, rgba(scale_rgb(KYTH_GLOW, 1.15)));
            c.glow(88.0, 44.0, 22.0, KYTH_GLOW, 0.7);
            kyth_eyes(&mut c, 2, 80.0, 28.0);
        }
        // Burrower: spade head, soil scatter.
        15 => {
            kyth_head(&mut c, 44.0, 34.0);
            c.poly(&[(50.0, 84.0), (88.0, 66.0), (126.0, 84.0), (110.0, 96.0), (66.0, 96.0)], rgba(CHITIN_LIGHT));
            kyth_eyes(&mut c, 2, 76.0, 30.0);
            for side in [-1.0f32, 1.0] {
                c.poly(&[(88.0 + side * 34.0, 100.0), (88.0 + side * 52.0, 128.0), (88.0 + side * 28.0, 112.0)], rgba(CHITIN_LIGHT));
                c.line(88.0 + side * 38.0, 104.0, 88.0 + side * 48.0, 124.0, 1.8, rgba(team));
            }
            for k in 0..6 {
                let h = hash2(k, 3, 271);
                let x = 40.0 + (h % 96) as f32;
                let y = 150.0 + ((h >> 8) % 20) as f32;
                c.dome(x, y, 4.0, 2.5, [58, 46, 36]);
            }
        }
        // Ferron: rusted machine heads with optic lenses.
        16 => {
            fer_shoulders(&mut c, team);
            plate(&mut c, &[(64.0, 44.0), (112.0, 44.0), (116.0, 96.0), (60.0, 96.0)], RUST, 1.0);
            for gx in [-12.0f32, 12.0] {
                c.ellipse(88.0 + gx, 68.0, 7.0, 7.0, rgba([38, 36, 40]));
                c.ellipse(88.0 + gx, 68.0, 4.0, 4.0, rgba(COIL));
                c.glow(88.0 + gx, 68.0, 8.0, COIL, 0.5);
            }
            c.line(112.0, 44.0, 122.0, 22.0, 2.0, rgba(SCRAP));
            rust_wear(&mut c, 62, 46, 52, 46, 171);
        }
        17 => {
            fer_shoulders(&mut c, team);
            c.dome(88.0, 70.0, 34.0, 36.0, SCRAP);
            c.ellipse(88.0, 72.0, 15.0, 15.0, rgba([30, 28, 34]));
            c.ellipse(88.0, 72.0, 9.0, 9.0, rgba(COIL));
            c.ellipse(84.0, 68.0, 3.0, 3.0, rgba([235, 230, 255]));
            c.glow(88.0, 72.0, 22.0, COIL, 0.6);
            for side in [-1.0f32, 1.0] {
                c.line(88.0 + side * 30.0, 52.0, 88.0 + side * 40.0, 28.0, 2.5, rgba(RUST_LIT));
            }
        }
        18 => {
            fer_shoulders(&mut c, team);
            plate(&mut c, &[(56.0, 50.0), (120.0, 50.0), (114.0, 104.0), (62.0, 104.0)], RUST, 1.0);
            c.rect(66, 74, 44, 8, rgba([34, 30, 30]));
            c.rect(70, 76, 12, 4, rgba(AMBER));
            c.glow(76.0, 78.0, 10.0, AMBER, 0.6);
            c.rect(60, 96, 56, 6, rgba(RUST_DARK));
            rust_wear(&mut c, 58, 52, 60, 48, 181);
        }
        19 => {
            fer_shoulders(&mut c, team);
            c.dome(88.0, 78.0, 38.0, 32.0, RUST);
            c.ellipse(88.0, 52.0, 26.0, 8.0, rgba(scale_rgb(COIL, 0.8)));
            c.glow(88.0, 52.0, 26.0, COIL, 0.4);
            c.ellipse(80.0, 76.0, 4.0, 4.0, rgba(COIL));
            c.ellipse(98.0, 76.0, 4.0, 4.0, rgba(COIL));
            rust_wear(&mut c, 56, 60, 64, 36, 191);
        }
        20 => {
            fer_shoulders(&mut c, team);
            let y = vc_helm(&mut c, [110, 84, 60]);
            vc_visor(&mut c, y, [150, 210, 235], 26.0);
            for side in [-1.0f32, 1.0] {
                c.poly(&[(88.0 + side * 36.0, 60.0), (88.0 + side * 52.0, 44.0), (88.0 + side * 38.0, 74.0)], rgba(RUST_LIT));
            }
        }
        21 => {
            fer_shoulders(&mut c, team);
            c.dome(88.0, 78.0, 30.0, 34.0, RUST);
            c.ellipse(88.0, 76.0, 5.0, 5.0, rgba(COIL));
            c.glow(88.0, 76.0, 12.0, COIL, 0.7);
            for (ry, rr) in [(40.0f32, 22.0f32), (28.0, 15.0)] {
                c.ellipse(88.0, ry, rr, rr * 0.35, rgba(scale_rgb(COIL, 0.8)));
                c.glow(88.0, ry, rr, COIL, 0.35);
            }
            rust_wear(&mut c, 62, 60, 52, 40, 201);
        }
        // Marshal: scarred commander, gold-trimmed crest helm.
        22 => {
            vc_shoulders(&mut c, team);
            let y = vc_helm(&mut c, scale_rgb(GUNMETAL, 1.1));
            c.poly(&[(84.0, 40.0), (92.0, 40.0), (94.0, 8.0), (82.0, 8.0)], rgba([201, 162, 39]));
            c.line(56.0, 24.0, 120.0, 24.0, 3.0, rgba([201, 162, 39]));
            vc_visor(&mut c, y, VISOR, 26.0);
            c.line(70.0, 96.0, 82.0, 104.0, 2.0, rgba([70, 74, 84]));
        }
        // Broodmother: crowned queen with an eye cluster.
        23 => {
            kyth_head(&mut c, 46.0, 40.0);
            for k in 0..5 {
                let x = 56.0 + k as f32 * 16.0;
                let hh = 22.0 + ((k as i32 - 2).abs() as f32) * -4.0 + 8.0;
                c.poly(&[(x - 4.0, 42.0), (x, 42.0 - hh), (x + 4.0, 42.0)], rgba(team));
                c.glow(x, 40.0 - hh, 5.0, team, 0.4);
            }
            kyth_eyes(&mut c, 5, 74.0, 48.0);
            c.dome(88.0, 118.0, 40.0, 18.0, MEMBRANE);
            c.glow(88.0, 120.0, 20.0, KYTH_GLOW, 0.4);
        }
        // Magnus: sealed mask under a coil halo.
        24 => {
            fer_shoulders(&mut c, team);
            plate(&mut c, &[(64.0, 48.0), (112.0, 48.0), (116.0, 100.0), (60.0, 100.0)], SCRAP, 1.05);
            c.rect(70, 70, 36, 6, rgba(COIL));
            c.glow(88.0, 73.0, 16.0, COIL, 0.7);
            c.ellipse(88.0, 36.0, 30.0, 9.0, rgba(scale_rgb(COIL, 0.8)));
            c.glow(88.0, 36.0, 28.0, COIL, 0.5);
            rust_wear(&mut c, 62, 50, 52, 48, 891);
        }
        // Broodling: reuse the skitter face.
        25 => {
            kyth_head(&mut c, 34.0, 42.0);
            kyth_eyes(&mut c, 2, 70.0, 26.0);
        }
        // Skywing already 4; trooper default for anything unmapped.
        _ => {
            vc_shoulders(&mut c, team);
            let y = vc_helm(&mut c, GUNMETAL);
            vc_visor(&mut c, y, VISOR, 28.0);
        }
    }
    c
}

#[cfg(test)]
mod portrait_tests {
    use super::*;

    /// Dump all portraits as one PPM strip for eyeballing (run with
    /// --ignored; writes to ORION_PORTRAIT_STRIP).
    #[test]
    #[ignore]
    fn dump_portrait_strip() {
        let Ok(path) = std::env::var("ORION_PORTRAIT_STRIP") else { return };
        let (w, h) = (176 * 16, 176 * 2);
        let mut px = vec![[10u8, 10, 14, 255]; w * h];
        for ut in 0..16 {
            for team in 0..2 {
                let c = paint_portrait(ut, TEAMS[team]);
                for y in 0..c.h as usize {
                    for x in 0..c.w as usize {
                        let p = c.px[y * c.w as usize + x];
                        if p[3] > 0 {
                            px[(team * 176 + y) * w + ut * 176 + x] = p;
                        }
                    }
                }
            }
        }
        let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
        for p in &px {
            out.extend_from_slice(&p[..3]);
        }
        std::fs::write(path, out).unwrap();
    }
}
