//! Bot tests: tests/*.bot.toml. The matching galaxy binary plays the route headless, as fast
//! as it goes (`galaxy play --headless`), and the GM replays its recording. Frame times are
//! the performance test's ([`crate::perf`]), never a bot's.
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};

use galaxy_module::{install, module::Module};
use galaxy_sim::{Recording, scenario::InputsFile};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    inputs: PathBuf,
    ticks: u64,
    size: [u32; 2],
    #[serde(default = "default_seed")]
    seed: u64,
    /// The UI scale: logical pixels are `size / scale`, the same on any display.
    #[serde(default = "default_scale")]
    scale: f32,
}

pub(crate) fn default_seed() -> u64 {
    1
}

pub(crate) fn default_scale() -> f32 {
    1.0
}

/// A declared screen: `size` in physical pixels, each 64..8192, at a UI `scale` of 0.5..4.
pub(crate) fn check_screen(size: [u32; 2], scale: f32) -> Result<(), String> {
    if size.iter().any(|n| !(64..=8192).contains(n)) {
        return Err("size = [width, height] takes physical pixels, each 64..8192".into());
    }
    if !(0.5..=4.0).contains(&scale) {
        return Err("scale is the UI scale, from 0.5 to 4 (1.25 for 125% display scaling)".into());
    }
    Ok(())
}

/// A declared inputs file: relative to the module folder, readable, with one size and
/// module set (no resizes or hot-loads).
pub(crate) fn check_inputs(inputs: &Path, dir: &Path) -> Result<(), String> {
    if inputs.as_os_str().is_empty()
        || inputs
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("inputs must be a file path relative to the module folder".into());
    }
    let read = InputsFile::read(&dir.join(inputs))?;
    if !read.resizes.is_empty() || !read.hot_loads.is_empty() {
        return Err(
            "a declared run uses one fixed size and module set; remove resize and hot_load entries"
                .into(),
        );
    }
    Ok(())
}

fn spec(text: &str, dir: &Path) -> Result<Spec, String> {
    let spec: Spec = toml::from_str(text).map_err(|e| format!("invalid bot declaration: {e}"))?;
    if spec.ticks == 0 {
        return Err("a bot needs ticks > 0".into());
    }
    check_screen(spec.size, spec.scale)?;
    check_inputs(&spec.inputs, dir)?;
    Ok(spec)
}

/// Use the executable beside this GM, never an unrelated build on PATH.
pub(crate) fn galaxy() -> Result<PathBuf, String> {
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    let name = if current.file_stem().is_some_and(|s| s == "engine-galaxy-gm") {
        "engine-galaxy"
    } else {
        "galaxy"
    };
    let path = current.with_file_name(install::exe(name));
    if !path.is_file() {
        return Err("the matching galaxy executable is missing beside galaxy-gm; install or build both binaries".into());
    }
    Ok(path)
}

