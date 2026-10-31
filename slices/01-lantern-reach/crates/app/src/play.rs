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

/// How far the mouse travels, in logical pixels, before a press on a part is a drag
/// rather than a click.
const DRAG_PX: f32 = 6.0;

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
        KeyCode::KeyL => Key::Log,
        KeyCode::Enter | KeyCode::NumpadEnter => Key::Enter,
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
    /// Where the camera was framing (the dock and the open room): a change glides.
    framing: (Option<StationId>, Option<StationPartId>),
    /// Where the left button went down on a part at the cradle: a drag starts there.
    press_at: Option<G2>,
    /// The view's settings (station life) and where they're kept.
    view_settings: crate::view::life::ViewSettings,
    view_settings_path: Option<PathBuf>,
    /// The station themes from the packs folder, for the view.
    themes: crate::view::themes::Themes,
    /// The radio: tuning and the mix, played through `audio` ([`crate::radio`]).
    radio: crate::radio::Radio,
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
    // Every OpenRouter call is logged beside the save (only when the mind calls out).
    if game.calls_out()
        && let Err(e) = game.open_call_log(&galaxy_mind::calllog::path_for(choice.path()))
    {
        eprintln!("{e}");
    }
    // The radio's music beside the save: the cache plays; with a key, new tracks.
    game.open_music(choice.path());
    for note in game.take_notes() {
        eprintln!("{note}");
    }
    let packs_loaded: Vec<String> = game.reports().iter().map(|r| r.pack.to_string()).collect();
    let mut session = Session::with_game(game);
    // How far each hint has taught in this save; a record that can't be read starts
    // over rather than stopping the game.
    let rules = ui::HintRules::load(&crate::game::core_data(&opts.packs)).map_err(|e| (4, e))?;
    let taught = saves::read_hints(choice.path()).unwrap_or_else(|e| {
        eprintln!("{e}");
        Default::default()
    });
    session.hints = ui::HintBook::new(rules, taught);
    // Where talk bubbles rise from, and which lines wait at which dock in this save. A
    // save with no record (or one that can't be read) starts from its current visit.
    session.anchors =
        ui::talk::TalkAnchors::load(&crate::game::core_data(&opts.packs)).map_err(|e| (4, e))?;
    match saves::read_talk(choice.path()) {
        Ok(Some(record)) => session.talk = ui::talk::TalkBook::from_record(&record),
        Ok(None) => {}
        Err(e) => eprintln!("{e}"),
    }
    // Staged packs apply now (loading is a calm moment), and the live socket opens.
    let mut save = save;
    let bridge = start_bridge(bridge, &mut session, &mut save);
    session.begin();
    let cues = crate::sound::Cues::new(session.world());
    // Station themes (#47, 4.1): the packs folder's, checked against the world.
    let themes = crate::view::themes::Themes::load(&crate::game::core_data(&opts.packs))
        .and_then(|t| t.check(session.world()).map(|()| t))
        .map_err(|e| (4, e))?;
    let view_settings_path = folder
        .as_ref()
        .map(|f| f.dir().join(crate::view::life::SETTINGS_FILE));
    let view_settings = view_settings_path
        .as_deref()
        .map_or_else(Default::default, |p| {
            let (settings, note) = crate::view::life::ViewSettings::load(p);
            if let Some(note) = note {
                eprintln!("view: {note}");
            }
            settings
        });
    let radio = crate::radio::Radio::new(
        session.game().radio().clone(),
        themes.clone(),
        audio.settings().radio_on,
    )
    .with_music(session.game().music().clone());
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
        framing: (None, None),
        press_at: None,
        view_settings,
        view_settings_path,
        themes,
        radio,
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
    saver.save_hints(session.hints.counts().clone());
    saver.save_talk(session.talk.record());
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
        crate::frame::prepare(&mut r.view, &mut r.renderer, &self.session, &self.ui);
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

    fn autosave(&mut self) {
        self.saver.save(self.session.state().clone());
        if self.session.hints.take_changed() {
            self.saver.save_hints(self.session.hints.counts().clone());
        }
        if self.session.talk.take_changed() {
            self.saver.save_talk(self.session.talk.record());
        }
    }

    fn viewport(&self) -> G2 {
        self.running
            .as_ref()
            .map_or(G2::new(1280.0, 720.0), |r| r.renderer.logical_size())
    }

    /// Q, or the PAUSED card's button: station life full or quiet, kept for next time.
    fn toggle_life(&mut self) {
        self.view_settings.life = self.view_settings.life.toggled();
        if let Some(r) = &mut self.running {
            r.view.life = self.view_settings.life;
        }
        if let Some(path) = self.view_settings_path.clone() {
            let settings = self.view_settings;
            let _ = std::thread::Builder::new()
                .name("view-settings".into())
                .spawn(move || {
                    if let Err(e) = settings.save(&path) {
                        eprintln!("view: {e}");
                    }
                });
        }
    }

    /// The radio's buttons, or R: tunes, and keeps On or Off in the sound settings.
    fn tune_radio(&mut self, t: crate::radio::Tune) {
        let on = self.radio.tune(self.session.world(), t);
        self.audio.set_radio_on(on);
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
            UiAction::Service(req) => {
                ui::rooms::fit::send(&mut self.session, &mut self.ui.wharf, req);
            }
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
            UiAction::NextLine => {
                self.session.next_line();
            }
            UiAction::SkipTalk => self.session.skip_talk(),
            UiAction::ToggleLog => self.ui.toggle_log(&mut self.session.talk),
            UiAction::LogFrom(first) => self.ui.log_first = first,
            UiAction::Replay(r) => self.ui.replay = r,
            UiAction::Absorb => {}
            UiAction::OpenRoom(id) => self.open_room(Some(id)),
            UiAction::CloseRoom => self.open_room(None),
            UiAction::Wharf(a) => self.wharf(&a),
            UiAction::Radio(t) => self.tune_radio(t),
        }
    }

    /// A click at the hullwright's cradle: a tab, a socket, the locker, a confirm.
    fn wharf(&mut self, a: &ui::rooms::fit::WharfAction) {
        let Some(station) = self.docked() else { return };
        let (world, history) = (self.session.world(), self.session.sim().history());
        if let Some(req) = self.ui.wharf.click(world, history, &station, a) {
            ui::rooms::fit::send(&mut self.session, &mut self.ui.wharf, req);
        }
    }

    /// The left button came up: the end of a drag at the cradle (a drop on a socket or
    /// the locker), or a click that picked a part up.
    fn released(&mut self) {
        let Some(at) = self.press_at.take() else {
            return;
        };
        if self.session.is_paused() {
            return;
        }
        let cursor = self.ui.cursor.unwrap_or(at);
        let moved = cursor.distance(at) > DRAG_PX;
        let over = ui::rooms::cradle::over_of(ui::hit(&self.buttons, cursor));
        self.ui.wharf.hover(over);
        let (world, history) = (self.session.world(), self.session.sim().history());
        if let Some(req) = self.ui.wharf.release(world, history, moved) {
            ui::rooms::fit::send(&mut self.session, &mut self.ui.wharf, req);
        }
    }

    /// Opens a station part's room beside its module, or (`None`) goes back to the
    /// whole station. The market's selection and the services' page start over.
    fn open_room(&mut self, room: Option<StationPartId>) {
        if self.docked().is_none() {
            return;
        }
        if room != self.ui.room {
            self.ui.selected = 0;
            self.ui.services_first = 0;
            self.ui.wharf.reset();
            self.press_at = None;
        }
        self.ui.room = room;
    }

    fn key_pressed(&mut self, key: Key) {
        // At the cradle, Esc puts down a part in hand (or closes a confirm) first.
        if key == Key::Escape && !self.session.is_paused() && self.ui.wharf.back() {
            self.press_at = None;
            return;
        }
        // At the dock, Esc goes back from a room to the whole station first.
        if key == Key::Escape
            && !self.session.is_paused()
            && self.ui.room.is_some()
            && self.docked().is_some()
            && self.ui.confirm.is_none()
        {
            self.open_room(None);
            return;
        }
        if matches!(key, Key::P | Key::Escape) {
            self.toggle_pause();
            return;
        }
        if self.session.is_paused() {
            return;
        }
        match key {
            Key::M => self.ui.map_open = !self.ui.map_open,
            Key::Log if !self.ui.map_open && self.ui.confirm.is_none() => {
                self.ui.toggle_log(&mut self.session.talk);
            }
            Key::Enter if !self.ui.map_open && self.ui.confirm.is_none() => {
                self.ui.enter(&mut self.session);
            }
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
            // The PAUSED card's one button: station life.
            let card = crate::view::life::paused_card(self.view_settings.life);
            if let Some(c) = self.ui.cursor
                && card
                    .toggle_rects(self.viewport())
                    .first()
                    .is_some_and(|r| ui::contains(r, c))
            {
                self.toggle_life();
            }
            return;
        }
        let Some(cursor) = self.ui.cursor else { return };
        // A click during a camera glide lands it at once; one during the fitting
        // animation ends it.
        if self.rig.gliding() {
            self.rig.skip_glide();
            return;
        }
        if self.ui.wharf.skip() {
            return;
        }
        // A press on a part at the cradle picks it up: a drag, or (let go where it
        // was) a click.
        if let Some(UiAction::Wharf(a)) = ui::hit(&self.buttons, cursor)
            && self.ui.wharf.press(self.session.world(), a)
        {
            self.press_at = Some(cursor);
            return;
        }
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
                self.ui.room = None;
                self.ui.wharf.reset();
                self.press_at = None;
            }
            ui::rooms::fit::settle(&self.session, &mut self.ui.wharf);
        }
        self.ui.wharf.advance(dt as f32);
        for note in self.session.game_mut().take_notes() {
            eprintln!("{note}");
        }
        // The radio follows the flagship: its mix to the audio, its widget to the HUD.
        let radio = self
            .radio
            .update(self.session.world(), self.session.sim().history());
        self.audio.radio(&radio.mix);
        self.ui.radio = radio.ui;
        let alpha = self.step.alpha();
        let time = (now - self.started).as_secs_f32();
        let Some(r) = &mut self.running else { return };
        r.view.sync(&mut r.renderer, self.session.world());
        let viewport = r.renderer.logical_size();
        if let Some(goal) = crate::frame::camera_goal(&self.session, &self.ui, alpha, viewport) {
            // Docking, undocking and opening or closing a room glide the camera there.
            let framing = (
                self.session.flagship().and_then(|f| f.docked_at.clone()),
                crate::frame::open_room(&self.session, &self.ui).map(|p| p.id.clone()),
            );
            if framing != self.framing {
                self.framing = framing;
                self.rig.begin_glide();
            }
            let world = self.session.world();
            match goal.zoom {
                Some(zoom) => self.rig.glide_to(goal.center, zoom, goal.tilt, dt as f32),
                None if self.rig.gliding() => {
                    let zoom = self.rig.flight_zoom(world);
                    self.rig.glide_to(goal.center, zoom, goal.tilt, dt as f32);
                }
                None => {
                    self.rig.fit(world);
                    self.rig.tilt = goal.tilt;
                    self.rig.follow(goal.center, dt as f32);
                }
            }
        }
        crate::frame::prepare(&mut r.view, &mut r.renderer, &self.session, &self.ui);
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
            .with_inner_size(LogicalSize::new(ui::DESIGN.x, ui::DESIGN.y))
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
        view.set_themes(self.themes.clone());
        view.life = self.view_settings.life;
        view.sync(&mut renderer, self.session.world());
        if let Some(goal) =
            crate::frame::camera_goal(&self.session, &self.ui, 1.0, renderer.logical_size())
        {
            self.rig.settle(&goal, self.session.world());
            self.framing = (
                self.docked(),
                crate::frame::open_room(&self.session, &self.ui).map(|p| p.id.clone()),
            );
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
                if code == KeyCode::F2 {
                    if down && !repeat {
                        crate::ui::fps::toggle();
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
                // R tunes the radio to the next channel in range (past the last, off; off,
                // on again), as its Next button does. Not a game key.
                if code == KeyCode::KeyR {
                    if down && !repeat && !self.session.is_paused() {
                        self.tune_radio(crate::radio::Tune::Cycle);
                    }
                    return;
                }
                // Q turns a docked station's ambient life down or back up, paused or not.
                if code == KeyCode::KeyQ {
                    if down && !repeat {
                        self.toggle_life();
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
                // A part in hand: which socket (or the locker) it's over now.
                if self.ui.wharf.held.is_some()
                    && let Some(c) = self.ui.cursor
                {
                    let over = ui::rooms::cradle::over_of(ui::hit(&self.buttons, c));
                    self.ui.wharf.hover(over);
                }
            }
            WindowEvent::CursorLeft { .. } => self.ui.cursor = None,
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.clicked(),
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => self.released(),
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => {
                // At the dock, a right-click puts down a part in hand, or goes back to
                // the whole station.
                if !self.ui.map_open && !self.session.is_paused() && self.docked().is_some() {
                    if self.ui.wharf.back() {
                        self.press_at = None;
                    } else if self.ui.confirm.is_none() {
                        self.open_room(None);
                    }
                    return;
                }
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
                // Docked, the camera frames the station; the wheel is for flying.
                if self.ui.map_open || self.session.is_paused() || self.docked().is_some() {
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
