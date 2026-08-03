//! Balance harness: Normal bots across matchups and seeds, printing winner
//! and game length. Run after any balance change to see what it did.
//! cargo run --release -p orion-sim --example balance

use orion_sim::ai::Bot;
use orion_sim::map::meridian;
use orion_sim::{GameData, State};

fn main() {
    let matchups: [(u8, u8, &str); 4] = [
        (0, 0, "VC mirror"),
        (1, 1, "Kyth mirror"),
        (0, 1, "VC vs Kyth"),
        (1, 0, "Kyth vs VC"),
    ];
    for (ra, rb, label) in matchups {
        let mut wins = [0u32; 2];
        let mut stalls = 0u32;
        let mut total_secs = 0u32;
        let seeds: Vec<u64> = (1..=8).map(|k| k * 7919).collect();
        for &seed in &seeds {
            let mut s = State::new_with_races(
                GameData::load_default(),
                meridian(),
                seed,
                &[ra, rb],
            );
            let mut bots = [
                Bot::with_style(0, orion_sim::ai::Difficulty::Normal, seed),
                Bot::with_style(1, orion_sim::ai::Difficulty::Normal, seed.rotate_left(17)),
            ];
            let mut winner = None;
            for _ in 0..24 * 60 * 15 {
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
