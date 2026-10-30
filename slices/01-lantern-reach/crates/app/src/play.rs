//! `galaxy play`: the game in a window.
//!
//! Real time maps onto fixed 30 Hz ticks through `galaxy_core::step::FixedStep`, and
//! ships are drawn between ticks by interpolation. The sim never learns about any of
//! this: it gets a `PlayerInput` per tick.
//!
//! Autosaves happen at calm moments (docking, opening the PAUSED card, quitting) on a
//! background thread ([`Saver`]).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use galaxy_contract::prelude::*;
use galaxy_core::Save;
use galaxy_core::step::FixedStep;
use render::Renderer;
use render::glam::Vec2 as G2;
use serde_json::{Value, json};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::game::{Game, GameOptions, MindMode};
use crate::input::{self, Key, Keys};
use crate::saves::{self, SaveChoice, SaveFolder, Saver};
use crate::session::Session;
use crate::ui::{self, Amount, Button, UiAction, UiState};
use crate::view::map::map_camera;
use crate::view::{CameraRig, View, g};

#[derive(Clone, Debug, Default)]
pub struct PlayArgs {
    pub beat: Option<u32>,
    pub save: Option<PathBuf>,
    pub new: bool,
    pub seed: u64,
    pub packs: Option<PathBuf>,
    /// Run the core pack only: no `gm-examples` (also `GALAXY_NO_GM=1`).
    pub no_gm: bool,
    /// Reload packs when their files change (development only).
    pub hot_reload: bool,
    /// Close after this many frames (a smoke test of the window without a player).
    pub quit_after_frames: Option<u32>,
    /// Mark the goal's station at the screen edge (an accessibility aid, off by default).
    pub goal_marker: bool,
}

/// The winit key for each game key.
pub fn key_of(code: KeyCode) -> Option<Key> {
    Some(match code {
        KeyCode::KeyW => Key::W,
        KeyCode::KeyA => Key::A,
        KeyCode::KeyS => Key::S,
        KeyCode::KeyD => Key::D,
        KeyCode::Space => Key::Space,
        KeyCode::ShiftLeft | KeyCode::ShiftRight => Key::Shift,
        KeyCode::KeyM => Key::M,
        KeyCode::KeyT => Key::T,
        KeyCode::KeyP => Key::P,
        KeyCode::Escape => Key::Escape,
        KeyCode::Minus | KeyCode::NumpadSubtract => Key::ZoomOut,
        KeyCode::Equal | KeyCode::NumpadAdd => Key::ZoomIn,
        KeyCode::Digit1 | KeyCode::Numpad1 => Key::Answer(0),
        KeyCode::Digit2 | KeyCode::Numpad2 => Key::Answer(1),
        KeyCode::Digit3 | KeyCode::Numpad3 => Key::Answer(2),
        KeyCode::Digit4 | KeyCode::Numpad4 => Key::Answer(3),
        KeyCode::Digit5 | KeyCode::Numpad5 => Key::Answer(4),
        KeyCode::Digit6 | KeyCode::Numpad6 => Key::Answer(5),
        KeyCode::Digit7 | KeyCode::Numpad7 => Key::Answer(6),
        KeyCode::Digit8 | KeyCode::Numpad8 => Key::Answer(7),
        KeyCode::Digit9 | KeyCode::Numpad9 => Key::Answer(8),
        _ => return None,
    })
}

struct Running {
    window: Arc<Window>,
    renderer: Renderer,
    view: View,
}

struct App {
    session: Session,
    saver: Saver,
    running: Option<Running>,
    keys: Keys,
    ui: UiState,
    rig: CameraRig,
    step: FixedStep,
    last: Instant,
    started: Instant,
    /// A click for the next tick.
    click: Option<Click>,
    buttons: Vec<Button>,
    error: Option<String>,
    render_errors: u32,
    frames: u32,
    quit_after_frames: Option<u32>,
    /// The game-master bridge, serviced between frames ([`App::serve_bridge`]).
    bridge: Option<galaxy_gm::Bridge>,
    /// Sound cues from committed events ([`crate::sound`]); N mutes.
    audio: galaxy_audio::Audio,
    /// Which cues each tick asks for.
    cues: crate::sound::Cues,
}

