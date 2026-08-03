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
pub mod combat;
pub mod data;
pub mod econ;
pub mod entity;
pub mod fixed;
pub mod fog;
pub mod map;
pub mod movement;
pub mod path;
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

/// Plasma Storm tuning.
pub const STORM_COST: u16 = 75;
pub const STORM_RADIUS: fixed::Fx = fixed::Fx::from_ratio(5, 2);
pub const STORM_PULSE_DMG: i32 = 8;
pub const STORM_DURATION: u16 = 72;
pub const STORM_CAST_RANGE: fixed::Fx = fixed::Fx::from_int(8);
