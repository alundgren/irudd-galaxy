//! `galaxy-gm perf <module>`: the module's performance scenes (`tests/*.perf.toml`), each
//! played in a real window at real speed, with the frame times the window measured and the
//! change since the last run on this machine. Not a test: `galaxy-gm test` never runs them,
//! and no number fails a scene. Run it once per finished change, on a quiet machine.
//!
//! A scene is a moment: a route (optional) that one window plays fast and undrawn up to
//! tick `at` (`galaxy play --fast-to`), so the screen is as the route left it (zoom,
//! pointer, scroll), then at real speed for `seconds`, the route's later inputs still
//! playing, or none with `hold`, so the window holds that moment (an open talk, a menu).
//! Measuring starts after the window's warm-up frames, as in `galaxy play`. The last result
//! of each scene is kept in [`install::perf_dir`], never in the module.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use galaxy_api::engine::TICKS_PER_SECOND;
use galaxy_module::cli::Output as CliOutput;
use galaxy_module::failure::code;
use galaxy_module::module::Module;
use galaxy_module::{Failure, discover, gate, install, manifest};
use galaxy_sim::BuildStamp;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::bot::{check_inputs, check_screen, default_scale, default_seed, play_report};

/// Options `perf` takes.
pub const OPTIONS: &[&str] = &["only"];

/// When to run it, said in every report.
const WHEN: &str = "once per finished PR, on a quiet machine with no builds running: \
                    the numbers are this machine's and anything else running moves them";

/// The longest a scene may hold the window.
const MAX_SECONDS: u64 = 600;

