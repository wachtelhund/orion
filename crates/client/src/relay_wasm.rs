//! Browser relay transport: real code-lobby multiplayer over a
//! web-sys WebSocket. The native module blocks threads on handshakes;
//! here everything is callback-driven — a 16ms interval drains outgoing
//! messages and advances a small handshake state machine, then hands the
//! finished `Started` through the same channel shapes the UI polls.
//! Ranked matchmaking and the lobby browser stay desktop-only for now.

use std::cell::RefCell;
use std::io;
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, Sender};

use orion_sim::net::{Joined, Msg, Net, RoomStarted, Started, INPUT_DELAY_MIN};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{MessageEvent, WebSocket};

pub fn fresh_code() -> String {
    // Derived from the ms clock — good enough for lobby codes.
    let ms = web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let mut t = ms ^ 0x5DEECE66D;
    let mut code = String::new();
    for _ in 0..5 {
        t = t.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        code.push((b'A' + ((t >> 33) % 26) as u8) as char);
    }
    code
}

fn ws_url(base: &str, code: &str, role: &str) -> String {
    format!("{base}/ws/{code}?role={role}")
}

/// Handshake progress for the interval driver.
enum Phase {
    /// Host: waiting for the joiner's Hello.
    HostWaitHello,
    /// Host: sent Ping, waiting for Pong (RTT start ms).
    HostWaitPong { join_race: u8, t0: f64 },
    /// Join: sent Hello, waiting for Start.
    JoinWaitStart,
    /// Join: waiting for the host's delay pick.
    JoinWaitGo {
        seed: u64,
        host_race: u8,
        join_race: u8,
        map: String,
        map_ron: Option<String>,
    },
    /// Auto-join: waiting for the relay's seat frame to learn whether
    /// this code is a duel (capacity 2) or a room.
    AutoWaitSeat,
    /// Room joiner: Hello2 sent, answering pings, waiting for Start2+Go.
    RoomWait {
        slot: u8,
        start: Option<(u64, Vec<u8>, Vec<u8>, Vec<bool>, String, Option<String>)>,
    },
    Done,
}

/// Where the finished handshake goes: the duel-only channel the classic
/// paths poll, or the auto channel that can carry either match kind.
enum Sink {
    Duel(Sender<io::Result<Started>>),
    Auto(Sender<io::Result<Joined>>),
}

impl Sink {
    fn err(&self, e: io::Error) {
        match self {
            Sink::Duel(tx) => drop(tx.send(Err(e))),
            Sink::Auto(tx) => drop(tx.send(Err(e))),
        }
    }
    fn duel(&self, st: Started) {
        match self {
            Sink::Duel(tx) => drop(tx.send(Ok(st))),
            Sink::Auto(tx) => drop(tx.send(Ok(Joined::Duel(st)))),
        }
    }
    fn room(&self, rs: RoomStarted) {
        match self {
            // Unreachable: room phases only exist on the auto path.
            Sink::Duel(_) => {}
            Sink::Auto(tx) => drop(tx.send(Ok(Joined::Room(rs)))),
        }
    }
}

struct Session {
    ws: WebSocket,
    out_rx: Option<Receiver<String>>,
    in_tx: Option<Sender<Msg>>,
    /// Messages that arrive during the handshake but belong to the game
    /// (early Cmds from a fast peer) — replayed into in_tx on completion.
    phase: Phase,
    my_race: u8,
    name: String,
    seed: u64,
    map: String,
    result_tx: Sink,
    net_parts: Option<(Sender<String>, Receiver<Msg>)>,
    raw_in: Rc<RefCell<Vec<String>>>,
    opened: Rc<RefCell<bool>>,
    closed: Rc<RefCell<bool>>,
    started_at: f64,
    /// Debug counters: pump calls, lines in, lines out.
    n_pump: u32,
    n_rx: u32,
    n_tx: u32,
}

fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}

fn parse_msg(line: &str) -> Option<Msg> {
    let line = line.trim();
    if line.starts_with('{') {
        // Relay control frames are JSON: seat assignment becomes a
        // synthetic Msg for the room handshake, others are dropped.
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        let slot = v.get("relay_slot")?.as_u64()? as u8;
        let capacity = v.get("capacity").and_then(|x| x.as_u64()).unwrap_or(2) as u8;
        let filled = v.get("filled").and_then(|x| x.as_u64()).unwrap_or(1) as u8;
        return Some(Msg::Seat { slot, capacity, filled });
    }
    ron::de::from_str(line).ok()
}

