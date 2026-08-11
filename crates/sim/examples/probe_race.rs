//! Race-macro probe: one Hard-vs-Hard game with a full macro snapshot
//! every 30s — banked resources, army mix, building mix, queues. Run it
//! when one race keeps losing to see WHERE its economy goes.
//!
//!   cargo run --release -p orion-sim --example probe_race [race0] [race1] [seed]

use orion_sim::ai::{Bot, Difficulty};
use orion_sim::map::by_name;
use orion_sim::{EntityKind, GameData, State};

fn main() {
    let mut args = std::env::args().skip(1);
    let r0: u8 = args.next().and_then(|a| a.parse().ok()).unwrap_or(0);
    let r1: u8 = args.next().and_then(|a| a.parse().ok()).unwrap_or(2);
    let seed: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(11);
    let map = args.next().unwrap_or_else(|| "meridian".into());
    let mut s = State::new_with_races(
        GameData::load_default(),
        by_name(&map).unwrap(),
        seed,
        &[r0, r1],
    );
    let mut bots = [
        Bot::with_style(0, Difficulty::Hard, seed ^ 5),
        Bot::with_style(1, Difficulty::Hard, seed ^ 77),
    ];
    for t in 0..24 * 60 * 15 {
        let mut cmds = Vec::new();
        for b in &mut bots {
            cmds.extend(b.think(&s));
        }
        s.step(&cmds);
        if t % (24 * 30) == 0 || s.winner.is_some() {
            for p in 0..2u8 {
                let mut units: Vec<(String, usize)> = Vec::new();
                let mut buildings: Vec<(String, usize)> = Vec::new();
                let mut queued = 0usize;
                let mut workers = 0usize;
                for e in s.entities.iter().filter(|e| e.alive && e.owner == p) {
                    match e.kind {
                        EntityKind::Unit => {
                            let d = &s.data.units[e.def as usize];
                            if d.harvester {
                                workers += 1;
                            } else {
                                match units.iter_mut().find(|(n, _)| *n == d.tag) {
                                    Some((_, c)) => *c += 1,
                                    None => units.push((d.tag.clone(), 1)),
                                }
                            }
                        }
                        EntityKind::Building => {
                            queued += e.queue.len();
                            let d = &s.data.buildings[e.def as usize];
                            match buildings.iter_mut().find(|(n, _)| *n == d.tag) {
                                Some((_, c)) => *c += 1,
                                None => buildings.push((d.tag.clone(), 1)),
                            }
                        }
                        _ => {}
                    }
                }
                let pl = &s.players[p as usize];
                let fmt = |v: &[(String, usize)]| {
                    v.iter().map(|(n, c)| format!("{c}{}", &n[..3.min(n.len())])).collect::<Vec<_>>().join(" ")
                };
                println!(
                    "t={:4}s P{p} min={:4} gas={:4} w={:2} q={} W{}A{} | {} | {}",
                    t / 24,
                    pl.minerals,
                    pl.gas,
                    workers,
                    queued,
                    pl.weapons_level,
                    pl.armor_level,
                    fmt(&units),
                    fmt(&buildings),
                );
            }
            println!();
        }
        if s.winner.is_some() {
            println!("WINNER: P{:?}", s.winner);
            break;
        }
    }
}