/// Starts the game-master bridge [`Game::open_gm`] opened on the session's game:
/// applies staged packs with a snapshot first and opens the live socket. A problem
/// here is printed and the game goes on without the bridge (or without the socket).
fn start_bridge(
    bridge: Result<galaxy_gm::Bridge, String>,
    session: &mut Session,
    save: &mut Save,
) -> Option<galaxy_gm::Bridge> {
    let mut bridge = match bridge {
        Ok(b) => b,
        Err(e) => {
            eprintln!("game master: {e}");
            return None;
        }
    };
    bridge.apply_staged(session.sim_mut(), save);
    if let Err(e) = bridge.start_live(saves::live_registry()) {
        eprintln!("game master: {e}");
    }
    for n in bridge.notes.drain(..) {
        eprintln!("game master: {n}");
    }
    Some(bridge)
}

/// Opens the window and plays until it's closed. Returns a JSON summary.
pub fn play(args: PlayArgs) -> Result<Value, (u8, String)> {
    let folder = SaveFolder::user();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let choice = saves::choose(
        folder.as_ref(),
        args.save.clone(),
        args.new || args.beat.is_some(),
        stamp,
    )
    .map_err(|e| (4, e))?;
    if let Some(dir) = choice.path().parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| (1, format!("{}: {e}", dir.display())))?;
    }
    let created = matches!(choice, SaveChoice::Create(_));
    let packs = crate::game::packs_dir(args.packs.clone()).ok_or((
        4,
        "can't find the packs folder; pass --packs or set GALAXY_PACKS".to_owned(),
    ))?;
    let gm = !args.no_gm && crate::game::gm_allowed();
    let (save, state) = match &choice {
        SaveChoice::Open(path) => {
            let save = Save::open(path).map_err(|e| (4, e.to_string()))?;
            let state = save.load().map_err(|e| (4, e.to_string()))?;
            (save, state)
        }
        SaveChoice::Create(path) => {
            let state = if let Some(beat) = args.beat {
                let run = crate::harness::jump(&packs, args.seed, gm, beat).map_err(|e| (4, e))?;
                if run.report["ok"] == false {
                    return Err((4, run.report.to_string()));
                }
                run.game.sim().state().clone()
            } else {
                crate::game::new_state(&packs, args.seed, gm).map_err(|e| (4, e))?
            };
            let save = Save::create(path, &state).map_err(|e| (1, e.to_string()))?;
            (save, state)
        }
    };
    let audio = galaxy_audio::Audio::open(
        folder
            .as_ref()
            .map(|f| f.dir().join(galaxy_audio::settings::FILE)),
    );
    if let Some(note) = audio.settings_note() {
        eprintln!("sound: {note}");
    }
    let opts = GameOptions {
        packs,
        gm,
        mind: MindMode::Live,
        hot_reload: args.hot_reload,
    };
    // One composition: the game-master bridge loads the packs the game master applied
    // into the game's script host, beside core and gm-examples.
    let bridge_opts = galaxy_gm::BridgeOptions {
        exe: std::env::current_exe().ok(),
        analyzer: galaxy_script::analyzer::find(None),
        // Absolute: `gm propose --live` runs its sandbox children with it, from
        // wherever it was started.
        packs: std::path::absolute(&opts.packs).ok(),
        ..galaxy_gm::BridgeOptions::default()
    };
    let (mut game, bridge) =
        Game::open_gm(state, &opts, choice.path(), bridge_opts).map_err(|e| (4, e))?;
    for note in game.take_notes() {
        eprintln!("{note}");
    }
    let packs_loaded: Vec<String> = game.reports().iter().map(|r| r.pack.to_string()).collect();
    let mut session = Session::with_game(game);
    // Staged packs apply now (loading is a calm moment), and the live socket opens.
    let mut save = save;
    let bridge = start_bridge(bridge, &mut session, &mut save);
    session.begin();
    let cues = crate::sound::Cues::new(session.world());
    let center = session.flagship().map_or(G2::ZERO, |s| g(s.position));
    let mut app = App {
        session,
        saver: Saver::start(save),
        running: None,
        keys: Keys::default(),
        ui: UiState {
            goal_marker: args.goal_marker,
            ..UiState::default()
        },
        rig: CameraRig::new(center),
        step: FixedStep::default(),
        last: Instant::now(),
        started: Instant::now(),
        click: None,
        buttons: Vec::new(),
        error: None,
        render_errors: 0,
        frames: 0,
        quit_after_frames: args.quit_after_frames,
        bridge,
        audio,
        cues,
    };
    let event_loop = EventLoop::new().map_err(|e| (1, format!("no window system: {e}")))?;
    let run = event_loop.run_app(&mut app);
    let App {
        mut session,
        saver,
        error,
        frames,
        render_errors,
        bridge,
        audio,
        ..
    } = app;
    // The mind books every call still out and its last batches apply, then the save.
    session.finish();
    let gm = bridge.as_ref().map(|b| {
        json!({
            "applied": b.state().applied.len(),
            "staged": b.state().staged.len(),
            "live": b.server().map(|s| s.discovery().endpoint.clone()),
        })
    });
    // Closes the socket and removes the discovery files.
    drop(bridge);
    let path = saver.path().to_path_buf();
    let saved = saver.finish(Some(session.state().clone()));
    run.map_err(|e| (1, e.to_string()))?;
    if let Some(e) = error {
        return Err((1, e));
    }
    saved.map_err(|e| (1, format!("the last save failed: {e}")))?;
    Ok(json!({
        "save": path,
        "new": created,
        "tick": session.world().meta.tick,
        "session": session.session(),
        "packs": packs_loaded,
        "frames": frames,
        "render_errors": render_errors,
        "gm": gm,
        "audio": audio.status(),
    }))
}

