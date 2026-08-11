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
    /// Guard/strike squad split + staging (off = old whole-army waves).
    use_squads: bool,
    /// Engagement score (x100) required to launch a push.
    attack_score: i64,
    /// Score below which a push comes home.
    retreat_score: i64,
    /// Ranged units focus-fire the weakest target in range.
    focus_fire: bool,
    /// Build the research lab and buy weapon/armor upgrades from surplus.
    use_research: bool,
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
                use_squads: false,
                attack_score: 0,
                retreat_score: 0,
                focus_fire: false,
                use_research: false,
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
                react_interval: 24 * 4,
                counter_build: true,
                worker_flee: true,
                use_squads: true,
                attack_score: 95, // optimistic: pushes near-even fights
                retreat_score: 45,
                focus_fire: true,
                use_research: true,
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
                react_interval: 24 * 2,
                counter_build: true,
                worker_flee: true,
                use_squads: true,
                attack_score: 110, // picks winning fights
                retreat_score: 60,
                focus_fire: true,
                use_research: true,
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
    /// REAL aggregate combat profile of what was seen here — same scale
    /// as our own side's evaluation, so engagement scores mean something.
    power: crate::ai_combat::SidePower,
    /// Any enemy structure here (attack targets + "their base is there").
    building: bool,
    tick: u32,
}

pub struct Bot {
    pub player: u8,
    pub difficulty: Difficulty,
    /// A/B benchmark switch: true disables perception/memory/defense/
    /// posture/counter-build, reverting to the old open-loop bot. Used by
    /// the arena example to MEASURE how much the intelligence is worth.
    pub legacy: bool,
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
    /// Stuck-builder watchdog: (worker idx, first tick seen traveling).
    /// A Build order to an unreachable site never completes and would
    /// otherwise freeze ALL construction via the busy-builder gate.
    builder_watch: Option<(u32, u32)>,
    /// Sites a builder provably could not reach — never picked again.
    pub bad_sites: Vec<TilePos>,
    /// Home guard: units that stay near the base and intercept threats.
    last_score: i64,
    last_known: i64,
    /// One concentrated army ball — splitting into home-guard + strike
    /// lost fights by defeat-in-detail (measured in the arena).
    ball: BallState,
    /// A base threat existed on the previous think (repel detection).
    had_threat: bool,
    last_rally_tick: u32,
    last_focus_tick: u32,
}

