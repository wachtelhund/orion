//! WebSocket transport to the Cloudflare lobby relay: lobby codes instead
//! of IP addresses. Each peer connects wss://relay/ws/CODE?role=... and the
//! relay forwards our RON lines verbatim; the normal Hello/Start handshake
//! and Lockstep protocol run unchanged on top via `Net::from_channels`.

use std::io;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use orion_sim::net::{host_handshake, join_handshake, Msg, Net, Started};
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::Message;

/// Human-friendly lobby code (unambiguous alphabet).
pub fn fresh_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut x = orion_sim::net::fresh_seed() | 1;
    (0..5)
        .map(|_| {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            let i = (x.wrapping_mul(0x2545F4914F6CDD1D) >> 59) as usize;
            ALPHABET[i % ALPHABET.len()] as char
        })
        .collect()
}

/// Connect a WebSocket and pump it as a `Net`. One thread owns the socket:
/// short read timeouts interleave sends and receives.
fn ws_net(url: &str) -> io::Result<Net> {
    let req = url
        .into_client_request()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
    let (mut socket, _resp) = tungstenite::connect(req)
        .map_err(|e| io::Error::new(io::ErrorKind::ConnectionRefused, e.to_string()))?;
    match socket.get_mut() {
        MaybeTlsStream::Plain(s) => {
            let _ = s.set_read_timeout(Some(Duration::from_millis(30)));
        }
        MaybeTlsStream::Rustls(s) => {
            let _ = s.get_mut().set_read_timeout(Some(Duration::from_millis(30)));
        }
        _ => {}
    }
    let (out_tx, out_rx): (Sender<String>, Receiver<String>) = channel();
    let (in_tx, in_rx): (Sender<Msg>, Receiver<Msg>) = channel();
    std::thread::spawn(move || {
        loop {
            // Drain outgoing.
            loop {
                match out_rx.try_recv() {
                    Ok(line) => {
                        if socket.send(Message::Text(line.into())).is_err() {
                            return;
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        let _ = socket.close(None);
                        return;
                    }
                }
            }
            // Read with timeout.
            match socket.read() {
                Ok(Message::Text(t)) => {
                    if let Some(m) = Net::parse_line(t.as_str()) {
                        if in_tx.send(m).is_err() {
                            return;
                        }
                    }
                    // Non-Msg text (e.g. relay_error JSON) is ignored; the
                    // relay closes right after, surfacing as a disconnect.
                }
                Ok(Message::Close(_)) => return,
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::TimedOut => {}
                Err(_) => return,
            }
        }
    });
    Ok(Net::from_channels(out_tx, in_rx))
}

fn ws_url(base: &str, code: &str, role: &str) -> String {
    let base = base.trim_end_matches('/');
    format!("{base}/ws/{code}?role={role}")
}

fn http_base(base: &str) -> String {
    base.trim_end_matches('/')
        .replacen("wss://", "https://", 1)
        .replacen("ws://", "http://", 1)
}

/// A public lobby as advertised by the relay directory.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct LobbyInfo {
    pub code: String,
    pub name: String,
    pub race: u8,
    #[allow(dead_code)]
    pub age_s: u32,
}

/// Fetch the public lobby list (blocking — call in a thread).
pub fn fetch_lobbies(base: &str) -> Option<Vec<LobbyInfo>> {
    let url = format!("{}/lobbies", http_base(base));
    let body = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .ok()?
        .into_string()
        .ok()?;
    serde_json::from_str(&body).ok()
}

pub fn fetch_lobbies_async(base: String) -> Receiver<Option<Vec<LobbyInfo>>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(fetch_lobbies(&base));
    });
    rx
}

/// Host a private (unlisted) lobby with a known code — used by --mp-auto
/// smoke tests. Returns the code immediately; the Started arrives on the
/// receiver when an opponent joins.
pub fn host_relay_async_with_code(
    base: String,
    code: String,
    my_race: u8,
) -> (String, Receiver<io::Result<Started>>) {
    host_relay_async_full(base, code, my_race, "COMMANDER", true, "meridian")
}

/// Full host entry: public lobbies appear in the directory under `name`.
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
    let map = map.to_string();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let result = ws_net(&url).and_then(|net| {
            host_handshake(net, my_race, orion_sim::net::fresh_seed(), &map)
        });
        let _ = tx.send(result);
    });
    (code, rx)
}

