//! Movement integration: desired velocities + soft separation + terrain
//! resolution, in one deterministic pass.
//!
//! Separation is positional (units push each other apart), not RVO — RVO is
//! float-heavy and iteration-order-sensitive. Pairs are visited once (i < j)
//! in index order; pushes are symmetric, so the result is order-independent
//! in aggregate and bit-identical across peers.

use crate::fixed::{dist, Fx, FxVec2};
use crate::map::TilePos;
use crate::state::{EntityKind, State};

/// Max positional-correction push per tick, to keep dense crowds stable.
const MAX_PUSH: Fx = Fx::from_ratio(1, 6);

impl State {
    pub(crate) fn rebuild_buckets(&mut self) {
        for &b in &self.buckets_used {
            self.buckets[b as usize].clear();
        }
        self.buckets_used.clear();
        for i in 0..self.entities.len() {
            let e = &self.entities[i];
            if e.alive && e.kind == EntityKind::Unit {
                let t = TilePos::of(e.pos);
                if self.map.in_bounds(t.x, t.y) {
                    let bi = self.map.idx(t.x, t.y);
                    if self.buckets[bi].is_empty() {
                        self.buckets_used.push(bi as u32);
                    }
                    self.buckets[bi].push(i as u32);
                }
            }
        }
    }

    pub(crate) fn integrate_movement(&mut self) {
        let n = self.entities.len();
        let mut tentative: Vec<FxVec2> = vec![FxVec2::ZERO; n];
        let mut push: Vec<FxVec2> = vec![FxVec2::ZERO; n];

        for i in 0..n {
            let e = &self.entities[i];
            if e.alive && e.kind == EntityKind::Unit {
                tentative[i] = e.pos + self.scratch_vel[i];
            }
        }

        // Pairwise separation via the tile buckets (3x3 neighborhood).
        // Flyers only separate against other flyers; ground against ground.
        for i in 0..n {
            let e = &self.entities[i];
            if !e.alive || e.kind != EntityKind::Unit || e.burrowed {
                continue;
            }
            let i_fly = self.data.units[e.def as usize].fly;
            let t = TilePos::of(e.pos);
            let ri = self.radius_of(i as u32);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (x, y) = (t.x + dx, t.y + dy);
                    if !self.map.in_bounds(x, y) {
                        continue;
                    }
                    for &j in &self.buckets[self.map.idx(x, y)] {
                        let j = j as usize;
                        if j <= i {
                            continue; // each pair once
                        }
                        let o = &self.entities[j];
                        if !o.alive || o.burrowed {
                            continue;
                        }
                        if self.data.units[o.def as usize].fly != i_fly {
                            continue; // air and ground pass through each other
                        }
                        let rj = self.radius_of(j as u32);
                        let d = tentative[i] - tentative[j];
                        let dist_ij = d.len();
                        let overlap = ri + rj - dist_ij;
                        if overlap.0 <= 0 {
                            continue;
                        }
                        let mag = (overlap * Fx::HALF).min(MAX_PUSH);
                        let dir = if d.len_sq_raw() == 0 {
                            // Perfectly coincident: deterministic tie-break.
                            FxVec2::new(Fx::from_ratio(1, 16), Fx::ZERO)
                        } else {
                            d.scaled_to(Fx::ONE)
                        };
                        let half = dir * mag;
                        push[i] += half;
                        push[j] += FxVec2::ZERO - half;
                    }
                }
            }
        }

        // Resolve against terrain + blockers, axis-separable wall sliding.
        // Flyers only clamp to map bounds; sieged units are anchored.
        for i in 0..n {
            let e = &self.entities[i];
            if !e.alive || e.kind != EntityKind::Unit {
                continue;
            }
            if e.sieged || e.burrowed {
                continue; // deployed/underground: immune to drift and pushes
            }
            let old = e.pos;
            let from = TilePos::of(old);
            if self.data.units[e.def as usize].fly {
                let next = self.map.clamp_pos(tentative[i] + push[i]);
                let e = &mut self.entities[i];
                e.stuck = 0;
                e.pos = next;
                continue;
            }
            let cand = self.map.clamp_pos(tentative[i] + push[i]);

            let ok = |s: &State, p: FxVec2| -> bool {
                let t = TilePos::of(p);
                s.map.walkable(t.x, t.y)
                    && !s.blocked[s.map.idx(t.x, t.y)]
                    && s.map.traversable(from.x, from.y, t.x, t.y)
            };

            let next = if ok(self, cand) {
                cand
            } else {
                let x_only = FxVec2::new(cand.x, old.y);
                let y_only = FxVec2::new(old.x, cand.y);
                if ok(self, x_only) {
                    x_only
                } else if ok(self, y_only) {
                    y_only
                } else {
                    old
                }
            };

            // Crowd-arrival accounting: wanted to move but couldn't.
            let desired = self.scratch_vel[i];
            let e = &mut self.entities[i];
            if desired.len_sq_raw() > 0 {
                let step = match e.kind {
                    EntityKind::Unit => self.data.units[e.def as usize].step,
                    _ => Fx::ZERO,
                };
                let moved = dist(old, next);
                let threshold = step * Fx::from_ratio(1, 4);
                if moved < threshold {
                    e.stuck = e.stuck.saturating_add(1);
                } else {
                    e.stuck = 0;
                }
            } else {
                e.stuck = 0;
            }
            e.pos = next;
        }
    }
}
