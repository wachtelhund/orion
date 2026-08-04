//! Watch the smart bot lose: one smart-vs-legacy game with a decision
//! timeline every 20s. Diagnosis, not vibes.

use orion_sim::ai::{Bot, Difficulty};
use orion_sim::map::by_name;
use orion_sim::{EntityKind, GameData, State};

fn main() {
    let seed = 6151 + 17; // arena k=1 (smart lost this one seat-swapped)
    let mut s = State::new_with_races(
        GameData::load_default(),
        by_name("meridian").unwrap(),
        seed,
        &[0, 0],
    );
    let mut bots = [
        Bot::with_style(0, Difficulty::Hard, seed ^ 3),
        Bot::with_style(1, Difficulty::Hard, seed ^ 11),
    ];
    bots[1].legacy = true;
    for t in 0..24 * 60 * 20 {
        let mut cmds = Vec::new();
        for b in &mut bots {
            cmds.extend(b.think(&s));
        }
        s.step(&cmds);
        if t % (24 * 20) == 0 || s.winner.is_some() {
            let army = |p: u8| {
                s.entities
                    .iter()
                    .filter(|e| {
                        e.alive
                            && e.owner == p
                            && e.kind == EntityKind::Unit
                            && !s.data.units[e.def as usize].harvester
                    })
                    .count()
            };
            println!(
                "t={:4}s smart: army={:2} guard={:2} strike={:2} push={} score={:3} known={:3} min={:4} | legacy: army={:2} min={:4}",
                t / 24,
                army(0),
                bots[0].dbg_guard(),
                bots[0].dbg_strike(),
                bots[0].dbg_pushing() as u8,
                bots[0].dbg_score(),
                bots[0].dbg_known(),
                s.players[0].minerals,
                army(1),
                s.players[1].minerals,
            );
        }
        if let Some(w) = s.winner {
            println!("winner P{w} at {}s", s.tick / 24);
            return;
        }
    }
    println!("no winner (stall)");
}
