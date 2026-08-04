//! Automated QA: run full bot games headless while checking invariants,
//! collecting metrics, and (optionally) shadow-stepping a second sim to
//! prove determinism. Built for machine consumption — the soak example and
//! CI tests drive this; a human never has to click through 30 matches.

use crate::ai::{Bot, Difficulty};

use crate::state::{EntityKind, RES_MINERALS};
use crate::{GameData, State};

/// One game's setup. Everything needed to reproduce it exactly.
#[derive(Clone, Debug)]
pub struct GameConfig {
    pub seed: u64,
    pub map: &'static str,
    pub races: [u8; 2],
    pub styles: [u64; 2],
    pub difficulty: Difficulty,
    pub max_ticks: u32,
    /// Run a shadow sim on the same inputs and compare checksums.
    pub shadow: bool,
}

#[derive(Clone, Debug, Default)]
pub struct PlayerMetrics {
    pub units_built: u32,
    pub units_lost: u32,
    pub buildings_lost: u32,
    pub minerals_mined: u32,
    pub gas_mined: u32,
    pub peak_army_supply: u32,
    pub upgrades: u32,
}

#[derive(Clone, Debug)]
pub struct GameReport {
    pub cfg: GameConfig,
    pub winner: Option<u8>,
    pub ticks: u32,
    pub players: [PlayerMetrics; 2],
    pub first_blood_tick: Option<u32>,
    pub violations: Vec<String>,
    pub final_checksum: u64,
}

impl GameReport {
    pub fn csv_header() -> &'static str {
        "seed,race0,race1,style0,style1,winner,secs,first_blood_s,\
         built0,lost0,mined0,gas0,peak0,built1,lost1,mined1,gas1,peak1,violations"
    }

    pub fn csv_row(&self) -> String {
        format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            self.cfg.seed,
            self.cfg.races[0],
            self.cfg.races[1],
            self.cfg.styles[0],
            self.cfg.styles[1],
            self.winner.map(|w| w.to_string()).unwrap_or_else(|| "-".into()),
            self.ticks / crate::TICKS_PER_SEC,
            self.first_blood_tick
                .map(|t| (t / crate::TICKS_PER_SEC).to_string())
                .unwrap_or_else(|| "-".into()),
            self.players[0].units_built,
            self.players[0].units_lost,
            self.players[0].minerals_mined,
            self.players[0].gas_mined,
            self.players[0].peak_army_supply,
            self.players[1].units_built,
            self.players[1].units_lost,
            self.players[1].minerals_mined,
            self.players[1].gas_mined,
            self.players[1].peak_army_supply,
            self.violations.len(),
        )
    }
}

/// Per-tick invariant checks — the bug detectors. Each violation is a
/// one-line description with the tick baked in.
pub struct Validator {
    violations: Vec<String>,
    econ_last: Vec<(u32, u32)>, // (mined_total, tick_it_changed)
    econ_flagged: Vec<bool>,
}

const ECON_STALL_TICKS: u32 = 24 * 180; // 3 minutes with income frozen

impl Validator {
    pub fn new(players: usize) -> Validator {
        Validator {
            violations: Vec::new(),
            econ_last: vec![(0, 0); players],
            econ_flagged: vec![false; players],
        }
    }

    fn flag(&mut self, tick: u32, msg: String) {
        if self.violations.len() < 50 {
            self.violations.push(format!("t={tick} {msg}"));
        }
    }

