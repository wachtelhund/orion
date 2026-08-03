//! Game client: session flow (menus / in-game), fixed-timestep sim driving,
//! input -> Commands, and world rendering. HUD lives in hud.rs, menus in
//! menu.rs.

use std::collections::HashSet;
use std::time::Instant;

use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

use orion_sim::ai::{Bot, Difficulty};
use orion_sim::data::DefId;
use orion_sim::map::{meridian, TileKind, TilePos};
use orion_sim::state::{SimEvent, NEUTRAL, RES_GEYSER, RES_MINERALS};
use orion_sim::{Command, EntityId, EntityKind, FxVec2, GameData, Order, State};

use crate::atlas::hash2;
use crate::audio::{Audio, Sfx};
use crate::config::{Action, Settings};
use crate::gfx::{Gfx, Inst};
use crate::iso::{self, Camera};
use crate::menu::MenuPage;

pub const TICK_DT: f64 = 1.0 / 24.0;
pub const HUMAN: u8 = 0;
pub const BOT: u8 = 1;

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
}

#[derive(Clone, Copy, PartialEq)]
pub enum EffKind {
    Flash,
    Tracer,
    Spark,
    Ring,
    Corpse,
    Rubble,
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
}

/// Building sprite anchor: canvas point that sits on the building's world
/// center. [type] -> (x, y) in canvas px. hq, depot, barracks, condenser.
pub const BUILDING_ANCHOR: [(f32, f32); 7] = [
    (50.0, 54.0), // hq
    (34.0, 46.0), // depot
    (50.0, 50.0), // barracks
    (34.0, 42.0), // condenser
    (50.0, 50.0), // forge
    (50.0, 48.0), // aerie
    (35.0, 46.0), // archive
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
    pub shot_focus: Option<(f32, f32)>,
    pub shot_zoom: Option<f32>,
    pub script: Option<String>,
    pub shot_reveal: bool,
    /// Render a menu page once, capture, exit: "main" | "settings" | "esc".
    pub menu_shot: Option<(String, String)>,
    pub finished: bool,

    // Session flow.
    pub in_game: bool,
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
    pub frame_t: Instant,

    // Render-side caches (never touch sim state).
    pub facings: Vec<u8>,
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
        let start = state.map.starts[HUMAN as usize];
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
                _ => 5,
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
                _ => 6,
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
            shot_bot0: shot.is_some().then(|| Bot::new(HUMAN)),
            shot,
            shot_focus: None,
            shot_zoom: None,
            script: None,
            shot_reveal: false,
            menu_shot: None,
            finished: false,
            in_game: headless,
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
            frame_t: Instant::now(),
            facings: Vec::new(),
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

    /// Init audio (skipped in headless capture modes).
    pub fn init_audio(&mut self) {
        self.audio = Audio::new(self.settings.music_volume, self.settings.sfx_volume);
    }

    pub(crate) fn sfx(&self, s: Sfx) {
        if let Some(a) = &self.audio {
            a.play(s);
        }
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
                if e.owner == HUMAN
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
                    if e.kind == kind && e.def == def && e.owner == HUMAN {
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
        self.state = new_game();
        self.bot = Bot::with(BOT, difficulty);
        self.difficulty = difficulty;
        self.pending.clear();
        self.selection.clear();
        self.groups = Default::default();
        self.mode = Mode::Normal;
        self.effects.clear();
        self.facings.clear();
        self.subgroup_offset = 0;
        self.acc = 0.0;
        self.in_game = true;
        self.page = MenuPage::None;
        let start = self.state.map.starts[HUMAN as usize];
        let (cx, cy) = iso::world_to_iso(start.x as f32 + 0.5, start.y as f32 + 0.5);
        self.cam.cx = cx;
        self.cam.cy = cy;
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
                    self.cam.zoom =
                        (self.cam.zoom * (1.0 + dy * 0.1)).clamp(1.0, 4.0 * self.base_scale);
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

        // Restart after game over.
        if code == KeyCode::KeyR && self.state.winner.is_some() {
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
        if self.any_selected_siege() && self.settings.key_for(Action::SiegeToggle) == code {
            return Some(Action::SiegeToggle);
        }
        if self.any_selected_caster() && self.settings.key_for(Action::CastStorm) == code {
            return Some(Action::CastStorm);
        }
        self.settings.action_for(code)
    }

    pub(crate) fn any_selected_siege(&self) -> bool {
        self.own_selected_units().any(|id| {
            let e = &self.state.entities[id.idx as usize];
            self.state.data.units[e.def as usize].weapon_siege.is_some()
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
            MenuPage::Difficulty | MenuPage::Multiplayer => self.page = MenuPage::MainRoot,
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
                    self.pending.push((HUMAN, Command::Stop { units }));
                }
                self.mode = Mode::Normal;
            }
            Action::Hold => {
                let units: Vec<_> = self.own_selected_units().collect();
                if !units.is_empty() {
                    self.pending.push((HUMAN, Command::Hold { units }));
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
                    let tag = match action {
                        Action::Place1 => "depot",
                        Action::Place2 => "barracks",
                        Action::Place3 => "hq",
                        Action::Place4 => "condenser",
                        Action::Place5 => "forge",
                        Action::Place6 => "aerie",
                        _ => "archive",
                    };
                    let def = self.state.data.building_tag(tag);
                    // Only enter placement if the tech requirement is met.
                    let requires = self.state.data.buildings[def as usize].requires;
                    if self.state.requirement_met(HUMAN, requires) {
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
                let units: Vec<_> = self
                    .own_selected_units()
                    .filter(|id| {
                        let e = &self.state.entities[id.idx as usize];
                        self.state.data.units[e.def as usize].weapon_siege.is_some()
                    })
                    .collect();
                if !units.is_empty() {
                    self.pending.push((HUMAN, Command::Siege { units }));
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
                        && e.owner == HUMAN
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
                            && e.owner == HUMAN
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
                            .push((HUMAN, Command::CancelConstruction { building: id }));
                    }
                }
            }
            Action::RevealFog => self.reveal_all = !self.reveal_all,
        }
    }

    fn left_button(&mut self, down: bool) {
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
                            HUMAN,
                            Command::AttackMove {
                                units,
                                target: fx(wx, wy),
                                queued: self.shift,
                            },
                        ));
                        self.ping(wx, wy, 1);
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
                            .push((HUMAN, Command::Cast { caster, target: fx(wx, wy) }));
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
                    && e.owner == HUMAN
                    && (e.kind == EntityKind::Unit || e.kind == EntityKind::Building)
                {
                    let def = e.def;
                    let kind = e.kind;
                    for i in 0..self.state.entities.len() {
                        let o = &self.state.entities[i];
                        if o.alive && o.owner == HUMAN && o.kind == kind && o.def == def {
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
            for i in 0..self.state.entities.len() {
                let e = &self.state.entities[i];
                if !e.alive || e.owner != HUMAN || e.kind != EntityKind::Unit {
                    continue;
                }
                let (sx, sy) = self.entity_screen_pos(i);
                if sx >= x0 && sx <= x1 && sy >= y0 && sy <= y1 {
                    picked.push(self.state.id_of(i as u32));
                }
            }
        }
        if picked.is_empty() && !is_click {
            return;
        }
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
                        .push((HUMAN, Command::SetRally { building: bid, target: fx(wx, wy) }));
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
            let gatherable = self.state.gatherable(HUMAN, tid);
            if gatherable {
                let (harv, rest): (Vec<_>, Vec<_>) = units.into_iter().partition(|id| {
                    let u = &self.state.entities[id.idx as usize];
                    self.state.data.units[u.def as usize].harvester
                });
                if !harv.is_empty() {
                    self.pending.push((HUMAN, Command::Gather {
                        units: harv,
                        resource: tid,
                        queued,
                    }));
                    self.ping(wx, wy, 2);
                }
                if !rest.is_empty() {
                    self.pending.push((HUMAN, Command::Move {
                        units: rest,
                        target: fx(wx, wy),
                        queued,
                    }));
                }
                return;
            }
            if e.owner != HUMAN && e.owner != NEUTRAL {
                self.pending.push((HUMAN, Command::AttackTarget { units, target: tid }));
                self.ping(wx, wy, 1);
                return;
            }
            // Right-click own unfinished building with a builder: resume it.
            if e.owner == HUMAN && e.kind == EntityKind::Building && e.construction.is_some() {
                let def = e.def;
                let site = self.state.footprint_origin(def, e.pos);
                if let Some(builder) = units.iter().copied().find(|id| {
                    let u = &self.state.entities[id.idx as usize];
                    self.state.data.units[u.def as usize].builder
                }) {
                    self.pending
                        .push((HUMAN, Command::Build { worker: builder, building: def, site, queued }));
                    let rest: Vec<_> = units.into_iter().filter(|id| *id != builder).collect();
                    if !rest.is_empty() {
                        self.pending.push((HUMAN, Command::Move {
                            units: rest,
                            target: fx(wx, wy),
                            queued,
                        }));
                    }
                    self.ping(wx, wy, 2);
                    return;
                }
            }
        }
        self.pending.push((HUMAN, Command::Move { units, target: fx(wx, wy), queued }));
        self.ping(wx, wy, 0);
        self.sfx(Sfx::Ping);
    }

    pub(crate) fn ping(&mut self, wx: f32, wy: f32, kind: i32) {
        self.effects.push(Effect {
            kind: EffKind::Ping,
            ax: wx,
            ay: wy,
            bx: kind as f32,
            by: 0.0,
            age: 0.0,
            ttl: 0.5,
        });
    }

    fn try_place(&mut self, def: DefId) {
        let Some(builder) = self.selected_builder() else {
            self.mode = Mode::Normal;
            return;
        };
        let site = self.hovered_site(def);
        if !self.state.valid_building_site(def, site, Some(builder.idx)) {
            self.deny("CANNOT BUILD THERE");
            return;
        }
        let d = &self.state.data.buildings[def as usize];
        let p = &self.state.players[HUMAN as usize];
        if p.minerals < d.cost_minerals {
            self.deny("NOT ENOUGH MINERALS");
            return;
        }
        if p.gas < d.cost_gas {
            self.deny("NOT ENOUGH PLASMA");
            return;
        }
        // Shift both chains placement mode and queues the build order.
        self.pending.push((HUMAN, Command::Build {
            worker: builder,
            building: def,
            site,
            queued: self.shift,
        }));
        self.sfx(Sfx::Click);
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
        if !self.state.requirement_met(HUMAN, d.requires) {
            let req = self.state.data.buildings[d.requires.unwrap() as usize].name.clone();
            self.deny(&format!("REQUIRES {}", req.to_uppercase()));
            return;
        }
        let p = &self.state.players[HUMAN as usize];
        if p.minerals < d.cost_minerals {
            self.deny("NOT ENOUGH MINERALS");
            return;
        }
        if p.gas < d.cost_gas {
            self.deny("NOT ENOUGH PLASMA");
            return;
        }
        let (used, provided) = self.state.supply(HUMAN);
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
            self.pending.push((HUMAN, Command::Train { building: bid, unit }));
            self.sfx(Sfx::Click);
        }
    }

    pub(crate) fn cancel_train(&mut self, slot: usize) {
        if let Some(bid) = self.primary_selected_building() {
            self.pending
                .push((HUMAN, Command::CancelTrain { building: bid, slot: slot as u8 }));
        }
    }

    // ------------------------------------------------------- selection ----

    pub(crate) fn own_selected_units(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.selection.iter().copied().filter(|id| {
            self.state
                .get(*id)
                .is_some_and(|e| e.owner == HUMAN && e.kind == EntityKind::Unit)
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
                    .is_some_and(|e| e.owner == HUMAN && e.kind == EntityKind::Building)
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
            if e.owner != HUMAN
                && e.kind != EntityKind::Resource
                && !self.reveal_all
                && !self.state.visible_to(HUMAN, i as u32)
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
                    let h = self.gfx.book.building_px_h[btype] as f32;
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
        self.facings.resize(self.state.entities.len(), 2);
        for i in 0..self.state.entities.len() {
            let e = &self.state.entities[i];
            if !e.alive || e.kind != EntityKind::Unit {
                continue;
            }
            let dx = e.pos.x.to_f32() - e.prev_pos.x.to_f32();
            let dy = e.pos.y.to_f32() - e.prev_pos.y.to_f32();
            if dx * dx + dy * dy > 1e-6 {
                let (ix, iy) = ((dx - dy) * iso::TILE_HALF_W, (dx + dy) * iso::TILE_HALF_H);
                let ang = iy.atan2(ix);
                let sector = ((ang / std::f32::consts::FRAC_PI_4).round() as i32).rem_euclid(8);
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
                    if t.owner == HUMAN {
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
                                ttl: 2.0,
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
                        || self.state.fog[HUMAN as usize].visible(&self.state.map, ft)
                        || self.state.fog[HUMAN as usize].visible(&self.state.map, tt);
                    if !seen {
                        continue;
                    }
                    let (fx_, fy_) = (f.pos.x.to_f32(), f.pos.y.to_f32());
                    let (tx_, ty_) = (t.pos.x.to_f32(), t.pos.y.to_f32());
                    let d = ((tx_ - fx_).powi(2) + (ty_ - fy_).powi(2)).sqrt().max(0.001);
                    let r = self.state.radius_of(from).to_f32() + 0.2;
                    let mx = fx_ + (tx_ - fx_) / d * r;
                    let my = fy_ + (ty_ - fy_) / d * r;
                    if combat_sounds < 3 {
                        combat_sounds += 1;
                        let sieged = self.state.entities[from as usize].sieged;
                        self.sfx(if sieged { Sfx::Cannon } else { Sfx::Shot });
                    }
                    self.effects.push(Effect {
                        kind: EffKind::Flash,
                        ax: mx,
                        ay: my,
                        bx: 0.0,
                        by: 0.0,
                        age: 0.0,
                        ttl: 0.09,
                    });
                    if d > 1.2 {
                        self.effects.push(Effect {
                            kind: EffKind::Tracer,
                            ax: mx,
                            ay: my,
                            bx: tx_,
                            by: ty_,
                            age: 0.0,
                            ttl: 0.05,
                        });
                    }
                    self.effects.push(Effect {
                        kind: EffKind::Spark,
                        ax: tx_,
                        ay: ty_,
                        bx: 0.0,
                        by: 0.0,
                        age: 0.0,
                        ttl: 0.16,
                    });
                }
                SimEvent::Ready { pos: _, owner } => {
                    if owner == HUMAN {
                        self.sfx(Sfx::UnitReady);
                    }
                }
                SimEvent::BuildingDone { pos: _, owner } => {
                    if owner == HUMAN {
                        self.sfx(Sfx::BuildDone);
                    }
                }
                SimEvent::ResearchDone { owner } => {
                    if owner == HUMAN {
                        self.sfx(Sfx::ResearchDone);
                    }
                }
                SimEvent::Cast { pos } => {
                    self.sfx(Sfx::Storm);
                    let (x, y) = (pos.x.to_f32(), pos.y.to_f32());
                    self.effects.push(Effect {
                        kind: EffKind::Ring,
                        ax: x,
                        ay: y,
                        bx: 2.0,
                        by: 0.0,
                        age: 0.0,
                        ttl: 0.4,
                    });
                }
                SimEvent::Death { pos, kind, .. } => {
                    let t = TilePos::of(pos);
                    let seen = self.reveal_all
                        || self.state.fog[HUMAN as usize].visible(&self.state.map, t);
                    if !seen {
                        continue;
                    }
                    if death_sounds < 2 {
                        death_sounds += 1;
                        self.sfx(if kind == EntityKind::Building {
                            Sfx::BigExplosion
                        } else {
                            Sfx::Explosion
                        });
                    }
                    let (x, y) = (pos.x.to_f32(), pos.y.to_f32());
                    self.effects.push(Effect {
                        kind: EffKind::Ring,
                        ax: x,
                        ay: y,
                        bx: if kind == EntityKind::Building { 2.0 } else { 1.0 },
                        by: 0.0,
                        age: 0.0,
                        ttl: 0.35,
                    });
                    self.effects.push(Effect {
                        kind: if kind == EntityKind::Building {
                            EffKind::Rubble
                        } else {
                            EffKind::Corpse
                        },
                        ax: x,
                        ay: y,
                        bx: 0.0,
                        by: 0.0,
                        age: 0.0,
                        ttl: if kind == EntityKind::Building { 30.0 } else { 8.0 },
                    });
                }
            }
        }
    }

    pub fn frame(&mut self) {
        if let Some((page, path)) = self.menu_shot.clone() {
            match page.as_str() {
                "settings" => self.page = MenuPage::Settings { from_game: false },
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
                        e.alive && e.owner == HUMAN && e.kind == EntityKind::Unit
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
                    let start = self.state.map.starts[HUMAN as usize];
                    let archive = self.state.data.building_tag("archive");
                    let a = self.state.spawn_building(
                        HUMAN,
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
                "units" => {
                    // Showcase: one of each new unit + a storm, selected.
                    self.in_game = true;
                    self.page = MenuPage::None;
                    let s = &mut self.state;
                    let start = s.map.starts[HUMAN as usize];
                    let (bx, by) = (start.x + 4, start.y + 2);
                    let breaker = s.data.unit_tag("breaker");
                    let sky = s.data.unit_tag("skywing");
                    let caster = s.data.unit_tag("stormcaller");
                    let b1 = s.spawn_unit(HUMAN, breaker, orion_sim::FxVec2::from_int(bx, by));
                    let b2 =
                        s.spawn_unit(HUMAN, breaker, orion_sim::FxVec2::from_int(bx + 3, by));
                    s.entities[b2.idx as usize].sieged = true;
                    let f1 =
                        s.spawn_unit(HUMAN, sky, orion_sim::FxVec2::from_int(bx + 1, by + 3));
                    let c1 =
                        s.spawn_unit(HUMAN, caster, orion_sim::FxVec2::from_int(bx - 2, by + 2));
                    s.entities[c1.idx as usize].energy = 143;
                    s.storms.push(orion_sim::state::Storm {
                        pos: orion_sim::FxVec2::from_int(bx + 6, by + 4),
                        ticks_left: 60,
                        owner: HUMAN,
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
        if let Some((target, path)) = self.shot.clone() {
            let chunk = 48u32;
            for _ in 0..chunk {
                if self.state.tick >= target || self.state.winner.is_some() {
                    break;
                }
                let mut cmds = Vec::new();
                if let Some(b0) = self.shot_bot0.as_mut() {
                    cmds.extend(b0.think(&self.state));
                }
                cmds.extend(self.bot.think(&self.state));
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

        let playing = self.in_game && self.page == MenuPage::None;

        if playing {
            // Keyboard pan.
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
            // Edge scroll.
            if self.settings.edge_scroll && self.cursor_in && self.panning.is_none() {
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

            if self.state.winner.is_none() {
                self.acc += dt * self.settings.game_speed as f64;
                while self.acc >= TICK_DT {
                    let mut cmds = std::mem::take(&mut self.pending);
                    cmds.extend(self.bot.think(&self.state));
                    self.step_sim(cmds);
                    self.acc -= TICK_DT;
                }
            }

            for e in &mut self.effects {
                e.age += dt as f32;
            }
            self.effects.retain(|e| e.age < e.ttl);
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
        let captures: [(u32, &str); 4] =
            [(300, "econ"), (1500, "build"), (3100, "army"), (4800, "attack")];
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
            cmds.extend(self.bot.think(&self.state));
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
            .filter(|e| e.alive && e.owner == HUMAN && e.kind == EntityKind::Building)
            .count();
        assert!(own_buildings >= 4, "script: expected HQ+depot+condenser+barracks, got {own_buildings}");
        assert!(
            self.state.players[HUMAN as usize].gas > 0
                || self
                    .state
                    .entities
                    .iter()
                    .any(|e| e.alive && e.owner == HUMAN && e.carry_gas),
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
                    cmds.push((HUMAN, Command::Gather {
                        units: workers,
                        resource: res,
                        queued: false,
                    }));
                }
            }
            30 | 31 => {
                if let Some(hq) = self.own_building_tagged("hq") {
                    let w = self.state.data.unit_tag("fabricator");
                    cmds.push((HUMAN, Command::Train { building: hq, unit: w }));
                }
            }
            360 => self.script_build("depot", cmds),
            700 => self.script_build_extractor(cmds),
            1400 => self.script_build("barracks", cmds),
            // Staff the condenser once it should be done.
            1500 => {
                if let Some(c) = self.own_building_tagged("condenser") {
                    if self.state.gatherable(HUMAN, c) {
                        let workers = self.own_units_of(|d| d.harvester);
                        let staff: Vec<_> = workers.into_iter().take(2).collect();
                        if !staff.is_empty() {
                            cmds.push((HUMAN, Command::Gather {
                                units: staff,
                                resource: c,
                                queued: false,
                            }));
                        }
                    }
                }
            }
            2500 | 2540 | 2580 | 2620 => {
                if let Some(b) = self.own_building_tagged("barracks") {
                    let tr = self.state.data.unit_tag("trooper");
                    cmds.push((HUMAN, Command::Train { building: b, unit: tr }));
                }
            }
            3150 => {
                let army = self.own_units_of(|d| !d.harvester);
                if !army.is_empty() {
                    cmds.push((HUMAN, Command::AttackMove {
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
                        cmds.push((HUMAN, Command::Build { worker, building: def, site, queued: false }));
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
                cmds.push((HUMAN, Command::Build { worker, building: def, site: origin, queued: false }));
                return;
            }
        }
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
                    && e.owner == HUMAN
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
            (e.alive && e.owner == HUMAN && e.kind == EntityKind::Building && e.def == def)
                .then(|| self.state.id_of(i))
        })
    }

    fn nearest_mineral_to_start(&self) -> Option<EntityId> {
        let start = self.state.map.starts[HUMAN as usize].center();
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
        self.draw_terrain(&mut out);
        self.draw_ground_effects(&mut out);
        self.draw_pending_builds(&mut out);
        self.draw_entities(&mut out);
        self.draw_air_effects(&mut out);
        self.draw_waypoints(&mut out);
        self.draw_placement_ghost(&mut out);
        self.draw_selection_box(&mut out);
        self.draw_hud(&mut out);
        self.draw_menu(&mut out);
        self.gfx.render(&out);
    }

    pub(crate) fn visible(&self, t: TilePos) -> bool {
        self.reveal_all || self.state.fog[HUMAN as usize].visible(&self.state.map, t)
    }

    pub(crate) fn explored(&self, t: TilePos) -> bool {
        self.reveal_all || self.state.fog[HUMAN as usize].explored(&self.state.map, t)
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
                _ => {}
            }
        }
    }

    fn draw_entities(&self, out: &mut Vec<Inst>) {
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
                _ if e.owner == HUMAN => true,
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
            let dim = if e.owner == HUMAN { 1.0 } else { self.fog_tint(t) };
            let tint = [dim, dim, dim, 1.0];
            match e.kind {
                EntityKind::Resource => {
                    if e.def == RES_GEYSER {
                        let r = book.geyser;
                        self.gfx.sprite(out, r, sx, sy - 6.0 * zoom, r.w as f32 * zoom, r.h as f32 * zoom, tint);
                    } else {
                        let variant = if e.amount > 900 {
                            0
                        } else if e.amount > 350 {
                            1
                        } else {
                            2
                        };
                        let r = book.minerals[variant];
                        self.gfx.sprite(out, r, sx, sy - 5.0 * zoom, r.w as f32 * zoom, r.h as f32 * zoom, tint);
                    }
                }
                EntityKind::Building => {
                    let btype = self.building_type[e.def as usize];
                    let team = (e.owner as usize).min(1);
                    let r = book.building(btype, team);
                    let (ax, ay) = BUILDING_ANCHOR[btype];
                    if selected {
                        let d = &self.state.data.buildings[e.def as usize];
                        let w = d.footprint.0 as f32 * iso::TILE_HALF_W * 2.0 * zoom;
                        self.gfx.sprite(out, book.diamond_outline, sx, sy, w + 8.0, w * 0.5 + 4.0, [0.4, 1.0, 0.4, 0.9]);
                    }
                    let alpha = if e.construction.is_some() { 0.6 } else { 1.0 };
                    self.gfx.sprite(
                        out,
                        r,
                        sx + (r.w as f32 * 0.5 - ax) * zoom,
                        sy + (r.h as f32 * 0.5 - ay) * zoom,
                        r.w as f32 * zoom,
                        r.h as f32 * zoom,
                        [tint[0], tint[1], tint[2], alpha],
                    );
                    let d = &self.state.data.buildings[e.def as usize];
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
                        let h_px = book.building_px_h[btype] as f32;
                        self.bar(out, sx, sy - (h_px * 0.75) * zoom, 46.0, hp_frac, hp_color(hp_frac));
                    }
                }
                EntityKind::Unit => {
                    let d = &self.state.data.units[e.def as usize];
                    // Sieged breakers use their deployed sprite set.
                    let utype = if e.sieged { 6 } else { self.unit_type[e.def as usize] };
                    let team = (e.owner as usize).min(1);
                    let facing = self.facings.get(i as usize).copied().unwrap_or(2) as usize;
                    let moving = e.pos != e.prev_pos;
                    let frame = if moving || d.fly {
                        ((self.state.tick / 4) % 2) as usize
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
                    self.gfx.sprite(out, book.circle, sx, sy + 2.0 * zoom, sw, sw * 0.45, [0.0, 0.0, 0.0, shadow_a]);
                    if selected {
                        let rw = sw + 8.0 * zoom;
                        self.gfx.sprite(out, book.ring, sx, sy + 2.0 * zoom, rw, rw * 0.5, [0.35, 1.0, 0.35, 0.9]);
                    }
                    self.gfx.sprite(
                        out,
                        r,
                        sx,
                        sy - (r.h as f32 * 0.5 - 4.0) * zoom - hover,
                        r.w as f32 * zoom,
                        r.h as f32 * zoom,
                        tint,
                    );
                    if e.amount > 0 {
                        let c = if e.carry_gas { GAS_COLOR } else { MINERAL_COLOR };
                        self.gfx.sprite(out, book.spark, sx + 5.0 * zoom, sy - 14.0 * zoom, 5.0 * zoom, 5.0 * zoom, [c[0], c[1], c[2], 1.0]);
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

    fn draw_air_effects(&self, out: &mut Vec<Inst>) {
        let zoom = self.cam.zoom;
        let book = &self.gfx.book;
        // Active Plasma Storms: crackling zone.
        for s in &self.state.storms {
            let t = TilePos::of(s.pos);
            if !self.visible(t) && !self.reveal_all {
                continue;
            }
            let (sx, sy) = self.world_to_screen_elev(s.pos.x.to_f32(), s.pos.y.to_f32());
            let rad = orion_sim::STORM_RADIUS.to_f32() * 32.0 * zoom;
            self.gfx.sprite(out, book.blast_ring, sx, sy, rad * 2.0, rad, [0.5, 0.9, 1.0, 0.35]);
            for k in 0..7 {
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
                    [0.7, 0.95, 1.0, 0.85],
                );
                self.gfx.sprite(out, book.spark, bx, by, 5.0 * zoom, 5.0 * zoom, [0.7, 0.95, 1.0, 0.9]);
            }
        }
        for e in &self.effects {
            let f = e.age / e.ttl;
            match e.kind {
                EffKind::Flash => {
                    let (sx, sy) = self.world_to_screen_elev(e.ax, e.ay);
                    let s = (11.0 + f * 4.0) * zoom;
                    self.gfx.sprite(out, book.flash, sx, sy - 7.0 * zoom, s, s, [1.0, 1.0, 1.0, 1.0 - f]);
                }
                EffKind::Tracer => {
                    let (x0, y0) = self.world_to_screen_elev(e.ax, e.ay);
                    let (x1, y1) = self.world_to_screen_elev(e.bx, e.by);
                    self.gfx.beam(out, x0, y0 - 7.0 * zoom, x1, y1 - 6.0 * zoom, 1.5 * zoom, [1.0, 0.95, 0.6, (1.0 - f) * 0.9]);
                }
                EffKind::Spark => {
                    let (sx, sy) = self.world_to_screen_elev(e.ax, e.ay);
                    let s = (5.0 + f * 6.0) * zoom;
                    self.gfx.sprite(out, book.spark, sx, sy - 6.0 * zoom, s, s, [1.0, 1.0, 1.0, 1.0 - f]);
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
            if !e.alive || e.owner != HUMAN || e.kind != EntityKind::Unit {
                continue;
            }
            for order in std::iter::once(&e.order).chain(e.order_queue.iter()) {
                if let Order::Build {
                    def,
                    site,
                    phase: orion_sim::state::BuildPhase::Travel,
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
            let r = self.gfx.book.building(btype, HUMAN as usize);
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
                cx + (r.w as f32 * 0.5 - ax) * zoom,
                cy + (r.h as f32 * 0.5 - ay) * zoom,
                r.w as f32 * zoom,
                r.h as f32 * zoom,
                [0.7, 0.85, 1.0, 0.38],
            );
        }
    }

    /// Waypoint lines for selected units + rally line for selected buildings.
    fn draw_waypoints(&self, out: &mut Vec<Inst>) {
        let book = &self.gfx.book;
        for id in &self.selection {
            let Some(e) = self.state.get(*id) else { continue };
            if e.owner != HUMAN {
                continue;
            }
            if e.kind == EntityKind::Unit {
                let mut prev = self.entity_screen_pos(id.idx as usize);
                for order in std::iter::once(&e.order).chain(e.order_queue.iter()) {
                    let (target, color) = match order {
                        Order::Move { target, .. } => (*target, [0.35, 1.0, 0.35, 0.35]),
                        Order::AttackMove { target, .. } => (*target, [1.0, 0.4, 0.3, 0.35]),
                        _ => continue,
                    };
                    let p = self.world_to_screen_elev(target.x.to_f32(), target.y.to_f32());
                    self.gfx.beam(out, prev.0, prev.1, p.0, p.1, 1.5, color);
                    self.gfx.sprite(out, book.diamond, p.0, p.1, 10.0, 5.0, color);
                    prev = p;
                }
            } else if e.kind == EntityKind::Building
                && e.construction.is_none()
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
        let r = self.gfx.book.building(btype, HUMAN as usize);
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
            cx + (r.w as f32 * 0.5 - ax) * zoom,
            cy + (r.h as f32 * 0.5 - ay) * zoom,
            r.w as f32 * zoom,
            r.h as f32 * zoom,
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
    // Seed fixed for now; multiplayer will exchange one in the lobby.
    State::new(GameData::load_default(), meridian(), 0xC0FFEE)
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
