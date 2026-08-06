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
    let join_race = match net.rx.recv() {
        Ok(Msg::Hello { race, version }) => {
            if version != PROTOCOL_VERSION {
                let reason = format!(
                    "version mismatch: you {PROTOCOL_VERSION}, opponent {version} - both players must update"
                );
                net.send(&Msg::Reject { reason: reason.clone() });
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, reason));
            }
            race
        }
        _ => {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "bad hello"));
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
    match net.rx.recv() {
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
            Ok(Started {
                net,
                seed,
                local_player: 1,
                races: [host_race, join_race],
                map,
                map_ron,
                input_delay,
            })
        }
        Ok(Msg::Reject { reason }) => {
            Err(std::io::Error::new(std::io::ErrorKind::InvalidData, reason))
        }
        _ => Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "bad start")),
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
    local: BTreeMap<u32, Vec<Command>>,
    remote: BTreeMap<u32, Vec<Command>>,
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
            local: BTreeMap::new(),
            remote: BTreeMap::new(),
            my_checksums: BTreeMap::new(),
            pending_checksum: None,
            sent_until: 0,
            desync: false,
            disconnected: false,
        }
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
                Ok(Msg::Cmds { tick, cmds, checksum }) => {
                    self.remote.insert(tick, cmds);
                    if let Some((t, sum)) = checksum {
                        if let Some(mine) = self.my_checksums.get(&t) {
                            if *mine != sum {
                                self.desync = true;
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
            if !self.net.send(&Msg::Cmds { tick, cmds, checksum }) {
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
            if !self.net.send(&Msg::Cmds { tick, cmds, checksum: None }) {
                self.disconnected = true;
                return false;
            }
            self.sent_until += 1;
        }
        // Step only with both halves of tick t present.
        let (Some(local), Some(remote)) = (self.local.get(&t), self.remote.get(&t)) else {
            // Count step->stall transitions, windowed to the last minute.
            if !self.was_waiting {
                self.was_waiting = true;
                self.stall_ticks.push_back(t);
                while self.stall_ticks.front().is_some_and(|&s| s + 1440 < t) {
                    self.stall_ticks.pop_front();
                }
            }
            return false;
        };
        self.was_waiting = false;
        let (host, join) = if self.local_player == 0 {
            (local, remote)
        } else {
            (remote, local)
        };
        let mut cmds: Vec<(u8, Command)> = Vec::new();
        cmds.extend(host.iter().cloned().map(|c| (0u8, c)));
        cmds.extend(join.iter().cloned().map(|c| (1u8, c)));
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
        self.remote.remove(&t);
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