impl App {
    /// Between frames: the bridge answers requests and applies staged packs at a calm
    /// moment; a live `shot` renders what's on screen now and writes it on a thread.
    fn serve_bridge(&mut self) {
        let Some(bridge) = &mut self.bridge else {
            return;
        };
        let left = bridge.serve(self.session.sim_mut(), &mut self.saver);
        for n in bridge.notes.drain(..) {
            eprintln!("game master: {n}");
        }
        for req in left {
            let galaxy_gm::live::Op::Shot { out } = req.op.clone() else {
                req.respond(Err(galaxy_gm::live::WireError::input("not handled here")));
                continue;
            };
            let image = self.capture();
            let world = self.session.world();
            let info = json!({
                "out": out,
                "tick": world.meta.tick,
                "paused": self.session.is_paused(),
                "map": self.ui.map_open,
                "docked_at": self.docked(),
                "position": self.session.flagship().map(|s| s.position),
                "goal": crate::hooks::goal_line(world),
            });
            let _ = std::thread::Builder::new()
                .name("gm-shot".into())
                .spawn(move || {
                    let result = image.and_then(|img| {
                        img.save_png(&out).map_err(|e| e.to_string())?;
                        let mut info = info;
                        info["width"] = json!(img.width);
                        info["height"] = json!(img.height);
                        Ok(info)
                    });
                    req.respond(result.map_err(galaxy_gm::live::WireError::runtime));
                });
        }
    }

    /// What the window shows now, read back as RGBA.
    fn capture(&mut self) -> Result<render::RgbaImage, String> {
        let Some(r) = &mut self.running else {
            return Err("the window isn't open yet".into());
        };
        let viewport = r.renderer.logical_size();
        let time = (Instant::now() - self.started).as_secs_f32();
        let (scene, _) = crate::frame::compose(
            &r.view,
            &self.session,
            &self.rig,
            &self.ui,
            self.step.alpha(),
            time,
            viewport,
        );
        r.renderer.render_to_rgba(&scene).map_err(|e| e.to_string())
    }

    fn docked(&self) -> Option<StationId> {
        self.session.flagship().and_then(|f| f.docked_at.clone())
    }

    fn autosave(&self) {
        self.saver.save(self.session.state().clone());
    }

    fn viewport(&self) -> G2 {
        self.running
            .as_ref()
            .map_or(G2::new(1280.0, 720.0), |r| r.renderer.logical_size())
    }

