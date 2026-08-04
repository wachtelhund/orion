//! Map symmetry audit: a fair mirror map must be exactly 180-degree
//! rotationally symmetric — terrain, elevation, resources, expansions,
//! and starts. Checks every shipping map; exits non-zero on any breach.

use orion_sim::map::{by_name, TilePos, MAP_NAMES};

fn main() {
    let mut bad = false;
    for name in MAP_NAMES {
        let m = by_name(name).unwrap();
        let (w, h) = (m.width, m.height);
        let mirror = |t: TilePos| TilePos::new(w - 1 - t.x, h - 1 - t.y);
        // Mirror of a footprint ORIGIN is offset by the footprint size.
        let mirror_org = |t: TilePos, fw: i32, fh: i32| TilePos::new(w - fw - t.x, h - fh - t.y);

        let mut terrain_bad = 0;
        let mut elev_bad = 0;
        for y in 0..h {
            for x in 0..w {
                let mt = mirror(TilePos::new(x, y));
                if m.walkable(x, y) != m.walkable(mt.x, mt.y) {
                    terrain_bad += 1;
                    if terrain_bad <= 5 {
                        println!("{name}: walkable asym ({x},{y}) vs ({},{})", mt.x, mt.y);
                    }
                }
                if m.elev_at(x, y) != m.elev_at(mt.x, mt.y) {
                    elev_bad += 1;
                    if elev_bad <= 5 {
                        println!("{name}: elev asym ({x},{y})");
                    }
                }
            }
        }

        let sym_set = |pts: &[(i32, i32)], mirrored: &[(i32, i32)]| -> bool {
            let mut a = pts.to_vec();
            let mut b = mirrored.to_vec();
            a.sort();
            b.sort();
            a == b
        };
        let mins: Vec<(i32, i32)> = m.minerals.iter().map(|&(t, _)| (t.x, t.y)).collect();
        let mins_m: Vec<(i32, i32)> = m
            .minerals
            .iter()
            .map(|&(t, _)| {
                let mt = mirror(t);
                (mt.x, mt.y)
            })
            .collect();
        let min_sym = sym_set(&mins, &mins_m);

        let gey: Vec<(i32, i32)> = m.geysers.iter().map(|&(t, _)| (t.x, t.y)).collect();
        let gey_m: Vec<(i32, i32)> = m
            .geysers
            .iter()
            .map(|&(t, _)| {
                let mt = mirror_org(t, 2, 2);
                (mt.x, mt.y)
            })
            .collect();
        let gey_sym = sym_set(&gey, &gey_m);

        let exp: Vec<(i32, i32)> = m.expansions.iter().map(|t| (t.x, t.y)).collect();
        let exp_m: Vec<(i32, i32)> = m
            .expansions
            .iter()
            .map(|t| {
                let mt = mirror_org(*t, 3, 3);
                (mt.x, mt.y)
            })
            .collect();
        let exp_sym = sym_set(&exp, &exp_m);

        let tr: Vec<(i32, i32)> = m.trees.iter().map(|t| (t.x, t.y)).collect();
        let tr_m: Vec<(i32, i32)> = m
            .trees
            .iter()
            .map(|t| {
                let mt = mirror(*t);
                (mt.x, mt.y)
            })
            .collect();
        let tree_sym = sym_set(&tr, &tr_m);
        let rk: Vec<(i32, i32)> = m.rocks.iter().map(|t| (t.x, t.y)).collect();
        let rk_m: Vec<(i32, i32)> = m
            .rocks
            .iter()
            .map(|t| {
                let mt = mirror(*t);
                (mt.x, mt.y)
            })
            .collect();
        let rock_sym = sym_set(&rk, &rk_m);

        let starts_sym = m.starts.len() == 2 && mirror(m.starts[0]) == m.starts[1];

        let ok = terrain_bad + elev_bad == 0
            && min_sym
            && gey_sym
            && exp_sym
            && starts_sym
            && tree_sym
            && rock_sym;
        println!(
            "{name}: terrain {terrain_bad} elev {elev_bad} minerals {} geysers {} expansions {} starts {} trees {} ({}) rocks {} -> {}",
            min_sym, gey_sym, exp_sym, starts_sym, tree_sym, m.trees.len(), rock_sym,
            if ok { "SYMMETRIC" } else { "ASYMMETRIC" }
        );
        bad |= !ok;
    }
    if bad {
        std::process::exit(1);
    }
}
