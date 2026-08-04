//! Reproduce the flagged econ-stall seed and dump p0's worker orders and
//! the mineral line state at the moment income froze.

use orion_sim::ai::{Bot, Difficulty};
use orion_sim::map::{meridian, TilePos};
use orion_sim::state::RES_MINERALS;
use orion_sim::{EntityKind, GameData, Order, State};

fn main() {
    let seed = 0x9b4c4fc011826cadu64;
    let data = GameData::load_default();
    let mut st = State::new_with_races(data, meridian(), seed, &[1, 0]);
    let mut bots = [
        Bot::with_style(0, Difficulty::Normal, seed ^ 0xA5A5),
        Bot::with_style(1, Difficulty::Normal, seed.rotate_left(23) ^ 0x5A5A),
    ];
    // qa uses its own style derivation; exact styles don't matter for the
    // stall mechanism — probe until income freezes for 20s with workers up.
    let mut last = (0i64, 0u32);
    for t in 0..24 * 60 * 18 {
        let mut cmds = Vec::new();
        for b in &mut bots {
            cmds.extend(b.think(&st));
        }
        st.step(&cmds);
        let mined = (st.players[0].minerals_mined + st.players[0].gas_mined) as i64;
        if mined != last.0 {
            last = (mined, t);
        }
        if t - last.1 > 24 * 30 && t > 24 * 60 * 5 {
            println!("income frozen since t={} (now {}), mined={}", last.1, t, mined);
            for (i, e) in st.entities.iter().enumerate() {
                if e.alive
                    && e.owner == 0
                    && e.kind == EntityKind::Unit
                    && st.data.units[e.def as usize].harvester
                {
                    let field_info = e.order.field().map(|f| {
                        st.fields.get(f).map_or("FIELD FREED".to_string(), |fl| {
                            let tile = TilePos::of(e.pos);
                            format!(
                                "field#{f} target=({},{}) dir_here={:?} cost_here={}",
                                fl.target.x,
                                fl.target.y,
                                fl.dir_at(&st.map, tile),
                                fl.cost_at(&st.map, tile)
                            )
                        })
                    });
                    println!(
                        "  w#{i} pos=({:.2},{:.2}) amt={} stuck={} order={:?} {}",
                        e.pos.x.to_f32(),
                        e.pos.y.to_f32(),
                        e.amount,
                        e.stuck,
                        match &e.order {
                            Order::Gather { resource, phase, .. } => format!(
                                "Gather({:?} of e{}, alive={} amt={})",
                                phase,
                                resource.idx,
                                st.get(*resource).is_some(),
                                st.get(*resource).map_or(-1, |r| r.amount)
                            ),
                            o => format!("{o:?}"),
                        },
                        field_info.unwrap_or_default()
                    );
                }
            }
            for (i, e) in st.entities.iter().enumerate() {
                if e.alive && e.owner == 0 && e.kind == EntityKind::Building {
                    let o = st.footprint_origin(e.def, e.pos);
                    let (fw, fh) = st.data.buildings[e.def as usize].footprint;
                    println!(
                        "  bld e{} {} at ({},{})..({},{})",
                        i,
                        st.data.buildings[e.def as usize].name,
                        o.x,
                        o.y,
                        o.x + fw - 1,
                        o.y + fh - 1
                    );
                }
            }
            let near = TilePos::new(14, 14);
            for (i, e) in st.entities.iter().enumerate() {
                if e.alive
                    && e.kind == EntityKind::Resource
                    && e.def == RES_MINERALS
                    && (TilePos::of(e.pos).x - near.x).abs() < 20
                    && (TilePos::of(e.pos).y - near.y).abs() < 20
                {
                    println!(
                        "  patch e{} at ({},{}) amt={}",
                        i,
                        TilePos::of(e.pos).x,
                        TilePos::of(e.pos).y,
                        e.amount
                    );
                }
            }
            return;
        }
    }
    println!("no freeze reproduced (styles differ from qa) — run soak seed directly");
}
