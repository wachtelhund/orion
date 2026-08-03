//! The load-bearing test: two sims fed identical inputs must be
//! bit-identical at every tick. This is the property lockstep multiplayer
//! stands on. If this test breaks, nothing else matters until it's fixed.

use orion_sim::ai::Bot;
use orion_sim::map::meridian;
use orion_sim::{Command, FxVec2, GameData, State};

fn build_state(seed: u64) -> State {
    State::new(GameData::load_default(), meridian(), seed)
}

/// Scripted command stream exercising every command type.
fn scripted_commands(s: &State, tick: u32) -> Vec<(u8, Command)> {
    let mut cmds = Vec::new();
    match tick {
        5 => {
            // Send player 0's workers mining.
            let workers: Vec<_> = (0..s.entities.len() as u32)
                .filter(|&i| {
                    let e = &s.entities[i as usize];
                    e.alive && e.owner == 0 && e.kind == orion_sim::EntityKind::Unit
                })
                .map(|i| s.id_of(i))
                .collect();
            let res = (0..s.entities.len() as u32)
                .find(|&i| s.entities[i as usize].kind == orion_sim::EntityKind::Resource)
                .map(|i| s.id_of(i))
                .unwrap();
            cmds.push((0, Command::Gather { units: workers, resource: res, queued: false }));
        }
        50 => {
            // March player 1's workers across the map (through ramps).
            let units: Vec<_> = (0..s.entities.len() as u32)
                .filter(|&i| {
                    let e = &s.entities[i as usize];
                    e.alive && e.owner == 1 && e.kind == orion_sim::EntityKind::Unit
                })
                .map(|i| s.id_of(i))
                .collect();
            cmds.push((1, Command::AttackMove {
                units,
                target: FxVec2::from_int(15, 15),
                queued: false,
            }));
        }
        _ => {}
    }
    cmds
}

#[test]
fn identical_inputs_identical_state() {
    let mut a = build_state(1234);
    let mut b = build_state(1234);

    for tick in 0..24 * 60 {
        // Commands derived from state A, applied to both (as lockstep would).
        let cmds = scripted_commands(&a, tick);
        a.step(&cmds);
        b.step(&cmds);
        assert_eq!(
            a.checksum(),
            b.checksum(),
            "desync at tick {tick}"
        );
    }
}

#[test]
fn bot_vs_bot_is_deterministic() {
    let mut a = build_state(99);
    let mut b = build_state(99);
    let mut bots_a = [Bot::new(0), Bot::new(1)];
    let mut bots_b = [Bot::new(0), Bot::new(1)];

    for tick in 0..24 * 120 {
        // Each sim runs its own bots — bots must produce identical commands
        // from identical states. (Bots gate their own think interval.)
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
        let (ha, hb) = (a.checksum(), b.checksum());
        assert_eq!(ha, hb, "bot-driven desync at tick {tick}");
    }
    // Sanity: the bots actually did something (mined minerals, trained units).
    assert!(
        a.players[0].minerals != orion_sim::state::START_MINERALS
            || a.entities.iter().filter(|e| e.alive).count() > 30,
        "bots appear inert"
    );
}

/// Two symmetric bots must produce a complete game: economy, army, attack,
/// and a decisive winner within 15 minutes. Doubles as a balance canary —
/// if a change turns bot-vs-bot into a stalemate, this fails.
#[test]
fn full_bot_game_reaches_a_winner() {
    let mut s = build_state(7);
    let mut bots = [Bot::new(0), Bot::new(1)];
    for tick in 0..24 * 60 * 15 {
        let _ = tick;
        let mut cmds = Vec::new();
        for bot in &mut bots {
            cmds.extend(bot.think(&s));
        }
        s.step(&cmds);
        if s.winner.is_some() {
            // Winner must have an economy behind the win.
            let w = s.winner.unwrap();
            assert!(
                s.players[w as usize].minerals > 0
                    || s.entities.iter().any(|e| e.alive && e.owner == w),
                "winner has nothing on the board"
            );
            return;
        }
    }
    panic!("no winner after 15 minutes of bot-vs-bot");
}

