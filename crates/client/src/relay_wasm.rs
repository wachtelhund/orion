//! Browser build stub for the relay module. Online play in the browser
//! needs a web-sys WebSocket transport — until that lands, every entry
//! point reports "not available on web" through the same channel shapes
//! the native module uses, so no call site changes.

use std::io;
use std::sync::mpsc::{channel, Receiver};

use orion_sim::net::Started;

pub fn fresh_code() -> String {
    "WEBXX".into()
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

fn unavailable() -> Receiver<io::Result<Started>> {
    let (tx, rx) = channel();
    let _ = tx.send(Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "online play is not in the web demo yet - grab the desktop build",
    )));
    rx
}

pub fn host_relay_async_with_code(
    _base: String,
    code: String,
    _race: u8,
) -> (String, Receiver<io::Result<Started>>) {
    (code, unavailable())
}

pub fn host_relay_async_full(
    _base: String,
    code: String,
    _race: u8,
    _name: &str,
    _private: bool,
    _map: &str,
) -> (String, Receiver<io::Result<Started>>) {
    (code, unavailable())
}

pub fn join_relay_async(
    _base: String,
    _code: String,
    _race: u8,
) -> Receiver<io::Result<Started>> {
    unavailable()
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
