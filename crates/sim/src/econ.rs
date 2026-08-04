//! Economy behaviors: the harvest loop (minerals + gas) and worker
//! construction.

use crate::entity::EntityId;
use crate::fixed::{dist, dist_sq_raw, Fx, FxVec2};
use crate::map::TilePos;
use crate::state::{
    BuildPhase, EntityKind, GatherPhase, Order, State, CARRY_AMOUNT, MINE_TICKS, RES_MINERALS,
};

/// Center-to-center distance at which a worker counts as "at" a mineral patch.
const MINE_REACH: Fx = Fx::from_ratio(8, 5); // 1.6 tiles
/// Extractors are 2x2: a bit more reach.
const GAS_REACH: Fx = Fx::from_ratio(12, 5); // 2.4 tiles

impl State {
    pub(crate) fn gather_behavior(
        &mut self,
        i: usize,
        resource: EntityId,
        phase: GatherPhase,
        field: u32,
    ) {
        let owner = self.entities[i].owner;
        // Valid target: mineral patch, or own working extractor.
        let target_ok = self.gatherable(owner, resource);
        match phase {
            GatherPhase::ToResource => {
                if !target_ok {
                    self.retarget_gather(i, self.entities[i].pos);
                    return;
                }
                let r = &self.entities[resource.idx as usize];
                let rpos = r.pos;
                let reach = if r.kind == EntityKind::Building { GAS_REACH } else { MINE_REACH };
                if dist(self.entities[i].pos, rpos) <= reach {
                    self.entities[i].order = Order::Gather {
                        resource,
                        phase: GatherPhase::Mining { ticks_left: MINE_TICKS },
                        field,
                    };
                } else {
                    self.scratch_vel[i] = self.follow_field(i, field, rpos);
                }
            }
            GatherPhase::Mining { ticks_left } => {
                if !target_ok {
                    self.retarget_gather(i, self.entities[i].pos);
                    return;
                }
                if ticks_left > 1 {
                    self.entities[i].order = Order::Gather {
                        resource,
                        phase: GatherPhase::Mining { ticks_left: ticks_left - 1 },
                        field,
                    };
                    return;
                }
                // Trip complete: take cargo, deplete source, head home.
                let ridx = resource.idx as usize;
                let is_gas = self.entities[ridx].kind == EntityKind::Building;
                let take = CARRY_AMOUNT.min(self.entities[ridx].amount);
                self.entities[ridx].amount -= take;
                if self.entities[ridx].amount <= 0 && !is_gas {
                    self.kill(resource.idx);
                }
                self.entities[i].amount = take;
                self.entities[i].carry_gas = is_gas;
                self.head_to_depot(i, resource);
            }
            GatherPhase::ToDepot => {
                let pos = self.entities[i].pos;
                let Some(depot) = self.nearest_deposit(owner, pos) else {
                    self.finish_order(i);
                    return;
                };
                let dpos = self.entities[depot as usize].pos;
                let reach = self.radius_of(depot) + self.radius_of(i as u32) + Fx::from_ratio(3, 4);
                if dist(pos, dpos) <= reach {
                    let cargo = self.entities[i].amount as u32;
                    if self.entities[i].carry_gas {
                        self.players[owner as usize].gas += cargo;
                        self.players[owner as usize].gas_mined += cargo;
                    } else {
                        self.players[owner as usize].minerals += cargo;
                        self.players[owner as usize].minerals_mined += cargo;
                    }
                    self.entities[i].amount = 0;
                    self.entities[i].carry_gas = false;
                    // A gather trip "completes" at cargo return: queued
                    // orders (shift-queued builds, moves) take over here.
                    if !self.entities[i].order_queue.is_empty() {
                        self.finish_order(i);
                        return;
                    }
                    // Back to the source (or a nearby one if it ran out).
                    if self.gatherable(owner, resource) {
                        let f = self.approach_field(resource.idx);
                        self.entities[i].order = Order::Gather {
                            resource,
                            phase: GatherPhase::ToResource,
                            field: f,
                        };
                    } else {
                        self.retarget_gather(i, pos);
                    }
                } else {
                    self.scratch_vel[i] = self.follow_field(i, field, dpos);
                }
            }
        }
    }

    /// Source ran out: find another mineral patch nearby, else next order.
    fn retarget_gather(&mut self, i: usize, near: FxVec2) {
        let max = Fx::from_int(12);
        let max_sq = (max.0 as i64) * (max.0 as i64);
        let mut best: Option<(i64, u32)> = None;
        for (j, e) in self.entities.iter().enumerate() {
            if e.alive && e.kind == EntityKind::Resource && e.def == RES_MINERALS && e.amount > 0 {
                let d = dist_sq_raw(near, e.pos);
                if d <= max_sq && best.map_or(true, |(bd, bj)| (d, j as u32) < (bd, bj)) {
                    best = Some((d, j as u32));
                }
            }
        }
        match best {
            Some((_, j)) => {
                let rid = self.id_of(j);
                let f = self.approach_field(j);
                self.entities[i].order =
                    Order::Gather { resource: rid, phase: GatherPhase::ToResource, field: f };
            }
            None => self.finish_order(i),
        }
    }

