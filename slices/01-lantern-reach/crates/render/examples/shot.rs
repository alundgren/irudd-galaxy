//! Renders the demo world headless and writes a PNG.
//!
//! ```text
//! cargo run -p render --example shot -- out.png [--size 1280x720] [--paused] [--tilt 20]
//!     [--zoom 120] [--center X,Y] [--time 1.5]
//! ```

use render::Renderer;
use render::demo::DemoWorld;
use render::glam::Vec2;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut out = None;
    let (mut w, mut h) = (1280, 720);
    let (mut paused, mut tilt, mut zoom, mut center, mut time) = (false, None, None, None, 1.0);
    let mut gallery = false;
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
            "--paused" => paused = true,
            "--gallery" => gallery = true,
            "--tilt" => tilt = val().parse().ok(),
            "--zoom" => zoom = val().parse().ok(),
            "--time" => time = val().parse().unwrap_or(time),
            "--center" => {
                let v = val();
                if let Some((x, y)) = v.split_once(',') {
                    center = Some(Vec2::new(
                        x.parse().unwrap_or(0.0),
                        y.parse().unwrap_or(0.0),
                    ));
                }
            }
            _ => out = Some(a),
        }
    }
    let Some(out) = out else {
        eprintln!(
            "usage: shot <out.png> [--size WxH] [--paused] [--tilt DEG] [--zoom H] [--center X,Y]"
        );
        return ExitCode::FAILURE;
    };
    let mut renderer = match Renderer::new_headless(w, h) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let mut world = DemoWorld::new(&mut renderer);
    world.paused = paused;
    world.time = time;
    if let Some(t) = tilt {
        world.tilt = t;
    }
    if let Some(z) = zoom {
        world.zoom = z;
    }
    if let Some(c) = center {
        world.camera_center = c;
    }
    let scene = if gallery {
        render::demo::gallery(&mut renderer)
    } else {
        world.scene(renderer.logical_size())
    };
    match renderer
        .render_to_rgba(&scene)
        .and_then(|img| img.save_png(&out))
    {
        Ok(()) => {
            println!("wrote {out} ({w}x{h}, {})", renderer.adapter_info());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