    pub fn check(&mut self, s: &State) {
        if s.tick % 24 != 0 {
            return;
        }
        let t = s.tick;

        // Entity invariants.
        for (i, e) in s.entities.iter().enumerate() {
            if !e.alive {
                continue;
            }
            let (x, y) = (e.pos.x.to_f32(), e.pos.y.to_f32());
            if x < 0.0 || y < 0.0 || x > s.map.width as f32 || y > s.map.height as f32 {
                self.flag(t, format!("entity {i} out of bounds at {x:.1},{y:.1}"));
            }
            match e.kind {
                EntityKind::Unit => {
                    let d = &s.data.units[e.def as usize];
                    if e.hp > d.hp {
                        self.flag(t, format!("unit {i} hp {} > max {}", e.hp, d.hp));
                    }
                    if e.hp <= 0 {
                        self.flag(t, format!("unit {i} alive with hp {}", e.hp));
                    }
                    if !d.fly {
                        let tile = crate::TilePos::of(e.pos);
                        if !s.map.walkable(tile.x, tile.y) {
                            self.flag(
                                t,
                                format!("ground unit {i} ({}) on unwalkable tile", d.tag),
                            );
                        }
                    }
                    if d.energy_max == 0 && e.energy > 0 {
                        self.flag(t, format!("non-caster {i} has energy"));
                    }
                }
                EntityKind::Building => {
                    let d = &s.data.buildings[e.def as usize];
                    if e.hp > d.hp {
                        self.flag(t, format!("building {i} hp {} > max {}", e.hp, d.hp));
                    }
                }
                EntityKind::Resource => {
                    if e.amount < 0 {
                        self.flag(t, format!("resource {i} negative amount {}", e.amount));
                    }
                }
            }
        }

        // Global sanity.
        let alive = s.entities.iter().filter(|e| e.alive).count();
        if alive > 2000 {
            self.flag(t, format!("entity explosion: {alive} alive"));
        }
        if s.fields.fields.len() > 5000 {
            self.flag(t, format!("flow field leak: {} slots", s.fields.fields.len()));
        }
        for (p, pl) in s.players.iter().enumerate() {
            if pl.minerals > 1_000_000 || pl.gas > 1_000_000 {
                self.flag(t, format!("p{p} resource overflow {}m {}g", pl.minerals, pl.gas));
            }
            let (used, provided) = s.supply(p as u8);
            if used > crate::SUPPLY_CAP + 20 || provided > crate::SUPPLY_CAP {
                self.flag(t, format!("p{p} supply out of range {used}/{provided}"));
            }
        }

        // Economy stall: workers alive, minerals on the map, income frozen.
        for p in 0..s.players.len() {
            let mined = s.players[p].minerals_mined + s.players[p].gas_mined;
            if mined != self.econ_last[p].0 {
                self.econ_last[p] = (mined, t);
                continue;
            }
            if self.econ_flagged[p] || t - self.econ_last[p].1 < ECON_STALL_TICKS {
                continue;
            }
            let workers = s
                .entities
                .iter()
                .filter(|e| {
                    e.alive
                        && e.owner == p as u8
                        && e.kind == EntityKind::Unit
                        && s.data.units[e.def as usize].harvester
                })
                .count();
            let depots: Vec<_> = s
                .entities
                .iter()
                .filter(|e| {
                    e.alive
                        && e.owner == p as u8
                        && e.kind == EntityKind::Building
                        && e.construction.is_none()
                        && s.data.buildings[e.def as usize].deposit
                })
                .map(|e| e.pos)
                .collect();
            let has_depot = !depots.is_empty();
            // Only live patches NEAR an own depot count: a mined-out main
            // with idle workers is the designed endgame, not a stall.
            let reach = crate::fixed::Fx::from_int(20);
            let reach_sq = (reach.0 as i64) * (reach.0 as i64);
            let patches = s.entities.iter().any(|e| {
                e.alive
                    && e.kind == EntityKind::Resource
                    && e.def == RES_MINERALS
                    && e.amount > 0
                    && depots
                        .iter()
                        .any(|&d| crate::fixed::dist_sq_raw(d, e.pos) <= reach_sq)
            });
            if workers >= 3 && patches && has_depot {
                self.econ_flagged[p] = true;
                self.flag(t, format!("p{p} economy stalled ({workers} workers idle-ish)"));
            }
        }
    }

    pub fn into_violations(self) -> Vec<String> {
        self.violations
    }
}

