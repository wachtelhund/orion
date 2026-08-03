//! Main menu, pause (escape) menu, and the settings screen with keybind
//! editing. Pure widgets over the sprite renderer — layout is shared between
//! draw and click handling.

use orion_sim::ai::Difficulty;

use crate::app::App;
use crate::config::{Action, ALL_ACTIONS};
use crate::gfx::Inst;

#[derive(Clone, Copy, PartialEq)]
pub enum MenuPage {
    None,
    MainRoot,
    Difficulty,
    Multiplayer,
    Settings { from_game: bool },
    EscRoot,
}

#[derive(Clone, Copy)]
enum MenuAction {
    OpenDifficulty,
    StartGame(Difficulty),
    OpenMultiplayer,
    OpenSettings,
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
                stack(
                    vec![
                        ("PLAY VS AI".into(), MenuAction::OpenDifficulty),
                        ("MULTIPLAYER".into(), MenuAction::OpenMultiplayer),
                        ("SETTINGS".into(), MenuAction::OpenSettings),
                        ("QUIT".into(), MenuAction::QuitApp),
                    ],
                    h * 0.42,
                );
            }
            MenuPage::Difficulty => {
                stack(
                    vec![
                        ("EASY".into(), MenuAction::StartGame(Difficulty::Easy)),
                        ("NORMAL".into(), MenuAction::StartGame(Difficulty::Normal)),
                        ("HARD".into(), MenuAction::StartGame(Difficulty::Hard)),
                        ("BACK".into(), MenuAction::Back),
                    ],
                    h * 0.42,
                );
            }
            MenuPage::Multiplayer => {
                stack(vec![("BACK".into(), MenuAction::Back)], h * 0.62);
            }
            MenuPage::EscRoot => {
                stack(
                    vec![
                        ("RESUME".into(), MenuAction::Resume),
                        ("SETTINGS".into(), MenuAction::OpenSettings),
                        ("QUIT TO MENU".into(), MenuAction::QuitToMenu),
                        ("QUIT GAME".into(), MenuAction::QuitApp),
                    ],
                    h * 0.38,
                );
            }
            MenuPage::Settings { .. } => {
                let s = &self.settings;
                let y0 = h * 0.22;
                let rows = vec![
                    (
                        format!("FULLSCREEN: {}", if s.fullscreen { "ON" } else { "OFF" }),
                        MenuAction::ToggleFullscreen,
                    ),
                    (
                        format!("EDGE SCROLL: {}", if s.edge_scroll { "ON" } else { "OFF" }),
                        MenuAction::ToggleEdge,
                    ),
                ];
                stack(rows, y0);
                // HUD size and speed with -/+ buttons.
                let small_w = 40.0 * ui;
                let row_y = y0 + 2.0 * (bh + gap);
                for (k, (label, dv, act)) in [
                    ("HUD SIZE", 0.1f32, 0),
                    ("GAME SPEED", 0.25, 1),
                    ("MUSIC", 0.1, 2),
                    ("SOUND", 0.1, 3),
                ]
                .iter()
                .enumerate()
                {
                    let y = row_y + k as f32 * (bh + gap);
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
                        h: bh,
                        label: "-".into(),
                        action: mk(-1.0),
                        small: true,
                    });
                    out.push(MBtn {
                        x: cx + bw * 0.5 - small_w,
                        y,
                        w: small_w,
                        h: bh,
                        label: "+".into(),
                        action: mk(1.0),
                        small: true,
                    });
                    let _ = label;
                }
                // Keybind grid: 2 columns.
                let ky0 = row_y + 4.0 * (bh + gap) + 34.0 * ui;
                let kbh = 26.0 * ui;
                let col_w = 330.0 * ui;
                for (k, a) in ALL_ACTIONS.iter().enumerate() {
                    let col = k % 2;
                    let row = k / 2;
                    out.push(MBtn {
                        x: cx - col_w + col as f32 * col_w + col_w - 90.0 * ui,
                        y: ky0 + row as f32 * (kbh + 6.0 * ui),
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
                let back_y = ky0 + n_rows * (kbh + 6.0 * ui) + 24.0 * ui;
                out.push(MBtn {
                    x: cx - bw * 0.5,
                    y: back_y,
                    w: bw,
                    h: bh,
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
        let dim = [0.6, 0.62, 0.66, 1.0];
        let accent = [0.35, 0.62, 1.0, 1.0];

        // Title.
        let (title, subtitle) = match self.page {
            MenuPage::EscRoot => ("PAUSED", ""),
            MenuPage::Settings { .. } => ("SETTINGS", ""),
            MenuPage::Difficulty => ("SELECT DIFFICULTY", ""),
            MenuPage::Multiplayer => ("MULTIPLAYER", ""),
            _ => ("ORION", "A DETERMINISTIC ISOMETRIC RTS"),
        };
        let big = if self.page == MenuPage::MainRoot { self.ts(8.0) } else { self.ts(4.0) };
        let tw = self.gfx.text_width(big, title);
        let ty = if self.page == MenuPage::MainRoot { h * 0.22 } else { h * 0.12 };
        self.gfx.text(out, cx - tw * 0.5, ty, big, if self.page == MenuPage::MainRoot { accent } else { white }, title);
        if !subtitle.is_empty() {
            let ts = self.ts(1.5);
            let sw = self.gfx.text_width(ts, subtitle);
            self.gfx.text(out, cx - sw * 0.5, ty + big * 9.0, ts, dim, subtitle);
        }

        // Page-specific copy.
        if self.page == MenuPage::Multiplayer {
            let ts = self.ts(1.5);
            let lines = [
                "RANKED 1V1 QUEUE AND PASSWORD LOBBIES",
                "ARE IN DEVELOPMENT.",
                "",
                "THE SIM IS LOCKSTEP-READY: DETERMINISTIC,",
                "FIXED-POINT, INPUT-ONLY. NETCODE IS NEXT.",
            ];
            for (k, l) in lines.iter().enumerate() {
                let lw = self.gfx.text_width(ts, l);
                self.gfx.text(out, cx - lw * 0.5, h * 0.34 + k as f32 * 22.0 * ui, ts, dim, l);
            }
        }
        if let MenuPage::Settings { .. } = self.page {
            // Labels for slider rows + bind rows (buttons drawn below).
            let bh = 44.0 * ui;
            let gap = 12.0 * ui;
            let y0 = h * 0.22;
            let row_y = y0 + 2.0 * (bh + gap);
            let ts = self.ts(1.5);
            let hud_l = format!("HUD SIZE  {:.1}X", self.settings.hud_scale);
            let spd_l = format!("GAME SPEED  {:.2}X", self.settings.game_speed);
            let mus_l = format!("MUSIC  {:.0}%", self.settings.music_volume * 100.0);
            let sfx_l = format!("SOUND  {:.0}%", self.settings.sfx_volume * 100.0);
            for (k, l) in [hud_l, spd_l, mus_l, sfx_l].iter().enumerate() {
                let lw = self.gfx.text_width(ts, l);
                self.gfx.text(out, cx - lw * 0.5, row_y + k as f32 * (bh + gap) + 14.0 * ui, ts, white, l);
            }
            let ky0 = row_y + 4.0 * (bh + gap) + 34.0 * ui;
            self.gfx.text(out, cx - 330.0 * ui, ky0 - 24.0 * ui, self.ts(1.5), accent, "KEYBINDS (CLICK, THEN PRESS A KEY)");
            let kbh = 26.0 * ui;
            let col_w = 330.0 * ui;
            for (k, a) in ALL_ACTIONS.iter().enumerate() {
                let col = k % 2;
                let row = k / 2;
                let x = cx - col_w + col as f32 * col_w;
                let y = ky0 + row as f32 * (kbh + 6.0 * ui);
                self.gfx.text(out, x, y + 7.0 * ui, self.ts(1.2), dim, a.label());
            }
        }
        if self.page == MenuPage::MainRoot {
            let ts = self.ts(1.0);
            let foot = "BUILT FROM SCRATCH IN RUST - NO ENGINE";
            let fw = self.gfx.text_width(ts, foot);
            self.gfx.text(out, cx - fw * 0.5, h - 40.0 * ui, ts, [0.4, 0.4, 0.42, 1.0], foot);
        }

        // Buttons.
        for b in self.menu_buttons() {
            let hover = self.mouse.0 >= b.x
                && self.mouse.0 <= b.x + b.w
                && self.mouse.1 >= b.y
                && self.mouse.1 <= b.y + b.h;
            let bg = if hover { [0.16, 0.19, 0.26, 0.95] } else { [0.08, 0.09, 0.13, 0.92] };
            self.gfx.quad(out, b.x, b.y, b.w, b.h, bg);
            self.gfx.quad(out, b.x, b.y, b.w, 2.0 * ui, [0.32, 0.36, 0.46, 1.0]);
            self.gfx.quad(out, b.x, b.y + b.h - 2.0 * ui, b.w, 2.0 * ui, [0.03, 0.03, 0.05, 1.0]);
            let ts = if b.small { self.ts(1.4) } else { self.ts(2.0) };
            let lw = self.gfx.text_width(ts, &b.label);
            let ly = b.y + b.h * 0.5 - ts * 3.5;
            self.gfx.text(out, b.x + b.w * 0.5 - lw * 0.5, ly, ts, white, &b.label);
        }
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
            MenuAction::OpenMultiplayer => self.page = MenuPage::Multiplayer,
            MenuAction::OpenSettings => {
                self.page = MenuPage::Settings { from_game: self.in_game };
            }
            MenuAction::Back => {
                self.settings.save();
                self.page = match self.page {
                    MenuPage::Settings { from_game: true } => MenuPage::EscRoot,
                    _ => MenuPage::MainRoot,
                };
                self.rebinding = None;
            }
            MenuAction::Resume => self.page = MenuPage::None,
            MenuAction::QuitToMenu => {
                self.in_game = false;
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
            MenuAction::Rebind(a) => {
                self.rebinding = Some(a);
            }
        }
    }
}
