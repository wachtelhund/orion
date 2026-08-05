//! Combat lab: pit equal-cost clumps against each other on open ground
//! and report survivors. This isolates unit-vs-unit combat math from
//! macro — if a matchup is lopsided here, the fix is unit stats, not
//! the bot.
//!
//!   cargo run --release -p orion-sim --example combat_lab

use orion_sim::fixed::FxVec2;
use orion_sim::map::meridian;
use orion_sim::{Command, GameData, State};

fn def_id(s: &State, tag: &str) -> u16 {
    s.data.units.iter().position(|u| u.tag == tag).unwrap() as u16
}

/// Total resource value (minerals + gas) of one side's living units.
fn value(s: &State, owner: u8) -> i64 {
    s.entities
        .iter()
        .filter(|e| e.alive && e.owner == owner)
        .map(|e| {
            let d = &s.data.units[e.def as usize];
            (d.cost_minerals + d.cost_gas) as i64
        })
        .sum()
}

fn fight(name: &str, a: &[(&str, u32)], b: &[(&str, u32)]) {
    let mut s = State::new_with_races(GameData::load_default(), meridian(), 0xC0FFEE, &[0, 2]);
    // Clear the map: only the lab armies fight.
    for i in 0..s.entities.len() {
        if s.entities[i].alive {
            s.kill(i as u32);
        }
    }
    let mut ids_a = Vec::new();
    let mut ids_b = Vec::new();
    let mut cost_a = 0i64;
    let mut cost_b = 0i64;
    let mut k = 0;
    for &(tag, n) in a {
        let d = def_id(&s, tag);
        let dd = &s.data.units[d as usize];
        cost_a += ((dd.cost_minerals + dd.cost_gas) * n) as i64;
        for _ in 0..n {
            let (dx, dy) = (k % 5, k / 5);
            ids_a.push(s.spawn_unit(0, d, FxVec2::from_int(30 + dx, 36 + dy)));
            k += 1;
        }
    }
    k = 0;
    for &(tag, n) in b {
        let d = def_id(&s, tag);
        let dd = &s.data.units[d as usize];
        cost_b += ((dd.cost_minerals + dd.cost_gas) * n) as i64;
        for _ in 0..n {
            let (dx, dy) = (k % 5, k / 5);
            ids_b.push(s.spawn_unit(1, d, FxVec2::from_int(48 + dx, 36 + dy)));
            k += 1;
        }
    }
    s.step(&[
        (0, Command::AttackMove { units: ids_a, target: FxVec2::from_int(50, 38), queued: false }),
        (1, Command::AttackMove { units: ids_b, target: FxVec2::from_int(32, 38), queued: false }),
    ]);
    for _ in 0..24 * 90 {
        s.step(&[]);
        let (a_alive, b_alive) = (
            s.entities.iter().any(|e| e.alive && e.owner == 0),
            s.entities.iter().any(|e| e.alive && e.owner == 1),
        );
        if !a_alive || !b_alive {
            break;
        }
    }
    let (va, vb) = (value(&s, 0), value(&s, 1));
    let na = s.entities.iter().filter(|e| e.alive && e.owner == 0).count();
    let nb = s.entities.iter().filter(|e| e.alive && e.owner == 1).count();
    println!(
        "{name:<42} cost {cost_a:>4} vs {cost_b:>4} -> survivors A:{na:>2} ({va:>4} value)  B:{nb:>2} ({vb:>4} value)"
    );
}

fn main() {
    println!("--- VC (A) vs Ferron (B), cost-matched clumps ---");
    fight("18 trooper vs 20 arclight", &[("trooper", 18)], &[("arclight", 20)]);
    fight("10 vanguard vs 11 mauler", &[("vanguard", 10)], &[("mauler", 11)]);
    fight(
        "12 troop + 6 vang vs 12 arc + 7 maul",
        &[("trooper", 12), ("vanguard", 6)],
        &[("arclight", 12), ("mauler", 7)],
    );
    fight(
        "core army 2000: 12t+6v+2 breaker vs 14a+6m+1 lode",
        &[("trooper", 12), ("vanguard", 6), ("breaker", 2)],
        &[("arclight", 14), ("mauler", 6), ("lodestone", 1)],
    );
    println!("--- swapped seats (lab has an A-side bias; compare pairs) ---");
    fight(
        "12 arc + 7 maul vs 12 troop + 6 vang",
        &[("arclight", 12), ("mauler", 7)],
        &[("trooper", 12), ("vanguard", 6)],
    );
    fight("20 arclight vs 18 trooper", &[("arclight", 20)], &[("trooper", 18)]);
    fight("11 mauler vs 10 vanguard", &[("mauler", 11)], &[("vanguard", 10)]);
    println!("--- mirrors (sanity: should be ~even) ---");
    fight("15 trooper vs 15 trooper", &[("trooper", 15)], &[("trooper", 15)]);
    fight("15 arclight vs 15 arclight", &[("arclight", 15)], &[("arclight", 15)]);
}