/// The frame times compared with the last run, each as median, p95, p99 and max.
const TIMES: [&str; 4] = ["interval_ms", "sim_ms", "scene_ms", "draw_ms"];
const STATS: [&str; 4] = ["median", "p95", "p99", "max"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scene {
    /// The route to the moment and on through it; none plays the module's start.
    inputs: Option<PathBuf>,
    /// The tick real time starts on: the window plays the route up to it, fast and undrawn.
    #[serde(default)]
    at: u64,
    /// The route stops at `at`: the window holds that moment instead of playing on.
    #[serde(default)]
    hold: bool,
    /// How long the window plays at real speed.
    seconds: u64,
    size: [u32; 2],
    #[serde(default = "default_scale")]
    scale: f32,
    #[serde(default = "default_seed")]
    seed: u64,
}

fn scene(text: &str, dir: &Path) -> Result<Scene, String> {
    let scene: Scene =
        toml::from_str(text).map_err(|e| format!("invalid performance scene: {e}"))?;
    if !(1..=MAX_SECONDS).contains(&scene.seconds) {
        return Err(format!(
            "seconds is how long the window plays, 1 to {MAX_SECONDS}"
        ));
    }
    check_screen(scene.size, scene.scale)?;
    if scene.hold && (scene.inputs.is_none() || scene.at == 0) {
        return Err("hold stops the route at `at`: it needs inputs and an at above 0".into());
    }
    if let Some(inputs) = &scene.inputs {
        check_inputs(inputs, dir)?;
    }
    Ok(scene)
}

/// Runs the performance scenes of the module at `dir`, one at a time. `only` keeps the
/// scenes `galaxy-gm test --only` would keep.
pub fn run(dir: &Path, only: Option<&str>) -> Result<CliOutput, Failure> {
    let dir = manifest::module_dir(dir);
    gate::open(std::slice::from_ref(&dir))?;
    let module = Module::read(&dir)?;
    // The modules beside it play too, as for its tests.
    let root = match dir.parent() {
        Some(parent) if discover::modules_in(parent).len() > 1 => parent.to_path_buf(),
        _ => dir.clone(),
    };
    let keep = crate::test::only_filter(only);
    let store = install::perf_dir().join(module.id());
    let galaxy = crate::bot::galaxy();
    let scenes: Vec<Value> = module
        .files
        .0
        .iter()
        .filter(|(path, _)| {
            path.starts_with("tests/") && path.ends_with(".perf.toml") && keep(path, "")
        })
        .map(|(path, text)| {
            run_scene(&module, &root, path, text, &store, |args, sessions| {
                Command::new(galaxy.clone()?)
                    .args(args)
                    .env(install::SESSIONS_VAR, sessions)
                    .output()
                    .map_err(|e| format!("could not start galaxy play: {e}"))
            })
        })
        .collect();
    let failures: Vec<String> = scenes
        .iter()
        .filter_map(|s| {
            Some(format!(
                "{}: {}",
                s["file"].as_str()?,
                s["failure"].as_str()?
            ))
        })
        .collect();
    let mut notes = vec![format!("run {WHEN}")];
    if scenes.is_empty() {
        notes.push(match only {
            Some(o) if crate::test::names_a_file(o) => {
                format!("the module has no performance scene `{o}`")
            }
            Some(o) => format!("no performance scene's file contains `{o}`"),
            None => "the module has no performance scenes: add tests/<name>.perf.toml; \
                     galaxy-gm guide tests shows one"
                .to_owned(),
        });
    }
    let out = json!({
        "ok": failures.is_empty(),
        "module": module.id(),
        "build": BuildStamp::current().label(),
        "kept": store.display().to_string(),
        "notes": notes,
        "failures": failures,
        "scenes": scenes,
    });
    if failures.is_empty() {
        return Ok(CliOutput::Json(out));
    }
    let mut failure = Failure::new(
        code::REJECTED,
        "perf_failed",
        format!(
            "{}: {} of {} performance scene(s) did not run",
            module.id(),
            failures.len(),
            out["scenes"].as_array().map_or(0, Vec::len)
        ),
    );
    if let Value::Object(fields) = out {
        failure.fields = fields;
    }
    Err(failure)
}

/// Plays one scene and compares it with the last result kept in `store`, which it then
/// replaces. `launch` runs `galaxy` with these arguments, keeping its sessions in the
/// given folder (thrown away after).
fn run_scene(
    module: &Module,
    modules_dir: &Path,
    file: &str,
    text: &str,
    store: &Path,
    mut launch: impl FnMut(&[String], &Path) -> Result<Output, String>,
) -> Value {
    let mut report = json!({"file": file, "status": "failed"});
    let work = store.join(format!(".work-{}", std::process::id()));
    let result = (|| -> Result<(), String> {
        let scene = scene(text, &module.manifest.dir)?;
        let end = scene.at + scene.seconds * u64::from(TICKS_PER_SECOND);
        report["at"] = json!(scene.at);
        report["seconds"] = json!(scene.seconds);
        report["hold"] = json!(scene.hold);
        report["requested_size"] = json!(scene.size);
        report["scale"] = json!(scene.scale);
        std::fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
        let sessions = work.join("sessions");
        // One window plays the route from the start: a save carries the world but not
        // the screen (zoom, pointer, scroll), so the moment is reached in the same process.
        let mut window = vec![
            "play".to_owned(),
            "--modules".into(),
            modules_dir.display().to_string(),
            "--size".into(),
            format!("{}x{}", scene.size[0], scene.size[1]),
            "--physical-size".into(),
            "--scale".into(),
            scene.scale.to_string(),
            "--seed".into(),
            scene.seed.to_string(),
            "--ticks".into(),
            end.to_string(),
            "--no-live".into(),
        ];
        if let Some(inputs) = &scene.inputs {
            window.push("--inputs".into());
            window.push(module.manifest.dir.join(inputs).display().to_string());
        }
        if scene.at > 0 {
            window.extend(["--fast-to".into(), scene.at.to_string()]);
        }
        if scene.hold {
            window.push("--hold".into());
        }
        let output = launch(&window, &sessions)?;
        let play = play_report(&output).map_err(|(e, unavailable)| {
            if unavailable {
                report["status"] = json!("unavailable");
            }
            e
        })?;
        if play["tick"] != end {
            return Err(format!(
                "the window closed at tick {} before the scene's end at {end}",
                play["tick"]
            ));
        }
        if play["size"] != json!(scene.size) {
            return Err("the window did not use the requested physical pixel size".into());
        }
        let times = &play["frame_time"];
        if times["interval_ms"].is_null() {
            return Err("the window measured no frame intervals; hold the scene longer".into());
        }
        let now = json!({
            "build": BuildStamp::current().label(),
            "when": SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()),
            "size": play["size"],
            "scale": scene.scale,
            "adapter": play["adapter"],
            "frame_time": times,
        });
        for key in ["size", "adapter", "frames"] {
            report[key] = play[key].clone();
        }
        report["frame_time"] = times.clone();
        let kept = store.join(
            Path::new(file)
                .file_name()
                .map(PathBuf::from)
                .unwrap_or_default()
                .with_extension("json"),
        );
        let previous: Option<Value> = std::fs::read_to_string(&kept)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok());
        if let Some(previous) = &previous {
            report["previous"] = json!({
                "build": previous["build"],
                "when": previous["when"],
                "frame_time": previous["frame_time"],
            });
            report["change"] = change(times, &previous["frame_time"]);
            let same = ["size", "scale", "adapter"]
                .iter()
                .all(|k| previous[*k] == now[*k]);
            if !same {
                report["change_note"] = json!(
                    "the last run had another size, scale or GPU: the change is not like for like"
                );
            }
        } else {
            report["previous"] = Value::Null;
            report["change_note"] = json!("no earlier run of this scene on this machine");
        }
        std::fs::write(&kept, serde_json::to_string_pretty(&now).expect("json"))
            .map_err(|e| format!("can't keep the result in {}: {e}", kept.display()))?;
        report["status"] = json!("measured");
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&work);
    if let Err(error) = result {
        report["failure"] = json!(error);
    }
    report
}

