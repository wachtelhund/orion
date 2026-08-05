//! Game client: session flow (menus / in-game), fixed-timestep sim driving,
//! input -> Commands, and world rendering. HUD lives in hud.rs, menus in
//! menu.rs.

use std::collections::HashSet;
use crate::clock::Instant;

use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

use orion_sim::ai::{Bot, Difficulty};
use orion_sim::net::{Lockstep, Started};
use orion_sim::data::DefId;
use orion_sim::map::{meridian, TileKind, TilePos};
use orion_sim::state::{BuildPhase, GatherPhase, SimEvent, NEUTRAL, RES_GEYSER, RES_MINERALS};
use orion_sim::{Command, EntityId, EntityKind, FxVec2, GameData, Order, State};

use crate::atlas::hash2;
use crate::audio::{Audio, Sfx};
use crate::config::{Action, Settings};
use crate::gfx::{Gfx, Inst};
use crate::iso::{self, Camera};
use crate::menu::MenuPage;

pub const TICK_DT: f64 = 1.0 / 24.0;

pub const BOT: u8 = 1;

pub use orion_sim::ai::Bot as Bot2;

pub const TEAM_COLORS: [[f32; 3]; 2] = [[0.30, 0.58, 1.0], [1.0, 0.38, 0.30]];
pub const MINERAL_COLOR: [f32; 3] = [0.35, 0.85, 0.95];
pub const GAS_COLOR: [f32; 3] = [0.35, 0.95, 0.85];
pub const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Normal,
    AttackMove,
    BuildMenu,
    Placing(DefId),
    /// Click a target point for Plasma Storm.
    CastTarget,
    /// Click a target point for a hero ability (slot).
    AbilityTarget(u8),
}

#[derive(Clone, Copy, PartialEq)]
pub enum EffKind {
    Flash,
    Tracer,
    Spark,
    Ring,
    Corpse,
    Rubble,
    /// Traveling projectile ax,ay -> bx,by with a lobbed arc. style: 0
    /// acid glob, 1 rail slug, 2 cannon shell.
    Glob,
    /// Melee slash fan at the target. style: 0 blade (team), 1 chitin
    /// claw, 2 heavy claw, 3 mech claw.
    Slash,
    /// Jagged electric arc ax,ay -> bx,by (Ferron arclight).
    Arc,
    /// Stateless particle burst. style: 0 blood, 1 goo, 2 mech debris.
    Burst,
    /// Ground decal. style: 0 blood stain, 1 goo puddle, 2 scorch.
    Stain,
    /// Rising smoke puff (also used by ambient emitters).
    Smoke,
    /// Rising teal geyser vapor.
    Vapor,
    /// Command acknowledgement ping. bx: 0=move, 1=attack, 2=gather/rally.
    Ping,
    /// Under-attack alert shown on the minimap.
    MapPing,
}

#[derive(Clone, Copy)]
pub struct Effect {
    pub kind: EffKind,
    pub ax: f32,
    pub ay: f32,
    pub bx: f32,
    pub by: f32,
    pub age: f32,
    pub ttl: f32,
    /// Kind-specific flavor: weapon style for combat effects, material
    /// for stains, seed salt for particle bursts.
    pub style: u8,
}

/// Building sprite anchor: canvas point that sits on the building's world
/// center. [type] -> (x, y) in canvas px. hq, depot, barracks, condenser.
pub const BUILDING_ANCHOR: [(f32, f32); 21] = [
    (50.0, 54.0), // hq
    (34.0, 46.0), // depot
    (50.0, 50.0), // barracks
    (34.0, 42.0), // condenser
    (50.0, 50.0), // forge
    (50.0, 48.0), // aerie
    (35.0, 46.0), // archive
    (50.0, 52.0), // hive
    (32.0, 46.0), // spire
    (34.0, 40.0), // sapwell
    (48.0, 44.0), // warren
    (48.0, 50.0), // incubator
    (42.0, 56.0), // roost
    (33.0, 42.0), // cortex
    (50.0, 54.0), // bastion
    (34.0, 46.0), // mast
    (34.0, 42.0), // fumetap
    (50.0, 50.0), // assembly
    (50.0, 50.0), // refit
    (50.0, 48.0), // skydock
    (33.0, 42.0), // relay
];

pub struct App {
    pub gfx: Gfx,
    pub state: State,
    pub bot: Bot,
    pub difficulty: Difficulty,
    pub pending: Vec<(u8, Command)>,
    pub acc: f64,
    pub last: Instant,
    pub smoke_deadline: Option<Instant>,
    pub shot: Option<(u32, String)>,
    pub shot_bot0: Option<Bot>,
    /// Staged-showcase capture: scripted orders only, no bot drivers.
    pub stage_mode: bool,
    pub shot_focus: Option<(f32, f32)>,
    pub shot_zoom: Option<f32>,
    pub script: Option<String>,
    /// Automated MP smoke: "host" or "join" — connects on localhost, plays
    /// ~10s of lockstep, exits 0.
    pub mp_auto: Option<String>,
    pub shot_reveal: bool,
    /// Render a menu page once, capture, exit: "main" | "settings" | "esc".
    pub menu_shot: Option<(String, String)>,
    /// Frame-sequence recorder: (prefix, start_tick, frames, every_ticks).
    pub record: Option<(String, u32, u32, u32)>,
    pub record_done: u32,
    /// Shot/record modes play cross-race (VC vs Kyth) instead of a mirror.
    pub shot_cross: bool,
    pub finished: bool,

    // Session flow.
    /// Local player index: 0 in single player / hosting, 1 when joining.
    pub human: u8,
    pub in_game: bool,
    // Multiplayer session.
    pub mp: Option<Lockstep>,
    pub mp_waiting: Option<std::sync::mpsc::Receiver<std::io::Result<Started>>>,
    pub mp_error: Option<String>,
    /// LAN/dev only (mp-auto tests); never shown in the UI.
    /// Join-code entry field.
    pub code_input: String,
    /// True while the name field has focus (typing edits the player name).
    pub name_focus: bool,
    /// Replay sharing: armed = clicking a replay uploads it for a code.
    pub replay_share_mode: bool,
    /// Code being typed on the replays page (fetch a shared replay).
    pub replay_code: String,
    /// Status line for the replays page (codes, errors).
    pub replay_status: Option<String>,
    pub replay_share_rx: Option<std::sync::mpsc::Receiver<std::io::Result<String>>>,
    pub replay_fetch_rx: Option<std::sync::mpsc::Receiver<std::io::Result<String>>>,
    /// Set while hosting through the relay: show this code to the opponent.
    pub mp_lobby_code: Option<String>,
    pub mp_private: bool,
    /// Public lobby browser state.
    pub lobby_list: Vec<crate::relay::LobbyInfo>,
    pub lobby_fetch: Option<std::sync::mpsc::Receiver<Option<Vec<crate::relay::LobbyInfo>>>>,
    pub lobby_fetch_at: Option<Instant>,
    // Ranked matchmaking session state.
    pub mm_queue: Option<std::sync::mpsc::Receiver<crate::relay::QueueEvent>>,
    pub mm_status: String,
    /// Ranked code of the running game — present = report the result.
    pub mm_code: Option<String>,
    pub mm_reported: bool,
    pub mm_rating: Option<(i32, u32)>,
    pub mm_rating_rx: Option<std::sync::mpsc::Receiver<Option<(i32, u32)>>>,
    /// Alternates ack variations so spam clicks don't grate.
    pub ack_flip: bool,
    /// Match-start freeze: countdown seconds remaining (5..=1), and the
    /// wall-clock moment the countdown began.
    pub countdown: Option<(Instant, u32)>,
    /// Ladder page contents + pending fetch.
    pub ladder: Option<Vec<crate::relay::LadderRow>>,
    pub ladder_rx: Option<std::sync::mpsc::Receiver<Option<Vec<crate::relay::LadderRow>>>>,
    /// Newer release on GitHub: (tag, html url).
    pub update: Option<(String, String)>,
    pub update_rx: Option<std::sync::mpsc::Receiver<Option<(String, String)>>>,
    /// An update exists and the player declined: online play is blocked
    /// (mismatched builds desync in lockstep; the relay won't pair them).
    pub mp_blocked: bool,
    /// Race picked for the human, and the enemy choice (0/1, 2 = random).
    pub chosen_race: u8,
    /// Active settings tab: 0 general, 1 hotkeys.
    pub settings_tab: u8,
    /// Map editor session (replaces the game view while Some).
    pub editor: Option<crate::editor::Editor>,
    editor_dragging: bool,
    pub enemy_race_choice: u8,
    /// Menu map pick (index into map::MAP_NAMES) + the map of the RUNNING
    /// game (MP joiners play the host's choice, whatever their menu says).
    pub map_choice: usize,
    pub game_map: String,
    // Replay session (Some = watching, not playing).
    pub replay: Option<orion_sim::replay::Replay>,
    pub replay_cursor: usize,
    pub replay_paused: bool,
    pub replay_speed: f32,
    /// Fog perspective while watching: player index, or 2 = reveal all.
    pub replay_view: u8,
    /// Auto-save guard: set for real games, cleared for scripted/shot modes
    /// and once a save happens.
    pub record_replay: bool,
    /// REPLAYS menu contents: (label, path).
    pub replay_files: Vec<(String, std::path::PathBuf)>,
    /// Headless replay capture: (target tick, out path).
    pub replay_shot: Option<(u32, String)>,
    /// Observer follow-cam for --record: EMA of recent combat positions.
    pub follow: bool,
    pub follow_pos: Option<(f32, f32)>,
    pub follow_quiet: u32,
    pub page: MenuPage,
    pub rebinding: Option<Action>,
    pub settings: Settings,
    pub base_scale: f32,
    pub want_fullscreen: Option<bool>,

    pub cam: Camera,
    pub mouse: (f32, f32),
    pub cursor_in: bool,
    pub shift: bool,
    pub ctrl: bool,
    pub keys_down: HashSet<KeyCode>,
    pub select_start: Option<(f32, f32)>,
    pub panning: Option<(f32, f32)>,
    pub selection: Vec<EntityId>,
    pub groups: [Vec<EntityId>; 10],
    pub mode: Mode,
    pub reveal_all: bool,
    pub fps: f32,
    /// Frame counter for throttled debug heartbeats.
    pub dbg_frames: u32,
    /// Clock for hidden-page background stepping (web multiplayer).
    bg_last: Option<Instant>,
    /// Menu map-preview cache: (map name, loaded map). RefCell because
    /// draw_menu is &self and loading a map per frame would be wasteful.
    pub(crate) thumb_cache: std::cell::RefCell<Option<(String, orion_sim::map::Map)>>,
    pub frame_t: Instant,

    // Render-side caches (never touch sim state).
    pub facings: Vec<u8>,
    /// Continuous screen-space heading per entity (radians) — the drawn
    /// 8-way facing quantizes this, so turns sweep instead of snapping.
    headings: Vec<f32>,
    /// Seconds of attack-recoil animation left, per entity index.
    pub recoil: Vec<f32>,
    pub effects: Vec<Effect>,
    pub unit_type: Vec<usize>,
    pub building_type: Vec<usize>,

    // Input feel state.
    pub last_group_tap: Option<(u8, Instant)>,
    pub last_click: Option<(Instant, (f32, f32))>,
    pub last_alert: Option<(f32, f32)>,
    pub base_cycle: usize,
    /// Tab-cycled subgroup index into `selection_types()`.
    pub subgroup_offset: usize,

    // Feedback.
    pub audio: Option<Audio>,
    /// Flashing mid-screen warning ("NOT ENOUGH MINERALS").
    pub error_flash: Option<(String, Instant)>,
    pub last_alarm_sfx: Option<Instant>,
}

impl App {
    pub fn new(gfx: Gfx, smoke: bool, shot: Option<(u32, String)>, scale_factor: f32) -> App {
        let state = new_game();
        let start = state.map.starts[0];
        let (cx, cy) = iso::world_to_iso(start.x as f32 + 0.5, start.y as f32 + 0.5);
        let cam = Camera {
            cx,
            cy,
            zoom: 2.0 * scale_factor,
            screen_w: gfx.config.width as f32,
            screen_h: gfx.config.height as f32,
        };
        let unit_type = state
            .data
            .units
            .iter()
            .map(|u| match u.tag.as_str() {
                "fabricator" => 0,
                "trooper" => 1,
                "vanguard" => 2,
                "breaker" => 3,
                "skywing" => 4,
                "stormcaller" => 5,
                "bulwark" => 13,
                "burrower" => 15,
                "marshal" => 22,
                "broodmother" => 23,
                "magnus" => 24,
                "broodling" => 25,
                "scrapper" => 16,
                "arclight" => 17,
                "mauler" => 18,
                "lodestone" => 19,
                "kestrel" => 20,
                "resonant" => 21,
                "kdrone" => 7,
                "skitter" => 8,
                "spitter" => 9,
                "ravager" => 10,
                "wisp" => 11,
                _ => 12,
            })
            .collect();
        let building_type = state
            .data
            .buildings
            .iter()
            .map(|b| match b.tag.as_str() {
                "hq" => 0,
                "depot" => 1,
                "barracks" => 2,
                "condenser" => 3,
                "forge" => 4,
                "aerie" => 5,
                "archive" => 6,
                "hive" => 7,
                "spire" => 8,
                "sapwell" => 9,
                "warren" => 10,
                "incubator" => 11,
                "roost" => 12,
                "cortex" => 13,
                "bastion" => 14,
                "mast" => 15,
                "fumetap" => 16,
                "assembly" => 17,
                "refit" => 18,
                "skydock" => 19,
                _ => 20,
            })
            .collect();
        let headless = smoke || shot.is_some();
        App {
            gfx,
            state,
            bot: Bot::new(BOT),
            difficulty: Difficulty::Normal,
            pending: Vec::new(),
            acc: 0.0,
            last: Instant::now(),
            smoke_deadline: smoke.then(|| Instant::now() + std::time::Duration::from_secs(3)),
            shot_bot0: shot.is_some().then(|| Bot::new(0)),
            stage_mode: false,
            shot,
            shot_focus: None,
            shot_zoom: None,
            script: None,
            mp_auto: None,
            shot_reveal: false,
            menu_shot: None,
            record: None,
            record_done: 0,
            shot_cross: false,
            finished: false,
            human: 0,
            in_game: headless,
            mp: None,
            mp_waiting: None,
            mp_error: None,
            code_input: String::new(),
            name_focus: false,
            replay_share_mode: false,
            replay_code: String::new(),
            replay_status: None,
            replay_share_rx: None,
            replay_fetch_rx: None,
            mp_lobby_code: None,
            mp_private: false,
            lobby_list: Vec::new(),
            lobby_fetch: None,
            lobby_fetch_at: None,
            mm_queue: None,
            mm_status: String::new(),
            mm_code: None,
            mm_reported: false,
            mm_rating: None,
            mm_rating_rx: None,
            ack_flip: false,
            countdown: None,
            ladder: None,
            ladder_rx: None,
            update: None,
            update_rx: Some(crate::relay::check_update_async()),
            mp_blocked: false,
            replay: None,
            replay_cursor: 0,
            replay_paused: false,
            replay_speed: 1.0,
            replay_view: 2,
            record_replay: false,
            replay_files: Vec::new(),
            replay_shot: None,
            follow: false,
            follow_pos: None,
            follow_quiet: 0,
            map_choice: 0,
            game_map: "meridian".into(),
            chosen_race: 0,
            settings_tab: 0,
            editor: None,
            editor_dragging: false,
            enemy_race_choice: 2,
            page: if headless { MenuPage::None } else { MenuPage::MainRoot },
            rebinding: None,
            settings: Settings::load(),
            base_scale: scale_factor,
            want_fullscreen: None,
            cam,
            mouse: (0.0, 0.0),
            cursor_in: false,
            shift: false,
            ctrl: false,
            keys_down: HashSet::new(),
            select_start: None,
            panning: None,
            selection: Vec::new(),
            groups: Default::default(),
            mode: Mode::Normal,
            reveal_all: false,
            fps: 60.0,
            dbg_frames: 0,
            bg_last: None,
            thumb_cache: std::cell::RefCell::new(None),
            frame_t: Instant::now(),
            facings: Vec::new(),
            headings: Vec::new(),
            recoil: Vec::new(),
            effects: Vec::new(),
            unit_type,
            building_type,
            last_group_tap: None,
            last_click: None,
            last_alert: None,
            base_cycle: 0,
            subgroup_offset: 0,
            audio: None,
            error_flash: None,
            last_alarm_sfx: None,
        }
    }

    /// Persist settings once at startup so the freshly generated ranked
    /// player_id survives (serde default regenerates it on every load
    /// until it lands in the file).
    pub fn persist_identity(&self) {
        self.settings.save();
    }

    /// Init audio (skipped in headless capture modes).
    pub fn init_audio(&mut self) {
        self.audio = Audio::new(self.settings.music_volume, self.settings.sfx_volume);
    }

    pub(crate) fn sfx(&self, s: Sfx) {
        if let Some(a) = &self.audio {
            a.play(s);
        }
    }

    /// Selection acknowledgment: race-flavored blip for the active thing.
    /// What a thing is made of, for death presentation: 0 mech (explodes),
    /// 1 flesh (bleeds), 2 alien (goos). By sprite type.
    fn material(&self, kind: EntityKind, def: u16) -> u8 {
        if kind == EntityKind::Building {
            let bt = self.building_type[def as usize];
            return if (7..=13).contains(&bt) { 2 } else { 0 };
        }
        match self.unit_type[def as usize] {
            1 | 2 | 5 => 1,
            7..=12 | 15 => 2,
            _ => 0,
        }
    }

    /// Weapon presentation class by attacker sprite type: how its fire
    /// looks and sounds. 0 rifle, 1 energy blade, 2 cannon, 3 rapid bolts,
    /// 4 acid glob, 5 chitin claw, 6 heavy claw, 7 electric arc, 8 rail
    /// slug, 9 bite, 10 utility tool.
    fn fire_style(&self, unit_type: usize) -> u8 {
        match unit_type {
            1 => 0,
            2 => 1,
            3 | 6 => 2,
            4 | 11 | 20 => 3,
            9 => 4,
            8 => 5,
            10 => 6,
            17 => 7,
            19 => 8,
            15 => 9,
            18 => 6,
            _ => 10,
        }
    }

    pub(crate) fn ack_select(&mut self) {
        self.ack_flip = !self.ack_flip;
        let Some(id) = self.active_entity() else { return };
        let Some(e) = self.state.get(id) else { return };
        if e.owner != self.human {
            return;
        }
        let s = match e.kind {
            EntityKind::Building => Sfx::SelBuilding,
            EntityKind::Unit => {
                let d = &self.state.data.units[e.def as usize];
                if d.race == 1 {
                    if self.ack_flip { Sfx::SelKyth1 } else { Sfx::SelKyth2 }
                } else if d.race == 2 {
                    if self.ack_flip { Sfx::SelFer1 } else { Sfx::SelFer2 }
                } else if d.harvester {
                    Sfx::SelVcWorker
                } else if self.ack_flip {
                    Sfx::SelVc1
                } else {
                    Sfx::SelVc2
                }
            }
            EntityKind::Resource => return,
        };
        self.sfx(s);
    }

    /// Reject a player action with a flashing warning + error sound.
    pub(crate) fn deny(&mut self, msg: &str) {
        self.error_flash = Some((msg.to_string(), Instant::now()));
        self.sfx(Sfx::Error);
    }

    /// Distinct own types in the selection, units before buildings — the
    /// Tab-cycle order.
    pub(crate) fn selection_types(&self) -> Vec<(EntityKind, DefId)> {
        let mut v: Vec<(EntityKind, DefId)> = Vec::new();
        for id in &self.selection {
            if let Some(e) = self.state.get(*id) {
                if e.owner == self.human
                    && matches!(e.kind, EntityKind::Unit | EntityKind::Building)
                    && !v.contains(&(e.kind, e.def))
                {
                    v.push((e.kind, e.def));
                }
            }
        }
        v.sort_by_key(|(k, d)| (matches!(k, EntityKind::Building), *d));
        v
    }

    /// The Tab-active subgroup type.
    pub(crate) fn active_type(&self) -> Option<(EntityKind, DefId)> {
        let t = self.selection_types();
        if t.is_empty() {
            None
        } else {
            Some(t[self.subgroup_offset % t.len()])
        }
    }

    /// Representative entity: first selected of the active subgroup type,
    /// else the first selected thing at all (enemy/resource inspection).
    pub(crate) fn active_entity(&self) -> Option<EntityId> {
        if let Some((kind, def)) = self.active_type() {
            for id in &self.selection {
                if let Some(e) = self.state.get(*id) {
                    if e.kind == kind && e.def == def && e.owner == self.human {
                        return Some(*id);
                    }
                }
            }
        }
        self.selection.first().copied()
    }

    /// Center the camera on a world position, clamped to the map.
    pub fn look_at(&mut self, wx: f32, wy: f32) {
        let (ix, iy) = iso::world_to_iso(wx, wy);
        self.cam.cx = ix;
        self.cam.cy = iy;
        self.clamp_camera();
    }

    /// Keep the camera anchored to the map — no scrolling into the void.
    pub fn clamp_camera(&mut self) {
        let w = self.state.map.width as f32;
        let h = self.state.map.height as f32;
        let margin = 60.0;
        let min_x = -h * iso::TILE_HALF_W - margin;
        let max_x = w * iso::TILE_HALF_W + margin;
        let min_y = -margin;
        let max_y = (w + h) * iso::TILE_HALF_H + margin;
        self.cam.cx = self.cam.cx.clamp(min_x, max_x);
        self.cam.cy = self.cam.cy.clamp(min_y, max_y);
    }

    /// HUD/menu pixel scale: display DPI x user preference.
    pub fn ui(&self) -> f32 {
        self.base_scale * self.settings.hud_scale
    }

    /// Text scale helper (integer-ish multiples keep the 5x7 font crisp).
    pub fn ts(&self, m: f32) -> f32 {
        (self.ui() * m).round().max(1.0)
    }

    pub fn smoke_expired(&self) -> bool {
        self.smoke_deadline.is_some_and(|d| Instant::now() >= d)
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        self.gfx.resize(w, h);
        self.cam.screen_w = w as f32;
        self.cam.screen_h = h as f32;
    }