/// Join a lobby code through the relay.
pub fn join_relay_async(base: String, code: String, my_race: u8) -> Receiver<io::Result<Started>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let url = ws_url(&base, &code.to_uppercase(), "join");
        let result = ws_net(&url).and_then(|net| join_handshake(net, my_race));
        let _ = tx.send(result);
    });
    rx
}

// ------------------------------------------------------------- ranked ----

/// Events streamed back to the UI while FIND MATCH runs.
pub enum QueueEvent {
    Queued { mmr: i32, games: u32 },
    Searching { tol: i32, waited_s: u32 },
    Matched { opp_name: String, opp_mmr: i32 },
    /// Terminal: the lockstep session (plus the ranked match code used for
    /// result reporting), or the error that ended the search.
    Started(io::Result<(Started, String)>),
}

/// Round-trip time to the relay edge, for the latency half of matchmaking.
fn measure_rtt(base: &str) -> u32 {
    let url = format!("{}/ping", http_base(base));
    let mut best = 2000u32;
    for _ in 0..2 {
        let t0 = std::time::Instant::now();
        if ureq::get(&url)
            .timeout(Duration::from_secs(2))
            .call()
            .is_ok()
        {
            best = best.min(t0.elapsed().as_millis() as u32);
        }
    }
    if best == 2000 {
        200 // probe failed: assume average, let the server decide
    } else {
        best
    }
}

/// Queue for a ranked match. The whole lifecycle runs in one thread:
/// measure RTT -> hold a /queue socket -> on match, run the ordinary lobby
/// handshake (host or join as told) -> emit Started.
pub fn find_match_async(
    base: String,
    id: String,
    name: String,
    race: u8,
) -> Receiver<QueueEvent> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let rtt = measure_rtt(&base);
        let clean: String = name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == ' ')
            .take(16)
            .collect();
        let url = format!(
            "{}/queue?id={}&name={}&race={}&rtt={}",
            base.trim_end_matches('/'),
            id,
            clean.replace(' ', "%20"),
            race,
            rtt
        );
        let req = match url.clone().into_client_request() {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.send(QueueEvent::Started(Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    e.to_string(),
                ))));
                return;
            }
        };
        let mut socket = match tungstenite::connect(req) {
            Ok((s, _)) => s,
            Err(e) => {
                let _ = tx.send(QueueEvent::Started(Err(io::Error::new(
                    io::ErrorKind::ConnectionRefused,
                    e.to_string(),
                ))));
                return;
            }
        };
        // Wait for match. The server talks every few seconds; failing to
        // forward an event means the UI cancelled — close and bail.
        let (code, role, map) = loop {
            match socket.read() {
                Ok(Message::Text(t)) => {
                    let Ok(v) = serde_json::from_str::<serde_json::Value>(t.as_str()) else {
                        continue;
                    };
                    match v["type"].as_str() {
                        Some("queued") => {
                            let ev = QueueEvent::Queued {
                                mmr: v["mmr"].as_i64().unwrap_or(1200) as i32,
                                games: v["games"].as_u64().unwrap_or(0) as u32,
                            };
                            if tx.send(ev).is_err() {
                                let _ = socket.close(None);
                                return;
                            }
                        }
                        Some("searching") => {
                            let ev = QueueEvent::Searching {
                                tol: v["tol"].as_i64().unwrap_or(100) as i32,
                                waited_s: v["waited_s"].as_u64().unwrap_or(0) as u32,
                            };
                            if tx.send(ev).is_err() {
                                let _ = socket.close(None);
                                return;
                            }
                        }
                        Some("match") => {
                            let ev = QueueEvent::Matched {
                                opp_name: v["opp_name"].as_str().unwrap_or("?").to_string(),
                                opp_mmr: v["opp_mmr"].as_i64().unwrap_or(0) as i32,
                            };
                            let _ = tx.send(ev);
                            break (
                                v["code"].as_str().unwrap_or("").to_string(),
                                v["role"].as_str().unwrap_or("join").to_string(),
                                v["map"].as_str().unwrap_or("meridian").to_string(),
                            );
                        }
                        _ => {
                            let _ = tx.send(QueueEvent::Started(Err(io::Error::new(
                                io::ErrorKind::Other,
                                "matchmaker rejected the queue",
                            ))));
                            return;
                        }
                    }
                }
                Ok(Message::Close(_)) | Err(_) => {
                    let _ = tx.send(QueueEvent::Started(Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        "matchmaking connection lost",
                    ))));
                    return;
                }
                Ok(_) => {}
            }
        };
        // Reconnect through the normal lobby relay for the actual game.
        let result = if role == "host" {
            let url = format!("{}&private=1", ws_url(&base, &code, "host"));
            ws_net(&url)
                .and_then(|net| host_handshake(net, race, orion_sim::net::fresh_seed(), &map))
        } else {
            // The host needs a beat to open the lobby; retry briefly.
            let mut last = io::Error::new(io::ErrorKind::NotFound, "host never arrived");
            let mut ok = None;
            for _ in 0..8 {
                std::thread::sleep(Duration::from_millis(400));
                match ws_net(&ws_url(&base, &code, "join"))
                    .and_then(|net| join_handshake(net, race))
                {
                    Ok(s) => {
                        ok = Some(s);
                        break;
                    }
                    Err(e) => last = e,
                }
            }
            ok.ok_or(last)
        };
        let _ = tx.send(QueueEvent::Started(result.map(|s| (s, code))));
    });
    rx
}

