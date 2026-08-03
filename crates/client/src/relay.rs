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

/// Host through the relay. Returns the lobby code immediately; the Started
/// arrives on the receiver when an opponent joins.
pub fn host_relay_async(base: String, my_race: u8) -> (String, Receiver<io::Result<Started>>) {
    host_relay_async_with_code(base, fresh_code(), my_race)
}

pub fn host_relay_async_with_code(
    base: String,
    code: String,
    my_race: u8,
) -> (String, Receiver<io::Result<Started>>) {
    let url = ws_url(&base, &code, "host");
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let result = ws_net(&url).and_then(|net| {
            host_handshake(net, my_race, orion_sim::net::fresh_seed())
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