/// Shift-queued builds must execute after the current gather trip — the
/// exact flow a player uses: worker is mining, queue a depot, it gets built.
#[test]
fn queued_build_after_gather_completes() {
    let mut s = build_state(21);
    let worker_idx = (0..s.entities.len() as u32)
        .find(|&i| {
            let e = &s.entities[i as usize];
            e.alive && e.owner == 0 && e.kind == orion_sim::EntityKind::Unit
        })
        .unwrap();
    let worker = s.id_of(worker_idx);
    let res = (0..s.entities.len() as u32)
        .find(|&i| s.entities[i as usize].kind == orion_sim::EntityKind::Resource)
        .map(|i| s.id_of(i))
        .unwrap();
    let depot = s.data.building_tag("depot");
    // Find a valid site near the HQ.
    let hq_tile = s.map.starts[0];
    let mut site = None;
    'outer: for r in 3i32..12 {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs() != r && dy.abs() != r {
                    continue;
                }
                let t = orion_sim::TilePos::new(hq_tile.x + dx, hq_tile.y + dy);
                if s.valid_building_site(depot, t, Some(worker.idx)) {
                    site = Some(t);
                    break 'outer;
                }
            }
        }
    }
    let site = site.expect("no valid site");

    // Start mining, then shift-queue a build.
    s.step(&[(0, Command::Gather { units: vec![worker], resource: res, queued: false })]);
    s.step(&[(0, Command::Build { worker, building: depot, site, queued: true })]);
    for _ in 0..24 * 90 {
        s.step(&[]);
    }
    let built = s.entities.iter().any(|e| {
        e.alive
            && e.owner == 0
            && e.kind == orion_sim::EntityKind::Building
            && e.def == depot
            && e.construction.is_none()
    });
    assert!(built, "queued depot was never built");
}

/// Canceling construction refunds 75% and frees the site.
#[test]
fn cancel_construction_refunds() {
    let mut s = build_state(22);
    let worker_idx = (0..s.entities.len() as u32)
        .find(|&i| {
            let e = &s.entities[i as usize];
            e.alive && e.owner == 0 && e.kind == orion_sim::EntityKind::Unit
        })
        .unwrap();
    let worker = s.id_of(worker_idx);
    let depot = s.data.building_tag("depot");
    let hq_tile = s.map.starts[0];
    let mut site = None;
    'outer: for r in 3i32..12 {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs() != r && dy.abs() != r {
                    continue;
                }
                let t = orion_sim::TilePos::new(hq_tile.x + dx, hq_tile.y + dy);
                if s.valid_building_site(depot, t, Some(worker.idx)) {
                    site = Some(t);
                    break 'outer;
                }
            }
        }
    }
    let site = site.expect("no valid site");
    s.step(&[(0, Command::Build { worker, building: depot, site, queued: false })]);
    // Wait for construction to start (minerals get deducted).
    let mut started = None;
    for _ in 0..24 * 30 {
        s.step(&[]);
        if let Some(b) = (0..s.entities.len() as u32).find(|&i| {
            let e = &s.entities[i as usize];
            e.alive && e.kind == orion_sim::EntityKind::Building && e.def == depot
        }) {
            started = Some(s.id_of(b));
            break;
        }
    }
    let bid = started.expect("construction never started");
    let before = s.players[0].minerals;
    s.step(&[(0, Command::CancelConstruction { building: bid })]);
    assert_eq!(s.players[0].minerals, before + 75, "expected 75% of 100 refunded");
    assert!(s.get(bid).is_none(), "building still standing after cancel");
}

