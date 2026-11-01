//! `galaxy play` and `galaxy shot`: the modules on screen.
//!
//! `play` opens a window and runs the session in real time: the sim steps at its fixed
//! rate, the view draws between the last two ticks every frame, keys a module binds
//! become its events, and the wheel zooms the camera. `shot` runs the same session
//! headless for some ticks and writes one frame as a PNG. Both report frame times.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use galaxy_module::Failure;
use galaxy_module::cli::Output;
use galaxy_module::module::Module;
use galaxy_render::Renderer;
use galaxy_render::glam::Vec2;
use galaxy_sim::scenario::InputsFile;
use galaxy_sim::step::FixedStep;
use galaxy_sim::watch::ModuleWatcher;
use galaxy_sim::{BuildStamp, Input, Session, save};
use galaxy_view::View;
use serde_json::{Map, Value, json};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::sim::{Options, bad_modules, load_modules, parse, usage};

/// The window's size, and a shot's, unless `--size` says otherwise.
const DEFAULT_SIZE: (u32, u32) = (1280, 720);
/// Ticks `shot` runs when not told: one second.
const SHOT_TICKS: u64 = 30;

const PLAY_OPTIONS: &[&str] = &[
    "--modules",
    "--seed",
    "--record",
    "--save",
    "--watch",
    "--size",
    "--frames",
    "--inputs",
    "--ticks",
];
const SHOT_OPTIONS: &[&str] = &[
    "--modules",
    "--seed",
    "--ticks",
    "--inputs",
    "--size",
    "--out",
];

/// Keys on the keyboard by their manifest key name (`galaxy_module::manifest::KEYS`).
const KEY_CODES: [(KeyCode, &str); 56] = [
    (KeyCode::KeyA, "a"),
    (KeyCode::KeyB, "b"),
    (KeyCode::KeyC, "c"),
    (KeyCode::KeyD, "d"),
    (KeyCode::KeyE, "e"),
    (KeyCode::KeyF, "f"),
    (KeyCode::KeyG, "g"),
    (KeyCode::KeyH, "h"),
    (KeyCode::KeyI, "i"),
    (KeyCode::KeyJ, "j"),
    (KeyCode::KeyK, "k"),
    (KeyCode::KeyL, "l"),
    (KeyCode::KeyM, "m"),
    (KeyCode::KeyN, "n"),
    (KeyCode::KeyO, "o"),
    (KeyCode::KeyP, "p"),
    (KeyCode::KeyQ, "q"),
    (KeyCode::KeyR, "r"),
    (KeyCode::KeyS, "s"),
    (KeyCode::KeyT, "t"),
    (KeyCode::KeyU, "u"),
    (KeyCode::KeyV, "v"),
    (KeyCode::KeyW, "w"),
    (KeyCode::KeyX, "x"),
    (KeyCode::KeyY, "y"),
    (KeyCode::KeyZ, "z"),
    (KeyCode::Digit0, "0"),
    (KeyCode::Digit1, "1"),
    (KeyCode::Digit2, "2"),
    (KeyCode::Digit3, "3"),
    (KeyCode::Digit4, "4"),
    (KeyCode::Digit5, "5"),
    (KeyCode::Digit6, "6"),
    (KeyCode::Digit7, "7"),
    (KeyCode::Digit8, "8"),
    (KeyCode::Digit9, "9"),
    (KeyCode::ArrowUp, "up"),
    (KeyCode::ArrowDown, "down"),
    (KeyCode::ArrowLeft, "left"),
    (KeyCode::ArrowRight, "right"),
    (KeyCode::Space, "space"),
    (KeyCode::Enter, "enter"),
    (KeyCode::NumpadEnter, "enter"),
    (KeyCode::Tab, "tab"),
    (KeyCode::Backspace, "backspace"),
    (KeyCode::ShiftLeft, "shift"),
    (KeyCode::ShiftRight, "shift"),
    (KeyCode::ControlLeft, "ctrl"),
    (KeyCode::ControlRight, "ctrl"),
    (KeyCode::AltLeft, "alt"),
    (KeyCode::AltRight, "alt"),
    (KeyCode::F1, "f1"),
    (KeyCode::F2, "f2"),
    (KeyCode::F3, "f3"),
    (KeyCode::F4, "f4"),
    (KeyCode::F5, "f5"),
];

