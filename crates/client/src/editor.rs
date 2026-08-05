//! Visual map editor: paint terrain, resources and starts with automatic
//! 180-degree mirroring, save to ~/.orion-maps/, and play-test in place.
//! Every stroke lands on both halves, so edited maps are symmetric by
//! construction — the same fairness rule the shipped maps obey.

use orion_sim::map::{Map, TileKind, TilePos};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Low,
    High,
    Blocked,
    Ramp,
    Tree,
    Rock,
    Mineral,
    Geyser,
    Start,
    Expansion,
    Erase,
}

pub const TOOLS: [(Tool, &str, &str); 11] = [
    (Tool::Low, "LOW", "LOW GROUND (WALKABLE)"),
    (Tool::High, "HIGH", "HIGH GROUND (WALKABLE, SIGHT ADVANTAGE)"),
    (Tool::Blocked, "ROCK", "IMPASSABLE ROCK"),
    (Tool::Ramp, "RAMP", "RAMP BETWEEN LEVELS"),
    (Tool::Tree, "TREE", "DESTRUCTIBLE TREE - BLOCKS MOVE + SIGHT"),
    (Tool::Rock, "WALL", "DESTRUCTIBLE ROCK WALL - BLOCKS MOVE"),
    (Tool::Mineral, "MINS", "MINERAL PATCH (1500)"),
    (Tool::Geyser, "GAS", "PLASMA GEYSER (2X2, 2250)"),
    (Tool::Start, "START", "PLAYER STARTS (PLACES BOTH, MIRRORED)"),
    (Tool::Expansion, "EXPO", "EXPANSION SITE MARKER (3X3)"),
    (Tool::Erase, "ERASE", "REMOVE FEATURES / RESET TO LOW GROUND"),
];

pub struct Editor {
    pub map: Map,
    pub tool: Tool,
    pub brush: i32,
    pub name: String,
    pub dirty: bool,
    /// Feedback line shown in the editor footer.
    pub status: String,
}

fn blank(size: i32) -> Map {
    Map {
        width: size,
        height: size,
        kind: vec![TileKind::Ground; (size * size) as usize],
        elev: vec![0; (size * size) as usize],
        starts: vec![TilePos::new(12, 12), TilePos::new(size - 13, size - 13)],
        minerals: Vec::new(),
        geysers: Vec::new(),
        expansions: Vec::new(),
        trees: Vec::new(),
        rocks: Vec::new(),
    }
}

pub fn maps_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(home).join(".orion-maps")
}

/// Names of saved custom maps (no extension), sorted.
pub fn custom_map_names() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(maps_dir()) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "ron") {
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    out.push(stem.to_string());
                }
            }
        }
    }
    out.sort();
    out
}

pub fn load_custom(name: &str) -> Option<Map> {
    let src = std::fs::read_to_string(maps_dir().join(format!("{name}.ron"))).ok()?;
    ron::de::from_str(&src).ok()
}

impl Editor {
    pub fn new() -> Editor {
        // Continue the autosave if one exists; else a fresh canvas.
        let map = load_custom("autosave").unwrap_or_else(|| blank(88));
        Editor {
            map,
            tool: Tool::High,
            brush: 2,
            name: "custom".into(),
            dirty: false,
            status: "PAINT WITH LMB - EVERY STROKE MIRRORS".into(),
        }
    }

    fn idx(&self, x: i32, y: i32) -> usize {
        (y * self.map.width + x) as usize
    }

    fn in_bounds(&self, x: i32, y: i32) -> bool {
        // Keep a 2-tile frame so units can always path around features.
        x >= 2 && y >= 2 && x < self.map.width - 2 && y < self.map.height - 2
    }

    fn clear_features_at(&mut self, x: i32, y: i32) {
        let t = TilePos::new(x, y);
        self.map.trees.retain(|p| *p != t);
        self.map.rocks.retain(|p| *p != t);
        self.map.minerals.retain(|(p, _)| *p != t);
        // Geysers/expansions have footprints — clear when the tile falls
        // inside one.
        self.map.geysers.retain(|(p, _)| {
            !(x >= p.x && x < p.x + 2 && y >= p.y && y < p.y + 2)
        });
        self.map.expansions.retain(|p| {
            !(x >= p.x && x < p.x + 3 && y >= p.y && y < p.y + 3)
        });
    }

