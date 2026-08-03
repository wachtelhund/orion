//! Generational entity IDs. Slots are reused; the generation catches stale
//! references (e.g. an attack order on a unit that died).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct EntityId {
    pub idx: u32,
    pub gen: u32,
}

impl EntityId {
    pub const INVALID: EntityId = EntityId { idx: u32::MAX, gen: 0 };
}
