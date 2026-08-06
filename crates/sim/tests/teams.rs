//! Team foundations: hostility, shared vision and victory are team-based
//! (with 1v1 exactly equal to the old owner-based rules — covered by the
//! determinism suite staying green).

use orion_sim::fixed::FxVec2;
use orion_sim::map::{meridian, TilePos};
use orion_sim::{EntityKind, GameData, State};

/// Meridian with four start slots — a 2v2 fixture.
fn four_start_map() -> orion_sim::map::Map {
    let mut m = meridian();
    let (w, h) = (m.width, m.height);
    m.starts.push(TilePos::new(12, h - 13));
    m.starts.push(TilePos::new(w - 13, 12));
    m
}

fn state_2v2() -> State {
    let mut s = State::new_with_races(
        GameData::load_default(),
        four_start_map(),
        42,
        &[0, 1, 2, 0],
    );
    for (p, team) in [(0u8, 0u8), (1, 0), (2, 1), (3, 1)] {
        s.players[p as usize].team = team;
    }
    s
}

fn def_u(s: &State, tag: &str) -> u16 {
    s.data.units.iter().position(|u| u.tag == tag).unwrap() as u16
}

#[test]
fn team_victory_needs_both_enemies_down() {
    let mut s = state_2v2();
    // Raze everything player 2 owns: their partner (3) still stands.
    for i in 0..s.entities.len() {
        if s.entities[i].alive && s.entities[i].owner == 2 {
            s.kill(i as u32);
        }
    }
    s.step(&[]);
    assert_eq!(s.winner, None, "half a team is not a victory");
    for i in 0..s.entities.len() {
        if s.entities[i].alive && s.entities[i].owner == 3 {
            s.kill(i as u32);
        }
    }
    s.step(&[]);
    assert_eq!(s.winner, Some(0), "lowest surviving player of the winning team");
}

#[test]
fn teammates_share_vision() {
    let mut s = state_2v2();
    s.step(&[]);
    // Player 1's start area must be visible to their teammate 0, and NOT
    // to enemy player 2.
    let start1 = s.map.starts[1];
    assert!(
        s.fog[0].visible(&s.map, start1),
        "teammate's base is shared vision"
    );
    assert!(
        !s.fog[2].visible(&s.map, start1),
        "enemies still see nothing"
    );
}

#[test]
fn no_friendly_fire_between_teammates() {
    let mut s = state_2v2();
    let trooper = def_u(&s, "trooper");
    let skitter = def_u(&s, "skitter");
    // A trooper of player 0 next to a unit of teammate 1: no auto-attack.
    let _mine = s.spawn_unit(0, trooper, FxVec2::from_int(40, 40));
    let ally = s.spawn_unit(1, skitter, FxVec2::from_int(41, 40));
    for _ in 0..48 {
        s.step(&[]);
    }
    let hp = s.get(ally).map(|e| e.hp);
    assert_eq!(
        hp,
        s.get(ally).map(|_| s.data.units[skitter as usize].hp),
        "teammate untouched"
    );
    // The same unit owned by an enemy team gets shot.
    let foe = s.spawn_unit(2, skitter, FxVec2::from_int(41, 41));
    let full = s.data.units[skitter as usize].hp;
    for _ in 0..48 {
        s.step(&[]);
    }
    let foe_hp = s.get(foe).map(|e| e.hp).unwrap_or(0);
    assert!(foe_hp < full, "enemy team is auto-acquired ({foe_hp} < {full})");
}
