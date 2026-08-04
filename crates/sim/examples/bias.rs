//! Spawn-bias bisector: run Kyth mirrors normally and with the map's start
//! positions swapped. If the same PLAYER keeps winning, the bias is in
//! player/index update order; if the same POSITION keeps winning, it is
//! geometric (pathing/rounding).

use orion_sim::ai::{Bot, Difficulty};
use orion_sim::map::meridian;
use orion_sim::{GameData, State};

fn run(seed: u64, swap: bool, race: u8) -> Option<u8> {
    let data = GameData::load_default();
    let mut map = meridian();
    if swap {
        map.starts.swap(0, 1);
    }
    let mut st = State::new_with_races(data, map, seed, &[race, race]);
    let mut bots = [
        Bot::with_style(0, Difficulty::Hard, seed ^ 0xA5A5),
        Bot::with_style(1, Difficulty::Hard, seed ^ 0x5A5A),
    ];
    for _ in 0..24 * 60 * 20 {
        let mut cmds = Vec::new();
        for b in &mut bots {
            cmds.extend(b.think(&st));
        }
        st.step(&cmds);
        if let Some(w) = st.winner {
            return Some(w);
        }
    }
    None
}

fn main() {
    for race in [0u8, 1] {
        for swap in [false, true] {
            let mut wins = [0u32; 2];
            let mut none = 0;
            for seed in 0..8u64 {
                match run(seed * 7919 + 13, swap, race) {
                    Some(w) => wins[w as usize] += 1,
                    None => none += 1,
                }
            }
            let (p0_pos, p1_pos) = if swap { ("SE", "NW") } else { ("NW", "SE") };
            println!(
                "race={race} swap={swap:5}  P0({p0_pos}) {} - {} P1({p1_pos})   timeouts {none}",
                wins[0], wins[1]
            );
        }
    }
}
