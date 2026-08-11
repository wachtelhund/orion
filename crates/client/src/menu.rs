//! Main menu, pause (escape) menu, and the settings screen with keybind
//! editing. Pure widgets over the sprite renderer — layout is shared between
//! draw and click handling.

use orion_sim::ai::Difficulty;

use crate::app::{App, GAS_COLOR, MINERAL_COLOR, TEAM_COLORS};
use crate::config::{Action, ALL_ACTIONS};
use crate::gfx::Inst;

#[derive(Clone, Copy, PartialEq)]
pub enum MenuPage {
    None,
    MainRoot,
    Difficulty,
    Multiplayer,
    Ladder,
    Replays,
    UpdatePrompt,
    Settings { from_game: bool },
    EscRoot,
}

#[derive(Clone, Copy)]
enum MenuAction {
    OpenDifficulty,
    StartGame(Difficulty),
    OpenMultiplayer,
    OpenSettings,
    SettingsTab(u8),
    OpenEditor,
    Back,
    Resume,
    QuitToMenu,
    QuitApp,
    ToggleFullscreen,
    ToggleEdge,
    HudScale(f32),
    Speed(f32),
    MusicVol(f32),
    SfxVol(f32),
    CycleRace,
    CycleEnemyRace,
    CycleMap,
    CreateLobby { private: bool },
    JoinRelay,
    JoinListed(usize),
    FocusName,
    FocusCode,
    CancelMp,
    FindMatch,
    CancelSearch,
    OpenLadder,
    Noop,
    DeclineUpdate,
    OpenReplays,
    PlayReplay(usize),
    ToggleReplayShare,
    FetchReplayCode,
    FetchMapCode,
    StartTutorial,
    CreateRoom,
    StartRoomNow,
    WatchCode,
    OpenUpdate,
    Rebind(Action),
}

struct MBtn {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    label: String,
    action: MenuAction,
    small: bool,
}

