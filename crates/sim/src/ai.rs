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
    last_attack_tick: u32,
}

impl Bot {
    pub fn new(player: u8) -> Bot {
        Bot::with(player, Difficulty::Normal)
    }

    pub fn with(player: u8, difficulty: Difficulty) -> Bot {
        Bot { player, difficulty, last_attack_tick: 0 }
    }

    pub fn think(&mut self, s: &State) -> Vec<(u8, Command)> {
        let prm = self.difficulty.params();
        if s.tick % prm.think_interval != 0 {
            return Vec::new();
        }
        let p = self.player;
        let mut cmds: Vec<Command> = Vec::new();

        let worker_def = s.data.unit_tag("fabricator");
        let trooper_def = s.data.unit_tag("trooper");
        let vanguard_def = s.data.unit_tag("vanguard");
        let depot_def = s.data.building_tag("depot");
        let barracks_def = s.data.building_tag("barracks");
        let condenser_def = s.data.building_tag("condenser");
        let forge_def = s.data.building_tag("forge");
        let breaker_def = s.data.unit_tag("breaker");

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
                    } else if e.def == forge_def {
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
            && forge.is_empty()
            && !barracks.is_empty()
            && condenser.is_some()
            && workers.len() >= 12;
        if constructing == 0 && !building_worker_busy {
            if want_depot && minerals >= 100 {
                self.order_build(s, &workers, depot_def, &mut cmds);
            } else if want_condenser && minerals >= 75 {
                self.order_build_extractor(s, &workers, condenser_def, &mut cmds);
            } else if want_barracks && minerals >= 150 {
                self.order_build(s, &workers, barracks_def, &mut cmds);
            } else if want_forge && minerals >= 230 && gas >= 50 {
                self.order_build(s, &workers, forge_def, &mut cmds);
            }
        }

        // ---- army production: vanguards whenever gas allows ----
        let mut gas_left = gas;
        for &b in &barracks {
            let e = &s.entities[b as usize];
            if e.queue.len() < 2 {
                let pick = if gas_left >= 50 { vanguard_def } else { trooper_def };
                let cost = s.data.units[pick as usize].cost_minerals;
                if minerals >= cost + prm.mineral_buffer {
                    if pick == vanguard_def {
                        gas_left -= 50;
                    }
                    cmds.push(Command::Train { building: s.id_of(b), unit: pick });
                }
            }
        }

        // ---- tanks from the forge ----
        let mut gas_after = gas_left;
        for &f in &forge {
            let e = &s.entities[f as usize];
            if e.queue.len() < 2 && minerals >= 150 + prm.mineral_buffer && gas_after >= 100 {
                gas_after -= 100;
                cmds.push(Command::Train { building: s.id_of(f), unit: breaker_def });
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
    fn order_build(&self, s: &State, workers: &[u32], def: DefId, cmds: &mut Vec<Command>) {
        let Some(builder) = self.pick_builder(s, workers) else { return };
        let Some(hq_tile) = self.hq_tile(s) else { return };
        for r in 3i32..14 {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    let site = TilePos::new(hq_tile.x + dx, hq_tile.y + dy);
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