fn send_msg(ws: &WebSocket, msg: &Msg) {
    if let Ok(txt) = ron::ser::to_string(msg) {
        let _ = ws.send_with_str(&txt);
    }
}

/// Wire a WebSocket into (Net, driver Session). The caller starts the
/// interval that pumps the session.
fn open_session(
    url: &str,
    my_race: u8,
    seed: u64,
    map: String,
    name: String,
    phase: Phase,
    result_tx: Sink,
) -> Result<Session, io::Error> {
    let ws = WebSocket::new(url)
        .map_err(|_| io::Error::new(io::ErrorKind::ConnectionRefused, "websocket failed"))?;
    let raw_in: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let opened = Rc::new(RefCell::new(false));
    let closed = Rc::new(RefCell::new(false));

    let raw2 = raw_in.clone();
    let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
        if let Some(txt) = e.data().as_string() {
            for line in txt.lines() {
                if !line.trim().is_empty() {
                    raw2.borrow_mut().push(line.to_string());
                }
            }
        }
    });
    ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();

    let op2 = opened.clone();
    let onopen = Closure::<dyn FnMut()>::new(move || {
        *op2.borrow_mut() = true;
    });
    ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    onopen.forget();

    let cl2 = closed.clone();
    let onclose = Closure::<dyn FnMut()>::new(move || {
        *cl2.borrow_mut() = true;
    });
    ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
    ws.set_onerror(None);

    onclose.forget();

    let (out_tx, out_rx) = channel::<String>();
    let (in_tx, in_rx) = channel::<Msg>();
    Ok(Session {
        ws,
        out_rx: Some(out_rx),
        in_tx: Some(in_tx),
        phase,
        my_race,
        name,
        seed,
        map,
        result_tx,
        net_parts: Some((out_tx, in_rx)),
        raw_in,
        opened,
        closed,
        started_at: now_ms(),
        n_pump: 0,
        n_rx: 0,
        n_tx: 0,
    })
}

