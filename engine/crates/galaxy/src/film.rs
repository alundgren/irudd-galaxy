//! `galaxy film`: a scripted session filmed headless, frame by frame on a fixed clock, as
//! the window would show it (`galaxy_view::film`), for the motion tools in `galaxy-gm
//! motion`.

use galaxy_module::Failure;
use galaxy_module::cli::Output;
use galaxy_render::Renderer;
use galaxy_sim::Session;
use galaxy_sim::scenario::InputsFile;
use galaxy_view::View;
use galaxy_view::film::{self, Scripted, Shoot, Zoom};
use serde_json::json;

use crate::sim::{bad_modules, load_modules, parse, usage};

/// A film's size unless `--size` says otherwise: smaller than the window, so a few
/// seconds stay a modest folder.
const DEFAULT_SIZE: (u32, u32) = (960, 540);
/// Ticks a film covers when `--to` is not given: three seconds.
const DEFAULT_LENGTH: u64 = 90;

const OPTIONS: &[&str] = &[
    "--modules",
    "--seed",
    "--inputs",
    "--from",
    "--to",
    "--fps",
    "--size",
    "--out",
    "--probes-only",
];

pub fn film(args: &[String]) -> Result<Output, Failure> {
    let (positional, options) = parse(args, OPTIONS)?;
    if !positional.is_empty() {
        return Err(usage(
            "film takes options only: galaxy film --modules <dir> --out <folder>",
        ));
    }
    let out = options
        .out
        .clone()
        .ok_or_else(|| usage("film needs a folder to write: --out <folder>"))?;
    let modules = load_modules(&options)?;
    let session = Session::new(modules, options.seed.unwrap_or(1)).map_err(bad_modules)?;
    let script = match &options.inputs {
        Some(path) => InputsFile::read(path)
            .map_err(|e| Failure::bad_input(e).with("path", path.display().to_string()))?,
        None => InputsFile::default(),
    };
    let zooms = script
        .zooms
        .iter()
        .map(|&(tick, steps)| (tick, Zoom::Steps(steps)))
        .collect();
    let mut scripted = Scripted::new(session, &script).map_err(Failure::bad_input)?;
    let from = options.from.unwrap_or(0);
    let to = options.to.unwrap_or(from + DEFAULT_LENGTH);
    let fps = options.fps.unwrap_or(u64::from(film::DEFAULT_FPS));
    if !(1..=240).contains(&fps) {
        return Err(usage("--fps takes 1 to 240 frames a second"));
    }
    let (width, height) = options.size.unwrap_or(DEFAULT_SIZE);
    let mut renderer = if options.probes_only {
        None
    } else {
        Some(
            Renderer::new_headless(width, height)
                .map_err(|e| Failure::runtime(format!("no GPU for the film: {e}")))?,
        )
    };
    let mut view = match renderer.as_mut() {
        Some(r) => View::new(r, scripted.session.modules(), scripted.session.world.seed),
        None => View::detached(scripted.session.modules(), scripted.session.world.seed),
    };
    let shoot = Shoot {
        fps: fps as u32,
        from,
        to,
        width,
        height,
        scale: 1.0,
        out: out.clone(),
        zooms,
        source: json!({
            "modules": options.modules,
            "inputs": options.inputs.as_ref().map(|p| p.display().to_string()),
            "seed": scripted.session.world.seed,
        }),
    };
    let manifest = film::shoot(&mut scripted, &mut view, renderer.as_mut(), &shoot)
        .map_err(|e| Failure::runtime(e).with("path", out.display().to_string()))?;
    let first = &manifest.frames[0];
    let last = manifest.frames.last().expect("a film has frames");
    Ok(Output::Json(json!({
        "ok": true,
        "command": "film",
        "out": out.display().to_string(),
        "frames": manifest.frames.len(),
        "fps": manifest.fps,
        "size": [width, height],
        "drawn": manifest.drawn,
        "from": { "t": first.time, "tick": first.tick },
        "to": { "t": last.time, "tick": last.tick },
        "hash": last.hash,
        "adapter": manifest.adapter,
        "problems": view.problems(),
        "next": "galaxy-gm motion <tool> <folder>: track, strip, sheet, flicker, flow, compare",
    })))
}