    pub fn start_game(&mut self, difficulty: Difficulty) {
        self.mp = None;
        self.mp_waiting = None;
        self.human = 0;
        self.replay = None;
        self.record_replay = true;
        let enemy = match self.enemy_race_choice {
            2 => {
                // Pre-game choice, not sim: wall clock is fine here.
                (crate::clock::SystemTime::now()
                    .duration_since(crate::clock::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos())
                    .unwrap_or(0)
                    % 2) as u8
            }
            r => r,
        };
        let names = all_map_names();
        self.game_map = names[self.map_choice % names.len()].clone();
        self.state = new_game_with(self.chosen_race, enemy, &self.game_map);
        let style = crate::clock::SystemTime::now()
            .duration_since(crate::clock::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        self.bot = Bot::with_style(BOT, difficulty, style);
        self.difficulty = difficulty;
        self.pending.clear();
        self.selection.clear();
        self.groups = Default::default();
        self.mode = Mode::Normal;
        self.effects.clear();
        self.facings.clear();
        self.headings.clear();
        self.subgroup_offset = 0;
        self.acc = 0.0;
        self.in_game = true;
        self.page = MenuPage::None;
        let start = self.state.map.starts[self.human as usize];
        let (cx, cy) = iso::world_to_iso(start.x as f32 + 0.5, start.y as f32 + 0.5);
        self.cam.cx = cx;
        self.cam.cy = cy;
        self.arm_countdown();
    }

    /// 5-second frozen countdown before the match starts (skipped for
    /// headless automation).
    fn arm_countdown(&mut self) {
        if self.mp_auto.is_none() && self.script.is_none() && self.shot.is_none() {
            self.countdown = Some((Instant::now(), 6));
        }
    }

    pub fn start_mp_game(&mut self, started: Started) {
        crate::weblog(&format!(
            "orion: start_mp_game local={} delay={}",
            started.local_player, started.input_delay
        ));
        self.mp_lobby_code = None;
        self.replay = None;
        self.record_replay = true;
        self.game_map = started.map.clone();
        self.state = new_game_mp(started.seed, started.races[0], started.races[1], &started.map);
        self.human = started.local_player;
        self.mp = Some(Lockstep::new(started.net, started.local_player, started.input_delay));
        self.mp_waiting = None;
        self.mp_error = None;
        self.pending.clear();
        self.selection.clear();
        self.groups = Default::default();
        self.mode = Mode::Normal;
        self.effects.clear();
        self.facings.clear();
        self.headings.clear();
        self.subgroup_offset = 0;
        self.acc = 0.0;
        self.in_game = true;
        self.page = MenuPage::None;
        let start = self.state.map.starts[self.human as usize];
        let (cx, cy) = iso::world_to_iso(start.x as f32 + 0.5, start.y as f32 + 0.5);
        self.cam.cx = cx;
        self.cam.cy = cy;
        self.clamp_camera();
        self.arm_countdown();
    }

    /// Tick the countdown; true while the sim should stay frozen.
    pub(crate) fn countdown_active(&mut self) -> bool {
        let Some((t0, last)) = self.countdown else { return false };
        let elapsed = t0.elapsed().as_secs_f32();
        let remaining = (5.0 - elapsed).ceil().max(0.0) as u32;
        if elapsed >= 5.0 {
            self.countdown = None;
            self.sfx(Sfx::CountGo);
            return false;
        }
        if remaining < last {
            self.countdown = Some((t0, remaining));
            self.sfx(Sfx::CountTick);
        }
        true
    }

    /// Shared camera input (keyboard pan + edge scroll).
    fn camera_input(&mut self, dt: f64) {
        let pan = 700.0 * dt as f32 / self.cam.zoom;
        if self.keys_down.contains(&KeyCode::ArrowLeft) {
            self.cam.cx -= pan;
        }
        if self.keys_down.contains(&KeyCode::ArrowRight) {
            self.cam.cx += pan;
        }
        if self.keys_down.contains(&KeyCode::ArrowUp) {
            self.cam.cy -= pan;
        }
        if self.keys_down.contains(&KeyCode::ArrowDown) {
            self.cam.cy += pan;
        }
        if self.settings.edge_scroll
            && self.cursor_in
            && self.panning.is_none()
            && self.page == MenuPage::None
        {
            let m = 10.0 * self.base_scale;
            let es = 900.0 * dt as f32 / self.cam.zoom;
            if self.mouse.0 <= m {
                self.cam.cx -= es;
            }
            if self.mouse.0 >= self.cam.screen_w - m {
                self.cam.cx += es;
            }
            if self.mouse.1 <= m {
                self.cam.cy -= es;
            }
            if self.mouse.1 >= self.cam.screen_h - m {
                self.cam.cy += es;
            }
        }
    }

    /// Start watching a replay from disk.
    pub fn start_replay(&mut self, path: &std::path::Path) {
        let Ok(src) = std::fs::read_to_string(path) else {
            self.mp_error = Some("cannot read replay".into());
            return;
        };
        let replay = match orion_sim::replay::Replay::from_ron(&src) {
            Ok(r) => r,
            Err(e) => {
                self.mp_error = Some(format!("bad replay: {e}"));
                return;
            }
        };
        let Some(state) = replay.start_state(GameData::load_default()) else {
            self.mp_error = Some(format!("unknown map '{}'", replay.map));
            return;
        };
        self.state = state;
        self.replay = Some(replay);
        self.replay_cursor = 0;
        self.replay_paused = false;
        self.replay_speed = 1.0;
        self.replay_view = 2;
        self.record_replay = false;
        self.mp = None;
        self.mp_waiting = None;
        self.human = 0;
        self.reveal_all = true;
        self.pending.clear();
        self.selection.clear();
        self.groups = Default::default();
        self.mode = Mode::Normal;
        self.effects.clear();
        self.facings.clear();
        self.headings.clear();
        self.subgroup_offset = 0;
        self.acc = 0.0;
        self.in_game = true;
        self.page = MenuPage::None;
        let start = self.state.map.starts[0];
        let (cx, cy) = iso::world_to_iso(start.x as f32 + 0.5, start.y as f32 + 0.5);
        self.cam.cx = cx;
        self.cam.cy = cy;
    }

    /// Auto-save the current game's replay once (game end or quit).
    /// Upload replay `k` from the list to the relay vault (native only).
    pub fn share_replay(&mut self, k: usize) {
        self.replay_share_mode = false;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some((_, path)) = self.replay_files.get(k) else { return };
            match std::fs::read_to_string(path) {
                Ok(ron) => {
                    self.replay_status = Some("UPLOADING...".into());
                    self.replay_share_rx = Some(crate::relay::share_replay_async(
                        self.settings.relay_url.clone(),
                        ron,
                    ));
                }
                Err(e) => self.replay_status = Some(format!("READ FAILED: {e}")),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = k;
        }
    }

    /// Download the replay behind `replay_code` into the local list.
    pub fn fetch_shared_replay(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let code = self.replay_code.trim().to_uppercase();
            if code.is_empty() || self.replay_fetch_rx.is_some() {
                return;
            }
            self.replay_status = Some(format!("FETCHING {code}..."));
            self.replay_fetch_rx = Some(crate::relay::fetch_replay_async(
                self.settings.relay_url.clone(),
                code,
            ));
        }
    }

    /// Poll the async share/fetch results (menu frame).
    #[cfg(not(target_arch = "wasm32"))]
    fn poll_replay_net(&mut self) {
        if let Some(rx) = &self.replay_share_rx {
            if let Ok(res) = rx.try_recv() {
                self.replay_status = Some(match res {
                    Ok(code) => format!("REPLAY CODE: {} - SHARE IT", code.trim()),
                    Err(e) => format!("SHARE FAILED: {e}").to_uppercase(),
                });
                self.replay_share_rx = None;
            }
        }
        if let Some(rx) = &self.replay_fetch_rx {
            if let Ok(res) = rx.try_recv() {
                match res {
                    Ok(ron) => {
                        let code = self.replay_code.trim().to_uppercase();
                        if orion_sim::replay::Replay::from_ron(&ron).is_err() {
                            self.replay_status =
                                Some("FETCH FAILED: NOT A VALID REPLAY".into());
                        } else {
                            let dir = crate::replays::dir();
                            let _ = std::fs::create_dir_all(&dir);
                            let path = dir.join(format!("shared-{code}.ron"));
                            match std::fs::write(&path, &ron) {
                                Ok(()) => {
                                    self.replay_files =
                                        crate::replays::list(&self.state.data.race_names);
                                    self.replay_status =
                                        Some(format!("{code} FETCHED - IT IS IN THE LIST"));
                                    self.replay_code.clear();
                                }
                                Err(e) => {
                                    self.replay_status =
                                        Some(format!("SAVE FAILED: {e}").to_uppercase())
                                }
                            }
                        }
                    }
                    Err(e) => {
                        self.replay_status = Some(format!("FETCH FAILED: {e}").to_uppercase())
                    }
                }
                self.replay_fetch_rx = None;
            }
        }
    }

    pub fn save_replay(&mut self) {
        if !self.record_replay || self.state.tick < 24 * 20 {
            return;
        }
        self.record_replay = false;
        let me = self.settings.player_name.clone();
        let names = if self.mp.is_some() {
            if self.human == 0 { vec![me, "OPPONENT".into()] } else { vec!["OPPONENT".into(), me] }
        } else {
            vec![me, "BOT".into()]
        };
        let replay =
            orion_sim::replay::Replay::from_state(&self.state, &self.game_map.clone(), names);
        if let Err(e) = crate::replays::save(&replay) {
            eprintln!("replay save failed: {e}");
        }
    }

    /// Replay frame: step recorded commands on the sim clock; the viewer
    /// only controls camera, speed, pause, and fog perspective.
    fn replay_frame(&mut self, dt: f64) {
        self.camera_input(dt);
        let Some(replay) = &self.replay else { return };
        let done = self.state.tick >= replay.duration_ticks;
        if !self.replay_paused && !done && self.page == MenuPage::None {
            self.acc += dt * self.replay_speed as f64;
            while self.acc >= TICK_DT {
                let replay = self.replay.as_ref().unwrap();
                if self.state.tick >= replay.duration_ticks {
                    break;
                }
                let cmds = replay.commands_for(self.state.tick, &mut self.replay_cursor);
                self.pending.clear(); // viewers don't command anyone
                self.step_sim(cmds);
                self.acc -= TICK_DT;
            }
        }
        for e in &mut self.effects {
            e.age += dt as f32;
        }
        self.effects.retain(|e| e.age < e.ttl);
        for r in &mut self.recoil {
            *r = (*r - dt as f32).max(0.0);
        }
    }

    /// Keep a lockstep match alive without rendering — called while the
    /// page is hidden in the browser and requestAnimationFrame is stopped.
    /// Uses its own clock so time hidden isn't double-counted when the
    /// normal frame path resumes.
    pub fn background_step(&mut self) {
        if !self.in_game || self.mp.is_none() || self.replay.is_some() {
            self.bg_last = None;
            return;
        }
        let now = Instant::now();
        let dt = self
            .bg_last
            .map(|t| (now - t).as_secs_f64())
            .unwrap_or(0.25)
            .min(0.5);
        self.bg_last = Some(now);
        self.mp_frame(dt);
    }

    /// Multiplayer frame: lockstep-driven stepping, menus never pause.
    fn mp_frame(&mut self, dt: f64) {
        self.camera_input(dt);
        self.dbg_frames = self.dbg_frames.wrapping_add(1);
        if self.dbg_frames % 120 == 0
            && (self.mp_auto.is_some() || cfg!(target_arch = "wasm32"))
        {
            crate::weblog(&format!(
                "mp hb: tick={} cd={:?} acc={:.3} stalls={}",
                self.state.tick,
                self.countdown.as_ref().map(|(t0, n)| (t0.elapsed().as_secs_f32(), *n)),
                self.acc,
                self.mp.as_ref().map_or(0, |m| m.stalls_per_min()),
            ));
        }
        if self.countdown_active() {
            self.acc = 0.0;
        } else if self.state.winner.is_none() {
            self.acc += dt; // speed is locked to 1.0 in multiplayer
            let mut steps = 0;
            while self.acc >= TICK_DT && steps < 8 {
                let mut pend: Vec<Command> =
                    self.pending.drain(..).map(|(_, c)| c).collect();
                let mut mp = self.mp.take().unwrap();
                let stepped = mp.try_step(&mut self.state, &mut pend);
                self.mp = Some(mp);
                if !pend.is_empty() {
                    let h = self.human;
                    self.pending.extend(pend.into_iter().map(|c| (h, c)));
                }
                if stepped {
                    self.step_post();
                    self.acc -= TICK_DT;
                    steps += 1;
                } else {
                    // Waiting on the peer: don't accumulate a debt.
                    self.acc = self.acc.min(TICK_DT);
                    break;
                }
            }
        }
        for e in &mut self.effects {
            e.age += dt as f32;
        }
        self.effects.retain(|e| e.age < e.ttl);
        for r in &mut self.recoil {
            *r = (*r - dt as f32).max(0.0);
        }
    }

    // ---------------------------------------------------------- events ----