/// The manifest key name of a key on the keyboard.
fn key_name(code: KeyCode) -> Option<&'static str> {
    KEY_CODES.iter().find(|(c, _)| *c == code).map(|(_, n)| *n)
}

/// Which events each key name sends, from every module's `[keys]`.
fn bindings<'a>(modules: impl Iterator<Item = &'a Module>) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for m in modules {
        for (key, event) in &m.manifest.keys {
            out.entry(key.clone())
                .or_default()
                .push(format!("{}.{event}", m.id()));
        }
    }
    out
}

fn key_input(event: &str, down: bool) -> Input {
    let mut payload = Map::new();
    payload.insert("down".into(), Value::Bool(down));
    Input {
        tick: 0,
        source: "keyboard".into(),
        name: event.to_owned(),
        payload,
    }
}

/// Frame times, in milliseconds.
#[derive(Default)]
struct Times {
    /// Between one frame and the next, as the player feels it.
    interval: Vec<f64>,
    /// The sim ticks a frame ran (none on most frames at 60 fps; one or two on others).
    sim: Vec<f64>,
    /// Building the scene from the world.
    scene: Vec<f64>,
    /// Handing the scene to the GPU and presenting, which waits for the display (vsync).
    draw: Vec<f64>,
}

fn summary(mut ms: Vec<f64>) -> Value {
    if ms.is_empty() {
        return Value::Null;
    }
    ms.sort_by(f64::total_cmp);
    let at = |q: f64| ms[((ms.len() - 1) as f64 * q).round() as usize];
    let round = |v: f64| (v * 1000.0).round() / 1000.0;
    json!({
        "median": round(at(0.5)),
        "p95": round(at(0.95)),
        "p99": round(at(0.99)),
        "max": round(at(1.0)),
    })
}

impl Times {
    /// Frames after the first few, which load the GPU's pipelines and are not typical.
    const SETTLE: usize = 10;

    fn push(&mut self, interval: f64, sim: f64, scene: f64, draw: f64) {
        self.interval.push(interval);
        self.sim.push(sim);
        self.scene.push(scene);
        self.draw.push(draw);
    }

    fn report(self) -> Value {
        let frames = self.interval.len();
        let settled = |v: Vec<f64>| v.into_iter().skip(Self::SETTLE).collect::<Vec<f64>>();
        let intervals = settled(self.interval);
        let mean = intervals.iter().sum::<f64>() / intervals.len().max(1) as f64;
        json!({
            "frames": frames,
            "fps": if mean > 0.0 { (10_000.0 / mean).round() / 10.0 } else { 0.0 },
            "interval_ms": summary(intervals),
            "sim_ms": summary(settled(self.sim)),
            "scene_ms": summary(settled(self.scene)),
            "draw_ms": summary(settled(self.draw)),
        })
    }
}

struct Running {
    window: Arc<Window>,
    renderer: Renderer,
    view: View,
}

struct App {
    session: Session,
    options: Options,
    size: (u32, u32),
    running: Option<Running>,
    pace: FixedStep,
    keys: BTreeMap<String, Vec<String>>,
    held: BTreeSet<&'static str>,
    pending: Vec<Input>,
    /// Scripted inputs by tick (`--inputs`): the bot.
    scripted: BTreeMap<u64, Vec<Input>>,
    watcher: Option<ModuleWatcher>,
    dirs: BTreeMap<String, PathBuf>,
    hot_load_failures: Vec<Value>,
    started: Instant,
    last: Instant,
    times: Times,
    frames: u64,
    error: Option<Failure>,
}