/// `galaxy play`'s report from its output. An error says why it failed, and whether it
/// was because there is no window or GPU (`render_unavailable`).
pub(crate) fn play_report(output: &Output) -> Result<Value, (String, bool)> {
    let play: Value = serde_json::from_slice(&output.stdout).map_err(|_| {
        (
            format!(
                "galaxy play returned no JSON report (exit {:?}): {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
            false,
        )
    })?;
    if !output.status.success() {
        let error = play["error"].as_str().unwrap_or("galaxy play failed");
        return Err((error.to_owned(), play["kind"] == "render_unavailable"));
    }
    Ok(play)
}

pub fn run(module: &Module, modules_dir: &Path, file: &str, text: &str) -> Value {
    run_with(module, modules_dir, file, text, |args| {
        Command::new(galaxy()?)
            .args(args)
            .output()
            .map_err(|e| format!("could not start galaxy play: {e}"))
    })
}

fn run_with(
    module: &Module,
    modules_dir: &Path,
    file: &str,
    text: &str,
    launch: impl FnOnce(&[String]) -> Result<Output, String>,
) -> Value {
    let mut report = json!({"file": file, "passed": false, "status": "failed"});
    let result = (|| -> Result<(), String> {
        let spec = spec(text, &module.manifest.dir)?;
        report["ticks"] = json!(spec.ticks);
        report["requested_size"] = json!(spec.size);
        report["scale"] = json!(spec.scale);
        let args = vec![
            "play".into(),
            "--headless".into(),
            "--modules".into(),
            modules_dir.display().to_string(),
            "--inputs".into(),
            module.manifest.dir.join(&spec.inputs).display().to_string(),
            "--ticks".into(),
            spec.ticks.to_string(),
            "--seed".into(),
            spec.seed.to_string(),
            "--size".into(),
            format!("{}x{}", spec.size[0], spec.size[1]),
            "--physical-size".into(),
            "--scale".into(),
            spec.scale.to_string(),
        ];
        let output = launch(&args)?;
        let play = play_report(&output).map_err(|(error, unavailable)| {
            if unavailable {
                report["status"] = json!("unavailable");
            }
            error
        })?;
        for key in [
            "session",
            "size",
            "frames",
            "adapter",
            "tick",
            "hash",
            "problems",
            "note_problems",
            "resting_clicks",
            "wall_ms",
            "speed",
        ] {
            report[key] = play[key].clone();
        }
        let folder = play["session"]
            .as_str()
            .ok_or("galaxy play omitted its session folder")?;
        let recording = Recording::read(&Path::new(folder).join("recording.json"))?;
        let (replayed, replay) = galaxy_sim::replay(&recording)?;
        report["replay"] =
            json!({"identical": replay.identical, "tick": replay.tick, "hash": replay.hash});
        if !replay.identical {
            return Err("the bot's recording diverged on replay".into());
        }
        if play["tick"] != spec.ticks {
            return Err("the bot stopped before the requested tick".into());
        }
        if play["size"] != json!(spec.size) {
            return Err("the bot did not play at the requested physical pixel size".into());
        }
        if replayed
            .world
            .facts
            .iter()
            .any(|f| f.kind == galaxy_api::engine::SCRIPT_FAILED)
        {
            return Err("a script failed during the bot run; inspect the session recording".into());
        }
        report["passed"] = json!(true);
        report["status"] = json!("passed");
        Ok(())
    })();
    if let Err(error) = result {
        report["failure"] = json!(error);
    }
    report
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A finished child process with this exit code and output.
    pub(crate) fn exited(code: u32, stdout: &[u8]) -> Output {
        #[cfg(unix)]
        use std::os::unix::process::ExitStatusExt;
        #[cfg(windows)]
        use std::os::windows::process::ExitStatusExt;
        #[cfg(unix)]
        let code = code << 8;
        Output {
            status: std::process::ExitStatus::from_raw(code as _),
            stdout: stdout.to_vec(),
            stderr: vec![],
        }
    }

    #[test]
    fn declarations_reject_mistakes_before_playing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("run.inputs"), "").unwrap();
        let valid = "inputs = 'run.inputs'\nticks = 60\nsize = [1280, 720]";
        assert!(spec(valid, dir.path()).is_ok());
        for bad in [
            valid.replace("60", "0"),
            valid.replace("1280", "0"),
            valid.replace("run.inputs", "../run.inputs"),
            format!("{valid}\n typo = true"),
            format!("{valid}\nscale = 0"),
        ] {
            assert!(spec(&bad, dir.path()).is_err());
        }
        std::fs::write(
            dir.path().join("run.inputs"),
            "[[resize]]\ntick = 1\nwidth = 800\nheight = 600",
        )
        .unwrap();
        assert!(spec(valid, dir.path()).is_err());
    }

    #[test]
    fn a_bot_plays_headless_and_a_missing_gpu_is_an_explicit_unavailable_result() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("manifest.toml"),
            "id = 'sample'\nversion = '0.1.0'\nmin_engine_version = '0.1.0'",
        )
        .unwrap();
        std::fs::write(dir.path().join("run.inputs"), "").unwrap();
        let module = Module::read(dir.path()).unwrap();
        let output = run_with(
            &module,
            dir.path(),
            "tests/run.bot.toml",
            "inputs = 'run.inputs'\nticks = 60\nsize = [1280, 720]\nscale = 1.25",
            |args| {
                for flag in ["--headless", "--physical-size"] {
                    assert!(args.contains(&flag.to_owned()), "{flag}");
                }
                let at = args.iter().position(|a| a == "--scale").unwrap();
                assert_eq!(args[at + 1], "1.25");
                Ok(exited(
                    1,
                    br#"{"kind":"render_unavailable","error":"no GPU to draw the notes"}"#,
                ))
            },
        );
        assert_eq!(output["status"], "unavailable");
        assert_eq!(output["passed"], false);
        assert_eq!(output["failure"], "no GPU to draw the notes");
    }
}