    fn head_to_depot(&mut self, i: usize, resource: EntityId) {
        let owner = self.entities[i].owner;
        let pos = self.entities[i].pos;
        let Some(depot) = self.nearest_deposit(owner, pos) else {
            self.finish_order(i);
            return;
        };
        let f = self.approach_field(depot);
        self.entities[i].order =
            Order::Gather { resource, phase: GatherPhase::ToDepot, field: f };
    }

    /// Nearest completed own deposit building. Ties break by index.
    fn nearest_deposit(&self, owner: u8, pos: FxVec2) -> Option<u32> {
        let mut best: Option<(i64, u32)> = None;
        for (j, e) in self.entities.iter().enumerate() {
            if e.alive
                && e.owner == owner
                && e.kind == EntityKind::Building
                && e.construction.is_none()
                && self.data.buildings[e.def as usize].deposit
            {
                let d = dist_sq_raw(pos, e.pos);
                if best.map_or(true, |(bd, bj)| (d, j as u32) < (bd, bj)) {
                    best = Some((d, j as u32));
                }
            }
        }
        best.map(|(_, j)| j)
    }

    pub(crate) fn build_behavior(
        &mut self,
        i: usize,
        def: crate::data::DefId,
        site: TilePos,
        phase: BuildPhase,
        field: u32,
    ) {
        match phase {
            BuildPhase::Travel => {
                let d = &self.data.buildings[def as usize];
                let (fw, fh) = d.footprint;
                let center = FxVec2::new(
                    Fx::from_int(site.x) + Fx::from_int(fw) * Fx::HALF,
                    Fx::from_int(site.y) + Fx::from_int(fh) * Fx::HALF,
                );
                let reach = Fx::from_int(fw.max(fh)) * Fx::HALF
                    + self.radius_of(i as u32)
                    + Fx::HALF;
                if dist(self.entities[i].pos, center) > reach {
                    self.scratch_vel[i] = self.follow_field(i, field, center);
                    return;
                }
                let owner = self.entities[i].owner;
                // Resume if an unfinished building of this def already stands
                // at the site (its own footprint makes the site "invalid").
                if let Some(b) = self.unfinished_building_at(owner, def, site) {
                    let bid = self.id_of(b);
                    self.entities[i].order = Order::Build {
                        def,
                        site,
                        phase: BuildPhase::Constructing { building: bid },
                        field,
                    };
                    return;
                }
                // Fresh construction: validate, pay, place. Extractors
                // consume their geyser, inheriting its gas amount.
                let cost_m = d.cost_minerals;
                let cost_g = d.cost_gas;
                let is_extractor = d.gas_extractor;
                if !self.valid_building_site(def, site, Some(i as u32))
                    || self.players[owner as usize].minerals < cost_m
                    || self.players[owner as usize].gas < cost_g
                {
                    self.finish_order(i);
                    return;
                }
                let gas_amount = if is_extractor {
                    let Some(g) = self.geyser_at(site) else {
                        self.finish_order(i);
                        return;
                    };
                    let amount = self.entities[g as usize].amount;
                    self.kill(g); // frees the geyser's blocked tiles
                    amount
                } else {
                    0
                };
                self.players[owner as usize].minerals -= cost_m;
                self.players[owner as usize].gas -= cost_g;
                let bid = self.spawn_building(owner, def, site, true);
                self.entities[bid.idx as usize].amount = gas_amount;
                self.entities[i].order = Order::Build {
                    def,
                    site,
                    phase: BuildPhase::Constructing { building: bid },
                    field,
                };
            }
            BuildPhase::Constructing { building } => {
                let Some(b) = self.get(building) else {
                    self.finish_order(i);
                    return;
                };
                let Some(progress) = b.construction else {
                    self.finish_order(i); // finished
                    return;
                };
                let bdef = &self.data.buildings[b.def as usize];
                let total = bdef.build_ticks;
                let full_hp = bdef.hp;
                let bidx = building.idx as usize;
                if progress + 1 >= total {
                    self.entities[bidx].construction = None;
                    self.entities[bidx].hp = full_hp;
                    let bpos = self.entities[bidx].pos;
                    let bowner = self.entities[bidx].owner;
                    self.events.push(crate::state::SimEvent::BuildingDone {
                        pos: bpos,
                        owner: bowner,
                    });
                    self.finish_order(i);
                } else {
                    self.entities[bidx].construction = Some(progress + 1);
                    // HP scales up as the building completes.
                    let base = full_hp / 10;
                    let hp = base + ((full_hp - base) as i64 * (progress + 1) as i64
                        / total as i64) as i32;
                    self.entities[bidx].hp = self.entities[bidx].hp.max(hp);
                }
            }
        }
    }
}