#[derive(Clone, Copy, PartialEq)]
enum BallState {
    /// Assembled at the ramp anchor, letting attackers walk into us.
    Hold,
    /// Committed to an attack across the map.
    Push,
    /// Coming home to the anchor after a bad fight.
    Retreat,
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
            legacy: false,
            style,
            last_attack_tick: 0,
            scouted: false,
            memory: Vec::new(),
            last_defense_tick: 0,
            last_flee_tick: 0,
            builder_watch: None,
            bad_sites: Vec::new(),
            last_score: 0,
            last_known: 0,
            ball: BallState::Hold,
            had_threat: false,
            last_rally_tick: 0,
            last_focus_tick: 0,
        }
    }

    // Debug taps for the probe example (cheap, read-only).
    pub fn dbg_guard(&self) -> usize {
        0
    }
    pub fn dbg_strike(&self) -> usize {
        matches!(self.ball, BallState::Push) as usize
    }
    pub fn dbg_pushing(&self) -> bool {
        matches!(self.ball, BallState::Push)
    }
    pub fn dbg_score(&self) -> i64 {
        self.last_score
    }
    pub fn dbg_known(&self) -> i64 {
        self.last_known
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
        // Research lab: whichever building can buy upgrades. May coincide
        // with the tier-1 producer (Ferron's refit does both jobs).
        let lab_def = s
            .data
            .buildings
            .iter()
            .position(|b| b.race == race && !b.researches.is_empty())
            .map(|i| i as DefId);

        // ---- census ----
        let mut workers: Vec<u32> = Vec::new();
        let mut idle_workers: Vec<u32> = Vec::new();
        let mut mineral_workers: Vec<u32> = Vec::new();
        let mut gas_workers = 0usize;
        let mut gas_worker_ids: Vec<u32> = Vec::new();
        let mut army: Vec<EntityId> = Vec::new();
        let mut army_supply = 0u32;
        let mut siegers: Vec<u32> = Vec::new();
        let mut casters: Vec<u32> = Vec::new();
        let mut shields: Vec<u32> = Vec::new();
        let mut burrowers: Vec<u32> = Vec::new();
        let mut hero_unit: Option<u32> = None;
        let mut has_hero = false;
        let mut hq: Option<u32> = None;
        let mut deposits = 0usize;
        let mut barracks: Vec<u32> = Vec::new();
        let mut forge: Vec<u32> = Vec::new();
        let mut condenser: Option<u32> = None;
        let mut constructing = 0u32;
        let mut building_worker_busy = false;
        let mut traveling_builder: Option<u32> = None;
        let mut lab_exists = false;
        let mut idle_lab: Option<u32> = None;
        for (i, e) in s.entities.iter().enumerate() {
            if !e.alive || e.owner != p {
                continue;
            }
            if e.kind == EntityKind::Building
                && !s.data.buildings[e.def as usize].researches.is_empty()
            {
                lab_exists = true;
                if e.construction.is_none() && e.research.is_none() && idle_lab.is_none() {
                    idle_lab = Some(i as u32);
                }
            }
            match e.kind {
                EntityKind::Unit => {
                    if e.def == worker_def {
                        workers.push(i as u32);
                        match e.order {
                            Order::Idle => idle_workers.push(i as u32),
                            Order::Build { .. } => {
                                building_worker_busy = true;
                                traveling_builder = Some(i as u32);
                            }
                            Order::Gather { resource, .. } => {
                                let gas = s
                                    .get(resource)
                                    .is_some_and(|r| r.kind == EntityKind::Building);
                                if gas {
                                    gas_workers += 1;
                                    gas_worker_ids.push(i as u32);
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
                        if d.shield_aura.is_some() {
                            shields.push(i as u32);
                        }
                        if d.burrow {
                            burrowers.push(i as u32);
                        }
                        if d.hero {
                            hero_unit = Some(i as u32);
                            has_hero = true;
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
        // Sudden death: two turtling bots on a multi-base map can rebuild
        // faster than they kill (thornwood Hard mirrors ran out the clock).
        // Past 15 minutes the macro tap closes — no new construction, no
        // replacement workers — so the armies decide it.
        let sudden_death = s.tick > 24 * 60 * 15;

        // ---- perception: fog-honest view of the enemy, folded into a
        // decaying memory so scouting information actually gets USED ----
        let legacy = self.legacy;
        let cell = |pos: FxVec2| -> i32 {
            (pos.y.floor_int() / 8) * 64 + (pos.x.floor_int() / 8)
        };
        // Current vision pass: per-cell aggregation of visible enemies.
        let mut seen_now: Vec<(i32, Sighting)> = Vec::new();
        for e in s.entities.iter() {
            if !e.alive || e.owner == p || e.owner == crate::state::NEUTRAL {
                continue;
            }
            if e.kind == EntityKind::Resource || e.burrowed {
                continue;
            }
            if !s.fog[p as usize].visible(&s.map, TilePos::of(e.pos)) {
                continue;
            }
            let c = cell(e.pos);
            let mut power = crate::ai_combat::SidePower::default();
            let mut building = false;
            match e.kind {
                EntityKind::Unit => {
                    let lvl = s.players[e.owner as usize].weapons_level as u32;
                    power.add_unit(&s.data.units[e.def as usize], e.hp, lvl);
                }
                EntityKind::Building => {
                    building = true;
                    // Tech inference: a structure that trains flyers means
                    // air is COMING — pretend one wing already exists so
                    // counter-build reacts before the first one flies.
                    let b = &s.data.buildings[e.def as usize];
                    if let Some(&u) =
                        b.trains.iter().find(|&&u| s.data.units[u as usize].fly)
                    {
                        let d = &s.data.units[u as usize];
                        power.add_unit(d, d.hp, 0);
                    }
                }
                EntityKind::Resource => unreachable!(),
            }
            match seen_now.iter_mut().find(|(cc, _)| *cc == c) {
                Some((_, sg)) => {
                    sg.power.dps_ground += power.dps_ground;
                    sg.power.dps_air += power.dps_air;
                    sg.power.hp_ground += power.hp_ground;
                    sg.power.hp_air += power.hp_air;
                    sg.building |= building;
                }
                None => seen_now.push((
                    c,
                    Sighting { pos: e.pos, power, building, tick: s.tick },
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

        // What the enemy has, as far as we know (real combat profile).
        let mut known_power = crate::ai_combat::SidePower::default();
        for (_, sg) in &self.memory {
            known_power.dps_ground += sg.power.dps_ground;
            known_power.dps_air += sg.power.dps_air;
            known_power.hp_ground += sg.power.hp_ground;
            known_power.hp_air += sg.power.hp_air;
        }
        let known_air: u32 = (known_power.hp_air / 12).max(0) as u32;
        let known_ground: u32 = (known_power.hp_ground / 12).max(0) as u32;
        let known_army = known_air + known_ground;

        // ---- threat evaluation: enemy strength near each of our bases ----
        let mut base_threat: Option<(FxVec2, crate::ai_combat::SidePower)> = None;
        let radius_sq = (crate::Fx::from_int(12).0 as i64).pow(2);
        for e in s.entities.iter() {
            if !(e.alive
                && e.owner == p
                && e.kind == EntityKind::Building
                && s.data.buildings[e.def as usize].deposit)
            {
                continue;
            }
            let mut tp = crate::ai_combat::SidePower::default();
            let mut tpos = None;
            for (_, sg) in &self.memory {
                if sg.building {
                    continue; // enemy structures near us are not a raid
                }
                if crate::fixed::dist_sq_raw(e.pos, sg.pos) <= radius_sq {
                    tp.dps_ground += sg.power.dps_ground;
                    tp.dps_air += sg.power.dps_air;
                    tp.hp_ground += sg.power.hp_ground;
                    tp.hp_air += sg.power.hp_air;
                    tpos.get_or_insert(sg.pos);
                }
            }
            if tp.hp_ground + tp.hp_air > 0 {
                let worse = base_threat
                    .as_ref()
                    .map_or(true, |(_, bp)| tp.hp_ground + tp.hp_air > bp.hp_ground + bp.hp_air);
                if worse {
                    base_threat = Some((tpos.unwrap(), tp));
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

        // ---- staff the extractor to demand, not to a fixed quota ----
        // A gas worker is a mineral worker you gave up. Races whose army is
        // mineral-heavy (measured: Ferron banking 1300+ gas at 50 minerals
        // while fielding half of VC's army) must pull workers back off gas
        // when nothing spends it, and restaff once the bank drains.
        if let Some(c) = condenser {
            let overflowing = gas >= 400 && gas >= minerals * 3;
            if overflowing && !gas_worker_ids.is_empty() {
                if let Some(&w) = gas_worker_ids.first() {
                    if let Some(res) = nearest_mineral(s, s.entities[w as usize].pos) {
                        cmds.push(Command::Gather {
                            units: vec![s.id_of(w)],
                            resource: res,
                            queued: false,
                        });
                    }
                }
            } else if !overflowing && gas_workers < prm.gas_workers {
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
            if !sudden_death
                && workers.len() < prm.max_workers
                && hq_e.queue.is_empty()
                && minerals >= 50
            {
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
        let want_lab = prm.use_research
            && lab_def.is_some()
            && !lab_exists
            && condenser.is_some()
            && army_supply >= 10
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
        // Stuck-builder watchdog: a builder still TRAVELING while nothing
        // is under construction, for 15s straight, is walking to a site it
        // will never reach — stop it or the busy gate freezes the macro
        // forever (found on caverns: unreachable depot pocket, seat froze
        // at 2 depots and lost every mirror game).
        if constructing == 0 {
            match (traveling_builder, self.builder_watch) {
                (Some(w), Some((pw, since))) if w == pw => {
                    if s.tick.saturating_sub(since) > 24 * 15 {
                        if let Order::Build { site, .. } = s.entities[w as usize].order {
                            if !self.bad_sites.contains(&site) {
                                self.bad_sites.push(site);
                                if self.bad_sites.len() > 16 {
                                    self.bad_sites.remove(0);
                                }
                            }
                        }
                        cmds.push(Command::Stop { units: vec![s.id_of(w)] });
                        self.builder_watch = None;
                    }
                }
                (Some(w), _) => self.builder_watch = Some((w, s.tick)),
                (None, _) => self.builder_watch = None,
            }
        } else {
            self.builder_watch = None;
        }
        if constructing == 0 && !building_worker_busy && !sudden_death {
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
            } else if want_lab {
                let ld = lab_def.unwrap();
                let l = &s.data.buildings[ld as usize];
                if minerals >= l.cost_minerals + 50 && gas >= l.cost_gas {
                    self.order_build(s, &workers, ld, &mut cmds);
                }
            }
        }

        // ---- research: turn surplus into permanent weapon/armor levels.
        // Cheapest first (weapons+1 / armor+1 before the +2 tiers); one
        // lab only so two labs can't both start the same upgrade.
        if prm.use_research {
            if let Some(lb) = idle_lab {
                let mut order: Vec<u8> =
                    s.data.buildings[s.entities[lb as usize].def as usize].researches.clone();
                order.sort_by_key(|&r| {
                    let rd = &s.data.research[r as usize];
                    (rd.cost_minerals + rd.cost_gas, r)
                });
                for r in order {
                    let rd = &s.data.research[r as usize];
                    if s.players[p as usize].research_done[r as usize] {
                        continue;
                    }
                    if let Some(pre) = rd.requires {
                        if !s.players[p as usize].research_done[pre as usize] {
                            continue;
                        }
                    }
                    // Only from genuine surplus — units come first.
                    if minerals >= rd.cost_minerals + prm.mineral_buffer + 60
                        && gas >= rd.cost_gas + 50
                    {
                        cmds.push(Command::Research { building: s.id_of(lb), research: r });
                    }
                    break;
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
                        && !d.hero
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

        // ---- hero: one per game, once the mid-game economy can carry it ----
        if !has_hero
            && s.tick > 24 * 60 * 7
            && min_left >= 300
            && gas_left >= 150
            && prm.use_squads
        {
            'hero: for &b in barracks.iter().chain(forge.iter()) {
                let e = &s.entities[b as usize];
                if e.queue.len() >= 2 {
                    continue;
                }
                for &u in &s.data.buildings[e.def as usize].trains {
                    let d = &s.data.units[u as usize];
                    if d.hero && s.requirement_met(p, d.requires) {
                        min_left -= d.cost_minerals;
                        gas_left -= d.cost_gas;
                        cmds.push(Command::Train { building: s.id_of(b), unit: u });
                        break 'hero;
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
            let need_aa =
                !legacy && prm.counter_build && known_air > 0 && own_aa * 8 < known_air;
            let score = |u: DefId| -> i64 {
                let d = &s.data.units[u as usize];
                let mut v = (d.cost_minerals + d.cost_gas) as i64;
                if prm.counter_build && !legacy {
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

        // ---- the army ball: ONE concentrated force with a state machine.
        // Hold at the ramp anchor (attackers walk into the ball), push
        // across the map when the fight math wins, retreat when it turns.
        // Splitting the army lost fights by defeat-in-detail — measured. ----
        let anchor = self.defense_anchor(s);
        let army_ids: Vec<u32> = army.iter().map(|id| id.idx).collect();
        let army_power = crate::ai_combat::power_of_units(
            s,
            &army_ids,
            s.players[p as usize].weapons_level as u32,
        );
        let fight_score = crate::ai_combat::engagement_score(army_power, known_power);
        self.last_score = fight_score;
        self.last_known = known_power.hp_ground + known_power.hp_air;
        let late_game = s.tick > 24 * 60 * 13;
        let held_back =
            s.tick.saturating_sub(self.last_attack_tick) >= prm.reattack_interval * 2;

        if prm.use_squads && !legacy {
            match self.ball {
                BallState::Hold => {
                    // Home threat: engage it when the math says we win or
                    // it reaches our workers — otherwise stay concentrated
                    // at the choke and make them come to us.
                    if let Some((tpos, them)) = base_threat {
                        let vs = crate::ai_combat::engagement_score(army_power, them);
                        let urgent = hq
                            .map(|i| {
                                crate::fixed::dist_sq_raw(
                                    s.entities[i as usize].pos,
                                    tpos,
                                ) <= (crate::Fx::from_int(9).0 as i64).pow(2)
                            })
                            .unwrap_or(false);
                        if (vs >= 80 || urgent)
                            && s.tick.saturating_sub(self.last_defense_tick)
                                >= prm.react_interval
                            && !army.is_empty()
                        {
                            // Only units that can shoot this threat mix.
                            let squad: Vec<EntityId> = army
                                .iter()
                                .copied()
                                .filter(|id| {
                                    let Some(e) = s.get(*id) else { return false };
                                    let d = &s.data.units[e.def as usize];
                                    let aa = d.weapon.as_ref().map_or(false, |w| w.air);
                                    let ag =
                                        d.weapon.is_some() || d.weapon_siege.is_some();
                                    (them.hp_air > 0 && aa)
                                        || (them.hp_ground > 0 && ag)
                                })
                                .collect();
                            if !squad.is_empty() {
                                cmds.push(Command::AttackMove {
                                    units: squad,
                                    target: tpos,
                                    queued: false,
                                });
                                self.last_defense_tick = s.tick;
                            }
                        }
                        // Workers flee only when the whole ball loses.
                        if prm.worker_flee
                            && vs < 60
                            && s.tick.saturating_sub(self.last_flee_tick)
                                >= prm.react_interval * 2
                        {
                            let danger_sq = (crate::Fx::from_int(9).0 as i64).pow(2);
                            let fleeing: Vec<EntityId> = workers
                                .iter()
                                .filter(|&&w| {
                                    crate::fixed::dist_sq_raw(
                                        s.entities[w as usize].pos,
                                        tpos,
                                    ) <= danger_sq
                                })
                                .map(|&w| s.id_of(w))
                                .collect();
                            if !fleeing.is_empty() {
                                if let Some(hq_idx) = hq {
                                    let hp = s.entities[hq_idx as usize].pos;
                                    let away = s.map.clamp_pos(FxVec2::new(
                                        hp.x + (hp.x - tpos.x),
                                        hp.y + (hp.y - tpos.y),
                                    ));
                                    cmds.push(Command::Move {
                                        units: fleeing,
                                        target: away,
                                        queued: false,
                                    });
                                    self.last_flee_tick = s.tick;
                                }
                            }
                        }
                    } else {
                        // No threat: keep the ball assembled at the anchor
                        // (new units walk there instead of loitering).
                        if s.tick.saturating_sub(self.last_rally_tick) >= 24 * 4 {
                            let scattered: Vec<EntityId> = army
                                .iter()
                                .copied()
                                .filter(|id| {
                                    s.get(*id).map_or(false, |e| {
                                        crate::fixed::dist_sq_raw(e.pos, anchor)
                                            > (crate::Fx::from_int(8).0 as i64).pow(2)
                                    })
                                })
                                .collect();
                            if scattered.len() > 1 {
                                cmds.push(Command::AttackMove {
                                    units: scattered,
                                    target: anchor,
                                    queued: false,
                                });
                                self.last_rally_tick = s.tick;
                            }
                        }
                        // Launch: winning math (or ignorance, or timing
                        // pressure) + supply threshold. A wave we just
                        // repelled opens a counter-window: the enemy is at
                        // its weakest right after its push dies.
                        let repelled = self.had_threat;
                        let go = fight_score >= prm.attack_score
                            || (repelled && fight_score >= 90)
                            || known_army == 0
                            || late_game
                            || held_back;
                        if army_supply >= prm.attack_supply
                            && go
                            && s.tick.saturating_sub(self.last_attack_tick)
                                >= prm.reattack_interval
                            && !army.is_empty()
                        {
                            let target = self.attack_target(s, hq);
                            cmds.push(Command::AttackMove {
                                units: army.clone(),
                                target,
                                queued: false,
                            });
                            self.last_attack_tick = s.tick;
                            self.ball = BallState::Push;
                        }
                    }
                }
                BallState::Push => {
                    // Full recall only when home is genuinely burning.
                    let home_burning = base_threat
                        .as_ref()
                        .map(|(tpos, them)| {
                            them.hp_ground + them.hp_air > 100
                                && hq.map_or(false, |i| {
                                    crate::fixed::dist_sq_raw(
                                        s.entities[i as usize].pos,
                                        *tpos,
                                    ) <= (crate::Fx::from_int(12).0 as i64)
                                        .pow(2)
                                })
                        })
                        .unwrap_or(false);
                    if home_burning && !late_game {
                        cmds.push(Command::AttackMove {
                            units: army.clone(),
                            target: base_threat.as_ref().unwrap().0,
                            queued: false,
                        });
                        self.ball = BallState::Hold;
                        self.last_defense_tick = s.tick;
                    } else if fight_score < prm.retreat_score && !late_game {
                        cmds.push(Command::Move {
                            units: army.clone(),
                            target: anchor,
                            queued: false,
                        });
                        self.ball = BallState::Retreat;
                        self.last_attack_tick = s.tick;
                    } else if known_army == 0
                        && s.tick.saturating_sub(self.last_attack_tick)
                            >= prm.reattack_interval
                    {
                        // Push went stale (target cleared): re-target.
                        let target = self.attack_target(s, hq);
                        cmds.push(Command::AttackMove {
                            units: army.clone(),
                            target,
                            queued: false,
                        });
                        self.last_attack_tick = s.tick;
                    }
                }
                BallState::Retreat => {
                    // Arrived home (or died trying): hold.
                    let centroid = {
                        let (mut sx, mut sy, mut n) = (0i64, 0i64, 0i64);
                        for id in &army {
                            if let Some(e) = s.get(*id) {
                                sx += e.pos.x.0 as i64;
                                sy += e.pos.y.0 as i64;
                                n += 1;
                            }
                        }
                        if n == 0 {
                            None
                        } else {
                            Some(FxVec2::new(
                                crate::Fx((sx / n) as i32),
                                crate::Fx((sy / n) as i32),
                            ))
                        }
                    };
                    let arrived = centroid.map_or(true, |c| {
                        crate::fixed::dist_sq_raw(c, anchor)
                            <= (crate::Fx::from_int(10).0 as i64).pow(2)
                    });
                    if arrived {
                        self.ball = BallState::Hold;
                    }
                }
            }
        } else {
            // Legacy / Easy: the old timer waves with the whole army.
            let go = legacy || fight_score >= prm.attack_score || known_army == 0 || late_game || held_back;
            if army_supply >= prm.attack_supply
                && go
                && s.tick.saturating_sub(self.last_attack_tick) >= prm.reattack_interval
                && !army.is_empty()
            {
                let target = self.attack_target(s, hq);
                cmds.push(Command::AttackMove { units: army.clone(), target, queued: false });
                self.last_attack_tick = s.tick;
            }
        }

        self.had_threat = base_threat.is_some();

        // ---- combat micro: siege positioning + storms; fog-honest (only
        // reacts to enemies the player can actually see) ----
        let visible_enemies: Vec<(FxVec2, bool)> = s
            .entities
            .iter()
            .filter(|e| {
                e.alive
                    && s.hostile(p, e.owner)
                    && e.kind != EntityKind::Resource
                    && !e.burrowed
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
            let deploy_r = w.range + Fx::from_int(3);
            let max_sq = (deploy_r.0 as i64) * (deploy_r.0 as i64);
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

        // Shield micro: deploy bulwarks when a fight is on top of them,
        // pack up when the field goes quiet (hysteresis avoids flapping).
        if prm.use_squads {
            let fight_sq = (Fx::from_int(8).0 as i64).pow(2);
            let quiet_sq = (Fx::from_int(12).0 as i64).pow(2);
            for &t in &shields {
                let e = &s.entities[t as usize];
                if e.transform != 0 {
                    continue;
                }
                let fighting = visible_enemies
                    .iter()
                    .any(|&(pos, _)| dist_sq_raw(e.pos, pos) <= fight_sq);
                let quiet = !visible_enemies
                    .iter()
                    .any(|&(pos, _)| dist_sq_raw(e.pos, pos) <= quiet_sq);
                if !e.sieged && fighting {
                    cmds.push(Command::Siege { units: vec![s.id_of(t)] });
                } else if e.sieged && quiet {
                    cmds.push(Command::Siege { units: vec![s.id_of(t)] });
                }
            }

            // Burrow micro: holding burrowers near home dig in as an ambush
            // screen; they surface for the bite when something walks onto
            // them, or whenever the ball marches out.
            let bite_sq = (Fx::from_int(3).0 as i64).pow(2);
            let hide_sq = (Fx::from_int(9).0 as i64).pow(2);
            let anchor_sq = (Fx::from_int(10).0 as i64).pow(2);
            for &t in &burrowers {
                let e = &s.entities[t as usize];
                if e.transform != 0 {
                    continue;
                }
                let near_enemy = visible_enemies
                    .iter()
                    .any(|&(pos, _)| dist_sq_raw(e.pos, pos) <= bite_sq);
                let safe = !visible_enemies
                    .iter()
                    .any(|&(pos, _)| dist_sq_raw(e.pos, pos) <= hide_sq);
                if e.burrowed {
                    if near_enemy || matches!(self.ball, BallState::Push) {
                        cmds.push(Command::Burrow { units: vec![s.id_of(t)] });
                    }
                } else if matches!(self.ball, BallState::Hold)
                    && safe
                    && dist_sq_raw(e.pos, anchor) <= anchor_sq
                    && matches!(e.order, Order::Idle | Order::Hold)
                {
                    cmds.push(Command::Burrow { units: vec![s.id_of(t)] });
                }
            }
        }

        // Kite: reloading ranged units back off from melee that's closing
        // in. Advanced micro only (same gate as focus fire).
        if prm.focus_fire {
            let danger_sq = (Fx::from_ratio(9, 5).0 as i64).pow(2);
            for id in &army {
                let Some(e) = s.get(*id) else { continue };
                if e.sieged || e.burrowed || e.cooldown < 5 {
                    continue;
                }
                let d = &s.data.units[e.def as usize];
                let Some(w) = &d.weapon else { continue };
                if w.range < Fx::from_int(3) || d.fly {
                    continue;
                }
                let Some(&(tpos, _)) = visible_enemies
                    .iter()
                    .filter(|&&(pos, is_unit)| {
                        is_unit && dist_sq_raw(e.pos, pos) <= danger_sq
                    })
                    .min_by_key(|&&(pos, _)| dist_sq_raw(e.pos, pos))
                else {
                    continue;
                };
                // Step directly away from the threat.
                let away = FxVec2::new(
                    e.pos.x + (e.pos.x - tpos.x) * Fx::from_int(2),
                    e.pos.y + (e.pos.y - tpos.y) * Fx::from_int(2),
                );
                cmds.push(Command::Move {
                    units: vec![*id],
                    target: s.map.clamp_pos(away),
                    queued: false,
                });
            }
        }

        // Hero abilities: aim the zone ability at the biggest clump in
        // reach; fire the instant ability on the trigger it wants.
        if let Some(hidx) = hero_unit {
            let e = &s.entities[hidx as usize];
            let tag = s.data.units[e.def as usize].tag.clone();
            let idle_enough = !matches!(e.order, Order::CastAbility { .. });
            if idle_enough {
                let (zone_slot, inst_slot) = match tag.as_str() {
                    "marshal" => (Some(0u8), Some(1u8)),
                    "broodmother" => (Some(1), Some(0)),
                    "magnus" => (Some(0), Some(1)),
                    _ => (None, None),
                };
                // Zone: >=3 enemy units clumped within reach.
                if let Some(slot) = zone_slot {
                    if let Some(spec) = crate::hero::ability(&tag, slot) {
                        if e.energy >= spec.cost {
                            let reach2 = spec.cast_range + Fx::from_int(3);
                            let reach2_sq = (reach2.0 as i64) * (reach2.0 as i64);
                            let zone_sq = (Fx::from_int(3).0 as i64).pow(2);
                            let mut best: Option<(usize, i64, FxVec2)> = None;
                            for &(pos, is_unit) in &visible_enemies {
                                if !is_unit || dist_sq_raw(e.pos, pos) > reach2_sq {
                                    continue;
                                }
                                let n = visible_enemies
                                    .iter()
                                    .filter(|&&(q, u)| u && dist_sq_raw(pos, q) <= zone_sq)
                                    .count();
                                let d = dist_sq_raw(e.pos, pos);
                                if n >= 3
                                    && best.map_or(true, |(bn, bd, _)| (n, std::cmp::Reverse(d)) > (bn, std::cmp::Reverse(bd)))
                                {
                                    best = Some((n, d, pos));
                                }
                            }
                            if let Some((_, _, target)) = best {
                                cmds.push(Command::UseAbility {
                                    caster: s.id_of(hidx),
                                    slot,
                                    target,
                                });
                            }
                        }
                    }
                }
                // Instant: enemies (or wounded allies for the marshal) close.
                if let Some(slot) = inst_slot {
                    if let Some(spec) = crate::hero::ability(&tag, slot) {
                        if e.energy >= spec.cost {
                            let near_sq = (Fx::from_int(4).0 as i64).pow(2);
                            let fire = match tag.as_str() {
                                "marshal" => s
                                    .entities
                                    .iter()
                                    .filter(|o| {
                                        o.alive
                                            && o.owner == p
                                            && o.kind == EntityKind::Unit
                                            && o.hp * 2 < s.data.units[o.def as usize].hp
                                            && dist_sq_raw(o.pos, e.pos)
                                                <= (Fx::from_int(5).0 as i64).pow(2)
                                    })
                                    .count()
                                    >= 3,
                                _ => visible_enemies
                                    .iter()
                                    .filter(|&&(q, u)| u && dist_sq_raw(e.pos, q) <= near_sq)
                                    .count()
                                    >= if tag == "magnus" { 3 } else { 2 },
                            };
                            if fire {
                                cmds.push(Command::UseAbility {
                                    caster: s.id_of(hidx),
                                    slot,
                                    target: e.pos,
                                });
                            }
                        }
                    }
                }
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

        // ---- micro: focus fire. Every couple of seconds, ranged units
        // with an enemy in weapon range all switch to the weakest such
        // enemy — concentrated fire kills units, spread fire tickles ----
        if prm.focus_fire
            && !legacy
            && s.tick.saturating_sub(self.last_focus_tick) >= prm.react_interval
        {
            // Weakest visible enemy that at least one of ours can shoot.
            let mut best: Option<(i32, u32)> = None; // (hp, enemy idx)
            for (j, e) in s.entities.iter().enumerate() {
                if !e.alive
                    || e.owner == p
                    || e.owner == crate::state::NEUTRAL
                    || e.kind != EntityKind::Unit
                {
                    continue;
                }
                if !s.fog[p as usize].visible(&s.map, TilePos::of(e.pos)) {
                    continue;
                }
                if best.map_or(true, |(hp, bj)| (e.hp, j as u32) < (hp, bj)) {
                    // Only worth focusing if several of ours are in range.
                    let can_hit = army
                        .iter()
                        .filter_map(|id| s.get(*id).map(|u| (id, u)))
                        .filter(|(_, u)| {
                            let d = &s.data.units[u.def as usize];
                            let Some(w) = &d.weapon else { return false };
                            if e.hp <= 0 || (s.data.units[e.def as usize].fly && !w.air) {
                                return false;
                            }
                            let r = w.range + crate::Fx::from_int(1);
                            crate::fixed::dist_sq_raw(u.pos, e.pos)
                                <= (r.0 as i64) * (r.0 as i64)
                        })
                        .count();
                    if can_hit >= 3 {
                        best = Some((e.hp, j as u32));
                    }
                }
            }
            if let Some((_, j)) = best {
                let target = s.id_of(j);
                let tpos = s.entities[j as usize].pos;
                let shooters: Vec<EntityId> = army
                    .iter()
                    .copied()
                    .filter(|id| {
                        let Some(u) = s.get(*id) else { return false };
                        let d = &s.data.units[u.def as usize];
                        let Some(w) = &d.weapon else { return false };
                        if s.data.units[s.entities[j as usize].def as usize].fly && !w.air {
                            return false;
                        }
                        // Ranged only, and already close enough to fire.
                        w.range > crate::Fx::from_int(2)
                            && crate::fixed::dist_sq_raw(u.pos, tpos)
                                <= ((w.range + crate::Fx::from_int(1)).0 as i64).pow(2)
                    })
                    .collect();
                if shooters.len() >= 3 {
                    cmds.push(Command::AttackTarget { units: shooters, target });
                    self.last_focus_tick = s.tick;
                }
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

    /// Nearest remembered enemy structure, else the enemy start. Late game
    /// alternates pushes onto the enemy MAIN so mutual multi-base turtles
    /// (Thornwood mirrors) collide and finish instead of trading at the
    /// nearest wall forever.
    fn attack_target(&self, s: &State, hq: Option<u32>) -> FxVec2 {
        let enemy_start =
            s.map.starts[(1 - self.player as usize).min(s.map.starts.len() - 1)];
        let late = s.tick > 24 * 60 * 15;
        let deep_strike = late && (s.tick / (24 * 45)) % 2 == 0;
        if deep_strike {
            return enemy_start.center();
        }
        let hq_pos = hq.map(|i| s.entities[i as usize].pos);
        if let Some(pos) = self
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
        {
            return pos;
        }
        // Nothing remembered: mop-up sweep. The last enemy building can be
        // a hidden pylon in a corner — rotate the army through the enemy
        // start, every expansion site, and the map corners until vision
        // finds it (a razed start used to soak every re-push forever).
        let mut sweep: Vec<FxVec2> = vec![enemy_start.center()];
        for e in &s.map.expansions {
            sweep.push(FxVec2::new(
                crate::Fx::from_int(e.x + 1),
                crate::Fx::from_int(e.y + 1),
            ));
        }
        let (w, h) = (s.map.width, s.map.height);
        for (cx, cy) in [(6, 6), (w - 7, 6), (6, h - 7), (w - 7, h - 7)] {
            sweep.push(FxVec2::new(crate::Fx::from_int(cx), crate::Fx::from_int(cy)));
        }
        sweep[(s.tick / (24 * 20)) as usize % sweep.len()]
    }

    /// The ramp chokepoint guarding home — centroid of ramp tiles near the
    /// HQ, else the HQ itself. Where the guard falls back to and the
    /// strike stages.
    fn defense_anchor(&self, s: &State) -> FxVec2 {
        let Some(hq_tile) = self.hq_tile(s) else {
            return s.map.clamp_pos(FxVec2::from_int(1, 1));
        };
        let (mut sx, mut sy, mut n) = (0i64, 0i64, 0i64);
        for y in (hq_tile.y - 20).max(0)..(hq_tile.y + 20).min(s.map.height) {
            for x in (hq_tile.x - 20).max(0)..(hq_tile.x + 20).min(s.map.width) {
                if s.map.kind_at(x, y) == crate::map::TileKind::Ramp {
                    sx += x as i64;
                    sy += y as i64;
                    n += 1;
                }
            }
        }
        if n == 0 {
            return hq_tile.center();
        }
        TilePos::new((sx / n) as i32, (sy / n) as i32).center()
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
                    if self.bad_sites.contains(&site) {
                        continue;
                    }
                    if site_ok(site)
                        && s.valid_building_site(def, site, Some(builder))
                        && site_reachable(s, s.entities[builder as usize].pos, site, (fw, fh))
                    {
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

    fn hq_entity<'a>(&self, s: &'a State) -> Option<&'a crate::Entity> {
        s.entities.iter().find(|e| {
            e.alive
                && e.owner == self.player
                && e.kind == EntityKind::Building
                && s.data.buildings[e.def as usize].headquarters
        })
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


/// Can a builder standing at `from` actually walk next to `site`? A
/// bounded BFS over the static+dynamic walkability — cheap (runs once
/// per accepted candidate), and it keeps the site scan from committing
/// a worker to a pocket across a cliff it can never enter.
fn site_reachable(
    s: &State,
    from: crate::fixed::FxVec2,
    site: TilePos,
    (fw, fh): (i32, i32),
) -> bool {
    // MIRROR COVARIANCE: every ingredient here must give the same answer
    // for the 180-degree-rotated question. The goal test is a box around
    // the whole FOOTPRINT (a corner-anchored box is lopsided under
    // rotation), and the search region is a min/max box with full
    // exploration — an exploration budget would cut off at different
    // frontiers in the two games.
    let start = TilePos::of(from);
    let near_goal = |t: TilePos| {
        t.x >= site.x - 1 && t.x <= site.x + fw && t.y >= site.y - 1 && t.y <= site.y + fh
    };
    if near_goal(start) {
        return true;
    }
    let x0 = start.x.min(site.x - 1) - 10;
    let x1 = start.x.max(site.x + fw) + 10;
    let y0 = start.y.min(site.y - 1) - 10;
    let y1 = start.y.max(site.y + fh) + 10;
    let ok = |t: TilePos| -> bool {
        t.x >= x0.max(0)
            && t.y >= y0.max(0)
            && t.x <= x1.min(s.map.width - 1)
            && t.y <= y1.min(s.map.height - 1)
            && s.map.walkable(t.x, t.y)
            && !s.blocked[s.map.idx(t.x, t.y)]
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut queue = std::collections::VecDeque::new();
    seen.insert((start.x, start.y));
    queue.push_back(start);
    while let Some(t) = queue.pop_front() {
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let n = TilePos::new(t.x + dx, t.y + dy);
            if near_goal(n) {
                return true;
            }
            if ok(n) && seen.insert((n.x, n.y)) {
                queue.push_back(n);
            }
        }
    }
    false
}
