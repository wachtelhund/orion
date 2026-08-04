//! Player settings + keybinds, persisted to `$HOME/.orion-settings.ron`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use winit::keyboard::KeyCode;

/// Rebindable game actions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Attack,
    Stop,
    Hold,
    BuildMenu,
    Train0,
    Train1,
    Place1,
    Place2,
    Place3,
    Place4,
    Place5,
    Place6,
    Place7,
    Train2,
    SiegeToggle,
    CastStorm,
    IdleWorker,
    JumpAlert,
    CycleBase,
    CycleSubgroup,
    CancelConstruction,
    RevealFog,
}

pub const ALL_ACTIONS: [Action; 22] = [
    Action::Attack,
    Action::Stop,
    Action::Hold,
    Action::BuildMenu,
    Action::Train0,
    Action::Train1,
    Action::Place1,
    Action::Place2,
    Action::Place3,
    Action::Place4,
    Action::Place5,
    Action::Place6,
    Action::Place7,
    Action::Train2,
    Action::SiegeToggle,
    Action::CastStorm,
    Action::IdleWorker,
    Action::JumpAlert,
    Action::CycleBase,
    Action::CycleSubgroup,
    Action::CancelConstruction,
    Action::RevealFog,
];

impl Action {
    pub fn name(self) -> &'static str {
        match self {
            Action::Attack => "attack",
            Action::Stop => "stop",
            Action::Hold => "hold",
            Action::BuildMenu => "build_menu",
            Action::Train0 => "train_slot_1",
            Action::Train1 => "train_slot_2",
            Action::Place1 => "build_pylon",
            Action::Place2 => "build_hall",
            Action::Place3 => "build_hq",
            Action::Place4 => "build_condenser",
            Action::Place5 => "build_forge",
            Action::Place6 => "build_aerie",
            Action::Place7 => "build_archive",
            Action::Train2 => "train_slot_3",
            Action::SiegeToggle => "siege_mode",
            Action::CastStorm => "plasma_storm",
            Action::IdleWorker => "idle_worker",
            Action::JumpAlert => "jump_to_alert",
            Action::CycleBase => "cycle_bases",
            Action::CycleSubgroup => "cycle_subgroup",
            Action::CancelConstruction => "cancel_construction",
            Action::RevealFog => "reveal_fog",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::Attack => "ATTACK MOVE",
            Action::Stop => "STOP",
            Action::Hold => "HOLD POSITION",
            Action::BuildMenu => "BUILD MENU",
            Action::Train0 => "TRAIN SLOT 1",
            Action::Train1 => "TRAIN SLOT 2",
            Action::Place1 => "PLACE PYLON",
            Action::Place2 => "PLACE MUSTER HALL",
            Action::Place3 => "PLACE HQ",
            Action::Place4 => "PLACE CONDENSER",
            Action::Place5 => "PLACE FORGE",
            Action::Place6 => "PLACE AERIE",
            Action::Place7 => "PLACE ARCHIVE",
            Action::Train2 => "TRAIN SLOT 3",
            Action::SiegeToggle => "SIEGE MODE",
            Action::CastStorm => "PLASMA STORM",
            Action::IdleWorker => "SELECT IDLE WORKER",
            Action::JumpAlert => "JUMP TO LAST ALERT",
            Action::CycleBase => "CYCLE BASES",
            Action::CycleSubgroup => "CYCLE SUBGROUP (TAB)",
            Action::CancelConstruction => "CANCEL CONSTRUCTION",
            Action::RevealFog => "REVEAL FOG (DEV)",
        }
    }

    /// SC2-style grid: build-menu entries sit on Q/W/E/R. Contexts overlap
    /// (Q trains in a building, places in the build menu) — resolution is
    /// contextual, see `App::action_for_context`.
    pub fn default_key(self) -> KeyCode {
        match self {
            Action::Attack => KeyCode::KeyA,
            Action::Stop => KeyCode::KeyS,
            Action::Hold => KeyCode::KeyH,
            Action::BuildMenu => KeyCode::KeyB,
            Action::Train0 => KeyCode::KeyQ,
            Action::Train1 => KeyCode::KeyW,
            Action::Place1 => KeyCode::KeyQ,
            Action::Place2 => KeyCode::KeyW,
            Action::Place3 => KeyCode::KeyE,
            Action::Place4 => KeyCode::KeyR,
            Action::Place5 => KeyCode::KeyA,
            Action::Place6 => KeyCode::KeyS,
            Action::Place7 => KeyCode::KeyD,
            Action::Train2 => KeyCode::KeyE,
            Action::SiegeToggle => KeyCode::KeyD,
            Action::CastStorm => KeyCode::KeyE,
            Action::IdleWorker => KeyCode::F1,
            Action::JumpAlert => KeyCode::Space,
            Action::CycleBase => KeyCode::Backspace,
            Action::CycleSubgroup => KeyCode::Tab,
            Action::CancelConstruction => KeyCode::KeyX,
            Action::RevealFog => KeyCode::F3,
        }
    }
}

