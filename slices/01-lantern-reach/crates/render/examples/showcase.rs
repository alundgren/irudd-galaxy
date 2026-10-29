//! A window with the demo world. Local demo only; the real game lives in the app crate.
//!
//! WASD flies (and stops when you let go), hold Space near a rock to mine it, P or Escape
//! pauses, the mouse wheel or Q/E zooms, T toggles the camera tilt, F12 saves a PNG.
//!
//! ```text
//! cargo run -p render --example showcase
//! cargo run -p render --example showcase -- --shot out.png   # autopilot, save, quit
//! ```

use render::Renderer;
use render::demo::DemoWorld;
use render::glam::Vec2;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

const SPEED: f32 = 28.0;
const MINE_REACH: f32 = 7.0;
const MINE_RATE: f32 = 0.12;

struct Running {
    window: Arc<Window>,
    renderer: Renderer,
    world: DemoWorld,
    last: Instant,
    mined: f32,
}

#[derive(Default)]
struct App {
    running: Option<Running>,
    keys: HashSet<KeyCode>,
    /// `--shot PATH`: fly and mine for a moment, save a frame through the window
    /// renderer, then quit. Checks the windowed path without a person at the keyboard.
    shot: Option<String>,
    frames: u32,
}

impl App {
    fn update(&mut self, dt: f32) {
        if self.shot.is_some() {
            self.frames += 1;
            let mining = self.frames > 40;
            for (k, on) in [
                (KeyCode::KeyD, !mining),
                (KeyCode::KeyS, !mining),
                (KeyCode::Space, mining),
            ] {
                if on {
                    self.keys.insert(k);
                } else {
                    self.keys.remove(&k);
                }
            }
        }
        let Some(r) = &mut self.running else { return };
        let w = &mut r.world;
        if w.paused {
            return;
        }
        let key = |k| self.keys.contains(&k) as i32 as f32;
        let dir = Vec2::new(
            key(KeyCode::KeyD) - key(KeyCode::KeyA),
            key(KeyCode::KeyW) - key(KeyCode::KeyS),
        );
        w.ship_moving = dir != Vec2::ZERO;
        if w.ship_moving {
            let dir = dir.normalize();
            w.ship_pos += dir * SPEED * dt;
            // Turn smoothly towards the direction of travel.
            let want = dir.to_angle();
            let diff = (want - w.ship_heading + std::f32::consts::PI)
                .rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
            w.ship_heading += diff * (dt * 10.0).min(1.0);
        }
        w.mining = None;
        if self.keys.contains(&KeyCode::Space)
            && let Some(i) = w.rock_in_reach(MINE_REACH)
        {
            w.mining = Some(i);
            let rock = &mut w.rocks[i];
            let take = (MINE_RATE * dt).min(rock.remaining);
            rock.remaining -= take;
            r.mined += take;
            w.hold = ((r.mined * 6.0) as u32).min(6);
        }
        let zoom = key(KeyCode::KeyE) - key(KeyCode::KeyQ);
        w.zoom = (w.zoom * (1.0 + zoom * dt * 1.5)).clamp(30.0, 300.0);
        // The camera follows the ship softly.
        w.camera_center = w.camera_center.lerp(w.ship_pos, (dt * 3.0).min(1.0));
        w.time += dt;
        w.sync_meshes(&mut r.renderer);
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.running.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Lantern Reach - render showcase")
            .with_inner_size(LogicalSize::new(1280.0, 720.0));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("window: {e}");
                event_loop.exit();
                return;
            }
        };
        let mut renderer = match Renderer::new_windowed(window.clone()) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("{e}");
                event_loop.exit();
                return;
            }
        };
        println!("rendering on {}", renderer.adapter_info());
        let mut world = DemoWorld::new(&mut renderer);
        world.camera_center = world.ship_pos;
        world.zoom = 80.0;
        world.ship_moving = false;
        world.mining = None;
        world.hold = 0;
        self.running = Some(Running {
            window,
            renderer,
            world,
            last: Instant::now(),
            mined: 0.0,
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
                if state == ElementState::Pressed {
                    self.keys.insert(code);
                } else {
                    self.keys.remove(&code);
                }
                let Some(r) = &mut self.running else { return };
                if state != ElementState::Pressed || repeat {
                    return;
                }
                match code {
                    KeyCode::KeyP | KeyCode::Escape => r.world.paused = !r.world.paused,
                    KeyCode::KeyT => r.world.tilt = if r.world.tilt > 0.0 { 0.0 } else { 20.0 },
                    KeyCode::F12 => {
                        let scene = r.world.scene(r.renderer.logical_size());
                        match r
                            .renderer
                            .render_to_rgba(&scene)
                            .and_then(|img| img.save_png("showcase.png"))
                        {
                            Ok(()) => println!("saved showcase.png"),
                            Err(e) => eprintln!("{e}"),
                        }
                    }
                    _ => {}
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(r) = &mut self.running {
                    let steps = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(p) => p.y as f32 / 60.0,
                    };
                    r.world.zoom = (r.world.zoom * 0.9f32.powf(steps)).clamp(30.0, 300.0);
                }
            }
            WindowEvent::RedrawRequested => {
                let dt = match &mut self.running {
                    Some(r) => {
                        let now = Instant::now();
                        let dt = (now - r.last).as_secs_f32().min(0.1);
                        r.last = now;
                        dt
                    }
                    None => return,
                };
                self.update(dt);
                if let (Some(path), Some(r)) = (&self.shot, &mut self.running)
                    && self.frames >= 120
                {
                    let scene = r.world.scene(r.renderer.logical_size());
                    match r
                        .renderer
                        .render_to_rgba(&scene)
                        .and_then(|i| i.save_png(path))
                    {
                        Ok(()) => println!("saved {path}"),
                        Err(e) => eprintln!("{e}"),
                    }
                    event_loop.exit();
                    return;
                }
                if let Some(r) = &mut self.running {
                    let scene = r.world.scene(r.renderer.logical_size());
                    if let Err(e) = r.renderer.render(&scene) {
                        eprintln!("{e}");
                        event_loop.exit();
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(r) = &self.running {
            r.window.request_redraw();
        }
    }
}

fn main() {
    let event_loop = match EventLoop::new() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("event loop: {e}");
            return;
        }
    };
    let mut app = App::default();
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--shot") {
        app.shot = Some(
            args.get(i + 1)
                .cloned()
                .unwrap_or_else(|| "showcase.png".into()),
        );
    }
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("{e}");
    }
}
