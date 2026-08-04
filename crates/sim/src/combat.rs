//! The behavior pass: turns each unit's order into a desired velocity for
//! this tick plus a list of damage events. Never moves anything — movement
//! integration happens afterwards, in one place.

use crate::fixed::{dist, dist_sq_raw, Fx, FxVec2};
use crate::map::TilePos;
use crate::path::compute_flow_field;
use crate::state::{EntityKind, Order, State, Storm};

/// How far (tiles) from the order target crowd-arrival can trigger.
const CROWD_ARRIVE_DIST: i64 = 3;
/// Consecutive stuck ticks before a unit near its goal gives up and arrives.
const CROWD_ARRIVE_TICKS: u16 = 8;

impl State {
    /// The weapon a unit currently fights with (siege mode swaps it).
    pub fn active_weapon(&self, i: usize) -> Option<&crate::data::Weapon> {
        let e = &self.entities[i];
        let d = &self.data.units[e.def as usize];
        if e.sieged {
            d.weapon_siege.as_ref().or(d.weapon.as_ref())
        } else {
            d.weapon.as_ref()
        }
    }

    /// Final damage after upgrades: attacker weapons level adds, defender
    /// armor level subtracts, floor of 1.
    fn upgraded_damage(&self, base: i32, attacker: u8, defender_idx: u32) -> i32 {
        let d = &self.entities[defender_idx as usize];
        let mut dmg = base;
        if (attacker as usize) < self.players.len() {
            dmg += self.players[attacker as usize].weapons_level as i32;
        }
        if (d.owner as usize) < self.players.len() {
            dmg -= self.players[d.owner as usize].armor_level as i32;
        }
        dmg.max(1)
    }

    pub(crate) fn tick_behavior(&mut self) -> Vec<(u32, i32, bool)> {
        self.rebuild_buckets();
        let mut hits: Vec<(u32, i32, bool)> = Vec::new();
        let n = self.entities.len();
        for i in 0..n {
            self.scratch_vel[i] = FxVec2::ZERO;
            {
                let e = &mut self.entities[i];
                if !e.alive || e.kind != EntityKind::Unit {
                    continue;
                }
                if e.cooldown > 0 {
                    e.cooldown -= 1;
                }
                // Mid-transform units are busy.
                if e.transform > 0 {
                    e.transform -= 1;
                    continue;
                }
                // Underground units hold position and do nothing.
                if e.burrowed {
                    e.engage = None;
                    continue;
                }
            }
            let order = self.entities[i].order;
            match order {
                Order::Idle => {
                    if let Some(t) = self.validated_engage(i).or_else(|| self.acquire(i)) {
                        self.entities[i].engage = Some(self.id_of(t));
                        let vel = self.attack_behavior(i, t, &mut hits);
                        self.scratch_vel[i] = vel;
                    } else {
                        self.entities[i].engage = None;
                    }
                }
                Order::Hold => {
                    // Fires at what's in range; never moves.
                    if let Some(t) = self.validated_engage(i).or_else(|| self.acquire(i)) {
                        self.entities[i].engage = Some(self.id_of(t));
                        let _ = self.attack_behavior(i, t, &mut hits);
                    } else {
                        self.entities[i].engage = None;
                    }
                }
                Order::Move { target, field } => {
                    if self.arrived(i, target) {
                        self.finish_order(i);
                    } else if !self.entities[i].sieged {
                        self.scratch_vel[i] = self.follow_field(i, field, target);
                    }
                }
                Order::AttackMove { target, field } => {
                    if let Some(t) = self.validated_engage(i).or_else(|| self.acquire(i)) {
                        self.entities[i].engage = Some(self.id_of(t));
                        let vel = self.attack_behavior(i, t, &mut hits);
                        self.scratch_vel[i] = vel;
                    } else {
                        self.entities[i].engage = None;
                        if self.arrived(i, target) {
                            self.finish_order(i);
                        } else if !self.entities[i].sieged {
                            self.scratch_vel[i] = self.follow_field(i, field, target);
                        }
                    }
                }
                Order::AttackTarget { target } => {
                    match self.get(target) {
                        None => {
                            self.finish_order(i);
                        }
                        Some(t) => {
                            let owner = self.entities[i].owner;
                            let tpos = t.pos;
                            if self.visible_to(owner, target.idx) {
                                let vel = self.attack_behavior(i, target.idx, &mut hits);
                                self.scratch_vel[i] = vel;
                            } else {
                                // Lost vision: convert to attack-move toward
                                // last seen position. No fog leak.
                                let tt = TilePos::of(tpos);
                                let field = self
                                    .fields
                                    .insert(compute_flow_field(&self.map, &self.blocked, tt));
                                self.entities[i].order =
                                    Order::AttackMove { target: tpos, field };
                            }
                        }
                    }
                }
                Order::Gather { resource, phase, field } => {
                    self.gather_behavior(i, resource, phase, field);
                }
                Order::Build { def, site, phase, field } => {
                    self.build_behavior(i, def, site, phase, field);
                }
                Order::Cast { target, field } => {
                    let pos = self.entities[i].pos;
                    let d_sq = dist_sq_raw(pos, target);
                    let range = crate::STORM_CAST_RANGE;
                    if d_sq <= (range.0 as i64) * (range.0 as i64) {
                        let owner = self.entities[i].owner;
                        if self.entities[i].energy >= crate::STORM_COST {
                            self.entities[i].energy -= crate::STORM_COST;
                            self.storms.push(Storm {
                                pos: target,
                                ticks_left: crate::STORM_DURATION,
                                owner,
                            });
                            self.events.push(crate::state::SimEvent::Cast { pos: target });
                        }
                        self.finish_order(i);
                    } else {
                        self.scratch_vel[i] = self.follow_field(i, field, target);
                    }
                }
            }
        }
        hits
    }

