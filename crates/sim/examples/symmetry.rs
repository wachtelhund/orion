//! Map symmetry audit: a fair mirror map must be exactly 180-degree
//! rotationally symmetric — terrain, elevation, resources, and starts.

use orion_sim::map::{meridian, TilePos};

fn main() {
    let m = meridian();
    let (w, h) = (m.width, m.height);
    let mirror = |t: TilePos| TilePos::new(w - 1 - t.x, h - 1 - t.y);

    let mut terrain_bad = 0;
    let mut elev_bad = 0;
    for y in 0..h {
        for x in 0..w {
            let mt = mirror(TilePos::new(x, y));
            if m.walkable(x, y) != m.walkable(mt.x, mt.y) {
                terrain_bad += 1;
                if terrain_bad <= 5 {
                    println!("walkable asym at ({x},{y}) vs ({},{})", mt.x, mt.y);
                }
            }
            if m.elev_at(x, y) != m.elev_at(mt.x, mt.y) {
                elev_bad += 1;
                if elev_bad <= 5 {
                    println!("elev asym at ({x},{y})={} vs ({},{})={}",
                        m.elev_at(x, y), mt.x, mt.y, m.elev_at(mt.x, mt.y));
                }
            }
        }
    }

    let mut mins: Vec<(i32, i32, i32)> =
        m.minerals.iter().map(|&(t, amt)| (t.x, t.y, amt)).collect();
    let mut mins_mirrored: Vec<(i32, i32, i32)> = m
        .minerals
        .iter()
        .map(|&(t, amt)| {
            let mt = mirror(t);
            (mt.x, mt.y, amt)
        })
        .collect();
    mins.sort();
    mins_mirrored.sort();
    let min_sym = mins == mins_mirrored;

    let mut gey: Vec<(i32, i32)> = m.geysers.iter().map(|&(t, _)| (t.x, t.y)).collect();
    let mut gey_m: Vec<(i32, i32)> = m
        .geysers
        .iter()
        .map(|&(t, _)| {
            let mt = mirror(t);
            (mt.x, mt.y)
        })
        .collect();
    gey.sort();
    gey_m.sort();
    let gey_sym = gey == gey_m;

    let starts_sym = m.starts.len() == 2 && mirror(m.starts[0]) == m.starts[1];

    println!("terrain asymmetric tiles: {terrain_bad}");
    println!("elev asymmetric tiles:    {elev_bad}");
    println!("minerals mirrored:        {min_sym}");
    println!("geysers mirrored:         {gey_sym}");
    println!("starts mirrored:          {starts_sym} ({:?})", m.starts);
    // Distance from each start to its nearest mineral patch center.
    for (k, &s) in m.starts.iter().enumerate() {
        let d = m
            .minerals
            .iter()
            .map(|&(t, _)| (t.x - s.x).abs() + (t.y - s.y).abs())
            .min()
            .unwrap();
        println!("start {k} at {:?}: nearest patch manhattan {d}", s);
    }
    if terrain_bad + elev_bad > 0 || !min_sym || !gey_sym || !starts_sym {
        std::process::exit(1);
    }
    println!("map is exactly 180-degree symmetric");
}
// (appended) — dump raw geyser origins for inspection
