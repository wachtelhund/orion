//! Scripted skirmish opponent.
//!
//! The bot is a *command source*: it reads state and emits the same
//! `Command`s a human would. It never touches sim internals, which means it
//! is deterministic, replay-compatible, and — later — lockstep-compatible
//! (its commands go into the same input stream as a remote player's).
//!
//! It reads the enemy start location from the map (fair: both players know
//! spawns on a two-player map) but targets only what the map gives away —
//! it does not read through fog for decision-making beyond that.

use crate::data::DefId;
use crate::entity::EntityId;
use crate::fixed::{dist_sq_raw, Fx, FxVec2};
use crate::map::TilePos;
use crate::state::{Command, EntityKind, GatherPhase, Order, State};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Difficulty {
    Easy,
    Normal,
    Hard,
}

/// Macro parameters per difficulty. The bot never cheats — harder just
/// means tighter macro and earlier aggression.
struct Params {
    think_interval: u32,
    max_workers: usize,
    gas_workers: usize,
    max_barracks: usize,
    attack_supply: u32,
    reattack_interval: u32,
    mineral_buffer: u32,
    /// Build a Forge and mix in Breaker tanks.
    use_forge: bool,
}

impl Difficulty {
    fn params(self) -> Params {
        match self {
            Difficulty::Easy => Params {
                think_interval: 24,
                max_workers: 9,
                gas_workers: 1,
                max_barracks: 1,
                attack_supply: 24,
                reattack_interval: 24 * 30,
                mineral_buffer: 250,
                use_forge: false,
            },
            Difficulty::Normal => Params {
                think_interval: 12,
                max_workers: 14,
                gas_workers: 2,
                max_barracks: 2,
                attack_supply: 14,
                reattack_interval: 24 * 20,
                mineral_buffer: 100,
                use_forge: true,
            },
            Difficulty::Hard => Params {
                think_interval: 6,
                max_workers: 16,
                gas_workers: 3,
                max_barracks: 3,
                attack_supply: 12,
                reattack_interval: 24 * 12,
                mineral_buffer: 40,
                use_forge: true,
            },
        }
    }
}

pub struct Bot {
    pub player: u8,
    pub difficulty: Difficulty,
    /// Personality: small deterministic offsets to timings/caps so games
    /// against (and between) bots vary run to run.
    pub style: u64,
    last_attack_tick: u32,
}

impl Bot {
    pub fn new(player: u8) -> Bot {
        Bot::with(player, Difficulty::Normal)
    }

    pub fn with(player: u8, difficulty: Difficulty) -> Bot {
        Bot { player, difficulty, style: 0, last_attack_tick: 0 }
    }

    pub fn with_style(player: u8, difficulty: Difficulty, style: u64) -> Bot {
        Bot { player, difficulty, style, last_attack_tick: 0 }
    }

