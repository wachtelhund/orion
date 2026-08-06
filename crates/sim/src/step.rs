//! The tick pipeline. `State::step` is THE deterministic function: same
//! state + same commands -> bit-identical next state, on every platform.
//!
//! System order is fixed and load-bearing:
//!   commands -> production -> behavior (econ + combat intents)
//!   -> damage/deaths -> movement -> fog -> victory -> GC

use crate::data::DefId;
use crate::entity::EntityId;
use crate::fixed::{dist_sq_raw, FxVec2};
use crate::map::TilePos;
use crate::path::compute_flow_field;
use crate::state::{
    BuildPhase, Command, EntityKind, GatherPhase, Order, State, RES_MINERALS,
};

impl State {
    pub fn step(&mut self, commands: &[(u8, Command)]) {
        self.events.clear();
        // Record prev positions for render interpolation.
        for e in self.entities.iter_mut() {
            e.prev_pos = e.pos;
        }

        // Replay capture: the full input stream, whatever its source
        // (human, bot, or remote peer) — this IS the game.
        for (p, c) in commands {
            self.command_log.push((self.tick, *p, c.clone()));
        }

        self.apply_commands(commands);
        self.tick_production();
        self.tick_energy();
        let mut damage = self.tick_behavior(); // fills scratch_vel, returns hits
        self.tick_storms(&mut damage);
        self.tick_decay();
        self.apply_damage(&damage);
        self.integrate_movement();
        self.update_fog();
        self.check_victory();
        if self.tick % 24 == 0 {
            self.sweep_fields();
        }
        self.tick += 1;
    }

    // ---- commands ----

    fn apply_commands(&mut self, commands: &[(u8, Command)]) {
        for (player, cmd) in commands {
            self.apply_command(*player, cmd);
        }
    }