fn ms_since(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

impl App {
    fn key(&mut self, code: KeyCode, down: bool) {
        let Some(name) = key_name(code) else { return };
        let changed = if down {
            self.held.insert(name)
        } else {
            self.held.remove(name)
        };
        if !changed {
            return;
        }
        for event in self.keys.get(name).into_iter().flatten() {
            self.pending.push(key_input(event, down));
        }
    }

    /// Lets go of every held key (the window lost focus), so none sticks down.
    fn release_all(&mut self) {
        for name in std::mem::take(&mut self.held) {
            for event in self.keys.get(name).into_iter().flatten() {
                self.pending.push(key_input(event, false));
            }
        }
    }

    fn hot_load(&mut self) {
        let Some(watcher) = &self.watcher else { return };
        let changed = watcher.changed();
        if changed.is_empty() {
            return;
        }
        for id in changed {
            let result = Module::read(&self.dirs[&id])
                .map_err(|f| f.message)
                .and_then(|m| self.session.hot_load(m));
            match result {
                Ok(()) => eprintln!("galaxy: hot-loaded {id}"),
                Err(e) => {
                    eprintln!("galaxy: hot-load of {id} failed: {e}");
                    self.hot_load_failures
                        .push(json!({ "tick": self.session.world.tick, "module": id, "error": e }));
                }
            }
        }
        self.keys = bindings(self.session.modules());
        if let Some(r) = &mut self.running {
            r.view.reload(&mut r.renderer, self.session.modules());
        }
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f64();
        self.last = now;
        self.hot_load();
        let Some(r) = &mut self.running else { return };
        let ticks = self.pace.advance(dt);
        let sim_start = Instant::now();
        for _ in 0..ticks {
            r.view.before_step(&self.session.world);
            let mut inputs = std::mem::take(&mut self.pending);
            let tick = self.session.world.tick + 1;
            inputs.extend(self.scripted.remove(&tick).unwrap_or_default());
            if let Err(e) = self.session.step(&inputs) {
                self.error = Some(Failure::runtime(e));
                event_loop.exit();
                return;
            }
        }
        let sim_ms = ms_since(sim_start);
        if self
            .options
            .ticks
            .is_some_and(|end| self.session.world.tick >= end)
        {
            event_loop.exit();
            return;
        }
        let logical = r.renderer.logical_size();
        let time = (now - self.started).as_secs_f32();
        let scene_start = Instant::now();
        let scene = r
            .view
            .scene(&self.session.world, self.pace.alpha(), logical, time);
        let scene_ms = ms_since(scene_start);
        let draw_start = Instant::now();
        if let Err(e) = r.renderer.render(&scene) {
            self.error = Some(Failure::runtime(format!("drawing failed: {e}")));
            event_loop.exit();
            return;
        }
        let draw_ms = ms_since(draw_start);
        self.frames += 1;
        // The first frame's interval is the time since the window opened.
        if self.frames > 1 && self.times.interval.len() < 1_000_000 {
            self.times.push(dt * 1000.0, sim_ms, scene_ms, draw_ms);
        }
        if self.options.frames.is_some_and(|n| self.frames >= n) {
            event_loop.exit();
            return;
        }
        r.window.request_redraw();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.running.is_some() {
            return;
        }
        let title = format!(
            "galaxy: {}",
            self.session
                .modules()
                .map(Module::id)
                .collect::<Vec<_>>()
                .join(", ")
        );
        let attrs = Window::default_attributes()
            .with_title(title)
            .with_inner_size(LogicalSize::new(self.size.0, self.size.1))
            .with_min_inner_size(LogicalSize::new(640, 360));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                self.error = Some(Failure::runtime(format!("can't open a window: {e}")));
                event_loop.exit();
                return;
            }
        };
        let mut renderer = match Renderer::new_windowed(window.clone()) {
            Ok(r) => r,
            Err(e) => {
                self.error = Some(Failure::runtime(e.to_string()));
                event_loop.exit();
                return;
            }
        };
        let view = View::new(
            &mut renderer,
            self.session.modules(),
            self.session.world.seed,
        );
        for problem in view.problems() {
            eprintln!("galaxy: {problem}");
        }
        self.pace.reset();
        self.last = Instant::now();
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
            WindowEvent::Focused(false) => self.release_all(),
            WindowEvent::KeyboardInput { event, .. } => {
                if event.repeat {
                    return;
                }
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                let down = event.state == ElementState::Pressed;
                if code == KeyCode::Escape && down {
                    event_loop.exit();
                    return;
                }
                self.key(code, down);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 60.0,
                };
                if let Some(r) = &mut self.running {
                    r.view.zoom(&self.session.world, steps);
                }
            }
            WindowEvent::RedrawRequested => self.frame(event_loop),
            _ => {}
        }
    }
}

