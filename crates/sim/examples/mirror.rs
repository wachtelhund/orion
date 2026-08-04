//! Mirror-covariance probe. Game A: Kyth mirror, P0 NW / P1 SE. Game B:
//! spawns and bot styles swapped. On a point-symmetric map with a point-
//! symmetric sim, B is exactly A with players relabeled and positions
//! mirrored, forever. The first tick where that breaks names the
//! asymmetric subsystem — the source of the SE spawn bias.

use orion_sim::ai::{Bot, Difficulty};
use orion_sim::fixed::Fx;
use orion_sim::map::meridian;
use orion_sim::{EntityKind, GameData, State};

fn snapshot(s: &State, swap_owner: bool, mirror: bool) -> Vec<(u8, u8, u16, i64, i64, i32)> {
    let w = Fx::from_int(s.map.width);
    let h = Fx::from_int(s.map.height);
    let mut v: Vec<_> = s
        .entities
        .iter()
        .filter(|e| e.alive)
        .map(|e| {
            let owner = if swap_owner && e.owner < 2 { 1 - e.owner } else { e.owner };
            let (px, py) = if mirror {
                ((w - e.pos.x).0 as i64, (h - e.pos.y).0 as i64)
            } else {
                (e.pos.x.0 as i64, e.pos.y.0 as i64)
            };
            let kind = match e.kind {
                EntityKind::Unit => 0u8,
                EntityKind::Building => 1,
                EntityKind::Resource => 2,
            };
            (kind, owner, e.def, px, py, e.hp)
        })
        .collect();
    v.sort();
    v
}

fn main() {
    let seed = 13u64;
    let data_a = GameData::load_default();
    let data_b = GameData::load_default();
    let map_a = meridian();
    let mut map_b = meridian();
    map_b.starts.swap(0, 1);

    let mut a = State::new_with_races(data_a, map_a, seed, &[1, 1]);
    let mut b = State::new_with_races(data_b, map_b, seed, &[1, 1]);
    let (s0, s1) = (0xA5A5u64, 0x5A5Au64);
    // Same styles, swapped spawns: game B's P0 is game A's P0 relocated to
    // the SE main. If the sim is point-symmetric, B is exactly A mirrored.
    let mut bots_a =
        [Bot::with_style(0, Difficulty::Hard, s0), Bot::with_style(1, Difficulty::Hard, s1)];
    let mut bots_b =
        [Bot::with_style(0, Difficulty::Hard, s0), Bot::with_style(1, Difficulty::Hard, s1)];

    for tick in 0..24 * 60 * 15 {
        let sa = snapshot(&a, false, false);
        let sb = snapshot(&b, false, true);
        if sa != sb {
            println!("MIRROR BROKEN at tick {tick} ({}s)", tick / 24);
            let in_a: Vec<_> = sa.iter().filter(|x| !sb.contains(x)).take(6).collect();
            let in_b: Vec<_> = sb.iter().filter(|x| !sa.contains(x)).take(6).collect();
            for x in &in_a {
                println!(
                    "  A-only: kind={} owner={} def={} pos=({:.3},{:.3}) hp={}",
                    x.0,
                    x.1,
                    x.2,
                    x.3 as f64 / 65536.0,
                    x.4 as f64 / 65536.0,
                    x.5
                );
            }
            for x in &in_b {
                println!(
                    "  B-only: kind={} owner={} def={} pos=({:.3},{:.3}) hp={}",
                    x.0,
                    x.1,
                    x.2,
                    x.3 as f64 / 65536.0,
                    x.4 as f64 / 65536.0,
                    x.5
                );
            }
            // Dump worker orders + field targets to localize the asymmetry.
            for (tag, st) in [("A", &a), ("B", &b)] {
                for (i, e) in st.entities.iter().enumerate() {
                    if e.alive && e.kind == EntityKind::Unit && e.owner == 1 {
                        let field = e.order.field().and_then(|f| st.fields.get(f));
                        let ft = field.map(|f| (f.target.x, f.target.y));
                        println!(
                            "  {tag} P1 worker#{i} pos=({:.3},{:.3}) order={:?} field_target={:?}",
                            e.pos.x.to_f32(),
                            e.pos.y.to_f32(),
                            e.order,
                            ft
                        );
                    }
                }
            }
            return;
        }
        let mut ca = Vec::new();
        for bot in &mut bots_a {
            ca.extend(bot.think(&a));
        }
        let mut cb = Vec::new();
        for bot in &mut bots_b {
            cb.extend(bot.think(&b));
        }
        a.step(&ca);
        b.step(&cb);
    }
    println!("mirror held for 15 minutes — sim is point-symmetric on this run");
}
