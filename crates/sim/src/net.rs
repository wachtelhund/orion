//! Lockstep multiplayer transport: newline-framed RON messages over any
//! byte transport (TCP directly, or a WebSocket relay wearing the same
//! channel-based `Net` shape).
//!
//! The protocol is the sim's determinism made network-shaped: peers exchange
//! ONLY commands (a few bytes a tick) plus periodic checksums. Each side
//! schedules its local commands a negotiated `delay` ticks ahead and may
//! only step tick T once it holds BOTH players' command lists for T.
//! Desyncs are detected by comparing checksums, not prevented — if this
//! fires, there is a determinism bug to hunt.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};

/// Instant that works on wasm (std's panics there).
mod nettime {
    #[cfg(not(target_arch = "wasm32"))]
    pub use std::time::{Instant, SystemTime, UNIX_EPOCH};
    #[cfg(target_arch = "wasm32")]
    pub use web_time::{Instant, SystemTime, UNIX_EPOCH};
}

use serde::{Deserialize, Serialize};

use crate::state::Command;
use crate::State;

/// Commands issued now execute this many ticks later (~167ms at 24Hz).
/// Input-delay bounds: the host measures handshake RTT and picks a delay
/// that keeps the lockstep pipeline deeper than the network is slow, so a
/// relay hop doesn't stall every tick. 4 = LAN feel; 12 = 500ms pipelines.
pub const INPUT_DELAY_MIN: u32 = 4;
pub const INPUT_DELAY_MAX: u32 = 12;
pub const DEFAULT_PORT: u16 = 27515;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Msg {
    Hello { race: u8, version: String },
    Start {
        seed: u64,
        host_race: u8,
        join_race: u8,
        map: String,
        /// Custom maps travel inside the handshake (RON of `map::Map`) —
        /// the joiner needs no file and no fetch. None for builtin maps.
        map_ron: Option<String>,
    },
    /// Handshake rejection (version mismatch etc.) — human-readable.
    Reject { reason: String },
    Cmds { tick: u32, cmds: Vec<Command>, checksum: Option<(u32, u64)> },
    /// Handshake RTT probe: host times Ping->Pong to size the input delay.
    Ping { k: u32 },
    Pong { k: u32 },
    /// Host's chosen input delay (ticks), sized to the measured RTT.
    Go { delay: u32 },
    // ---- team rooms (3+ players; 1v1 keeps the messages above) ----
    /// Synthesized by the TRANSPORT from the relay's {relay_slot} control
    /// frame — tells a client its seat before it says Hello. Never sent
    /// by peers.
    Seat { slot: u8, capacity: u8, filled: u8 },
    /// Room Hello: a joiner announces itself with its relay seat.
    Hello2 { slot: u8, race: u8, name: String, version: String },
    /// Room start: everything every seat needs to build the same state.
    Start2 {
        seed: u64,
        races: Vec<u8>,
        teams: Vec<u8>,
        /// Seats the HOST drives with an AI (started before the room
        /// filled). Bot commands arrive tagged like any player's.
        bots: Vec<bool>,
        map: String,
        map_ron: Option<String>,
    },
    /// Room commands: broadcast, so the sender's seat rides along.
    Cmds2 { player: u8, tick: u32, cmds: Vec<Command>, checksum: Option<(u32, u64)> },
    /// Room RTT reply, tagged so the host can await one per seat.
    Pong2 { slot: u8, k: u32 },
}

/// Two builds may only play together when their sim versions match —
/// lockstep over diverging sims desyncs by definition.
pub const PROTOCOL_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A connected peer, transport-agnostic: outgoing message lines go into a
/// channel consumed by a transport thread; incoming parsed messages arrive
/// on `rx`. TCP and the WebSocket relay both wear this shape.
pub struct Net {
    out: Sender<String>,
    pub rx: Receiver<Msg>,
}

impl Net {
    /// Assemble a Net from raw channel ends — for transports that pump
    /// I/O themselves (the browser WebSocket driver).
    pub fn from_parts(out: Sender<String>, rx: Receiver<Msg>) -> Net {
        Net { out, rx }
    }

    /// Build a Net from raw line channels (used by the relay transport):
    /// whatever pumps `rx` and drains the returned receiver owns the wire.
    pub fn from_channels(out: Sender<String>, rx: Receiver<Msg>) -> Net {
        Net { out, rx }
    }