    pub fn handle_event(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let (nx, ny) = (position.x as f32, position.y as f32);
                if let Some((px, py)) = self.panning {
                    self.cam.cx -= (nx - px) / self.cam.zoom;
                    self.cam.cy -= (ny - py) / self.cam.zoom;
                    self.panning = Some((nx, ny));
                }
                self.mouse = (nx, ny);
                self.cursor_in = true;
            }
            WindowEvent::CursorEntered { .. } => self.cursor_in = true,
            WindowEvent::CursorLeft { .. } => self.cursor_in = false,
            WindowEvent::MouseWheel { delta, .. } => {
                if self.page == MenuPage::None {
                    let dy = match delta {
                        MouseScrollDelta::LineDelta(_, y) => *y,
                        MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                    };
                    // Zoom anchored on the cursor: the world point under the
                    // mouse stays under the mouse.
                    let (wx, wy) = self.cam.screen_to_world(self.mouse.0, self.mouse.1);
                    self.cam.zoom =
                        (self.cam.zoom * (1.0 + dy * 0.1)).clamp(1.0, 4.0 * self.base_scale);
                    let (ix, iy) = iso::world_to_iso(wx, wy);
                    self.cam.cx = ix - (self.mouse.0 - self.cam.screen_w * 0.5) / self.cam.zoom;
                    self.cam.cy = iy - (self.mouse.1 - self.cam.screen_h * 0.5) / self.cam.zoom;
                    self.clamp_camera();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let down = *state == ElementState::Pressed;
                match button {
                    MouseButton::Left => self.left_button(down),
                    MouseButton::Right => {
                        if down && self.page == MenuPage::None && self.in_game {
                            self.right_click();
                        }
                    }
                    MouseButton::Middle => {
                        self.panning =
                            (down && self.page == MenuPage::None).then_some(self.mouse);
                    }
                    _ => {}
                }
            }
            WindowEvent::ModifiersChanged(m) => {
                self.shift = m.state().shift_key();
                self.ctrl = m.state().control_key() || m.state().super_key();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                // Name / lobby-code entry on the multiplayer page.
                if self.page == MenuPage::Multiplayer
                    && self.mp_waiting.is_none()
                    && event.state == ElementState::Pressed
                {
                    if event.physical_key == PhysicalKey::Code(KeyCode::Backspace) {
                        if self.name_focus {
                            self.settings.player_name.pop();
                        } else {
                            self.code_input.pop();
                        }
                        return;
                    }
                    // Enter in the code field = join that private lobby.
                    if event.physical_key == PhysicalKey::Code(KeyCode::Enter)
                        && !self.name_focus
                        && !self.code_input.trim().is_empty()
                    {
                        self.join_private_lobby();
                        return;
                    }
                    if let Some(text) = &event.text {
                        let mut used = false;
                        for ch in text.chars() {
                            if self.name_focus {
                                if (ch.is_ascii_alphanumeric() || ch == ' ')
                                    && self.settings.player_name.len() < 14
                                {
                                    self.settings.player_name.push(ch.to_ascii_uppercase());
                                    used = true;
                                }
                            } else if ch.is_ascii_alphanumeric() && self.code_input.len() < 8 {
                                self.code_input.push(ch.to_ascii_uppercase());
                                used = true;
                            }
                        }
                        if used {
                            return;
                        }
                    }
                }
                // Fetch-code entry on the replays page.
                if self.page == MenuPage::Replays
                    && event.state == ElementState::Pressed
                {
                    if event.physical_key == PhysicalKey::Code(KeyCode::Backspace) {
                        self.replay_code.pop();
                        return;
                    }
                    if event.physical_key == PhysicalKey::Code(KeyCode::Enter)
                        && !self.replay_code.trim().is_empty()
                    {
                        self.fetch_shared_replay();
                        return;
                    }
                    if let Some(text) = &event.text {
                        let mut used = false;
                        for ch in text.chars() {
                            if ch.is_ascii_alphanumeric() && self.replay_code.len() < 8 {
                                self.replay_code.push(ch.to_ascii_uppercase());
                                used = true;
                            }
                        }
                        if used {
                            return;
                        }
                    }
                }
                if let PhysicalKey::Code(code) = event.physical_key {
                    if event.state == ElementState::Pressed {
                        self.keys_down.insert(code);
                        if !event.repeat {
                            self.key_pressed(code);
                        }
                    } else {
                        self.keys_down.remove(&code);
                    }
                }
            }
            _ => {}
        }
    }

    fn key_pressed(&mut self, code: KeyCode) {
        if self.editor.is_some() && self.page == MenuPage::None && self.editor_key(code) {
            return;
        }
        // Rebind capture takes everything except Escape.
        if let Some(action) = self.rebinding {
            if code != KeyCode::Escape && crate::config::BINDABLE_KEYS.contains(&code) {
                self.settings.set_key(action, code);
                self.settings.save();
            }
            self.rebinding = None;
            return;
        }

        if code == KeyCode::Escape {
            self.escape_pressed();
            return;
        }

        if self.page != MenuPage::None || !self.in_game {
            return;
        }

        // Replay viewer controls swallow the conflicting game keys.
        if self.replay.is_some() {
            match code {
                KeyCode::Space => {
                    self.replay_paused = !self.replay_paused;
                    return;
                }
                KeyCode::Tab => {
                    self.replay_view = (self.replay_view + 1) % 3;
                    if self.replay_view == 2 {
                        self.reveal_all = true;
                    } else {
                        self.reveal_all = false;
                        self.human = self.replay_view;
                    }
                    return;
                }
                KeyCode::Digit1 => {
                    self.replay_speed = 1.0;
                    return;
                }
                KeyCode::Digit2 => {
                    self.replay_speed = 2.0;
                    return;
                }
                KeyCode::Digit3 => {
                    self.replay_speed = 4.0;
                    return;
                }
                _ => {}
            }
        }

        // Restart after game over (single player only — in MP it would
        // silently abandon the session into a bot game).
        if code == KeyCode::KeyR
            && self.state.winner.is_some()
            && self.replay.is_none()
            && self.mp.is_none()
        {
            let d = self.difficulty;
            self.start_game(d);
            return;
        }

        // Digits: control groups. Ctrl sets, Shift adds, plain recalls,
        // double-tap jumps the camera to the group.
        if let Some(d) = digit_of(code) {
            if self.ctrl {
                self.groups[d as usize] = self.selection.clone();
                return;
            }
            if self.shift {
                let sel = self.selection.clone();
                for id in sel {
                    if !self.groups[d as usize].contains(&id) {
                        self.groups[d as usize].push(id);
                    }
                }
                return;
            }
            let live: Vec<_> = self.groups[d as usize]
                .clone()
                .into_iter()
                .filter(|id| self.state.get(*id).is_some())
                .collect();
            if !live.is_empty() {
                let double = self
                    .last_group_tap
                    .is_some_and(|(g, t)| g == d && t.elapsed().as_millis() < 400);
                self.subgroup_offset = 0;
                if double {
                    // Second tap: center camera on the group.
                    let mut cx = 0.0;
                    let mut cy = 0.0;
                    for id in &live {
                        let e = &self.state.entities[id.idx as usize];
                        cx += e.pos.x.to_f32();
                        cy += e.pos.y.to_f32();
                    }
                    let n = live.len() as f32;
                    self.look_at(cx / n, cy / n);
                }
                self.selection = live;
                self.last_group_tap = Some((d, Instant::now()));
                return;
            }
        }

        // Hero ability hotkeys (F/G) — fixed keys, mutable context.
        if self.selected_hero().is_some() {
            if code == KeyCode::KeyF {
                self.use_hero_ability(0);
                return;
            } else if code == KeyCode::KeyG {
                self.use_hero_ability(1);
                return;
            }
        }
        let Some(action) = self.action_for_context(code) else { return };
        self.run_action(action);
    }

    /// Bind resolution with context priority: the same key can place a
    /// building in the build menu and train a unit in a production building
    /// (SC2 grid layout).
    fn action_for_context(&self, code: KeyCode) -> Option<Action> {
        if self.mode == Mode::BuildMenu {
            for a in [
                Action::Place1,
                Action::Place2,
                Action::Place3,
                Action::Place4,
                Action::Place5,
                Action::Place6,
                Action::Place7,
            ] {
                if self.settings.key_for(a) == code {
                    return Some(a);
                }
            }
        }
        if self.single_own_building().is_some() {
            for a in [Action::Train0, Action::Train1, Action::Train2] {
                if self.settings.key_for(a) == code {
                    return Some(a);
                }
            }
        }
        // Unit-context abilities take priority over generic fallbacks.
        if (self.any_selected_siege() || self.any_selected_burrow())
            && self.settings.key_for(Action::SiegeToggle) == code
        {
            return Some(Action::SiegeToggle);
        }
        if self.any_selected_caster() && self.settings.key_for(Action::CastStorm) == code {
            return Some(Action::CastStorm);
        }

        self.settings.action_for(code)
    }

    /// The human race's buildable structures, in data order — the build
    /// menu grid, mapped onto Place1..Place7.
    pub(crate) fn build_menu_defs(&self) -> Vec<DefId> {
        let race = self.state.players[self.human as usize].race;
        (0..self.state.data.buildings.len() as DefId)
            .filter(|&d| self.state.data.buildings[d as usize].race == race)
            .collect()
    }

    pub(crate) fn any_selected_siege(&self) -> bool {
        self.own_selected_units().any(|id| {
            let e = &self.state.entities[id.idx as usize];
            let d = &self.state.data.units[e.def as usize];
            d.weapon_siege.is_some() || d.shield_aura.is_some()
        })
    }

    /// The selected hero (if any) — abilities key off it.
    pub(crate) fn selected_hero(&self) -> Option<orion_sim::EntityId> {
        self.own_selected_units().find(|id| {
            let e = &self.state.entities[id.idx as usize];
            self.state.data.units[e.def as usize].hero
        })
    }

    /// Fire or arm hero ability `slot` for the selected hero.
    pub(crate) fn use_hero_ability(&mut self, slot: u8) {
        let Some(id) = self.selected_hero() else { return };
        let e = &self.state.entities[id.idx as usize];
        let tag = self.state.data.units[e.def as usize].tag.clone();
        let Some(spec) = orion_sim::hero::ability(&tag, slot) else { return };
        if e.energy < spec.cost {
            self.deny("NOT ENOUGH ENERGY");
            return;
        }
        if spec.cast_range.0 == 0 {
            let target = e.pos;
            self.pending.push((
                self.human,
                Command::UseAbility { caster: id, slot, target },
            ));
        } else {
            self.mode = Mode::AbilityTarget(slot);
        }
    }

    pub(crate) fn any_selected_burrow(&self) -> bool {
        self.own_selected_units().any(|id| {
            let e = &self.state.entities[id.idx as usize];
            self.state.data.units[e.def as usize].burrow
        })
    }

    pub(crate) fn any_selected_caster(&self) -> bool {
        self.own_selected_units().any(|id| {
            let e = &self.state.entities[id.idx as usize];
            self.state.data.units[e.def as usize].energy_max > 0
        })
    }

    fn escape_pressed(&mut self) {
        match self.page {
            MenuPage::None => {
                if self.mode != Mode::Normal {
                    self.mode = Mode::Normal;
                } else if self.in_game {
                    self.page = MenuPage::EscRoot;
                }
            }
            MenuPage::Settings { from_game } => {
                self.settings.save();
                self.page = if from_game { MenuPage::EscRoot } else { MenuPage::MainRoot };
            }
            MenuPage::EscRoot => self.page = MenuPage::None,
            MenuPage::Difficulty
            | MenuPage::Multiplayer
            | MenuPage::Replays
            | MenuPage::UpdatePrompt => self.page = MenuPage::MainRoot,
            MenuPage::Ladder => self.page = MenuPage::Multiplayer,
            MenuPage::MainRoot => {}
        }
    }

    pub(crate) fn run_action(&mut self, action: Action) {
        match action {
            Action::Attack => {
                if self.own_selected_units().next().is_some() {
                    self.mode = Mode::AttackMove;
                }
            }
            Action::Stop => {
                let units: Vec<_> = self.own_selected_units().collect();
                if !units.is_empty() {
                    self.pending.push((self.human, Command::Stop { units }));
                }
                self.mode = Mode::Normal;
            }
            Action::Hold => {
                let units: Vec<_> = self.own_selected_units().collect();
                if !units.is_empty() {
                    self.pending.push((self.human, Command::Hold { units }));
                }
                self.mode = Mode::Normal;
            }
            Action::BuildMenu => {
                if self.selected_builder().is_some() {
                    self.mode = Mode::BuildMenu;
                }
            }
            Action::Train0 => self.train_hotkey(0),
            Action::Train1 => self.train_hotkey(1),
            Action::Place1
            | Action::Place2
            | Action::Place3
            | Action::Place4
            | Action::Place5
            | Action::Place6
            | Action::Place7 => {
                if self.mode == Mode::BuildMenu {
                    let slot = match action {
                        Action::Place1 => 0,
                        Action::Place2 => 1,
                        Action::Place3 => 2,
                        Action::Place4 => 3,
                        Action::Place5 => 4,
                        Action::Place6 => 5,
                        _ => 6,
                    };
                    let Some(&def) = self.build_menu_defs().get(slot) else { return };
                    // Only enter placement if the tech requirement is met.
                    let requires = self.state.data.buildings[def as usize].requires;
                    if self.state.requirement_met(self.human, requires) {
                        self.mode = Mode::Placing(def);
                    } else {
                        let req =
                            self.state.data.buildings[requires.unwrap() as usize].name.clone();
                        self.deny(&format!("REQUIRES {}", req.to_uppercase()));
                    }
                }
            }
            Action::Train2 => self.train_hotkey(2),
            Action::SiegeToggle => {
                let deployers: Vec<_> = self
                    .own_selected_units()
                    .filter(|id| {
                        let e = &self.state.entities[id.idx as usize];
                        let d = &self.state.data.units[e.def as usize];
                        d.weapon_siege.is_some() || d.shield_aura.is_some()
                    })
                    .collect();
                if !deployers.is_empty() {
                    self.pending.push((self.human, Command::Siege { units: deployers }));
                }
                let diggers: Vec<_> = self
                    .own_selected_units()
                    .filter(|id| {
                        let e = &self.state.entities[id.idx as usize];
                        self.state.data.units[e.def as usize].burrow
                    })
                    .collect();
                if !diggers.is_empty() {
                    self.pending.push((self.human, Command::Burrow { units: diggers }));
                }
            }
            Action::CastStorm => {
                if self.any_selected_caster() {
                    let has_energy = self.own_selected_units().any(|id| {
                        let e = &self.state.entities[id.idx as usize];
                        self.state.data.units[e.def as usize].energy_max > 0
                            && e.energy >= orion_sim::STORM_COST
                    });
                    if has_energy {
                        self.mode = Mode::CastTarget;
                    } else {
                        self.deny("NOT ENOUGH ENERGY");
                    }
                }
            }
            Action::IdleWorker => {
                let idle = (0..self.state.entities.len() as u32).find(|&i| {
                    let e = &self.state.entities[i as usize];
                    e.alive
                        && e.owner == self.human
                        && e.kind == EntityKind::Unit
                        && self.state.data.units[e.def as usize].harvester
                        && matches!(e.order, Order::Idle)
                });
                if let Some(i) = idle {
                    self.selection = vec![self.state.id_of(i)];
                    self.subgroup_offset = 0;
                    let e = &self.state.entities[i as usize];
                    let (px, py) = (e.pos.x.to_f32(), e.pos.y.to_f32());
                    self.look_at(px, py);
                }
            }
            Action::JumpAlert => {
                if let Some((x, y)) = self.last_alert {
                    self.look_at(x, y);
                }
            }
            Action::CycleBase => {
                let hqs: Vec<u32> = (0..self.state.entities.len() as u32)
                    .filter(|&i| {
                        let e = &self.state.entities[i as usize];
                        e.alive
                            && e.owner == self.human
                            && e.kind == EntityKind::Building
                            && self.state.data.buildings[e.def as usize].headquarters
                    })
                    .collect();
                if !hqs.is_empty() {
                    let i = hqs[self.base_cycle % hqs.len()];
                    self.base_cycle += 1;
                    let e = &self.state.entities[i as usize];
                    let (px, py) = (e.pos.x.to_f32(), e.pos.y.to_f32());
                    self.look_at(px, py);
                }
            }
            Action::CycleSubgroup => {
                let n = self.selection_types().len();
                if n > 0 {
                    self.subgroup_offset = (self.subgroup_offset + 1) % n;
                }
            }
            Action::CancelConstruction => {
                if let Some(id) = self.primary_selected_building() {
                    let e = &self.state.entities[id.idx as usize];
                    if e.construction.is_some() {
                        self.pending
                            .push((self.human, Command::CancelConstruction { building: id }));
                    }
                }
            }
            Action::RevealFog => self.reveal_all = !self.reveal_all,
        }
    }

    fn left_button(&mut self, down: bool) {
        if self.editor.is_some() && self.page == MenuPage::None {
            if down {
                self.editor_click();
            }
            self.editor_dragging = down;
            return;
        }
        if self.page != MenuPage::None {
            if down {
                self.sfx(Sfx::Click);
                self.menu_click();
            }
            return;
        }
        if !self.in_game {
            return;
        }
        if down {
            // Idle-worker chip.
            let (bx, by, bw, bh) = self.idle_badge_rect();
            if self.mouse.0 >= bx
                && self.mouse.0 <= bx + bw
                && self.mouse.1 >= by
                && self.mouse.1 <= by + bh
            {
                self.run_action(crate::config::Action::IdleWorker);
                return;
            }
            // Restart shortcut on the end-game banner.
            if self.state.winner.is_some() {
                return;
            }
            // Console region: minimap, command card, production queue.
            if self.mouse.1 >= self.cam.screen_h - self.console_h() {
                self.console_click();
                return;
            }
            match self.mode {
                Mode::AttackMove => {
                    let (wx, wy) = self.cam.screen_to_world(self.mouse.0, self.mouse.1);
                    let units: Vec<_> = self.own_selected_units().collect();
                    if !units.is_empty() {
                        self.pending.push((
                            self.human,
                            Command::AttackMove {
                                units,
                                target: fx(wx, wy),
                                queued: self.shift,
                            },
                        ));
                        self.ping(wx, wy, 1);
                        self.sfx(Sfx::AckAttack);
                    }
                    self.mode = Mode::Normal;
                }
                Mode::AbilityTarget(slot) => {
                    let (wx, wy) = self.cam.screen_to_world(self.mouse.0, self.mouse.1);
                    if let Some(caster) = self.selected_hero() {
                        self.pending.push((
                            self.human,
                            Command::UseAbility { caster, slot, target: fx(wx, wy) },
                        ));
                        self.sfx(Sfx::AckAttack);
                    }
                    self.mode = Mode::Normal;
                }
                Mode::CastTarget => {
                    let (wx, wy) = self.cam.screen_to_world(self.mouse.0, self.mouse.1);
                    // Cast from the selected caster with the most energy.
                    let caster = self
                        .own_selected_units()
                        .filter(|id| {
                            let e = &self.state.entities[id.idx as usize];
                            self.state.data.units[e.def as usize].energy_max > 0
                        })
                        .max_by_key(|id| {
                            (self.state.entities[id.idx as usize].energy, id.idx)
                        });
                    if let Some(caster) = caster {
                        self.pending
                            .push((self.human, Command::Cast { caster, target: fx(wx, wy) }));
                        self.ping(wx, wy, 1);
                    }
                    self.mode = Mode::Normal;
                }
                Mode::Placing(def) => {
                    self.try_place(def);
                    if !self.shift {
                        self.mode = Mode::Normal;
                    }
                }
                _ => self.select_start = Some(self.mouse),
            }
        } else if let Some(start) = self.select_start.take() {
            self.finish_selection(start, self.mouse);
        }
    }

    fn finish_selection(&mut self, a: (f32, f32), b: (f32, f32)) {
        let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
        let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
        let is_click = (x1 - x0) < 6.0 && (y1 - y0) < 6.0;

        let mut picked: Vec<EntityId> = Vec::new();
        if is_click {
            // Double-click detection (same spot within 400ms).
            let double = self
                .last_click
                .is_some_and(|(t, p)| {
                    t.elapsed().as_millis() < 400
                        && (p.0 - b.0).abs() < 8.0
                        && (p.1 - b.1).abs() < 8.0
                });
            self.last_click = Some((Instant::now(), b));
            if let Some(idx) = self.entity_under_cursor(b.0, b.1) {
                // Ctrl+click or double-click: select all own units OR
                // buildings of the same type on screen (SC2 behavior).
                let e = &self.state.entities[idx as usize];
                if (self.ctrl || double)
                    && e.owner == self.human
                    && (e.kind == EntityKind::Unit || e.kind == EntityKind::Building)
                {
                    let def = e.def;
                    let kind = e.kind;
                    for i in 0..self.state.entities.len() {
                        let o = &self.state.entities[i];
                        if o.alive && o.owner == self.human && o.kind == kind && o.def == def {
                            let (sx, sy) = self.entity_screen_pos(i);
                            if sx >= 0.0
                                && sx <= self.cam.screen_w
                                && sy >= 0.0
                                && sy <= self.cam.screen_h
                            {
                                picked.push(self.state.id_of(i as u32));
                            }
                        }
                    }
                } else if self.shift {
                    // Shift+click toggles a single unit in/out of selection.
                    let id = self.state.id_of(idx);
                    if let Some(pos) = self.selection.iter().position(|s| *s == id) {
                        self.selection.remove(pos);
                        return;
                    }
                    picked.push(id);
                } else {
                    picked.push(self.state.id_of(idx));
                }
            }
        } else {
            // Box select: units win when both are inside (SC2 rule), but a
            // box over buildings alone selects ALL of them.
            let mut buildings: Vec<EntityId> = Vec::new();
            for i in 0..self.state.entities.len() {
                let e = &self.state.entities[i];
                if !e.alive
                    || e.owner != self.human
                    || !matches!(e.kind, EntityKind::Unit | EntityKind::Building)
                {
                    continue;
                }
                let kind = e.kind;
                let (sx, sy) = self.entity_screen_pos(i);
                if sx >= x0 && sx <= x1 && sy >= y0 && sy <= y1 {
                    if kind == EntityKind::Unit {
                        picked.push(self.state.id_of(i as u32));
                    } else {
                        buildings.push(self.state.id_of(i as u32));
                    }
                }
            }
            if picked.is_empty() {
                picked = buildings;
            }
        }
        if picked.is_empty() && !is_click {
            return;
        }
        let had_pick = !picked.is_empty();
        if self.shift {
            for id in picked {
                if !self.selection.contains(&id) {
                    self.selection.push(id);
                }
            }
        } else {
            self.selection = picked;
        }
        self.subgroup_offset = 0;
        if had_pick {
            self.ack_select();
        }
    }

    fn right_click(&mut self) {
        let target_world = self.minimap_pick(self.mouse.0, self.mouse.1);
        if target_world.is_none() && self.mouse.1 >= self.cam.screen_h - self.console_h() {
            return; // right-click elsewhere on the console: ignore
        }
        let (wx, wy) =
            target_world.unwrap_or_else(|| self.cam.screen_to_world(self.mouse.0, self.mouse.1));
        self.mode = Mode::Normal;

        // Own production building(s) selected (and no units) -> set rally
        // on all of them.
        if self.own_selected_units().next().is_none() {
            let ralliers: Vec<_> = self
                .selected_own_buildings()
                .into_iter()
                .filter(|id| {
                    let b = &self.state.entities[id.idx as usize];
                    !self.state.data.buildings[b.def as usize].trains.is_empty()
                })
                .collect();
            if !ralliers.is_empty() {
                for bid in ralliers {
                    self.pending
                        .push((self.human, Command::SetRally { building: bid, target: fx(wx, wy) }));
                }
                self.ping(wx, wy, 2);
                return;
            }
        }

        let units: Vec<_> = self.own_selected_units().collect();
        if units.is_empty() {
            return;
        }
        let queued = self.shift;

        let mut under = if target_world.is_none() {
            self.entity_under_cursor(self.mouse.0, self.mouse.1)
        } else {
            None
        };
        // Sprite pick missed but the tile holds a mineral patch: gather.
        if under.is_none() && target_world.is_none() {
            let t = TilePos::new(wx.floor() as i32, wy.floor() as i32);
            under = (0..self.state.entities.len() as u32).find(|&i| {
                let e = &self.state.entities[i as usize];
                e.alive
                    && e.kind == EntityKind::Resource
                    && e.def == RES_MINERALS
                    && TilePos::of(e.pos) == t
            });
        }

        if let Some(t) = under {
            let e = &self.state.entities[t as usize];
            let tid = self.state.id_of(t);
            let gatherable = self.state.gatherable(self.human, tid);
            if gatherable {
                let (harv, rest): (Vec<_>, Vec<_>) = units.into_iter().partition(|id| {
                    let u = &self.state.entities[id.idx as usize];
                    self.state.data.units[u.def as usize].harvester
                });
                if !harv.is_empty() {
                    self.pending.push((self.human, Command::Gather {
                        units: harv,
                        resource: tid,
                        queued,
                    }));
                    self.ping(wx, wy, 2);
                    self.sfx(Sfx::AckGather);
                }
                if !rest.is_empty() {
                    self.pending.push((self.human, Command::Move {
                        units: rest,
                        target: fx(wx, wy),
                        queued,
                    }));
                }
                return;
            }
            if e.owner != self.human && e.owner != NEUTRAL {
                self.pending.push((self.human, Command::AttackTarget { units, target: tid }));
                self.ping(wx, wy, 1);
                self.sfx(Sfx::AckAttack);
                return;
            }
            // Destructibles: right-click orders armed units to clear them.
            if e.kind == EntityKind::Resource
                && (e.def == orion_sim::state::RES_TREE
                    || e.def == orion_sim::state::RES_ROCK)
            {
                let armed: Vec<_> = units
                    .iter()
                    .copied()
                    .filter(|id| {
                        let u = &self.state.entities[id.idx as usize];
                        self.state.data.units[u.def as usize].weapon.is_some()
                    })
                    .collect();
                if !armed.is_empty() {
                    self.pending
                        .push((self.human, Command::AttackTarget { units: armed, target: tid }));
                    self.ping(wx, wy, 1);
                    self.sfx(Sfx::AckAttack);
                    return;
                }
            }
            // Right-click own unfinished building with a builder: resume it.
            if e.owner == self.human && e.kind == EntityKind::Building && e.construction.is_some() {
                let def = e.def;
                let site = self.state.footprint_origin(def, e.pos);
                if let Some(builder) = units.iter().copied().find(|id| {
                    let u = &self.state.entities[id.idx as usize];
                    self.state.data.units[u.def as usize].builder
                }) {
                    self.pending
                        .push((self.human, Command::Build { worker: builder, building: def, site, queued }));
                    let rest: Vec<_> = units.into_iter().filter(|id| *id != builder).collect();
                    if !rest.is_empty() {
                        self.pending.push((self.human, Command::Move {
                            units: rest,
                            target: fx(wx, wy),
                            queued,
                        }));
                    }
                    self.ping(wx, wy, 2);
                    self.sfx(Sfx::AckBuild);
                    return;
                }
            }
        }
        self.pending.push((self.human, Command::Move { units, target: fx(wx, wy), queued }));
        self.ping(wx, wy, 0);
        self.ack_flip = !self.ack_flip;
        self.sfx(if self.ack_flip { Sfx::AckMove1 } else { Sfx::AckMove2 });
    }

    pub(crate) fn ping(&mut self, wx: f32, wy: f32, kind: i32) {
        self.effects.push(Effect {
            kind: EffKind::Ping,
            ax: wx,
            ay: wy,
            bx: kind as f32,
            by: 0.0,
            age: 0.0,
            ttl: 0.5, style: 0,
        });
    }

    /// Build orders a worker already has: current + shift-queued + commands
    /// issued this frame but not yet stepped. Used to spread placements
    /// across the selection instead of piling them on one worker.
    fn pending_builds_of(&self, worker: EntityId) -> usize {
        let e = &self.state.entities[worker.idx as usize];
        let in_orders = std::iter::once(&e.order)
            .chain(e.order_queue.iter())
            .filter(|o| matches!(o, Order::Build { .. }))
            .count();
        let in_pending = self
            .pending
            .iter()
            .filter(|(_, c)| matches!(c, Command::Build { worker: w, .. } if *w == worker))
            .count();
        in_orders + in_pending
    }

    fn try_place(&mut self, def: DefId) {
        // Spread builds across selected workers: each placement goes to the
        // builder with the fewest build orders (nearest on ties), so 3
        // queued buildings with 5 workers selected = 3 workers peel off and
        // the other 2 keep doing what they were doing.
        let site = self.hovered_site(def);
        let builders: Vec<EntityId> = self
            .own_selected_units()
            .filter(|id| {
                let e = &self.state.entities[id.idx as usize];
                self.state.data.units[e.def as usize].builder
            })
            .collect();
        let Some(&builder) = builders.iter().min_by_key(|id| {
            let e = &self.state.entities[id.idx as usize];
            let dx = e.pos.x.to_f32() - (site.x as f32 + 0.5);
            let dy = e.pos.y.to_f32() - (site.y as f32 + 0.5);
            let dist = ((dx * dx + dy * dy) * 16.0) as i64;
            (self.pending_builds_of(**id), dist, id.idx)
        }) else {
            self.mode = Mode::Normal;
            return;
        };
        if !self.state.valid_building_site(def, site, Some(builder.idx)) {
            self.deny("CANNOT BUILD THERE");
            return;
        }
        let d = &self.state.data.buildings[def as usize];
        let p = &self.state.players[self.human as usize];
        if p.minerals < d.cost_minerals {
            self.deny("NOT ENOUGH MINERALS");
            return;
        }
        if p.gas < d.cost_gas {
            self.deny("NOT ENOUGH PLASMA");
            return;
        }
        // A worker with build work already gets this appended; a free one
        // starts now. With a single worker selected, shift keeps its usual
        // "queue after current activity" meaning.
        let queued = if builders.len() == 1 {
            self.shift
        } else {
            self.pending_builds_of(builder) > 0
        };
        // Shift also chains placement mode for the next building.
        self.pending.push((self.human, Command::Build {
            worker: builder,
            building: def,
            site,
            queued,
        }));
        self.sfx(Sfx::AckBuild);
    }

    /// Train from the least-queued selected building of the active type —
    /// SC2 multi-building macro.
    pub(crate) fn train_hotkey(&mut self, slot: usize) {
        let Some(first) = self.primary_selected_building() else { return };
        let def0 = self.state.entities[first.idx as usize].def;
        let trains = &self.state.data.buildings[def0 as usize].trains;
        let Some(&unit) = trains.get(slot) else { return };
        // Understandable failure feedback before the sim silently drops it.
        let d = &self.state.data.units[unit as usize];
        if !self.state.requirement_met(self.human, d.requires) {
            let req = self.state.data.buildings[d.requires.unwrap() as usize].name.clone();
            self.deny(&format!("REQUIRES {}", req.to_uppercase()));
            return;
        }
        let p = &self.state.players[self.human as usize];
        if p.minerals < d.cost_minerals {
            self.deny("NOT ENOUGH MINERALS");
            return;
        }
        if p.gas < d.cost_gas {
            self.deny("NOT ENOUGH PLASMA");
            return;
        }
        let (used, provided) = self.state.supply(self.human);
        if used + d.supply > provided {
            self.deny("NOT ENOUGH SUPPLY - BUILD SUPPLY PYLONS");
            return;
        }
        let bs = self.selected_own_buildings();
        let best = bs
            .iter()
            .filter(|id| {
                let e = &self.state.entities[id.idx as usize];
                e.def == def0 && e.construction.is_none()
            })
            .min_by_key(|id| (self.state.entities[id.idx as usize].queue.len(), id.idx));
        if let Some(&bid) = best {
            if self.state.entities[bid.idx as usize].queue.len() >= 5 {
                self.deny("PRODUCTION QUEUE FULL");
                return;
            }
            self.pending.push((self.human, Command::Train { building: bid, unit }));
            self.sfx(Sfx::Click);
        }
    }

    pub(crate) fn cancel_train_in(&mut self, building: EntityId, slot: usize) {
        self.pending
            .push((self.human, Command::CancelTrain { building, slot: slot as u8 }));
    }

    // ------------------------------------------------------- selection ----

    pub(crate) fn own_selected_units(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.selection.iter().copied().filter(|id| {
            self.state
                .get(*id)
                .is_some_and(|e| e.owner == self.human && e.kind == EntityKind::Unit)
        })
    }

    pub(crate) fn selected_builder(&self) -> Option<EntityId> {
        self.own_selected_units().find(|id| {
            let e = &self.state.entities[id.idx as usize];
            self.state.data.units[e.def as usize].builder
        })
    }

    /// All selected own buildings (including under construction — rally can
    /// be preset and construction can be canceled).
    pub(crate) fn selected_own_buildings(&self) -> Vec<EntityId> {
        self.selection
            .iter()
            .copied()
            .filter(|id| {
                self.state
                    .get(*id)
                    .is_some_and(|e| e.owner == self.human && e.kind == EntityKind::Building)
            })
            .collect()
    }

    /// The building the queue display / command card operates on: the
    /// Tab-active subgroup's representative if it is a building.
    pub(crate) fn primary_selected_building(&self) -> Option<EntityId> {
        match self.active_type() {
            Some((EntityKind::Building, _)) => self.active_entity(),
            _ => self.selected_own_buildings().into_iter().next(),
        }
    }

    pub(crate) fn single_own_building(&self) -> Option<EntityId> {
        self.primary_selected_building()
    }

    pub(crate) fn lerp_pos(&self, i: usize) -> (f32, f32) {
        let e = &self.state.entities[i];
        let alpha = (self.acc / TICK_DT) as f32;
        (
            e.prev_pos.x.to_f32() + (e.pos.x.to_f32() - e.prev_pos.x.to_f32()) * alpha,
            e.prev_pos.y.to_f32() + (e.pos.y.to_f32() - e.prev_pos.y.to_f32()) * alpha,
        )
    }

    pub(crate) fn entity_screen_pos(&self, i: usize) -> (f32, f32) {
        let (px, py) = self.lerp_pos(i);
        self.world_to_screen_elev(px, py)
    }

    /// Visual elevation of a tile: ramps sit halfway between their levels
    /// so they read as inclines.
    pub(crate) fn visual_elev(&self, x: i32, y: i32) -> f32 {
        let map = &self.state.map;
        if map.kind_at(x, y) == TileKind::Ramp {
            // Halfway up to the highest adjacent walkable tile.
            let mut hi = map.elev_at(x, y);
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                let (nx, ny) = (x + dx, y + dy);
                if map.walkable(nx, ny) {
                    hi = hi.max(map.elev_at(nx, ny));
                }
            }
            map.elev_at(x, y) as f32 + (hi as f32 - map.elev_at(x, y) as f32) * 0.5
        } else {
            map.elev_at(x, y) as f32
        }
    }

    pub(crate) fn world_to_screen_elev(&self, wx: f32, wy: f32) -> (f32, f32) {
        let (sx, sy) = self.cam.world_to_screen(wx, wy);
        let elev = self.visual_elev(wx.floor() as i32, wy.floor() as i32);
        (sx, sy - elev * iso::ELEV_PX * self.cam.zoom)
    }

    /// Sprite-rect based picking. Units win over resources over buildings.
    fn entity_under_cursor(&self, mx: f32, my: f32) -> Option<u32> {
        let z = self.cam.zoom;
        let mut best: Option<(u32, f32, u32)> = None; // (priority, dist, idx)
        for i in 0..self.state.entities.len() {
            let e = &self.state.entities[i];
            if !e.alive {
                continue;
            }
            if e.owner != self.human
                && e.kind != EntityKind::Resource
                && !self.reveal_all
                && !self.state.visible_to(self.human, i as u32)
            {
                continue;
            }
            let (sx, sy) = self.entity_screen_pos(i);
            let (hw, top, bottom, priority) = match e.kind {
                EntityKind::Unit => {
                    let r = self.state.data.units[e.def as usize].radius.to_f32();
                    ((r * 30.0 * z).max(8.0 * z), 26.0 * z, 6.0 * z, 0u32)
                }
                EntityKind::Resource => {
                    if e.def == RES_GEYSER {
                        (32.0 * z, 34.0 * z, 12.0 * z, 1)
                    } else {
                        (15.0 * z, 24.0 * z, 6.0 * z, 1)
                    }
                }
                EntityKind::Building => {
                    let btype = self.building_type[e.def as usize];
                    let (fw, fh) = self.state.data.buildings[e.def as usize].footprint;
                    let h = self.gfx.book.building_px_h[btype];
                    (fw as f32 * 18.0 * z, h * 0.8 * z, fh as f32 * 9.0 * z, 2)
                }
            };
            if mx >= sx - hw && mx <= sx + hw && my >= sy - top && my <= sy + bottom {
                let d = (mx - sx).abs() + (my - (sy - top * 0.3)).abs();
                if best.map_or(true, |(bp, bd, _)| (priority, d) < (bp, bd)) {
                    best = Some((priority, d, i as u32));
                }
            }
        }
        best.map(|(_, _, i)| i)
    }

    pub(crate) fn hovered_site(&self, def: DefId) -> TilePos {
        let (wx, wy) = self.cam.screen_to_world(self.mouse.0, self.mouse.1);
        let d = &self.state.data.buildings[def as usize];
        // Extractors snap to the nearest free geyser.
        if d.gas_extractor {
            let mut best: Option<(f32, TilePos)> = None;
            for &(origin, _) in &self.state.map.geysers {
                if self.state.geyser_at(origin).is_none() {
                    continue;
                }
                let c = origin.center();
                let dist = (c.x.to_f32() - wx).abs() + (c.y.to_f32() - wy).abs();
                if best.map_or(true, |(bd, _)| dist < bd) {
                    best = Some((dist, origin));
                }
            }
            if let Some((_, origin)) = best {
                return origin;
            }
        }
        let (fw, fh) = d.footprint;
        TilePos::new(
            (wx.floor() as i32 - fw / 2).clamp(0, self.state.map.width - fw),
            (wy.floor() as i32 - fh / 2).clamp(0, self.state.map.height - fh),
        )
    }

    // ---------------------------------------------------------- update ----

    pub(crate) fn step_sim(&mut self, cmds: Vec<(u8, Command)>) {
        self.state.step(&cmds);
        self.step_post();
    }

    /// Ambient world particles: geyser vapor, chimney smoke, hive spores.
    /// Called once per sim tick; render-side only.
    fn tick_emitters(&mut self) {
        let tick = self.state.tick;
        for i in 0..self.state.entities.len() {
            let e = &self.state.entities[i];
            if !e.alive {
                continue;
            }
            let salt = crate::atlas::hash2(i as i32, 0, 5150) % 97;
            let (x, y) = (e.pos.x.to_f32(), e.pos.y.to_f32());
            match e.kind {
                EntityKind::Resource if e.def == RES_GEYSER => {
                    if (tick + salt) % 10 == 0 {
                        let j = (crate::atlas::hash2(i as i32, tick as i32, 77) % 60) as f32 * 0.02 - 0.6;
                        self.effects.push(Effect { kind: EffKind::Vapor, ax: x + j, ay: y, bx: 0.0, by: 0.0, age: 0.0, ttl: 1.6, style: 0 });
                    }
                }
                EntityKind::Building if e.construction.is_none() => {
                    let bt = self.building_type[e.def as usize];
                    match bt {
                        // Forge / bastion smelter / fume tap flare: smoke.
                        4 | 14 | 16 => {
                            if (tick + salt) % 14 == 0 {
                                let (ox, oy) = if bt == 4 { (0.9, 40.0) } else if bt == 14 { (0.7, 52.0) } else { (0.55, 38.0) };
                                self.effects.push(Effect { kind: EffKind::Smoke, ax: x + ox, ay: y, bx: (salt % 9) as f32, by: oy, age: 0.0, ttl: 2.2, style: 0 });
                            }
                        }
                        // Hive: drifting green spores.
                        7 => {
                            if (tick + salt) % 16 == 0 {
                                let j = (crate::atlas::hash2(i as i32, tick as i32, 31) % 100) as f32 * 0.03 - 1.5;
                                self.effects.push(Effect { kind: EffKind::Vapor, ax: x + j, ay: y + 0.5, bx: 0.0, by: 0.0, age: 0.0, ttl: 2.0, style: 1 });
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    /// Render-side bookkeeping after any sim step (SP or lockstep).
    /// Report a ranked result once and schedule a rating refresh for the
    /// end screen (delayed so the opponent's confirming report can land).
    pub(crate) fn report_ranked(&mut self, winner_slot: u8) {
        if self.mm_reported {
            return;
        }
        let Some(code) = self.mm_code.clone() else { return };
        self.mm_reported = true;
        crate::relay::report_result_async(
            self.settings.relay_url.clone(),
            code,
            self.settings.player_id.clone(),
            winner_slot,
        );
        self.mm_rating_rx = Some(crate::relay::fetch_rating_async_delayed(
            self.settings.relay_url.clone(),
            self.settings.player_id.clone(),
            2500,
        ));
    }

    pub(crate) fn step_post(&mut self) {
        self.tick_emitters();
        if self.state.winner.is_some() {
            self.save_replay();
            if let Some(w) = self.state.winner {
                self.report_ranked(w);
            }
        }
        self.facings.resize(self.state.entities.len(), 2);
        self.headings
            .resize(self.state.entities.len(), std::f32::consts::FRAC_PI_2);
        self.recoil.resize(self.state.entities.len(), 0.0);
        for i in 0..self.state.entities.len() {
            let e = &self.state.entities[i];
            if !e.alive || e.kind != EntityKind::Unit {
                continue;
            }
            let dx = e.pos.x.to_f32() - e.prev_pos.x.to_f32();
            let dy = e.pos.y.to_f32() - e.prev_pos.y.to_f32();
            if dx * dx + dy * dy > 1e-6 {
                let (ix, iy) = ((dx - dy) * iso::TILE_HALF_W, (dx + dy) * iso::TILE_HALF_H);
                let target = iy.atan2(ix);
                // Sweep toward the new heading: heavier chassis turn slower.
                let d = &self.state.data.units[e.def as usize];
                let rate = if d.radius.to_f32() > 0.5 {
                    0.28
                } else if d.fly {
                    0.5
                } else {
                    0.65
                };
                let cur = self.headings[i];
                let mut diff = target - cur;
                while diff > std::f32::consts::PI {
                    diff -= std::f32::consts::TAU;
                }
                while diff < -std::f32::consts::PI {
                    diff += std::f32::consts::TAU;
                }
                let step = diff.clamp(-rate, rate);
                let ang = cur + step;
                self.headings[i] = ang;
                let sector =
                    ((ang / std::f32::consts::FRAC_PI_4).round() as i32).rem_euclid(8);
                self.facings[i] = sector as u8;
            }
        }
        self.spawn_effects();
    }

    fn spawn_effects(&mut self) {
        let events: Vec<SimEvent> = self.state.events.clone();
        let mut combat_sounds = 0u32;
        let mut death_sounds = 0u32;
        for ev in events {
            match ev {
                SimEvent::Attack { from, to } => {
                    let f = &self.state.entities[from as usize];
                    let t = &self.state.entities[to as usize];
                    let ft = TilePos::of(f.pos);
                    let tt = TilePos::of(t.pos);
                    // Under-attack minimap alert for own stuff.
                    if t.owner == self.human {
                        let (tx_, ty_) = (t.pos.x.to_f32(), t.pos.y.to_f32());
                        let dup = self.effects.iter().any(|e| {
                            e.kind == EffKind::MapPing
                                && (e.ax - tx_).abs() + (e.ay - ty_).abs() < 8.0
                        });
                        if !dup {
                            self.effects.push(Effect {
                                kind: EffKind::MapPing,
                                ax: tx_,
                                ay: ty_,
                                bx: 0.0,
                                by: 0.0,
                                age: 0.0,
                                ttl: 2.0, style: 0,
                            });
                            let alarm_ok = self
                                .last_alarm_sfx
                                .map_or(true, |t| t.elapsed().as_secs_f32() > 5.0);
                            if alarm_ok {
                                self.sfx(Sfx::Alarm);
                                self.last_alarm_sfx = Some(Instant::now());
                            }
                        }
                        self.last_alert = Some((tx_, ty_));
                    }
                    let seen = self.reveal_all
                        || self.state.fog[self.human as usize].visible(&self.state.map, ft)
                        || self.state.fog[self.human as usize].visible(&self.state.map, tt);
                    if !seen {
                        continue;
                    }
                    let (fx_, fy_) = (f.pos.x.to_f32(), f.pos.y.to_f32());
                    let (tx_, ty_) = (t.pos.x.to_f32(), t.pos.y.to_f32());
                    let d = ((tx_ - fx_).powi(2) + (ty_ - fy_).powi(2)).sqrt().max(0.001);
                    let r = self.state.radius_of(from).to_f32() + 0.2;
                    let mx = fx_ + (tx_ - fx_) / d * r;
                    let my = fy_ + (ty_ - fy_) / d * r;
                    if let Some(r) = self.recoil.get_mut(from as usize) {
                        *r = 0.14;
                    }
                    let ut = if f.sieged && self.unit_type[f.def as usize] == 3 {
                        6
                    } else {
                        self.unit_type[f.def as usize]
                    };
                    let style = self.fire_style(ut);
                    if combat_sounds < 3 {
                        combat_sounds += 1;
                        self.sfx(match style {
                            2 => Sfx::Cannon,
                            8 => Sfx::Rail,
                            4 => Sfx::Spit,
                            7 => Sfx::Zap,
                            1 | 5 | 6 | 9 => Sfx::Slash,
                            _ => Sfx::Shot,
                        });
                    }
                    let push = |fx: &mut Vec<Effect>, kind, ax, ay, bx, by, ttl, st| {
                        fx.push(Effect { kind, ax, ay, bx, by, age: 0.0, ttl, style: st });
                    };
                    match style {
                        // Energy blade / claws / bite: slash fan at the victim,
                        // oriented away from the attacker.
                        1 => push(&mut self.effects, EffKind::Slash, tx_, ty_, fx_, fy_, 0.16, 0),
                        5 | 9 => push(&mut self.effects, EffKind::Slash, tx_, ty_, fx_, fy_, 0.15, 1),
                        6 => {
                            push(&mut self.effects, EffKind::Slash, tx_, ty_, fx_, fy_, 0.18, 2);
                            push(&mut self.effects, EffKind::Ring, tx_, ty_, 0.7, 0.0, 0.22, 0);
                        }
                        // Acid glob: lobbed projectile, goo spark on land.
                        4 => {
                            push(&mut self.effects, EffKind::Glob, mx, my, tx_, ty_, 0.16, 0);
                            push(&mut self.effects, EffKind::Burst, tx_, ty_, 5.0, 0.0, 0.28, 1);
                        }
                        // Cannon: heavy muzzle, fast shell, impact ring.
                        2 => {
                            push(&mut self.effects, EffKind::Flash, mx, my, 0.0, 0.0, 0.12, 1);
                            push(&mut self.effects, EffKind::Glob, mx, my, tx_, ty_, 0.09, 2);
                            push(&mut self.effects, EffKind::Ring, tx_, ty_, 1.0, 0.0, 0.3, 0);
                            push(&mut self.effects, EffKind::Spark, tx_, ty_, 0.0, 0.0, 0.2, 0);
                        }
                        // Electric arc: jagged bolt, no tracer.
                        7 => {
                            push(&mut self.effects, EffKind::Arc, mx, my, tx_, ty_, 0.1, 0);
                            push(&mut self.effects, EffKind::Spark, tx_, ty_, 0.0, 0.0, 0.14, 2);
                        }
                        // Rail slug: violet streak + hard impact.
                        8 => {
                            push(&mut self.effects, EffKind::Flash, mx, my, 0.0, 0.0, 0.1, 2);
                            push(&mut self.effects, EffKind::Glob, mx, my, tx_, ty_, 0.06, 1);
                            push(&mut self.effects, EffKind::Ring, tx_, ty_, 0.9, 0.0, 0.26, 0);
                        }
                        // Rapid bolts: thin twin cyan tracers.
                        3 => {
                            push(&mut self.effects, EffKind::Flash, mx, my, 0.0, 0.0, 0.06, 0);
                            if d > 1.0 {
                                push(&mut self.effects, EffKind::Tracer, mx, my, tx_, ty_, 0.045, 1);
                                push(&mut self.effects, EffKind::Tracer, mx, my + 0.12, tx_, ty_ + 0.1, 0.05, 1);
                            }
                            push(&mut self.effects, EffKind::Spark, tx_, ty_, 0.0, 0.0, 0.12, 0);
                        }
                        // Utility tools: brief chip spark only.
                        10 => push(&mut self.effects, EffKind::Spark, tx_, ty_, 0.0, 0.0, 0.1, 0),
                        // Rifle default: muzzle, tracer, impact.
                        _ => {
                            push(&mut self.effects, EffKind::Flash, mx, my, 0.0, 0.0, 0.09, 0);
                            if d > 1.2 {
                                push(&mut self.effects, EffKind::Tracer, mx, my, tx_, ty_, 0.05, 0);
                            }
                            push(&mut self.effects, EffKind::Spark, tx_, ty_, 0.0, 0.0, 0.16, 0);
                        }
                    }
                }
                SimEvent::Ready { pos: _, owner } => {
                    if owner == self.human {
                        self.sfx(Sfx::UnitReady);
                    }
                }
                SimEvent::BuildingDone { pos: _, owner } => {
                    if owner == self.human {
                        self.sfx(Sfx::BuildDone);
                    }
                }
                SimEvent::ResearchDone { owner } => {
                    if owner == self.human {
                        self.sfx(Sfx::ResearchDone);
                    }
                }
                SimEvent::Cast { pos, kind } => {
                    self.sfx(match kind {
                        1 => Sfx::Cannon,
                        2 => Sfx::Spit,
                        3 => Sfx::Rail,
                        4 => Sfx::BuildDone,
                        5 => Sfx::Zap,
                        6 => Sfx::Spit,
                        _ => Sfx::Storm,
                    });
                    let (x, y) = (pos.x.to_f32(), pos.y.to_f32());
                    self.effects.push(Effect {
                        kind: EffKind::Ring,
                        ax: x,
                        ay: y,
                        bx: 2.0,
                        by: 0.0,
                        age: 0.0,
                        ttl: 0.4, style: 0,
                    });
                }
                SimEvent::Death { pos, kind, def, .. } => {
                    let t = TilePos::of(pos);
                    let seen = self.reveal_all
                        || self.state.fog[self.human as usize].visible(&self.state.map, t);
                    if !seen {
                        continue;
                    }
                    let mat = self.material(kind, def);
                    if death_sounds < 2 {
                        death_sounds += 1;
                        self.sfx(if kind == EntityKind::Building {
                            Sfx::BigExplosion
                        } else if mat == 0 {
                            Sfx::Explosion
                        } else {
                            Sfx::Slash
                        });
                    }
                    let (x, y) = (pos.x.to_f32(), pos.y.to_f32());
                    let big = kind == EntityKind::Building;
                    let push = |fx: &mut Vec<Effect>, kind, bx, by, ttl, st| {
                        fx.push(Effect { kind, ax: x, ay: y, bx, by, age: 0.0, ttl, style: st });
                    };
                    match mat {
                        // Mech: flash, ring, flying plates, smoke, scorch.
                        0 => {
                            push(&mut self.effects, EffKind::Flash, 0.0, 0.0, 0.14, 1);
                            push(&mut self.effects, EffKind::Ring, if big { 2.0 } else { 1.0 }, 0.0, 0.35, 0);
                            push(&mut self.effects, EffKind::Burst, 0.0, 0.0, if big { 0.7 } else { 0.5 }, 2);
                            for k in 0..if big { 4 } else { 2 } {
                                push(&mut self.effects, EffKind::Smoke, k as f32 * 5.0, 6.0 + k as f32 * 2.0, 1.6 + k as f32 * 0.4, 0);
                            }
                            push(&mut self.effects, EffKind::Stain, 0.0, 0.0, if big { 30.0 } else { 12.0 }, 2);
                        }
                        // Flesh: blood spray + stain + the body.
                        1 => {
                            push(&mut self.effects, EffKind::Burst, 0.0, 0.0, 0.4, 0);
                            push(&mut self.effects, EffKind::Stain, 0.0, 0.0, 14.0, 0);
                        }
                        // Alien: goo burst + puddle.
                        _ => {
                            push(&mut self.effects, EffKind::Burst, 0.0, 0.0, 0.45, 1);
                            push(&mut self.effects, EffKind::Stain, 0.0, 0.0, 16.0, 1);
                            if big {
                                push(&mut self.effects, EffKind::Burst, 3.0, 0.0, 0.6, 1);
                                push(&mut self.effects, EffKind::Ring, 1.6, 0.0, 0.35, 0);
                            }
                        }
                    }
                    if big {
                        push(&mut self.effects, EffKind::Rubble, 0.0, 0.0, 30.0, 0);
                    } else if mat == 1 {
                        push(&mut self.effects, EffKind::Corpse, 0.0, 0.0, 8.0, 0);
                    }
                }
            }
        }
    }

    pub fn frame(&mut self) {
        if self.editor.is_some() {
            let now = Instant::now();
            let dt = (now - self.last).as_secs_f64().min(0.25);
            self.last = now;
            let fdt = (now - self.frame_t).as_secs_f32().max(1e-4);
            self.frame_t = now;
            self.fps = self.fps * 0.95 + (1.0 / fdt) * 0.05;
            self.camera_input(dt);
            if self.editor_dragging {
                self.editor_paint_at_mouse();
            }
            self.render();
            return;
        }
        if let Some((page, path)) = self.menu_shot.clone() {
            match page.as_str() {
                "settings" => self.page = MenuPage::Settings { from_game: false },
                "keys" => {
                    self.settings_tab = 1;
                    self.page = MenuPage::Settings { from_game: false };
                }
                "difficulty" => self.page = MenuPage::Difficulty,
                "editor" => {
                    self.editor = Some(crate::editor::Editor::new());
                    // Sketch strokes so the capture shows a worked canvas.
                    if let Some(ed) = &mut self.editor {
                        ed.tool = crate::editor::Tool::High;
                        ed.brush = 3;
                        for k in 0..7 {
                            ed.paint(24 + k * 2, 30 - k);
                        }
                        ed.tool = crate::editor::Tool::Tree;
                        ed.brush = 2;
                        ed.paint(36, 44);
                        ed.paint(40, 47);
                        ed.tool = crate::editor::Tool::Mineral;
                        for k in 0..4 {
                            ed.paint(14 + k, 10);
                        }
                        ed.tool = crate::editor::Tool::Geyser;
                        ed.paint(20, 6);
                        ed.tool = crate::editor::Tool::High;
                    }
                    self.rebuild_editor_preview();
                    self.page = MenuPage::None;
                }
                "mp" => self.page = MenuPage::Multiplayer,
                "replays" => {
                    self.replay_files =
                        crate::replays::list(&self.state.data.race_names);
                    self.replay_code = "KRCAP".into();
                    self.replay_status =
                        Some("REPLAY CODE: KRCAP - SHARE IT".into());
                    self.page = MenuPage::Replays;
                }
                "ladder" => {
                    // Blocking fetch so the capture shows real rows.
                    if let Ok(Some(rows)) = crate::relay::fetch_ladder_async(
                        self.settings.relay_url.clone(),
                    )
                    .recv()
                    {
                        self.ladder = Some(rows);
                    }
                    self.page = MenuPage::Ladder;
                }
                "esc" => {
                    self.in_game = true;
                    self.page = MenuPage::EscRoot;
                }
                "card" => {
                    // Build-menu command card with a worker selected.
                    self.in_game = true;
                    self.page = MenuPage::None;
                    let worker = (0..self.state.entities.len() as u32).find(|&i| {
                        let e = &self.state.entities[i as usize];
                        e.alive && e.owner == self.human && e.kind == EntityKind::Unit
                    });
                    if let Some(w) = worker {
                        self.selection = vec![self.state.id_of(w)];
                        self.mode = Mode::BuildMenu;
                        if let Some(b) = self.card_buttons().first() {
                            self.mouse = (b.x + b.w * 0.5, b.y + b.h * 0.5);
                        }
                    }
                }
                "research" => {
                    // Archive selected with the research card + a hovered
                    // tooltip, for UI verification.
                    self.in_game = true;
                    self.page = MenuPage::None;
                    let start = self.state.map.starts[self.human as usize];
                    let archive = self.state.data.building_tag("archive");
                    let a = self.state.spawn_building(
                        self.human,
                        archive,
                        TilePos::new(start.x + 5, start.y + 3),
                        false,
                    );
                    self.state.step(&[]);
                    self.selection = vec![a];
                    if let Some(b) = self.card_buttons().first() {
                        self.mouse = (b.x + b.w * 0.5, b.y + b.h * 0.5);
                    }
                }
                "kyth" => {
                    // Kyth Assembly showcase: one of each unit + structures.
                    self.state = new_game_with(1, 0, "meridian");
                    self.in_game = true;
                    self.page = MenuPage::None;
                    let s = &mut self.state;
                    let start = s.map.starts[self.human as usize];
                    let (bx, by) = (start.x + 4, start.y + 2);
                    let mut sel = Vec::new();
                    for (k, tag) in ["skitter", "spitter", "ravager", "wisp", "weaver"]
                        .iter()
                        .enumerate()
                    {
                        let def = s.data.unit_tag(tag);
                        let id = s.spawn_unit(
                            self.human,
                            def,
                            orion_sim::FxVec2::from_int(bx + (k as i32 % 3) * 2, by + (k as i32 / 3) * 2),
                        );
                        sel.push(id);
                    }
                    let warren = s.data.building_tag("warren");
                    s.spawn_building(self.human, warren, TilePos::new(bx + 2, by + 5), false);
                    let spire = s.data.building_tag("spire");
                    s.spawn_building(self.human, spire, TilePos::new(bx - 3, by + 5), false);
                    s.step(&[]);
                    self.selection = sel;
                    let start = self.state.map.starts[self.human as usize];
                    self.look_at(start.x as f32 + 4.0, start.y as f32 + 3.0);
                }
                "units" => {
                    // Showcase: one of each new unit + a storm, selected.
                    self.in_game = true;
                    self.page = MenuPage::None;
                    let s = &mut self.state;
                    let start = s.map.starts[self.human as usize];
                    let (bx, by) = (start.x + 4, start.y + 2);
                    let breaker = s.data.unit_tag("breaker");
                    let sky = s.data.unit_tag("skywing");
                    let caster = s.data.unit_tag("stormcaller");
                    let b1 = s.spawn_unit(self.human, breaker, orion_sim::FxVec2::from_int(bx, by));
                    let b2 =
                        s.spawn_unit(self.human, breaker, orion_sim::FxVec2::from_int(bx + 3, by));
                    s.entities[b2.idx as usize].sieged = true;
                    let f1 =
                        s.spawn_unit(self.human, sky, orion_sim::FxVec2::from_int(bx + 1, by + 3));
                    let c1 =
                        s.spawn_unit(self.human, caster, orion_sim::FxVec2::from_int(bx - 2, by + 2));
                    s.entities[c1.idx as usize].energy = 143;
                    s.storms.push(orion_sim::state::Storm {
                        pos: orion_sim::FxVec2::from_int(bx + 6, by + 4),
                        ticks_left: 60,
                        owner: self.human,
                        kind: 0,
                    });
                    s.step(&[]);
                    self.selection =
                        vec![b1, b2, f1, c1].iter().map(|id| *id).collect();
                }
                _ => self.page = MenuPage::MainRoot,
            }
            self.gfx.capture = Some(path);
            self.render();
            self.finished = true;
            return;
        }
        if let Some(prefix) = self.script.clone() {
            self.run_script(&prefix);
            return;
        }
        if let Some((prefix, start, frames, every)) = self.record.clone() {
            if self.shot_cross && self.state.tick == 0 {
                self.state = new_game_with(0, 1, &self.game_map.clone());
            }
            if self.shot_reveal {
                self.reveal_all = true;
            }
            self.in_game = true;
            self.page = MenuPage::None;
            // Fast-forward to the start.
            while self.state.tick < start && self.state.winner.is_none() {
                let mut cmds = Vec::new();
                if let Some(b0) = self.shot_bot0.as_mut() {
                    cmds.extend(b0.think(&self.state));
                }
                if !self.stage_mode {
                    cmds.extend(self.bot.think(&self.state));
                }
                self.step_sim(cmds);
                for e in &mut self.effects {
                    e.age += TICK_DT as f32;
                }
                self.effects.retain(|e| e.age < e.ttl);
            }
            if self.record_done >= frames || self.state.winner.is_some() {
                self.finished = true;
                return;
            }
            // Advance `every` ticks, then capture one frame.
            for _ in 0..every {
                let mut cmds = Vec::new();
                if let Some(b0) = self.shot_bot0.as_mut() {
                    cmds.extend(b0.think(&self.state));
                }
                if !self.stage_mode {
                    cmds.extend(self.bot.think(&self.state));
                }
                self.step_sim(cmds);
                for e in &mut self.effects {
                    e.age += TICK_DT as f32 * (every as f32).recip().max(0.04);
                }
                self.effects.retain(|e| e.age < e.ttl);
                for r in &mut self.recoil {
                    *r = (*r - TICK_DT as f32).max(0.0);
                }
            }
            if self.follow {
                // Observer camera: drift toward where the fighting is —
                // mean of this frame's attack events, else army midpoint.
                let mut sum = (0.0f32, 0.0f32);
                let mut n = 0.0f32;
                for ev in &self.state.events {
                    if let orion_sim::state::SimEvent::Attack { from, .. } = ev {
                        let e = &self.state.entities[*from as usize];
                        sum.0 += e.pos.x.to_f32();
                        sum.1 += e.pos.y.to_f32();
                        n += 1.0;
                    }
                }
                self.follow_quiet = if n > 0.0 { 0 } else { self.follow_quiet + 1 };
                let target = if n > 0.0 {
                    Some((sum.0 / n, sum.1 / n))
                } else if self.follow_quiet > 10 {
                    // Long lull: drift to the DENSEST army clump (the
                    // midpoint of two distant armies is empty ground).
                    let armies: Vec<(f32, f32)> = self
                        .state
                        .entities
                        .iter()
                        .filter(|e| {
                            e.alive
                                && e.kind == EntityKind::Unit
                                && !self.state.data.units[e.def as usize].harvester
                        })
                        .map(|e| (e.pos.x.to_f32(), e.pos.y.to_f32()))
                        .collect();
                    armies
                        .iter()
                        .max_by_key(|(x, y)| {
                            armies
                                .iter()
                                .filter(|(ox, oy)| {
                                    (ox - x).powi(2) + (oy - y).powi(2) < 64.0
                                })
                                .count()
                        })
                        .copied()
                } else {
                    None // linger on the last action spot
                };
                if let Some((tx_, ty_)) = target {
                    let f = self.follow_pos.get_or_insert((tx_, ty_));
                    // Fights snap hard, quiet moments drift gently.
                    let k = if n > 0.0 { 0.4 } else { 0.10 };
                    f.0 += (tx_ - f.0) * k;
                    f.1 += (ty_ - f.1) * k;
                }
                if let Some((fx_, fy_)) = self.follow_pos {
                    let (ix, iy) = iso::world_to_iso(fx_, fy_);
                    self.cam.cx = ix;
                    self.cam.cy = iy;
                }
            } else if let Some((fx_, fy_)) = self.shot_focus {
                let (ix, iy) = iso::world_to_iso(fx_, fy_);
                self.cam.cx = ix;
                self.cam.cy = iy;
            }
            if let Some(z) = self.shot_zoom {
                self.cam.zoom = z;
            }
            self.gfx.capture = Some(format!("{prefix}_{:04}.ppm", self.record_done));
            self.render();
            self.record_done += 1;
            return;
        }
        // Replay capture mode: fast-forward a loaded replay, shoot, exit.
        if let Some((target, path)) = self.replay_shot.clone() {
            if self.replay.is_none() {
                eprintln!("replay-shot: no replay loaded");
                self.finished = true;
                return;
            }
            let chunk = 96u32;
            for _ in 0..chunk {
                let replay = self.replay.as_ref().unwrap();
                if self.state.tick >= target || self.state.tick >= replay.duration_ticks {
                    break;
                }
                let cmds = replay.commands_for(self.state.tick, &mut self.replay_cursor);
                self.step_sim(cmds);
                for e in &mut self.effects {
                    e.age += TICK_DT as f32;
                }
                self.effects.retain(|e| e.age < e.ttl);
            }
            let replay = self.replay.as_ref().unwrap();
            if self.state.tick >= target || self.state.tick >= replay.duration_ticks {
                println!(
                    "replay-shot: tick {} checksum {:016x}",
                    self.state.tick,
                    self.state.checksum()
                );
                self.gfx.capture = Some(path);
                self.render();
                self.finished = true;
                return;
            }
            self.render();
            return;
        }
        if let Some((target, path)) = self.shot.clone() {
            if self.shot_cross && self.state.tick == 0 {
                self.state = new_game_with(0, 1, &self.game_map.clone());
            }
            let chunk = 48u32;
            for _ in 0..chunk {
                if self.state.tick >= target || self.state.winner.is_some() {
                    break;
                }
                let mut cmds = Vec::new();
                if let Some(b0) = self.shot_bot0.as_mut() {
                    cmds.extend(b0.think(&self.state));
                }
                if !self.stage_mode {
                    cmds.extend(self.bot.think(&self.state));
                }
                self.step_sim(cmds);
                for e in &mut self.effects {
                    e.age += TICK_DT as f32;
                }
                self.effects.retain(|e| e.age < e.ttl);
            }
            if self.state.tick >= target || self.state.winner.is_some() {
                if self.shot_reveal {
                    self.reveal_all = true;
                }
                if let Some((fx_, fy_)) = self.shot_focus {
                    let (ix, iy) = iso::world_to_iso(fx_, fy_);
                    self.cam.cx = ix;
                    self.cam.cy = iy;
                }
                if let Some(z) = self.shot_zoom {
                    self.cam.zoom = z;
                }
                self.gfx.capture = Some(path);
                self.render();
                self.finished = true;
                return;
            }
            self.render();
            return;
        }

        let now = Instant::now();
        let dt = (now - self.last).as_secs_f64().min(0.25);
        self.last = now;
        let fdt = (now - self.frame_t).as_secs_f32().max(1e-4);
        self.frame_t = now;
        self.fps = self.fps * 0.95 + (1.0 / fdt) * 0.05;

        // Automated MP smoke driver.
        if let Some(role) = self.mp_auto.clone() {
            if self.mp.is_none() && self.mp_waiting.is_none() && self.mm_queue.is_none() {
                if let Some(suffix) = role.strip_prefix("queue:") {
                    // Ranked E2E: fresh identity per process so the
                    // matchmaker sees two distinct players.
                    #[cfg(not(target_arch = "wasm32"))]
                    let id = format!("qa{}{}", suffix, std::process::id());
                    #[cfg(target_arch = "wasm32")]
                    let id = format!("qa{suffix}web");
                    self.settings.player_id = id.clone();
                    println!("mm queue as {id}");
                    self.mm_queue = Some(crate::relay::find_match_async(
                        self.settings.relay_url.clone(),
                        id,
                        format!("QA {suffix}"),
                        0,
                    ));
                } else if let Some(code) = role.strip_prefix("host-pub:") {
                    let (shown, rx) = crate::relay::host_relay_async_full(
                        self.settings.relay_url.clone(),
                        code.to_string(),
                        0,
                        "QA TEST LOBBY",
                        false,
                        "meridian",
                    );
                    self.mp_lobby_code = Some(shown);
                    self.mp_waiting = Some(rx);
                } else if let Some(code) = role.strip_prefix("host-relay:") {
                    let (shown, rx) = crate::relay::host_relay_async_with_code(
                        self.settings.relay_url.clone(),
                        code.to_string(),
                        0,
                    );
                    self.mp_lobby_code = Some(shown);
                    self.mp_waiting = Some(rx);
                } else if let Some(code) = role.strip_prefix("join-relay:") {
                    self.mp_waiting = Some(crate::relay::join_relay_async(
                        self.settings.relay_url.clone(),
                        code.to_string(),
                        1,
                    ));
                } else if role == "host" {
                    match orion_sim::net::host_async(0, orion_sim::net::DEFAULT_PORT) {
                        Ok(rx) => self.mp_waiting = Some(rx),
                        Err(e) => {
                            eprintln!("mp-auto host failed: {e}");
                            self.finished = true;
                            return;
                        }
                    }
                } else {
                    self.mp_waiting = Some(orion_sim::net::join_async(
                        format!("127.0.0.1:{}", orion_sim::net::DEFAULT_PORT),
                        1,
                    ));
                }
            }
            if self.mp.is_some() && self.state.tick >= 24 * 10 {
                let ok = self
                    .mp
                    .as_ref()
                    .map(|m| !m.desync && !m.disconnected)
                    .unwrap_or(false);
                println!(
                    "mp-auto {role}: tick {} desync={} -> {}",
                    self.state.tick,
                    self.mp.as_ref().map(|m| m.desync).unwrap_or(true),
                    if ok { "OK" } else { "FAIL" }
                );
                self.finished = true;
                return;
            }
        }

        #[cfg(not(target_arch = "wasm32"))]
        self.poll_replay_net();

        // Lobby browser: poll the directory while on the multiplayer page.
        if self.page == MenuPage::Multiplayer
            && !self.settings.relay_url.is_empty()
            && self.mp.is_none()
        {
            if let Some(rx) = &self.lobby_fetch {
                if let Ok(result) = rx.try_recv() {
                    if let Some(list) = result {
                        self.lobby_list = list;
                    }
                    self.lobby_fetch = None;
                }
            } else if self
                .lobby_fetch_at
                .map_or(true, |t| t.elapsed().as_secs_f32() > 3.0)
            {
                self.lobby_fetch =
                    Some(crate::relay::fetch_lobbies_async(self.settings.relay_url.clone()));
                self.lobby_fetch_at = Some(Instant::now());
            }
        }

        // Pending host/join attempt resolved?
        if let Some(rx) = &self.mp_waiting {
            match rx.try_recv() {
                Ok(Ok(started)) => self.start_mp_game(started),
                Ok(Err(e)) => {
                    self.mp_error = Some(format!("CONNECTION FAILED: {e}"));
                    self.mp_waiting = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.mp_waiting = None;
                }
            }
        }

        // Ranked queue events.
        if let Some(rx) = &self.mm_queue {
            use crate::relay::QueueEvent;
            match rx.try_recv() {
                Ok(QueueEvent::Queued { mmr, games }) => {
                    self.mm_rating = Some((mmr, games));
                    self.mm_status = format!("IN QUEUE  MMR {mmr}");
                }
                Ok(QueueEvent::Searching { tol, waited_s }) => {
                    self.mm_status =
                        format!("SEARCHING {waited_s}S  RANGE +-{tol} MMR");
                }
                Ok(QueueEvent::Matched { opp_name, opp_mmr }) => {
                    self.mm_status = format!("FOUND: {opp_name} ({opp_mmr})  CONNECTING...");
                }
                Ok(QueueEvent::Started(Ok((started, code)))) => {
                    self.mm_queue = None;
                    if self.mp_auto.is_some() {
                        println!("mm matched code {code}");
                    }
                    self.mm_code = Some(code);
                    self.mm_reported = false;
                    self.start_mp_game(started);
                }
                Ok(QueueEvent::Started(Err(e))) => {
                    self.mm_queue = None;
                    self.mm_status.clear();
                    self.mp_error = Some(format!("MATCHMAKING FAILED: {e}"));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.mm_queue = None;
                    self.mm_status.clear();
                }
            }
        }
        // Ladder fetch result.
        if let Some(rx) = &self.ladder_rx {
            if let Ok(got) = rx.try_recv() {
                self.ladder = got;
                self.ladder_rx = None;
            }
        }
        // Update check result (one-shot at startup). Finding one blocks
        // online play and interrupts with the update prompt.
        if let Some(rx) = &self.update_rx {
            if let Ok(got) = rx.try_recv() {
                self.update = got;
                self.update_rx = None;
                if self.update.is_some() {
                    self.mp_blocked = true;
                    if !self.in_game && self.page == MenuPage::MainRoot {
                        self.page = MenuPage::UpdatePrompt;
                    }
                }
            }
        }
        // Rating for the FIND MATCH row.
        if let Some(rx) = &self.mm_rating_rx {
            if let Ok(got) = rx.try_recv() {
                if got.is_some() {
                    self.mm_rating = got;
                }
                self.mm_rating_rx = None;
            }
        }

        if self.in_game && self.replay.is_some() {
            self.replay_frame(dt);
            self.clamp_camera();
            self.selection.retain(|id| self.state.get(*id).is_some());
            self.render();
            return;
        }
        if self.in_game && self.mp.is_some() {
            self.mp_frame(dt);
            if let Some((_, t0)) = &self.error_flash {
                if t0.elapsed().as_secs_f32() > 1.8 {
                    self.error_flash = None;
                }
            }
            self.clamp_camera();
            self.selection.retain(|id| self.state.get(*id).is_some());
            self.render();
            return;
        }

        let playing = self.in_game && self.page == MenuPage::None;

        if playing {
            self.camera_input(dt);

            if self.countdown_active() {
                self.acc = 0.0;
            } else if self.state.winner.is_none() {
                self.acc += dt * self.settings.game_speed as f64;
                while self.acc >= TICK_DT {
                    let mut cmds = std::mem::take(&mut self.pending);
                    if !self.stage_mode {
                    cmds.extend(self.bot.think(&self.state));
                }
                    self.step_sim(cmds);
                    self.acc -= TICK_DT;
                }
            }

            for e in &mut self.effects {
                e.age += dt as f32;
            }
            self.effects.retain(|e| e.age < e.ttl);
            for r in &mut self.recoil {
                *r = (*r - dt as f32).max(0.0);
            }
        }

        if let Some((_, t0)) = &self.error_flash {
            if t0.elapsed().as_secs_f32() > 1.8 {
                self.error_flash = None;
            }
        }
        self.clamp_camera();
        self.selection.retain(|id| self.state.get(*id).is_some());
        self.render();
    }

    // ---------------------------------------------------------- script ----

    /// Deterministic human-play test. Exercises the real command path:
    /// gather, train (queue), build placement + construction, multi-select,
    /// attack-move — capturing the UI at each stage.
    fn run_script(&mut self, prefix: &str) {
        self.in_game = true;
        self.page = MenuPage::None;
        let captures: [(u32, &str); 5] = [
            (300, "econ"),
            (1500, "build"),
            (2008, "multibuild"),
            (3100, "army"),
            (4800, "attack"),
        ];
        let mut ci = 0;
        while ci < captures.len() {
            let (t, name) = captures[ci];
            if self.state.tick >= t {
                self.stage_script_selection(name);
                self.gfx.capture = Some(format!("{prefix}_{name}.ppm"));
                self.render();
                ci += 1;
                continue;
            }
            let mut cmds = std::mem::take(&mut self.pending);
            self.script_commands(&mut cmds);
            if !self.stage_mode {
                    cmds.extend(self.bot.think(&self.state));
                }
            self.step_sim(cmds);
            for e in &mut self.effects {
                e.age += TICK_DT as f32;
            }
            self.effects.retain(|e| e.age < e.ttl);
        }
        let own_buildings = self
            .state
            .entities
            .iter()
            .filter(|e| e.alive && e.owner == self.human && e.kind == EntityKind::Building)
            .count();
        assert!(own_buildings >= 4, "script: expected HQ+depot+condenser+barracks, got {own_buildings}");
        assert!(
            self.state.players[self.human as usize].gas > 0
                || self
                    .state
                    .entities
                    .iter()
                    .any(|e| e.alive && e.owner == self.human && e.carry_gas),
            "script: gas never flowed"
        );
        println!("script OK: {own_buildings} buildings standing, gas flowing");
        self.finished = true;
    }

    fn script_commands(&mut self, cmds: &mut Vec<(u8, Command)>) {
        let t = self.state.tick;
        match t {
            24 => {
                let workers = self.own_units_of(|d| d.harvester);
                if let Some(res) = self.nearest_mineral_to_start() {
                    cmds.push((self.human, Command::Gather {
                        units: workers,
                        resource: res,
                        queued: false,
                    }));
                }
            }
            30 | 31 => {
                if let Some(hq) = self.own_building_tagged("hq") {
                    let w = self.state.data.unit_tag("fabricator");
                    cmds.push((self.human, Command::Train { building: hq, unit: w }));
                }
            }
            360 => self.script_build("depot", cmds),
            700 => self.script_build_extractor(cmds),
            1400 => self.script_build("barracks", cmds),
            // Staff the condenser once it should be done.
            1500 => {
                if let Some(c) = self.own_building_tagged("condenser") {
                    if self.state.gatherable(self.human, c) {
                        let workers = self.own_units_of(|d| d.harvester);
                        let staff: Vec<_> = workers.into_iter().take(2).collect();
                        if !staff.is_empty() {
                            cmds.push((self.human, Command::Gather {
                                units: staff,
                                resource: c,
                                queued: false,
                            }));
                        }
                    }
                }
            }
            // Multi-worker placement through the real try_place path: 3
            // workers selected + 2 placements must peel off 2 DIFFERENT
            // workers and leave the third mining.
            2000 => self.script_multibuild(),
            // Box-selecting several buildings (no units in the box) must
            // select all of them — regression: buildings were unboxable.
            2450 => self.script_boxselect_buildings(),
            2500 | 2540 | 2580 | 2620 => {
                if let Some(b) = self.own_building_tagged("barracks") {
                    let tr = self.state.data.unit_tag("trooper");
                    cmds.push((self.human, Command::Train { building: b, unit: tr }));
                }
            }
            3150 => {
                let army = self.own_units_of(|d| !d.harvester);
                if !army.is_empty() {
                    cmds.push((self.human, Command::AttackMove {
                        units: army,
                        target: FxVec2::from_int(40, 40),
                        queued: false,
                    }));
                }
            }
            _ => {}
        }
    }

    fn script_build(&mut self, tag: &str, cmds: &mut Vec<(u8, Command)>) {
        let def = self.state.data.building_tag(tag);
        let Some(worker) = self.free_builder() else { return };
        let Some(hq) = self.own_building_tagged("hq") else { return };
        let hq_tile = TilePos::of(self.state.entities[hq.idx as usize].pos);
        for r in 3i32..14 {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    let site = TilePos::new(hq_tile.x + dx, hq_tile.y + dy);
                    if self.state.valid_building_site(def, site, Some(worker.idx)) {
                        cmds.push((self.human, Command::Build { worker, building: def, site, queued: false }));
                        return;
                    }
                }
            }
        }
    }

    fn script_build_extractor(&mut self, cmds: &mut Vec<(u8, Command)>) {
        let def = self.state.data.building_tag("condenser");
        let Some(worker) = self.free_builder() else { return };
        for &(origin, _) in &self.state.map.geysers.clone() {
            if self.state.geyser_at(origin).is_some()
                && self.state.valid_building_site(def, origin, Some(worker.idx))
            {
                cmds.push((self.human, Command::Build { worker, building: def, site: origin, queued: false }));
                return;
            }
        }
    }

    /// Select 3 mining workers, place two depots via try_place, and assert
    /// the builds went to two distinct workers while the third kept mining.
    fn script_multibuild(&mut self) {
        let miners: Vec<EntityId> = self
            .own_units_of(|d| d.builder)
            .into_iter()
            .filter(|id| {
                matches!(self.state.entities[id.idx as usize].order, Order::Gather { .. })
            })
            .take(3)
            .collect();
        if miners.len() < 3 {
            return; // economy not there yet; skip rather than flake
        }
        self.selection = miners.clone();
        let def = self.state.data.building_tag("depot");
        let Some(hq) = self.own_building_tagged("hq") else { return };
        let hq_tile = TilePos::of(self.state.entities[hq.idx as usize].pos);
        let mut placed = 0;
        'search: for r in 3i32..14 {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    let site = TilePos::new(hq_tile.x + dx, hq_tile.y + dy);
                    if self.state.valid_building_site(def, site, None) {
                        let (fw, fh) = self.state.data.buildings[def as usize].footprint;
                        self.mouse = self.cam.world_to_screen(
                            site.x as f32 + fw as f32 * 0.5,
                            site.y as f32 + fh as f32 * 0.5,
                        );
                        let before = self.pending.len();
                        self.try_place(def);
                        if self.pending.len() > before {
                            placed += 1;
                            if placed == 2 {
                                break 'search;
                            }
                        }
                    }
                }
            }
        }
        let build_workers: Vec<EntityId> = self
            .pending
            .iter()
            .filter_map(|(_, c)| match c {
                Command::Build { worker, queued, .. } => {
                    assert!(!queued, "script: distributed builds must start immediately");
                    Some(*worker)
                }
                _ => None,
            })
            .collect();
        assert_eq!(build_workers.len(), 2, "script: expected 2 placements to issue");
        assert_ne!(
            build_workers[0], build_workers[1],
            "script: both builds went to the same worker"
        );
        let untouched = miners
            .iter()
            .filter(|id| !build_workers.contains(id))
            .count();
        assert_eq!(untouched, 1, "script: exactly one worker should keep mining");
        println!("script: multibuild distributed across 2 workers, 1 left mining");
    }

    /// Drag a screen box over every own building; assert they all land in
    /// the selection (units elsewhere must not be required).
    fn script_boxselect_buildings(&mut self) {
        let depot = self.state.data.building_tag("depot");
        let mut min = (f32::MAX, f32::MAX);
        let mut max = (f32::MIN, f32::MIN);
        let mut n = 0;
        for i in 0..self.state.entities.len() {
            let e = &self.state.entities[i];
            if e.alive && e.owner == self.human && e.kind == EntityKind::Building && e.def == depot
            {
                let (sx, sy) = self.entity_screen_pos(i);
                min = (min.0.min(sx), min.1.min(sy));
                max = (max.0.max(sx), max.1.max(sy));
                n += 1;
            }
        }
        if n < 2 {
            return;
        }
        self.selection.clear();
        self.finish_selection((min.0 - 8.0, min.1 - 8.0), (max.0 + 8.0, max.1 + 8.0));
        let picked_buildings = self
            .selection
            .iter()
            .filter(|id| {
                self.state
                    .get(**id)
                    .is_some_and(|e| e.kind == EntityKind::Building)
            })
            .count();
        // Workers may stand inside the box — then units win by design. Only
        // assert the building path when the box was building-only.
        let any_units = self
            .selection
            .iter()
            .any(|id| self.state.get(*id).is_some_and(|e| e.kind == EntityKind::Unit));
        assert!(
            any_units || picked_buildings >= 2,
            "script: box over {n} buildings selected only {picked_buildings}"
        );
        println!("script: box-selected {picked_buildings} buildings (units_in_box={any_units})");
        self.selection.clear();
    }

    fn free_builder(&self) -> Option<EntityId> {
        self.own_units_of(|d| d.builder).into_iter().find(|id| {
            !matches!(
                self.state.entities[id.idx as usize].order,
                Order::Build { .. }
            )
        })
    }

    fn stage_script_selection(&mut self, stage: &str) {
        self.selection = match stage {
            "econ" => self.own_building_tagged("hq").into_iter().collect(),
            // Keep the workers script_multibuild selected — the capture
            // verifies their order-queue lines (build sites + gather).
            "multibuild" => return,
            "build" => self
                .own_building_tagged("condenser")
                .or_else(|| self.own_building_tagged("depot"))
                .into_iter()
                .collect(),
            _ => self.own_units_of(|d| !d.harvester),
        };
    }

    fn own_units_of(&self, f: impl Fn(&orion_sim::UnitDef) -> bool) -> Vec<EntityId> {
        (0..self.state.entities.len() as u32)
            .filter(|&i| {
                let e = &self.state.entities[i as usize];
                e.alive
                    && e.owner == self.human
                    && e.kind == EntityKind::Unit
                    && f(&self.state.data.units[e.def as usize])
            })
            .map(|i| self.state.id_of(i))
            .collect()
    }

    fn own_building_tagged(&self, tag: &str) -> Option<EntityId> {
        let def = self.state.data.building_tag(tag);
        (0..self.state.entities.len() as u32).find_map(|i| {
            let e = &self.state.entities[i as usize];
            (e.alive && e.owner == self.human && e.kind == EntityKind::Building && e.def == def)
                .then(|| self.state.id_of(i))
        })
    }

    /// Dev staging (--stage): a three-way arranged battle near map center
    /// so every weapon style can be captured on demand. Single-player
    /// only — this mutates state outside the command stream by design.
    pub fn stage_showcase(&mut self) {
        use orion_sim::fixed::FxVec2;
        // The staged brawl runs on scripted orders alone — no bot drivers.
        self.shot_bot0 = None;
        self.stage_mode = true;
        let ids: std::collections::HashMap<String, u16> = self
            .state
            .data
            .units
            .iter()
            .enumerate()
            .map(|(k, u)| (u.tag.clone(), k as u16))
            .collect();
        let ud = |tag: &str| -> u16 { ids[tag] };
        // Clear the bot armies: kill everything mobile, keep buildings.
        for i in 0..self.state.entities.len() {
            let e = &self.state.entities[i];
            if e.alive && e.kind == EntityKind::Unit {
                self.state.kill(i as u32);
            }
        }
        let c = (40, 40);
        // West: Vanguard Combine.
        let mut west = Vec::new();
        for k in 0..4 {
            west.push(self.state.spawn_unit(0, ud("trooper"), FxVec2::from_int(c.0 - 7, c.1 - 3 + k * 2)));
        }
        for k in 0..2 {
            west.push(self.state.spawn_unit(0, ud("vanguard"), FxVec2::from_int(c.0 - 5, c.1 - 1 + k * 2)));
        }
        let tank = self.state.spawn_unit(0, ud("breaker"), FxVec2::from_int(c.0 - 10, c.1));
        self.state.entities[tank.idx as usize].sieged = true;
        let dome = self.state.spawn_unit(0, ud("bulwark"), FxVec2::from_int(c.0 - 6, c.1 + 3));
        self.state.entities[dome.idx as usize].sieged = true;
        west.push(self.state.spawn_unit(0, ud("skywing"), FxVec2::from_int(c.0 - 8, c.1 + 4)));
        let caster = self.state.spawn_unit(0, ud("stormcaller"), FxVec2::from_int(c.0 - 9, c.1 - 5));
        self.state.entities[caster.idx as usize].energy = 200;
        // Heroes, one per faction, energized for ability captures.
        let mar = self.state.spawn_unit(0, ud("marshal"), FxVec2::from_int(c.0 - 12, c.1 + 2));
        self.state.entities[mar.idx as usize].energy = 200;
        let bm = self.state.spawn_unit(1, ud("broodmother"), FxVec2::from_int(c.0 + 12, c.1 + 1));
        self.state.entities[bm.idx as usize].energy = 200;
        let mag = self.state.spawn_unit(0, ud("magnus"), FxVec2::from_int(c.0 + 4, c.1 - 12));
        self.state.entities[mag.idx as usize].energy = 200;
        // East: Kyth Assembly.
        let mut east = Vec::new();
        for k in 0..5 {
            east.push(self.state.spawn_unit(1, ud("skitter"), FxVec2::from_int(c.0 + 7, c.1 - 4 + k * 2)));
        }
        for k in 0..2 {
            east.push(self.state.spawn_unit(1, ud("spitter"), FxVec2::from_int(c.0 + 9, c.1 - 1 + k * 2)));
        }
        east.push(self.state.spawn_unit(1, ud("ravager"), FxVec2::from_int(c.0 + 10, c.1 + 2)));
        east.push(self.state.spawn_unit(1, ud("wisp"), FxVec2::from_int(c.0 + 8, c.1 - 6)));
        // North: Ferron Compact (fights whoever it meets first).
        let mut north = Vec::new();
        for k in 0..3 {
            north.push(self.state.spawn_unit(0, ud("arclight"), FxVec2::from_int(c.0 - 2 + k * 2, c.1 - 9)));
        }
        north.push(self.state.spawn_unit(0, ud("mauler"), FxVec2::from_int(c.0, c.1 - 7)));
        north.push(self.state.spawn_unit(0, ud("lodestone"), FxVec2::from_int(c.0 + 2, c.1 - 11)));
        // Everyone brawls at the center. Step the orders in directly —
        // the --shot fast-forward path never drains `pending`.
        let target = FxVec2::from_int(c.0, c.1);
        let cmds = vec![
            (0, Command::AttackMove { units: west, target, queued: false }),
            (1, Command::AttackMove { units: east, target, queued: false }),
            (0, Command::AttackMove { units: north, target, queued: false }),
            (0, Command::Cast { caster, target: FxVec2::from_int(c.0 + 6, c.1) }),
            (0, Command::UseAbility { caster: mar, slot: 0, target: FxVec2::from_int(c.0 + 8, c.1 - 2) }),
            (1, Command::UseAbility { caster: bm, slot: 0, target: FxVec2::from_int(c.0 + 12, c.1 + 1) }),
            (0, Command::UseAbility { caster: mag, slot: 0, target: FxVec2::from_int(c.0 + 2, c.1 - 4) }),
        ];
        self.state.step(&cmds);
        // Park the capture camera on the brawl.
        self.shot_focus = Some((c.0 as f32, c.1 as f32));
    }

    /// Rebuild the preview State from the edited map: resources and
    /// destructibles spawn as entities, starting units/buildings are
    /// purged so the canvas shows terrain + features only.
    pub(crate) fn rebuild_editor_preview(&mut self) {
        let Some(ed) = &self.editor else { return };
        let mut st = State::new_with_races(
            GameData::load_default(),
            ed.map.clone(),
            0xED17,
            &[0, 0],
        );
        for i in 0..st.entities.len() {
            let e = &st.entities[i];
            if e.alive && e.kind != EntityKind::Resource {
                st.kill(i as u32);
            }
        }
        self.state = st;
        self.reveal_all = true;
        // Frame the whole map.
        let (cx, cy) = crate::iso::world_to_iso(
            ed.map.width as f32 * 0.5,
            ed.map.height as f32 * 0.5,
        );
        self.cam.cx = cx;
        self.cam.cy = cy;
    }

    /// Editor tool palette rows: (x, y, w, h, tool index) — shared by draw
    /// and hit-testing.
    pub(crate) fn editor_palette(&self) -> Vec<(f32, f32, f32, f32, usize)> {
        let ui = self.ui();
        (0..crate::editor::TOOLS.len())
            .map(|k| {
                (
                    10.0 * ui,
                    60.0 * ui + k as f32 * 30.0 * ui,
                    92.0 * ui,
                    26.0 * ui,
                    k,
                )
            })
            .collect()
    }

    /// A click while the editor is open: palette first, else paint.
    pub(crate) fn editor_click(&mut self) {
        let (mx, my) = self.mouse;
        for (x, y, w, h, k) in self.editor_palette() {
            if mx >= x && mx <= x + w && my >= y && my <= y + h {
                if let Some(ed) = &mut self.editor {
                    ed.tool = crate::editor::TOOLS[k].0;
                    ed.status = crate::editor::TOOLS[k].2.into();
                }
                return;
            }
        }
        self.editor_paint_at_mouse();
    }

    pub(crate) fn editor_paint_at_mouse(&mut self) {
        let (wx, wy) = self.cam.screen_to_world(self.mouse.0, self.mouse.1);
        let (tx, ty) = (wx.floor() as i32, wy.floor() as i32);
        if let Some(ed) = &mut self.editor {
            ed.paint(tx, ty);
        }
        self.rebuild_editor_preview();
    }

    /// Editor key handling. Returns true when the key was consumed.
    pub(crate) fn editor_key(&mut self, code: KeyCode) -> bool {
        let Some(ed) = &mut self.editor else { return false };
        match code {
            KeyCode::BracketRight => {
                ed.brush = (ed.brush + 1).min(4);
                ed.status = format!("BRUSH {}", ed.brush);
            }
            KeyCode::BracketLeft => {
                ed.brush = (ed.brush - 1).max(1);
                ed.status = format!("BRUSH {}", ed.brush);
            }
            KeyCode::KeyS => {
                // Save under the next free custom slot.
                let names = crate::editor::custom_map_names();
                let mut n = 1;
                while names.contains(&format!("custom{n}")) {
                    n += 1;
                }
                let name = format!("custom{n}");
                match ed.validate().and_then(|_| ed.save(&name)) {
                    Ok(()) => ed.status = format!("SAVED AS {name} - IN THE MAP PICKER NOW"),
                    Err(e) => ed.status = e,
                }
            }
            KeyCode::KeyP => {
                match ed.validate() {
                    Ok(()) => {
                        let _ = ed.save("autosave");
                        let map = ed.map.clone();
                        self.editor = None;
                        self.reveal_all = false;
                        self.state = State::new_with_races(
                            GameData::load_default(),
                            map,
                            0xED17_F00D,
                            &[self.chosen_race, 1],
                        );
                        self.bot = Bot::with_style(
                            1,
                            orion_sim::ai::Difficulty::Normal,
                            0xED17,
                        );
                        self.human = 0;
                        self.in_game = true;
                        self.page = MenuPage::None;
                        self.arm_countdown();
                    }
                    Err(e) => ed.status = e,
                }
            }
            KeyCode::Escape => {
                let _ = ed.save("autosave");
                self.editor = None;
                self.reveal_all = false;
                self.page = MenuPage::MainRoot;
            }
            _ => return false,
        }
        true
    }

    fn nearest_mineral_to_start(&self) -> Option<EntityId> {
        let start = self.state.map.starts[self.human as usize].center();
        let mut best: Option<(i64, u32)> = None;
        for (j, e) in self.state.entities.iter().enumerate() {
            if e.alive && e.kind == EntityKind::Resource && e.def == RES_MINERALS {
                let d = orion_sim::fixed::dist_sq_raw(start, e.pos);
                if best.map_or(true, |(bd, _)| d < bd) {
                    best = Some((d, j as u32));
                }
            }
        }
        best.map(|(_, j)| self.state.id_of(j))
    }

    // ---------------------------------------------------------- render ----

    fn render(&mut self) {
        let mut out: Vec<Inst> = Vec::with_capacity(1 << 15);
        let mut glow: Vec<Inst> = Vec::with_capacity(1 << 10);
        self.draw_terrain(&mut out);
        self.draw_ground_effects(&mut out);
        self.draw_pending_builds(&mut out);
        self.draw_entities(&mut out, &mut glow);
        self.draw_air_effects(&mut out, &mut glow);
        self.draw_waypoints(&mut out);
        self.draw_saturation(&mut out);
        self.draw_placement_ghost(&mut out);
        self.draw_selection_box(&mut out);
        self.draw_fog_drift(&mut out);
        // Everything above lives in the world; glow adds on top of it,
        // and the console/menus draw over both.
        let world_n = out.len();
        if self.editor.is_some() {
            self.draw_editor_ui(&mut out);
        } else {
            self.draw_hud(&mut out);
        }
        self.draw_menu(&mut out);
        self.gfx.render(&out, world_n, &glow);
    }

    /// Editor chrome: tool palette, brush/status footer, markers for
    /// starts and expansions, hovered-tile cursor.
    fn draw_editor_ui(&mut self, out: &mut Vec<Inst>) {
        let Some(ed) = &self.editor else { return };
        let ui = self.ui();
        let book = &self.gfx.book;
        let w = self.cam.screen_w;
        let h = self.cam.screen_h;
        let white = [0.92, 0.92, 0.88, 1.0];
        let gold = [0.95, 0.78, 0.25, 1.0];
        let dim = [0.62, 0.62, 0.6, 1.0];
        // Start + expansion markers.
        let zoom = self.cam.zoom;
        for (k, st) in ed.map.starts.iter().enumerate() {
            let (sx, sy) = self.cam.world_to_screen(st.x as f32 + 0.5, st.y as f32 + 0.5);
            let tc = TEAM_COLORS[k.min(1)];
            self.gfx.sprite(out, book.diamond_outline, sx, sy, 96.0 * zoom, 48.0 * zoom, [tc[0], tc[1], tc[2], 0.9]);
            let ts = self.ts(1.4);
            self.gfx.text(out, sx - 8.0 * ui, sy - 5.0 * ui, ts, [tc[0], tc[1], tc[2], 1.0], &format!("P{}", k + 1));
        }
        for e in &ed.map.expansions {
            let (sx, sy) = self.cam.world_to_screen(e.x as f32 + 1.5, e.y as f32 + 1.5);
            self.gfx.sprite(out, book.diamond_outline, sx, sy, 96.0 * zoom, 48.0 * zoom, [0.9, 0.85, 0.4, 0.6]);
        }
        // Hovered tile cursor sized to the brush.
        let (wx, wy) = self.cam.screen_to_world(self.mouse.0, self.mouse.1);
        let (tx, ty) = (wx.floor(), wy.floor());
        let b = ed.brush as f32;
        let (cx, cy) = self.cam.world_to_screen(tx + 0.5, ty + 0.5);
        self.gfx.sprite(
            out,
            book.diamond_outline,
            cx,
            cy,
            (b * 2.0 - 1.0) * 32.0 * zoom,
            (b * 2.0 - 1.0) * 16.0 * zoom,
            [0.4, 1.0, 0.4, 0.8],
        );
        // Title bar.
        self.gfx.sprite(out, book.chrome_panel, w * 0.5, 16.0 * ui, w, 32.0 * ui, [1.0, 1.0, 1.0, 0.97]);
        self.gfx.text(out, 12.0 * ui, 9.0 * ui, self.ts(2.0), gold, "MAP EDITOR");
        let hint = "LMB PAINT   [ ] BRUSH   S SAVE   P PLAY-TEST   ESC BACK";
        let hw = self.gfx.text_width(self.ts(1.1), hint);
        self.gfx.text(out, w - hw - 12.0 * ui, 12.0 * ui, self.ts(1.1), dim, hint);
        // Palette.
        for (x, y, bw, bh, k) in self.editor_palette() {
            let (tool, label, _) = crate::editor::TOOLS[k];
            let active = ed.tool == tool;
            let plate = if active { book.menu_plate_hi } else { book.menu_plate };
            self.gfx.sprite(out, plate, x + bw * 0.5, y + bh * 0.5, bw, bh, [1.0, 1.0, 1.0, 1.0]);
            let ts = self.ts(1.2);
            let lw = self.gfx.text_width(ts, label);
            let tc = if active { gold } else { white };
            self.gfx.text(out, x + (bw - lw) * 0.5, y + bh * 0.5 - ts * 3.5, ts, tc, label);
        }
        // Status footer.
        self.gfx.sprite(out, book.chrome_panel, w * 0.5, h - 16.0 * ui, w, 32.0 * ui, [1.0, 1.0, 1.0, 0.97]);
        let ts = self.ts(1.3);
        self.gfx.text(out, 12.0 * ui, h - 22.0 * ui, ts, white, &ed.status);
        let bs = format!("BRUSH {}   MINERALS {}   TREES {}", ed.brush, ed.map.minerals.len(), ed.map.trees.len());
        let bw2 = self.gfx.text_width(ts, &bs);
        self.gfx.text(out, w - bw2 - 12.0 * ui, h - 22.0 * ui, ts, dim, &bs);
    }

    pub(crate) fn visible(&self, t: TilePos) -> bool {
        self.reveal_all || self.state.fog[self.human as usize].visible(&self.state.map, t)
    }

    pub(crate) fn explored(&self, t: TilePos) -> bool {
        self.reveal_all || self.state.fog[self.human as usize].explored(&self.state.map, t)
    }

    /// SC2-style fog tint: terrain is always drawn — bright when visible,
    /// dimmed when explored, heavily dimmed when unexplored.
    pub(crate) fn fog_tint(&self, t: TilePos) -> f32 {
        if self.visible(t) {
            1.0
        } else if self.explored(t) {
            0.55
        } else {
            0.32
        }
    }

    fn draw_terrain(&self, out: &mut Vec<Inst>) {
        let map = &self.state.map;
        let zoom = self.cam.zoom;
        let (tw, th) = (iso::TILE_HALF_W * 2.0 * zoom, iso::TILE_HALF_H * 2.0 * zoom);
        let book = &self.gfx.book;
        for y in 0..map.height {
            for x in 0..map.width {
                let t = TilePos::new(x, y);
                let (sx, sy) = self.cam.world_to_screen(x as f32 + 0.5, y as f32 + 0.5);
                let elev = self.visual_elev(x, y);
                let sy = sy - elev * iso::ELEV_PX * zoom;
                let margin = tw + iso::ELEV_PX * 3.0 * zoom;
                if sx < -margin
                    || sx > self.cam.screen_w + margin
                    || sy < -margin
                    || sy > self.cam.screen_h + margin
                {
                    continue;
                }
                let h = hash2(x, y, 12345);
                let region = match map.kind_at(x, y) {
                    TileKind::Blocked => book.rock[(h % 4) as usize],
                    TileKind::Ramp => book.ramp[(h % 2) as usize],
                    TileKind::Ground => {
                        if map.elev_at(x, y) > 0 {
                            book.ground_high[(h % 4) as usize]
                        } else {
                            book.ground_low[(h % 4) as usize]
                        }
                    }
                };
                let dim = self.fog_tint(t);
                let tint = [dim, dim, dim, 1.0];
                self.gfx.sprite(out, region, sx, sy, tw, th, tint);

                let e_here = self.visual_elev(x, y);
                let below = |s: &Self, nx: i32, ny: i32| -> bool {
                    map.in_bounds(nx, ny) && s.visual_elev(nx, ny) < e_here - 0.01
                };
                let cliff_h_screen = (16.0 + iso::ELEV_PX) * zoom;
                let cliff_cy = sy + (cliff_h_screen - th) * 0.5;
                if below(self, x, y + 1) {
                    self.gfx.sprite(out, book.cliff_left, sx, cliff_cy, tw, cliff_h_screen, tint);
                }
                if below(self, x + 1, y) {
                    self.gfx.sprite(out, book.cliff_right, sx, cliff_cy, tw, cliff_h_screen, tint);
                }
            }
        }
    }

    /// Drifting haze over fogged ground: two parallax layers of soft
    /// blobs, denser over unexplored tiles. The fog of war gets actual fog.
    fn draw_fog_drift(&self, out: &mut Vec<Inst>) {
        if self.reveal_all {
            return;
        }
        let map = &self.state.map;
        let fog = &self.state.fog[self.human as usize];
        let book = &self.gfx.book;
        let zoom = self.cam.zoom;
        // Visible world-tile bounds via the screen corners.
        let (w, h) = (self.cam.screen_w, self.cam.screen_h);
        let mut x0 = f32::MAX;
        let mut y0 = f32::MAX;
        let mut x1 = f32::MIN;
        let mut y1 = f32::MIN;
        for (px, py) in [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)] {
            let (wx, wy) = self.cam.screen_to_world(px, py);
            x0 = x0.min(wx);
            y0 = y0.min(wy);
            x1 = x1.max(wx);
            y1 = y1.max(wy);
        }
        let t = self.state.tick as f32 / 24.0;
        const STEP: i32 = 3;
        let bx0 = ((x0 as i32 - 4) / STEP) * STEP;
        let by0 = ((y0 as i32 - 4) / STEP) * STEP;
        for by in (by0..=(y1 as i32 + 4)).step_by(STEP as usize) {
            for bx in (bx0..=(x1 as i32 + 4)).step_by(STEP as usize) {
                let ct = TilePos::new(bx + STEP / 2, by + STEP / 2);
                if !map.in_bounds(ct.x, ct.y) || fog.visible(map, ct) {
                    continue;
                }
                let deep = !fog.explored(map, ct);
                let hsh = crate::atlas::hash2(bx, by, 4242);
                let phase = (hsh % 628) as f32 / 100.0;
                for layer in 0..2 {
                    let spd = if layer == 0 { 0.05 } else { 0.085 };
                    let amp = if layer == 0 { 1.1 } else { 1.7 };
                    let wx = bx as f32 + 1.5 + (t * spd + phase).sin() * amp + t * spd * 2.0 % 1.0;
                    let wy = by as f32 + 1.5 + (t * spd * 0.7 + phase * 1.3).cos() * amp * 0.7;
                    let (sx, sy) = self.cam.world_to_screen(wx, wy);
                    let sz = (STEP as f32 * 2.4 + layer as f32 * 26.0) * iso::TILE_HALF_W * 0.14 * zoom * 4.0;
                    let a = if deep { 0.16 } else { 0.08 } * (1.0 - layer as f32 * 0.3);
                    self.gfx.sprite(out, book.glow_soft, sx, sy, sz, sz * 0.55, [0.35, 0.4, 0.5, a]);
                }
            }
        }
    }

    fn draw_ground_effects(&self, out: &mut Vec<Inst>) {
        let zoom = self.cam.zoom;
        let book = &self.gfx.book;
        for e in &self.effects {
            let (sx, sy) = self.world_to_screen_elev(e.ax, e.ay);
            match e.kind {
                EffKind::Corpse => {
                    let fade = (1.0 - e.age / e.ttl).min(1.0);
                    self.gfx.sprite(out, book.corpse, sx, sy, 20.0 * zoom, 10.0 * zoom, [1.0, 1.0, 1.0, fade * 0.9]);
                }
                EffKind::Rubble => {
                    let fade = (1.0 - e.age / e.ttl).min(1.0);
                    self.gfx.sprite(out, book.rubble, sx, sy, 48.0 * zoom, 28.0 * zoom, [1.0, 1.0, 1.0, fade]);
                }
                EffKind::Stain => {
                    // Ground decal: quick soak-in, slow fade-out.
                    let f = e.age / e.ttl;
                    let a = (f * 12.0).min(1.0) * (1.0 - f).powf(0.6);
                    let (c, w, h) = match e.style {
                        1 => ([0.32, 0.55, 0.2], 15.0, 8.0),  // goo puddle
                        2 => ([0.06, 0.055, 0.05], 18.0, 9.0), // scorch
                        _ => ([0.4, 0.07, 0.05], 12.0, 6.0),   // blood
                    };
                    self.gfx.sprite(out, book.circle, sx, sy + 1.0 * zoom, w * zoom, h * zoom, [c[0], c[1], c[2], a * 0.75]);
                    self.gfx.sprite(out, book.circle, sx - 2.0 * zoom, sy, w * 0.4 * zoom, h * 0.4 * zoom, [c[0], c[1], c[2], a * 0.6]);
                }
                _ => {}
            }
        }
    }

    fn draw_entities(&self, out: &mut Vec<Inst>, glow: &mut Vec<Inst>) {
        let zoom = self.cam.zoom;
        let book = &self.gfx.book;
        let mut order: Vec<(f32, u32)> = Vec::new();
        for i in 0..self.state.entities.len() {
            let e = &self.state.entities[i];
            if !e.alive {
                continue;
            }
            let t = TilePos::of(e.pos);
            let vis = match e.kind {
                EntityKind::Resource => true, // terrain-like: visible through fog
                _ if e.owner == self.human => true,
                _ if e.burrowed => false, // underground: hidden from the enemy
                _ => self.visible(t),
            };
            if !vis {
                continue;
            }
            let (_, sy) = self.entity_screen_pos(i);
            let bias = if e.kind == EntityKind::Building { -0.5 } else { 0.0 };
            order.push((sy + bias, i as u32));
        }
        order.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));

        for &(_, i) in &order {
            let e = &self.state.entities[i as usize];
            let (sx, sy) = self.entity_screen_pos(i as usize);
            let selected = self.selection.contains(&self.state.id_of(i));
            let t = TilePos::of(e.pos);
            let dim = if e.owner == self.human { 1.0 } else { self.fog_tint(t) };
            let tint = [dim, dim, dim, 1.0];
            match e.kind {
                EntityKind::Resource => {
                    if e.def == RES_GEYSER {
                        let r = book.geyser;
                        self.gfx.sprite(out, r, sx, sy - 6.0 * zoom, r.w as f32 / r.scale * zoom, r.h as f32 / r.scale * zoom, tint);
                        let breathe = 0.16 + 0.06 * (self.state.tick as f32 * 0.11 + (i % 5) as f32).sin();
                        self.gfx.sprite(glow, book.glow_soft, sx, sy - 10.0 * zoom, 44.0 * zoom, 24.0 * zoom, [0.25, 0.9, 0.8, breathe * dim]);
                    } else if e.def == orion_sim::state::RES_TREE {
                        let t = TilePos::of(e.pos);
                        let h = crate::atlas::hash2(t.x, t.y, 77);
                        let r = book.trees[(h % 4) as usize];
                        // Per-tree size jitter breaks the cloned-forest look.
                        let js = 0.88 + ((h >> 8) % 25) as f32 * 0.01;
                        let tw = r.w as f32 / r.scale * js * zoom;
                        let th = r.h as f32 / r.scale * js * zoom;
                        self.gfx.sprite(out, book.circle, sx, sy + 2.0 * zoom, 26.0 * js * zoom, 11.0 * js * zoom, [0.0, 0.0, 0.0, 0.28]);
                        self.gfx.sprite(out, r, sx, sy - (th / zoom * 0.5 - 3.0) * zoom, tw, th, tint);
                        if e.hp < orion_sim::state::TREE_HP {
                            let frac = e.hp as f32 / orion_sim::state::TREE_HP as f32;
                            self.bar(out, sx, sy - 24.0 * zoom, 16.0, frac, hp_color(frac));
                        }
                    } else if e.def == orion_sim::state::RES_ROCK {
                        let r = book.rock_wall;
                        self.gfx.sprite(out, r, sx, sy - 5.0 * zoom, r.w as f32 / r.scale * zoom, r.h as f32 / r.scale * zoom, tint);
                        if e.hp < orion_sim::state::ROCK_HP {
                            let frac = e.hp as f32 / orion_sim::state::ROCK_HP as f32;
                            self.bar(out, sx, sy - 16.0 * zoom, 18.0, frac, hp_color(frac));
                        }
                    } else {
                        let variant = if e.amount > 900 {
                            0
                        } else if e.amount > 350 {
                            1
                        } else {
                            2
                        };
                        let r = book.minerals[variant];
                        self.gfx.sprite(out, r, sx, sy - 5.0 * zoom, r.w as f32 / r.scale * zoom, r.h as f32 / r.scale * zoom, tint);
                        // Crystal shimmer: slow per-patch phase offset.
                        let ph = (self.state.tick as f32 * 0.06 + (i % 7) as f32).sin() * 0.04;
                        self.gfx.sprite(glow, book.glow_soft, sx, sy - 9.0 * zoom, 30.0 * zoom, 20.0 * zoom, [0.35, 0.85, 1.0, (0.13 + ph) * dim]);
                    }
                }
                EntityKind::Building => {
                    let btype = self.building_type[e.def as usize];
                    let team = (e.owner as usize).min(1);
                    let r = book.building(btype, team);
                    let (ax, ay) = BUILDING_ANCHOR[btype];
                    // Drop shadow: light from the upper-left.
                    {
                        let fw = self.state.data.buildings[e.def as usize].footprint.0 as f32;
                        let sw = fw * iso::TILE_HALF_W * 2.1 * zoom;
                        self.gfx.sprite(out, book.circle, sx + 4.0 * zoom, sy + 3.0 * zoom, sw, sw * 0.45, [0.0, 0.0, 0.0, 0.28]);
                    }
                    if selected {
                        let d = &self.state.data.buildings[e.def as usize];
                        let w = d.footprint.0 as f32 * iso::TILE_HALF_W * 2.0 * zoom;
                        self.gfx.sprite(out, book.diamond_outline, sx, sy, w + 8.0, w * 0.5 + 4.0, [0.4, 1.0, 0.4, 0.9]);
                    }
                    let alpha = if e.construction.is_some() { 0.6 } else { 1.0 };
                    self.gfx.sprite(
                        out,
                        r,
                        sx + (r.w as f32 / r.scale * 0.5 - ax) * zoom,
                        sy + (r.h as f32 / r.scale * 0.5 - ay) * zoom,
                        r.w as f32 / r.scale * zoom,
                        r.h as f32 / r.scale * zoom,
                        [tint[0], tint[1], tint[2], alpha],
                    );
                    let d = &self.state.data.buildings[e.def as usize];
                    if e.construction.is_none() {
                        let tc = TEAM_COLORS[team];
                        match btype {
                            // Pylon obelisk + Ferron capacitor mast: standing light.
                            1 => {
                                let pu = 0.22 + 0.06 * (self.state.tick as f32 * 0.13 + i as f32).sin();
                                self.gfx.sprite(glow, book.glow_soft, sx, sy - 26.0 * zoom, 36.0 * zoom, 44.0 * zoom, [tc[0], tc[1], tc[2], pu * dim]);
                            }
                            15 => {
                                let pu = 0.20 + 0.05 * (self.state.tick as f32 * 0.17 + i as f32).sin();
                                self.gfx.sprite(glow, book.glow_soft, sx, sy - 30.0 * zoom, 30.0 * zoom, 50.0 * zoom, [0.62, 0.52, 1.0, pu * dim]);
                            }
                            // Hive maw + smelter crucibles: furnace light.
                            7 => {
                                self.gfx.sprite(glow, book.glow_soft, sx, sy + 6.0 * zoom, 30.0 * zoom, 16.0 * zoom, [0.55, 1.0, 0.35, 0.16 * dim]);
                            }
                            4 | 14 => {
                                let fl = 0.14 + 0.05 * (self.state.tick as f32 * 0.4 + i as f32).sin().abs();
                                self.gfx.sprite(glow, book.glow_soft, sx - 8.0 * zoom, sy - 14.0 * zoom, 26.0 * zoom, 18.0 * zoom, [1.0, 0.55, 0.2, fl * dim]);
                            }
                            _ => {}
                        }
                    }
                    if let Some(p) = e.construction {
                        let w = d.footprint.0 as f32 * iso::TILE_HALF_W * 1.6 * zoom;
                        self.gfx.beam(out, sx - w * 0.5, sy, sx + w * 0.5, sy - 14.0 * zoom, 1.5, [0.85, 0.75, 0.4, 0.8]);
                        self.gfx.beam(out, sx + w * 0.5, sy, sx - w * 0.5, sy - 14.0 * zoom, 1.5, [0.85, 0.75, 0.4, 0.8]);
                        let frac = p as f32 / d.build_ticks.max(1) as f32;
                        self.bar(out, sx, sy + 8.0 * zoom, 44.0, frac, [0.9, 0.8, 0.2, 1.0]);
                    } else if let Some(&front) = e.queue.first() {
                        let total = self.state.data.units[front as usize].build_ticks;
                        let frac = e.progress as f32 / total.max(1) as f32;
                        self.bar(out, sx, sy + 8.0 * zoom, 44.0, frac, [0.3, 0.8, 1.0, 1.0]);
                    }
                    let hp_frac = e.hp as f32 / d.hp as f32;
                    if selected || hp_frac < 1.0 {
                        let h_px = book.building_px_h[btype];
                        self.bar(out, sx, sy - (h_px * 0.75) * zoom, 46.0, hp_frac, hp_color(hp_frac));
                    }
                }
                EntityKind::Unit => {
                    let d = &self.state.data.units[e.def as usize];
                    if e.burrowed {
                        let r = book.burrow_mound;
                        if selected {
                            let rw = d.radius.to_f32() * 44.0 * zoom + 8.0 * zoom;
                            self.gfx.sprite(out, book.ring, sx, sy + 2.0 * zoom, rw, rw * 0.5, [0.35, 1.0, 0.35, 0.9]);
                        }
                        self.gfx.sprite(
                            out,
                            r,
                            sx,
                            sy - 2.0 * zoom,
                            r.w as f32 / r.scale * zoom,
                            r.h as f32 / r.scale * zoom,
                            tint,
                        );
                        continue;
                    }
                    // Sieged breakers use their deployed sprite set.
                    let utype = if e.sieged {
                        if self.unit_type[e.def as usize] == 13 { 14 } else { 6 }
                    } else {
                        self.unit_type[e.def as usize]
                    };
                    let team = (e.owner as usize).min(1);
                    let facing = self.facings.get(i as usize).copied().unwrap_or(2) as usize;
                    let moving = e.pos != e.prev_pos;
                    // Walk cycle speed scales with movement speed; flyers and
                    // casters idle-animate too.
                    let rate = (10.0 / (d.step.to_f32() * 24.0).max(1.0)).round().clamp(2.0, 5.0)
                        as u32;
                    let frame = if moving || d.fly || d.energy_max > 0 {
                        ((self.state.tick / rate) % 2) as usize
                    } else {
                        0
                    };
                    let r = book.unit(utype, team, facing, frame);
                    // Flyers hover with a gentle bob; shadow stays grounded.
                    let hover = if d.fly {
                        let bob =
                            ((self.state.tick as f32 * 0.22 + i as f32).sin()) * 1.6;
                        (16.0 + bob) * zoom
                    } else {
                        0.0
                    };
                    let sw = d.radius.to_f32() * 44.0 * zoom;
                    let shadow_a = if d.fly { 0.22 } else { 0.35 };
                    self.gfx.sprite(out, book.circle, sx + 3.0 * zoom, sy + 2.5 * zoom, sw, sw * 0.45, [0.0, 0.0, 0.0, shadow_a]);
                    if selected {
                        let rw = sw + 8.0 * zoom;
                        self.gfx.sprite(out, book.ring, sx, sy + 2.0 * zoom, rw, rw * 0.5, [0.35, 1.0, 0.35, 0.9]);
                    }
                    // Attack recoil: kick back along facing + brief flash.
                    let rec = self.recoil.get(i as usize).copied().unwrap_or(0.0);
                    let (rx_off, ry_off, flash) = if rec > 0.0 {
                        let a = facing as f32 * std::f32::consts::FRAC_PI_4;
                        let k = (rec / 0.14) * 2.5 * zoom;
                        (-a.cos() * k, -a.sin() * k * 0.5, 1.0 + rec * 2.0)
                    } else {
                        (0.0, 0.0, 1.0)
                    };
                    self.gfx.sprite(
                        out,
                        r,
                        sx + rx_off,
                        sy - (r.h as f32 / r.scale * 0.5 - 4.0) * zoom - hover + ry_off,
                        r.w as f32 / r.scale * zoom,
                        r.h as f32 / r.scale * zoom,
                        [tint[0] * flash, tint[1] * flash, tint[2] * flash, tint[3]],
                    );
                    if e.sieged {
                        if let Some((r_aura, _)) = d.shield_aura {
                            // 3D translucent dome over the aura radius: the
                            // base ellipse sits on the ground at the unit.
                            let r = book.shield_dome;
                            let w = r_aura.to_f32() * iso::TILE_HALF_W * 2.0 * 2.0 * zoom;
                            let hgt = w * (r.h as f32 / r.w as f32);
                            let cy2 = sy - (200.0 / 352.0 - 0.5) * hgt;
                            let tc = TEAM_COLORS[(e.owner as usize).min(1)];
                            let pulse = 0.75 + 0.1 * (self.state.tick as f32 * 0.2).sin();
                            self.gfx.sprite(out, r, sx, cy2, w, hgt, [tc[0], tc[1], tc[2], pulse]);
                            self.gfx.sprite(glow, r, sx, cy2, w, hgt, [tc[0], tc[1], tc[2], 0.22]);
                        }
                    }
                    if e.amount > 0 {
                        // Carried cargo: a bobbing chunk so full workers read
                        // at a glance.
                        let c = if e.carry_gas { GAS_COLOR } else { MINERAL_COLOR };
                        let bob = ((self.state.tick as f32 * 0.3 + i as f32).sin()) * 1.2 * zoom;
                        self.gfx.sprite(out, book.spark, sx + 6.0 * zoom, sy - 15.0 * zoom + bob, 7.0 * zoom, 7.0 * zoom, [c[0], c[1], c[2], 1.0]);
                    }
                    // Active-work feedback: chip sparks while mining, weld
                    // arcs while constructing — driven straight off the
                    // order state, no effect bookkeeping.
                    match &e.order {
                        Order::Gather { resource, phase: GatherPhase::Mining { .. }, .. } => {
                            if let Some(res) = self.state.get(*resource) {
                                let (rx, ry) = self
                                    .world_to_screen_elev(res.pos.x.to_f32(), res.pos.y.to_f32());
                                // Chip point on the crystal, on the worker's side.
                                let px = rx + (sx - rx) * 0.35;
                                let py = ry + (sy - ry) * 0.35 - 6.0 * zoom;
                                let h = crate::atlas::hash2(i as i32, (self.state.tick / 3) as i32, 77);
                                if h % 8 < 4 {
                                    let c = if res.kind == EntityKind::Building {
                                        GAS_COLOR
                                    } else {
                                        MINERAL_COLOR
                                    };
                                    self.gfx.beam(out, sx, sy - 8.0 * zoom, px, py, 1.2 * zoom, [1.0, 1.0, 0.9, 0.7]);
                                    for k in 0..2 {
                                        let hk = crate::atlas::hash2(k, h as i32, 31);
                                        let ox = ((hk % 11) as f32 - 5.0) * zoom;
                                        let oy = ((hk >> 4) % 7) as f32 * zoom * 0.5;
                                        self.gfx.sprite(out, book.spark, px + ox, py - oy, 4.0 * zoom, 4.0 * zoom, [c[0], c[1], c[2], 0.9]);
                                    }
                                }
                            }
                        }
                        Order::Build { phase: BuildPhase::Constructing { building }, .. } => {
                            if let Some(b) = self.state.get(*building) {
                                let (bx, by) = self
                                    .world_to_screen_elev(b.pos.x.to_f32(), b.pos.y.to_f32());
                                let h = crate::atlas::hash2(i as i32, (self.state.tick / 2) as i32, 913);
                                let fw = self.state.data.buildings[b.def as usize].footprint.0 as f32;
                                let ox = ((h % 100) as f32 / 100.0 - 0.5) * fw * 20.0 * zoom;
                                let oy = (((h >> 8) % 100) as f32 / 100.0) * 16.0 * zoom;
                                // Weld arc from the worker to a wandering
                                // point on the structure + white-hot spark.
                                if h % 8 < 5 {
                                    self.gfx.beam(out, sx, sy - 8.0 * zoom, bx + ox, by - oy, 1.3 * zoom, [1.0, 0.95, 0.6, 0.8]);
                                    self.gfx.sprite(out, book.spark, bx + ox, by - oy, 6.0 * zoom, 6.0 * zoom, [1.0, 1.0, 0.85, 0.95]);
                                    self.gfx.sprite(out, book.spark, bx + ox + 3.0 * zoom, by - oy + 2.0 * zoom, 3.5 * zoom, 3.5 * zoom, [1.0, 0.8, 0.3, 0.9]);
                                }
                            }
                        }
                        _ => {}
                    }
                    let hp_frac = e.hp as f32 / d.hp as f32;
                    let bar_y = sy - (r.h as f32 + 4.0) * zoom * 0.9 - hover;
                    if selected || hp_frac < 1.0 {
                        self.bar(out, sx, bar_y, 22.0, hp_frac, hp_color(hp_frac));
                    }
                    // Energy bar for casters.
                    if d.energy_max > 0 && (selected || hp_frac < 1.0) {
                        let en = e.energy as f32 / d.energy_max as f32;
                        self.bar(out, sx, bar_y + 6.0, 22.0, en, [0.65, 0.45, 1.0, 1.0]);
                    }
                }
            }
        }
    }

    fn draw_air_effects(&self, out: &mut Vec<Inst>, glow: &mut Vec<Inst>) {
        let zoom = self.cam.zoom;
        let book = &self.gfx.book;
        // Active zones: storms + hero abilities, tinted by kind.
        for s in &self.state.storms {
            let t = TilePos::of(s.pos);
            if !self.visible(t) && !self.reveal_all {
                continue;
            }
            let (sx, sy) = self.world_to_screen_elev(s.pos.x.to_f32(), s.pos.y.to_f32());
            let (zc, zr): ([f32; 3], f32) = match s.kind {
                1 => ([1.0, 0.7, 0.3], 2.5),  // barrage
                2 => ([0.55, 1.0, 0.35], 3.0), // corrosive
                3 => ([0.72, 0.58, 1.0], 3.5), // magnetic well
                _ => ([0.5, 0.9, 1.0], orion_sim::STORM_RADIUS.to_f32()),
            };
            let rad = zr * 32.0 * zoom;
            self.gfx.sprite(out, book.blast_ring, sx, sy, rad * 2.0, rad, [zc[0], zc[1], zc[2], 0.35]);
            // Charged air: pulsing additive dome over the zone.
            let pulse = 0.10 + 0.05 * (self.state.tick as f32 * 0.35).sin();
            self.gfx.sprite(glow, book.glow_soft, sx, sy, rad * 2.2, rad * 1.1, [zc[0], zc[1], zc[2], pulse]);
            // Magnetic well: debris spiraling inward.
            if s.kind == 3 {
                for k in 0..6 {
                    let a = k as f32 * 1.05 - self.state.tick as f32 * 0.14;
                    let rr = rad * (0.25 + ((self.state.tick as f32 * 0.02 + k as f32 * 0.37) % 1.0) * 0.75);
                    let px = sx + a.cos() * rr;
                    let py = sy + a.sin() * rr * 0.5;
                    self.gfx.sprite(glow, book.spark, px, py, 4.0 * zoom, 4.0 * zoom, [zc[0], zc[1], zc[2], 0.7]);
                }
            }
            let bolts = if s.kind == 0 { 7 } else if s.kind == 1 { 4 } else { 0 };
            for k in 0..bolts {
                let h = crate::atlas::hash2(k, (self.state.tick / 2) as i32, 913);
                let a = (h % 628) as f32 / 100.0;
                let rr = ((h >> 8) % 100) as f32 / 100.0 * rad;
                let bx = sx + a.cos() * rr;
                let by = sy + a.sin() * rr * 0.5;
                self.gfx.beam(
                    out,
                    bx,
                    by - 18.0 * zoom,
                    bx + ((h >> 16) % 9) as f32 - 4.0,
                    by,
                    1.3 * zoom,
                    [zc[0].max(0.7), zc[1], zc[2], 0.85],
                );
                self.gfx.sprite(out, book.spark, bx, by, 5.0 * zoom, 5.0 * zoom, [zc[0], zc[1], zc[2], 0.9]);
                self.gfx.sprite(glow, book.glow_soft, bx, by, 16.0 * zoom, 10.0 * zoom, [zc[0], zc[1], zc[2], 0.5]);
            }
        }
        for e in &self.effects {
            let f = e.age / e.ttl;
            match e.kind {
                EffKind::Flash => {
                    let (sx, sy) = self.world_to_screen_elev(e.ax, e.ay);
                    let (mul, gc) = match e.style {
                        1 => (1.7, [1.0, 0.7, 0.35]),
                        2 => (1.3, [0.75, 0.6, 1.0]),
                        _ => (1.0, [1.0, 0.8, 0.45]),
                    };
                    let s = (11.0 + f * 4.0) * mul * zoom;
                    self.gfx.sprite(out, book.flash, sx, sy - 7.0 * zoom, s, s, [1.0, 1.0, 1.0, 1.0 - f]);
                    self.gfx.sprite(glow, book.glow_soft, sx, sy - 7.0 * zoom, s * 2.6, s * 2.0, [gc[0], gc[1], gc[2], (1.0 - f) * 0.7]);
                }
                EffKind::Tracer => {
                    let (x0, y0) = self.world_to_screen_elev(e.ax, e.ay);
                    let (x1, y1) = self.world_to_screen_elev(e.bx, e.by);
                    let c = match e.style {
                        1 => [0.55, 0.95, 1.0],
                        2 => [0.75, 0.62, 1.0],
                        3 => [0.6, 1.0, 0.45],
                        _ => [1.0, 0.95, 0.6],
                    };
                    let th = if e.style == 1 { 1.1 } else { 1.5 };
                    self.gfx.beam(out, x0, y0 - 7.0 * zoom, x1, y1 - 6.0 * zoom, th * zoom, [c[0], c[1], c[2], (1.0 - f) * 0.9]);
                    self.gfx.beam(glow, x0, y0 - 7.0 * zoom, x1, y1 - 6.0 * zoom, th * 3.0 * zoom, [c[0], c[1], c[2], (1.0 - f) * 0.35]);
                }
                EffKind::Glob => {
                    // Lobbed/flat projectile along its flight arc.
                    let (arc, c, sz) = match e.style {
                        1 => (0.05, [0.78, 0.62, 1.0], 4.5), // rail slug
                        2 => (0.28, [1.0, 0.82, 0.5], 5.0),  // cannon shell
                        _ => (0.6, [0.55, 0.95, 0.3], 6.0),  // acid glob
                    };
                    let wx = e.ax + (e.bx - e.ax) * f;
                    let wy = e.ay + (e.by - e.ay) * f;
                    let (sx, sy) = self.world_to_screen_elev(wx, wy);
                    let lift = (std::f32::consts::PI * f).sin() * arc * 32.0 * zoom + 7.0 * zoom;
                    self.gfx.sprite(out, book.spark, sx, sy - lift, sz * zoom, sz * zoom, [c[0], c[1], c[2], 1.0]);
                    self.gfx.sprite(glow, book.glow_soft, sx, sy - lift, sz * 3.2 * zoom, sz * 2.4 * zoom, [c[0], c[1], c[2], 0.5]);
                    if e.style == 1 {
                        // Rail: streak back toward the muzzle.
                        let (px, py) = self.world_to_screen_elev(e.ax + (e.bx - e.ax) * (f - 0.35).max(0.0), e.ay + (e.by - e.ay) * (f - 0.35).max(0.0));
                        self.gfx.beam(glow, px, py - 7.0 * zoom, sx, sy - lift, 3.0 * zoom, [c[0], c[1], c[2], 0.45]);
                    }
                }
                EffKind::Slash => {
                    // Melee fan sweeping across the victim.
                    let (sx, sy) = self.world_to_screen_elev(e.ax, e.ay);
                    let (ox, oy) = self.world_to_screen_elev(e.bx, e.by);
                    let base = (sy - oy).atan2(sx - ox);
                    let c = match e.style {
                        1 => [0.6, 1.0, 0.45],
                        2 => [0.75, 1.0, 0.4],
                        3 => [0.8, 0.7, 1.0],
                        _ => [0.75, 0.88, 1.0],
                    };
                    let sweep = -0.8 + f * 1.6;
                    for k in 0..3 {
                        let a = base + sweep + (k as f32 - 1.0) * 0.38;
                        let len = (11.0 - k as f32 * 2.0) * (if e.style == 2 { 1.5 } else { 1.0 }) * zoom;
                        let x0 = sx + a.cos() * 4.0 * zoom;
                        let y0 = sy - 7.0 * zoom + a.sin() * 2.0 * zoom;
                        self.gfx.beam(out, x0, y0, x0 + a.cos() * len, y0 + a.sin() * len * 0.5, 1.8 * zoom, [c[0], c[1], c[2], (1.0 - f) * 0.9]);
                    }
                    self.gfx.sprite(glow, book.glow_soft, sx, sy - 6.0 * zoom, 22.0 * zoom, 14.0 * zoom, [c[0], c[1], c[2], (1.0 - f) * 0.4]);
                }
                EffKind::Arc => {
                    // Jagged lightning between muzzle prongs and the victim.
                    let (x0, y0) = self.world_to_screen_elev(e.ax, e.ay);
                    let (x1, y1) = self.world_to_screen_elev(e.bx, e.by);
                    let segs = 5;
                    let mut px = x0;
                    let mut py = y0 - 7.0 * zoom;
                    for k in 1..=segs {
                        let t = k as f32 / segs as f32;
                        let h = crate::atlas::hash2(k, (self.state.tick as i32) ^ (e.ax * 17.0) as i32, 431);
                        let jag = if k == segs { 0.0 } else { ((h % 15) as f32 - 7.0) * zoom };
                        let nx = x0 + (x1 - x0) * t - (y1 - y0) / 60.0 * jag * 0.2;
                        let ny = y0 - 7.0 * zoom + (y1 - y0 + 1.0 * zoom) * t + jag * 0.5;
                        self.gfx.beam(out, px, py, nx, ny, 1.3 * zoom, [0.92, 0.88, 1.0, (1.0 - f) * 0.95]);
                        self.gfx.beam(glow, px, py, nx, ny, 4.0 * zoom, [0.7, 0.55, 1.0, (1.0 - f) * 0.5]);
                        px = nx;
                        py = ny;
                    }
                }
                EffKind::Burst => {
                    // Stateless particle burst: droplets fly ballistically.
                    let (sx, sy) = self.world_to_screen_elev(e.ax, e.ay);
                    let seed = (e.ax * 31.0 + e.ay * 17.0) as i32;
                    let (n, c, g, sz) = match e.style {
                        1 => (6, [0.5, 0.9, 0.3], 26.0, 4.0),   // goo
                        2 => (9, [0.42, 0.4, 0.38], 40.0, 3.2), // debris
                        _ => (7, [0.78, 0.12, 0.1], 34.0, 2.6), // blood
                    };
                    for k in 0..n {
                        let h = crate::atlas::hash2(k, seed, 97 + e.style as u32);
                        let a = (h % 628) as f32 / 100.0;
                        let sp = (8.0 + ((h >> 8) % 14) as f32) * zoom;
                        let px = sx + a.cos() * sp * f;
                        let py = sy - 6.0 * zoom + a.sin() * sp * f * 0.5 + g * f * f * zoom * 0.5;
                        let fade = (1.0 - f) * 0.95;
                        if e.style == 2 {
                            self.gfx.sprite(out, book.white, px, py, sz * zoom * (1.0 - f * 0.4), sz * 0.7 * zoom, [c[0], c[1], c[2], fade]);
                        } else {
                            self.gfx.sprite(out, book.spark, px, py, sz * zoom * (1.0 - f * 0.5), sz * zoom * (1.0 - f * 0.5), [c[0], c[1], c[2], fade]);
                        }
                    }
                    if e.style == 1 {
                        self.gfx.sprite(glow, book.glow_soft, sx, sy - 4.0 * zoom, 18.0 * zoom, 12.0 * zoom, [0.5, 0.9, 0.3, (1.0 - f) * 0.3]);
                    }
                }
                EffKind::Smoke => {
                    let (sx, sy) = self.world_to_screen_elev(e.ax, e.ay);
                    let rise = (e.bx + 14.0) * f * zoom;
                    let sz = (6.0 + f * 14.0) * zoom;
                    let a = (1.0 - f) * 0.3 * (f * 8.0).min(1.0);
                    self.gfx.sprite(out, book.glow_soft, sx + (f * 9.0).sin() * 2.0 * zoom, sy - e.by * zoom - rise, sz, sz * 0.8, [0.32, 0.32, 0.34, a]);
                }
                EffKind::Vapor => {
                    let (sx, sy) = self.world_to_screen_elev(e.ax, e.ay);
                    let (c, dir): ([f32; 3], f32) = if e.style == 1 {
                        ([0.5, 0.95, 0.4], 0.35) // hive spores: lazy drift up
                    } else {
                        ([0.35, 0.9, 0.8], 1.0)
                    };
                    let rise = 20.0 * f * dir * zoom;
                    let sz = (5.0 + f * 10.0) * zoom;
                    let a = (1.0 - f) * 0.25 * (f * 6.0).min(1.0);
                    self.gfx.sprite(glow, book.glow_soft, sx + (f * 7.0).sin() * 2.5 * zoom, sy - 8.0 * zoom - rise, sz, sz * 0.8, [c[0], c[1], c[2], a]);
                }
                EffKind::Spark => {
                    let (sx, sy) = self.world_to_screen_elev(e.ax, e.ay);
                    let st: [f32; 3] = match e.style {
                        1 => [0.6, 1.0, 0.45],
                        2 => [0.75, 0.62, 1.0],
                        _ => [1.0, 1.0, 1.0],
                    };
                    let s = (5.0 + f * 6.0) * zoom;
                    self.gfx.sprite(out, book.spark, sx, sy - 6.0 * zoom, s, s, [st[0], st[1], st[2], 1.0 - f]);
                }
                EffKind::Ring => {
                    let (sx, sy) = self.world_to_screen_elev(e.ax, e.ay);
                    let s = (8.0 + f * 26.0) * e.bx * zoom;
                    self.gfx.sprite(out, book.blast_ring, sx, sy, s, s * 0.5, [1.0, 0.8, 0.5, (1.0 - f) * 0.9]);
                }
                EffKind::Ping => {
                    let (sx, sy) = self.world_to_screen_elev(e.ax, e.ay);
                    let s = (26.0 - f * 18.0) * zoom;
                    let c = match e.bx as i32 {
                        1 => [1.0, 0.35, 0.3, 1.0 - f * 0.6],
                        2 => [0.4, 0.9, 1.0, 1.0 - f * 0.6],
                        _ => [0.35, 1.0, 0.35, 1.0 - f * 0.6],
                    };
                    self.gfx.sprite(out, book.ring, sx, sy, s, s * 0.5, c);
                }
                _ => {}
            }
        }
    }

    /// Translucent ghost at every site a worker is ordered (now or queued)
    /// to build on — SC2-style pending-construction feedback.
    fn draw_pending_builds(&self, out: &mut Vec<Inst>) {
        let zoom = self.cam.zoom;
        let mut sites: Vec<(DefId, TilePos)> = Vec::new();
        for e in &self.state.entities {
            if !e.alive || e.owner != self.human || e.kind != EntityKind::Unit {
                continue;
            }
            for order in std::iter::once(&e.order).chain(e.order_queue.iter()) {
                if let Order::Build {
                    def,
                    site,
                    phase: BuildPhase::Travel,
                    ..
                } = order
                {
                    if !sites.contains(&(*def, *site)) {
                        sites.push((*def, *site));
                    }
                }
            }
        }
        for (def, site) in sites {
            let (fw, fh) = self.state.data.buildings[def as usize].footprint;
            let btype = self.building_type[def as usize];
            let r = self.gfx.book.building(btype, self.human as usize);
            let (ax, ay) = BUILDING_ANCHOR[btype];
            let (cx, cy) = self.cam.world_to_screen(
                site.x as f32 + fw as f32 * 0.5,
                site.y as f32 + fh as f32 * 0.5,
            );
            let elev = self.state.map.elev_at(site.x, site.y) as f32;
            let cy = cy - elev * iso::ELEV_PX * zoom;
            self.gfx.sprite(
                out,
                r,
                cx + (r.w as f32 / r.scale * 0.5 - ax) * zoom,
                cy + (r.h as f32 / r.scale * 0.5 - ay) * zoom,
                r.w as f32 / r.scale * zoom,
                r.h as f32 / r.scale * zoom,
                [0.7, 0.85, 1.0, 0.38],
            );
        }
    }

    /// Harvester saturation labels ("8/16" over a base, "2/3" over an
    /// extractor) whenever the selection involves the economy: a worker,
    /// a depot, or an extractor.
    fn draw_saturation(&self, out: &mut Vec<Inst>) {
        let econ_selected = self.selection.iter().any(|id| {
            self.state.get(*id).is_some_and(|e| {
                e.owner == self.human
                    && match e.kind {
                        EntityKind::Unit => self.state.data.units[e.def as usize].harvester,
                        EntityKind::Building => {
                            let d = &self.state.data.buildings[e.def as usize];
                            d.deposit || d.gas_extractor
                        }
                        _ => false,
                    }
            })
        });
        if !econ_selected {
            return;
        }
        let ents = &self.state.entities;
        // Nearest own completed depot for each live mineral patch.
        let depots: Vec<usize> = (0..ents.len())
            .filter(|&j| {
                let e = &ents[j];
                e.alive
                    && e.owner == self.human
                    && e.kind == EntityKind::Building
                    && e.construction.is_none()
                    && self.state.data.buildings[e.def as usize].deposit
            })
            .collect();
        let patch_depot: Vec<(usize, usize)> = (0..ents.len())
            .filter(|&j| {
                let e = &ents[j];
                e.alive && e.kind == EntityKind::Resource && e.def == RES_MINERALS && e.amount > 0
            })
            .filter_map(|j| {
                depots
                    .iter()
                    .copied()
                    .min_by_key(|&d| {
                        let dx = (ents[d].pos.x - ents[j].pos.x).to_f32();
                        let dy = (ents[d].pos.y - ents[j].pos.y).to_f32();
                        ((dx * dx + dy * dy) * 16.0) as i64
                    })
                    .map(|d| (j, d))
            })
            .collect();
        // Who is gathering what (current order only — one loop covers
        // ToResource, Mining and ToDepot phases).
        let mut on_patch = vec![0u32; ents.len()];
        for e in ents.iter() {
            if e.alive && e.owner == self.human && e.kind == EntityKind::Unit {
                if let Order::Gather { resource, .. } = e.order {
                    if self.state.get(resource).is_some() {
                        on_patch[resource.idx as usize] += 1;
                    }
                }
            }
        }
        let zoom = self.cam.zoom;
        let ts = (1.6 * zoom).clamp(1.2, 2.4);
        let label = |out: &mut Vec<Inst>, j: usize, n: u32, cap: u32, c3: [f32; 3]| {
            let e = &ents[j];
            let btype = self.building_type[e.def as usize];
            let top = self.gfx.book.building_px_h[btype];
            let (sx, sy) = self.world_to_screen_elev(e.pos.x.to_f32(), e.pos.y.to_f32());
            let text = format!("{n}/{cap}");
            let w = self.gfx.text_width(ts, &text);
            let color = if n > cap {
                [1.0, 0.45, 0.25, 0.95]
            } else if n == cap {
                [1.0, 0.85, 0.3, 0.95]
            } else {
                [0.95, 0.98, 1.0, 0.95]
            };
            let x = sx - w * 0.5;
            let y = sy - (top + 12.0) * zoom;
            self.gfx.quad(out, x - 10.0 * zoom - 2.0, y - 2.0, w + 12.0 * zoom + 4.0, 9.0 * ts, [0.02, 0.05, 0.08, 0.6]);
            self.gfx.sprite(out, self.gfx.book.spark, x - 5.0 * zoom, y + 3.5 * ts, 7.0 * zoom, 7.0 * zoom, [c3[0], c3[1], c3[2], 1.0]);
            self.gfx.text(out, x, y, ts, color, &text);
        };
        // Mineral line per depot: cap 2 workers per live patch.
        for &d in &depots {
            let mut n = 0u32;
            let mut patches = 0u32;
            for &(p, dd) in &patch_depot {
                if dd == d {
                    patches += 1;
                    n += on_patch[p];
                }
            }
            if patches > 0 {
                label(out, d, n, patches * 2, MINERAL_COLOR);
            }
        }
        // Extractors: cap 3.
        for (j, e) in ents.iter().enumerate() {
            if e.alive
                && e.owner == self.human
                && e.kind == EntityKind::Building
                && e.construction.is_none()
                && self.state.data.buildings[e.def as usize].gas_extractor
                && e.amount > 0
            {
                label(out, j, on_patch[j], 3, GAS_COLOR);
            }
        }
    }

    /// Waypoint lines for selected units + rally line for selected buildings.
    fn draw_waypoints(&self, out: &mut Vec<Inst>) {
        let book = &self.gfx.book;
        for id in &self.selection {
            let Some(e) = self.state.get(*id) else { continue };
            if e.owner != self.human {
                continue;
            }
            if e.kind == EntityKind::Unit {
                let mut prev = self.entity_screen_pos(id.idx as usize);
                for order in std::iter::once(&e.order).chain(e.order_queue.iter()) {
                    let (target, color) = match order {
                        Order::Move { target, .. } => (*target, [0.35, 1.0, 0.35, 0.35]),
                        Order::AttackMove { target, .. } => (*target, [1.0, 0.4, 0.3, 0.35]),
                        // Worker plans are part of the chain: amber to a
                        // build site, mineral-blue to a gather target.
                        Order::Build { def, site, .. } => {
                            let (fw, fh) = self.state.data.buildings[*def as usize].footprint;
                            let c = fx(
                                site.x as f32 + fw as f32 * 0.5,
                                site.y as f32 + fh as f32 * 0.5,
                            );
                            (c, [1.0, 0.8, 0.3, 0.45])
                        }
                        Order::Gather { resource, .. } => {
                            let Some(r) = self.state.get(*resource) else { continue };
                            (r.pos, [0.45, 0.85, 1.0, 0.45])
                        }
                        _ => continue,
                    };
                    let p = self.world_to_screen_elev(target.x.to_f32(), target.y.to_f32());
                    self.gfx.beam(out, prev.0, prev.1, p.0, p.1, 1.5, color);
                    self.gfx.sprite(out, book.diamond, p.0, p.1, 10.0, 5.0, color);
                    prev = p;
                }
            } else if e.kind == EntityKind::Building
                // Under-construction production buildings show their preset
                // rally too — the sim honors it, so the UI must.
                && !self.state.data.buildings[e.def as usize].trains.is_empty()
                && e.rally != e.pos
            {
                let from = self.entity_screen_pos(id.idx as usize);
                let to = self.world_to_screen_elev(e.rally.x.to_f32(), e.rally.y.to_f32());
                let c = [0.4, 0.9, 1.0, 0.5];
                self.gfx.beam(out, from.0, from.1, to.0, to.1, 1.5, c);
                // Rally flag.
                self.gfx.beam(out, to.0, to.1, to.0, to.1 - 14.0 * self.cam.zoom, 2.0, c);
                self.gfx.quad(out, to.0, to.1 - 14.0 * self.cam.zoom, 8.0 * self.cam.zoom, 5.0 * self.cam.zoom, c);
            }
        }
    }

    pub(crate) fn bar(&self, out: &mut Vec<Inst>, cx: f32, cy: f32, w: f32, frac: f32, color: [f32; 4]) {
        let frac = frac.clamp(0.0, 1.0);
        self.gfx.quad(out, cx - w * 0.5 - 1.0, cy - 3.0, w + 2.0, 5.0, [0.05, 0.05, 0.05, 0.85]);
        self.gfx.quad(out, cx - w * 0.5, cy - 2.0, w * frac, 3.0, color);
    }

    fn draw_placement_ghost(&self, out: &mut Vec<Inst>) {
        let Mode::Placing(def) = self.mode else { return };
        let Some(builder) = self.selected_builder() else { return };
        let site = self.hovered_site(def);
        let ok = self.state.valid_building_site(def, site, Some(builder.idx));
        let (fw, fh) = self.state.data.buildings[def as usize].footprint;
        let zoom = self.cam.zoom;
        let color = if ok { [0.3, 1.0, 0.3, 0.4] } else { [1.0, 0.25, 0.25, 0.4] };
        for x in site.x..site.x + fw {
            for y in site.y..site.y + fh {
                let (sx, sy) = self.cam.world_to_screen(x as f32 + 0.5, y as f32 + 0.5);
                let elev = self.visual_elev(x, y);
                self.gfx.sprite(
                    out,
                    self.gfx.book.diamond,
                    sx,
                    sy - elev * iso::ELEV_PX * zoom,
                    iso::TILE_HALF_W * 2.0 * zoom,
                    iso::TILE_HALF_H * 2.0 * zoom,
                    color,
                );
            }
        }
        let btype = self.building_type[def as usize];
        let r = self.gfx.book.building(btype, self.human as usize);
        let (ax, ay) = BUILDING_ANCHOR[btype];
        let (cx, cy) = self.cam.world_to_screen(
            site.x as f32 + fw as f32 * 0.5,
            site.y as f32 + fh as f32 * 0.5,
        );
        let elev = self.state.map.elev_at(site.x, site.y) as f32;
        let cy = cy - elev * iso::ELEV_PX * zoom;
        self.gfx.sprite(
            out,
            r,
            cx + (r.w as f32 / r.scale * 0.5 - ax) * zoom,
            cy + (r.h as f32 / r.scale * 0.5 - ay) * zoom,
            r.w as f32 / r.scale * zoom,
            r.h as f32 / r.scale * zoom,
            [1.0, 1.0, 1.0, 0.5],
        );
    }

    fn draw_selection_box(&self, out: &mut Vec<Inst>) {
        let Some((ax, ay)) = self.select_start else { return };
        let (bx, by) = self.mouse;
        let (x0, x1) = (ax.min(bx), ax.max(bx));
        let (y0, y1) = (ay.min(by), ay.max(by));
        let c = [0.4, 1.0, 0.4, 0.8];
        self.gfx.quad(out, x0, y0, x1 - x0, 1.5, c);
        self.gfx.quad(out, x0, y1, x1 - x0, 1.5, c);
        self.gfx.quad(out, x0, y0, 1.5, y1 - y0, c);
        self.gfx.quad(out, x1, y0, 1.5, y1 - y0, c);
    }
}