/// The inputs `--inputs` names, by tick, each checked against the session.
fn scripted_inputs(
    session: &Session,
    options: &Options,
) -> Result<BTreeMap<u64, Vec<Input>>, Failure> {
    let Some(path) = &options.inputs else {
        return Ok(BTreeMap::new());
    };
    let script = InputsFile::read(path)
        .map_err(|e| Failure::bad_input(e).with("path", path.display().to_string()))?;
    for input in &script.inputs {
        session.check_input(input).map_err(|e| {
            Failure::bad_input(format!(
                "input `{}` on tick {}: {e}",
                input.name, input.tick
            ))
        })?;
    }
    Ok(script.by_tick())
}

/// `galaxy play`.
pub fn play(args: &[String]) -> Result<Output, Failure> {
    let (positional, options) = parse(args, PLAY_OPTIONS)?;
    if !positional.is_empty() {
        return Err(usage(
            "play takes options only: galaxy play --modules <dir>",
        ));
    }
    let modules = load_modules(&options)?;
    let mut session = Session::new(modules, options.seed.unwrap_or(1)).map_err(bad_modules)?;
    let scripted = scripted_inputs(&session, &options)?;
    if options.record.is_some() {
        session.start_recording(BuildStamp::current());
    }
    let dirs: BTreeMap<String, PathBuf> = session
        .modules()
        .map(|m| (m.id().to_owned(), m.manifest.dir.clone()))
        .collect();
    let watcher = if options.watch {
        Some(
            ModuleWatcher::new(dirs.clone().into_iter().collect())
                .map_err(|e| Failure::runtime(format!("can't watch the module folders: {e}")))?,
        )
    } else {
        None
    };
    let mut app = App {
        keys: bindings(session.modules()),
        session,
        size: options.size.unwrap_or(DEFAULT_SIZE),
        options,
        running: None,
        pace: FixedStep::default(),
        held: BTreeSet::new(),
        pending: Vec::new(),
        scripted,
        watcher,
        dirs,
        hot_load_failures: Vec::new(),
        started: Instant::now(),
        last: Instant::now(),
        times: Times::default(),
        frames: 0,
        error: None,
    };
    let event_loop =
        EventLoop::new().map_err(|e| Failure::runtime(format!("no window system: {e}")))?;
    event_loop
        .run_app(&mut app)
        .map_err(|e| Failure::runtime(format!("the window loop failed: {e}")))?;
    if let Some(e) = app.error {
        return Err(e);
    }
    let build = BuildStamp::current();
    if let Some(path) = &app.options.save {
        save::write(path, &app.session.world, app.session.modules(), &build)
            .map_err(|e| Failure::runtime(e).with("path", path.display().to_string()))?;
    }
    if let Some(path) = &app.options.record {
        let rec = app.session.recording().expect("recording was started");
        rec.write(path)
            .map_err(|e| Failure::runtime(e).with("path", path.display().to_string()))?;
    }
    let problems = app
        .running
        .as_ref()
        .map(|r| r.view.problems())
        .unwrap_or_default();
    Ok(Output::Json(json!({
        "ok": true,
        "command": "play",
        "tick": app.session.world.tick,
        "hash": app.session.world.hash(),
        "frame_time": app.times.report(),
        "adapter": app.running.as_ref().map(|r| r.renderer.adapter_info()),
        "problems": problems,
        "hot_load_failures": app.hot_load_failures,
        "log": app.session.log,
        "saved": app.options.save.as_ref().map(|p| p.display().to_string()),
        "recorded": app.options.record.as_ref().map(|p| p.display().to_string()),
    })))
}