    /// Parse a wire line into a Msg (transport pumps use this).
    pub fn parse_line(line: &str) -> Option<Msg> {
        ron::from_str::<Msg>(line).ok()
    }

    fn from_stream(stream: TcpStream) -> std::io::Result<Net> {
        stream.set_nodelay(true)?;
        let reader = stream.try_clone()?;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut lines = BufReader::new(reader).lines();
            while let Some(Ok(line)) = lines.next() {
                match ron::from_str::<Msg>(&line) {
                    Ok(m) => {
                        if tx.send(m).is_err() {
                            break;
                        }
                    }
                    Err(_) => break, // corrupt stream: treat as disconnect
                }
            }
            // Dropping tx closes the channel = disconnect signal.
        });
        let (out_tx, out_rx): (Sender<String>, Receiver<String>) = channel();
        let mut writer = stream;
        std::thread::spawn(move || {
            while let Ok(line) = out_rx.recv() {
                if writer
                    .write_all(line.as_bytes())
                    .and_then(|_| writer.write_all(b"\n"))
                    .is_err()
                {
                    break;
                }
            }
        });
        Ok(Net { out: out_tx, rx })
    }

    /// False = peer gone.
    pub fn send(&mut self, m: &Msg) -> bool {
        let Ok(s) = ron::to_string(m) else { return false };
        self.out.send(s).is_ok()
    }
}

/// Best-effort LAN IP (UDP connect trick — no packets are sent).
pub fn local_ip() -> Option<String> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("8.8.8.8:80").ok()?;
    Some(s.local_addr().ok()?.ip().to_string())
}

/// A fully established match, ready to hand to `Lockstep`.
pub struct Started {
    pub net: Net,
    pub seed: u64,
    pub local_player: u8,
    pub races: [u8; 2],
    /// Map name (host's choice), resolved via `map::by_name`.
    pub map: String,
    /// The map itself when it is not a builtin (custom/editor maps).
    pub map_ron: Option<String>,
    /// Negotiated input delay in ticks.
    pub input_delay: u32,
}

impl Started {
    /// The actual map to play: embedded custom map first, else builtin.
    pub fn resolve_map(&self) -> Option<crate::map::Map> {
        match &self.map_ron {
            Some(src) => ron::de::from_str(src).ok(),
            None => crate::map::by_name(&self.map),
        }
    }
}

/// Host-side handshake over any transport. The host picks the map.
pub fn host_handshake(
    mut net: Net,
    my_race: u8,
    seed: u64,
    map: &str,
    map_ron: Option<String>,
) -> std::io::Result<Started> {
    let join_race = loop {
        match net.rx.recv() {
        Ok(Msg::Seat { .. }) => continue, // relay control frame, not a peer
        Ok(Msg::Hello { race, version }) => {
            if version != PROTOCOL_VERSION {
                let reason = format!(
                    "version mismatch: you {PROTOCOL_VERSION}, opponent {version} - both players must update"
                );
                net.send(&Msg::Reject { reason: reason.clone() });
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, reason));
            }
            break race;
        }
        _ => {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "bad hello"));
        }
        }
    };
    net.send(&Msg::Start {
        seed,
        host_race: my_race,
        join_race,
        map: map.to_string(),
        map_ron: map_ron.clone(),
    });
    // RTT probe: one round trip through whatever transport (and relay hop)
    // this session uses, so the input delay matches reality.
    let t0 = nettime::Instant::now();
    net.send(&Msg::Ping { k: 1 });
    let rtt = loop {
        match net.rx.recv() {
            Ok(Msg::Pong { .. }) => break t0.elapsed(),
            Ok(_) => continue,
            Err(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "peer left during handshake",
                ))
            }
        }
    };
    let rtt_ticks = (rtt.as_secs_f32() * crate::TICKS_PER_SEC as f32).ceil() as u32;
    let input_delay = (rtt_ticks + 2).clamp(INPUT_DELAY_MIN, INPUT_DELAY_MAX);
    net.send(&Msg::Go { delay: input_delay });
    Ok(Started {
        net,
        seed,
        local_player: 0,
        races: [my_race, join_race],
        map: map.to_string(),
        map_ron,
        input_delay,
    })
}