/// One pump tick: flush outgoing, take incoming, advance the handshake.
/// Returns false when the session is finished (stop the interval).
fn pump(sess: &mut Session) -> bool {
    sess.n_pump = sess.n_pump.wrapping_add(1);
    if sess.n_pump % 120 == 0 {
        let ph = match sess.phase {
            Phase::HostWaitHello => "host-wait-hello",
            Phase::HostWaitPong { .. } => "host-wait-pong",
            Phase::JoinWaitStart => "join-wait-start",
            Phase::JoinWaitGo { .. } => "join-wait-go",
            Phase::AutoWaitSeat => "auto-wait-seat",
            Phase::RoomWait { .. } => "room-wait",
            Phase::Done => "done",
        };
        crate::weblog(&format!(
            "pump hb: phase={ph} rx={} tx={} open={} closed={}",
            sess.n_rx, sess.n_tx, *sess.opened.borrow(), *sess.closed.borrow()
        ));
    }
    if *sess.closed.borrow() {
        sess.result_tx.err(io::Error::new(
            io::ErrorKind::ConnectionReset,
            "connection closed",
        ));
        return false;
    }
    if !*sess.opened.borrow() {
        // 15s connect timeout.
        if now_ms() - sess.started_at > 15_000.0 {
            sess.result_tx.err(io::Error::new(
                io::ErrorKind::TimedOut,
                "relay connect timeout",
            ));
            return false;
        }
        return true;
    }
    // Join side opens with Hello exactly once.
    if matches!(sess.phase, Phase::JoinWaitStart) && sess.started_at > 0.0 {
        if sess.started_at != -1.0 {
            send_msg(
                &sess.ws,
                &Msg::Hello {
                    race: sess.my_race,
                    version: orion_sim::net::PROTOCOL_VERSION.to_string(),
                },
            );
            sess.started_at = -1.0;
        }
    }
    let lines: Vec<String> = sess.raw_in.borrow_mut().drain(..).collect();
    sess.n_rx = sess.n_rx.wrapping_add(lines.len() as u32);
    for line in lines {
        let Some(msg) = parse_msg(&line) else { continue };
        match &mut sess.phase {
            Phase::HostWaitHello => {
                if let Msg::Hello { race, version } = &msg {
                    if *version != orion_sim::net::PROTOCOL_VERSION {
                        send_msg(
                            &sess.ws,
                            &Msg::Reject {
                                reason: format!(
                                    "version mismatch: you {}, opponent {version}",
                                    orion_sim::net::PROTOCOL_VERSION
                                ),
                            },
                        );
                        sess.result_tx.err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "version mismatch - both players must update",
                        ));
                        return false;
                    }
                    let join_race = *race;
                    send_msg(
                        &sess.ws,
                        &Msg::Start {
                            seed: sess.seed,
                            host_race: sess.my_race,
                            join_race,
                            map: sess.map.clone(),
                            // Browser hosts have no filesystem, so no custom
                            // maps to embed — builtin names only.
                            map_ron: None,
                        },
                    );
                    send_msg(&sess.ws, &Msg::Ping { k: 1 });
                    sess.phase = Phase::HostWaitPong { join_race, t0: now_ms() };
                }
            }
            Phase::HostWaitPong { join_race, t0 } => {
                if matches!(msg, Msg::Pong { .. }) {
                    let rtt_s = ((now_ms() - *t0) / 1000.0) as f32;
                    let rtt_ticks =
                        (rtt_s * orion_sim::TICKS_PER_SEC as f32).ceil() as u32;
                    let delay = (rtt_ticks + 2)
                        .clamp(INPUT_DELAY_MIN, orion_sim::net::INPUT_DELAY_MAX);
                    send_msg(&sess.ws, &Msg::Go { delay });
                    let (out_tx, in_rx) = sess.net_parts.take().unwrap();
                    let started = Started {
                        net: Net::from_parts(out_tx, in_rx),
                        seed: sess.seed,
                        local_player: 0,
                        races: [sess.my_race, *join_race],
                        map: sess.map.clone(),
                        map_ron: None,
                        input_delay: delay,
                    };
                    sess.result_tx.duel(started);
                    sess.phase = Phase::Done;
                }
            }
            Phase::JoinWaitStart => match &msg {
                Msg::Start { seed, host_race, join_race, map, map_ron } => {
                    sess.phase = Phase::JoinWaitGo {
                        seed: *seed,
                        host_race: *host_race,
                        join_race: *join_race,
                        map: map.clone(),
                        map_ron: map_ron.clone(),
                    };
                }
                Msg::Reject { reason } => {
                    sess.result_tx
                        .err(io::Error::new(io::ErrorKind::InvalidData, reason.clone()));
                    return false;
                }
                _ => {}
            },
            Phase::JoinWaitGo { seed, host_race, join_race, map, map_ron } => match &msg {
                Msg::Ping { k } => {
                    send_msg(&sess.ws, &Msg::Pong { k: *k });
                }
                Msg::Go { delay } => {
                    let delay =
                        (*delay).clamp(INPUT_DELAY_MIN, orion_sim::net::INPUT_DELAY_MAX);
                    let (out_tx, in_rx) = sess.net_parts.take().unwrap();
                    let started = Started {
                        net: Net::from_parts(out_tx, in_rx),
                        seed: *seed,
                        local_player: 1,
                        races: [*host_race, *join_race],
                        map: map.clone(),
                        map_ron: map_ron.clone(),
                        input_delay: delay,
                    };
                    sess.result_tx.duel(started);
                    sess.phase = Phase::Done;
                }
                _ => {}
            },
            Phase::AutoWaitSeat => {
                if let Msg::Seat { slot, capacity, .. } = &msg {
                    if *capacity <= 2 {
                        // A classic duel: fall into the ordinary join path.
                        send_msg(
                            &sess.ws,
                            &Msg::Hello {
                                race: sess.my_race,
                                version: orion_sim::net::PROTOCOL_VERSION.to_string(),
                            },
                        );
                        sess.started_at = -1.0; // Hello sent — disarm the auto-send.
                        sess.phase = Phase::JoinWaitStart;
                    } else {
                        send_msg(
                            &sess.ws,
                            &Msg::Hello2 {
                                slot: *slot,
                                race: sess.my_race,
                                name: sess.name.clone(),
                                version: orion_sim::net::PROTOCOL_VERSION.to_string(),
                            },
                        );
                        sess.phase = Phase::RoomWait { slot: *slot, start: None };
                    }
                }
            }
            Phase::RoomWait { slot, start } => match &msg {
                Msg::Ping { k } => {
                    send_msg(&sess.ws, &Msg::Pong2 { slot: *slot, k: *k });
                }
                Msg::Start2 { seed, races, teams, bots, map, map_ron } => {
                    *start = Some((
                        *seed,
                        races.clone(),
                        teams.clone(),
                        bots.clone(),
                        map.clone(),
                        map_ron.clone(),
                    ));
                }
                Msg::Go { delay } => {
                    let Some((seed, races, teams, bots, map, map_ron)) = start.take() else {
                        sess.result_tx.err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "no start data before go",
                        ));
                        return false;
                    };
                    let delay =
                        (*delay).clamp(INPUT_DELAY_MIN, orion_sim::net::INPUT_DELAY_MAX);
                    let (out_tx, in_rx) = sess.net_parts.take().unwrap();
                    sess.result_tx.room(RoomStarted {
                        net: Net::from_parts(out_tx, in_rx),
                        local_player: *slot,
                        seed,
                        races,
                        teams,
                        bots,
                        map,
                        map_ron,
                        input_delay: delay,
                    });
                    sess.phase = Phase::Done;
                }
                Msg::Reject { reason } => {
                    sess.result_tx
                        .err(io::Error::new(io::ErrorKind::InvalidData, reason.clone()));
                    return false;
                }
                _ => {}
            },
            Phase::Done => {
                // In-game: forward straight to the Lockstep receiver.
                if let Some(tx) = &sess.in_tx {
                    let _ = tx.send(msg);
                }
            }
        }
    }
    // Flush game-side outgoing (Lockstep pushes into the Net sender).
    if let Some(rx) = &sess.out_rx {
        while let Ok(line) = rx.try_recv() {
            sess.n_tx = sess.n_tx.wrapping_add(1);
            let _ = sess.ws.send_with_str(&line);
        }
    }
    true
}

