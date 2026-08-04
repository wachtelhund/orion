//! Balance harness: bots across matchups, seeds, and every shipping map,
//! printing winner and game length. Run after any balance change.
//! cargo run --release -p orion-sim --example balance [seeds-per-cell]

use orion_sim::ai::{Bot, Difficulty};
use orion_sim::map::{by_name, MAP_NAMES};
use orion_sim::{GameData, State};

fn main() {
    let n: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(8);
    let matchups: [(u8, u8, &str); 9] = [
        (0, 0, "VC mirror"),
        (1, 1, "Kyth mirror"),
        (2, 2, "Ferron mirror"),
        (0, 1, "VC vs Kyth"),
        (1, 0, "Kyth vs VC"),
        (0, 2, "VC vs Ferron"),
        (2, 0, "Ferron vs VC"),
        (1, 2, "Kyth vs Ferron"),
        (2, 1, "Ferron vs Kyth"),
    ];
    let mut race_wins = [0u32; 3];
    for map_name in MAP_NAMES {
        println!("--- {map_name} (Hard, {n} seeds/matchup) ---");
        for (ra, rb, label) in matchups {
            let mut wins = [0u32; 2];
            let mut stalls = 0u32;
            let mut total_secs = 0u32;
            let seeds: Vec<u64> = (1..=n).map(|k| k * 7919).collect();
            for &seed in &seeds {
                let mut s = State::new_with_races(
                    GameData::load_default(),
                    by_name(map_name).unwrap(),
                    seed,
                    &[ra, rb],
                );
                let mut bots = [
                    Bot::with_style(0, Difficulty::Hard, seed),
                    Bot::with_style(1, Difficulty::Hard, seed.rotate_left(17)),
                ];
                let mut winner = None;
                for _ in 0..24 * 60 * 20 {
                    let mut cmds = Vec::new();
                    for bot in &mut bots {
                        cmds.extend(bot.think(&s));
                    }
                    s.step(&cmds);
                    if let Some(w) = s.winner {
                        winner = Some(w);
                        break;
                    }
                }
                match winner {
                    Some(w) => {
                        wins[w as usize] += 1;
                        total_secs += s.tick / 24;
                        if ra != rb {
                            let race = if w == 0 { ra } else { rb };
                            race_wins[race as usize] += 1;
                        }
                    }
                    None => stalls += 1,
                }
            }
            let games = seeds.len() as u32 - stalls;
            let avg = if games > 0 { total_secs / games } else { 0 };
            println!(
                "{label:12}  P1 {} - {} P2   stalls {}   avg {}:{:02}",
                wins[0],
                wins[1],
                stalls,
                avg / 60,
                avg % 60
            );
        }
    }
    println!(
        "cross-race total: VC {}  Kyth {}  Ferron {}",
        race_wins[0], race_wins[1], race_wins[2]
    );
}
