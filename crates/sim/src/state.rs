//! Sim state: entities, players, orders, commands.
//!
//! One flat `Entity` struct covers units, buildings, and resources (SC1 did
//! the same). Unused fields sit at their defaults; the alternative — three
//! entity arrays with cross-referencing IDs — costs more in bookkeeping than
//! this costs in memory.

use crate::data::{DefId, GameData};
use crate::entity::EntityId;
use crate::fixed::{Fx, FxVec2};
use crate::fog::FogGrid;
use crate::map::{Map, TileKind, TilePos};
use crate::path::FieldPool;
use crate::rng::SimRng;

pub const NEUTRAL: u8 = 255;
/// Resource entity def ids (Entity.def for kind == Resource).
pub const RES_MINERALS: u16 = 0;
pub const RES_GEYSER: u16 = 1;
/// Destructible tree: blocks the tile AND line of sight; killable by an
/// explicit attack (never auto-acquired). Death opens both.
pub const RES_TREE: u16 = 2;
/// Destructible rock wall: blocks the tile only (vision passes).
pub const RES_ROCK: u16 = 3;
pub const TREE_HP: i32 = 120;
pub const ROCK_HP: i32 = 500;
/// Minerals carried per harvester trip.
pub const CARRY_AMOUNT: i32 = 8;
/// Ticks spent mining at a patch per trip.
pub const MINE_TICKS: u16 = 45;
/// Starting minerals per player.
pub const START_MINERALS: u32 = 600;
/// Starting workers per player.
pub const START_WORKERS: usize = 6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntityKind {
    Unit,
    Building,
    Resource,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GatherPhase {
    ToResource,
    Mining { ticks_left: u16 },
    ToDepot,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuildPhase {
    Travel,
    Constructing { building: EntityId },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Order {
    Idle,
    Hold,
    Move { target: FxVec2, field: u32 },
    AttackMove { target: FxVec2, field: u32 },
    AttackTarget { target: EntityId },
    Gather { resource: EntityId, phase: GatherPhase, field: u32 },
    Build { def: DefId, site: TilePos, phase: BuildPhase, field: u32 },
    /// Walk into range of `target`, then unleash a Plasma Storm.
    Cast { target: FxVec2, field: u32 },
    /// Walking to cast a hero ability (slot into the hero's kit).
    CastAbility { slot: u8, target: FxVec2, field: u32 },
}

impl Order {
    pub fn field(&self) -> Option<u32> {
        match *self {
            Order::Move { field, .. }
            | Order::AttackMove { field, .. }
            | Order::Gather { field, .. }
            | Order::Build { field, .. }
            | Order::Cast { field, .. } => Some(field),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entity {
    pub gen: u32,
    pub alive: bool,
    pub kind: EntityKind,
    pub def: DefId,
    pub owner: u8,
    pub pos: FxVec2,
    /// Position at the previous tick — the renderer interpolates between
    /// prev_pos and pos. Sim never reads it.
    pub prev_pos: FxVec2,
    pub hp: i32,
    pub order: Order,
    /// Shift-queued follow-up orders, executed as each order completes.
    pub order_queue: Vec<Order>,
    /// Transient combat sub-target (acquired while attack-moving/idle).
    pub engage: Option<EntityId>,
    pub cooldown: u16,
    /// Resource: amount remaining. Harvester: cargo carried.
    /// Extractor building: gas remaining.
    pub amount: i32,
    /// Harvester cargo is gas (from an extractor) rather than minerals.
    pub carry_gas: bool,
    /// Spellcaster energy points.
    pub energy: u16,
    /// Remaining lifetime in ticks for summoned units (0 = permanent).
    pub decay: u16,
    /// Siege-capable unit is deployed (immobile, uses weapon_siege).
    pub sieged: bool,
    /// Burrow-capable unit is underground: hidden, untargetable by direct
    /// fire (area damage still hits), cannot move or attack.
    pub burrowed: bool,
    /// Ticks remaining of a siege/unsiege transform (unit is busy).
    pub transform: u16,
    /// Building: active research (index into data.research, progress ticks).
    pub research: Option<(u8, u32)>,
    /// Building production queue (front = in progress).
    pub queue: Vec<DefId>,
    /// Ticks of progress on the front queue item.
    pub progress: u32,
    /// Some(progress_ticks) while under construction.
    pub construction: Option<u32>,
    pub rally: FxVec2,
    /// Consecutive ticks the unit wanted to move but couldn't (crowd arrival).
    pub stuck: u16,
}

impl Entity {
    fn blank() -> Entity {
        Entity {
            gen: 0,
            alive: false,
            kind: EntityKind::Unit,
            def: 0,
            owner: NEUTRAL,
            pos: FxVec2::ZERO,
            prev_pos: FxVec2::ZERO,
            hp: 0,
            order: Order::Idle,
            order_queue: Vec::new(),
            engage: None,
            cooldown: 0,
            amount: 0,
            carry_gas: false,
            energy: 0,
            decay: 0,
            sieged: false,
            burrowed: false,
            transform: 0,
            research: None,
            queue: Vec::new(),
            progress: 0,
            construction: None,
            rally: FxVec2::ZERO,
            stuck: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Player {
    pub minerals: u32,
    pub gas: u32,
    pub defeated: bool,
    pub race: u8,
    // End-of-game stats (deterministic, checksummed).
    pub units_built: u32,
    pub units_lost: u32,
    pub buildings_lost: u32,
    pub minerals_mined: u32,
    pub gas_mined: u32,
    /// Upgrade levels from research (add to damage / subtract from taken).
    pub weapons_level: u8,
    pub armor_level: u8,
    /// Per-research completion, indexed like GameData::research.
    pub research_done: Vec<bool>,
}

/// Player-issued commands. The only mutation channel into the sim — in
/// multiplayer these are exactly what goes over the wire.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Command {
    Move { units: Vec<EntityId>, target: FxVec2, queued: bool },
    AttackMove { units: Vec<EntityId>, target: FxVec2, queued: bool },
    AttackTarget { units: Vec<EntityId>, target: EntityId },
    Gather { units: Vec<EntityId>, resource: EntityId, queued: bool },
    Stop { units: Vec<EntityId> },
    Hold { units: Vec<EntityId> },
    Train { building: EntityId, unit: DefId },
    CancelTrain { building: EntityId, slot: u8 },
    /// Abort an under-construction building: 75% refund, site freed.
    CancelConstruction { building: EntityId },
    /// Start a research at a tech building.
    Research { building: EntityId, research: u8 },
    /// Toggle siege mode on capable units.
    Siege { units: Vec<EntityId> },
    /// Toggle burrow on capable units.
    Burrow { units: Vec<EntityId> },
    /// Cast Plasma Storm at a point (caster walks into range first).
    Cast { caster: EntityId, target: FxVec2 },
    /// Hero ability by slot (0/1); zone abilities walk into range first.
    UseAbility { caster: EntityId, slot: u8, target: FxVec2 },
    Build { worker: EntityId, building: DefId, site: TilePos, queued: bool },
    SetRally { building: EntityId, target: FxVec2 },
}

/// Renderer-facing happenings of the last tick. Derived from deterministic
/// state transitions — never checksummed, never read by sim logic.
#[derive(Clone, Copy, Debug)]
pub enum SimEvent {
    /// `from` hit `to` (both entity indices, valid at emission time).
    Attack { from: u32, to: u32 },
    Death { pos: FxVec2, owner: u8, kind: EntityKind, def: u16 },
    /// A Plasma Storm landed here.
    Cast { pos: FxVec2 },
    /// A unit finished training.
    Ready { pos: FxVec2, owner: u8 },
    /// A building finished construction.
    BuildingDone { pos: FxVec2, owner: u8 },
    /// A research completed.
    ResearchDone { owner: u8 },
}

/// An active area zone (storms + hero abilities). Part of logical state
/// (checksummed). Kinds: 0 Plasma Storm, 1 Barrage, 2 Corrosive Cloud,
/// 3 Magnetic Well.
#[derive(Clone, Debug)]
pub struct Storm {
    pub pos: FxVec2,
    pub ticks_left: u16,
    pub owner: u8,
    pub kind: u8,
}

pub struct State {
    pub tick: u32,
    pub data: GameData,
    pub map: Map,
    pub entities: Vec<Entity>,
    pub free: Vec<u32>,
    pub players: Vec<Player>,
    pub fields: FieldPool,
    /// Dynamic blockers: building footprints + mineral patches.
    pub blocked: Vec<bool>,
    /// Tiles that block LINE OF SIGHT (trees). Maintained with `blocked`.
    pub vision_block: Vec<bool>,
    /// Fast path: skip LOS raycasts entirely on maps without trees.
    pub has_vision_blockers: bool,
    pub fog: Vec<FogGrid>,
    pub rng: SimRng,
    pub winner: Option<u8>,
    pub storms: Vec<Storm>,
    // ---- scratch (not part of logical state, never checksummed) ----
    pub events: Vec<SimEvent>,
    pub scratch_vel: Vec<FxVec2>,
    pub buckets: Vec<Vec<u32>>,
    pub buckets_used: Vec<u32>,
    /// The seed this state was created with (for replays).
    pub seed: u64,
    /// Every command ever stepped, as (tick, player, command) — the full
    /// input stream. With `seed` this reproduces the game bit-exactly.
    pub command_log: Vec<(u32, u8, Command)>,
}

impl State {
    pub fn new(data: GameData, map: Map, seed: u64) -> State {
        let n = map.starts.len();
        Self::new_with_races(data, map, seed, &vec![0u8; n])
    }

    pub fn new_with_races(data: GameData, map: Map, seed: u64, races: &[u8]) -> State {
        let n_tiles = (map.width * map.height) as usize;
        let n_players = map.starts.len();
        let mut s = State {
            tick: 0,
            entities: Vec::new(),
            free: Vec::new(),
            players: (0..n_players)
                .map(|p| Player {
                    minerals: START_MINERALS,
                    gas: 0,
                    defeated: false,
                    race: races.get(p).copied().unwrap_or(0),
                    units_built: 0,
                    units_lost: 0,
                    buildings_lost: 0,
                    minerals_mined: 0,
                    gas_mined: 0,
                    weapons_level: 0,
                    armor_level: 0,
                    research_done: vec![false; data.research.len()],
                })
                .collect(),
            fields: FieldPool::default(),
            blocked: vec![false; n_tiles],
            vision_block: vec![false; n_tiles],
            has_vision_blockers: false,
            fog: (0..n_players).map(|_| FogGrid::new(map.width, map.height)).collect(),
            rng: SimRng::new(seed),
            winner: None,
            storms: Vec::new(),
            events: Vec::new(),
            scratch_vel: Vec::new(),
            buckets: vec![Vec::new(); n_tiles],
            buckets_used: Vec::new(),
            seed,
            command_log: Vec::new(),
            data,
            map,
        };

        // Mineral patches and plasma geysers.
        for (tile, amount) in s.map.minerals.clone() {
            s.spawn_resource(RES_MINERALS, tile, (1, 1), amount);
        }
        for (origin, amount) in s.map.geysers.clone() {
            s.spawn_resource(RES_GEYSER, origin, (2, 2), amount);
        }
        for t in s.map.trees.clone() {
            s.spawn_resource(RES_TREE, t, (1, 1), 0);
        }
        for t in s.map.rocks.clone() {
            s.spawn_resource(RES_ROCK, t, (1, 1), 0);
        }

        // One HQ + starting workers per player, per their race.
        for p in 0..n_players as u8 {
            let hq = s.data.hq_of_race(s.players[p as usize].race);
            let worker = s.data.worker_of_race(s.players[p as usize].race);
            let start = s.map.starts[p as usize];
            let origin = TilePos::new(start.x - 1, start.y - 1);
            let id = s.spawn_building(p, hq, origin, false);
            let center = s.entities[id.idx as usize].pos;
            // Workers in an arc on the mineral-line side of the HQ. The
            // arc is centered and fully sign-flipped per side so the two
            // spawns are exact point-mirrors — an off-center arc gave the
            // SE main a measurably different opening (mirror.rs probe).
            let side = if start.x < s.map.width / 2 { -1 } else { 1 };
            for w in 0..START_WORKERS {
                // Quarter-tile nudge keeps spawn positions OFF tile
                // boundaries: a unit exactly on a boundary floors into
                // different tiles on the two mirrored halves, which is
                // enough to diverge the whole opening.
                let base = 2 * w as i32 - (START_WORKERS as i32 - 1);
                let centered = Fx::from_ratio(base, 2) + Fx::from_ratio(base.signum(), 4);
                let off = FxVec2::new(Fx::from_int(side * 3), centered * Fx::from_int(side));
                s.spawn_unit(p, worker, center + off);
            }
        }

        s.update_fog();
        s
    }

    // ---- entity plumbing ----

    fn alloc(&mut self) -> u32 {
        if let Some(idx) = self.free.pop() {
            idx
        } else {
            self.entities.push(Entity::blank());
            self.scratch_vel.push(FxVec2::ZERO);
            (self.entities.len() - 1) as u32
        }
    }

    pub fn spawn_unit(&mut self, owner: u8, def: DefId, pos: FxVec2) -> EntityId {
        let idx = self.alloc();
        let hp = self.data.units[def as usize].hp;
        let energy = (self.data.units[def as usize].energy_max / 4) as u16;
        if (owner as usize) < self.players.len() {
            self.players[owner as usize].units_built += 1;
        }
        let e = &mut self.entities[idx as usize];
        let gen = e.gen;
        *e = Entity {
            gen,
            alive: true,
            kind: EntityKind::Unit,
            def,
            owner,
            pos,
            prev_pos: pos,
            hp,
            energy,
            ..Entity::blank()
        };
        EntityId { idx, gen }
    }

    pub fn spawn_building(&mut self, owner: u8, def: DefId, origin: TilePos, under_construction: bool) -> EntityId {
        let d = &self.data.buildings[def as usize];
        let (fw, fh) = d.footprint;
        let hp = if under_construction { d.hp / 10 } else { d.hp };
        let pos = FxVec2::new(
            Fx::from_int(origin.x) + Fx::from_int(fw) * Fx::HALF,
            Fx::from_int(origin.y) + Fx::from_int(fh) * Fx::HALF,
        );
        let idx = self.alloc();
        let e = &mut self.entities[idx as usize];
        let gen = e.gen;
        *e = Entity {
            gen,
            alive: true,
            kind: EntityKind::Building,
            def,
            owner,
            pos,
            prev_pos: pos,
            hp,
            construction: if under_construction { Some(0) } else { None },
            rally: pos,
            ..Entity::blank()
        };
        self.set_footprint_blocked(def, origin, true);
        EntityId { idx, gen }
    }

    pub fn spawn_resource(
        &mut self,
        def: u16,
        origin: TilePos,
        size: (i32, i32),
        amount: i32,
    ) -> EntityId {
        let idx = self.alloc();
        let pos = FxVec2::new(
            Fx::from_int(origin.x) + Fx::from_int(size.0) * Fx::HALF,
            Fx::from_int(origin.y) + Fx::from_int(size.1) * Fx::HALF,
        );
        let e = &mut self.entities[idx as usize];
        let gen = e.gen;
        *e = Entity {
            gen,
            alive: true,
            kind: EntityKind::Resource,
            def,
            owner: NEUTRAL,
            pos,
            prev_pos: pos,
            hp: match def {
                RES_TREE => TREE_HP,
                RES_ROCK => ROCK_HP,
                _ => 1,
            },
            amount,
            ..Entity::blank()
        };
        let sees_through = def != RES_TREE;
        for x in origin.x..origin.x + size.0 {
            for y in origin.y..origin.y + size.1 {
                if self.map.in_bounds(x, y) {
                    let i = self.map.idx(x, y);
                    self.blocked[i] = true;
                    if !sees_through {
                        self.vision_block[i] = true;
                        self.has_vision_blockers = true;
                    }
                }
            }
        }
        EntityId { idx, gen }
    }

    pub fn kill(&mut self, idx: u32) {
        let e = &mut self.entities[idx as usize];
        if !e.alive {
            return;
        }
        if e.kind != EntityKind::Resource {
            self.events.push(SimEvent::Death { pos: e.pos, owner: e.owner, kind: e.kind, def: e.def });
            let owner = e.owner as usize;
            let kind = e.kind;
            if owner < self.players.len() {
                match kind {
                    EntityKind::Unit => self.players[owner].units_lost += 1,
                    EntityKind::Building => self.players[owner].buildings_lost += 1,
                    EntityKind::Resource => {}
                }
            }
        }
        let e = &mut self.entities[idx as usize];
        e.alive = false;
        e.gen = e.gen.wrapping_add(1);
        e.engage = None;
        e.queue.clear();
        let kind = e.kind;
        let def = e.def;
        let pos = e.pos;
        match kind {
            EntityKind::Building => {
                let origin = self.footprint_origin(def, pos);
                self.set_footprint_blocked(def, origin, false);
            }
            EntityKind::Resource => {
                let size = if def == RES_GEYSER { (2, 2) } else { (1, 1) };
                let origin = TilePos::new(
                    (pos.x - Fx::from_int(size.0) * Fx::HALF).round_int(),
                    (pos.y - Fx::from_int(size.1) * Fx::HALF).round_int(),
                );
                for x in origin.x..origin.x + size.0 {
                    for y in origin.y..origin.y + size.1 {
                        if self.map.in_bounds(x, y) {
                            let i = self.map.idx(x, y);
                            self.blocked[i] = false;
                            self.vision_block[i] = false;
                        }
                    }
                }
            }
            EntityKind::Unit => {}
        }
        self.free.push(idx);
    }

    pub fn get(&self, id: EntityId) -> Option<&Entity> {
        let e = self.entities.get(id.idx as usize)?;
        (e.alive && e.gen == id.gen).then_some(e)
    }

    pub fn id_of(&self, idx: u32) -> EntityId {
        EntityId { idx, gen: self.entities[idx as usize].gen }
    }

    // ---- footprints & placement ----

    /// Flow field for approaching entity `idx` (resource or building):
    /// seeded from every open tile ringing its footprint, so units reach
    /// whichever side is open instead of marching at one (possibly sealed)
    /// tile. Returns the inserted field slot.
    pub fn approach_field(&mut self, idx: u32) -> u32 {
        let e = &self.entities[idx as usize];
        let (origin, fw, fh) = match e.kind {
            EntityKind::Building => {
                let (fw, fh) = self.data.buildings[e.def as usize].footprint;
                (self.footprint_origin(e.def, e.pos), fw, fh)
            }
            EntityKind::Resource => {
                let side = if e.def == RES_GEYSER { 2 } else { 1 };
                (TilePos::of(e.pos), side, side)
            }
            EntityKind::Unit => (TilePos::of(e.pos), 1, 1),
        };
        let seeds = crate::path::ring_seeds(origin, fw, fh);
        self.fields
            .insert(crate::path::compute_flow_field_multi(&self.map, &self.blocked, &seeds))
    }

    pub fn footprint_origin(&self, def: DefId, center: FxVec2) -> TilePos {
        let (fw, fh) = self.data.buildings[def as usize].footprint;
        TilePos::new(
            (center.x - Fx::from_int(fw) * Fx::HALF).round_int(),
            (center.y - Fx::from_int(fh) * Fx::HALF).round_int(),
        )
    }

    fn set_footprint_blocked(&mut self, def: DefId, origin: TilePos, val: bool) {
        let (fw, fh) = self.data.buildings[def as usize].footprint;
        for x in origin.x..origin.x + fw {
            for y in origin.y..origin.y + fh {
                if self.map.in_bounds(x, y) {
                    let i = self.map.idx(x, y);
                    self.blocked[i] = val;
                }
            }
        }
    }

    /// A live geyser whose 2x2 footprint origin is exactly `origin`.
    pub fn geyser_at(&self, origin: TilePos) -> Option<u32> {
        let center = FxVec2::new(
            Fx::from_int(origin.x) + Fx::ONE,
            Fx::from_int(origin.y) + Fx::ONE,
        );
        (0..self.entities.len() as u32).find(|&i| {
            let e = &self.entities[i as usize];
            e.alive && e.kind == EntityKind::Resource && e.def == RES_GEYSER && e.pos == center
        })
    }

    /// Valid site: flat Ground tiles, unblocked, no unit standing in the
    /// footprint other than the builder itself. Gas extractors instead
    /// require exactly a live geyser at the site.
    pub fn valid_building_site(&self, def: DefId, origin: TilePos, builder: Option<u32>) -> bool {
        if self.data.buildings[def as usize].gas_extractor {
            return self.geyser_at(origin).is_some();
        }
        let (fw, fh) = self.data.buildings[def as usize].footprint;
        let elev = self.map.elev_at(origin.x, origin.y);
        for x in origin.x..origin.x + fw {
            for y in origin.y..origin.y + fh {
                if !self.map.in_bounds(x, y)
                    || self.map.kind_at(x, y) != TileKind::Ground
                    || self.map.elev_at(x, y) != elev
                    || self.blocked[self.map.idx(x, y)]
                {
                    return false;
                }
            }
        }
        for (i, e) in self.entities.iter().enumerate() {
            if !e.alive || e.kind != EntityKind::Unit || Some(i as u32) == builder {
                continue;
            }
            let t = TilePos::of(e.pos);
            if t.x >= origin.x - 1
                && t.x < origin.x + fw + 1
                && t.y >= origin.y - 1
                && t.y < origin.y + fh + 1
            {
                // Strict interior blocks; the 1-tile margin only blocks if
                // the unit is actually overlapping the footprint edge.
                if t.x >= origin.x && t.x < origin.x + fw && t.y >= origin.y && t.y < origin.y + fh {
                    return false;
                }
            }
        }
        true
    }

    /// Open tile ringing a building footprint, preferring the side facing
    /// `toward` (the rally point) — symmetric on mirrored maps and units
    /// exit in the useful direction.
    pub fn spawn_tile_near(&self, def: DefId, origin: TilePos, toward: FxVec2) -> Option<TilePos> {
        let (fw, fh) = self.data.buildings[def as usize].footprint;
        // Distance ties (common: rally straight at a mineral line makes the
        // ring symmetric) must break toward mirrored tiles on the two map
        // halves, or every spawn drifts NW and the halves play differently.
        let n = (self.map.width * self.map.height) as u32;
        let flip = 2 * (self.map.idx(origin.x, origin.y) as u32) > n - 1;
        let key = |x: i32, y: i32| if flip { (-x, -y) } else { (x, y) };
        for r in 1..8 {
            let mut best: Option<(i64, (i32, i32))> = None;
            for y in origin.y - r..origin.y + fh + r {
                for x in origin.x - r..origin.x + fw + r {
                    let on_ring =
                        x < origin.x || x >= origin.x + fw || y < origin.y || y >= origin.y + fh;
                    if on_ring
                        && self.map.walkable(x, y)
                        && !self.blocked[self.map.idx(x, y)]
                    {
                        let d = crate::fixed::dist_sq_raw(
                            TilePos::new(x, y).center(),
                            toward,
                        );
                        if best.map_or(true, |(bd, bk)| (d, key(x, y)) < (bd, bk)) {
                            best = Some((d, key(x, y)));
                        }
                    }
                }
            }
            if let Some((_, k)) = best {
                let (x, y) = if flip { (-k.0, -k.1) } else { k };
                return Some(TilePos::new(x, y));
            }
        }
        None
    }

    // ---- queries ----

    /// Collision/targeting radius. Buildings use half their smaller footprint
    /// dimension.
    pub fn radius_of(&self, idx: u32) -> Fx {
        let e = &self.entities[idx as usize];
        match e.kind {
            EntityKind::Unit => self.data.units[e.def as usize].radius,
            EntityKind::Building => {
                let (fw, fh) = self.data.buildings[e.def as usize].footprint;
                Fx::from_int(fw.min(fh)) * Fx::HALF
            }
            EntityKind::Resource => Fx::HALF,
        }
    }

    pub fn sight_of(&self, idx: u32) -> u32 {
        let e = &self.entities[idx as usize];
        match e.kind {
            EntityKind::Unit => self.data.units[e.def as usize].sight,
            EntityKind::Building => self.data.buildings[e.def as usize].sight,
            EntityKind::Resource => 0,
        }
    }

    /// Does the player own a completed building of this def? (Tech tree.)
    pub fn has_building(&self, player: u8, def: DefId) -> bool {
        self.entities.iter().any(|e| {
            e.alive
                && e.owner == player
                && e.kind == EntityKind::Building
                && e.def == def
                && e.construction.is_none()
        })
    }

    /// Requirement check for training a unit / placing a building.
    pub fn requirement_met(&self, player: u8, requires: Option<DefId>) -> bool {
        requires.map_or(true, |d| self.has_building(player, d))
    }

    /// (used, provided) supply for a player.
    pub fn supply(&self, player: u8) -> (u32, u32) {
        let mut used = 0;
        let mut provided = 0;
        for e in &self.entities {
            if !e.alive || e.owner != player {
                continue;
            }
            match e.kind {
                EntityKind::Unit => used += self.data.units[e.def as usize].supply,
                EntityKind::Building => {
                    if e.construction.is_none() {
                        provided += self.data.buildings[e.def as usize].supply_provided;
                    }
                    if let Some(&front) = e.queue.first() {
                        if e.progress > 0 {
                            used += self.data.units[front as usize].supply;
                        }
                    }
                }
                EntityKind::Resource => {}
            }
        }
        (used, provided.min(crate::SUPPLY_CAP))
    }

    /// Set a unit's order. `queued` appends to the waypoint queue instead
    /// (unless the unit is idle, in which case it starts immediately).
    pub(crate) fn issue_order(&mut self, i: usize, order: Order, queued: bool) {
        let e = &mut self.entities[i];
        if queued && !matches!(e.order, Order::Idle) {
            if e.order_queue.len() < 8 {
                e.order_queue.push(order);
            }
        } else {
            e.order = order;
            e.order_queue.clear();
            e.engage = None;
            e.stuck = 0;
        }
    }

    /// Current order finished: start the next queued one, or go idle.
    pub(crate) fn finish_order(&mut self, i: usize) {
        let e = &mut self.entities[i];
        e.order = if e.order_queue.is_empty() {
            Order::Idle
        } else {
            e.order_queue.remove(0)
        };
        e.engage = None;
        e.stuck = 0;
    }

    pub fn visible_to(&self, player: u8, idx: u32) -> bool {
        let e = &self.entities[idx as usize];
        let t = TilePos::of(e.pos);
        self.fog[player as usize].visible(&self.map, t)
    }

    /// FNV-1a over logical state. Lockstep peers compare this every tick in
    /// dev builds; a mismatch is a desync at that exact tick.
    pub fn checksum(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut mix = |v: u64| {
            h ^= v;
            h = h.wrapping_mul(0x100000001b3);
        };
        mix(self.tick as u64);
        for p in &self.players {
            mix(p.minerals as u64);
            mix(p.gas as u64);
            mix(p.defeated as u64);
            mix(p.weapons_level as u64);
            mix(p.armor_level as u64);
            mix(p.units_built as u64);
            mix(p.units_lost as u64);
            mix(p.minerals_mined as u64);
        }
        for s in &self.storms {
            mix(s.pos.x.0 as u64);
            mix(s.pos.y.0 as u64);
            mix(s.ticks_left as u64);
        }
        for (i, e) in self.entities.iter().enumerate() {
            if !e.alive {
                continue;
            }
            mix(i as u64);
            mix(e.pos.x.0 as u64);
            mix(e.pos.y.0 as u64);
            mix(e.hp as u64);
            mix(e.owner as u64);
            mix(e.amount as u64);
            mix(e.carry_gas as u64);
            mix(e.cooldown as u64);
            mix(e.progress as u64);
            mix(e.order_queue.len() as u64);
            mix(e.energy as u64);
            mix(e.decay as u64);
            mix(e.sieged as u64);
            mix(e.burrowed as u64);
        }
        h
    }
}
