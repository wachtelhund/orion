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
    /// How long a sighting stays in memory after it leaves vision (0 =
    /// the bot only reacts to what it can literally see right now).
    memory_ticks: u32,
    /// Minimum ticks between re-issuing defense orders.
    react_interval: u32,
    /// Weight production toward counters of what the enemy has shown.
    counter_build: bool,
    /// Pull endangered workers out of harm's way.
    worker_flee: bool,
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
                memory_ticks: 0,
                react_interval: 24 * 3,
                counter_build: false,
                worker_flee: false,
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
                memory_ticks: 24 * 15,
                react_interval: 24 * 2,
                counter_build: true,
                worker_flee: true,
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
                memory_ticks: 24 * 30,
                react_interval: 12,
                counter_build: true,
                worker_flee: true,
            },
        }
    }
}

/// One remembered enemy presence, bucketed to an 8x8-tile cell. Entries
/// only ever come from entities the player could actually see at the time
/// — this is the bot's fog-honest model of "what I know is out there".
#[derive(Clone, Copy)]
struct Sighting {
    pos: FxVec2,
    /// Supply-weighted strength, split by what stands on the ground vs
    /// what flies (the split decides who can even shoot back).
    ground: u32,
    air: u32,
    /// Any enemy structure here (attack targets + "their base is there").
    building: bool,
    tick: u32,
}

pub struct Bot {
    pub player: u8,
    pub difficulty: Difficulty,
    /// Personality: small deterministic offsets to timings/caps so games
    /// against (and between) bots vary run to run.
    pub style: u64,
    last_attack_tick: u32,
    /// The one-time worker scout has been sent.
    scouted: bool,
    /// Decaying sightings, keyed by 8-tile grid cell (kept sorted by cell
    /// for determinism).
    memory: Vec<(i32, Sighting)>,
    last_defense_tick: u32,
    last_flee_tick: u32,
}

impl Bot {
    pub fn new(player: u8) -> Bot {
        Bot::with(player, Difficulty::Normal)
    }

    pub fn with(player: u8, difficulty: Difficulty) -> Bot {
        Bot::with_style(player, difficulty, 0)
    }

