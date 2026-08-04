//! Destructible trees: block movement and line of sight; killing one
//! opens both.

use orion_sim::map::{meridian, TilePos};
use orion_sim::state::{RES_TREE, TREE_HP};
use orion_sim::{Command, EntityKind, GameData, State};

#[test]
fn tree_blocks_sight_until_killed() {
    let mut s = State::new(GameData::load_default(), meridian(), 9);
    // A wall of trees east of the NW start, then a probe tile behind it.
    let start = s.map.starts[0];
    let mut wall = Vec::new();
    for dy in -4..=4 {
        wall.push(s.spawn_resource(RES_TREE, TilePos::new(start.x + 5, start.y + dy), (1, 1), 0));
    }
    s.step(&[]); // fog update
    let hidden = TilePos::new(start.x + 8, start.y);
    let tree_tile = TilePos::new(start.x + 5, start.y);
    assert!(
        s.fog[0].visible(&s.map, tree_tile),
        "the tree itself must stay visible"
    );
    assert!(
        !s.fog[0].visible(&s.map, hidden),
        "tiles behind the tree wall must be hidden"
    );

    // Order units to chop the center tree down.
    let units: Vec<_> = (0..s.entities.len() as u32)
        .filter(|&i| {
            let e = &s.entities[i as usize];
            e.alive && e.owner == 0 && e.kind == EntityKind::Unit
        })
        .map(|i| s.id_of(i))
        .collect();
    assert!(!units.is_empty());
    let target = wall[4];
    assert_eq!(s.get(target).unwrap().hp, TREE_HP);
    s.step(&[(0, Command::AttackTarget { units, target })]);
    for _ in 0..24 * 60 {
        s.step(&[]);
        if s.get(target).is_none() {
            break;
        }
    }
    assert!(s.get(target).is_none(), "tree never died to attacks");
    // The tile opens for movement and the gap opens sight.
    assert!(!s.blocked[s.map.idx(start.x + 5, start.y)]);
    s.step(&[]);
    assert!(
        s.fog[0].visible(&s.map, hidden),
        "sight must open through the felled tree"
    );
}
