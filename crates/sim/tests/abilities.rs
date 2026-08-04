//! Bulwark shield aura and Burrower burrow: the two transform abilities.

use orion_sim::fixed::FxVec2;
use orion_sim::map::meridian;
use orion_sim::{Command, GameData, State};

fn def_id(s: &State, tag: &str) -> u16 {
    s.data.units.iter().position(|u| u.tag == tag).unwrap() as u16
}

/// Fresh state with no starting entities near the middle of the map.
fn arena() -> State {
    State::new(GameData::load_default(), meridian(), 42)
}

#[test]
fn bulwark_soaks_damage_only_while_deployed() {
    // Two identical victims under fire; one stands next to a deployed
    // bulwark. Compare hp loss over the same bombardment.
    let mut s = arena();
    let trooper = def_id(&s, "trooper");
    let bulwark = def_id(&s, "bulwark");

    let shielded = s.spawn_unit(0, trooper, FxVec2::from_int(40, 38));
    let naked = s.spawn_unit(0, trooper, FxVec2::from_int(52, 38));
    let dome = s.spawn_unit(0, bulwark, FxVec2::from_int(41, 38));
    // Attackers: one enemy trooper on each victim.
    let a1 = s.spawn_unit(1, trooper, FxVec2::from_int(40, 41));
    let a2 = s.spawn_unit(1, trooper, FxVec2::from_int(52, 41));

    // Deploy the bulwark (transform takes 1.5s = 36 ticks).
    s.step(&[(0, Command::Siege { units: vec![dome] })]);
    for _ in 0..40 {
        s.step(&[]);
    }
    let hp0_shielded = s.get(shielded).unwrap().hp;
    let hp0_naked = s.get(naked).unwrap().hp;
    let cmd = |s: &mut State| {
        let c1 = Command::AttackTarget { units: vec![a1], target: shielded };
        let c2 = Command::AttackTarget { units: vec![a2], target: naked };
        s.step(&[(1, c1), (1, c2)]);
    };
    cmd(&mut s);
    for _ in 0..48 {
        s.step(&[]);
    }
    let loss_shielded = hp0_shielded - s.get(shielded).unwrap().hp;
    let loss_naked = hp0_naked - s.get(naked).unwrap().hp;
    assert!(loss_naked > 0, "control victim must take damage");
    assert!(
        loss_shielded < loss_naked,
        "aura must soak damage: shielded lost {loss_shielded}, naked lost {loss_naked}"
    );
    // 35% soak on 7 damage -> 5 per hit (floor at least 1 below raw).
    assert!(loss_shielded * 100 <= loss_naked * 75, "soak should be roughly 35 pct");
}

#[test]
fn burrowed_is_hidden_untargetable_and_immobile() {
    let mut s = arena();
    let trooper = def_id(&s, "trooper");
    let burrower = def_id(&s, "burrower");

    let bug = s.spawn_unit(1, burrower, FxVec2::from_int(44, 40));
    let gun = s.spawn_unit(0, trooper, FxVec2::from_int(44, 43));

    // Burrow (1s transform).
    s.step(&[(1, Command::Burrow { units: vec![bug] })]);
    for _ in 0..30 {
        s.step(&[]);
    }
    assert!(s.get(bug).unwrap().burrowed, "burrow must stick");
    let hp0 = s.get(bug).unwrap().hp;

    // Direct fire cannot connect; the attack order resolves without damage.
    s.step(&[(0, Command::AttackTarget { units: vec![gun], target: bug })]);
    for _ in 0..60 {
        s.step(&[]);
    }
    assert_eq!(s.get(bug).unwrap().hp, hp0, "direct fire must miss a burrowed unit");

    // Move orders are ignored underground.
    let pos0 = s.get(bug).unwrap().pos;
    s.step(&[(1, Command::Move { units: vec![bug], target: FxVec2::from_int(60, 60), queued: false })]);
    for _ in 0..30 {
        s.step(&[]);
    }
    assert_eq!(s.get(bug).unwrap().pos, pos0, "burrowed units must not move");

    // Unburrow: becomes targetable again and fights back.
    s.step(&[(1, Command::Burrow { units: vec![bug] })]);
    for _ in 0..30 {
        s.step(&[]);
    }
    assert!(!s.get(bug).unwrap().burrowed);
    s.step(&[(0, Command::AttackTarget { units: vec![gun], target: bug })]);
    for _ in 0..40 {
        s.step(&[]);
    }
    assert!(s.get(bug).unwrap().hp < hp0, "surfaced unit must be hittable");
}

#[test]
fn burrow_and_shield_are_deterministic() {
    // Two sims stepping the identical command script stay checksum-equal.
    let script = |s: &mut State| {
        let bulwark = def_id(s, "bulwark");
        let burrower = def_id(s, "burrower");
        let trooper = def_id(s, "trooper");
        let d = s.spawn_unit(0, bulwark, FxVec2::from_int(40, 40));
        let b = s.spawn_unit(1, burrower, FxVec2::from_int(46, 40));
        let t = s.spawn_unit(0, trooper, FxVec2::from_int(43, 40));
        s.step(&[(0, Command::Siege { units: vec![d] }), (1, Command::Burrow { units: vec![b] })]);
        for _ in 0..50 {
            s.step(&[]);
        }
        s.step(&[(1, Command::Burrow { units: vec![b] }), (1, Command::AttackTarget { units: vec![b], target: t })]);
        for _ in 0..80 {
            s.step(&[]);
        }
        s.checksum()
    };
    let mut a = arena();
    let mut b = arena();
    assert_eq!(script(&mut a), script(&mut b));
}
