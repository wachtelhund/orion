//! Four peers over an in-memory broadcast hub (the relay's shape) stay in
//! lockstep on a 2v2 game: every seat steps the same ticks and produces
//! identical checksums.

use std::sync::mpsc::{channel, Receiver, Sender};

use orion_sim::fixed::FxVec2;
use orion_sim::map::{meridian, TilePos};
use orion_sim::net::{Lockstep, Msg, Net};
use orion_sim::{Command, GameData, State};

fn four_start_map() -> orion_sim::map::Map {
    let mut m = meridian();
    let (w, h) = (m.width, m.height);
    m.starts.push(TilePos::new(12, h - 13));
    m.starts.push(TilePos::new(w - 13, 12));
    m
}

fn fresh_state() -> State {
    let mut s = State::new_with_races(
        GameData::load_default(),
        four_start_map(),
        7,
        &[0, 1, 2, 0],
    );
    for (p, team) in [(0u8, 0u8), (1, 0), (2, 1), (3, 1)] {
        s.players[p as usize].team = team;
    }
    s
}

/// A relay-shaped hub: every line a peer sends is parsed and delivered to
/// every OTHER peer, exactly like the Lobby DO broadcast.
fn hub(n: usize) -> Vec<Net> {
    let mut outs: Vec<(Sender<String>, Receiver<String>)> = Vec::new();
    let mut ins: Vec<(Sender<Msg>, Receiver<Msg>)> = Vec::new();
    for _ in 0..n {
        outs.push(channel());
        ins.push(channel());
    }
    let out_rxs: Vec<Receiver<String>> = outs.iter_mut().map(|(_, r)| std::mem::replace(r, channel().1)).collect();
    let in_txs: Vec<Sender<Msg>> = ins.iter().map(|(t, _)| t.clone()).collect();
    std::thread::spawn(move || loop {
        let mut moved = false;
        let mut dead = 0;
        for (i, rx) in out_rxs.iter().enumerate() {
            loop {
                match rx.try_recv() {
                    Ok(line) => {
                        moved = true;
                        if let Some(m) = Net::parse_line(&line) {
                            for (j, tx) in in_txs.iter().enumerate() {
                                if j != i {
                                    let _ = tx.send(m.clone());
                                }
                            }
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        dead += 1;
                        break;
                    }
                }
            }
        }
        if dead == out_rxs.len() {
            return;
        }
        if !moved {
            std::thread::sleep(std::time::Duration::from_micros(200));
        }
    });
    outs.into_iter()
        .zip(ins)
        .map(|((out_tx, _), (_, in_rx))| Net::from_parts(out_tx, in_rx))
        .collect()
}

#[test]
fn four_seats_stay_in_lockstep() {
    let nets = hub(4);
    let mut steps: Vec<Lockstep> = nets
        .into_iter()
        .enumerate()
        .map(|(p, net)| Lockstep::new_room(net, p as u8, 4, 4))
        .collect();
    let mut states: Vec<State> = (0..4).map(|_| fresh_state()).collect();

    // Seat 2 orders a worker around so commands actually flow.
    let worker = states[2]
        .entities
        .iter()
        .enumerate()
        .find(|(_, e)| {
            e.alive && e.owner == 2 && states[2].data.units[e.def as usize].harvester
        })
        .map(|(i, _)| states[2].id_of(i as u32))
        .unwrap();

    // Every seat stops at exactly target_ticks, so all four states are
    // comparable at the same tick.
    let target_ticks = 120u32;
    for round in 0..20_000 {
        for p in 0..4 {
            if states[p].tick >= target_ticks {
                continue;
            }
            let mut pending = Vec::new();
            if p == 2 && states[2].tick == 24 && round % 3 == 0 {
                pending.push(Command::Move {
                    units: vec![worker],
                    target: FxVec2::from_int(30, 30),
                    queued: false,
                });
            }
            steps[p].try_step(&mut states[p], &mut pending);
        }
        if states.iter().all(|s| s.tick >= target_ticks) {
            break;
        }
    }
    let sums: std::collections::BTreeSet<u64> = states
        .iter()
        .map(|s| {
            assert_eq!(s.tick, target_ticks, "every seat lands on the target tick");
            s.checksum()
        })
        .collect();
    assert_eq!(sums.len(), 1, "identical checksums on all four seats");
    for st in &steps {
        assert!(!st.desync, "no desync flagged");
        assert!(!st.disconnected, "no disconnect flagged");
    }
}