    /// Apply the current tool at one tile (no mirroring here).
    fn apply_at(&mut self, x: i32, y: i32) {
        if !self.in_bounds(x, y) {
            return;
        }
        let i = self.idx(x, y);
        match self.tool {
            Tool::Low => {
                self.map.kind[i] = TileKind::Ground;
                self.map.elev[i] = 0;
            }
            Tool::High => {
                self.map.kind[i] = TileKind::Ground;
                self.map.elev[i] = 1;
            }
            Tool::Blocked => {
                self.clear_features_at(x, y);
                self.map.kind[i] = TileKind::Blocked;
            }
            Tool::Ramp => {
                self.map.kind[i] = TileKind::Ramp;
                self.map.elev[i] = 0;
            }
            Tool::Tree => {
                self.clear_features_at(x, y);
                self.map.kind[i] = TileKind::Ground;
                if !self.map.trees.contains(&TilePos::new(x, y)) {
                    self.map.trees.push(TilePos::new(x, y));
                }
            }
            Tool::Rock => {
                self.clear_features_at(x, y);
                self.map.kind[i] = TileKind::Ground;
                if !self.map.rocks.contains(&TilePos::new(x, y)) {
                    self.map.rocks.push(TilePos::new(x, y));
                }
            }
            Tool::Mineral => {
                self.clear_features_at(x, y);
                self.map.kind[i] = TileKind::Ground;
                self.map.minerals.push((TilePos::new(x, y), 1500));
            }
            Tool::Geyser => {
                self.clear_features_at(x, y);
                for dy in 0..2 {
                    for dx in 0..2 {
                        let j = self.idx(x + dx, y + dy);
                        self.map.kind[j] = TileKind::Ground;
                        self.clear_features_at(x + dx, y + dy);
                    }
                }
                self.map.geysers.push((TilePos::new(x, y), 2250));
            }
            Tool::Start => {
                let (w, h) = (self.map.width, self.map.height);
                self.map.starts = vec![
                    TilePos::new(x, y),
                    TilePos::new(w - 1 - x, h - 1 - y),
                ];
            }
            Tool::Expansion => {
                self.map.expansions.push(TilePos::new(x, y));
            }
            Tool::Erase => {
                self.clear_features_at(x, y);
                self.map.kind[i] = TileKind::Ground;
                self.map.elev[i] = 0;
            }
        }
        self.dirty = true;
    }

    /// Paint with the brush at a tile — mirrored onto the far half.
    pub fn paint(&mut self, cx: i32, cy: i32) {
        let (w, h) = (self.map.width, self.map.height);
        let r = self.brush - 1;
        // Point features ignore the brush (one per click reads better).
        let point_tool = matches!(
            self.tool,
            Tool::Mineral | Tool::Geyser | Tool::Start | Tool::Expansion
        );
        let r = if point_tool { 0 } else { r };
        for dy in -r..=r {
            for dx in -r..=r {
                let (x, y) = (cx + dx, cy + dy);
                self.apply_at(x, y);
                // Mirror twin (footprint-aware for 2x2/3x3 origins). Start
                // handles both itself.
                if self.tool != Tool::Start {
                    let (mx, my) = match self.tool {
                        Tool::Geyser => (w - 2 - x, h - 2 - y),
                        Tool::Expansion => (w - 3 - x, h - 3 - y),
                        _ => (w - 1 - x, h - 1 - y),
                    };
                    self.apply_at(mx, my);
                }
            }
        }
    }

    /// Light validation before saving/playing.
    pub fn validate(&self) -> Result<(), String> {
        if self.map.starts.len() != 2 {
            return Err("PLACE STARTS FIRST".into());
        }
        if self.map.minerals.len() < 8 {
            return Err("NEED AT LEAST 4 MINERAL PATCHES PER SIDE".into());
        }
        let s0 = self.map.starts[0];
        let near = self.map.minerals.iter().any(|(p, _)| {
            (p.x - s0.x).abs() + (p.y - s0.y).abs() < 20
        });
        if !near {
            return Err("PUT MINERALS NEAR THE STARTS".into());
        }
        Ok(())
    }

    pub fn save(&mut self, name: &str) -> Result<(), String> {
        let dir = maps_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let ron = ron::ser::to_string(&self.map).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(format!("{name}.ron")), ron).map_err(|e| e.to_string())?;
        self.dirty = false;
        Ok(())
    }
}