/// `galaxy shot`.
pub fn shot(args: &[String]) -> Result<Output, Failure> {
    let (positional, options) = parse(args, SHOT_OPTIONS)?;
    if !positional.is_empty() {
        return Err(usage(
            "shot takes options only: galaxy shot --modules <dir> --out <file.png>",
        ));
    }
    let out = options
        .out
        .clone()
        .ok_or_else(|| usage("shot needs a file to write: --out <file.png>"))?;
    let modules = load_modules(&options)?;
    let mut session = Session::new(modules, options.seed.unwrap_or(1)).map_err(bad_modules)?;
    let inputs = scripted_inputs(&session, &options)?;
    let (w, h) = options.size.unwrap_or(DEFAULT_SIZE);
    let mut renderer = Renderer::new_headless(w, h)
        .map_err(|e| Failure::runtime(format!("no GPU for the shot: {e}")))?;
    let mut view = View::new(&mut renderer, session.modules(), session.world.seed);
    let ticks = options.ticks.unwrap_or(SHOT_TICKS);
    for _ in 0..ticks {
        view.before_step(&session.world);
        let tick = session.world.tick + 1;
        session
            .step(inputs.get(&tick).map_or(&[][..], Vec::as_slice))
            .map_err(Failure::runtime)?;
    }
    let started = Instant::now();
    let scene = view.scene(&session.world, 1.0, Vec2::new(w as f32, h as f32), 0.0);
    let build_ms = ms_since(started);
    let image = renderer
        .render_to_rgba(&scene)
        .map_err(|e| Failure::runtime(format!("drawing failed: {e}")))?;
    image
        .save_png(&out)
        .map_err(|e| Failure::runtime(e.to_string()).with("path", out.display().to_string()))?;
    Ok(Output::Json(json!({
        "ok": true,
        "command": "shot",
        "out": out.display().to_string(),
        "size": [w, h],
        "tick": session.world.tick,
        "hash": session.world.hash(),
        "scene_ms": (build_ms * 1000.0).round() / 1000.0,
        "models_drawn": scene.meshes.len(),
        "camera": view.camera_json(),
        "adapter": renderer.adapter_info(),
        "problems": view.problems(),
        "log": session.log,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_a_module_may_bind_is_on_the_keyboard() {
        use galaxy_module::manifest::KEYS;
        for name in KEYS {
            assert!(
                KEY_CODES.iter().any(|(_, n)| *n == name),
                "no key for `{name}`"
            );
        }
        for (_, name) in KEY_CODES {
            assert!(KEYS.contains(&name), "`{name}` is not a manifest key name");
        }
        assert_eq!(key_name(KeyCode::KeyW), Some("w"));
        assert_eq!(key_name(KeyCode::ShiftRight), Some("shift"));
        assert_eq!(key_name(KeyCode::F8), None, "F8 is the engine's");
    }

    #[test]
    fn frame_times_skip_the_first_frames() {
        let mut t = Times::default();
        for i in 0..20 {
            let slow = if i < Times::SETTLE { 100.0 } else { 16.0 };
            t.push(slow, 0.1, 0.2, 15.0);
        }
        let r = t.report();
        assert_eq!(r["frames"], 20);
        assert_eq!(r["interval_ms"]["max"], 16.0);
        assert_eq!(r["fps"], 62.5);
    }
}