/// Fire-and-forget result report: both clients report, the matchmaker
/// updates Elo when they agree (or on timeout with one report).
pub fn report_result_async(base: String, code: String, id: String, winner_slot: u8) {
    std::thread::spawn(move || {
        let url = format!("{}/result", http_base(&base));
        let _ = ureq::post(&url)
            .timeout(Duration::from_secs(6))
            .send_json(serde_json::json!({
                "code": code,
                "id": id,
                "winner_slot": winner_slot,
            }));
    });
}

/// Current rating for the FIND MATCH row / end screen.
pub fn fetch_rating_async(base: String, id: String) -> Receiver<Option<(i32, u32)>> {
    fetch_rating_async_delayed(base, id, 0)
}

/// Delayed variant: the end screen waits a beat so the opponent's
/// confirming report can land before we read the updated Elo.
pub fn fetch_rating_async_delayed(
    base: String,
    id: String,
    delay_ms: u64,
) -> Receiver<Option<(i32, u32)>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        if delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(delay_ms));
        }
        let url = format!("{}/rating?id={}", http_base(&base), id);
        let got = ureq::get(&url)
            .timeout(Duration::from_secs(5))
            .call()
            .ok()
            .and_then(|r| r.into_string().ok())
            .and_then(|b| serde_json::from_str::<serde_json::Value>(&b).ok())
            .map(|v| {
                (
                    v["mmr"].as_f64().unwrap_or(1200.0).round() as i32,
                    v["games"].as_u64().unwrap_or(0) as u32,
                )
            });
        let _ = tx.send(got);
    });
    rx
}

// ------------------------------------------------------------- updates ----

/// Latest release tag on GitHub (e.g. "v0.4.0"), if newer than ours.
/// Best-effort: any failure just means no notice.
pub fn check_update_async() -> Receiver<Option<(String, String)>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let got = ureq::get("https://api.github.com/repos/wachtelhund/orion/releases/latest")
            .set("User-Agent", "orion-client")
            .timeout(Duration::from_secs(6))
            .call()
            .ok()
            .and_then(|r| r.into_string().ok())
            .and_then(|b| serde_json::from_str::<serde_json::Value>(&b).ok())
            .and_then(|v| {
                let tag = v["tag_name"].as_str()?.to_string();
                let url = v["html_url"].as_str()?.to_string();
                Some((tag, url))
            })
            .filter(|(tag, _)| {
                newer_than_current(tag.trim_start_matches('v'), env!("CARGO_PKG_VERSION"))
            });
        let _ = tx.send(got);
    });
    rx
}

/// Semver-ish compare: is `remote` strictly newer than `local`?
fn newer_than_current(remote: &str, local: &str) -> bool {
    let parse = |s: &str| -> Vec<u32> {
        s.split('.').map(|p| p.parse().unwrap_or(0)).collect()
    };
    parse(remote) > parse(local)
}

/// Open a URL in the system browser (fire-and-forget).
pub fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(target_os = "windows")]
    let cmd = "cmd";
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let cmd = "xdg-open";
    let mut c = std::process::Command::new(cmd);
    #[cfg(target_os = "windows")]
    c.args(["/C", "start", "", url]);
    #[cfg(not(target_os = "windows"))]
    c.arg(url);
    let _ = c.spawn();
}