    fn toggle_pause(&mut self) {
        let now_paused = self.session.toggle_pause();
        self.step.reset();
        self.last = Instant::now();
        // Both ways: nothing held or pressed across the card counts afterwards.
        self.keys.clear();
        self.audio.set_paused(now_paused);
        if now_paused {
            self.autosave();
        }
    }

    fn trade(&mut self, line: usize, amount: Amount) {
        let Some(station) = self.docked() else { return };
        let lines = ui::dock_lines(self.session.world(), &station);
        let Some(l) = lines.get(line) else { return };
        self.ui.selected = line;
        if let Some(req) = ui::trade_request(self.session.world(), &station, l, amount) {
            self.session.request_trade(req);
        }
    }

    fn act(&mut self, action: UiAction) {
        match action {
            UiAction::Trade { line, amount } => self.trade(line, amount),
            UiAction::Select(i) => self.ui.selected = i,
            UiAction::CloseMap => self.ui.map_open = false,
            UiAction::Service(req) => self.session.request_service(req),
            UiAction::Prestige(to) => self.ui.confirm = Some(to),
            UiAction::ConfirmPrestige(to) => {
                self.ui.confirm = None;
                self.session
                    .request_service(ServiceRequest::Prestige { to });
            }
            UiAction::CancelPrestige => self.ui.confirm = None,
            UiAction::ServicesFrom(first) => self.ui.services_first = first,
            UiAction::Reply { npc, choice } => {
                self.session.reply(npc, choice);
            }
            UiAction::AskWhatChanged => {
                self.session.ask_what_changed();
            }
        }
    }

    fn key_pressed(&mut self, key: Key) {
        if matches!(key, Key::P | Key::Escape) {
            self.toggle_pause();
            return;
        }
        if self.session.is_paused() {
            return;
        }
        match key {
            Key::M => self.ui.map_open = !self.ui.map_open,
            Key::ZoomIn => self.rig.zoom_by(1.0),
            Key::ZoomOut => self.rig.zoom_by(-1.0),
            // The dock screen is mouse-first: WASD stays the ship's (flying off undocks).
            _ if self.docked().is_none() || self.ui.map_open => {}
            // The prestige card takes no answers under it.
            Key::Answer(i) if self.ui.confirm.is_none() => {
                self.session.reply_to_open(usize::from(i));
            }
            _ => {}
        }
    }

    fn clicked(&mut self) {
        if self.session.is_paused() {
            return;
        }
        let Some(cursor) = self.ui.cursor else { return };
        if let Some(action) = ui::hit(&self.buttons, cursor).cloned() {
            self.act(action);
            return;
        }
        let queue = self.keys.held(Key::Shift);
        let viewport = self.viewport();
        let world = self.session.world();
        if self.ui.map_open {
            // Map click travel: a region, once the flag unlocks it (the sim decides).
            let map = galaxy_economy::MapView::build(world);
            let camera = map_camera(&map, viewport);
            if let Some(at) = input::plane_point(&camera, cursor, viewport)
                && let Some(region) = galaxy_ship::space::region_at(world, at)
            {
                self.click = Some(Click {
                    target: ClickTarget::Region {
                        region: region.clone(),
                    },
                    queue,
                });
            }
            return;
        }
        if self.docked().is_some() {
            return;
        }
        self.click = input::click_at(world, &self.rig.camera(), cursor, viewport, queue);
    }