    /// Every command is validated against ownership and liveness — a stale or
    /// hostile command is dropped, never a panic. In lockstep, both peers drop
    /// it identically.
    fn apply_command(&mut self, player: u8, cmd: &Command) {
        match cmd {
            Command::Move { units, target, queued } => {
                self.order_move(player, units, *target, false, *queued);
            }
            Command::AttackMove { units, target, queued } => {
                self.order_move(player, units, *target, true, *queued);
            }
            Command::AttackTarget { units, target } => {
                if self.get(*target).is_none() {
                    return;
                }
                for id in units {
                    if let Some(i) = self.owned_unit(player, *id) {
                        self.issue_order(i, Order::AttackTarget { target: *target }, false);
                    }
                }
            }
            Command::Gather { units, resource, queued } => {
                if !self.gatherable(player, *resource) {
                    return;
                }
                let field = self.approach_field(resource.idx);
                for id in units {
                    if let Some(i) = self.owned_unit(player, *id) {
                        if self.data.units[self.entities[i].def as usize].harvester {
                            self.issue_order(
                                i,
                                Order::Gather {
                                    resource: *resource,
                                    phase: GatherPhase::ToResource,
                                    field,
                                },
                                *queued,
                            );
                        }
                    }
                }
            }
            Command::Stop { units } => {
                for id in units {
                    if let Some(i) = self.owned_unit(player, *id) {
                        self.entities[i].order = Order::Idle;
                        self.entities[i].order_queue.clear();
                        self.entities[i].engage = None;
                    }
                }
            }
            Command::Hold { units } => {
                for id in units {
                    if let Some(i) = self.owned_unit(player, *id) {
                        self.entities[i].order = Order::Hold;
                        self.entities[i].order_queue.clear();
                        self.entities[i].engage = None;
                    }
                }
            }
            Command::Train { building, unit } => {
                let Some(b) = self.get(*building) else { return };
                if b.owner != player || b.kind != EntityKind::Building || b.construction.is_some() {
                    return;
                }
                let bdef = &self.data.buildings[b.def as usize];
                if !bdef.trains.contains(unit) || b.queue.len() >= 5 {
                    return;
                }
                let udef = &self.data.units[*unit as usize];
                if udef.race != self.players[player as usize].race {
                    return;
                }
                if !self.requirement_met(player, udef.requires) {
                    return;
                }
                // Heroes are unique: one alive (or queued) per player.
                if udef.hero {
                    let alive = self.entities.iter().any(|e| {
                        e.alive
                            && e.owner == player
                            && e.kind == EntityKind::Unit
                            && e.def == *unit
                    });
                    let queued = self.entities.iter().any(|e| {
                        e.alive && e.owner == player && e.queue.contains(unit)
                    });
                    if alive || queued {
                        return;
                    }
                }
                let (used, provided) = self.supply(player);
                if used + udef.supply > provided {
                    return;
                }
                let p = &self.players[player as usize];
                if p.minerals < udef.cost_minerals || p.gas < udef.cost_gas {
                    return;
                }
                self.players[player as usize].minerals -= udef.cost_minerals;
                self.players[player as usize].gas -= udef.cost_gas;
                let idx = building.idx as usize;
                self.entities[idx].queue.push(*unit);
            }
            Command::CancelConstruction { building } => {
                let Some(b) = self.get(*building) else { return };
                if b.owner != player || b.kind != EntityKind::Building || b.construction.is_none()
                {
                    return;
                }
                let bdef = &self.data.buildings[b.def as usize];
                // SC-style 75% refund on aborted construction.
                self.players[player as usize].minerals += bdef.cost_minerals * 3 / 4;
                self.players[player as usize].gas += bdef.cost_gas * 3 / 4;
                self.kill(building.idx);
            }
            Command::CancelTrain { building, slot } => {
                let Some(b) = self.get(*building) else { return };
                if b.owner != player || b.kind != EntityKind::Building {
                    return;
                }
                let idx = building.idx as usize;
                let slot = *slot as usize;
                if slot >= self.entities[idx].queue.len() {
                    return;
                }
                let unit = self.entities[idx].queue.remove(slot);
                let udef = &self.data.units[unit as usize];
                self.players[player as usize].minerals += udef.cost_minerals;
                self.players[player as usize].gas += udef.cost_gas;
                if slot == 0 {
                    self.entities[idx].progress = 0;
                }
            }
            Command::Research { building, research } => {
                let Some(b) = self.get(*building) else { return };
                if b.owner != player || b.kind != EntityKind::Building || b.construction.is_some()
                {
                    return;
                }
                let r = *research as usize;
                if r >= self.data.research.len()
                    || !self.data.buildings[b.def as usize].researches.contains(research)
                    || b.research.is_some()
                    || self.players[player as usize].research_done[r]
                {
                    return;
                }
                let rdef = &self.data.research[r];
                if let Some(pre) = rdef.requires {
                    if !self.players[player as usize].research_done[pre as usize] {
                        return;
                    }
                }
                let p = &self.players[player as usize];
                if p.minerals < rdef.cost_minerals || p.gas < rdef.cost_gas {
                    return;
                }
                self.players[player as usize].minerals -= rdef.cost_minerals;
                self.players[player as usize].gas -= rdef.cost_gas;
                self.entities[building.idx as usize].research = Some((*research, 0));
            }
            Command::Siege { units } => {
                for id in units {
                    if let Some(i) = self.owned_unit(player, *id) {
                        let e = &self.entities[i];
                        let d = &self.data.units[e.def as usize];
                        let has_siege = d.weapon_siege.is_some() || d.shield_aura.is_some();
                        if has_siege && e.transform == 0 {
                            let e = &mut self.entities[i];
                            e.sieged = !e.sieged;
                            e.transform = 36; // 1.5s transform
                            e.order = Order::Idle;
                            e.order_queue.clear();
                            e.engage = None;
                        }
                    }
                }
            }
            Command::Burrow { units } => {
                for id in units {
                    if let Some(i) = self.owned_unit(player, *id) {
                        let e = &self.entities[i];
                        if self.data.units[e.def as usize].burrow && e.transform == 0 {
                            let e = &mut self.entities[i];
                            e.burrowed = !e.burrowed;
                            e.transform = 24; // 1s dig
                            e.order = Order::Idle;
                            e.order_queue.clear();
                            e.engage = None;
                        }
                    }
                }
            }
            Command::Cast { caster, target } => {
                let Some(i) = self.owned_unit(player, *caster) else { return };
                let e = &self.entities[i];
                if self.data.units[e.def as usize].energy_max == 0
                    || e.energy < crate::STORM_COST
                {
                    return;
                }
                let target = self.map.clamp_pos(*target);
                let tt = TilePos::of(target);
                let field =
                    self.fields.insert(compute_flow_field(&self.map, &self.blocked, tt));
                self.issue_order(i, Order::Cast { target, field }, false);
            }
            Command::UseAbility { caster, slot, target } => {
                let Some(i) = self.owned_unit(player, *caster) else { return };
                let e = &self.entities[i];
                let tag = self.data.units[e.def as usize].tag.clone();
                let Some(spec) = crate::hero::ability(&tag, *slot) else { return };
                if e.energy < spec.cost || e.burrowed {
                    return;
                }
                if spec.cast_range.0 == 0 {
                    // Instant, centered on the hero.
                    self.fire_ability(i, *slot, None);
                } else {
                    let target = self.map.clamp_pos(*target);
                    let tt = TilePos::of(target);
                    let field = self
                        .fields
                        .insert(compute_flow_field(&self.map, &self.blocked, tt));
                    self.issue_order(i, Order::CastAbility { slot: *slot, target, field }, false);
                }
            }
            Command::Build { worker, building, site, queued } => {
                let Some(i) = self.owned_unit(player, *worker) else { return };
                if !self.data.units[self.entities[i].def as usize].builder {
                    return;
                }
                // Resuming an abandoned construction at this site: skip site
                // validation (the building itself blocks it) and cost (paid
                // when it was started).
                let resuming = self.unfinished_building_at(player, *building, *site).is_some();
                if !resuming {
                    if self.data.buildings[*building as usize].race
                        != self.players[player as usize].race
                    {
                        return;
                    }
                    if !self.valid_building_site(*building, *site, Some(worker.idx)) {
                        return;
                    }
                    let bdef = &self.data.buildings[*building as usize];
                    if !self.requirement_met(player, bdef.requires) {
                        return;
                    }
                    let p = &self.players[player as usize];
                    if p.minerals < bdef.cost_minerals || p.gas < bdef.cost_gas {
                        return;
                    }
                }
                // Cost is deducted when construction actually starts (worker
                // arrives), like SC. Here we only order the travel. The
                // field seeds from the whole site ring — any open side.
                let (fw, fh) = self.data.buildings[*building as usize].footprint;
                let seeds = crate::path::ring_seeds(*site, fw, fh);
                let field = self
                    .fields
                    .insert(crate::path::compute_flow_field_multi(&self.map, &self.blocked, &seeds));
                self.issue_order(
                    i,
                    Order::Build { def: *building, site: *site, phase: BuildPhase::Travel, field },
                    *queued,
                );
            }
            Command::SetRally { building, target } => {
                if let Some(b) = self.get(*building) {
                    if b.owner == player && b.kind == EntityKind::Building {
                        self.entities[building.idx as usize].rally = *target;
                    }
                }
            }
        }
    }

