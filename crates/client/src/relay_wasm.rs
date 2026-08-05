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

use orion_sim::net::{Msg, Net, Started, INPUT_DELAY_MIN};
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
    JoinWaitGo { seed: u64, host_race: u8, join_race: u8, map: String },
    Done,
}

struct Session {
    ws: WebSocket,
    out_rx: Receiver<String>,
    in_tx: Sender<Msg>,
    /// Messages that arrive during the handshake but belong to the game
    /// (early Cmds from a fast peer) — replayed into in_tx on completion.
    phase: Phase,
    my_race: u8,
    seed: u64,
    map: String,
    result_tx: Sender<io::Result<Started>>,
    net_parts: Option<(Sender<String>, Receiver<Msg>)>,
    raw_in: Rc<RefCell<Vec<String>>>,
    opened: Rc<RefCell<bool>>,
    closed: Rc<RefCell<bool>>,
    started_at: f64,
}

fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}

fn parse_msg(line: &str) -> Option<Msg> {
    ron::de::from_str(line.trim()).ok()
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
    host: bool,
    result_tx: Sender<io::Result<Started>>,
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
        out_rx,
        in_tx,
        phase: if host { Phase::HostWaitHello } else { Phase::JoinWaitStart },
        my_race,
        seed,
        map,
        result_tx,
        net_parts: Some((out_tx, in_rx)),
        raw_in,
        opened,
        closed,
        started_at: now_ms(),
    })
}

/// One pump tick: flush outgoing, take incoming, advance the handshake.
/// Returns false when the session is finished (stop the interval).
fn pump(sess: &mut Session) -> bool {
    if *sess.closed.borrow() {
        let _ = sess.result_tx.send(Err(io::Error::new(
            io::ErrorKind::ConnectionReset,
            "connection closed",
        )));
        return false;
    }
    if !*sess.opened.borrow() {
        // 15s connect timeout.
        if now_ms() - sess.started_at > 15_000.0 {
            let _ = sess.result_tx.send(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "relay connect timeout",
            )));
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
                        let _ = sess.result_tx.send(Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "version mismatch - both players must update",
                        )));
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
                        input_delay: delay,
                    };
                    let _ = sess.result_tx.send(Ok(started));
                    sess.phase = Phase::Done;
                }
            }
            Phase::JoinWaitStart => match &msg {
                Msg::Start { seed, host_race, join_race, map } => {
                    sess.phase = Phase::JoinWaitGo {
                        seed: *seed,
                        host_race: *host_race,
                        join_race: *join_race,
                        map: map.clone(),
                    };
                }
                Msg::Reject { reason } => {
                    let _ = sess
                        .result_tx
                        .send(Err(io::Error::new(io::ErrorKind::InvalidData, reason.clone())));
                    return false;
                }
                _ => {}
            },
            Phase::JoinWaitGo { seed, host_race, join_race, map } => match &msg {
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
                        input_delay: delay,
                    };
                    let _ = sess.result_tx.send(Ok(started));
                    sess.phase = Phase::Done;
                }
                _ => {}
            },
            Phase::Done => {
                // In-game: forward straight to the Lockstep receiver.
                let _ = sess.in_tx.send(msg);
            }
        }
    }
    // Flush game-side outgoing (Lockstep pushes into the Net sender).
    while let Ok(line) = sess.out_rx.try_recv() {
        let _ = sess.ws.send_with_str(&line);
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

#[derive(Clone, Debug)]
pub struct LobbyInfo {
    pub code: String,
    pub name: String,
    pub race: u8,
    #[allow(dead_code)]
    pub age_s: u32,
}

pub fn fetch_lobbies_async(_base: String) -> Receiver<Option<Vec<LobbyInfo>>> {
    let (tx, rx) = channel();
    let _ = tx.send(Some(Vec::new()));
    rx
}

pub fn host_relay_async_with_code(
    base: String,
    code: String,
    race: u8,
) -> (String, Receiver<io::Result<Started>>) {
    host_relay_async_full(base, code, race, "COMMANDER", true, "meridian")
}

pub fn host_relay_async_full(
    base: String,
    code: String,
    my_race: u8,
    name: &str,
    private: bool,
    map: &str,
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
    match open_session(&url, my_race, seed, map.to_string(), true, tx.clone()) {
        Ok(sess) => drive(sess),
        Err(e) => {
            let _ = tx.send(Err(e));
        }
    }
    (code, rx)
}

pub fn join_relay_async(
    base: String,
    code: String,
    my_race: u8,
) -> Receiver<io::Result<Started>> {
    let url = ws_url(&base, &code.to_uppercase(), "join");
    let (tx, rx) = channel();
    match open_session(&url, my_race, 0, String::new(), false, tx.clone()) {
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
