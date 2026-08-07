//! Serialize a builtin map to RON on stdout — for seeding custom-map
//! fixtures and testing the share/embed pipeline.
//!
//!   cargo run -p orion-sim --example dumpmap -- causeway > my-map.ron

fn main() {
    let name = std::env::args().nth(1).unwrap_or_else(|| "meridian".into());
    // "meridian4": the 2v2 test fixture — meridian plus two mirrored
    // starts, mineral lines cloned for the extra corners.
    let map = if name == "meridian4" {
        let mut m = orion_sim::map::meridian();
        let (w, h) = (m.width, m.height);
        m.starts.push(orion_sim::map::TilePos::new(12, h - 13));
        m.starts.push(orion_sim::map::TilePos::new(w - 13, 12));
        // Mirror the existing mineral lines into the fresh corners so all
        // four bases have an economy (x-mirror of each original patch).
        let extra: Vec<_> = m
            .minerals
            .iter()
            .map(|(p, amt)| (orion_sim::map::TilePos::new(p.x, h - 1 - p.y), *amt))
            .collect();
        m.minerals.extend(extra);
        let extra_g: Vec<_> = m
            .geysers
            .iter()
            .map(|(p, amt)| (orion_sim::map::TilePos::new(p.x, h - 2 - p.y), *amt))
            .collect();
        m.geysers.extend(extra_g);
        m
    } else {
        orion_sim::map::by_name(&name).expect("unknown map")
    };
    println!("{}", ron::ser::to_string(&map).expect("serialize"));
}