/// This run's frame times minus the last run's, in milliseconds (positive is slower), and
/// the change in frames a second.
fn change(now: &Value, before: &Value) -> Value {
    let mut out = json!({});
    for time in TIMES {
        for stat in STATS {
            if let (Some(a), Some(b)) = (now[time][stat].as_f64(), before[time][stat].as_f64()) {
                out[time][stat] = json!(((a - b) * 1000.0).round() / 1000.0);
            }
        }
    }
    if let (Some(a), Some(b)) = (now["fps"].as_f64(), before["fps"].as_f64()) {
        out["fps"] = json!(((a - b) * 10.0).round() / 10.0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::tests::exited;

    fn module(dir: &Path) -> Module {
        std::fs::write(
            dir.join("manifest.toml"),
            "id = 'sample'\nversion = '0.1.0'\nmin_engine_version = '0.1.0'",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("tests")).unwrap();
        std::fs::write(dir.join("tests/route.inputs"), "").unwrap();
        Module::read(dir).unwrap()
    }

    fn played(interval: f64) -> Vec<u8> {
        let stat = |v: f64| json!({"median": v, "p95": v + 1.0, "p99": v + 2.0, "max": v + 3.0});
        json!({
            "ok": true, "tick": 1060, "size": [1280, 720], "frames": 1800, "adapter": "gpu",
            "frame_time": {
                "fps": 60.0, "interval_ms": stat(interval), "sim_ms": stat(0.5),
                "scene_ms": stat(2.0), "draw_ms": stat(10.0),
            },
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn declarations_reject_mistakes_before_opening_a_window() {
        let dir = tempfile::tempdir().unwrap();
        module(dir.path());
        let valid = "inputs = 'tests/route.inputs'\nat = 100\nseconds = 20\nsize = [1280, 720]";
        assert!(scene(valid, dir.path()).is_ok());
        assert!(scene("seconds = 5\nsize = [640, 360]", dir.path()).is_ok());
        for bad in [
            valid.replace("20", "0"),
            valid.replace("20", "601"),
            valid.replace("1280", "10"),
            valid.replace("tests/route", "../route"),
            format!("{valid}\nticks = 5"),
            "hold = true\nat = 100\nseconds = 5\nsize = [640, 360]".into(),
            valid.replace("at = 100", "hold = true"),
        ] {
            assert!(scene(&bad, dir.path()).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_scene_plays_its_route_to_the_moment_in_one_window_and_compares_with_the_last_run() {
        let dir = tempfile::tempdir().unwrap();
        let module = module(dir.path());
        let store = dir.path().join("perf");
        let text = "inputs = 'tests/route.inputs'\nat = 160\nseconds = 30\nsize = [1280, 720]\nscale = 1.25";
        let mut calls: Vec<Vec<String>> = Vec::new();
        let first = run_scene(
            &module,
            dir.path(),
            "tests/busy.perf.toml",
            text,
            &store,
            |args, _| {
                calls.push(args.to_vec());
                Ok(exited(0, &played(16.0)))
            },
        );
        assert_eq!(first["status"], "measured", "{first}");
        assert_eq!(first["previous"], Value::Null);
        // One window plays the route from the start, fast and undrawn to tick 160, so what
        // the route set on the screen (its zoom) is still set; no save hands it over.
        let has = |c: &Vec<String>, flag: &str| c.contains(&flag.to_owned());
        let at =
            |c: &Vec<String>, flag: &str| c[c.iter().position(|a| a == flag).unwrap() + 1].clone();
        assert_eq!(calls.len(), 1);
        let window = &calls[0];
        assert!(!has(window, "--headless") && !has(window, "--load"));
        assert_eq!(at(window, "--fast-to"), "160");
        assert_eq!(at(window, "--ticks"), "1060", "30 seconds on from tick 160");
        assert_eq!(at(window, "--scale"), "1.25");
        assert_eq!(at(window, "--seed"), "1");
        assert!(
            has(window, "--inputs") && !has(window, "--hold"),
            "the route plays on"
        );
        assert!(store.join("busy.perf.json").is_file());

        // Held, the window gets the moment and no more of the route.
        let mut held: Vec<Vec<String>> = Vec::new();
        run_scene(
            &module,
            dir.path(),
            "tests/held.perf.toml",
            &format!("{text}\nhold = true"),
            &store,
            |args, _| {
                held.push(args.to_vec());
                Ok(exited(0, &played(16.0)))
            },
        );
        assert_eq!(held.len(), 1);
        assert!(has(&held[0], "--inputs") && has(&held[0], "--hold"));

        let second = run_scene(
            &module,
            dir.path(),
            "tests/busy.perf.toml",
            text,
            &store,
            |_, _| Ok(exited(0, &played(17.5))),
        );
        assert_eq!(second["change"]["interval_ms"]["median"], 1.5, "{second}");
        assert_eq!(second["change"]["draw_ms"]["p99"], 0.0);
        assert_eq!(
            second["previous"]["frame_time"]["interval_ms"]["median"],
            16.0
        );
        assert!(second.get("change_note").is_none());
        assert!(!store.join(format!(".work-{}", std::process::id())).exists());

        let unavailable = run_scene(
            &module,
            dir.path(),
            "tests/busy.perf.toml",
            text,
            &store,
            |_, _| {
                Ok(exited(
                    1,
                    br#"{"kind":"render_unavailable","error":"no window system available"}"#,
                ))
            },
        );
        assert_eq!(unavailable["status"], "unavailable");
        assert!(
            unavailable["failure"]
                .as_str()
                .unwrap()
                .contains("no window")
        );
    }
}