    /// Can this player's harvesters gather from `id`? Mineral patches and
    /// own completed extractor buildings qualify.
    pub fn gatherable(&self, player: u8, id: EntityId) -> bool {
        match self.get(id) {
            Some(e) if e.kind == EntityKind::Resource => e.def == RES_MINERALS,
            Some(e) if e.kind == EntityKind::Building => {
                e.owner == player
                    && e.construction.is_none()
                    && self.data.buildings[e.def as usize].gas_extractor
                    && e.amount > 0
            }
            _ => false,
        }
    }

    fn order_move(&mut self, player: u8, units: &[EntityId], target: FxVec2, attack: bool, queued: bool) {
        let target = self.map.clamp_pos(target);
        let tt = TilePos::of(target);
        let field = self.fields.insert(compute_flow_field(&self.map, &self.blocked, tt));
        for id in units {
            if let Some(i) = self.owned_unit(player, *id) {
                let order = if attack {
                    Order::AttackMove { target, field }
                } else {
                    Order::Move { target, field }
                };
                self.issue_order(i, order, queued);
            }
        }
    }

    /// Resolve an EntityId to an index iff it is a live unit owned by player.
    fn owned_unit(&self, player: u8, id: EntityId) -> Option<usize> {
        let e = self.get(id)?;
        (e.owner == player && e.kind == EntityKind::Unit).then_some(id.idx as usize)
    }

