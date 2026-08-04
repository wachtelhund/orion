//! Does the bot take its natural on caverns? Run one game, report base
//! counts over time.

use orion_sim::ai::{Bot, Difficulty};
use orion_sim::map::by_name;
use orion_sim::{EntityKind, GameData, State};

fn main() {
    let mut s = State::new_with_races(
        GameData::load_default(),
        by_name("caverns").unwrap(),
        7,
        &[0, 1],
    );
    let mut bots = [
        Bot::with_style(0, Difficulty::Hard, 3),
        Bot::with_style(1, Difficulty::Hard, 11),
    ];
    for t in 0..24 * 60 * 14 {
        let mut cmds = Vec::new();
        for b in &mut bots {
            cmds.extend(b.think(&s));
        }
        s.step(&cmds);
        if t % (24 * 120) == 0 || s.winner.is_some() {
            let bases = |p: u8| {
                s.entities
                    .iter()
                    .filter(|e| {
                        e.alive
                            && e.owner == p
                            && e.kind == EntityKind::Building
                            && s.data.buildings[e.def as usize].deposit
                    })
                    .count()
            };
            println!(
                "t={:4}s p0 bases={} workers={:2}m | p1 bases={} {}m",
                t / 24,
                bases(0),
                s.players[0].minerals,
                bases(1),
                s.players[1].minerals
            );
        }
        if let Some(w) = s.winner {
            println!("winner: P{w}");
            return;
        }
    }
    println!("no winner in 14 min");
}