    pub fn with_style(player: u8, difficulty: Difficulty, style: u64) -> Bot {
        Bot {
            player,
            difficulty,
            style,
            last_attack_tick: 0,
            scouted: false,
            memory: Vec::new(),
            last_defense_tick: 0,
            last_flee_tick: 0,
        }
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
        // Past 13 minutes: permanent aggression; past 16: total commitment.
        // The map mines out around minute 15 — whatever army remains must
        // decide the game instead of running out the clock.
        if s.tick > 24 * 60 * 13 {
            prm.attack_supply = prm.attack_supply.min(16);
            prm.reattack_interval = 24 * 8;
            prm.mineral_buffer = 0;
        }
        if s.tick > 24 * 60 * 16 {
            prm.attack_supply = 4;
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
        let mut siegers: Vec<u32> = Vec::new();
        let mut casters: Vec<u32> = Vec::new();
        let mut hq: Option<u32> = None;
        let mut deposits = 0usize;
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
                        let d = &s.data.units[e.def as usize];
                        army.push(s.id_of(i as u32));
                        army_supply += d.supply;
                        if d.weapon_siege.is_some() {
                            siegers.push(i as u32);
                        }
                        if d.energy_max > 0 {
                            casters.push(i as u32);
                        }
                    }
                }
                EntityKind::Building => {
                    if e.construction.is_some() {
                        constructing += 1;
                        if s.data.buildings[e.def as usize].deposit {
                            deposits += 1; // an expansion going up counts
                        }
                    } else if s.data.buildings[e.def as usize].headquarters {
                        if hq.is_none() {
                            hq = Some(i as u32);
                        }
                        deposits += 1;
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

        // ---- perception: fog-honest view of the enemy, folded into a
        // decaying memory so scouting information actually gets USED ----
        let cell = |pos: FxVec2| -> i32 {
            (pos.y.floor_int() / 8) * 64 + (pos.x.floor_int() / 8)
        };
        let strength_of = |d: &crate::UnitDef| -> u32 { d.supply * 4 + 2 };
        // Current vision pass: per-cell aggregation of visible enemies.
        let mut seen_now: Vec<(i32, Sighting)> = Vec::new();
        for e in s.entities.iter() {
            if !e.alive || e.owner == p || e.owner == crate::state::NEUTRAL {
                continue;
            }
            if e.kind == EntityKind::Resource {
                continue;
            }
            if !s.fog[p as usize].visible(&s.map, TilePos::of(e.pos)) {
                continue;
            }
            let c = cell(e.pos);
            let (g, a, b) = match e.kind {
                EntityKind::Unit => {
                    let d = &s.data.units[e.def as usize];
                    if d.fly { (0, strength_of(d), false) } else { (strength_of(d), 0, false) }
                }
                EntityKind::Building => (0, 0, true),
                EntityKind::Resource => unreachable!(),
            };
            match seen_now.iter_mut().find(|(cc, _)| *cc == c) {
                Some((_, sg)) => {
                    sg.ground += g;
                    sg.air += a;
                    sg.building |= b;
                }
                None => seen_now.push((
                    c,
                    Sighting { pos: e.pos, ground: g, air: a, building: b, tick: s.tick },
                )),
            }
        }
        // Merge: fresh sightings replace their cell; a spot we can SEE is
        // the ground truth (empty = forget the ghost); unseen sightings
        // decay after memory_ticks.
        self.memory.retain(|(c, sg)| {
            !seen_now.iter().any(|(nc, _)| nc == c)
                && !s.fog[p as usize].visible(&s.map, TilePos::of(sg.pos))
                && s.tick.saturating_sub(sg.tick) < prm.memory_ticks.max(1)
        });
        self.memory.extend(seen_now.iter().cloned());
        self.memory.sort_unstable_by_key(|(c, _)| *c);

        // What the enemy has, as far as we know.
        let known_air: u32 = self.memory.iter().map(|(_, sg)| sg.air).sum();
        let known_ground: u32 = self.memory.iter().map(|(_, sg)| sg.ground).sum();
        let known_army = known_air + known_ground;

        // ---- threat evaluation: enemy strength near each of our bases ----
        let mut base_threat: Option<(FxVec2, u32, u32)> = None; // pos, ground, air
        let radius_sq = (crate::Fx::from_int(16).0 as i64).pow(2);
        for e in s.entities.iter() {
            if !(e.alive
                && e.owner == p
                && e.kind == EntityKind::Building
                && s.data.buildings[e.def as usize].deposit)
            {
                continue;
            }
            let (mut tg, mut ta) = (0u32, 0u32);
            let mut tpos = None;
            for (_, sg) in &self.memory {
                if crate::fixed::dist_sq_raw(e.pos, sg.pos) <= radius_sq {
                    tg += sg.ground;
                    ta += sg.air;
                    tpos.get_or_insert(sg.pos);
                }
            }
            if tg + ta > 0 {
                let worse = base_threat.map_or(true, |(_, bg, ba)| tg + ta > bg + ba);
                if worse {
                    base_threat = Some((tpos.unwrap(), tg, ta));
                }
            }
        }

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
        // Expand to the natural on maps that have one, once the main is
        // established (SC-style second base ~13 workers).
        let hq_def = s.data.hq_of_race(race);
        let hq_cost = s.data.buildings[hq_def as usize].cost_minerals;
        let want_expand =
            !s.map.expansions.is_empty() && deposits < 2 && workers.len() >= 13;
        if constructing == 0 && !building_worker_busy {
            if want_expand && minerals >= hq_cost {
                self.order_build_expansion(s, &workers, hq_def, &mut cmds);
            } else if want_depot && minerals >= depot_cost {
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

        // ---- casters: one at 16 army supply, a second at 28 ----
        let mut gas_left = gas;
        let mut min_left = minerals;
        let caster_cap = if army_supply >= 28 {
            2
        } else {
            (army_supply >= 16) as usize
        };
        if casters.len() < caster_cap {
            'caster: for &b in barracks.iter().chain(forge.iter()) {
                let e = &s.entities[b as usize];
                if e.queue.len() >= 2 {
                    continue;
                }
                for &u in &s.data.buildings[e.def as usize].trains {
                    let d = &s.data.units[u as usize];
                    if d.energy_max > 0
                        && s.requirement_met(p, d.requires)
                        && min_left >= d.cost_minerals
                        && gas_left >= d.cost_gas
                    {
                        min_left -= d.cost_minerals;
                        gas_left -= d.cost_gas;
                        cmds.push(Command::Train { building: s.id_of(b), unit: u });
                        break 'caster;
                    }
                }
            }
        }

        // ---- army production: each production building trains the most
        // expensive combat unit it can afford right now ----
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
                        && d.energy_max == 0 // casters are trained separately
                        && s.requirement_met(p, d.requires)
                        && min_left >= d.cost_minerals + prm.mineral_buffer
                        && gas_left >= d.cost_gas
                })
                .collect();
            // Score picks against what the enemy has SHOWN us: anti-air
            // when they fly (mandatory if we own zero AA), splash into
            // ground mass, value otherwise. Every third unit stays cheap
            // filler so the swarm screens the core.
            let own_aa: u32 = army
                .iter()
                .filter_map(|id| s.get(*id))
                .filter(|e| {
                    s.data.units[e.def as usize]
                        .weapon
                        .as_ref()
                        .map_or(false, |w| w.air)
                })
                .count() as u32;
            let need_aa = prm.counter_build && known_air > 0 && own_aa * 8 < known_air;
            let score = |u: DefId| -> i64 {
                let d = &s.data.units[u as usize];
                let mut v = (d.cost_minerals + d.cost_gas) as i64;
                if prm.counter_build {
                    let hits_air = d.weapon.as_ref().map_or(false, |w| w.air);
                    if known_air > known_ground && hits_air {
                        v += 220;
                    }
                    if need_aa {
                        v += if hits_air { 800 } else { -800 };
                    }
                    let splashy = d
                        .weapon_siege
                        .as_ref()
                        .map_or(false, |w| w.splash.0 > 0)
                        || d.weapon.as_ref().map_or(false, |w| w.splash.0 > 0);
                    if known_ground > known_air.saturating_mul(3) && splashy {
                        v += 150;
                    }
                }
                v
            };
            let pick = if (army.len() + e.queue.len()) % 3 == 2 && !need_aa {
                affordable.iter().copied().min_by_key(|&u| {
                    let d = &s.data.units[u as usize];
                    (d.cost_minerals + d.cost_gas, u)
                })
            } else {
                affordable.iter().copied().max_by_key(|&u| (score(u), u))
            };
            if let Some(pick) = pick {
                let d = &s.data.units[pick as usize];
                min_left -= d.cost_minerals;
                gas_left -= d.cost_gas;
                cmds.push(Command::Train { building: s.id_of(b), unit: pick });
            }
        }