    /// An under-construction building of this player/def whose footprint
    /// origin is `site` — the target of a resume-construction order.
    pub fn unfinished_building_at(&self, player: u8, def: DefId, site: TilePos) -> Option<u32> {
        (0..self.entities.len() as u32).find(|&i| {
            let e = &self.entities[i as usize];
            e.alive
                && e.owner == player
                && e.kind == EntityKind::Building
                && e.def == def
                && e.construction.is_some()
                && self.footprint_origin(def, e.pos) == site
        })
    }

    // ---- production ----

    fn tick_production(&mut self) {
        for i in 0..self.entities.len() {
            let e = &self.entities[i];
            if !e.alive || e.kind != EntityKind::Building || e.construction.is_some() {
                continue;
            }
            // Active research pauses the unit queue.
            if let Some((r, p)) = e.research {
                let rdef = &self.data.research[r as usize];
                let owner = e.owner as usize;
                if p + 1 >= rdef.ticks {
                    self.players[owner].research_done[r as usize] = true;
                    if rdef.upgrades_weapons {
                        self.players[owner].weapons_level += 1;
                    } else {
                        self.players[owner].armor_level += 1;
                    }
                    self.entities[i].research = None;
                    self.events
                        .push(crate::state::SimEvent::ResearchDone { owner: owner as u8 });
                } else {
                    self.entities[i].research = Some((r, p + 1));
                }
                continue;
            }
            let Some(&front) = e.queue.first() else { continue };
            let udef = &self.data.units[front as usize];
            // Supply gate: production starts (progress 0 -> 1) only if supply
            // allows; mid-production it keeps going.
            if e.progress == 0 {
                let (used, provided) = self.supply(e.owner);
                if used + udef.supply > provided {
                    continue;
                }
            }
            let done = e.progress + 1 >= udef.build_ticks;
            let owner = e.owner;
            let def = e.def;
            let pos = e.pos;
            let rally = e.rally;
            if !done {
                self.entities[i].progress += 1;
                continue;
            }
            let origin = self.footprint_origin(def, pos);
            // No rally set: spawn on the home side (symmetric on mirrored
            // maps) instead of an arbitrary compass corner.
            let toward = if rally != pos {
                rally
            } else {
                self.map.starts[(owner as usize).min(self.map.starts.len() - 1)].center()
            };
            let Some(spawn) = self.spawn_tile_near(def, origin, toward) else { continue };
            self.entities[i].progress = 0;
            self.entities[i].queue.remove(0);
            let unit_def = front;
            let id = self.spawn_unit(owner, unit_def, spawn.center());
            self.events.push(crate::state::SimEvent::Ready { pos: spawn.center(), owner });
            if rally != pos {
                let ui = id.idx as usize;
                // Rally onto a resource: harvesters start gathering there.
                let harvester = self.data.units[unit_def as usize].harvester;
                let rally_res = harvester.then(|| self.resource_at_point(owner, rally)).flatten();
                let rt = TilePos::of(rally);
                let field = self.fields.insert(compute_flow_field(&self.map, &self.blocked, rt));
                self.entities[ui].order = match rally_res {
                    Some(res) => Order::Gather {
                        resource: res,
                        phase: GatherPhase::ToResource,
                        field,
                    },
                    None => Order::Move { target: rally, field },
                };
            }
        }
    }

