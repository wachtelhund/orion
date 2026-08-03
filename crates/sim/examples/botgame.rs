//! Headless bot-vs-bot game with a status printout — dev tool for watching
//! the macro loop work. cargo run -p orion-sim --example botgame

use orion_sim::ai::Bot;
use orion_sim::map::meridian;
use orion_sim::{EntityKind, GameData, State};

fn main() {
    let mut s = State::new(GameData::load_default(), meridian(), 7);
    let mut bots = [Bot::new(0), Bot::new(1)];

    for tick in 0..24 * 60 * 10 {
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
    println!("no winner after 10 minutes");
    print_status(&s, 24 * 60 * 10);
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