/// Run one full bot game under the validator. Panics inside the sim are
/// bugs; the config is in the report for reproduction.
pub fn run_game(cfg: GameConfig) -> GameReport {
    let map = crate::map::by_name(cfg.map).expect("known map");
    let mut s = State::new_with_races(GameData::load_default(), map.clone(), cfg.seed, &cfg.races);
    let mut shadow = cfg
        .shadow
        .then(|| State::new_with_races(GameData::load_default(), map, cfg.seed, &cfg.races));
    let mut bots = [
        Bot::with_style(0, cfg.difficulty, cfg.styles[0]),
        Bot::with_style(1, cfg.difficulty, cfg.styles[1]),
    ];
    let mut validator = Validator::new(2);
    let mut peak = [0u32; 2];
    let mut first_blood = None;

    while s.tick < cfg.max_ticks && s.winner.is_none() {
        let mut cmds = Vec::new();
        for bot in &mut bots {
            cmds.extend(bot.think(&s));
        }
        s.step(&cmds);
        if let Some(sh) = shadow.as_mut() {
            sh.step(&cmds);
            if s.tick % 24 == 0 && sh.checksum() != s.checksum() {
                validator.flag(s.tick, "SHADOW DESYNC: determinism broken".into());
                shadow = None; // one report is enough
            }
        }
        validator.check(&s);
        if s.tick % 24 == 0 {
            for p in 0..2usize {
                let army: u32 = s
                    .entities
                    .iter()
                    .filter(|e| {
                        e.alive
                            && e.owner == p as u8
                            && e.kind == EntityKind::Unit
                            && !s.data.units[e.def as usize].harvester
                    })
                    .map(|e| s.data.units[e.def as usize].supply)
                    .sum();
                peak[p] = peak[p].max(army);
            }
            if first_blood.is_none()
                && s.players.iter().any(|p| p.units_lost + p.buildings_lost > 0)
            {
                first_blood = Some(s.tick);
            }
        }
    }

    let mk = |p: usize| PlayerMetrics {
        units_built: s.players[p].units_built,
        units_lost: s.players[p].units_lost,
        buildings_lost: s.players[p].buildings_lost,
        minerals_mined: s.players[p].minerals_mined,
        gas_mined: s.players[p].gas_mined,
        peak_army_supply: peak[p],
        upgrades: (s.players[p].weapons_level + s.players[p].armor_level) as u32,
    };
    GameReport {
        winner: s.winner,
        ticks: s.tick,
        players: [mk(0), mk(1)],
        first_blood_tick: first_blood,
        violations: validator.into_violations(),
        final_checksum: s.checksum(),
        cfg,
    }
}

/// Standard soak matrix: `n` games across all race matchups with varied
/// seeds/styles. Deterministic given `base_seed`.
pub fn soak_configs(n: usize, base_seed: u64, shadow_every: usize) -> Vec<GameConfig> {
    let matchups = [[0u8, 0u8], [1, 1], [2, 2], [0, 1], [1, 0], [0, 2], [2, 0], [1, 2], [2, 1]];
    (0..n)
        .map(|k| {
            let seed = base_seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(k as u64 * 1442695040888963407);
            GameConfig {
                seed,
                // Alternate maps every full matchup cycle.
                map: crate::map::MAP_NAMES[(k / matchups.len()) % crate::map::MAP_NAMES.len()],
                races: matchups[k % matchups.len()],
                styles: [seed ^ 0xA5A5, seed.rotate_left(23) ^ 0x5A5A],
                difficulty: Difficulty::Normal,
                max_ticks: 24 * 60 * 15,
                shadow: shadow_every > 0 && k % shadow_every == 0,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CI-sized soak: a handful of short games with all invariants and one
    /// shadow-determinism run. Any violation fails the build.
    #[test]
    fn small_soak_is_clean() {
        let mut cfgs = soak_configs(8, 0xC1, 4); // 4 matchups x both maps
        for c in &mut cfgs {
            c.max_ticks = 24 * 60 * 6; // keep CI fast
        }
        for cfg in cfgs {
            let label = format!("{cfg:?}");
            let r = run_game(cfg);
            assert!(
                r.violations.is_empty(),
                "violations in {label}: {:?}",
                r.violations
            );
        }
    }
}
