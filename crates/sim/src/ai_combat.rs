//! Combat evaluation for AI decisions: a Lanchester-hybrid time-to-kill
//! model over two unit sets. Not a full forward simulation — but unlike a
//! supply count it understands range classes, air/ground targeting,
//! splash, and upgrades, which is what engagement decisions actually need.
//! Pure integer math: deterministic everywhere.

use crate::data::UnitDef;
use crate::state::State;

/// Aggregate combat profile of one side.
#[derive(Clone, Copy, Default, Debug)]
pub struct SidePower {
    /// Damage per second vs ground targets, x100.
    pub dps_ground: i64,
    /// Damage per second vs air targets, x100.
    pub dps_air: i64,
    /// Total hit points standing on the ground / in the air.
    pub hp_ground: i64,
    pub hp_air: i64,
}

impl SidePower {
    pub fn add_unit(&mut self, d: &UnitDef, hp: i32, weapons_level: u32) {
        let hp = hp.max(0) as i64;
        if d.fly {
            self.hp_air += hp;
        } else {
            self.hp_ground += hp;
        }
        // Prefer the stronger of mobile/siege weapon profiles.
        let mut best_g = 0i64;
        let mut best_a = 0i64;
        for w in [d.weapon.as_ref(), d.weapon_siege.as_ref()].into_iter().flatten() {
            let dmg = w.damage as i64 + weapons_level as i64;
            let cd_ticks = w.cooldown_ticks.max(1) as i64;
            let mut dps = dmg * 100 * crate::TICKS_PER_SEC as i64 / cd_ticks;
            // Splash roughly multiplies effect against massed targets.
            if w.splash.0 > 0 {
                dps = dps * 3 / 2;
            }
            best_g = best_g.max(dps);
            if w.air {
                best_a = best_a.max(dps);
            }
        }
        self.dps_ground += best_g;
        self.dps_air += best_a;
    }
}

/// Score a prospective fight: >100 means `us` wins, <100 means they do,
/// scale is the ratio of their time-to-die vs ours (x100, clamped).
/// A side that cannot shoot what the other side fields loses outright.
pub fn engagement_score(us: SidePower, them: SidePower) -> i64 {
    let their_ttd = time_to_die(&them, &us);
    let our_ttd = time_to_die(&us, &them);
    match (their_ttd, our_ttd) {
        (None, None) => 100,            // nobody can hurt anybody
        (Some(_), None) => 400,         // we hurt them, they can't touch us
        (None, Some(_)) => 25,          // we cannot even shoot back
        (Some(t), Some(o)) => (o * 100 / t.max(1)).clamp(10, 400),
    }
}

/// Ticks until `victim` runs out of HP under `shooter` fire, None if the
/// shooter cannot damage what the victim fields.
fn time_to_die(victim: &SidePower, shooter: &SidePower) -> Option<i64> {
    let total_hp = victim.hp_ground + victim.hp_air;
    if total_hp == 0 {
        return Some(0);
    }
    // Weighted: ground HP falls to ground DPS, air HP to air DPS.
    let mut ticks = 0i64;
    if victim.hp_ground > 0 {
        if shooter.dps_ground == 0 {
            return None;
        }
        ticks = ticks.max(victim.hp_ground * 100 * crate::TICKS_PER_SEC as i64 / shooter.dps_ground);
    }
    if victim.hp_air > 0 {
        if shooter.dps_air == 0 {
            return None;
        }
        ticks = ticks.max(victim.hp_air * 100 * crate::TICKS_PER_SEC as i64 / shooter.dps_air);
    }
    Some(ticks.max(1))
}

/// Power of a set of own units (by entity index).
pub fn power_of_units(s: &State, ids: &[u32], weapons_level: u32) -> SidePower {
    let mut p = SidePower::default();
    for &i in ids {
        let e = &s.entities[i as usize];
        if e.alive {
            p.add_unit(&s.data.units[e.def as usize], e.hp, weapons_level);
        }
    }
    p
}