/// Spawn the 16ms pump interval owning the session.
fn drive(mut sess: Session) {
    let win = web_sys::window().unwrap();
    let handle: Rc<RefCell<Option<i32>>> = Rc::new(RefCell::new(None));
    let h2 = handle.clone();
    let cb = Closure::<dyn FnMut()>::new(move || {
        if !pump(&mut sess) {
            // The closure itself is leaked (forget below), so the game's
            // channel ends must be dropped EXPLICITLY — the Lockstep only
            // notices a dead peer when its receiver disconnects. Without
            // this, an opponent leaving froze the match forever.
            sess.in_tx = None;
            sess.out_rx = None;
            let _ = sess.ws.close();
            if let Some(id) = h2.borrow_mut().take() {
                if let Some(w) = web_sys::window() {
                    w.clear_interval_with_handle(id);
                }
            }
        }
    });
    let id = win
        .set_interval_with_callback_and_timeout_and_arguments_0(
            cb.as_ref().unchecked_ref(),
            16,
        )
        .unwrap_or(0);
    *handle.borrow_mut() = Some(id);
    cb.forget();
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct LobbyInfo {
    pub code: String,
    pub name: String,
    pub race: u8,
    #[allow(dead_code)]
    pub age_s: u32,
    /// Room capacity (2 = classic duel). Older relays omit these.
    #[serde(default = "two")]
    pub slots: u8,
    #[serde(default = "one")]
    pub filled: u8,
}

fn two() -> u8 {
    2
}
fn one() -> u8 {
    1
}

fn http_base(base: &str) -> String {
    base.trim_end_matches('/')
        .replacen("wss://", "https://", 1)
        .replacen("ws://", "http://", 1)
}

/// Download a shared replay by code — browser fetch, same channel shape
/// as the native thread version.
pub fn fetch_replay_async(base: String, code: String) -> Receiver<io::Result<String>> {
    let (tx, rx) = channel();
    let url = format!("{}/replay/{}", http_base(&base), code.to_uppercase());
    wasm_bindgen_futures::spawn_local(async move {
        let result: io::Result<String> = async {
            let win = web_sys::window()
                .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "no window"))?;
            let resp = wasm_bindgen_futures::JsFuture::from(win.fetch_with_str(&url))
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::Other, "fetch failed"))?;
            let resp: web_sys::Response = resp
                .dyn_into()
                .map_err(|_| io::Error::new(io::ErrorKind::Other, "bad response"))?;
            if resp.status() == 404 {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "no replay with that code",
                ));
            }
            if !resp.ok() {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("relay error {}", resp.status()),
                ));
            }
            let text = wasm_bindgen_futures::JsFuture::from(
                resp.text()
                    .map_err(|_| io::Error::new(io::ErrorKind::Other, "no body"))?,
            )
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "read failed"))?;
            text.as_string()
                .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "not text"))
        }
        .await;
        let _ = tx.send(result);
    });
    rx
}

/// Browser lobby list: window.fetch -> JSON, delivered through the same
/// channel shape the native thread version uses.
pub fn fetch_lobbies_async(base: String) -> Receiver<Option<Vec<LobbyInfo>>> {
    let (tx, rx) = channel();
    let url = format!("{}/lobbies", http_base(&base));
    wasm_bindgen_futures::spawn_local(async move {
        let result: Option<Vec<LobbyInfo>> = async {
            let win = web_sys::window()?;
            let resp = wasm_bindgen_futures::JsFuture::from(win.fetch_with_str(&url))
                .await
                .ok()?;
            let resp: web_sys::Response = resp.dyn_into().ok()?;
            let text = wasm_bindgen_futures::JsFuture::from(resp.text().ok()?)
                .await
                .ok()?;
            serde_json::from_str(&text.as_string()?).ok()
        }
        .await;
        let _ = tx.send(result);
    });
    rx
}

