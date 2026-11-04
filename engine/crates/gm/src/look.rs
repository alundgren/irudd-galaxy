use galaxy_module::{Failure, cli::Output};
use galaxy_motion::Opts;
use galaxy_render::glam::Vec2;
use galaxy_sim::{Recording, Replay, recording::RecEvent};
use galaxy_view::{
    Moment, View,
    film::{Zoom, recorded_zooms},
};
use serde_json::json;
use std::path::Path;

pub const OPTIONS: &[&str] = &["note", "tick", "point", "size"];

pub fn run(path: &Path, opts: &Opts) -> Result<Output, Failure> {
    opts.only("look", OPTIONS).map_err(Failure::usage)?;
    let rec = Recording::read(path).map_err(Failure::bad_input)?;
    let note = opts.count("note").map_err(Failure::usage)?;
    let tick = opts.count("tick").map_err(Failure::usage)?;
    if note.is_some() == tick.is_some() {
        return Err(Failure::usage(
            "choose exactly one of --note <n> or --tick <n>",
        ));
    }
    let mut moment = None;
    let mut expected_hash = None;
    let tick = if let Some(n) = note {
        let notes: Vec<_> = rec
            .events
            .iter()
            .filter_map(|e| match e {
                RecEvent::Note {
                    tick, camera, hash, ..
                } => Some((*tick, camera, hash)),
                _ => None,
            })
            .collect();
        let (tick, camera, hash) = notes
            .get(n.wrapping_sub(1))
            .ok_or_else(|| Failure::usage("--note counts from 1; no such note"))?;
        expected_hash = (*hash).clone();
        moment = Some(
            serde_json::from_value::<Moment>(
                (*camera)
                    .clone()
                    .ok_or_else(|| Failure::bad_input("note has no recorded frame"))?,
            )
            .map_err(|e| Failure::bad_input(e.to_string()))?,
        );
        *tick
    } else {
        tick.unwrap() as u64
    };
    if tick > rec.end.tick {
        return Err(Failure::usage("--tick is beyond the recording"));
    }
    let point = match opts.text("point") {
        Some(p) => {
            let (x, y) = p
                .split_once(',')
                .ok_or_else(|| Failure::usage("--point takes logical pixels x,y"))?;
            Vec2::new(
                x.parse().map_err(|_| Failure::usage("invalid point x"))?,
                y.parse().map_err(|_| Failure::usage("invalid point y"))?,
            )
        }
        None => moment
            .as_ref()
            .and_then(|m| m.screens.pointer)
            .map(Vec2::from)
            .ok_or_else(|| {
                Failure::usage("--point x,y is required unless the note kept a pointer")
            })?,
    };
    if !point.is_finite() {
        return Err(Failure::usage("point must be finite"));
    }
    if moment.is_some() && opts.text("size").is_some() {
        return Err(Failure::usage(
            "a note uses its recorded size; --size is for --tick",
        ));
    }
    let mut size = Vec2::new(1280.0, 720.0);
    if let Some(s) = opts.text("size") {
        let (w, h) = s
            .split_once('x')
            .ok_or_else(|| Failure::usage("--size takes WIDTHxHEIGHT"))?;
        size = Vec2::new(
            w.parse().map_err(|_| Failure::usage("invalid width"))?,
            h.parse().map_err(|_| Failure::usage("invalid height"))?,
        );
        if !size.is_finite() || size.min_element() < 64.0 || size.max_element() > 8192.0 {
            return Err(Failure::usage("size must be 64 to 8192"));
        }
    }
    let mut replay = Replay::new(&rec).map_err(Failure::bad_input)?;
    let mut view = View::detached(replay.session.modules(), replay.session.world.seed);
    let mut zooms = recorded_zooms(&rec).into_iter().peekable();
    loop {
        while zooms
            .peek()
            .is_some_and(|z| z.0 <= replay.session.world.tick)
        {
            match zooms.next().unwrap().1 {
                Zoom::Steps(s) => view.zoom(&replay.session.world, s),
                Zoom::View(v) => view.eye.view = Some(v),
                Zoom::Presentation(p) => view.screens.presentation = *p,
                Zoom::Still(still) => view.eye.still = still,
            }
        }
        if replay.session.world.tick == tick {
            break;
        }
        view.before_step(&replay.session.world);
        replay.step().map_err(Failure::runtime)?;
        if replay.modules_changed {
            view.reload(None, replay.session.modules());
        }
    }
    if expected_hash.is_some_and(|hash| hash != replay.session.world.hash()) {
        return Err(Failure::new(
            galaxy_module::failure::code::REJECTED,
            "replay_diverged",
            "the world at this note differs from the recording",
        ));
    }
    let alpha = if let Some(m) = &moment {
        size = Vec2::new(m.width as f32, m.height as f32) / m.scale;
        view.note_scene(&replay.session.world, m);
        // look must use the exact style as well as the boxes reconstructed for this note.
        view.screens.style = m.screen_style.unwrap_or(view.screens.style);
        m.alpha
    } else {
        view.scene(&replay.session.world, 1.0, size, tick as f32 / 30.0);
        1.0
    };
    let mut result = view.look(&replay.session.world, alpha, size, point);
    result["tick"] = json!(tick);
    result["note"] = json!(note);
    result["size"] = json!(size.to_array());
    result["alpha"] = json!(alpha);
    result["ok"] = json!(true);
    let selection = note
        .map(|n| format!("--note {n}"))
        .unwrap_or_else(|| format!("--tick {tick} --size {}x{}", size.x, size.y));
    result["builds"] = galaxy_sim::recording::builds(
        &rec.build,
        &galaxy_sim::BuildStamp::current(),
        "looked",
        &format!(
            "galaxy-gm look \"{}\" {} --point {},{}",
            path.display(),
            selection,
            point.x,
            point.y
        ),
    );

    Ok(Output::Json(result))
}
