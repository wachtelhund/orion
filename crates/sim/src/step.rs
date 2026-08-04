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
                        let has_siege =
                            self.data.units[e.def as usize].weapon_siege.is_some();
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
    fn tick_storms(&mut self, hits: &mut Vec<(u32, i32)>) {
        let radius = crate::STORM_RADIUS;
        let r_sq = (radius.0 as i64) * (radius.0 as i64);
        // Pulses ride the global clock (not per-storm phase) so "one pulse
        // per unit" holds across storms cast at different times.
        let mut struck = vec![false; self.entities.len()];
        let pulse_now = self.tick % 8 == 0;
        for k in 0..self.storms.len() {
            self.storms[k].ticks_left -= 1;
            if pulse_now {
                let pos = self.storms[k].pos;
                for (i, e) in self.entities.iter().enumerate() {
                    if e.alive
                        && !struck[i]
                        && e.kind != EntityKind::Resource
                        && dist_sq_raw(pos, e.pos) <= r_sq
                    {
                        struck[i] = true;
                        hits.push((i as u32, crate::STORM_PULSE_DMG));
                    }
                }
            }
        }
        self.storms.retain(|s| s.ticks_left > 0);
    }

    // ---- damage ----

    fn apply_damage(&mut self, hits: &[(u32, i32)]) {
        for &(target, dmg) in hits {
            let e = &mut self.entities[target as usize];
            if e.alive {
                e.hp -= dmg;
            }
        }
        for i in 0..self.entities.len() {
            let e = &self.entities[i];
            if e.alive && e.hp <= 0 && e.kind != EntityKind::Resource {
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
        let alive: Vec<u8> = (0..self.players.len() as u8)
            .filter(|&p| !self.players[p as usize].defeated)
            .collect();
        if alive.len() == 1 && self.players.len() > 1 {
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
