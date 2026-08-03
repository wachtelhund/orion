//! 200-unit pathing stress test — the de-risking milestone from SPEC.md.
//! Spawns 100 units per side on opposite ends of the map, orders repeated
//! cross-map attack-moves through ramps and chokepoints, and measures tick
//! cost. Run with: cargo run --release -p orion-sim --example stress

use std::time::Instant;

use orion_sim::map::meridian;
use orion_sim::{Command, EntityKind, FxVec2, GameData, State};

fn main() {
    let data = GameData::load_default();
    let map = meridian();
    let mut s = State::new(data, map, 42);

    // 100 extra troopers per player, packed near each main's ramp.
    let trooper = s.data.unit_tag("trooper");
    for p in 0..2u8 {
        let start = s.map.starts[p as usize];
        for k in 0..100 {
            let dx = (k % 10) as i32;
            let dy = (k / 10) as i32;
            let pos = FxVec2::from_int(
                (start.x - 5 + dx).clamp(2, s.map.width - 3),
                (start.y - 5 + dy).clamp(2, s.map.height - 3),
            );
            s.spawn_unit(p, trooper, pos);
        }
    }

    let unit_count = s
        .entities
        .iter()
        .filter(|e| e.alive && e.kind == EntityKind::Unit)
        .count();
    println!("units alive: {unit_count}");

    let all_units = |s: &State, p: u8| -> Vec<_> {
        (0..s.entities.len() as u32)
            .filter(|&i| {
                let e = &s.entities[i as usize];
                e.alive && e.owner == p && e.kind == EntityKind::Unit
            })
            .map(|i| s.id_of(i))
            .collect()
    };

    const TICKS: u32 = 24 * 60; // one simulated minute
    let mut worst = 0u128;
    let mut total = 0u128;
    let start_time = Instant::now();

    for tick in 0..TICKS {
        let mut cmds = Vec::new();
        // Cross-map orders every 15s: both armies swap sides, colliding in
        // the middle and squeezing through chokepoints under fire.
        if tick % (24 * 15) == 0 {
            let t0 = s.map.starts[1];
            let t1 = s.map.starts[0];
            cmds.push((0, Command::AttackMove {
                units: all_units(&s, 0),
                target: t0.center(),
                queued: false,
            }));
            cmds.push((1, Command::AttackMove {
                units: all_units(&s, 1),
                target: t1.center(),
                queued: false,
            }));
        }
        let t = Instant::now();
        s.step(&cmds);
        let dt = t.elapsed().as_micros();
        worst = worst.max(dt);
        total += dt;
    }

    let elapsed = start_time.elapsed();
    let avg_us = total / TICKS as u128;
    let alive_after = s
        .entities
        .iter()
        .filter(|e| e.alive && e.kind == EntityKind::Unit)
        .count();
    println!("simulated {TICKS} ticks ({}s of game time)", TICKS / 24);
    println!("wall time: {elapsed:?}");
    println!("avg tick: {avg_us} µs   worst tick: {worst} µs");
    println!("units alive after combat: {alive_after}");
    println!("checksum: {:#018x}", s.checksum());

    // Budget: a tick must fit comfortably inside 1/24s (41,666 µs) with
    // room for render + net. Fail loudly if we blow 25% of the frame.
    assert!(
        avg_us < 10_000,
        "average tick cost {avg_us}µs exceeds 10ms budget"
    );
    println!("PASS: within tick budget");
}