/// Join-side handshake over any transport.
pub fn join_handshake(mut net: Net, my_race: u8) -> std::io::Result<Started> {
    net.send(&Msg::Hello { race: my_race, version: PROTOCOL_VERSION.to_string() });
    loop {
    match net.rx.recv() {
        Ok(Msg::Seat { .. }) => continue, // relay control frame, not a peer
        Ok(Msg::Start { seed, host_race, join_race, map, map_ron }) => {
            // Answer the host's RTT probe, then wait for its delay pick.
            let mut input_delay = INPUT_DELAY_MIN;
            loop {
                match net.rx.recv() {
                    Ok(Msg::Ping { k }) => {
                        net.send(&Msg::Pong { k });
                    }
                    Ok(Msg::Go { delay }) => {
                        input_delay = delay.clamp(INPUT_DELAY_MIN, INPUT_DELAY_MAX);
                        break;
                    }
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }
            return Ok(Started {
                net,
                seed,
                local_player: 1,
                races: [host_race, join_race],
                map,
                map_ron,
                input_delay,
            });
        }
        Ok(Msg::Reject { reason }) => {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, reason));
        }
        _ => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "bad start")),
    }
    }
}

/// Fresh seed for a hosted match (pre-game choice, not sim-side).
pub fn fresh_seed() -> u64 {
    nettime::SystemTime::now()
        .duration_since(nettime::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0xC0FFEE)
}

/// Host: accept one opponent, run the handshake. Blocking — call in a thread.
pub fn host_blocking(listener: TcpListener, my_race: u8, seed: u64) -> std::io::Result<Started> {
    let (stream, _) = listener.accept()?;
    let net = Net::from_stream(stream)?;
    host_handshake(net, my_race, seed, "meridian", None)
}

/// Join a host. Blocking — call in a thread.
pub fn join_blocking(addr: impl ToSocketAddrs, my_race: u8) -> std::io::Result<Started> {
    let stream = TcpStream::connect(addr)?;
    let net = Net::from_stream(stream)?;
    join_handshake(net, my_race)
}

/// Spawned-thread helpers returning through a channel the UI can poll.
pub fn host_async(my_race: u8, port: u16) -> std::io::Result<Receiver<std::io::Result<Started>>> {
    let listener = TcpListener::bind(("0.0.0.0", port))?;
    let seed = fresh_seed();
    let (tx, rx): (Sender<std::io::Result<Started>>, _) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(host_blocking(listener, my_race, seed));
    });
    Ok(rx)
}

pub fn join_async(addr: String, my_race: u8) -> Receiver<std::io::Result<Started>> {
    let (tx, rx): (Sender<std::io::Result<Started>>, _) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(join_blocking(addr.as_str(), my_race));
    });
    rx
}

/// A fully established ROOM match (3+ seats).
pub struct RoomStarted {
    pub net: Net,
    pub local_player: u8,
    pub seed: u64,
    pub races: Vec<u8>,
    pub teams: Vec<u8>,
    /// Host-driven AI seats (empty seats filled at start).
    pub bots: Vec<bool>,
    pub map: String,
    pub map_ron: Option<String>,
    pub input_delay: u32,
}

impl RoomStarted {
    /// The actual map to play: embedded custom map first, else builtin.
    pub fn resolve_map(&self) -> Option<crate::map::Map> {
        match &self.map_ron {
            Some(src) => ron::de::from_str(src).ok(),
            None => crate::map::by_name(&self.map),
        }
    }
}

/// Room host: wait for `capacity - 1` seated Hellos, deal seats into a
/// game (teams by seat parity: 0+2 vs 1+3 for 2v2... no — adjacent pairs:
/// seats 0+1 vs 2+3), measure the worst seat RTT, broadcast Start2 + Go.
/// Blocking; run in a thread. `races[0]` is the host's race; joiner races
/// arrive in their Hellos.
pub fn room_host_handshake(
    net: Net,
    capacity: u8,
    my_race: u8,
    seed: u64,
    map: &str,
    map_ron: Option<String>,
) -> std::io::Result<RoomStarted> {
    // No start signal: waits for a full room (the classic flow).
    let (_tx, never) = std::sync::mpsc::channel();
    room_host_handshake_signaled(net, capacity, my_race, seed, map, map_ron, never)
}