    pub fn think(&mut self, s: &State) -> Vec<(u8, Command)> {
        let mut prm = self.difficulty.params();
        // Personality offsets.
        prm.attack_supply += (self.style % 5) as u32;
        prm.reattack_interval += ((self.style / 5) % 7) as u32 * 24;
        prm.max_workers += ((self.style / 35) % 3) as usize;
        // Escalation: as the game drags on, mass BIGGER decisive pushes
        // (small-wave attrition stalls games), and push more often late.
        let esc = (s.tick / (24 * 120)) * 3;
        prm.attack_supply = (prm.attack_supply + esc).min(36);
        if s.tick > 24 * 60 * 12 {
            prm.reattack_interval /= 2;
        }
        // Phase-shift think ticks by style so two bots' decision points
        // interleave differently per game — decorrelates mirror matches.
        let phase = (self.style % prm.think_interval as u64) as u32;
        if (s.tick + phase) % prm.think_interval != 0 {
            return Vec::new();
        }
        let p = self.player;
        let race = s.players[p as usize].race;
        let mut cmds: Vec<Command> = Vec::new();

        // Capability-driven role lookup: works for any race in the data.
        let worker_def = s.data.worker_of_race(race);
        let depot_def = s
            .data
            .buildings
            .iter()
            .position(|b| b.race == race && b.supply_provided > 0 && !b.headquarters)
            .expect("race has no supply building") as DefId;
        let condenser_def = s
            .data
            .buildings
            .iter()
            .position(|b| b.race == race && b.gas_extractor)
            .expect("race has no extractor") as DefId;
        // Tier-0 production: trains combat units, no tech requirement.
        let barracks_def = s
            .data
            .buildings
            .iter()
            .position(|b| {
                b.race == race && !b.headquarters && !b.trains.is_empty() && b.requires.is_none()
            })
            .expect("race has no basic production") as DefId;
        // Tier-1 production: cheapest one gated behind tech.
        let forge_def = s
            .data
            .buildings
            .iter()
            .enumerate()
            .filter(|(_, b)| {
                b.race == race && !b.trains.is_empty() && b.requires == Some(barracks_def)
            })
            .min_by_key(|(_, b)| b.cost_minerals + b.cost_gas)
            .map(|(i, _)| i as DefId);

        // ---- census ----
        let mut workers: Vec<u32> = Vec::new();
        let mut idle_workers: Vec<u32> = Vec::new();
        let mut mineral_workers: Vec<u32> = Vec::new();
        let mut gas_workers = 0usize;
        let mut army: Vec<EntityId> = Vec::new();
        let mut army_supply = 0u32;
        let mut hq: Option<u32> = None;
        let mut barracks: Vec<u32> = Vec::new();
        let mut forge: Vec<u32> = Vec::new();
        let mut condenser: Option<u32> = None;
        let mut constructing = 0u32;
        let mut building_worker_busy = false;
        for (i, e) in s.entities.iter().enumerate() {
            if !e.alive || e.owner != p {
                continue;
            }
            match e.kind {
                EntityKind::Unit => {
                    if e.def == worker_def {
                        workers.push(i as u32);
                        match e.order {
                            Order::Idle => idle_workers.push(i as u32),
                            Order::Build { .. } => building_worker_busy = true,
                            Order::Gather { resource, .. } => {
                                let gas = s
                                    .get(resource)
                                    .is_some_and(|r| r.kind == EntityKind::Building);
                                if gas {
                                    gas_workers += 1;
                                } else {
                                    mineral_workers.push(i as u32);
                                }
                            }
                            _ => {}
                        }
                    } else {
                        army.push(s.id_of(i as u32));
                        army_supply += s.data.units[e.def as usize].supply;
                    }
                }
                EntityKind::Building => {
                    if e.construction.is_some() {
                        constructing += 1;
                    } else if s.data.buildings[e.def as usize].headquarters {
                        hq = Some(i as u32);
                    } else if e.def == barracks_def {
                        barracks.push(i as u32);
                    } else if Some(e.def) == forge_def {
                        forge.push(i as u32);
                    } else if e.def == condenser_def {
                        condenser = Some(i as u32);
                    }
                }
                EntityKind::Resource => {}
            }
        }
        let minerals = s.players[p as usize].minerals;
        let gas = s.players[p as usize].gas;
        let (used, provided) = s.supply(p);

        // ---- idle workers back to mining ----
        for &w in &idle_workers {
            if let Some(res) = nearest_mineral(s, s.entities[w as usize].pos) {
                cmds.push(Command::Gather {
                    units: vec![s.id_of(w)],
                    resource: res,
                    queued: false,
                });
            }
        }

        // ---- staff the condenser from the mineral line ----
        if let Some(c) = condenser {
            if gas_workers < prm.gas_workers {
                if let Some(&w) = mineral_workers.first() {
                    cmds.push(Command::Gather {
                        units: vec![s.id_of(w)],
                        resource: s.id_of(c),
                        queued: false,
                    });
                }
            }
        }

        // ---- worker production ----
        if let Some(hq_idx) = hq {
            let hq_e = &s.entities[hq_idx as usize];
            if workers.len() < prm.max_workers && hq_e.queue.is_empty() && minerals >= 50 {
                cmds.push(Command::Train { building: s.id_of(hq_idx), unit: worker_def });
            }
        }

        // ---- expansion: supply, tech, gas — one construction at a time ----
        let supply_headroom = provided.saturating_sub(used);
        let want_depot = supply_headroom <= 3 && provided < crate::SUPPLY_CAP;
        let want_barracks = barracks.len() < prm.max_barracks && workers.len() >= 8;
        let want_condenser =
            condenser.is_none() && !barracks.is_empty() && workers.len() >= 10;
        let want_forge = prm.use_forge
            && forge_def.is_some()
            && forge.is_empty()
            && !barracks.is_empty()
            && condenser.is_some()
            && workers.len() >= 12;
        let depot_cost = s.data.buildings[depot_def as usize].cost_minerals;
        let cond_cost = s.data.buildings[condenser_def as usize].cost_minerals;
        let rax_cost = s.data.buildings[barracks_def as usize].cost_minerals;
        if constructing == 0 && !building_worker_busy {
            if want_depot && minerals >= depot_cost {
                self.order_build(s, &workers, depot_def, &mut cmds);
            } else if want_condenser && minerals >= cond_cost {
                self.order_build_extractor(s, &workers, condenser_def, &mut cmds);
            } else if want_barracks && minerals >= rax_cost {
                self.order_build(s, &workers, barracks_def, &mut cmds);
            } else if want_forge {
                let fd = forge_def.unwrap();
                let f = &s.data.buildings[fd as usize];
                if minerals >= f.cost_minerals + 50 && gas >= f.cost_gas {
                    self.order_build(s, &workers, fd, &mut cmds);
                }
            }
        }

        // ---- army production: each production building trains the most
        // expensive combat unit it can afford right now ----
        let mut gas_left = gas;
        let mut min_left = minerals;
        for &b in barracks.iter().chain(forge.iter()) {
            let e = &s.entities[b as usize];
            if e.queue.len() >= 2 {
                continue;
            }
            let affordable: Vec<DefId> = s.data.buildings[e.def as usize]
                .trains
                .iter()
                .copied()
                .filter(|&u| {
                    let d = &s.data.units[u as usize];
                    !d.harvester
                        && d.energy_max == 0 // bot skips casters
                        && s.requirement_met(p, d.requires)
                        && min_left >= d.cost_minerals + prm.mineral_buffer
                        && gas_left >= d.cost_gas
                })
                .collect();
            // Mix compositions: mostly the best unit, every third the
            // cheapest — swarm filler screens the expensive core.
            let pick = if (army.len() + e.queue.len()) % 3 == 2 {
                affordable.iter().copied().min_by_key(|&u| {
                    let d = &s.data.units[u as usize];
                    (d.cost_minerals + d.cost_gas, u)
                })
            } else {
                affordable.iter().copied().max_by_key(|&u| {
                    let d = &s.data.units[u as usize];
                    (d.cost_minerals + d.cost_gas, u)
                })
            };
            if let Some(pick) = pick {
                let d = &s.data.units[pick as usize];
                min_left -= d.cost_minerals;
                gas_left -= d.cost_gas;
                cmds.push(Command::Train { building: s.id_of(b), unit: pick });
            }
        }

        // ---- attack waves ----
        if army_supply >= prm.attack_supply
            && s.tick.saturating_sub(self.last_attack_tick) >= prm.reattack_interval
        {
            let enemy_start = s.map.starts[(1 - p as usize).min(s.map.starts.len() - 1)];
            cmds.push(Command::AttackMove {
                units: army.clone(),
                target: enemy_start.center(),
                queued: false,
            });
            self.last_attack_tick = s.tick;
        }

        cmds.into_iter().map(|c| (p, c)).collect()
    }

