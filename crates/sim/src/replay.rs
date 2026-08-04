//! Replays: seed + races + map + the command stream. Because the sim is
//! bit-deterministic and mutates only through `State::step`, that tuple
//! reproduces a game exactly — a replay is an input recording, SC-style,
//! and weighs kilobytes.

use serde::{Deserialize, Serialize};

use crate::state::{Command, State};
use crate::GameData;

pub const REPLAY_VERSION: u32 = 1;

#[derive(Clone, Serialize, Deserialize)]
pub struct Replay {
    pub version: u32,
    /// Map name resolved via `map::by_name`.
    pub map: String,
    pub seed: u64,
    pub races: Vec<u8>,
    pub player_names: Vec<String>,
    pub winner: Option<u8>,
    pub duration_ticks: u32,
    pub commands: Vec<(u32, u8, Command)>,
}

impl Replay {
    /// Snapshot a finished (or in-progress) game.
    pub fn from_state(s: &State, map_name: &str, player_names: Vec<String>) -> Replay {
        Replay {
            version: REPLAY_VERSION,
            map: map_name.to_string(),
            seed: s.seed,
            races: s.players.iter().map(|p| p.race).collect(),
            player_names,
            winner: s.winner,
            duration_ticks: s.tick,
            commands: s.command_log.clone(),
        }
    }

    /// Fresh state at tick 0 for this replay. Feed it ticks via
    /// `commands_for` (or use `resimulate` to fast-forward headlessly).
    pub fn start_state(&self, data: GameData) -> Option<State> {
        let map = crate::map::by_name(&self.map)?;
        Some(State::new_with_races(data, map, self.seed, &self.races))
    }

    /// The command slice scheduled for `tick`. `cursor` tracks progress —
    /// pass 0 at tick 0 and reuse the returned cursor each tick.
    pub fn commands_for(&self, tick: u32, cursor: &mut usize) -> Vec<(u8, Command)> {
        let mut out = Vec::new();
        while *cursor < self.commands.len() && self.commands[*cursor].0 == tick {
            let (_, p, c) = &self.commands[*cursor];
            out.push((*p, c.clone()));
            *cursor += 1;
        }
        out
    }

    /// Headless full re-simulation; returns the final state.
    pub fn resimulate(&self, data: GameData) -> Option<State> {
        let mut s = self.start_state(data)?;
        let mut cursor = 0usize;
        while s.tick < self.duration_ticks {
            let cmds = self.commands_for(s.tick, &mut cursor);
            s.step(&cmds);
        }
        Some(s)
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string(self).expect("replay serialization cannot fail")
    }

    pub fn from_ron(src: &str) -> Result<Replay, String> {
        let r: Replay = ron::de::from_str(src).map_err(|e| e.to_string())?;
        if r.version != REPLAY_VERSION {
            return Err(format!("replay version {} (client speaks {})", r.version, REPLAY_VERSION));
        }
        Ok(r)
    }
}