        // ---- attack posture: strength-aware, not a blind timer. Attack
        // when we outmatch what we KNOW the enemy fields (memory-fed by
        // scouting), never while the home base is under threat; target the
        // nearest remembered enemy structure ----
        let own_army_strength: u32 = army
            .iter()
            .filter_map(|id| s.get(*id))
            .map(|e| strength_of(&s.data.units[e.def as usize]))
            .sum();
        let late_game = s.tick > 24 * 60 * 13;
        let outmatch = own_army_strength >= known_army + known_army / 5;
        if army_supply >= prm.attack_supply
            && base_threat.is_none()
            && (outmatch || known_army == 0 || late_game)
            && s.tick.saturating_sub(self.last_attack_tick) >= prm.reattack_interval
        {
            let enemy_start = s.map.starts[(1 - p as usize).min(s.map.starts.len() - 1)];
            let hq_pos = hq.map(|i| s.entities[i as usize].pos);
            let target = self
                .memory
                .iter()
                .filter(|(_, sg)| sg.building)
                .min_by_key(|(c, sg)| {
                    let d = hq_pos
                        .map(|hp| crate::fixed::dist_sq_raw(hp, sg.pos))
                        .unwrap_or(i64::MAX);
                    (d, *c)
                })
                .map(|(_, sg)| sg.pos)
                .unwrap_or_else(|| enemy_start.center());
            cmds.push(Command::AttackMove { units: army.clone(), target, queued: false });
            self.last_attack_tick = s.tick;
        }
        // Retreat: mid-push discovery that we're badly outmatched sends the
        // army home instead of feeding it away (late game excepted — the
        // map is mined out, there is nothing to save up for).
        else if !late_game
            && self.last_attack_tick > 0
            && s.tick.saturating_sub(self.last_attack_tick) < prm.reattack_interval
            && known_army > own_army_strength * 2
            && base_threat.is_none()
        {
            if let Some(hq_idx) = hq {
                let home = s.entities[hq_idx as usize].pos;
                let far: Vec<EntityId> = army
                    .iter()
                    .copied()
                    .filter(|id| {
                        s.get(*id).map_or(false, |e| {
                            crate::fixed::dist_sq_raw(e.pos, home)
                                > (crate::Fx::from_int(24).0 as i64).pow(2)
                        })
                    })
                    .collect();
                if !far.is_empty() {
                    cmds.push(Command::Move { units: far, target: home, queued: false });
                    // Back off the next push a little.
                    self.last_attack_tick = s.tick;
                }
            }
        }