/// Keys the rebind UI accepts.
pub const BINDABLE_KEYS: &[KeyCode] = &[
    KeyCode::KeyA,
    KeyCode::KeyB,
    KeyCode::KeyC,
    KeyCode::KeyD,
    KeyCode::KeyE,
    KeyCode::KeyF,
    KeyCode::KeyG,
    KeyCode::KeyH,
    KeyCode::KeyI,
    KeyCode::KeyJ,
    KeyCode::KeyK,
    KeyCode::KeyL,
    KeyCode::KeyM,
    KeyCode::KeyN,
    KeyCode::KeyO,
    KeyCode::KeyP,
    KeyCode::KeyQ,
    KeyCode::KeyR,
    KeyCode::KeyS,
    KeyCode::KeyT,
    KeyCode::KeyU,
    KeyCode::KeyV,
    KeyCode::KeyW,
    KeyCode::KeyX,
    KeyCode::KeyY,
    KeyCode::KeyZ,
    KeyCode::Digit0,
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
    KeyCode::F1,
    KeyCode::F2,
    KeyCode::F3,
    KeyCode::F4,
    KeyCode::F5,
    KeyCode::F6,
    KeyCode::F7,
    KeyCode::F8,
    KeyCode::Space,
    KeyCode::Tab,
    KeyCode::Backspace,
    KeyCode::Backquote,
    KeyCode::Minus,
    KeyCode::Equal,
];

pub fn key_name(code: KeyCode) -> String {
    format!("{code:?}")
}

pub fn key_from_name(name: &str) -> Option<KeyCode> {
    BINDABLE_KEYS.iter().copied().find(|k| key_name(*k) == name)
}

/// Short display label for a key (strips Key/Digit prefixes).
pub fn key_label(code: KeyCode) -> String {
    let n = key_name(code);
    n.strip_prefix("Key")
        .or_else(|| n.strip_prefix("Digit"))
        .unwrap_or(&n)
        .to_string()
}

fn default_name() -> String {
    "COMMANDER".into()
}
fn default_relay() -> String {
    "wss://orion-relay.wachtelbruno.workers.dev".into()
}
fn default_music() -> f32 {
    0.5
}
fn default_sfx() -> f32 {
    0.7
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Settings {
    pub hud_scale: f32,
    pub fullscreen: bool,
    pub edge_scroll: bool,
    pub game_speed: f32,
    #[serde(default = "default_music")]
    pub music_volume: f32,
    #[serde(default = "default_sfx")]
    pub sfx_volume: f32,
    /// Lobby relay endpoint (Cloudflare worker). Empty disables online play.
    #[serde(default = "default_relay")]
    pub relay_url: String,
    /// Name shown in the multiplayer lobby list.
    #[serde(default = "default_name")]
    pub player_name: String,
    /// Anonymous persistent identity for ranked matchmaking (random hex,
    /// generated on first launch). The MMR on the relay is keyed by this.
    #[serde(default = "fresh_player_id")]
    pub player_id: String,
    /// action name -> key name; missing entries use defaults.
    pub binds: BTreeMap<String, String>,
}

/// 16 hex chars from OS entropy (client-side identity, never sim-side).
fn fresh_player_id() -> String {
    let mut t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(12345);
    t ^= std::process::id() as u64 * 0x9E3779B97F4A7C15;
    let mut s = String::new();
    for _ in 0..2 {
        t = t.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        s.push_str(&format!("{:08x}", (t >> 24) as u32));
    }
    s
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            hud_scale: 1.15,
            fullscreen: false,
            edge_scroll: true,
            game_speed: 1.0,
            music_volume: default_music(),
            sfx_volume: default_sfx(),
            relay_url: default_relay(),
            player_name: default_name(),
            player_id: fresh_player_id(),
            binds: BTreeMap::new(),
        }
    }
}

fn path() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".orion-settings.ron"))
}

impl Settings {
    pub fn load() -> Settings {
        let Some(p) = path() else { return Settings::default() };
        std::fs::read_to_string(p)
            .ok()
            .and_then(|s| ron::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(p) = path() {
            if let Ok(s) = ron::ser::to_string_pretty(self, Default::default()) {
                let _ = std::fs::write(p, s);
            }
        }
    }

    pub fn key_for(&self, a: Action) -> KeyCode {
        self.binds
            .get(a.name())
            .and_then(|n| key_from_name(n))
            .unwrap_or_else(|| a.default_key())
    }

    pub fn set_key(&mut self, a: Action, code: KeyCode) {
        // Duplicates across contexts are fine (grid layout); resolution is
        // contextual in the client.
        self.binds.insert(a.name().to_string(), key_name(code));
    }

    /// First action bound to a key in ALL_ACTIONS order. Prefer
    /// `App::action_for_context` in game code.
    pub fn action_for(&self, code: KeyCode) -> Option<Action> {
        ALL_ACTIONS.iter().copied().find(|a| self.key_for(*a) == code)
    }
}