/// Like `room_host_handshake`, but a signal on `start_rx` begins the game
/// immediately — empty seats are filled with host-driven bots (random
/// race, seeded deterministically from the match seed).
pub fn room_host_handshake_signaled(
    mut net: Net,
    capacity: u8,
    my_race: u8,
    seed: u64,
    map: &str,
    map_ron: Option<String>,
    start_rx: std::sync::mpsc::Receiver<()>,
) -> std::io::Result<RoomStarted> {
    let bad = |m: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, m.to_string());
    let mut races: Vec<Option<u8>> = vec![None; capacity as usize];
    races[0] = Some(my_race);
    // Collect Hellos until the room fills or the host says go.
    let mut start_now = false;
    while races.iter().any(|r| r.is_none()) && !start_now {
        if start_rx.try_recv().is_ok() {
            start_now = true;
            break;
        }
        match net.rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(Msg::Hello2 { slot, race, version, .. }) => {
                if version != PROTOCOL_VERSION {
                    let reason = format!(
                        "version mismatch: you {PROTOCOL_VERSION}, opponent {version} - all players must update"
                    );
                    net.send(&Msg::Reject { reason: reason.clone() });
                    return Err(bad(&reason));
                }
                if (slot as usize) < races.len() {
                    races[slot as usize] = Some(race);
                }
            }
            Ok(_) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => return Err(bad("a player left while the room filled")),
        }
    }
    // Empty seats become bots. Race picks derive from the match seed so
    // every peer could reproduce them (they ride Start2 regardless).
    let bots: Vec<bool> = races.iter().map(|r| r.is_none()).collect();
    let races: Vec<u8> = races
        .into_iter()
        .enumerate()
        .map(|(i, r)| r.unwrap_or_else(|| ((seed >> (i * 8)) % 3) as u8))
        .collect();
    // Adjacent seats team up: 0+1 vs 2+3 (a 3-seat room is 1v2).
    let teams: Vec<u8> = (0..capacity).map(|s| (s >= capacity / 2) as u8).collect();
    // Worst-seat RTT sizes the shared pipeline.
    let t0 = nettime::Instant::now();
    net.send(&Msg::Ping { k: 7 });
    let mut seen: Vec<bool> = bots.clone(); // bot seats need no probe
    seen[0] = true;
    let mut worst = std::time::Duration::ZERO;
    while seen.iter().any(|s| !s) {
        match net.rx.recv() {
            Ok(Msg::Pong2 { slot, k: 7 }) => {
                if (slot as usize) < seen.len() && !seen[slot as usize] {
                    seen[slot as usize] = true;
                    worst = t0.elapsed();
                }
            }
            Ok(_) => continue,
            Err(_) => return Err(bad("a player left during the RTT probe")),
        }
    }
    let rtt_ticks = (worst.as_secs_f32() * crate::TICKS_PER_SEC as f32).ceil() as u32;
    let input_delay = (rtt_ticks + 2).clamp(INPUT_DELAY_MIN, INPUT_DELAY_MAX);
    net.send(&Msg::Start2 {
        seed,
        races: races.clone(),
        teams: teams.clone(),
        bots: bots.clone(),
        map: map.to_string(),
        map_ron: map_ron.clone(),
    });
    net.send(&Msg::Go { delay: input_delay });
    Ok(RoomStarted {
        net,
        local_player: 0,
        seed,
        races,
        teams,
        bots,
        map: map.to_string(),
        map_ron,
        input_delay,
    })
}

/// Room joiner: learn the seat from the transport's Seat message, say
/// Hello2, answer the probe, adopt Start2 + Go.
pub fn room_join_handshake(
    mut net: Net,
    my_race: u8,
    name: &str,
) -> std::io::Result<RoomStarted> {
    let bad = |m: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, m.to_string());
    // The relay's control frame arrives first on connect.
    let slot = loop {
        match net.rx.recv() {
            Ok(Msg::Seat { slot, .. }) => break slot,
            Ok(_) => continue,
            Err(_) => return Err(bad("no seat assignment from the relay")),
        }
    };
    net.send(&Msg::Hello2 {
        slot,
        race: my_race,
        name: name.to_string(),
        version: PROTOCOL_VERSION.to_string(),
    });
    let mut start: Option<(u64, Vec<u8>, Vec<u8>, Vec<bool>, String, Option<String>)> = None;
    let delay = loop {
        match net.rx.recv() {
            Ok(Msg::Ping { k }) => {
                net.send(&Msg::Pong2 { slot, k });
            }
            Ok(Msg::Start2 { seed, races, teams, bots, map, map_ron }) => {
                start = Some((seed, races, teams, bots, map, map_ron));
            }
            Ok(Msg::Go { delay }) => break delay.clamp(INPUT_DELAY_MIN, INPUT_DELAY_MAX),
            Ok(Msg::Reject { reason }) => return Err(bad(&reason)),
            Ok(_) => continue,
            Err(_) => return Err(bad("the host left during the handshake")),
        }
    };
    let (seed, races, teams, bots, map, map_ron) =
        start.ok_or_else(|| bad("no start data before go"))?;
    Ok(RoomStarted {
        net,
        local_player: slot,
        seed,
        races,
        teams,
        bots,
        map,
        map_ron,
        input_delay: delay,
    })
}

