//! Renders a run of consecutive frames of the demo Wren mining a rock, for checking
//! that shading, ink and the beam stay steady while things move and shrink.
//!
//! ```text
//! cargo run -p render --example frames -- out_dir [--frames 30] [--rate 0.01]
//!     [--pan 0.2] [--zoom 60] [--size 640x360] [--rock 0] [--start 1.0]
//! ```
//!
//! Each frame advances 1/30 s, takes `--rate` of the rock's full size and pans the
//! camera `--pan` world units to the right. Frames are written as `frame_000.png`, ...

use render::Renderer;
use render::demo::DemoWorld;
use render::glam::Vec2;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut out = None;
    let (mut w, mut h) = (640, 360);
    let (mut frames, mut rate, mut pan, mut zoom) = (30u32, 0.01f32, 0.0f32, 60.0f32);
    let (mut rock, mut start) = (0usize, 1.0f32);
    while let Some(a) = args.next() {
        let mut val = || args.next().unwrap_or_default();
        match a.as_str() {
            "--size" => {
                let v = val();
                if let Some((a, b)) = v.split_once('x') {
                    w = a.parse().unwrap_or(w);
                    h = b.parse().unwrap_or(h);
                }
            }
            "--frames" => frames = val().parse().unwrap_or(frames),
            "--rate" => rate = val().parse().unwrap_or(rate),
            "--pan" => pan = val().parse().unwrap_or(pan),
            "--zoom" => zoom = val().parse().unwrap_or(zoom),
            "--rock" => rock = val().parse().unwrap_or(rock),
            "--start" => start = val().parse().unwrap_or(start),
            _ => out = Some(PathBuf::from(a)),
        }
    }
    let Some(out) = out else {
        eprintln!("usage: frames <out_dir> [--frames N] [--rate R] [--pan P] [--zoom H]");
        return ExitCode::FAILURE;
    };
    if let Err(e) = std::fs::create_dir_all(&out) {
        eprintln!("{e}");
        return ExitCode::FAILURE;
    }
    let mut renderer = match Renderer::new_headless(w, h) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let mut world = DemoWorld::new(&mut renderer);
    let Some(target) = world.rocks.get(rock).map(|r| (r.pos, r.radius)) else {
        eprintln!("no rock {rock}");
        return ExitCode::FAILURE;
    };
    // Park the Wren beside the rock, nose on.
    let (pos, radius) = target;
    world.ship_pos = pos + Vec2::new(-radius - 7.0, 3.0);
    world.ship_heading = (pos - world.ship_pos).to_angle();
    world.ship_moving = false;
    world.mining = Some(rock);
    world.rocks[rock].remaining = start;
    world.sync_meshes(&mut renderer);
    world.zoom = zoom;
    world.camera_center = (world.ship_pos + pos) * 0.5;
    for i in 0..frames {
        let scene = world.scene(renderer.logical_size());
        let path = out.join(format!("frame_{i:03}.png"));
        if let Err(e) = renderer
            .render_to_rgba(&scene)
            .and_then(|img| img.save_png(&path))
        {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
        world.time += 1.0 / 30.0;
        let r = &mut world.rocks[rock];
        r.remaining = (r.remaining - rate).max(0.0);
        world.camera_center.x += pan;
        world.sync_meshes(&mut renderer);
    }
    println!(
        "wrote {frames} frames to {} ({w}x{h}, {})",
        out.display(),
        renderer.adapter_info()
    );
    ExitCode::SUCCESS
}