pub fn new_game() -> State {
    State::new(GameData::load_default(), meridian(), 0xC0FFEE)
}

fn map_or_meridian(name: &str) -> orion_sim::map::Map {
    orion_sim::map::by_name(name)
        .or_else(|| crate::editor::load_custom(name))
        .unwrap_or_else(meridian)
}

/// Built-in map names plus saved custom maps (picker order).
pub fn all_map_names() -> Vec<String> {
    let mut v: Vec<String> =
        orion_sim::map::MAP_NAMES.iter().map(|s| s.to_string()).collect();
    v.extend(crate::editor::custom_map_names().into_iter().filter(|n| n != "autosave"));
    v
}

pub fn new_game_with(race0: u8, race1: u8, map: &str) -> State {
    State::new_with_races(GameData::load_default(), map_or_meridian(map), 0xC0FFEE, &[race0, race1])
}

pub fn new_game_mp(seed: u64, race0: u8, race1: u8, map: &str) -> State {
    State::new_with_races(GameData::load_default(), map_or_meridian(map), seed, &[race0, race1])
}

pub fn fx(x: f32, y: f32) -> FxVec2 {
    // Input positions quantized to 1/256 tile before entering the sim: the
    // conversion is exact and identical on every machine, so cursor floats
    // never leak nondeterminism into the command stream.
    FxVec2::new(
        orion_sim::Fx(((x * 256.0).round() as i32) << 8),
        orion_sim::Fx(((y * 256.0).round() as i32) << 8),
    )
}

pub fn digit_of(code: KeyCode) -> Option<u8> {
    Some(match code {
        KeyCode::Digit0 => 0,
        KeyCode::Digit1 => 1,
        KeyCode::Digit2 => 2,
        KeyCode::Digit3 => 3,
        KeyCode::Digit4 => 4,
        KeyCode::Digit5 => 5,
        KeyCode::Digit6 => 6,
        KeyCode::Digit7 => 7,
        KeyCode::Digit8 => 8,
        KeyCode::Digit9 => 9,
        _ => return None,
    })
}

pub fn hp_color(frac: f32) -> [f32; 4] {
    if frac > 0.66 {
        [0.3, 0.9, 0.3, 1.0]
    } else if frac > 0.33 {
        [0.95, 0.85, 0.2, 1.0]
    } else {
        [1.0, 0.3, 0.2, 1.0]
    }
}