    /// Gatherable resource (patch or own extractor) near a world point.
    fn resource_at_point(&self, player: u8, p: FxVec2) -> Option<EntityId> {
        let mut best: Option<(i64, u32)> = None;
        let max = crate::fixed::Fx::from_int(2);
        let max_sq = (max.0 as i64) * (max.0 as i64);
        for j in 0..self.entities.len() as u32 {
            let id = self.id_of(j);
            if !self.gatherable(player, id) {
                continue;
            }
            let d = dist_sq_raw(p, self.entities[j as usize].pos);
            if d <= max_sq && best.map_or(true, |(bd, bj)| (d, j) < (bd, bj)) {
                best = Some((d, j));
            }
        }
        best.map(|(_, j)| self.id_of(j))
    }

    /// Casters regenerate 0.5 energy/sec.
    fn tick_energy(&mut self) {
        if self.tick % 48 != 0 {
            return;
        }
        for e in self.entities.iter_mut() {
            if !e.alive || e.kind != EntityKind::Unit {
                continue;
            }
            let max = self.data.units[e.def as usize].energy_max as u16;
            if max > 0 && e.energy < max {
                e.energy += 1;
            }
        }
    }

    /// Plasma Storms pulse damage in their radius every 8 ticks. A unit
    /// inside several storms takes ONE pulse, not one per storm — stacked
    /// storms would otherwise multiply damage past any counterplay.
    /// Execute a hero ability from `i` (validated by the caller for cost).
    /// `at` targets zone abilities; instant kinds center on the hero.
    pub(crate) fn fire_ability(&mut self, i: usize, slot: u8, at: Option<crate::fixed::FxVec2>) {
        let tag = self.data.units[self.entities[i].def as usize].tag.clone();
        let Some(spec) = crate::hero::ability(&tag, slot) else { return };
        if self.entities[i].energy < spec.cost {
            return;
        }
        self.entities[i].energy -= spec.cost;
        let pos = self.entities[i].pos;
        let owner = self.entities[i].owner;
        match spec.kind {
            crate::hero::AbilityKind::Heal { amount, radius } => {
                let r_sq = (radius.0 as i64) * (radius.0 as i64);
                for j in 0..self.entities.len() {
                    let t = &self.entities[j];
                    if t.alive
                        && self.allied(t.owner, owner)
                        && t.kind == EntityKind::Unit
                        && crate::fixed::dist_sq_raw(t.pos, pos) <= r_sq
                    {
                        let cap = self.data.units[t.def as usize].hp;
                        let t = &mut self.entities[j];
                        t.hp = (t.hp + amount).min(cap);
                    }
                }
                self.events.push(crate::state::SimEvent::Cast { pos, kind: 4 });
            }
            crate::hero::AbilityKind::Burst { damage, radius } => {
                let r_sq = (radius.0 as i64) * (radius.0 as i64);
                let mut burst: Vec<(u32, i32, bool)> = Vec::new();
                for j in 0..self.entities.len() {
                    let t = &self.entities[j];
                    if t.alive
                        && self.hostile(owner, t.owner)
                        && t.kind != EntityKind::Resource
                        && crate::fixed::dist_sq_raw(t.pos, pos) <= r_sq
                    {
                        burst.push((j as u32, damage, true));
                    }
                }
                self.apply_damage(&burst);
                self.events.push(crate::state::SimEvent::Cast { pos, kind: 5 });
            }
            crate::hero::AbilityKind::Summon { unit_tag, count, ttl } => {
                if let Some(def) =
                    self.data.units.iter().position(|u| u.tag == unit_tag)
                {
                    // Integer ring offsets, in half-tiles. No float trig:
                    // libm sin/cos differ across platforms in the last ulp,
                    // and a browser peer must never desync a native one.
                    const RING: [(i32, i32); 8] =
                        [(3, 0), (0, 3), (-3, 0), (0, -3), (2, 2), (-2, 2), (-2, -2), (2, -2)];
                    for k in 0..count {
                        let (ox, oy) = RING[k as usize % RING.len()];
                        let off = crate::fixed::FxVec2::new(
                            crate::fixed::Fx::from_ratio(ox, 2),
                            crate::fixed::Fx::from_ratio(oy, 2),
                        );
                        // Summons must land on open ground — a broodling
                        // dropped into a cliff or a tree wedges there for
                        // its whole lifetime (caught by the soak QA).
                        let sp = self.walkable_near(self.map.clamp_pos(pos + off), pos);
                        let id = self.spawn_unit(owner, def as u16, sp);
                        self.entities[id.idx as usize].decay = ttl;
                    }
                    self.events.push(crate::state::SimEvent::Cast { pos, kind: 6 });
                }
            }
            crate::hero::AbilityKind::Zone { kind, duration } => {
                let zp = at.unwrap_or(pos);
                self.storms.push(crate::state::Storm { pos: zp, ticks_left: duration, owner, kind });
                self.events.push(crate::state::SimEvent::Cast { pos: zp, kind });
            }
        }
    }

