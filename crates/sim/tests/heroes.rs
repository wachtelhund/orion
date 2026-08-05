//! Hero abilities: zones, heals, bursts, summons, uniqueness.

use orion_sim::fixed::FxVec2;
use orion_sim::map::meridian;
use orion_sim::{Command, GameData, State};

fn def_id(s: &State, tag: &str) -> u16 {
    s.data.units.iter().position(|u| u.tag == tag).unwrap() as u16
}

fn arena() -> State {
    State::new(GameData::load_default(), meridian(), 99)
}

#[test]
fn barrage_zone_damages_a_clump() {
    let mut s = arena();
    let marshal = def_id(&s, "marshal");
    let skitter = def_id(&s, "skitter");
    let hero = s.spawn_unit(0, marshal, FxVec2::from_int(40, 40));
    s.entities[hero.idx as usize].energy = 200;
    let victims: Vec<_> =
        (0..3).map(|k| s.spawn_unit(1, skitter, FxVec2::from_int(45, 39 + k))).collect();
    let hp0: i32 = victims.iter().map(|v| s.get(*v).unwrap().hp).sum();
    s.step(&[(0, Command::UseAbility { caster: hero, slot: 0, target: FxVec2::from_int(45, 40) })]);
    for _ in 0..90 {
        s.step(&[]);
    }
    let hp1: i32 =
        victims.iter().map(|v| s.get(*v).map_or(0, |e| e.hp.max(0))).sum();
    assert!(hp1 < hp0, "barrage must hurt the clump ({hp0} -> {hp1})");
    assert!(s.get(hero).unwrap().energy < 200, "energy must be spent");
}

#[test]
fn overcharge_heals_allies() {
    let mut s = arena();
    let marshal = def_id(&s, "marshal");
    let trooper = def_id(&s, "trooper");
    let hero = s.spawn_unit(0, marshal, FxVec2::from_int(40, 40));
    s.entities[hero.idx as usize].energy = 200;
    let hurt = s.spawn_unit(0, trooper, FxVec2::from_int(41, 40));
    s.entities[hurt.idx as usize].hp = 10;
    s.step(&[(0, Command::UseAbility { caster: hero, slot: 1, target: FxVec2::from_int(40, 40) })]);
    s.step(&[]);
    let hp = s.get(hurt).unwrap().hp;
    assert!(hp > 10 && hp <= 45, "trooper healed to {hp}, capped at max");
}

#[test]
fn brood_summons_fight_then_expire() {
    let mut s = arena();
    let bm = def_id(&s, "broodmother");
    let hero = s.spawn_unit(1, bm, FxVec2::from_int(40, 40));
    s.entities[hero.idx as usize].energy = 200;
    let before = s.entities.iter().filter(|e| e.alive && e.owner == 1).count();
    s.step(&[(1, Command::UseAbility { caster: hero, slot: 0, target: FxVec2::from_int(40, 40) })]);
    s.step(&[]);
    let during = s.entities.iter().filter(|e| e.alive && e.owner == 1).count();
    assert_eq!(during, before + 4, "four broodlings spawn");
    for _ in 0..400 {
        s.step(&[]);
    }
    let after = s.entities.iter().filter(|e| e.alive && e.owner == 1).count();
    assert_eq!(after, before, "broodlings decay away");
}

#[test]
fn magnetic_well_pulls_and_overload_burns() {
    let mut s = arena();
    let magnus = def_id(&s, "magnus");
    let trooper = def_id(&s, "trooper");
    let hero = s.spawn_unit(0, magnus, FxVec2::from_int(40, 40));
    s.entities[hero.idx as usize].energy = 200;
    let victim = s.spawn_unit(1, trooper, FxVec2::from_int(47, 46));
    // Well at a point 2.5 tiles from the victim: it should get dragged in.
    let well = FxVec2::from_int(45, 45);
    let d0 = orion_sim::fixed::dist_sq_raw(s.get(victim).unwrap().pos, well);
    s.step(&[(0, Command::UseAbility { caster: hero, slot: 0, target: well })]);
    let mut d1 = d0;
    for _ in 0..20 {
        s.step(&[]);
        if let Some(v) = s.get(victim) {
            d1 = orion_sim::fixed::dist_sq_raw(v.pos, well);
        } else {
            break; // dragged into the hero's guns — pull definitely worked
        }
    }
    assert!(d1 < d0, "well must drag the victim closer ({d0} -> {d1})");

    // Overload: instant burn around the hero.
    let close = s.spawn_unit(1, trooper, FxVec2::from_int(41, 41));
    let hp0 = s.get(close).unwrap().hp;
    s.entities[hero.idx as usize].energy = 200;
    s.step(&[(0, Command::UseAbility { caster: hero, slot: 1, target: FxVec2::from_int(40, 40) })]);
    s.step(&[]);
    assert!(s.get(close).unwrap().hp < hp0, "overload must damage nearby enemies");
}

#[test]
fn heroes_are_unique_per_player() {
    let mut s = arena();
    let marshal = def_id(&s, "marshal");
    let _hero = s.spawn_unit(0, marshal, FxVec2::from_int(40, 40));
    // Find the barracks-equivalent isn't placed in a fresh arena; validate
    // via Train directly against an HQ we spawn.
    let barracks_def = s
        .data
        .buildings
        .iter()
        .position(|b| b.trains.contains(&marshal))
        .unwrap() as u16;
    let b = s.spawn_building(0, barracks_def, orion_sim::TilePos::new(50, 50), false);
    s.players[0].minerals = 10_000;
    s.players[0].gas = 10_000;
    s.step(&[(0, Command::Train { building: b, unit: marshal })]);
    let queued = s.get(b).unwrap().queue.len();
    assert_eq!(queued, 0, "second marshal must be rejected while one lives");
}
