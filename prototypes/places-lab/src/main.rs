//! places-lab: how does a station become a place you are in? Tries ways of going between
//! places, of showing a room, of people talking and of people living, switched live with
//! keys `1`-`4`. Throwaway: only the findings carry over (a REPORT.md at the end).
//!
//! `places-lab` opens the window. `places-lab --shots <folder>` plays a scripted run
//! headless at 1280x720 (station, each room, talk, back), saves a PNG per step, reports
//! what a player would feel, and fails on any rule break. `places-lab --look <folder>
//! "<steps>"` plays steps headless and prints what is on screen at each shot. Both take
//! `--combo cut,staged,bubble,still`.

mod app;
mod combo;
mod figures;
mod life;
mod notes;
mod places;
mod room;
mod script;
mod sets;
mod shots;
mod stage;
mod talk;
mod travel;
mod ui;
mod widgets;
mod world;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use render::Renderer;
use render::glam::Vec2;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Window, WindowId};

use app::App;
use combo::Combo;

fn arg<'a>(args: &'a [String], flag: &str) -> Option<&'a String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
}

fn lab_dir(args: &[String]) -> PathBuf {
    arg(args, "--lab")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lab"))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = lab_dir(&args);
    let mut combo = Combo::new();
    if let Some(spec) = arg(&args, "--combo")
        && let Err(e) = combo.set(spec)
    {
        eprintln!("places-lab: {e}");
        return ExitCode::FAILURE;
    }
    if let Some(i) = args.iter().position(|a| a == "--shots") {
        let out = args
            .get(i + 1)
            .filter(|a| !a.starts_with("--"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("shots"));
        return shots::run(dir, out, combo);
    }
    if let Some(i) = args.iter().position(|a| a == "--look") {
        let (Some(out), Some(script)) = (args.get(i + 1), args.get(i + 2)) else {
            eprintln!("usage: places-lab --look <folder> \"click key_office; shot office\"");
            return ExitCode::FAILURE;
        };
        return shots::look(dir, PathBuf::from(out), script, combo);
    }
    let event_loop = match EventLoop::new() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("places-lab: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut lab = Lab {
        dir,
        combo: Some(combo),
        running: None,
        error: None,
        last: Instant::now(),
        mods: ModifiersState::empty(),
    };
    if let Err(e) = event_loop.run_app(&mut lab) {
        eprintln!("places-lab: {e}");
        return ExitCode::FAILURE;
    }
    match lab.error {
        Some(e) => {
            eprintln!("places-lab: {e}");
            ExitCode::FAILURE
        }
        None => ExitCode::SUCCESS,
    }
}

struct Running {
    window: Arc<Window>,
    renderer: Renderer,
    app: App,
}

struct Lab {
    dir: PathBuf,
    combo: Option<Combo>,
    running: Option<Running>,
    error: Option<String>,
    last: Instant,
    mods: ModifiersState,
}

impl ApplicationHandler for Lab {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.running.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("places-lab: a station as a place you are in")
            .with_inner_size(LogicalSize::new(1280.0, 720.0));
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
        let combo = self.combo.take().unwrap_or_else(Combo::new);
        let app = match App::new(&mut renderer, self.dir.clone(), combo) {
            Ok(a) => a,
            Err(e) => {
                self.error = Some(e);
                event_loop.exit();
                return;
            }
        };
        window.request_redraw();
        self.last = Instant::now();
        self.running = Some(Running {
            window,
            renderer,
            app,
        });
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(r) = &mut self.running else { return };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                r.renderer.resize(size.width, size.height);
                r.app.resized();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                r.renderer.set_scale_factor(scale_factor as f32);
                r.app.resized();
            }
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::CursorMoved { position, .. } => {
                let s = r.renderer.scale_factor();
                r.app
                    .pointer_move(Vec2::new(position.x as f32, position.y as f32) / s);
            }
            WindowEvent::CursorLeft { .. } => r.app.pointer_left(),
            WindowEvent::MouseWheel { delta, .. } => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 60.0,
                };
                if r.app.model.here().kind == places::Kind::Station {
                    let viewport = r.renderer.logical_size();
                    r.app.world.zoom_at(steps, r.app.ix.pointer, viewport);
                }
            }
            WindowEvent::MouseInput { state, button, .. } if r.app.note.is_none() => {
                match (button, state) {
                    (MouseButton::Left, ElementState::Pressed) => r.app.pointer_down(),
                    (MouseButton::Left, ElementState::Released) => r.app.pointer_up(),
                    (MouseButton::Right, ElementState::Pressed) => r.app.back(),
                    _ => {}
                }
            }
            // Writing a note: keys go to the note.
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key,
                        state: ElementState::Pressed,
                        text,
                        ..
                    },
                ..
            } if r.app.note.is_some() => match physical_key {
                PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => r.app.save_note(),
                PhysicalKey::Code(KeyCode::Escape) => r.app.note = None,
                PhysicalKey::Code(KeyCode::Backspace) => {
                    if let Some(d) = &mut r.app.note {
                        d.text.pop();
                    }
                }
                _ => {
                    if let (Some(t), Some(d)) = (text, &mut r.app.note) {
                        d.text.extend(t.chars().filter(|c| !c.is_control()));
                    }
                }
            },
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state: ElementState::Pressed,
                        repeat: false,
                        ..
                    },
                ..
            } => {
                let back = self.mods.shift_key();
                match code {
                    KeyCode::Escape => r.app.back(),
                    KeyCode::F3 => r.app.show_stats = !r.app.show_stats,
                    KeyCode::F5 => r.app.reset(&mut r.renderer),
                    KeyCode::F8 => r.app.want_note = true,
                    KeyCode::Digit1 => r.app.cycle(0, back),
                    KeyCode::Digit2 => r.app.cycle(1, back),
                    KeyCode::Digit3 => r.app.cycle(2, back),
                    KeyCode::Digit4 => r.app.cycle(3, back),
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = (now - self.last).as_secs_f32().min(0.1);
                self.last = now;
                let t = Instant::now();
                let scene = r.app.frame(&mut r.renderer, dt);
                let ms = t.elapsed().as_secs_f32() * 1000.0;
                if r.app.want_note {
                    // The note's shot: this frame, before the note box covers it.
                    r.app.want_note = false;
                    let img = r.renderer.render_to_rgba(&scene).ok();
                    r.app.begin_note(img);
                }
                if let Err(e) = r.renderer.render(&scene) {
                    eprintln!("places-lab: {e}");
                }
                let m = &mut r.app.measures;
                m.frame_ms = if m.frame_ms == 0.0 {
                    ms
                } else {
                    m.frame_ms * 0.9 + ms * 0.1
                };
                r.window.request_redraw();
            }
            _ => {}
        }
    }
}