impl App {
    fn menu_buttons(&self) -> Vec<MBtn> {
        let ui = self.ui();
        let w = self.cam.screen_w;
        let h = self.cam.screen_h;
        let cx = w * 0.5;
        let bw = 300.0 * ui;
        let bh = 44.0 * ui;
        let gap = 12.0 * ui;
        let mut out = Vec::new();
        let mut stack = |labels: Vec<(String, MenuAction)>, y0: f32| {
            for (k, (label, action)) in labels.into_iter().enumerate() {
                out.push(MBtn {
                    x: cx - bw * 0.5,
                    y: y0 + k as f32 * (bh + gap),
                    w: bw,
                    h: bh,
                    label,
                    action,
                    small: false,
                });
            }
        };
        match self.page {
            MenuPage::MainRoot => {
                if let Some((tag, _)) = &self.update {
                    stack(
                        vec![(
                            format!("UPDATE {} AVAILABLE - DOWNLOAD", tag.to_uppercase()),
                            MenuAction::OpenUpdate,
                        )],
                        h * 0.345,
                    );
                }
                stack(
                    vec![
                        ("TUTORIAL".into(), MenuAction::StartTutorial),
                        ("PLAY VS AI".into(), MenuAction::OpenDifficulty),
                        (
                            if self.mp_blocked {
                                "MULTIPLAYER - UPDATE REQUIRED".into()
                            } else {
                                "MULTIPLAYER".to_string()
                            },
                            MenuAction::OpenMultiplayer,
                        ),
                        ("REPLAYS".into(), MenuAction::OpenReplays),
                        ("MAP EDITOR".into(), MenuAction::OpenEditor),
                        ("SETTINGS".into(), MenuAction::OpenSettings),
                        // No process to quit in a browser tab.
                        #[cfg(not(target_arch = "wasm32"))]
                        ("QUIT".into(), MenuAction::QuitApp),
                    ],
                    h * 0.40,
                );
            }
            MenuPage::Difficulty => {
                let races = &self.state.data.race_names;
                let my = races
                    .get(self.chosen_race as usize)
                    .cloned()
                    .unwrap_or_default()
                    .to_uppercase();
                let enemy = match self.enemy_race_choice {
                    2 => "RANDOM".to_string(),
                    r => races.get(r as usize).cloned().unwrap_or_default().to_uppercase(),
                };
                stack(
                    vec![
                        (format!("YOUR RACE: {my}"), MenuAction::CycleRace),
                        (format!("ENEMY: {enemy}"), MenuAction::CycleEnemyRace),
                        (
                            format!(
                                "MAP: {}",
                                {
                                    let names = crate::app::all_map_names();
                                    names[self.map_choice % names.len()].to_uppercase()
                                }
                            ),
                            MenuAction::CycleMap,
                        ),
                        // Fetch a shared map by its 5-letter code.
                        #[cfg(not(target_arch = "wasm32"))]
                        (format!("MAP CODE: {}_", self.map_code), MenuAction::FetchMapCode),
                        ("EASY".into(), MenuAction::StartGame(Difficulty::Easy)),
                        ("NORMAL".into(), MenuAction::StartGame(Difficulty::Normal)),
                        ("HARD".into(), MenuAction::StartGame(Difficulty::Hard)),
                        ("BACK".into(), MenuAction::Back),
                    ],
                    h * 0.34,
                );
            }
            MenuPage::Multiplayer => {
                let races = &self.state.data.race_names;
                let my = races
                    .get(self.chosen_race as usize)
                    .cloned()
                    .unwrap_or_default()
                    .to_uppercase();
                if self.mm_queue.is_some() {
                    // Ranked search in progress: status + cancel.
                    stack(
                        vec![
                            (self.mm_status.clone(), MenuAction::CancelSearch),
                            ("CANCEL SEARCH".into(), MenuAction::CancelSearch),
                        ],
                        h * 0.55,
                    );
                } else if self.mp_busy() {
                    let mut rows = Vec::new();
                    #[cfg(not(target_arch = "wasm32"))]
                    if self.room_start_tx.is_some() && self.room_waiting.is_some() {
                        rows.push((
                            "START NOW - BOTS FILL EMPTY SEATS".to_string(),
                            MenuAction::StartRoomNow,
                        ));
                    }
                    rows.push(("CANCEL".into(), MenuAction::CancelMp));
                    stack(rows, h * 0.58);
                } else {
                    let name_marker = if self.name_focus { "_" } else { "" };
                    let code_marker = if self.name_focus { "" } else { "_" };
                    let find = match self.mm_rating {
                        Some((mmr, games)) => {
                            format!("FIND MATCH  (MMR {mmr} - {games} GAMES)")
                        }
                        None => "FIND MATCH".to_string(),
                    };
                    // Ranked play needs the desktop build (threads + queue
                    // socket) — say so instead of dangling dead buttons.
                    #[cfg(target_arch = "wasm32")]
                    let (find, find_act) =
                        ("RANKED: DESKTOP BUILD ONLY".to_string(), MenuAction::Noop);
                    #[cfg(not(target_arch = "wasm32"))]
                    let find_act = MenuAction::FindMatch;
                    let mut rows = vec![
                        (find, find_act),
                        #[cfg(not(target_arch = "wasm32"))]
                        ("LADDER".into(), MenuAction::OpenLadder),
                        (
                            format!("NAME: {}{}", self.settings.player_name, name_marker),
                            MenuAction::FocusName,
                        ),
                        (format!("YOUR RACE: {my}"), MenuAction::CycleRace),
                        (
                            format!(
                                "MAP: {}",
                                {
                                    let names = crate::app::all_map_names();
                                    names[self.map_choice % names.len()].to_uppercase()
                                }
                            ),
                            MenuAction::CycleMap,
                        ),
                        ("CREATE LOBBY".into(), MenuAction::CreateLobby { private: false }),
                        (
                            "CREATE PRIVATE LOBBY".into(),
                            MenuAction::CreateLobby { private: true },
                        ),
                        #[cfg(not(target_arch = "wasm32"))]
                        ("CREATE 2V2 ROOM".into(), MenuAction::CreateRoom),
                    ];
                    // Public lobbies, click to join.
                    for (k, l) in self.lobby_list.iter().take(4).enumerate() {
                        let race = self
                            .state
                            .data
                            .race_names
                            .get(l.race as usize)
                            .map(|r| r.split_whitespace().next().unwrap_or("").to_string())
                            .unwrap_or_default()
                            .to_uppercase();
                        let label = if l.slots > 2 {
                            format!("JOIN 2V2  {}  {}/{}", l.name, l.filled, l.slots)
                        } else {
                            format!("JOIN  {}  ({race})", l.name)
                        };
                        rows.push((
                            label,
                            MenuAction::JoinListed(k),
                        ));
                    }
                    rows.push((
                        format!("PRIVATE CODE: {}{}", self.code_input, code_marker),
                        MenuAction::FocusCode,
                    ));
                    if self.code_input.trim().len() >= 4 {
                        rows.push(("JOIN PRIVATE".into(), MenuAction::JoinRelay));
                        // Silent spectator tap on the same code.
                        #[cfg(not(target_arch = "wasm32"))]
                        rows.push(("WATCH THIS CODE".into(), MenuAction::WatchCode));
                    }
                    rows.push(("BACK".into(), MenuAction::Back));
                    stack(rows, h * 0.24);
                }
            }
            MenuPage::UpdatePrompt => {
                let tag = self
                    .update
                    .as_ref()
                    .map(|(t, _)| t.to_uppercase())
                    .unwrap_or_default();
                stack(
                    vec![
                        (format!("DOWNLOAD UPDATE {tag}"), MenuAction::OpenUpdate),
                        (
                            "CONTINUE - SINGLEPLAYER ONLY".into(),
                            MenuAction::DeclineUpdate,
                        ),
                    ],
                    h * 0.44,
                );
            }
            MenuPage::Ladder => {
                let mut rows: Vec<(String, MenuAction)> = Vec::new();
                match &self.ladder {
                    None => rows.push(("FETCHING LADDER...".into(), MenuAction::Noop)),
                    Some(list) if list.is_empty() => {
                        rows.push(("NO RANKED GAMES PLAYED YET".into(), MenuAction::Noop))
                    }
                    Some(list) => {
                        let me = &self.settings.player_id;
                        for (k, r) in list.iter().take(10).enumerate() {
                            let you = me.starts_with(&r.id);
                            let mark = if you { "  < YOU" } else { "" };
                            rows.push((
                                format!(
                                    "{}. {}  {} MMR  ({} GAMES){}",
                                    k + 1,
                                    r.name,
                                    r.mmr,
                                    r.games,
                                    mark
                                ),
                                MenuAction::Noop,
                            ));
                        }
                    }
                }
                rows.push(("BACK".into(), MenuAction::Back));
                stack(rows, h * 0.26);
            }
            MenuPage::Replays => {
                let mut rows: Vec<(String, MenuAction)> = self
                    .replay_files
                    .iter()
                    .enumerate()
                    .map(|(k, (label, _))| (label.clone(), MenuAction::PlayReplay(k)))
                    .collect();
                if rows.is_empty() {
                    rows.push(("NO REPLAYS YET - PLAY A GAME".into(), MenuAction::Back));
                }
                // Uploading needs local files — desktop builds only.
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let share = if self.replay_share_mode {
                        "SHARING: CLICK A REPLAY ABOVE".to_string()
                    } else {
                        "SHARE A REPLAY - GET A CODE".to_string()
                    };
                    rows.push((share, MenuAction::ToggleReplayShare));
                }
                // Fetching by code works everywhere; the browser plays the
                // replay directly instead of saving a file.
                rows.push((
                    format!("FETCH CODE: {}_", self.replay_code),
                    MenuAction::FetchReplayCode,
                ));
                rows.push(("BACK".into(), MenuAction::Back));
                stack(rows, h * 0.30);
            }
            MenuPage::EscRoot => {
                stack(
                    vec![
                        ("RESUME".into(), MenuAction::Resume),
                        ("SETTINGS".into(), MenuAction::OpenSettings),
                        ("QUIT TO MENU".into(), MenuAction::QuitToMenu),
                        #[cfg(not(target_arch = "wasm32"))]
                        ("QUIT GAME".into(), MenuAction::QuitApp),
                    ],
                    h * 0.38,
                );
            }
            MenuPage::Settings { .. } => {
                // Two tabs: GENERAL (toggles + sliders) and HOTKEYS (binds).
                // One page of options ran off screen (user report).
                let s = &self.settings;
                let y0 = h * 0.175;
                let sbh = 36.0 * ui;
                let sgap = 6.0 * ui;
                let tab_w = 150.0 * ui;
                for (k, label) in ["GENERAL", "HOTKEYS"].iter().enumerate() {
                    let sel = self.settings_tab == k as u8;
                    out.push(MBtn {
                        x: cx - tab_w - 6.0 * ui + k as f32 * (tab_w + 12.0 * ui),
                        y: y0,
                        w: tab_w,
                        h: sbh,
                        label: if sel { format!("[ {label} ]") } else { (*label).into() },
                        action: MenuAction::SettingsTab(k as u8),
                        small: true,
                    });
                }
                let c0 = y0 + sbh + 18.0 * ui;
                let mut back_y = c0;
                if self.settings_tab == 0 {
                    for (k, (label, action)) in [
                        (
                            format!("FULLSCREEN: {}", if s.fullscreen { "ON" } else { "OFF" }),
                            MenuAction::ToggleFullscreen,
                        ),
                        (
                            format!("EDGE SCROLL: {}", if s.edge_scroll { "ON" } else { "OFF" }),
                            MenuAction::ToggleEdge,
                        ),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        out.push(MBtn {
                            x: cx - bw * 0.5,
                            y: c0 + k as f32 * (sbh + sgap),
                            w: bw,
                            h: sbh,
                            label,
                            action,
                            small: true,
                        });
                    }
                    // HUD size / speed / volumes with -/+ buttons.
                    let small_w = 40.0 * ui;
                    let row_y = c0 + 2.0 * (sbh + sgap);
                    for (k, (_, dv, act)) in [
                        ("HUD SIZE", 0.1f32, 0),
                        ("GAME SPEED", 0.25, 1),
                        ("MUSIC", 0.1, 2),
                        ("SOUND", 0.1, 3),
                    ]
                    .iter()
                    .enumerate()
                    {
                        let y = row_y + k as f32 * (sbh + sgap);
                        let mk = |d: f32| match *act {
                            0 => MenuAction::HudScale(d * dv),
                            1 => MenuAction::Speed(d * dv),
                            2 => MenuAction::MusicVol(d * dv),
                            _ => MenuAction::SfxVol(d * dv),
                        };
                        out.push(MBtn {
                            x: cx - bw * 0.5,
                            y,
                            w: small_w,
                            h: sbh,
                            label: "-".into(),
                            action: mk(-1.0),
                            small: true,
                        });
                        out.push(MBtn {
                            x: cx + bw * 0.5 - small_w,
                            y,
                            w: small_w,
                            h: sbh,
                            label: "+".into(),
                            action: mk(1.0),
                            small: true,
                        });
                    }
                    back_y = row_y + 4.0 * (sbh + sgap) + 18.0 * ui;
                } else {
                    // Keybind grid: 2 columns.
                    let ky0 = c0 + 30.0 * ui;
                    let kbh = 22.0 * ui;
                    let col_w = 330.0 * ui;
                    for (k, a) in ALL_ACTIONS.iter().enumerate() {
                        let col = k % 2;
                        let row = k / 2;
                        out.push(MBtn {
                            x: cx - col_w + col as f32 * col_w + col_w - 90.0 * ui,
                            y: ky0 + row as f32 * (kbh + 4.0 * ui),
                            w: 84.0 * ui,
                            h: kbh,
                            label: if self.rebinding == Some(*a) {
                                "???".into()
                            } else {
                                crate::config::key_label(self.settings.key_for(*a))
                            },
                            action: MenuAction::Rebind(*a),
                            small: true,
                        });
                    }
                    let n_rows = (ALL_ACTIONS.len() as f32 / 2.0).ceil();
                    back_y = ky0 + n_rows * (kbh + 4.0 * ui) + 18.0 * ui;
                }
                out.push(MBtn {
                    x: cx - bw * 0.5,
                    y: back_y,
                    w: bw,
                    h: 38.0 * ui,
                    label: "BACK".into(),
                    action: MenuAction::Back,
                    small: false,
                });
            }
            MenuPage::None => {}
        }
        out
    }

    pub(crate) fn draw_menu(&self, out: &mut Vec<Inst>) {
        if self.page == MenuPage::None {
            return;
        }
        let ui = self.ui();
        let w = self.cam.screen_w;
        let h = self.cam.screen_h;
        let cx = w * 0.5;
        self.gfx.quad(out, 0.0, 0.0, w, h, [0.01, 0.01, 0.02, 0.72]);

        let white = [0.92, 0.92, 0.88, 1.0];
        let dim = [0.66, 0.7, 0.76, 1.0];
        let gold = [0.95, 0.78, 0.25, 1.0];
        let book = &self.gfx.book;

        // Dialog panel: navy tech plate framed in gold, sized around the
        // button stack (plus headroom for page copy).
        let buttons = self.menu_buttons();
        if !buttons.is_empty() && self.page != MenuPage::MainRoot {
            let mut x0 = f32::MAX;
            let mut y0 = f32::MAX;
            let mut x1 = f32::MIN;
            let mut y1 = f32::MIN;
            for b in &buttons {
                x0 = x0.min(b.x);
                y0 = y0.min(b.y);
                x1 = x1.max(b.x + b.w);
                y1 = y1.max(b.y + b.h);
            }
            let pad = 26.0 * ui;
            // Headroom above the stack only where copy actually lives there
            // (a fixed 64ui left a dead zone on plain button pages).
            let mut head = match self.page {
                MenuPage::Multiplayer | MenuPage::Replays => 64.0 * ui,
                MenuPage::UpdatePrompt => 40.0 * ui,
                _ => 16.0 * ui,
            };
            if matches!(self.page, MenuPage::Settings { .. }) && self.settings_tab == 1 {
                // The keybind label columns are text, not buttons — the
                // panel must cover them too.
                x0 = x0.min(cx - 345.0 * ui);
                x1 = x1.max(cx + 345.0 * ui);
            }
            // Pages with intro copy: the panel must be wide enough for the
            // longest line (it used to cut straight through the text).
            if self.page == MenuPage::Multiplayer {
                let ts = self.ts(1.5);
                let copy_w = self
                    .multiplayer_copy()
                    .iter()
                    .map(|l| self.gfx.text_width(ts, l))
                    .fold(0.0f32, f32::max);
                x0 = x0.min(cx - copy_w * 0.5 - 14.0 * ui);
                x1 = x1.max(cx + copy_w * 0.5 + 14.0 * ui);
                head = head.max(h * 0.24 - (y0 - pad) + 60.0 * ui);
            }
            let (px, py) = (x0 - pad, y0 - pad - head);
            let (pw, ph) = (x1 - x0 + pad * 2.0, y1 - y0 + pad * 2.0 + head);
            self.gfx
                .sprite(out, book.chrome_panel, px + pw * 0.5, py + ph * 0.5, pw, ph, [1.0, 1.0, 1.0, 0.97]);
            self.gold_frame_menu(out, px, py, pw, ph);
            for (rx, ry) in
                [(px + 10.0 * ui, py + 10.0 * ui), (px + pw - 10.0 * ui, py + 10.0 * ui),
                 (px + 10.0 * ui, py + ph - 10.0 * ui), (px + pw - 10.0 * ui, py + ph - 10.0 * ui)]
            {
                let s = 6.0 * ui;
                self.gfx.sprite(out, book.rivet, rx, ry, s, s, [1.0, 1.0, 1.0, 1.0]);
            }
            // Map preview beside the picker wherever a MAP row exists.
            let has_map_row = self.page == MenuPage::Difficulty
                || (self.page == MenuPage::Multiplayer
                    && self.mp_waiting.is_none()
                    && self.mm_queue.is_none());
            if has_map_row {
                let ts = 170.0 * ui;
                let tx = px + pw + 24.0 * ui;
                if tx + ts + 12.0 * ui < w {
                    self.draw_map_thumb(out, tx, py + head.max(26.0 * ui), ts);
                }
            }
        }

        // Title on its winged plate.
        let (title, subtitle) = match self.page {
            MenuPage::EscRoot => ("PAUSED", ""),
            MenuPage::Settings { .. } => ("SETTINGS", ""),
            MenuPage::Difficulty => ("SELECT DIFFICULTY", ""),
            MenuPage::Multiplayer => ("MULTIPLAYER", ""),
            MenuPage::Ladder => ("LADDER", ""),
            MenuPage::UpdatePrompt => (
                "UPDATE AVAILABLE",
                "ONLINE PLAY REQUIRES THE LATEST VERSION - MISMATCHED GAMES DESYNC.",
            ),
            MenuPage::Replays => ("REPLAYS", ""),
            _ => ("ORION", "A DETERMINISTIC ISOMETRIC RTS"),
        };
        let big = if self.page == MenuPage::MainRoot { self.ts(8.0) } else { self.ts(4.0) };
        let tw = self.gfx.text_width(big, title);
        let ty = if self.page == MenuPage::MainRoot { h * 0.22 } else { h * 0.12 };
        if self.page != MenuPage::MainRoot {
            let tp_w = (tw + 120.0 * ui).max(340.0 * ui);
            let tp_h = big * 8.0 + 18.0 * ui;
            self.gfx.sprite(
                out,
                book.title_plate,
                cx,
                ty + big * 3.6,
                tp_w,
                tp_h,
                [1.0, 1.0, 1.0, 0.97],
            );
        }
        self.gfx.text(out, cx - tw * 0.5, ty, big, gold, title);
        if !subtitle.is_empty() {
            let ts = self.ts(1.5);
            let sw = self.gfx.text_width(ts, subtitle);
            self.gfx.text(out, cx - sw * 0.5, ty + big * 9.0, ts, dim, subtitle);
        }

        // Page-specific copy.
        if self.page == MenuPage::Difficulty {
            if let Some(st) = &self.map_status {
                let ts = self.ts(1.5);
                let ok = !st.contains("FAILED");
                let color = if ok { gold } else { [1.0, 0.5, 0.4, 1.0] };
                let lw = self.gfx.text_width(ts, st);
                self.gfx.text(out, cx - lw * 0.5, h * 0.215, ts, color, st);
            }
        }
        if self.page == MenuPage::Replays {
            if let Some(err) = &self.mp_error {
                let ts = self.ts(1.5);
                let l = err.to_uppercase();
                let lw = self.gfx.text_width(ts, &l);
                self.gfx.text(out, cx - lw * 0.5, h * 0.21, ts, [1.0, 0.5, 0.4, 1.0], &l);
            }
            if let Some(st) = &self.replay_status {
                let ts = self.ts(1.5);
                let ok = !st.contains("FAILED");
                let color = if ok { gold } else { [1.0, 0.5, 0.4, 1.0] };
                let lw = self.gfx.text_width(ts, st);
                self.gfx.text(out, cx - lw * 0.5, h * 0.245, ts, color, st);
            }
        }
        if self.page == MenuPage::Multiplayer {
            let ts = self.ts(1.5);
            let lines = self.multiplayer_copy();
            for (k, l) in lines.iter().enumerate() {
                let lw = self.gfx.text_width(ts, l);
                let color = if l.starts_with("CONNECTION") { [1.0, 0.5, 0.4, 1.0] } else { dim };
                // Above the button stack (stack starts at h*0.24).
                self.gfx.text(out, cx - lw * 0.5, h * 0.192 + k as f32 * 22.0 * ui, ts, color, l);
            }
        }
        if let MenuPage::Settings { .. } = self.page {
            // Labels mirror the tabbed button layout in menu_buttons.
            let sbh = 36.0 * ui;
            let sgap = 6.0 * ui;
            let y0 = h * 0.175;
            let c0 = y0 + sbh + 18.0 * ui;
            if self.settings_tab == 0 {
                let row_y = c0 + 2.0 * (sbh + sgap);
                let ts = self.ts(1.5);
                let hud_l = format!("HUD SIZE  {:.1}X", self.settings.hud_scale);
                let spd_l = format!("GAME SPEED  {:.2}X", self.settings.game_speed);
                let mus_l = format!("MUSIC  {:.0}%", self.settings.music_volume * 100.0);
                let sfx_l = format!("SOUND  {:.0}%", self.settings.sfx_volume * 100.0);
                for (k, l) in [hud_l, spd_l, mus_l, sfx_l].iter().enumerate() {
                    let lw = self.gfx.text_width(ts, l);
                    self.gfx.text(out, cx - lw * 0.5, row_y + k as f32 * (sbh + sgap) + 11.0 * ui, ts, white, l);
                }
            } else {
                let ky0 = c0 + 30.0 * ui;
                self.gfx.text(out, cx - 330.0 * ui, ky0 - 20.0 * ui, self.ts(1.4), gold, "KEYBINDS (CLICK, THEN PRESS A KEY)");
                let kbh = 22.0 * ui;
                let col_w = 330.0 * ui;
                for (k, a) in ALL_ACTIONS.iter().enumerate() {
                    let col = k % 2;
                    let row = k / 2;
                    let x = cx - col_w + col as f32 * col_w;
                    let y = ky0 + row as f32 * (kbh + 4.0 * ui);
                    self.gfx.text(out, x, y + 5.0 * ui, self.ts(1.1), dim, a.label());
                }
            }
        }
        if self.page == MenuPage::MainRoot {
            let ts = self.ts(1.0);
            let foot = "BUILT FROM SCRATCH IN RUST - NO ENGINE";
            let fw = self.gfx.text_width(ts, foot);
            self.gfx.text(out, cx - fw * 0.5, h - 40.0 * ui, ts, [0.4, 0.4, 0.42, 1.0], foot);
            let ver = concat!("V", env!("CARGO_PKG_VERSION"));
            let vw = self.gfx.text_width(ts, ver);
            self.gfx.text(out, w - vw - 14.0 * ui, h - 22.0 * ui, ts, [0.45, 0.45, 0.5, 1.0], ver);
        }

        // Buttons: gold-capped navy plates with hover glow.
        for b in self.menu_buttons() {
            let hover = self.mouse.0 >= b.x
                && self.mouse.0 <= b.x + b.w
                && self.mouse.1 >= b.y
                && self.mouse.1 <= b.y + b.h;
            let plate = if hover { book.menu_plate_hi } else { book.menu_plate };
            self.gfx.sprite(
                out,
                plate,
                b.x + b.w * 0.5,
                b.y + b.h * 0.5,
                b.w,
                b.h,
                [1.0, 1.0, 1.0, 1.0],
            );
            let mut ts = if b.small { self.ts(1.4) } else { self.ts(2.0) };
            // Long labels shrink to fit their plate instead of overflowing.
            while ts > self.ts(1.0) && self.gfx.text_width(ts, &b.label) > b.w - 20.0 * ui {
                ts *= 0.9;
            }
            let lw = self.gfx.text_width(ts, &b.label);
            let ly = b.y + b.h * 0.5 - ts * 3.5;
            let tc = if hover { gold } else { white };
            self.gfx.text(out, b.x + b.w * 0.5 - lw * 0.5, ly, ts, tc, &b.label);
        }
    }

    /// The multiplayer page's intro/status lines — used for both drawing
    /// and sizing the dialog panel around them.
    fn multiplayer_copy(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        if self.mp_busy() {
            if self.mp_private {
                if let Some(code) = &self.mp_lobby_code {
                    lines.push(format!("PRIVATE LOBBY CODE:  {code}"));
                    lines.push("SHARE IT - ONLY PEOPLE WITH THE CODE CAN JOIN.".into());
                }
            } else {
                lines.push("LOBBY OPEN - WAITING FOR AN OPPONENT...".into());
                lines.push("YOUR GAME IS LISTED FOR EVERYONE TO JOIN.".into());
            }
        } else {
            lines.push("CREATE A LOBBY, OR CLICK ONE BELOW TO JOIN.".into());
            lines.push("PRIVATE LOBBIES ARE JOINED BY THEIR CODE.".into());
        }
        if let Some(err) = &self.mp_error {
            lines.push(String::new());
            lines.push(err.to_uppercase());
        }
        lines
    }

    /// Gold frame reachable from menu drawing (hud.rs has the twin used by
    /// the console; this one lives here to keep menu.rs self-contained).
    /// Terrain thumbnail of the currently selected map, drawn beside the
    /// picker panel: minimap palette + resources + start markers, so map
    /// choice isn't blind.
    fn draw_map_thumb(&self, out: &mut Vec<Inst>, x: f32, y: f32, size: f32) {
        use orion_sim::map::TileKind;
        let names = crate::app::all_map_names();
        if names.is_empty() {
            return;
        }
        let name = names[self.map_choice % names.len()].clone();
        {
            let mut cache = self.thumb_cache.borrow_mut();
            let stale = cache.as_ref().map_or(true, |(n, _)| *n != name);
            if stale {
                let map = orion_sim::map::by_name(&name)
                    .or_else(|| crate::editor::load_custom(&name));
                match map {
                    Some(m) => *cache = Some((name.clone(), m)),
                    None => return,
                }
            }
        }
        let cache = self.thumb_cache.borrow();
        let Some((_, map)) = cache.as_ref() else { return };
        let ui = self.ui();
        let scale = size / map.width.max(map.height) as f32;
        self.gfx.quad(out, x, y, size, size, [0.02, 0.02, 0.03, 1.0]);
        self.gold_frame_menu(out, x, y, size, size);
        for ty in 0..map.height {
            for tx in 0..map.width {
                let c = match map.kind_at(tx, ty) {
                    TileKind::Blocked => [0.11, 0.10, 0.11],
                    TileKind::Ramp => [0.29, 0.27, 0.23],
                    TileKind::Ground => {
                        if map.elev_at(tx, ty) > 0 {
                            [0.32, 0.31, 0.28]
                        } else {
                            [0.21, 0.19, 0.17]
                        }
                    }
                };
                self.gfx.quad(
                    out,
                    x + tx as f32 * scale,
                    y + ty as f32 * scale,
                    scale + 0.5,
                    scale + 0.5,
                    [c[0], c[1], c[2], 1.0],
                );
            }
        }
        let dot = |out: &mut Vec<Inst>, tx: i32, ty: i32, s: f32, c: [f32; 4]| {
            self.gfx.quad(
                out,
                x + tx as f32 * scale - s * 0.5 + scale * 0.5,
                y + ty as f32 * scale - s * 0.5 + scale * 0.5,
                s,
                s,
                c,
            );
        };
        for t in &map.trees {
            dot(out, t.x, t.y, scale.max(1.5), [0.16, 0.30, 0.18, 1.0]);
        }
        for r in &map.rocks {
            dot(out, r.x, r.y, scale.max(1.5), [0.42, 0.40, 0.38, 1.0]);
        }
        for (m, _) in &map.minerals {
            dot(out, m.x, m.y, (scale * 1.2).max(2.0), [
                MINERAL_COLOR[0],
                MINERAL_COLOR[1],
                MINERAL_COLOR[2],
                1.0,
            ]);
        }
        for (g, _) in &map.geysers {
            dot(out, g.x, g.y, (scale * 1.6).max(2.5), [
                GAS_COLOR[0],
                GAS_COLOR[1],
                GAS_COLOR[2],
                1.0,
            ]);
        }
        for (k, st) in map.starts.iter().enumerate() {
            let c = TEAM_COLORS[k % 2];
            dot(out, st.x, st.y, (scale * 2.4).max(4.0), [c[0], c[1], c[2], 1.0]);
        }
        // Name plate under the thumb.
        let ts = self.ts(1.3);
        let label = names[self.map_choice % names.len()].to_uppercase();
        let lw = self.gfx.text_width(ts, &label);
        self.gfx.text(
            out,
            x + (size - lw) * 0.5,
            y + size + 8.0 * ui,
            ts,
            [0.66, 0.7, 0.76, 1.0],
            &label,
        );
    }

    fn gold_frame_menu(&self, out: &mut Vec<Inst>, x: f32, y: f32, w: f32, h: f32) {
        let ui = self.ui();
        let book = &self.gfx.book;
        let t = 4.0 * ui;
        let c = 10.0 * ui;
        let wh = [1.0, 1.0, 1.0, 1.0];
        let half = std::f32::consts::FRAC_PI_2;
        self.gfx.sprite(out, book.gold_h, x + w * 0.5, y + t * 0.5, w - 2.0 * c, t, wh);
        self.gfx.sprite(out, book.gold_h, x + w * 0.5, y + h - t * 0.5, w - 2.0 * c, t, wh);
        self.gfx.sprite(out, book.gold_v, x + t * 0.5, y + h * 0.5, t, h - 2.0 * c, wh);
        self.gfx.sprite(out, book.gold_v, x + w - t * 0.5, y + h * 0.5, t, h - 2.0 * c, wh);
        self.gfx.sprite(out, book.gold_corner, x + c * 0.5, y + c * 0.5, c, c, wh);
        self.gfx.sprite_rot(out, book.gold_corner, x + w - c * 0.5, y + c * 0.5, c, c, half, wh);
        self.gfx
            .sprite_rot(out, book.gold_corner, x + w - c * 0.5, y + h - c * 0.5, c, c, half * 2.0, wh);
        self.gfx
            .sprite_rot(out, book.gold_corner, x + c * 0.5, y + h - c * 0.5, c, c, half * 3.0, wh);
    }

    /// Join the lobby whose code sits in the code field. Reached by the
    /// JOIN PRIVATE button or by pressing Enter while typing the code.
    pub(crate) fn join_private_lobby(&mut self) {
        self.mp_error = None;
        let code = self.code_input.trim().to_uppercase();
        if code.len() < 4 {
            self.mp_error = Some("type the lobby code first".into());
            return;
        }
        // The relay's seat frame decides duel vs 2v2 room — both native
        // and browser joiners speak the room handshake.
        self.join_waiting = Some(crate::relay::join_auto_async(
            self.settings.relay_url.clone(),
            code,
            self.chosen_race,
            self.settings.player_name.clone(),
        ));
    }

    pub(crate) fn menu_click(&mut self) {
        // Click during rebinding cancels it (clicking a bind row re-arms).
        let mut clicked: Option<MenuAction> = None;
        for b in self.menu_buttons() {
            if self.mouse.0 >= b.x
                && self.mouse.0 <= b.x + b.w
                && self.mouse.1 >= b.y
                && self.mouse.1 <= b.y + b.h
            {
                clicked = Some(b.action);
                break;
            }
        }
        let Some(action) = clicked else {
            self.rebinding = None;
            return;
        };
        match action {
            MenuAction::OpenDifficulty => self.page = MenuPage::Difficulty,
            MenuAction::StartGame(d) => self.start_game(d),
            MenuAction::OpenMultiplayer => {
                if self.mp_blocked {
                    self.page = MenuPage::UpdatePrompt;
                    return;
                }
                self.page = MenuPage::Multiplayer;
                if !self.settings.relay_url.is_empty() {
                    self.mm_rating_rx = Some(crate::relay::fetch_rating_async(
                        self.settings.relay_url.clone(),
                        self.settings.player_id.clone(),
                    ));
                }
            }
            MenuAction::SettingsTab(t) => {
                self.settings_tab = t;
            }
            MenuAction::OpenEditor => {
                self.editor = Some(crate::editor::Editor::new());
                self.rebuild_editor_preview();
                self.page = MenuPage::None;
                self.in_game = false;
            }
            MenuAction::OpenSettings => {
                self.page = MenuPage::Settings { from_game: self.in_game };
            }
            MenuAction::Back => {
                self.settings.save();
                self.page = match self.page {
                    MenuPage::Settings { from_game: true } => MenuPage::EscRoot,
                    MenuPage::Ladder => MenuPage::Multiplayer,
                    _ => MenuPage::MainRoot,
                };
                self.rebinding = None;
            }
            MenuAction::Resume => self.page = MenuPage::None,
            MenuAction::QuitToMenu => {
                self.save_replay(); // abandoned games are replays too
                // Leaving a ranked game with no winner is a forfeit — unless
                // the opponent already disconnected, in which case it's ours.
                if self.state.winner.is_none() && self.mm_code.is_some() {
                    let peer_gone = self.mp.as_ref().is_some_and(|m| m.disconnected);
                    let w = if peer_gone { self.human } else { 1 - self.human };
                    self.report_ranked(w);
                }
                self.in_game = false;
                self.tutorial = None;
                self.mp = None; // closes the socket; peer sees a disconnect
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.observer = None;
                }
                self.mm_code = None;
                self.replay = None;
                self.page = MenuPage::MainRoot;
            }
            MenuAction::QuitApp => {
                self.settings.save();
                self.finished = true;
            }
            MenuAction::ToggleFullscreen => {
                self.settings.fullscreen = !self.settings.fullscreen;
                self.want_fullscreen = Some(self.settings.fullscreen);
                self.settings.save();
            }
            MenuAction::ToggleEdge => {
                self.settings.edge_scroll = !self.settings.edge_scroll;
                self.settings.save();
            }
            MenuAction::HudScale(d) => {
                self.settings.hud_scale = (self.settings.hud_scale + d).clamp(0.6, 1.8);
                self.settings.save();
            }
            MenuAction::Speed(d) => {
                self.settings.game_speed = (self.settings.game_speed + d).clamp(0.25, 3.0);
                self.settings.save();
            }
            MenuAction::MusicVol(d) => {
                self.settings.music_volume = (self.settings.music_volume + d).clamp(0.0, 1.0);
                if let Some(a) = &self.audio {
                    a.set_music_volume(self.settings.music_volume);
                }
                self.settings.save();
            }
            MenuAction::SfxVol(d) => {
                self.settings.sfx_volume = (self.settings.sfx_volume + d).clamp(0.0, 1.0);
                if let Some(a) = &mut self.audio {
                    a.sfx_volume = self.settings.sfx_volume;
                }
                self.settings.save();
            }
            MenuAction::CreateLobby { private } => {
                self.mp_error = None;
                let names = crate::app::all_map_names();
                let chosen = names[self.map_choice % names.len()].clone();
                // Custom maps travel inside the handshake — the joiner
                // needs no file. Builtins go by name (small frames).
                let map_ron = if orion_sim::map::MAP_NAMES.contains(&chosen.as_str()) {
                    None
                } else {
                    match crate::editor::load_custom(&chosen)
                        .and_then(|m| ron::ser::to_string(&m).ok())
                    {
                        Some(ron) => Some(ron),
                        None => {
                            self.mp_error = Some("could not load that custom map".into());
                            return;
                        }
                    }
                };
                self.settings.save(); // persist the name
                self.mp_private = private;
                let (code, rx) = crate::relay::host_relay_async_full(
                    self.settings.relay_url.clone(),
                    crate::relay::fresh_code(),
                    self.chosen_race,
                    &self.settings.player_name,
                    private,
                    &chosen,
                    map_ron,
                );
                self.mp_lobby_code = Some(code);
                self.mp_waiting = Some(rx);
            }
            MenuAction::JoinRelay => self.join_private_lobby(),
            MenuAction::WatchCode => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.mp_error = None;
                    let code = self.code_input.trim().to_uppercase();
                    if code.len() >= 4 {
                        self.obs_waiting = Some(crate::relay::observe_async(
                            self.settings.relay_url.clone(),
                            code,
                        ));
                    }
                }
            }
            MenuAction::JoinListed(k) => {
                if let Some(l) = self.lobby_list.get(k) {
                    self.mp_error = None;
                    self.code_input = l.code.clone();
                    self.join_private_lobby();
                }
            }
            MenuAction::FocusName => {
                self.name_focus = true;
            }
            MenuAction::FocusCode => {
                self.name_focus = false;
            }
            MenuAction::FindMatch => {
                self.mp_error = None;
                self.settings.save(); // persist name + identity
                self.mm_status = "CONNECTING TO MATCHMAKER...".into();
                self.mm_queue = Some(crate::relay::find_match_async(
                    self.settings.relay_url.clone(),
                    self.settings.player_id.clone(),
                    self.settings.player_name.clone(),
                    self.chosen_race,
                ));
            }
            MenuAction::CancelSearch => {
                // Dropping the receiver ends the queue thread on its next
                // server message; the socket close removes us server-side.
                self.mm_queue = None;
                self.mm_status.clear();
            }
            MenuAction::StartRoomNow => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(tx) = &self.room_start_tx {
                    let _ = tx.send(());
                }
            }
            MenuAction::CancelMp => {
                self.mp_waiting = None;
                self.mp_lobby_code = None;
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.room_waiting = None;
                    self.join_waiting = None;
                    self.room_start_tx = None;
                }
            }
            MenuAction::CycleMap => {
                self.map_choice = (self.map_choice + 1) % crate::app::all_map_names().len();
            }
            MenuAction::CycleRace => {
                let n = self.state.data.race_names.len() as u8;
                self.chosen_race = (self.chosen_race + 1) % n.max(1);
            }
            MenuAction::CycleEnemyRace => {
                let n = self.state.data.race_names.len() as u8;
                self.enemy_race_choice = (self.enemy_race_choice + 1) % (n + 1).max(1);
            }
            MenuAction::OpenLadder => {
                self.ladder = None;
                self.ladder_rx =
                    Some(crate::relay::fetch_ladder_async(self.settings.relay_url.clone()));
                self.page = MenuPage::Ladder;
            }
            MenuAction::Noop => {}
            MenuAction::DeclineUpdate => {
                self.page = MenuPage::MainRoot;
            }
            MenuAction::OpenReplays => {
                self.replay_files = crate::replays::list(&self.state.data.race_names);
                self.page = MenuPage::Replays;
            }
            MenuAction::OpenUpdate => {
                if let Some((_, url)) = &self.update {
                    crate::relay::open_url(url);
                }
            }
            MenuAction::ToggleReplayShare => {
                self.replay_share_mode = !self.replay_share_mode;
                if self.replay_share_mode {
                    self.replay_status = None;
                }
            }
            MenuAction::FetchReplayCode => {
                self.fetch_shared_replay();
            }
            MenuAction::FetchMapCode => {
                self.fetch_shared_map();
            }
            MenuAction::StartTutorial => {
                self.start_tutorial();
            }
            MenuAction::CreateRoom => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.mp_error = None;
                    let names = crate::app::all_map_names();
                    let chosen = names[self.map_choice % names.len()].clone();
                    let map = orion_sim::map::by_name(&chosen)
                        .or_else(|| crate::editor::load_custom(&chosen));
                    let Some(map) = map else {
                        self.mp_error = Some("could not load that map".into());
                        return;
                    };
                    if map.starts.len() < 4 {
                        self.mp_error =
                            Some("2v2 needs a 4-start map - pick crossfire".into());
                        return;
                    }
                    let map_ron = if orion_sim::map::MAP_NAMES.contains(&chosen.as_str()) {
                        None
                    } else {
                        ron::ser::to_string(&map).ok()
                    };
                    self.settings.save();
                    self.mp_private = true;
                    let (code, rx, start_tx) = crate::relay::host_room_async_full(
                        self.settings.relay_url.clone(),
                        crate::relay::fresh_code(),
                        self.chosen_race,
                        &chosen,
                        map_ron,
                        &self.settings.player_name,
                        false,
                    );
                    self.mp_lobby_code = Some(code);
                    self.room_waiting = Some(rx);
                    self.room_start_tx = Some(start_tx);
                }
            }
            MenuAction::PlayReplay(k) => {
                if self.replay_share_mode {
                    self.share_replay(k);
                    return;
                }
                if let Some((_, path)) = self.replay_files.get(k) {
                    let path = path.clone();
                    self.start_replay(&path);
                }
            }
            MenuAction::Rebind(a) => {
                self.rebinding = Some(a);
            }
        }
    }
}
