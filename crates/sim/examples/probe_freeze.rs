//! Catch a bot that stops spending: run a mirror game and, once a seat
//! banks over FLOAT_MIN while its production idles, dump every command
//! that seat issues (and whether Build sites validate) for a few seconds.
//!
//!   cargo run --release -p orion-sim --example probe_freeze -- [map] [seed] [race]

use orion_sim::ai::{Bot, Difficulty};
use orion_sim::map::by_name;
use orion_sim::{Command, EntityKind, GameData, State};

fn main() {
    let mut args = std::env::args().skip(1);
    let map = args.next().unwrap_or_else(|| "caverns".into());
    let seed: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(3);
    let race: u8 = args.next().and_then(|a| a.parse().ok()).unwrap_or(0);
    let mut s = State::new_with_races(
        GameData::load_default(),
        by_name(&map).unwrap(),
        seed,
        &[race, race],
    );
    let mut bots = [
        Bot::with_style(0, Difficulty::Hard, seed ^ 5),
        Bot::with_style(1, Difficulty::Hard, seed ^ 77),
    ];
    let mut dump_until = 0u32;
    let mut dumped_for: Option<u8> = None;
    for _ in 0..24 * 60 * 12 {
        let mut cmds = Vec::new();
        for b in &mut bots {
            let mine = b.think(&s);
            if s.tick < dump_until {
                if Some(b.player) == dumped_for && s.tick % 48 == 0 {
                    let stuck: Vec<String> = s
                        .entities
                        .iter()
                        .enumerate()
                        .filter(|(_, e)| {
                            e.alive
                                && e.owner == b.player
                                && matches!(e.order, orion_sim::Order::Build { .. })
                        })
                        .map(|(i, e)| {
                            format!(
                                "w#{i} ({:.2},{:.2}) {:?}",
                                e.pos.x.to_f32(),
                                e.pos.y.to_f32(),
                                e.order
                            )
                        })
                        .collect();
                    let (u, pr) = s.supply(b.player);
                    println!(
                        "t={}s P{} supply={u}/{pr} bad_sites={} builders={:?}",
                        s.tick / 24,
                        b.player,
                        b.bad_sites.len(),
                        stuck
                    );
                }
                for (p, c) in &mine {
                    if true {
                        continue;
                    }
                    #[allow(unreachable_code)]
                    if Some(*p) != dumped_for {
                        continue;
                    }
                    let extra = match c {
                        Command::Train { building, unit } => {
                            match s.get(*building) {
                                None => " [REJECT: stale building id]".to_string(),
                                Some(b) => {
                                    let bdef = &s.data.buildings[b.def as usize];
                                    let udef = &s.data.units[*unit as usize];
                                    let pl = &s.players[*p as usize];
                                    format!(
                                        " [own={} cons={} trains={} q={} race={}/{} req={} min={}>={} gas={}>={}]",
                                        b.owner == *p,
                                        b.construction.is_some(),
                                        bdef.trains.contains(unit),
                                        b.queue.len(),
                                        udef.race,
                                        pl.race,
                                        s.requirement_met(*p, udef.requires),
                                        pl.minerals,
                                        udef.cost_minerals,
                                        pl.gas,
                                        udef.cost_gas,
                                    )
                                }
                            }
                        }
                        Command::Build { building, site, worker, .. } => {
                            let bi = s
                                .get(*worker)
                                .map(|_| worker.idx);
                            let ok = s.valid_building_site(*building, *site, bi);
                            format!(
                                " [site_valid={} def={}]",
                                ok, s.data.buildings[*building as usize].tag
                            )
                        }
                        _ => String::new(),
                    };
                    let short = format!("{c:?}");
                    println!(
                        "t={}s P{p} cmd: {}{extra}",
                        s.tick / 24,
                        &short[..short.len().min(110)]
                    );
                }
            }
            cmds.extend(mine);
        }
        s.step(&cmds);
        if s.winner.is_some() {
            println!("WINNER {:?} at t={}s", s.winner, s.tick / 24);
            return;
        }
        // Arm the dump when a seat floats: bank > 2000 with < 3 queued.
        if dump_until == 0 && s.tick > 24 * 240 {
            for p in 0..2u8 {
                let bank = s.players[p as usize].minerals;
                let queued: usize = s
                    .entities
                    .iter()
                    .filter(|e| e.alive && e.owner == p && e.kind == EntityKind::Building)
                    .map(|e| e.queue.len())
                    .sum();
                if bank > 2500 && queued < 3 {
                    dump_until = s.tick + 24 * 60;
                    dumped_for = Some(p);
                    println!(
                        "--- P{p} floats {bank} minerals with {queued} queued at t={}s; dumping 12s ---",
                        s.tick / 24
                    );
                    let (used, provided) = s.supply(p);
                    println!("    supply {used}/{provided}");
                    for (i, e) in s.entities.iter().enumerate() {
                        if e.alive
                            && e.owner == p
                            && e.kind == EntityKind::Unit
                            && s.data.units[e.def as usize].harvester
                        {
                            if let orion_sim::Order::Build { .. } = &e.order {
                                println!(
                                    "    worker#{i} pos=({:.2},{:.2}) order={:?}",
                                    e.pos.x.to_f32(),
                                    e.pos.y.to_f32(),
                                    e.order
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    println!("no freeze armed (or game ran out) at t={}s", s.tick / 24);
}