    fn frame(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f64().min(0.25);
        self.last = now;
        let ticks = if self.session.is_paused() {
            0
        } else {
            self.step.advance(dt)
        };
        for i in 0..ticks {
            let docked = self.docked().is_some();
            let click = if i == 0 { self.click.take() } else { None };
            let input = self.keys.tick_input(docked, click);
            let out = self.session.tick(input);
            let world = self.session.world();
            for (cue, seed) in self.cues.tick(world, self.session.last_events(), &out) {
                self.audio.play(cue, seed);
            }
            if out.docked.is_some() {
                self.ui.selected = 0;
                self.ui.services_first = 0;
                self.autosave();
            }
            if out.undocked {
                self.ui.confirm = None;
            }
        }
        for note in self.session.game_mut().take_notes() {
            eprintln!("{note}");
        }
        let alpha = self.step.alpha();
        let time = (now - self.started).as_secs_f32();
        let Some(r) = &mut self.running else { return };
        r.view.sync(&mut r.renderer, self.session.world());
        let viewport = r.renderer.logical_size();
        if let Some(target) = crate::frame::camera_target(&self.session, &self.rig, alpha, viewport)
        {
            self.rig.fit(self.session.world());
            self.rig.follow(target, dt as f32);
        }
        let (scene, buttons) = crate::frame::compose(
            &r.view,
            &self.session,
            &self.rig,
            &self.ui,
            alpha,
            time,
            viewport,
        );
        self.buttons = buttons;
        if let Err(e) = r.renderer.render(&scene) {
            self.render_errors += 1;
            if self.render_errors < 5 {
                eprintln!("render: {e}");
            }
        }
        if let Some(e) = self.saver.last_error()
            && self.error.is_none()
        {
            eprintln!("save: {e}");
            self.error = Some(e);
        }
        r.window.request_redraw();
    }
}

impl ApplicationHandler for App {
    /// After each batch of window events, so never inside a tick. Wakes at least ten
    /// times a second so the bridge answers while the window is minimised.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.serve_bridge();
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(100),
        ));
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.running.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("The Lantern Reach")
            .with_inner_size(LogicalSize::new(1280.0, 760.0))
            .with_min_inner_size(LogicalSize::new(ui::MIN_WINDOW.x, ui::MIN_WINDOW.y));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                self.error = Some(format!("can't open a window: {e}"));
                event_loop.exit();
                return;
            }
        };
        let mut renderer = match Renderer::new_windowed(window.clone()) {
            Ok(r) => r,
            Err(e) => {
                self.error = Some(e.to_string());
                event_loop.exit();
                return;
            }
        };
        let mut view = View::new();
        view.sync(&mut renderer, self.session.world());
        if let Some(target) =
            crate::frame::camera_target(&self.session, &self.rig, 1.0, renderer.logical_size())
        {
            self.rig.center = target;
        }
        self.last = Instant::now();
        self.step.reset();
        window.request_redraw();
        self.running = Some(Running {
            window,
            renderer,
            view,
        });
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = &mut self.running {
                    r.renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(r) = &mut self.running {
                    r.renderer.set_scale_factor(scale_factor as f32);
                }
            }
            WindowEvent::Focused(false) => self.keys.clear(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state,
                        repeat,
                        ..
                    },
                ..
            } => {
                let down = state == ElementState::Pressed;
                if code == KeyCode::F3 {
                    if down && !repeat {
                        self.ui.debug_open = !self.ui.debug_open;
                    }
                    return;
                }
                // N mutes and unmutes, paused or not. Not a game key: it never reaches
                // the sim.
                if code == KeyCode::KeyN {
                    if down && !repeat {
                        self.audio.toggle_mute();
                    }
                    return;
                }
                let Some(key) = key_of(code) else { return };
                if self.keys.event(key, down, repeat) {
                    self.key_pressed(key);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self
                    .running
                    .as_ref()
                    .map_or(1.0, |r| r.renderer.scale_factor());
                self.ui.cursor = Some(G2::new(position.x as f32, position.y as f32) / scale);
            }
            WindowEvent::CursorLeft { .. } => self.ui.cursor = None,
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.clicked(),
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => {
                if !self.ui.map_open
                    && !self.session.is_paused()
                    && self.docked().is_none()
                    && let Some(cursor) = self.ui.cursor
                {
                    self.click = input::click_at(
                        self.session.world(),
                        &self.rig.camera(),
                        cursor,
                        self.viewport(),
                        self.keys.held(Key::Shift),
                    )
                    .and_then(|mut c| {
                        if let ClickTarget::Rock { rock } = c.target {
                            c.target = ClickTarget::Tag { rock };
                            Some(c)
                        } else {
                            None
                        }
                    });
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if self.ui.map_open || self.session.is_paused() {
                    return;
                }
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 60.0,
                };
                self.rig.zoom_by(steps);
            }
            WindowEvent::RedrawRequested => {
                self.frame();
                self.frames += 1;
                if self.quit_after_frames.is_some_and(|n| self.frames >= n) {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }
}