        // ---- base defense: the reported hole. A threat at home gets an
        // interception force made of units that can actually SHOOT it —
        // air harass answered by anti-air, not by melee staring upward ----
        if let Some((tpos, tg, ta)) = base_threat {
            if s.tick.saturating_sub(self.last_defense_tick) >= prm.react_interval {
                let mut defenders: Vec<(i64, EntityId, u32)> = Vec::new();
                for &id in &army {
                    let Some(e) = s.get(id) else { continue };
                    let d = &s.data.units[e.def as usize];
                    let hits_air = d.weapon.as_ref().map_or(false, |w| w.air);
                    let hits_ground = d.weapon.is_some() || d.weapon_siege.is_some();
                    // Useful against this particular threat mix?
                    let useful = (ta > 0 && hits_air) || (tg > 0 && hits_ground);
                    if useful {
                        defenders.push((
                            crate::fixed::dist_sq_raw(e.pos, tpos),
                            id,
                            strength_of(d),
                        ));
                    }
                }
                defenders.sort_unstable_by_key(|(d, id, _)| (*d, id.idx));
                // Send enough to outmatch the threat by 25%.
                let want = (tg + ta) + (tg + ta) / 4;
                let mut sent = 0u32;
                let mut squad: Vec<EntityId> = Vec::new();
                for (_, id, st) in defenders {
                    if sent >= want {
                        break;
                    }
                    squad.push(id);
                    sent += st;
                }
                if !squad.is_empty() {
                    cmds.push(Command::AttackMove { units: squad, target: tpos, queued: false });
                    self.last_defense_tick = s.tick;
                }
                // Workers flee when the threat outguns what we sent.
                if prm.worker_flee
                    && sent < tg + ta
                    && s.tick.saturating_sub(self.last_flee_tick) >= prm.react_interval * 2
                {
                    let danger_sq = (crate::Fx::from_int(9).0 as i64).pow(2);
                    let fleeing: Vec<EntityId> = workers
                        .iter()
                        .filter(|&&w| {
                            crate::fixed::dist_sq_raw(s.entities[w as usize].pos, tpos)
                                <= danger_sq
                        })
                        .map(|&w| s.id_of(w))
                        .collect();
                    if !fleeing.is_empty() {
                        // Run directly away from the threat, toward map center.
                        if let Some(hq_idx) = hq {
                            let hp = s.entities[hq_idx as usize].pos;
                            let away = FxVec2::new(
                                hp.x + (hp.x - tpos.x),
                                hp.y + (hp.y - tpos.y),
                            );
                            let away = s.map.clamp_pos(away);
                            cmds.push(Command::Move {
                                units: fleeing,
                                target: away,
                                queued: false,
                            });
                            self.last_flee_tick = s.tick;
                        }
                    }
                }
            }
        }