    /// The wanted position if its tile is open ground, else the center of
    /// the nearest open tile (deterministic ring scan), else `fallback`.
    pub(crate) fn walkable_near(
        &self,
        want: crate::fixed::FxVec2,
        fallback: crate::fixed::FxVec2,
    ) -> crate::fixed::FxVec2 {
        let open = |x: i32, y: i32| {
            self.map.in_bounds(x, y)
                && self.map.walkable(x, y)
                && !self.blocked[self.map.idx(x, y)]
        };
        let t = crate::map::TilePos::of(want);
        if open(t.x, t.y) {
            return want;
        }
        for r in 1..=4i32 {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs().max(dy.abs()) != r {
                        continue; // ring only, fixed scan order
                    }
                    if open(t.x + dx, t.y + dy) {
                        return crate::map::TilePos::new(t.x + dx, t.y + dy).center();
                    }
                }
            }
        }
        fallback
    }

    /// Summoned units burn down their lifetime.
    pub(crate) fn tick_decay(&mut self) {
        for i in 0..self.entities.len() {
            let e = &mut self.entities[i];
            if e.alive && e.decay > 0 {
                e.decay -= 1;
                if e.decay == 0 {
                    self.kill(i as u32);
                }
            }
        }
    }

    fn tick_storms(&mut self, hits: &mut Vec<(u32, i32, bool)>) {
        // Per-kind zone stats: (radius, pulse damage, pulse period).
        // Pulses ride the global clock (not per-storm phase) so "one pulse
        // per unit per kind" holds across overlapping zones.
        fn zone_stats(kind: u8) -> (crate::fixed::Fx, i32, u32) {
            match kind {
                1 => (crate::fixed::Fx::from_ratio(5, 2), 12, 12), // barrage
                2 => (crate::fixed::Fx::from_int(3), 4, 8),        // corrosive
                3 => (crate::fixed::Fx::from_ratio(7, 2), 2, 8),   // mag well
                _ => (crate::STORM_RADIUS, crate::STORM_PULSE_DMG, 8),
            }
        }
        let mut struck = vec![[false; 4]; self.entities.len()];
        for k in 0..self.storms.len() {
            self.storms[k].ticks_left -= 1;
            let kind = self.storms[k].kind;
            let pos = self.storms[k].pos;
            let (radius, dmg, period) = zone_stats(kind);
            let r_sq = (radius.0 as i64) * (radius.0 as i64);
            if self.tick % period == 0 {
                for (i, e) in self.entities.iter().enumerate() {
                    if e.alive
                        && !struck[i][kind as usize]
                        && e.kind != EntityKind::Resource
                        && dist_sq_raw(pos, e.pos) <= r_sq
                    {
                        struck[i][kind as usize] = true;
                        hits.push((i as u32, dmg, true));
                    }
                }
            }
            // Magnetic well drags ground units toward its center.
            if kind == 3 {
                let pull = crate::fixed::Fx::from_ratio(3, 20);
                for i in 0..self.entities.len() {
                    let e = &self.entities[i];
                    if !e.alive
                        || e.kind != EntityKind::Unit
                        || e.sieged
                        || e.burrowed
                        || self.data.units[e.def as usize].fly
                    {
                        continue;
                    }
                    let d_sq = dist_sq_raw(pos, e.pos);
                    if d_sq > r_sq || d_sq == 0 {
                        continue;
                    }
                    let to = pos - e.pos;
                    let nudge = to.scaled_to(pull);
                    let next = self.map.clamp_pos(e.pos + nudge);
                    if self.map.walkable(TilePos::of(next).x, TilePos::of(next).y) {
                        self.entities[i].pos = next;
                    }
                }
            }
        }
        self.storms.retain(|s| s.ticks_left > 0);
    }

    // ---- damage ----

    fn apply_damage(&mut self, hits: &[(u32, i32, bool)]) {
        for &(target, dmg, area) in hits {
            let te = &self.entities[target as usize];
            if !te.alive {
                continue;
            }
            // Underground: direct fire misses; area damage (splash, storms)
            // still connects.
            if te.burrowed && !area {
                continue;
            }
            // Shield aura: any allied shield projector deployed in radius
            // soaks a percentage. Strongest single aura applies (no stacking);
            // index-order scan with max keeps it deterministic.
            let (tpos, towner) = (te.pos, te.owner);
            let mut soak = 0i32;
            for j in 0..self.entities.len() {
                let sp = &self.entities[j];
                if !sp.alive
                    || !self.allied(sp.owner, towner)
                    || sp.kind != EntityKind::Unit
                    || !sp.sieged
                {
                    continue;
                }
                if let Some((r, pct)) = self.data.units[sp.def as usize].shield_aura {
                    let dx = (sp.pos.x - tpos.x).0 as i64;
                    let dy = (sp.pos.y - tpos.y).0 as i64;
                    if dx * dx + dy * dy <= (r.0 as i64) * (r.0 as i64) {
                        soak = soak.max(pct);
                    }
                }
            }
            let dealt = if soak > 0 { (dmg - dmg * soak / 100).max(1) } else { dmg };
            self.entities[target as usize].hp -= dealt;
        }
        for i in 0..self.entities.len() {
            let e = &self.entities[i];
            let destructible = e.kind == EntityKind::Resource
                && (e.def == crate::state::RES_TREE || e.def == crate::state::RES_ROCK);
            if e.alive && e.hp <= 0 && (e.kind != EntityKind::Resource || destructible) {
                self.kill(i as u32);
            }
        }
    }

    // ---- victory ----

    fn check_victory(&mut self) {
        if self.winner.is_some() {
            return;
        }
        for p in 0..self.players.len() {
            let has_building = self.entities.iter().any(|e| {
                e.alive && e.owner == p as u8 && e.kind == EntityKind::Building
            });
            self.players[p].defeated = !has_building;
        }
        // Team victory: the game ends when one TEAM has standing players.
        // The winner slot is the lowest surviving player index — identical
        // to the old rule in 1v1, where team == player.
        let alive: Vec<u8> = (0..self.players.len() as u8)
            .filter(|&p| !self.players[p as usize].defeated)
            .collect();
        let teams: std::collections::BTreeSet<u8> =
            alive.iter().map(|&p| self.players[p as usize].team).collect();
        if teams.len() == 1 && !alive.is_empty() && self.players.len() > 1 {
            self.winner = Some(alive[0]);
        }
    }

    // ---- flow field GC ----

    fn sweep_fields(&mut self) {
        let mut live = vec![false; self.fields.fields.len()];
        for e in &self.entities {
            if !e.alive {
                continue;
            }
            for order in std::iter::once(&e.order).chain(e.order_queue.iter()) {
                if let Some(f) = order.field() {
                    if let Some(slot) = live.get_mut(f as usize) {
                        *slot = true;
                    }
                }
            }
        }
        self.fields.sweep(&live);
    }
}