    /// Pick a builder and a site near the HQ, spiral-scanned deterministically.
    /// The scan is mirrored so both players prefer sites on the far side of
    /// their HQ from the enemy — symmetric behavior on a mirrored map.
    fn order_build(&self, s: &State, workers: &[u32], def: DefId, cmds: &mut Vec<Command>) {
        let Some(builder) = self.pick_builder(s, workers) else { return };
        let Some(hq_tile) = self.hq_tile(s) else { return };
        let enemy = s.map.starts[(1 - self.player as usize).min(s.map.starts.len() - 1)];
        let sx: i32 = if enemy.x >= hq_tile.x { 1 } else { -1 };
        let sy: i32 = if enemy.y >= hq_tile.y { 1 } else { -1 };
        for r in 3i32..14 {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    let site = TilePos::new(hq_tile.x + dx * sx, hq_tile.y + dy * sy);
                    if s.valid_building_site(def, site, Some(builder)) {
                        cmds.push(Command::Build {
                            worker: s.id_of(builder),
                            building: def,
                            site,
                            queued: false,
                        });
                        return;
                    }
                }
            }
        }
    }

    /// Extractors go on the nearest free geyser.
    fn order_build_extractor(
        &self,
        s: &State,
        workers: &[u32],
        def: DefId,
        cmds: &mut Vec<Command>,
    ) {
        let Some(builder) = self.pick_builder(s, workers) else { return };
        let Some(hq_tile) = self.hq_tile(s) else { return };
        let hq_pos = hq_tile.center();
        let mut best: Option<(i64, TilePos)> = None;
        for &(origin, _) in &s.map.geysers {
            if s.geyser_at(origin).is_none() {
                continue; // taken or gone
            }
            let d = dist_sq_raw(hq_pos, origin.center());
            if best.map_or(true, |(bd, _)| d < bd) {
                best = Some((d, origin));
            }
        }
        if let Some((_, site)) = best {
            if s.valid_building_site(def, site, Some(builder)) {
                cmds.push(Command::Build { worker: s.id_of(builder), building: def, site, queued: false });
            }
        }
    }

    fn pick_builder(&self, s: &State, workers: &[u32]) -> Option<u32> {
        workers
            .iter()
            .find(|&&w| {
                let e = &s.entities[w as usize];
                e.amount == 0
                    && !matches!(e.order, Order::Build { .. })
                    && !matches!(
                        e.order,
                        Order::Gather { phase: GatherPhase::Mining { .. }, .. }
                    )
            })
            .copied()
    }

    fn hq_tile(&self, s: &State) -> Option<TilePos> {
        s.entities
            .iter()
            .position(|e| {
                e.alive
                    && e.owner == self.player
                    && e.kind == EntityKind::Building
                    && s.data.buildings[e.def as usize].headquarters
            })
            .map(|i| TilePos::of(s.entities[i].pos))
    }
}

fn nearest_mineral(s: &State, pos: FxVec2) -> Option<EntityId> {
    let max = Fx::from_int(40);
    let max_sq = (max.0 as i64) * (max.0 as i64);
    let mut best: Option<(i64, u32)> = None;
    for (j, e) in s.entities.iter().enumerate() {
        if e.alive
            && e.kind == EntityKind::Resource
            && e.def == crate::state::RES_MINERALS
            && e.amount > 0
        {
            let d = dist_sq_raw(pos, e.pos);
            if d <= max_sq && best.map_or(true, |(bd, bj)| (d, j as u32) < (bd, bj)) {
                best = Some((d, j as u32));
            }
        }
    }
    best.map(|(_, j)| s.id_of(j))
}
