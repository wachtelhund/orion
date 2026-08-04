//! Orion client entry point. `--smoke` runs the window for ~3 seconds and
//! exits 0 — used by CI/dev to verify the render stack comes up.

mod app;
mod atlas;
mod audio;
mod config;
mod font;
mod gfx;
mod hud;
mod iso;
mod menu;
mod relay;
mod replays;

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Fullscreen, Window, WindowId};

use app::App;
use gfx::Gfx;

#[derive(Default)]
struct Shell {
    app: Option<App>,
    window: Option<Arc<Window>>,
    smoke: bool,
    shot: Option<(u32, String)>,
    shot_focus: Option<(f32, f32)>,
    shot_zoom: Option<f32>,
    script: Option<String>,
    shot_reveal: bool,
    menu_shot: Option<(String, String)>,
    mp_auto: Option<String>,
    record: Option<(String, u32, u32, u32)>,
    shot_cross: bool,
    replay_open: Option<String>,
    replay_shot: Option<(String, String, u32)>,
    map_arg: Option<String>,
}

impl ApplicationHandler for Shell {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.app.is_some() {
            return;
        }
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Orion")
                        .with_inner_size(LogicalSize::new(1440.0, 900.0)),
                )
                .expect("create window"),
        );
        let gfx = Gfx::new(window.clone());
        let sf = window.scale_factor() as f32;
        let mut app = App::new(gfx, self.smoke, self.shot.clone(), sf);
        app.shot_focus = self.shot_focus;
        app.shot_zoom = self.shot_zoom.map(|z| z * sf);
        app.script = self.script.clone();
        app.mp_auto = self.mp_auto.clone();
        if app.mp_auto.is_some() {
            app.in_game = false;
            app.page = crate::menu::MenuPage::MainRoot;
        }
        app.shot_reveal = self.shot_reveal;
        app.menu_shot = self.menu_shot.clone();
        app.record = self.record.clone();
        app.shot_cross = self.shot_cross;
        if app.record.is_some() {
            app.shot_bot0 = Some(app::Bot2::new(0));
        }
        if let Some(m) = &self.map_arg {
            if let Some(k) = orion_sim::map::MAP_NAMES.iter().position(|n| n == m) {
                app.map_choice = k;
                app.game_map = m.clone();
                app.state = app::new_game_with(0, 1, m);
            } else {
                eprintln!("unknown map '{m}', maps: {:?}", orion_sim::map::MAP_NAMES);
            }
        }
        if let Some(path) = &self.replay_open {
            app.start_replay(std::path::Path::new(path));
        }
        if let Some((path, out, tick)) = &self.replay_shot {
            app.start_replay(std::path::Path::new(path));
            app.replay_shot = Some((*tick, out.clone()));
        }
        let headless = self.smoke
            || self.shot.is_some()
            || self.script.is_some()
            || self.menu_shot.is_some()
            || self.replay_shot.is_some();
        if !headless {
            app.init_audio();
            app.persist_identity();
        }
        // Persisted fullscreen preference (not in capture modes).
        if app.settings.fullscreen && self.shot.is_none() && self.script.is_none() && !self.smoke
        {
            window.set_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        self.app = Some(app);
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(app) = self.app.as_mut() else { return };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => app.resize(size.width, size.height),
            WindowEvent::RedrawRequested => {
                app.frame();
                if let Some(fs) = app.want_fullscreen.take() {
                    if let Some(w) = &self.window {
                        w.set_fullscreen(fs.then(|| Fullscreen::Borderless(None)));
                    }
                }
                if app.finished {
                    event_loop.exit();
                    return;
                }
                if app.smoke_expired() {
                    println!("smoke test OK");
                    event_loop.exit();
                    return;
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            other => app.handle_event(&other),
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

fn main() {
    env_logger::init();
    // rustls 0.23 needs a process-wide crypto provider before any TLS use.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let args: Vec<String> = std::env::args().collect();
    let smoke = args.iter().any(|a| a == "--smoke");
    // --shot out.ppm [--shot-ticks N]: bot-vs-bot fast-forward, capture one
    // frame, exit. Visual verification without a human at the screen.
    let shot = args
        .iter()
        .position(|a| a == "--shot")
        .and_then(|i| args.get(i + 1))
        .map(|path| {
            let ticks = args
                .iter()
                .position(|a| a == "--shot-ticks")
                .and_then(|i| args.get(i + 1))
                .and_then(|t| t.parse().ok())
                .unwrap_or(24 * 30);
            (ticks, path.clone())
        });
    // --shot-focus X,Y (world tiles): center the capture camera there.
    let shot_focus = args
        .iter()
        .position(|a| a == "--shot-focus")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| {
            let (x, y) = s.split_once(',')?;
            Some((x.parse().ok()?, y.parse().ok()?))
        });
    // --shot-zoom Z (logical, DPI-scaled internally).
    let shot_zoom = args
        .iter()
        .position(|a| a == "--shot-zoom")
        .and_then(|i| args.get(i + 1))
        .and_then(|z| z.parse().ok());
    // --script prefix: scripted human-play test with a capture series.
    let script = args
        .iter()
        .position(|a| a == "--script")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let shot_reveal = args.iter().any(|a| a == "--shot-reveal");
    // --record prefix:start:frames:every — frame sequence for GIFs.
    let record = args
        .iter()
        .position(|a| a == "--record")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| {
            let p: Vec<&str> = s.split(':').collect();
            Some((
                p.first()?.to_string(),
                p.get(1)?.parse().ok()?,
                p.get(2)?.parse().ok()?,
                p.get(3)?.parse().ok()?,
            ))
        });
    let shot_cross = args.iter().any(|a| a == "--shot-cross");
    let mp_auto = args
        .iter()
        .position(|a| a == "--mp-auto")
        .and_then(|i| args.get(i + 1))
        .cloned();
    // --map name — map for --shot/--script/--record captures and the
    // initial menu selection.
    let map_arg = args
        .iter()
        .position(|a| a == "--map")
        .and_then(|i| args.get(i + 1))
        .cloned();
    // --replay path — open straight into the replay viewer.
    let replay_open = args
        .iter()
        .position(|a| a == "--replay")
        .and_then(|i| args.get(i + 1))
        .cloned();
    // --replay-shot path:out.ppm:tick — headless replay capture + checksum.
    let replay_shot = args
        .iter()
        .position(|a| a == "--replay-shot")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| {
            let p: Vec<&str> = s.split(':').collect();
            Some((p.first()?.to_string(), p.get(1)?.to_string(), p.get(2)?.parse().ok()?))
        });
    // --menu-shot page:path — capture a menu page (main/settings/esc).
    let menu_shot = args
        .iter()
        .position(|a| a == "--menu-shot")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| {
            let (page, path) = s.split_once(':')?;
            Some((page.to_string(), path.to_string()))
        });
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut shell = Shell {
        smoke,
        shot,
        shot_focus,
        shot_zoom,
        script,
        shot_reveal,
        menu_shot,
        mp_auto,
        record,
        shot_cross,
        replay_open,
        replay_shot,
        map_arg,
        ..Default::default()
    };
    event_loop.run_app(&mut shell).expect("run app");
}