pub fn host_relay_async_with_code(
    base: String,
    code: String,
    race: u8,
) -> (String, Receiver<io::Result<Started>>) {
    host_relay_async_full(base, code, race, "COMMANDER", true, "meridian", None)
}

pub fn host_relay_async_full(
    base: String,
    code: String,
    my_race: u8,
    name: &str,
    private: bool,
    map: &str,
    // Browser hosts have no local custom maps to embed; accepted for
    // signature parity with the native module.
    _map_ron: Option<String>,
) -> (String, Receiver<io::Result<Started>>) {
    let clean: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == ' ')
        .take(16)
        .collect();
    let url = format!(
        "{}&name={}&race={}&private={}",
        ws_url(&base, &code, "host"),
        clean.replace(' ', "%20"),
        my_race,
        if private { 1 } else { 0 }
    );
    let (tx, rx) = channel();
    let seed = orion_sim::net::fresh_seed();
    match open_session(
        &url,
        my_race,
        seed,
        map.to_string(),
        String::new(),
        Phase::HostWaitHello,
        Sink::Duel(tx.clone()),
    ) {
        Ok(sess) => drive(sess),
        Err(e) => {
            let _ = tx.send(Err(e));
        }
    }
    (code, rx)
}

/// Join any code — the relay's seat frame decides duel vs room.
pub fn join_auto_async(
    base: String,
    code: String,
    my_race: u8,
    name: String,
) -> Receiver<io::Result<Joined>> {
    let url = ws_url(&base, &code.to_uppercase(), "join");
    let clean: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == ' ')
        .take(16)
        .collect();
    let (tx, rx) = channel();
    match open_session(
        &url,
        my_race,
        0,
        String::new(),
        clean,
        Phase::AutoWaitSeat,
        Sink::Auto(tx.clone()),
    ) {
        Ok(sess) => drive(sess),
        Err(e) => {
            let _ = tx.send(Err(e));
        }
    }
    rx
}

pub fn join_relay_async(
    base: String,
    code: String,
    my_race: u8,
) -> Receiver<io::Result<Started>> {
    let url = ws_url(&base, &code.to_uppercase(), "join");
    let (tx, rx) = channel();
    match open_session(
        &url,
        my_race,
        0,
        String::new(),
        String::new(),
        Phase::JoinWaitStart,
        Sink::Duel(tx.clone()),
    ) {
        Ok(sess) => drive(sess),
        Err(e) => {
            let _ = tx.send(Err(e));
        }
    }
    rx
}

pub enum QueueEvent {
    Queued { mmr: i32, games: u32 },
    Searching { tol: i32, waited_s: u32 },
    Matched { opp_name: String, opp_mmr: i32 },
    Started(io::Result<(Started, String)>),
}

pub fn find_match_async(
    _base: String,
    _id: String,
    _name: String,
    _race: u8,
) -> Receiver<QueueEvent> {
    let (tx, rx) = channel();
    let _ = tx.send(QueueEvent::Started(Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "matchmaking needs the desktop build",
    ))));
    rx
}

pub fn report_result_async(_base: String, _code: String, _id: String, _winner_slot: u8) {}

pub fn fetch_rating_async(_base: String, _id: String) -> Receiver<Option<(i32, u32)>> {
    let (tx, rx) = channel();
    let _ = tx.send(None);
    rx
}

pub fn fetch_rating_async_delayed(
    _base: String,
    _id: String,
    _delay_ms: u64,
) -> Receiver<Option<(i32, u32)>> {
    let (tx, rx) = channel();
    let _ = tx.send(None);
    rx
}

pub fn check_update_async() -> Receiver<Option<(String, String)>> {
    let (tx, rx) = channel();
    let _ = tx.send(None);
    rx
}

pub fn open_url(_url: &str) {}

#[derive(Clone, Debug)]
pub struct LadderRow {
    pub id: String,
    pub name: String,
    pub mmr: i32,
    pub games: u32,
}

pub fn fetch_ladder_async(_base: String) -> Receiver<Option<Vec<LadderRow>>> {
    let (tx, rx) = channel();
    let _ = tx.send(None);
    rx
}
