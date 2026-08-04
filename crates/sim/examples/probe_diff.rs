//! Difficulty ladder sanity: Hard must beat Easy convincingly.

use orion_sim::ai::{Bot, Difficulty};
use orion_sim::map::by_name;
use orion_sim::{GameData, State};

fn main() {
    let mut hard = 0;
    let mut easy = 0;
    let mut none = 0;
    for k in 0..8u64 {
        let hard_seat = (k % 2) as u8;
        let seed = k * 3271 + 5;
        let mut s = State::new_with_races(
            GameData::load_default(),
            by_name("meridian").unwrap(),
            seed,
            &[0, 0],
        );
        let mut bots = [
            Bot::with_style(0, Difficulty::Easy, seed ^ 7),
            Bot::with_style(1, Difficulty::Easy, seed ^ 21),
        ];
        bots[hard_seat as usize].difficulty = Difficulty::Hard;
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
            Some(w) if w == hard_seat => hard += 1,
            Some(_) => easy += 1,
            None => none += 1,
        }
    }
    println!("LADDER: hard {hard} - {easy} easy   stalls {none}");
}