        // ---- combat micro: siege positioning + storms; fog-honest (only
        // reacts to enemies the player can actually see) ----
        let visible_enemies: Vec<(FxVec2, bool)> = s
            .entities
            .iter()
            .filter(|e| {
                e.alive
                    && e.owner != p
                    && e.owner != crate::state::NEUTRAL
                    && e.kind != EntityKind::Resource
                    && s.fog[p as usize].visible(&s.map, TilePos::of(e.pos))
            })
            .map(|e| (e.pos, e.kind == EntityKind::Unit))
            .collect();

        // Siege micro: deploy inside working range, pack up when the field
        // is empty. Hysteresis (+2 tiles on the way out) avoids flapping.
        for &t in &siegers {
            let e = &s.entities[t as usize];
            if e.transform != 0 {
                continue;
            }
            let w = s.data.units[e.def as usize].weapon_siege.as_ref().unwrap();
            let max_sq = (w.range.0 as i64) * (w.range.0 as i64);
            let min_sq = (w.min_range.0 as i64) * (w.min_range.0 as i64);
            let out_r = w.range + Fx::from_int(2);
            let out_sq = (out_r.0 as i64) * (out_r.0 as i64);
            let in_band = visible_enemies.iter().any(|&(pos, _)| {
                let d = dist_sq_raw(e.pos, pos);
                d <= max_sq && d > min_sq
            });
            let any_near = visible_enemies
                .iter()
                .any(|&(pos, _)| dist_sq_raw(e.pos, pos) <= out_sq);
            if !e.sieged && in_band {
                cmds.push(Command::Siege { units: vec![s.id_of(t)] });
            } else if e.sieged && !any_near {
                cmds.push(Command::Siege { units: vec![s.id_of(t)] });
            }
        }

        // Storm the biggest visible clump in cast range: at least 3 enemy
        // units, minus penalty for own units in the blast, and never on top
        // of an active storm (they no longer stack).
        let storm_r_sq = (crate::STORM_RADIUS.0 as i64) * (crate::STORM_RADIUS.0 as i64);
        let reach = crate::STORM_CAST_RANGE + Fx::from_int(3);
        let reach_sq = (reach.0 as i64) * (reach.0 as i64);
        for &c in &casters {
            let e = &s.entities[c as usize];
            if (e.energy as u16) < crate::STORM_COST {
                continue;
            }
            if matches!(e.order, Order::Cast { .. }) {
                continue;
            }
            let own_units: Vec<FxVec2> = s
                .entities
                .iter()
                .filter(|o| o.alive && o.owner == p && o.kind == EntityKind::Unit)
                .map(|o| o.pos)
                .collect();
            let mut best: Option<(i32, i64, FxVec2)> = None;
            for &(pos, is_unit) in &visible_enemies {
                if !is_unit || dist_sq_raw(e.pos, pos) > reach_sq {
                    continue;
                }
                if s.storms.iter().any(|st| dist_sq_raw(st.pos, pos) <= storm_r_sq) {
                    continue;
                }
                let enemies = visible_enemies
                    .iter()
                    .filter(|&&(q, u)| u && dist_sq_raw(pos, q) <= storm_r_sq)
                    .count() as i32;
                let friendlies = own_units
                    .iter()
                    .filter(|&&q| dist_sq_raw(pos, q) <= storm_r_sq)
                    .count() as i32;
                let score = enemies - 2 * friendlies;
                if score >= 3 {
                    let d = dist_sq_raw(e.pos, pos);
                    if best.map_or(true, |(bs, bd, _)| (score, -(d as i64)) > (bs, -(bd as i64)))
                    {
                        best = Some((score, d, pos));
                    }
                }
            }
            if let Some((_, _, target)) = best {
                cmds.push(Command::Cast { caster: s.id_of(c), target });
            }
        }

