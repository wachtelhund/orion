//! orion-sim: the deterministic simulation core.
//!
//! HARD RULES (lockstep determinism depends on these):
//! - No floating point anywhere in this crate's runtime logic. `f64` appears
//!   only in data loading, where decimal->fixed conversion is IEEE-exact and
//!   identical on every platform.
//! - No `HashMap`/`HashSet` iteration. Ordered structures only.
//! - No system time, no OS randomness, no thread nondeterminism.
//! - All entity iteration happens in index order.
//!
//! The renderer must never mutate sim state; the only way in is `Command`s
//! passed to `State::step`.

pub mod ai;
pub mod ai_combat;
pub mod combat;
pub mod data;
pub mod econ;
pub mod entity;
pub mod fixed;
pub mod fog;
pub mod map;
pub mod movement;
pub mod net;
pub mod path;
pub mod qa;
pub mod replay;
pub mod rng;
pub mod state;
pub mod step;

pub use data::{BuildingDef, GameData, UnitDef};
pub use entity::EntityId;
pub use fixed::{Fx, FxVec2};
pub use map::{Map, TileKind, TilePos};
pub use state::{Command, Entity, EntityKind, Order, Player, State};

/// Simulation tick rate. 24 Hz, same as StarCraft's "fastest".
pub const TICKS_PER_SEC: u32 = 24;

/// Supply cap per player.
pub const SUPPLY_CAP: u32 = 200;

/// Plasma Storm tuning. 8 pulses x 6 = 48 total damage, two casts per
/// full-energy caster, and overlapping storms do NOT stack (SC2 rule) —
/// the 72-damage stacking version deleted armies outright.
pub const STORM_COST: u16 = 100;
pub const STORM_RADIUS: fixed::Fx = fixed::Fx::from_ratio(5, 2);
pub const STORM_PULSE_DMG: i32 = 6;
pub const STORM_DURATION: u16 = 64;
pub const STORM_CAST_RANGE: fixed::Fx = fixed::Fx::from_int(8);

/// Hero ability catalog. Hardcoded per hero tag + slot — three heroes and
/// six abilities don't justify a data schema yet; revisit if they multiply.
pub mod hero {
    use crate::fixed::Fx;

    pub enum AbilityKind {
        /// Drop a zone (Storm.kind) at the target.
        Zone { kind: u8, duration: u16 },
        /// Instant: heal allied units within radius.
        Heal { amount: i32, radius: Fx },
        /// Instant: damage enemy units within radius.
        Burst { damage: i32, radius: Fx },
        /// Summon short-lived units around the hero.
        Summon { unit_tag: &'static str, count: u32, ttl: u16 },
    }

    pub struct Ability {
        pub name: &'static str,
        pub cost: u16,
        pub cast_range: Fx,
        pub kind: AbilityKind,
    }

    pub fn ability(tag: &str, slot: u8) -> Option<Ability> {
        Some(match (tag, slot) {
            ("marshal", 0) => Ability {
                name: "BARRAGE",
                cost: 75,
                cast_range: Fx::from_int(8),
                kind: AbilityKind::Zone { kind: 1, duration: 72 },
            },
            ("marshal", 1) => Ability {
                name: "OVERCHARGE",
                cost: 60,
                cast_range: Fx::ZERO,
                kind: AbilityKind::Heal { amount: 60, radius: Fx::from_int(5) },
            },
            ("broodmother", 0) => Ability {
                name: "SPAWN BROOD",
                cost: 70,
                cast_range: Fx::ZERO,
                kind: AbilityKind::Summon { unit_tag: "broodling", count: 4, ttl: 360 },
            },
            ("broodmother", 1) => Ability {
                name: "CORROSIVE CLOUD",
                cost: 75,
                cast_range: Fx::from_int(7),
                kind: AbilityKind::Zone { kind: 2, duration: 144 },
            },
            ("magnus", 0) => Ability {
                name: "MAGNETIC WELL",
                cost: 75,
                cast_range: Fx::from_int(8),
                kind: AbilityKind::Zone { kind: 3, duration: 96 },
            },
            ("magnus", 1) => Ability {
                name: "OVERLOAD",
                cost: 65,
                cast_range: Fx::ZERO,
                kind: AbilityKind::Burst { damage: 35, radius: Fx::from_int(4) },
            },
            _ => return None,
        })
    }
}