/// Either kind of established match, from a code the joiner knew nothing
/// about — the relay's seat frame says whether this is a duel or a room.
pub enum Joined {
    Duel(Started),
    Room(RoomStarted),
}

/// Join by code, auto-detecting duel vs room from the relay's Seat frame.
pub fn join_auto(mut net: Net, my_race: u8, name: &str) -> std::io::Result<Joined> {
    let bad = |m: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, m.to_string());
    let (slot, capacity) = loop {
        match net.rx.recv() {
            Ok(Msg::Seat { slot, capacity, .. }) => break (slot, capacity),
            Ok(_) => continue,
            Err(_) => return Err(bad("no seat assignment from the relay")),
        }
    };
    if capacity <= 2 {
        return join_handshake(net, my_race).map(Joined::Duel);
    }
    // Room flow, seat already in hand.
    net.send(&Msg::Hello2 {
        slot,
        race: my_race,
        name: name.to_string(),
        version: PROTOCOL_VERSION.to_string(),
    });
    let mut start: Option<(u64, Vec<u8>, Vec<u8>, Vec<bool>, String, Option<String>)> = None;
    let delay = loop {
        match net.rx.recv() {
            Ok(Msg::Ping { k }) => {
                net.send(&Msg::Pong2 { slot, k });
            }
            Ok(Msg::Start2 { seed, races, teams, bots, map, map_ron }) => {
                start = Some((seed, races, teams, bots, map, map_ron));
            }
            Ok(Msg::Go { delay }) => break delay.clamp(INPUT_DELAY_MIN, INPUT_DELAY_MAX),
            Ok(Msg::Reject { reason }) => return Err(bad(&reason)),
            Ok(_) => continue,
            Err(_) => return Err(bad("the host left during the handshake")),
        }
    };
    let (seed, races, teams, bots, map, map_ron) =
        start.ok_or_else(|| bad("no start data before go"))?;
    Ok(Joined::Room(RoomStarted {
        net,
        local_player: slot,
        seed,
        races,
        teams,
        bots,
        map,
        map_ron,
        input_delay: delay,
    }))
}

/// The lockstep driver: schedules local input, waits for remote input,
/// steps when both sides of a tick are present, cross-checks checksums.
pub struct Lockstep {
    pub net: Net,
    pub local_player: u8,
    /// Negotiated pipeline depth in ticks.
    pub delay: u32,
    /// Live RTT estimate from in-game ping/pong (ms).
    pub rtt_ms: u32,
    ping_sent: Option<(u32, nettime::Instant)>,
    /// Tick stamps of recent step->stall transitions (last minute kept).
    stall_ticks: std::collections::VecDeque<u32>,
    was_waiting: bool,
    /// Total seats in the game (2 for 1v1). Command streams are indexed
    /// by player slot; our own slot aliases `local`.
    pub n_players: u8,
    local: BTreeMap<u32, Vec<Command>>,
    remotes: Vec<BTreeMap<u32, Vec<Command>>>,
    my_checksums: BTreeMap<u32, u64>,
    pending_checksum: Option<(u32, u64)>,
    sent_until: u32,
    pub desync: bool,
    pub disconnected: bool,
}

impl Lockstep {
    pub fn new(net: Net, local_player: u8, delay: u32) -> Lockstep {
        Lockstep {
            net,
            local_player,
            delay,
            rtt_ms: 0,
            ping_sent: None,
            stall_ticks: std::collections::VecDeque::new(),
            was_waiting: false,
            n_players: 2,
            local: BTreeMap::new(),
            remotes: vec![BTreeMap::new(); 2],
            my_checksums: BTreeMap::new(),
            pending_checksum: None,
            sent_until: 0,
            desync: false,
            disconnected: false,
        }
    }

