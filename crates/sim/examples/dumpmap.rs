//! Serialize a builtin map to RON on stdout — for seeding custom-map
//! fixtures and testing the share/embed pipeline.
//!
//!   cargo run -p orion-sim --example dumpmap -- causeway > my-map.ron

fn main() {
    let name = std::env::args().nth(1).unwrap_or_else(|| "meridian".into());
    let map = orion_sim::map::by_name(&name).expect("unknown map");
    println!("{}", ron::ser::to_string(&map).expect("serialize"));
}
