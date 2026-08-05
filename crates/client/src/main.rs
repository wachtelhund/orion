//! Orion client entry point. `--smoke` runs the window for ~3 seconds and
//! exits 0 — used by CI/dev to verify the render stack comes up.

mod app;
mod atlas;
mod audio;
mod config;
/// Instant that works on wasm (std panics there).
mod clock {
    #[cfg(not(target_arch = "wasm32"))]
    pub use std::time::{Instant, SystemTime};
    #[cfg(not(target_arch = "wasm32"))]
    pub use std::time::UNIX_EPOCH;
    #[cfg(target_arch = "wasm32")]
    pub use web_time::{Instant, SystemTime, UNIX_EPOCH};
}

/// Debug line that reaches the browser console on wasm, stderr natively.
pub fn weblog(s: &str) {
    #[cfg(target_arch = "wasm32")]
    web_sys::console::log_1(&s.into());
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("{s}");
}
mod editor;
mod font;
mod gfx;
mod hud;
mod iso;
mod menu;
#[cfg(not(target_arch = "wasm32"))]
mod relay;
#[cfg(target_arch = "wasm32")]
#[path = "relay_wasm.rs"]
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
    record_follow: bool,
    shot_cross: bool,
    replay_open: Option<String>,
    replay_shot: Option<(String, String, u32)>,
    map_arg: Option<String>,
    races_arg: Option<(u8, u8)>,
    stage: bool,
    #[cfg(target_arch = "wasm32")]
    pending_gfx: std::rc::Rc<std::cell::RefCell<Option<Gfx>>>,
}

impl ApplicationHandler for Shell {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Web fires resumed more than once; a second pass while the async
        // gfx is still in flight must not spawn another window/canvas.
        if self.app.is_some() || self.window.is_some() {
            return;
        }
        // Automated MP runs live in a small corner window: a fullscreen
        // window popping over the desktop gets closed by the human, which
        // reads as a mystery disconnect in the logs.
        let (win_w, win_h) = if self.mp_auto.is_some() { (480.0, 300.0) } else { (1440.0, 900.0) };
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title(if self.mp_auto.is_some() { "Orion MP test" } else { "Orion" })
                        .with_inner_size(LogicalSize::new(win_w, win_h)),
                )
                .expect("create window"),
        );
        // Web: the canvas winit made must join the DOM, and the wgpu
        // adapter only resolves asynchronously — stash the Gfx when it
        // lands and finish init from about_to_wait.
        #[cfg(target_arch = "wasm32")]
        {
            use winit::platform::web::WindowExtWebSys;
            let canvas = window.canvas().expect("canvas");
            // Give the surface real pixels — a 0x0 canvas renders nothing.
            canvas.set_width(1440);
            canvas.set_height(900);
            let doc = web_sys::window().unwrap().document().unwrap();
            let body = doc.body().unwrap();
            let _ = body.append_child(&canvas);
            // Winit must agree with the canvas about the surface size —
            // without this its inner_size stays 0x0 and wgpu draws nothing.
            let _ = window.request_inner_size(LogicalSize::new(1440.0, 900.0));
            web_sys::console::log_1(&"orion: canvas attached".into());
            let cell = self.pending_gfx.clone();
            let win2 = window.clone();
            wasm_bindgen_futures::spawn_local(async move {
                web_sys::console::log_1(&"orion: requesting adapter".into());
                let gfx = Gfx::new_async(win2).await;
                web_sys::console::log_1(&"orion: gfx ready".into());
                *cell.borrow_mut() = Some(gfx);
            });
            self.window = Some(window);
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let gfx = Gfx::new(window.clone());
            self.finish_init(gfx, window);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        self.poll_pending_gfx();
        self.window_event_inner(event_loop, event);
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        self.poll_pending_gfx();
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// Wakeups from the wasm keep-alive interval: when the page is hidden
    /// (rAF stopped), keep a multiplayer match stepping so the peer sees a
    /// slow game instead of a frozen one.
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _ev: ()) {
        #[cfg(target_arch = "wasm32")]
        {
            self.poll_pending_gfx();
            let hidden = web_sys::window()
                .and_then(|w| w.document())
                .map(|d| d.hidden())
                .unwrap_or(false);
            if hidden {
                if let Some(app) = self.app.as_mut() {
                    app.background_step();
                }
            }
        }
    }
}