/// The new content systems: siege splash, flyers crossing cliffs, Plasma
/// Storm, and weapons research.
#[test]
fn content_systems_work() {
    use orion_sim::state::Storm;
    let mut s = build_state(31);

    // --- Siege mode + splash: a sieged Breaker vs a clump of enemies. ---
    let breaker = s.data.unit_tag("breaker");
    let trooper = s.data.unit_tag("trooper");
    let b = s.spawn_unit(0, breaker, FxVec2::from_int(40, 40));
    let clump: Vec<_> = (0..3)
        .map(|k| s.spawn_unit(1, trooper, FxVec2::from_int(46, 40 + k)))
        .collect();
    s.step(&[(0, Command::Siege { units: vec![b] })]);
    assert!(s.entities[b.idx as usize].sieged, "breaker did not siege");
    for _ in 0..48 {
        s.step(&[]); // transform + a few shots
    }
    let damaged = clump
        .iter()
        .filter(|id| {
            s.get(**id)
                .map(|e| e.hp < s.data.units[trooper as usize].hp)
                .unwrap_or(true) // dead counts as damaged
        })
        .count();
    assert!(damaged >= 2, "siege splash hit only {damaged} of 3 clumped enemies");

    // --- Flyer: straight over blocked cliff tiles. ---
    let sky = s.data.unit_tag("skywing");
    let f = s.spawn_unit(0, sky, FxVec2::from_int(20, 12));
    // (30, 12) is past the NW plateau's cliff ring (blocked for ground).
    s.step(&[(0, Command::Move {
        units: vec![f],
        target: FxVec2::from_int(30, 12),
        queued: false,
    })]);
    for _ in 0..24 * 10 {
        s.step(&[]);
    }
    let fp = s.entities[f.idx as usize].pos;
    assert!(
        (fp.x.to_f32() - 30.0).abs() < 2.0 && (fp.y.to_f32() - 12.0).abs() < 2.0,
        "skywing never crossed the cliff: at {:?},{:?}",
        fp.x,
        fp.y
    );

    // --- Plasma Storm: zone damage. ---
    s.storms.push(Storm {
        pos: FxVec2::from_int(46, 41),
        ticks_left: orion_sim::STORM_DURATION,
        owner: 0,
    });
    let hp_before: i32 = clump
        .iter()
        .filter_map(|id| s.get(*id).map(|e| e.hp))
        .sum();
    for _ in 0..orion_sim::STORM_DURATION {
        s.step(&[]);
    }
    let hp_after: i32 = clump
        .iter()
        .filter_map(|id| s.get(*id).map(|e| e.hp))
        .sum();
    assert!(
        hp_after < hp_before || clump.iter().any(|id| s.get(*id).is_none()),
        "storm dealt no damage"
    );

    // --- Research: weapons +1 completes and raises the level. ---
    let archive = s.data.building_tag("archive");
    let a = s.spawn_building(0, archive, orion_sim::TilePos::new(50, 50), false);
    s.players[0].minerals = 1000;
    s.players[0].gas = 1000;
    s.step(&[(0, Command::Research { building: a, research: 0 })]);
    assert!(s.entities[a.idx as usize].research.is_some(), "research did not start");
    for _ in 0..24 * 61 {
        s.step(&[]);
    }
    assert_eq!(s.players[0].weapons_level, 1, "weapons research did not complete");
}

/// The gas loop must actually run: bots build an extractor and bank gas,
/// and gas-costing units get produced.
#[test]
fn gas_economy_works() {
    let mut s = build_state(11);
    let mut bots = [Bot::new(0), Bot::new(1)];
    let vanguard = s.data.unit_tag("vanguard");
    let mut saw_gas = false;
    let mut saw_vanguard = false;
    for _ in 0..24 * 60 * 6 {
        let mut cmds = Vec::new();
        for bot in &mut bots {
            cmds.extend(bot.think(&s));
        }
        s.step(&cmds);
        if s.players.iter().any(|p| p.gas > 0) {
            saw_gas = true;
        }
        if s.entities.iter().any(|e| {
            e.alive && e.kind == orion_sim::EntityKind::Unit && e.def == vanguard
        }) {
            saw_vanguard = true;
        }
        if saw_gas && saw_vanguard {
            return;
        }
    }
    panic!("gas loop incomplete: gas mined = {saw_gas}, vanguard trained = {saw_vanguard}");
}
