//! ui-lab: tries "a screen is a Luau function returning a widget tree" on the market
//! room, the station key, a talk bubble and the drag-to-fit cradle. Throwaway: only the
//! findings carry over (see REPORT.md).
//!
//! `ui-lab` opens the window. `ui-lab --shots <folder>` plays a scripted run headless at
//! 1280x720, saves a PNG per step, hovers everything in every state it passes, and fails
//! if any rule is broken. `ui-lab --look <folder> "<steps>"` plays steps headless and
//! prints what is on screen at each shot (see `shots::look`).

mod app;
mod script;
mod shots;
mod ui;
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
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use app::App;

fn lab_dir(args: &[String]) -> PathBuf {
    args.iter()
        .position(|a| a == "--lab")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lab"))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = lab_dir(&args);
    if let Some(i) = args.iter().position(|a| a == "--shots") {
        let out = args
            .get(i + 1)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("shots"));
        return shots::run(dir, out);
    }
    if let Some(i) = args.iter().position(|a| a == "--look") {
        let (Some(out), Some(script)) = (args.get(i + 1), args.get(i + 2)) else {
            eprintln!("usage: ui-lab --look <folder> \"hover spot_market; shot market\"");
            return ExitCode::FAILURE;
        };
        return shots::look(dir, PathBuf::from(out), script);
    }
    let event_loop = match EventLoop::new() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("ui-lab: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut lab = Lab {
        dir,
        running: None,
        error: None,
        last: Instant::now(),
    };
    if let Err(e) = event_loop.run_app(&mut lab) {
        eprintln!("ui-lab: {e}");
        return ExitCode::FAILURE;
    }
    match lab.error {
        Some(e) => {
            eprintln!("ui-lab: {e}");
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
    running: Option<Running>,
    error: Option<String>,
    last: Instant,
}

impl ApplicationHandler for Lab {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.running.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("ui-lab: screens as Luau widget trees")
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
        let app = App::new(&mut renderer, self.dir.clone());
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
                let viewport = r.renderer.logical_size();
                r.app.world.zoom_at(steps, r.app.ix.pointer, viewport);
            }
            WindowEvent::MouseInput { state, button, .. } => match (button, state) {
                (MouseButton::Left, ElementState::Pressed) => r.app.pointer_down(),
                (MouseButton::Left, ElementState::Released) => r.app.pointer_up(),
                (MouseButton::Right, ElementState::Pressed) => r.app.back(),
                _ => {}
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
            } => match code {
                KeyCode::Escape => r.app.back(),
                KeyCode::F3 => r.app.show_stats = !r.app.show_stats,
                KeyCode::F5 => r.app.reset(),
                _ => {}
            },
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = (now - self.last).as_secs_f32().min(0.1);
                self.last = now;
                let t = Instant::now();
                let scene = r.app.frame(&mut r.renderer, dt);
                if let Err(e) = r.renderer.render(&scene) {
                    eprintln!("ui-lab: {e}");
                }
                let ms = t.elapsed().as_secs_f32() * 1000.0;
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