    /// A room lockstep: same driver, N command streams.
    pub fn new_room(net: Net, local_player: u8, delay: u32, n_players: u8) -> Lockstep {
        let mut l = Lockstep::new(net, local_player, delay);
        l.n_players = n_players;
        l.remotes = vec![BTreeMap::new(); n_players as usize];
        l
    }

    /// Host-side: schedule a BOT seat's commands for a future tick and
    /// broadcast them tagged with that seat — peers receive them exactly
    /// like a human player's stream.
    pub fn push_bot(&mut self, player: u8, tick: u32, cmds: Vec<Command>) -> bool {
        if (player as usize) >= self.remotes.len() {
            return false;
        }
        self.remotes[player as usize].insert(tick, cmds.clone());
        self.net.send(&Msg::Cmds2 { player, tick, cmds, checksum: None })
    }

    /// The furthest tick a bot stream has been scheduled to (for the
    /// host's fill loop).
    pub fn bot_sent_until(&self, player: u8) -> u32 {
        self.remotes
            .get(player as usize)
            .and_then(|m| m.keys().next_back().map(|k| k + 1))
            .unwrap_or(0)
    }

    /// Drain the receive channel.
    fn pump(&mut self) {
        loop {
            match self.net.rx.try_recv() {
                Ok(Msg::Ping { k }) => {
                    let _ = self.net.send(&Msg::Pong { k });
                }
                Ok(Msg::Pong { k }) => {
                    if let Some((sk, at)) = self.ping_sent {
                        if sk == k {
                            self.rtt_ms = at.elapsed().as_millis() as u32;
                            self.ping_sent = None;
                        }
                    }
                }
                Ok(Msg::Cmds2 { player, tick, cmds, checksum }) => {
                    if player != self.local_player
                        && (player as usize) < self.remotes.len()
                    {
                        self.remotes[player as usize].insert(tick, cmds);
                        if let Some((t, sum)) = checksum {
                            if let Some(mine) = self.my_checksums.get(&t) {
                                if *mine != sum {
                                    self.desync = true;
                                }
                            }
                        }
                    }
                }
                Ok(_) => {}
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.disconnected = true;
                    break;
                }
            }
        }
    }

    /// Attempt to advance one tick. `pending` holds the local player's
    /// commands gathered since the last call; they are scheduled
    /// `delay` ticks ahead. Returns false when waiting on the peer.
    pub fn try_step(&mut self, state: &mut State, pending: &mut Vec<Command>) -> bool {
        self.pump();
        if self.disconnected || self.desync {
            return false;
        }
        let t = state.tick;
        // Live RTT probe every ~2s.
        if t % 48 == 0 && self.ping_sent.is_none() {
            let _ = self.net.send(&Msg::Ping { k: t });
            self.ping_sent = Some((t, nettime::Instant::now()));
        }
        // Send local schedules up to t+delay; new input rides the last one.
        while self.sent_until <= t + self.delay {
            let tick = self.sent_until;
            let cmds = if tick == t + self.delay {
                std::mem::take(pending)
            } else {
                Vec::new()
            };
            self.local.insert(tick, cmds.clone());
            let checksum = self.pending_checksum.take();
            let player = self.local_player;
            if !self.net.send(&Msg::Cmds2 { player, tick, cmds, checksum }) {
                self.disconnected = true;
                return false;
            }
            self.sent_until += 1;
        }
        // Stalled with fresh input: flush it on an extra frame ahead of the
        // pipeline instead of sitting on it — orders keep flowing even while
        // the sim waits on the peer. Bounded so a long stall can't run away.
        if !pending.is_empty() && self.sent_until <= t + self.delay + 8 {
            let tick = self.sent_until;
            let cmds = std::mem::take(pending);
            self.local.insert(tick, cmds.clone());
            let player = self.local_player;
            if !self.net.send(&Msg::Cmds2 { player, tick, cmds, checksum: None }) {
                self.disconnected = true;
                return false;
            }
            self.sent_until += 1;
        }
        // Step only when EVERY seat's commands for tick t are present.
        let ready = (0..self.n_players).all(|p| {
            if p == self.local_player {
                self.local.contains_key(&t)
            } else {
                self.remotes[p as usize].contains_key(&t)
            }
        });
        if !ready {
            // Count step->stall transitions, windowed to the last minute.
            if !self.was_waiting {
                self.was_waiting = true;
                self.stall_ticks.push_back(t);
                while self.stall_ticks.front().is_some_and(|&s| s + 1440 < t) {
                    self.stall_ticks.pop_front();
                }
            }
            return false;
        }
        self.was_waiting = false;
        // Slot order keeps the combined stream deterministic on all peers.
        let mut cmds: Vec<(u8, Command)> = Vec::new();
        for p in 0..self.n_players {
            let list = if p == self.local_player {
                self.local.get(&t)
            } else {
                self.remotes[p as usize].get(&t)
            };
            if let Some(list) = list {
                cmds.extend(list.iter().cloned().map(|c| (p, c)));
            }
        }
        state.step(&cmds);

        // Periodic checksum exchange.
        if t % 24 == 0 {
            let sum = state.checksum();
            self.my_checksums.insert(t, sum);
            self.pending_checksum = Some((t, sum));
            // Prune old bookkeeping.
            let keep = t.saturating_sub(24 * 10);
            self.my_checksums.retain(|k, _| *k >= keep);
        }
        self.local.remove(&t);
        for r in &mut self.remotes {
            r.remove(&t);
        }
        true
    }

    /// Stalls in the last in-game minute.
    pub fn stalls_per_min(&self) -> usize {
        self.stall_ticks.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::meridian;
    use crate::{FxVec2, GameData};

    /// Full loopback match: host + joiner over real TCP on localhost, cross
    /// races, scripted commands from both sides, checksums compared every
    /// stepped tick out-of-band (the in-band exchange is also exercised).
    #[test]
    fn lockstep_over_tcp_stays_in_sync() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let host_thread =
            std::thread::spawn(move || host_blocking(listener, 0, 424242).unwrap());
        let join = join_blocking(("127.0.0.1", port), 1).unwrap();
        let host = host_thread.join().unwrap();
        assert_eq!(host.races, [0, 1]);
        assert_eq!(join.seed, 424242);

        let mk = |s: &Started| {
            State::new_with_races(GameData::load_default(), meridian(), s.seed, &s.races)
        };
        let mut sh = mk(&host);
        let mut sj = mk(&join);
        let mut lh = Lockstep::new(host.net, 0, host.input_delay);
        let mut lj = Lockstep::new(join.net, 1, join.input_delay);

        let mut pending_h: Vec<Command> = Vec::new();
        let mut pending_j: Vec<Command> = Vec::new();
        let mut hist_h: BTreeMap<u32, u64> = BTreeMap::new();
        let mut hist_j: BTreeMap<u32, u64> = BTreeMap::new();

        let target = 24 * 20; // 20 seconds of game
        let mut guard = 0;
        while (sh.tick < target || sj.tick < target) && guard < 2_000_000 {
            guard += 1;
            // Scripted inputs: each side orders its workers around sometimes.
            if sh.tick == 24 && pending_h.is_empty() {
                let units: Vec<_> = (0..sh.entities.len() as u32)
                    .filter(|&i| {
                        let e = &sh.entities[i as usize];
                        e.alive && e.owner == 0 && e.kind == crate::EntityKind::Unit
                    })
                    .map(|i| sh.id_of(i))
                    .collect();
                pending_h.push(Command::Move {
                    units,
                    target: FxVec2::from_int(30, 30),
                    queued: false,
                });
            }
            if sj.tick == 30 && pending_j.is_empty() {
                let units: Vec<_> = (0..sj.entities.len() as u32)
                    .filter(|&i| {
                        let e = &sj.entities[i as usize];
                        e.alive && e.owner == 1 && e.kind == crate::EntityKind::Unit
                    })
                    .map(|i| sj.id_of(i))
                    .collect();
                pending_j.push(Command::AttackMove {
                    units,
                    target: FxVec2::from_int(50, 50),
                    queued: false,
                });
            }
            if sh.tick < target && lh.try_step(&mut sh, &mut pending_h) {
                hist_h.insert(sh.tick, sh.checksum());
            }
            if sj.tick < target && lj.try_step(&mut sj, &mut pending_j) {
                hist_j.insert(sj.tick, sj.checksum());
            }
            assert!(!lh.desync && !lj.desync, "in-band desync flagged");
        }
        assert!(sh.tick >= target && sj.tick >= target, "lockstep stalled");
        // Every tick both sides stepped must hash identically.
        for (t, h) in &hist_h {
            if let Some(j) = hist_j.get(t) {
                assert_eq!(h, j, "checksum divergence at tick {t}");
            }
        }
        // The scripted moves actually happened on both sims identically.
        assert_eq!(sh.checksum(), sj.checksum());
    }
}
