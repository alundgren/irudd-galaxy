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
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use galaxy_contract::prelude::*;
use galaxy_core::Save;
use galaxy_core::step::FixedStep;
use render::Renderer;
use render::glam::Vec2 as G2;
use serde_json::{Value, json};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
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
}

/// The winit key for each game key.
pub fn key_of(code: KeyCode) -> Option<Key> {
    Some(match code {
        KeyCode::KeyW => Key::W,
        KeyCode::KeyA => Key::A,
        KeyCode::KeyS => Key::S,
        KeyCode::KeyD => Key::D,
        KeyCode::ArrowUp => Key::Up,
        KeyCode::ArrowDown => Key::Down,
        KeyCode::ArrowLeft => Key::Left,
        KeyCode::ArrowRight => Key::Right,
        KeyCode::Space => Key::Space,
        KeyCode::ShiftLeft | KeyCode::ShiftRight => Key::Shift,
        KeyCode::KeyE => Key::E,
        KeyCode::Enter | KeyCode::NumpadEnter => Key::Enter,
        KeyCode::KeyM => Key::M,
        KeyCode::KeyP => Key::P,
        KeyCode::Escape => Key::Escape,
        KeyCode::Minus | KeyCode::NumpadSubtract => Key::ZoomOut,
        KeyCode::Equal | KeyCode::NumpadAdd => Key::ZoomIn,
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
}

/// Opens the window and plays until it's closed. Returns a JSON summary.
pub fn play(args: PlayArgs) -> Result<Value, (u8, String)> {
    let folder = SaveFolder::user();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let choice =
        saves::choose(folder.as_ref(), args.save.clone(), args.new, stamp).map_err(|e| (4, e))?;
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
            let state = crate::game::new_state(&packs, args.seed, gm).map_err(|e| (4, e))?;
            let save = Save::create(path, &state).map_err(|e| (1, e.to_string()))?;
            (save, state)
        }
    };
    let audio = galaxy_audio::Audio::open();
    let opts = GameOptions {
        packs,
        gm,
        mind: MindMode::Live,
        hot_reload: args.hot_reload,
    };
    let mut game = Game::open(state, &opts).map_err(|e| (4, e))?;
    for note in game.take_notes() {
        eprintln!("{note}");
    }
    let packs_loaded: Vec<String> = game.reports().iter().map(|r| r.pack.to_string()).collect();
    let mut session = Session::with_game(game);
    session.begin();
    let center = session.flagship().map_or(G2::ZERO, |s| g(s.position));
    let mut app = App {
        session,
        saver: Saver::start(save),
        running: None,
        keys: Keys::default(),
        ui: UiState::default(),
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
    };
    let event_loop = EventLoop::new().map_err(|e| (1, format!("no window system: {e}")))?;
    let run = event_loop.run_app(&mut app);
    let App {
        mut session,
        saver,
        error,
        frames,
        render_errors,
        ..
    } = app;
    // The mind books every call still out and its last batches apply, then the save.
    session.finish();
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
        "audio": if audio.has_device() { "silent (device open)".to_owned() } else {
            format!("no device: {}", audio.problem().unwrap_or("none"))
        },
    }))
}

impl App {
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

    fn select(&mut self, delta: i32) {
        let Some(station) = self.docked() else { return };
        let n = ui::dock_lines(self.session.world(), &station).len().max(1) as i32;
        self.ui.selected = (self.ui.selected as i32 + delta).rem_euclid(n) as usize;
    }

    fn act(&mut self, action: UiAction) {
        match action {
            UiAction::Trade { line, amount } => self.trade(line, amount),
            UiAction::Select(i) => self.ui.selected = i,
            UiAction::Undock => self.keys.press_action(),
            UiAction::CloseMap => self.ui.map_open = false,
            UiAction::Service(req) => self.session.request_service(req),
            UiAction::Prestige(to) => self.ui.confirm = Some(to),
            UiAction::ConfirmPrestige(to) => {
                self.ui.confirm = None;
                self.session.request_service(ServiceRequest::Prestige { to });
            }
            UiAction::CancelPrestige => self.ui.confirm = None,
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
        let shift = self.keys.held(Key::Shift);
        match key {
            Key::M => self.ui.map_open = !self.ui.map_open,
            Key::ZoomIn => self.rig.zoom_by(1.0),
            Key::ZoomOut => self.rig.zoom_by(-1.0),
            _ if self.docked().is_none() || self.ui.map_open => {}
            Key::W | Key::Up => self.select(-1),
            Key::S | Key::Down => self.select(1),
            Key::E | Key::Enter => {
                let amount = if shift { Amount::Most } else { Amount::One };
                self.trade(self.ui.selected, amount);
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
            if out.docked.is_some() {
                self.ui.selected = 0;
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
                let Some(key) = key_of(code) else { return };
                let down = state == ElementState::Pressed;
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