        // ---- one-time worker scout (~80s, style-jittered): peek at the
        // enemy main, come home ----
        if !self.scouted
            && s.tick >= 24 * 80 + (self.style % 13) as u32 * 24
            && workers.len() >= 10
        {
            if let Some(&w) = mineral_workers.last() {
                let enemy_start = s.map.starts[(1 - p as usize).min(s.map.starts.len() - 1)];
                cmds.push(Command::Move {
                    units: vec![s.id_of(w)],
                    target: enemy_start.center(),
                    queued: false,
                });
                if let Some(hq_idx) = hq {
                    cmds.push(Command::Move {
                        units: vec![s.id_of(w)],
                        target: s.entities[hq_idx as usize].pos,
                        queued: true,
                    });
                }
                self.scouted = true;
            }
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
        // Mirroring a REGION: the flipped scan must also shift the origin
        // by the footprint, or the two players' candidate sites are off by
        // (fw-1, fh-1) and the openings diverge.
        let (fw, fh) = s.data.buildings[def as usize].footprint;
        let ox = if sx < 0 { -(fw - 1) } else { 0 };
        let oy = if sy < 0 { -(fh - 1) } else { 0 };
        // Livability margins: never build touching the HQ ring (deposit
        // approaches stay open) and keep 2 tiles clear of live resources
        // (mining lanes). The bot once walled its own Hive in and starved.
        let hq_e = s
            .entities
            .iter()
            .find(|e| {
                e.alive
                    && e.owner == self.player
                    && e.kind == EntityKind::Building
                    && s.data.buildings[e.def as usize].headquarters
            });
        let hq_zone = hq_e.map(|e| {
            let (hw, hh) = s.data.buildings[e.def as usize].footprint;
            (s.footprint_origin(e.def, e.pos), hw, hh)
        });
        let site_ok = |site: TilePos| -> bool {
            if let Some((ho, hw, hh)) = hq_zone {
                // Overlap test of the site footprint against HQ+1 ring.
                if site.x < ho.x + hw + 1
                    && site.x + fw > ho.x - 1
                    && site.y < ho.y + hh + 1
                    && site.y + fh > ho.y - 1
                {
                    return false;
                }
            }
            for e in &s.entities {
                if e.alive && e.kind == EntityKind::Resource && e.amount > 0 {
                    let rt = TilePos::of(e.pos);
                    let cx = rt.x.clamp(site.x, site.x + fw - 1);
                    let cy = rt.y.clamp(site.y, site.y + fh - 1);
                    if (cx - rt.x).abs() <= 2 && (cy - rt.y).abs() <= 2 {
                        return false;
                    }
                }
            }
            true
        };
        for r in 3i32..14 {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    let site = TilePos::new(hq_tile.x + dx * sx + ox, hq_tile.y + dy * sy + oy);
                    if site_ok(site) && s.valid_building_site(def, site, Some(builder)) {
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

    /// Expansion HQ on the nearest free suggested slot.
    fn order_build_expansion(
        &self,
        s: &State,
        workers: &[u32],
        hq_def: DefId,
        cmds: &mut Vec<Command>,
    ) {
        let Some(builder) = self.pick_builder(s, workers) else { return };
        let Some(hq_tile) = self.hq_tile(s) else { return };
        let mut slots: Vec<TilePos> = s.map.expansions.clone();
        slots.sort_by_key(|t| {
            ((t.x - hq_tile.x).pow(2) + (t.y - hq_tile.y).pow(2), t.x, t.y)
        });
        for site in slots {
            if s.valid_building_site(hq_def, site, Some(builder)) {
                cmds.push(Command::Build {
                    worker: s.id_of(builder),
                    building: hq_def,
                    site,
                    queued: false,
                });
                return;
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
