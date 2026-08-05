//! Headless bot-vs-bot game with a status printout — dev tool for watching
//! the macro loop work. cargo run -p orion-sim --example botgame

use orion_sim::ai::{Bot, Difficulty};
use orion_sim::map::by_name;
use orion_sim::{EntityKind, GameData, State};

fn main() {
    // botgame [map] [minutes] [seed] — Hard bots when a map is named.
    let mut args = std::env::args().skip(1);
    let map_name = args.next().unwrap_or_else(|| "meridian".into());
    let minutes: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(10);
    let seed: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(7);
    let map = by_name(&map_name).expect("unknown map");
    let mut s = State::new(GameData::load_default(), map, seed);
    let mut bots = [
        Bot::with_style(0, Difficulty::Hard, seed ^ 5),
        Bot::with_style(1, Difficulty::Hard, seed ^ 77),
    ];

    for tick in 0..24 * 60 * minutes {
        let mut cmds = Vec::new();
        for bot in &mut bots {
            cmds.extend(bot.think(&s));
        }
        s.step(&cmds);
        if tick % (24 * 30) == 0 {
            print_status(&s, tick);
        }
        if let Some(w) = s.winner {
            println!("tick {tick}: PLAYER {w} WINS");
            return;
        }
    }
    println!("no winner after {minutes} minutes");
    print_status(&s, 24 * 60 * minutes);
}

fn print_status(s: &State, tick: u32) {
    print!("t={:>5} ({:>3}s) ", tick, tick / 24);
    for p in 0..2u8 {
        let mut workers = 0;
        let mut army = 0;
        let mut buildings = 0;
        let mut constructing = 0;
        for e in &s.entities {
            if !e.alive || e.owner != p {
                continue;
            }
            match e.kind {
                EntityKind::Unit => {
                    if s.data.units[e.def as usize].harvester {
                        workers += 1;
                    } else {
                        army += 1;
                    }
                }
                EntityKind::Building => {
                    if e.construction.is_some() {
                        constructing += 1;
                    } else {
                        buildings += 1;
                    }
                }
                EntityKind::Resource => {}
            }
        }
        let (used, prov) = s.supply(p);
        print!(
            "| P{p}: {}m {}g {}w {}a {}b(+{}) {}/{} ",
            s.players[p as usize].minerals,
            s.players[p as usize].gas,
            workers,
            army,
            buildings,
            constructing,
            used,
            prov
        );
    }
    println!();
}