    /// Current engage target if it is still alive, visible, and hittable.
    fn validated_engage(&mut self, i: usize) -> Option<u32> {
        let id = self.entities[i].engage?;
        let owner = self.entities[i].owner;
        match self.get(id) {
            Some(t) if self.visible_to(owner, id.idx) && self.can_hit(i, t) => Some(id.idx),
            _ => {
                self.entities[i].engage = None;
                None
            }
        }
    }

    fn can_hit(&self, i: usize, target: &crate::state::Entity) -> bool {
        if target.burrowed {
            return false;
        }
        let Some(w) = self.active_weapon(i) else { return false };
        let flying = target.kind == EntityKind::Unit
            && self.data.units[target.def as usize].fly;
        !flying || w.air
    }

    /// Attack `t` if in range (emitting hits when off cooldown, with splash),
    /// else return a chase velocity (zero when sieged).
    fn attack_behavior(&mut self, i: usize, t: u32, hits: &mut Vec<(u32, i32, bool)>) -> FxVec2 {
        let Some(w) = self.active_weapon(i).cloned() else {
            return FxVec2::ZERO;
        };
        let sieged = self.entities[i].sieged;
        let attacker_owner = self.entities[i].owner;
        let mypos = self.entities[i].pos;
        let tpos = self.entities[t as usize].pos;
        let gap = dist(mypos, tpos) - self.radius_of(i as u32) - self.radius_of(t);
        if gap <= w.range {
            // Inside minimum range: siege guns can't depress that far.
            if gap < w.min_range {
                return FxVec2::ZERO;
            }
            if self.entities[i].cooldown == 0 {
                let dmg = self.upgraded_damage(w.damage, attacker_owner, t);
                hits.push((t, dmg, false));
                // Splash: half damage to everything else near the impact
                // (friendly fire included — position your army).
                if w.splash.0 > 0 {
                    let r_sq = (w.splash.0 as i64) * (w.splash.0 as i64);
                    for j in 0..self.entities.len() {
                        let o = &self.entities[j];
                        if j as u32 != t
                            && j != i
                            && o.alive
                            && o.kind != EntityKind::Resource
                            && dist_sq_raw(tpos, o.pos) <= r_sq
                        {
                            let half =
                                self.upgraded_damage(w.damage / 2, attacker_owner, j as u32);
                            hits.push((j as u32, half, true));
                        }
                    }
                }
                self.entities[i].cooldown = w.cooldown_ticks;
                self.events.push(crate::state::SimEvent::Attack { from: i as u32, to: t });
            }
            FxVec2::ZERO
        } else if sieged {
            FxVec2::ZERO
        } else {
            let def = self.entities[i].def;
            let step = self.data.units[def as usize].step;
            (tpos - mypos).scaled_to(step)
        }
    }

