//! Procedural sprite atlas — the entire art of the game, painted at startup.
//!
//! Style target: SC1-era isometric pixel art. Dark outlines, 3-4 tone
//! material ramps, dithered texture, light from the upper-left, hard team
//! color accents. Every sprite is painted per team so no runtime palette
//! tricks are needed.

use crate::font;

pub const ATLAS: u32 = 2048;

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
    pub building_px_h: [u32; 14],
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
                    units.push(p.place(&c));
                }
            }
        }
    }

    // Buildings: [type][team].
    let mut buildings = Vec::new();
    let mut building_px_h = [0u32; 14];
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
            building_px_h[b_type] = c.h as u32;
            buildings.push(p.place(&c));
        }
    }

    // Destructible flora + rocks.
    let trees = [p.place(&tree_canvas(0)), p.place(&tree_canvas(1))];
    let rock_wall = {
        let mut c = Canvas::new(22, 18);
        c.ellipse_shaded(7.0, 11.0, 6.5, 5.0, [96, 92, 88]);
        c.ellipse_shaded(15.0, 12.0, 6.0, 4.5, [88, 84, 82]);
        c.ellipse_shaded(11.0, 7.0, 5.0, 4.0, [104, 100, 94]);
        c.outline([30, 28, 26, 255]);
        p.place(&c)
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
    let mut c = Canvas::new(20, 26);
    // Trunk.
    c.rect(9, 18, 2, 6, [58, 44, 34, 255]);
    c.rect(8, 22, 4, 2, [48, 36, 28, 255]);
    // Canopy: three stacked shaded blobs, slightly skewed per variant.
    let sk = if v == 0 { 0.0 } else { 1.5 };
    c.ellipse_shaded(10.0 + sk, 14.0, 8.0, 5.0, [38, 92, 74]);
    c.ellipse_shaded(10.0 - sk, 9.0, 6.5, 4.5, [46, 108, 82]);
    c.ellipse_shaded(10.0, 5.0, 4.5, 3.5, [58, 124, 92]);
    // Sparse lighter needles.
    for k in 0..10 {
        let h = hash2(k, v, 421);
        let x = 4 + (h % 12) as i32;
        let y = 3 + ((h >> 8) % 12) as i32;
        c.blend(x, y, [92, 160, 118, 160]);
    }
    c.outline([16, 26, 22, 255]);
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

// ----------------------------------------------------- Kyth Assembly ----

const CHITIN: [u8; 3] = [96, 78, 104];
const CHITIN_LIGHT: [u8; 3] = [128, 106, 138];
const MEMBRANE: [u8; 3] = [168, 120, 92];
const KYTH_GLOW: [u8; 3] = [180, 255, 140];

/// Drone: round hover-bug worker. 24x22.
fn paint_kdrone(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(24, 22);
    let (dx, dy) = facing_vec(f);
    let cx = 12.0;
    let cy = 12.0;
    let bob = if frame == 0 { 0.0 } else { -0.8 };
    // Legs.
    for side in [-1.0f32, 1.0] {
        let lo = if frame == 0 { 1.0 } else { 0.0 };
        c.line(cx, cy + bob + 2.0, cx + side * 6.0, cy + 7.0 + lo * side, 1.2, rgba([58, 48, 62]));
    }
    // Segmented body.
    c.ellipse_shaded(cx - dx * 2.0, cy + bob, 5.0, 4.0, CHITIN);
    c.ellipse_shaded(cx + dx * 3.0, cy + bob + dy * 1.5, 3.6, 3.0, CHITIN_LIGHT);
    // Team marking + eye.
    c.ellipse(cx - dx * 3.0, cy + bob - 1.0, 1.8, 1.4, rgba(team));
    if dy > -0.5 {
        c.ellipse(cx + dx * 4.5, cy + bob + dy * 1.8, 1.2, 1.0, rgba(KYTH_GLOW));
    }
    c.outline(OUTLINE);
    c
}

/// Skitter: small fast 4-legged blade-bug. 22x20.
fn paint_skitter(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(22, 20);
    let (dx, dy) = facing_vec(f);
    let cx = 11.0;
    let cy = 11.0;
    let step = if frame == 0 { 1.5 } else { -1.5 };
    for (k, side) in [-1.0f32, 1.0].iter().enumerate() {
        let sw = if k == 0 { step } else { -step };
        c.line(cx - 2.0, cy + 1.0, cx - 5.0 + sw, cy + 6.0, 1.1, rgba([58, 48, 62]));
        c.line(cx + 2.0, cy + 1.0, cx + 5.0 - sw, cy + 6.0, 1.1, rgba([58, 48, 62]));
        let _ = side;
    }
    // Low sleek body.
    c.ellipse_shaded(cx - dx * 1.5, cy, 4.5, 2.8, CHITIN);
    // Blade mandibles toward facing.
    c.line(cx + dx * 3.0, cy + dy * 1.5, cx + dx * 8.0, cy + dy * 4.0 - 1.0, 1.4, rgba(team));
    c.line(cx + dx * 3.0, cy + dy * 1.5 + 1.5, cx + dx * 8.0, cy + dy * 4.0 + 1.5, 1.4, rgba(team));
    if dy > -0.5 {
        c.ellipse(cx + dx * 2.5, cy + dy * 1.2 - 0.5, 1.0, 0.8, rgba(KYTH_GLOW));
    }
    c.outline(OUTLINE);
    c
}

/// Spitter: slug with an acid tube. 26x24.
fn paint_spitter(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(26, 24);
    let (dx, dy) = facing_vec(f);
    let cx = 13.0;
    let cy = 14.0;
    let squish = if frame == 0 { 0.0 } else { 0.6 };
    // Slug body.
    c.ellipse_shaded(cx - dx * 2.0, cy + squish, 6.5, 4.5 - squish, MEMBRANE);
    c.ellipse_shaded(cx - dx * 4.0, cy - 1.0, 3.5, 2.8, CHITIN);
    // Team ridge.
    c.line(cx - dx * 6.0, cy - 3.0, cx + dx * 1.0, cy - 4.0, 1.4, rgba(team));
    // Acid tube toward facing, tilted up.
    c.line(cx + dx * 2.0, cy - 1.0, cx + dx * 8.0, cy - 4.0 + dy * 3.0, 2.2, rgba(CHITIN_LIGHT));
    c.ellipse(cx + dx * 8.5, cy - 4.5 + dy * 3.2, 1.6, 1.3, rgba(KYTH_GLOW));
    c.outline(OUTLINE);
    c
}

/// Ravager: towering splash beetle. 34x32.
fn paint_ravager(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(34, 32);
    let (dx, dy) = facing_vec(f);
    let cx = 17.0;
    let cy = 18.0;
    let lift = if frame == 0 { 0.0 } else { 1.0 };
    // Six thick legs.
    for k in 0..3 {
        let off = -5.0 + k as f32 * 5.0;
        c.line(cx + off, cy + 2.0, cx + off - 3.0, cy + 10.0 - lift, 1.8, rgba([58, 48, 62]));
        c.line(cx + off, cy + 2.0, cx + off + 3.0, cy + 10.0 + lift - 1.0, 1.8, rgba([58, 48, 62]));
    }
    // Massive carapace with plates.
    c.ellipse_shaded(cx - dx * 2.0, cy - 2.0, 10.0, 7.5, CHITIN);
    c.ellipse_shaded(cx - dx * 4.0, cy - 5.0, 6.0, 4.0, CHITIN_LIGHT);
    c.line(cx - 8.0, cy - 2.0, cx + 8.0, cy - 2.0, 1.2, rgba(scale_rgb(CHITIN, 0.7)));
    // Team spines.
    for k in 0..3 {
        let sx = cx - 5.0 + k as f32 * 5.0;
        c.line(sx, cy - 7.0, sx + 1.0, cy - 11.0, 1.4, rgba(team));
    }
    // Crusher claws toward facing.
    for side in [-1.0f32, 1.0] {
        let px = cx + dx * 6.0 - dy * side * 5.0;
        let py = cy + dy * 5.0 + dx * side * 3.0;
        c.line(px, py, px + dx * 6.5, py + dy * 4.5, 2.6, rgba(CHITIN_LIGHT));
        c.line(px + dx * 5.0, py + dy * 3.5, px + dx * 7.0, py + dy * 5.0, 1.6, rgba(team));
    }
    if dy > -0.4 {
        c.ellipse(cx + dx * 4.0, cy - 1.0 + dy * 2.0, 1.6, 1.2, rgba(KYTH_GLOW));
    }
    c.outline(OUTLINE);
    c
}

/// Wisp: floating jelly flyer. 26x26.
fn paint_wisp(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(26, 26);
    let (dx, dy) = facing_vec(f);
    let cx = 13.0;
    let cy = 11.0;
    let pulse = if frame == 0 { 0.0 } else { 1.0 };
    // Bell.
    c.ellipse_shaded(cx, cy, 6.0 + pulse * 0.8, 5.0 - pulse * 0.5, MEMBRANE);
    c.ellipse(cx - 1.5, cy - 1.5, 2.5, 2.0, rgba(scale_rgb(MEMBRANE, 1.25)));
    c.ellipse(cx + dx * 2.0, cy + dy * 1.5, 2.0, 1.6, rgba(team));
    // Tentacles trailing away from facing.
    for k in 0..4 {
        let off = -4.5 + k as f32 * 3.0;
        let sway = if (k + frame) % 2 == 0 { 1.5 } else { -1.0 };
        c.line(
            cx + off,
            cy + 4.0,
            cx + off - dx * 4.0 + sway,
            cy + 10.0 - dy * 2.0,
            1.1,
            rgba(scale_rgb(MEMBRANE, 0.8)),
        );
    }
    c.ellipse(cx + dx * 3.5, cy + dy * 2.0 - 1.0, 1.2, 1.0, rgba(KYTH_GLOW));
    c.outline(OUTLINE);
    c
}

/// Weaver: psionic tentacle node. 26x30.
fn paint_weaver(f: usize, frame: usize, team: [u8; 3]) -> Canvas {
    let mut c = Canvas::new(26, 30);
    let (dx, dy) = facing_vec(f);
    let cx = 13.0;
    let cy = 17.0;
    // Root tentacles.
    for k in 0..4 {
        let off = -5.0 + k as f32 * 3.3;
        c.line(cx + off, cy + 4.0, cx + off * 1.5, cy + 10.0, 1.3, rgba([58, 48, 62]));
    }
    // Bulb body.
    c.ellipse_shaded(cx, cy, 5.5, 6.0, CHITIN);
    c.ellipse_shaded(cx, cy - 6.0, 4.0, 3.5, CHITIN_LIGHT);
    c.line(cx - 3.0, cy + 2.0, cx + 3.0, cy + 3.0, 1.4, rgba(team));
    // Floating rift shards orbiting toward facing.
    let a0 = if frame == 0 { 0.0f32 } else { 0.8 };
    for k in 0..3 {
        let a = a0 + k as f32 * 2.1;
        let px = cx + dx * 6.0 + a.cos() * 3.5;
        let py = cy - 8.0 + dy * 3.0 + a.sin() * 2.0;
        c.line(px, py - 1.5, px, py + 1.5, 1.2, rgba(KYTH_GLOW));
    }
    if dy > -0.4 {
        c.ellipse(cx + dx * 1.5, cy - 6.0, 1.6, 1.1, rgba(KYTH_GLOW));
    }
    c.outline(OUTLINE);
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
