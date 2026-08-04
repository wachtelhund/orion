//! A/B arena: the new intelligence vs the old open-loop bot, same race,
//! same difficulty, mirrored seats across seeds. The winrate IS the
//! measured value of the AI overhaul.

use orion_sim::ai::{Bot, Difficulty};
use orion_sim::map::by_name;
use orion_sim::{GameData, State};

fn main() {
    let n: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(8);
    let mut smart_wins = 0u32;
    let mut legacy_wins = 0u32;
    let mut stalls = 0u32;
    for map_name in ["meridian", "caverns"] {
        for race in [0u8, 1] {
            for k in 0..n {
                // Alternate which seat the smart bot takes.
                let smart_seat = (k % 2) as u8;
                let seed = k * 6151 + 17;
                let mut s = State::new_with_races(
                    GameData::load_default(),
                    by_name(map_name).unwrap(),
                    seed,
                    &[race, race],
                );
                let mut bots = [
                    Bot::with_style(0, Difficulty::Hard, seed ^ 3),
                    Bot::with_style(1, Difficulty::Hard, seed ^ 11),
                ];
                bots[(1 - smart_seat) as usize].legacy = true;
                let mut winner = None;
                for _ in 0..24 * 60 * 20 {
                    let mut cmds = Vec::new();
                    for b in &mut bots {
                        cmds.extend(b.think(&s));
                    }
                    s.step(&cmds);
                    if let Some(w) = s.winner {
                        winner = Some(w);
                        break;
                    }
                }
                match winner {
                    Some(w) if w == smart_seat => smart_wins += 1,
                    Some(_) => legacy_wins += 1,
                    None => stalls += 1,
                }
            }
        }
    }
    println!("ARENA: smart {smart_wins} - {legacy_wins} legacy   stalls {stalls}");
}