impl Shell {
    /// Web: adopt the async-created Gfx once it resolves.
    fn poll_pending_gfx(&mut self) {
        #[cfg(target_arch = "wasm32")]
        {
            if self.app.is_none() {
                let gfx = self.pending_gfx.borrow_mut().take();
                if let (Some(gfx), Some(window)) = (gfx, self.window.clone()) {
                    web_sys::console::log_1(&"orion: init complete".into());
                    self.finish_init(gfx, window.clone());
                    // Winit-web's ResizeObserver pipeline left the canvas
                    // backing store at 1x1 — force real pixels into both
                    // the canvas and the wgpu surface.
                    use winit::platform::web::WindowExtWebSys;
                    // Fill the browser viewport.
                    let web_win = web_sys::window().unwrap();
                    let w = web_win
                        .inner_width()
                        .ok()
                        .and_then(|v| v.as_f64())
                        .unwrap_or(1440.0) as u32;
                    let h = web_win
                        .inner_height()
                        .ok()
                        .and_then(|v| v.as_f64())
                        .unwrap_or(900.0) as u32;
                    if let Some(canvas) = window.canvas() {
                        canvas.set_width(w);
                        canvas.set_height(h);
                        let st = canvas.style();
                        let _ = st.set_property("width", &format!("{w}px"));
                        let _ = st.set_property("height", &format!("{h}px"));
                    }
                    if let Some(app) = self.app.as_mut() {
                        app.resize(w, h);
                    }
                    web_sys::console::log_1(&"orion: surface forced 1440x900".into());
                    // ?page=mp|replays|difficulty jumps straight to a menu
                    // page — lets headless-Chrome screenshots verify pages
                    // without scripted clicks.
                    if let Ok(search) = web_win.location().search() {
                        if let Some(app) = self.app.as_mut() {
                            if search.contains("page=mp") {
                                app.page = crate::menu::MenuPage::Multiplayer;
                            } else if search.contains("page=replays") {
                                app.page = crate::menu::MenuPage::Replays;
                            } else if search.contains("page=difficulty") {
                                app.page = crate::menu::MenuPage::Difficulty;
                            }
                        }
                    }
                    if let Some(win) = &self.window {
                        win.request_redraw();
                    }
                }
                // Keep the event loop breathing until init lands.
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
        }
    }

    fn finish_init(&mut self, gfx: Gfx, window: Arc<Window>) {

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
            app.follow = self.record_follow;
        }
        let (r0, r1) = self.races_arg.unwrap_or((0, 1));
        if let Some(m) = &self.map_arg {
            if let Some(k) = orion_sim::map::MAP_NAMES.iter().position(|n| n == m) {
                app.map_choice = k;
                app.game_map = m.clone();
                app.state = app::new_game_with(r0, r1, m);
            } else {
                eprintln!("unknown map '{m}', maps: {:?}", orion_sim::map::MAP_NAMES);
            }
        } else if self.races_arg.is_some() {
            let map = app.game_map.clone();
            app.state = app::new_game_with(r0, r1, &map);
        }
        if self.stage {
            app.stage_showcase();
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
            || self.replay_shot.is_some()
            || self.mp_auto.is_some();
        if !headless {
            app.init_audio();
            app.persist_identity();
        }
        // Persisted fullscreen preference (not in capture/test modes).
        if app.settings.fullscreen && !headless {
            window.set_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        self.app = Some(app);
        self.window = Some(window);
    }

    fn window_event_inner(&mut self, event_loop: &ActiveEventLoop, event: WindowEvent) {
        let Some(app) = self.app.as_mut() else { return };
        match event {
            WindowEvent::CloseRequested => {
                // In automated runs a close means someone dismissed the test
                // window — say so, or the exit reads as a transport bug.
                if app.mp_auto.is_some() {
                    eprintln!("mp-auto: window closed by user - aborting test");
                }
                event_loop.exit();
            }
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

}

fn main() {
    #[cfg(target_arch = "wasm32")]
    console_error_panic_hook::set_once();
    #[cfg(target_arch = "wasm32")]
    web_sys::console::log_1(&"orion: main() enter".into());
    env_logger::init();
    // rustls 0.23 needs a process-wide crypto provider before any TLS use.
    #[cfg(not(target_arch = "wasm32"))]
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
    let record_follow = args.iter().any(|a| a == "--record-follow");
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
    let stage = args.iter().any(|a| a == "--stage");
    // --races A,B — bot race indices for --shot/--record captures.
    let races_arg = args
        .iter()
        .position(|a| a == "--races")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| {
            let (a, b) = s.split_once(',')?;
            Some((a.parse().ok()?, b.parse().ok()?))
        });
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
    let event_loop = EventLoop::with_user_event().build().expect("event loop");
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
        record_follow,
        shot_cross,
        replay_open,
        replay_shot,
        map_arg,
        races_arg,
        stage,
        ..Default::default()
    };
    // Web: run_app's exception-based escape would unwind main while events
    // still target our stack-borrowed shell — spawn_app takes ownership.
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        use winit::platform::web::EventLoopExtWebSys;
        // Chrome stops requestAnimationFrame entirely for hidden pages, so a
        // backgrounded tab would freeze its half of a lockstep match. Proxy
        // wakeups ride postMessage, which keeps firing (throttled) while
        // hidden — a slow interval nudges the shell so multiplayer keeps
        // stepping at reduced rate instead of stalling the peer.
        let proxy = event_loop.create_proxy();
        let nudge = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
            let _ = proxy.send_event(());
        });
        if let Some(w) = web_sys::window() {
            let _ = w.set_interval_with_callback_and_timeout_and_arguments_0(
                nudge.as_ref().unchecked_ref(),
                250,
            );
        }
        nudge.forget();
        event_loop.spawn_app(shell);
    }
    #[cfg(not(target_arch = "wasm32"))]
    event_loop.run_app(&mut shell).expect("run app");
}
