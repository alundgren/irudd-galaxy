//! Explicit rendered tests: tests/*.bot.toml. The matching galaxy binary owns the window.
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
}

fn default_seed() -> u64 {
    1
}

fn spec(text: &str, dir: &Path) -> Result<Spec, String> {
    let spec: Spec = toml::from_str(text).map_err(|e| format!("invalid bot declaration: {e}"))?;
    if spec.ticks == 0 || spec.size.iter().any(|n| !(64..=8192).contains(n)) {
        return Err(
            "a bot needs ticks > 0 and size = [width, height], each 64..8192 physical pixels"
                .into(),
        );
    }
    if spec.inputs.as_os_str().is_empty()
        || spec
            .inputs
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("inputs must be a file path relative to the module folder".into());
    }
    let inputs = InputsFile::read(&dir.join(&spec.inputs))?;
    if !inputs.resizes.is_empty() || !inputs.hot_loads.is_empty() {
        return Err(
            "rendered tests use one fixed size and module set; remove resize and hot_load entries"
                .into(),
        );
    }
    Ok(spec)
}

/// Use the executable beside this GM, never an unrelated build on PATH.
fn galaxy() -> Result<PathBuf, String> {
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
        let args = vec![
            "play".into(),
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
            "--no-live".into(),
        ];
        let output = launch(&args)?;
        let play: Value = serde_json::from_slice(&output.stdout).map_err(|_| {
            format!("galaxy play returned no JSON report (exit {:?}); a rendered bot needs a GPU and window: {}",
                output.status.code(), String::from_utf8_lossy(&output.stderr).trim())
        })?;
        if !output.status.success() {
            if play["kind"] == "render_unavailable" {
                report["status"] = json!("unavailable");
            }
            return Err(play["error"]
                .as_str()
                .unwrap_or("galaxy play failed")
                .to_owned());
        }
        for key in [
            "session",
            "size",
            "frames",
            "frame_time",
            "adapter",
            "tick",
            "hash",
            "problems",
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
            return Err("the rendered bot's recording diverged on replay".into());
        }
        if play["tick"] != spec.ticks {
            return Err("the window closed before the requested tick".into());
        }
        if play["size"] != json!(spec.size) {
            return Err("the window did not use the requested physical pixel size".into());
        }
        if play["frame_time"]["interval_ms"].is_null() {
            return Err(
                "the bot produced no measured frame intervals; run it for more ticks".into(),
            );
        }
        if replayed
            .world
            .facts
            .iter()
            .any(|f| f.kind == galaxy_api::engine::SCRIPT_FAILED)
        {
            return Err(
                "a script failed during the rendered bot run; inspect the session recording".into(),
            );
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
mod tests {
    use super::*;

    #[test]
    fn declarations_reject_mistakes_before_opening_a_window() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("run.inputs"), "").unwrap();
        let valid = "inputs = 'run.inputs'\nticks = 60\nsize = [1280, 720]";
        assert!(spec(valid, dir.path()).is_ok());
        for bad in [
            valid.replace("60", "0"),
            valid.replace("1280", "0"),
            valid.replace("run.inputs", "../run.inputs"),
            format!("{valid}\n typo = true"),
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
    fn a_missing_window_is_an_explicit_unavailable_result() {
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
            "inputs = 'run.inputs'\nticks = 60\nsize = [1280, 720]",
            |args| {
                assert!(args.contains(&"--physical-size".to_owned()));
                #[cfg(unix)]
                use std::os::unix::process::ExitStatusExt;
                #[cfg(windows)]
                use std::os::windows::process::ExitStatusExt;
                Ok(Output {
                    status: std::process::ExitStatus::from_raw(256),
                    stdout:
                        br#"{"kind":"render_unavailable","error":"no window system available"}"#
                            .to_vec(),
                    stderr: vec![],
                })
            },
        );
        assert_eq!(output["status"], "unavailable");
        assert_eq!(output["passed"], false);
        assert_eq!(output["failure"], "no window system available");
    }
}