    /// Nearest visible, hittable enemy within sight: units first (bucket
    /// scan), then buildings (linear scan). Ties break by entity index.
    fn acquire(&self, i: usize) -> Option<u32> {
        if self.active_weapon(i).is_none() {
            return None;
        }
        let e = &self.entities[i];
        let owner = e.owner;
        let pos = e.pos;
        let sight = self.sight_of(i as u32) as i32;
        let t = TilePos::of(pos);
        let max_sq = {
            let s = Fx::from_int(sight);
            (s.0 as i64) * (s.0 as i64)
        };

        fn better(best: &mut Option<(i64, u32)>, d: i64, idx: u32) {
            if best.map_or(true, |(bd, bi)| (d, idx) < (bd, bi)) {
                *best = Some((d, idx));
            }
        }

        let mut best: Option<(i64, u32)> = None;
        for dy in -sight..=sight {
            for dx in -sight..=sight {
                let (x, y) = (t.x + dx, t.y + dy);
                if !self.map.in_bounds(x, y) {
                    continue;
                }
                for &j in &self.buckets[self.map.idx(x, y)] {
                    let o = &self.entities[j as usize];
                    if o.alive
                        && o.owner != owner
                        && o.owner != crate::state::NEUTRAL
                        && self.can_hit(i, o)
                    {
                        let d = dist_sq_raw(pos, o.pos);
                        if d <= max_sq && self.visible_to(owner, j) {
                            better(&mut best, d, j);
                        }
                    }
                }
            }
        }
        if best.is_none() {
            for (j, o) in self.entities.iter().enumerate() {
                if o.alive
                    && o.kind == EntityKind::Building
                    && o.owner != owner
                    && o.owner != crate::state::NEUTRAL
                {
                    let d = dist_sq_raw(pos, o.pos);
                    if d <= max_sq && self.visible_to(owner, j as u32) {
                        better(&mut best, d, j as u32);
                    }
                }
            }
        }
        best.map(|(_, idx)| idx)
    }

    /// Move-order arrival: close to target, or stuck in the crowd near it.
    pub(crate) fn arrived(&mut self, i: usize, target: FxVec2) -> bool {
        let pos = self.entities[i].pos;
        let d_sq = dist_sq_raw(pos, target);
        let half = (Fx::HALF.0 as i64) * (Fx::HALF.0 as i64);
        if d_sq <= half {
            return true;
        }
        let crowd = (Fx::from_int(CROWD_ARRIVE_DIST as i32).0 as i64).pow(2);
        if self.entities[i].stuck >= CROWD_ARRIVE_TICKS && d_sq <= crowd {
            self.entities[i].stuck = 0;
            return true;
        }
        false
    }

    /// Velocity following a flow field, steering toward the next tile center.
    /// Flyers ignore fields entirely and go straight.
    pub(crate) fn follow_field(&self, i: usize, field: u32, target: FxVec2) -> FxVec2 {
        let e = &self.entities[i];
        let d = &self.data.units[e.def as usize];
        let step = d.step;
        if d.fly {
            return (target - e.pos).clamped(step);
        }
        let tile = TilePos::of(e.pos);
        if let Some(f) = self.fields.get(field) {
            if f.cost_at(&self.map, tile) == 0 {
                return (target - e.pos).clamped(step);
            }
            if let Some((dx, dy)) = f.dir_at(&self.map, tile) {
                let next = TilePos::new(tile.x + dx, tile.y + dy).center();
                let dvec = next - e.pos;
                if dvec.len_sq_raw() == 0 {
                    return FxVec2::ZERO;
                }
                return dvec.scaled_to(step);
            }
        }
        // No field / unreachable tile: close-range direct steering only.
        let dvec = target - e.pos;
        let two = Fx::from_int(2);
        if dvec.len_sq_raw() <= (two.0 as i64) * (two.0 as i64) {
            dvec.clamped(step)
        } else {
            FxVec2::ZERO
        }
    }
}
