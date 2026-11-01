//! `galaxy film`: a scripted session filmed headless, frame by frame on a fixed clock, as
//! the window would show it (`galaxy_view::film`), for the motion tools in `galaxy-gm
//! motion`. With `--builds a,b`, filmed by two installed builds and compared
//! (`galaxy_view::builds`).

use std::path::Path;

use galaxy_module::cli::{self, Output};
use galaxy_module::{Failure, discover};
use galaxy_render::Renderer;
use galaxy_sim::Session;
use galaxy_sim::scenario::InputsFile;
use galaxy_view::View;
use galaxy_view::builds::{self, Drawer};
use galaxy_view::film::{self, Scripted, Shoot, Zoom};
use serde_json::{Value, json};

use crate::sim::{Options, bad_modules, load_modules, parse, usage};

/// A film's size unless `--size` says otherwise: smaller than the window, so a few
/// seconds stay a modest folder.
const DEFAULT_SIZE: (u32, u32) = (960, 540);
/// Ticks a film covers when `--to` is not given: three seconds.
const DEFAULT_LENGTH: u64 = 90;

pub(crate) const OPTIONS: &[&str] = &[
    "--modules",
    "--seed",
    "--inputs",
    "--from",
    "--to",
    "--fps",
    "--size",
    "--out",
    "--probes-only",
    "--builds",
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
    let fps = options.fps.unwrap_or(u64::from(film::DEFAULT_FPS));
    if !(1..=240).contains(&fps) {
        return Err(usage("--fps takes 1 to 240 frames a second"));
    }
    let Some(list) = &options.builds else {
        return film_here(&options, &out, options.probes_only).map(Output::Json);
    };
    let drawers = list
        .split(',')
        .map(|w| builds::resolve(w, None))
        .collect::<Result<Vec<_>, _>>()?;
    if drawers.len() != 2 {
        return Err(usage(
            "--builds takes two builds, such as 0.2.0-<commit>,this (galaxy doctor lists them)",
        ));
    }
    builds::film_on_each(&drawers, &out, !options.probes_only, &mut |d, dir| {
        film_with(&options, d, dir)
    })
    .map(Output::Json)
}

/// Films with `drawer` into `out`: this build itself, or another that draws while this
/// one adds what it doesn't record.
fn film_with(options: &Options, drawer: &Drawer, out: &Path) -> Result<Value, Failure> {
    let Drawer::Other(build) = drawer else {
        return film_here(options, out, options.probes_only);
    };
    let cwd = cli::current_dir()?;
    let modules = options
        .modules
        .as_deref()
        .ok_or_else(|| usage("this needs the modules: --modules <dir>"))?;
    let from = options.from.unwrap_or(0);
    let (w, h) = options.size.unwrap_or(DEFAULT_SIZE);
    // Only what every build since films began takes.
    let mut args = vec![
        "film".to_owned(),
        "--modules".into(),
        discover::resolve(&cwd, Path::new(modules))
            .display()
            .to_string(),
        "--seed".into(),
        options.seed.unwrap_or(1).to_string(),
        "--from".into(),
        from.to_string(),
        "--to".into(),
        options.to.unwrap_or(from + DEFAULT_LENGTH).to_string(),
        "--fps".into(),
        options
            .fps
            .unwrap_or(u64::from(film::DEFAULT_FPS))
            .to_string(),
        "--size".into(),
        format!("{w}x{h}"),
        "--out".into(),
        out.display().to_string(),
    ];
    if let Some(inputs) = &options.inputs {
        args.extend(["--inputs".into(), inputs.display().to_string()]);
    }
    if options.probes_only {
        args.push("--probes-only".into());
    }
    let mut report = builds::run(build, "galaxy", &args)?;
    let scratch = builds::Scratch::new("probes");
    film_here(options, &scratch.0, true)?;
    let probes = galaxy_motion::Film::read(&scratch.0)
        .map_err(|e| Failure::runtime(e).with("path", scratch.0.display().to_string()))?;
    let stamp = json!({
        "drawn": build.name,
        "note": format!("drawn by {} from the modules as they are now", build.name),
    });
    let (added, same) = builds::adopt(out, stamp.clone(), Some(&probes.manifest))
        .map_err(|e| Failure::runtime(e).with("path", out.display().to_string()))?;
    report["builds"] = stamp;
    report["added"] = json!(format!(
        "{} drew it; this build added what it doesn't record: {}",
        build.name,
        if added.is_empty() {
            "nothing".to_owned()
        } else {
            added.join(", ")
        }
    ));
    if same == Some(false) {
        report["world_differs"] = json!(
            "the world on this build differs from the one it drew: a change to the simulation \
             between the builds, so the two films don't show the same moments"
        );
    }
    Ok(report)
}

/// Films with this build.
fn film_here(options: &Options, out: &Path, probes_only: bool) -> Result<Value, Failure> {
    let modules = load_modules(options)?;
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
    let (width, height) = options.size.unwrap_or(DEFAULT_SIZE);
    let mut renderer = if probes_only {
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
        out: out.to_path_buf(),
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
    Ok(json!({
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
        "builds": manifest.builds,
        "next": format!("galaxy-gm motion <tool> <folder>: {}", galaxy_motion::TOOLS.join(", ")),
    }))
}
