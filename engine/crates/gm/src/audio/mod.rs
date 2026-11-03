//! Offline authoring only: measurements, reversible edits, real-mix checks and a judge.
mod check;
mod inspect;
mod judge;
mod meter;
mod mix;
mod playback;
mod process;
mod summary;
#[cfg(test)]
mod tests;

use galaxy_audio::Pcm;
use galaxy_module::{Failure, cli::Output};
use galaxy_motion::Opts;
use serde_json::json;
use std::path::Path;

pub const INSPECT_OPTIONS: &[&str] = &["target-lufs", "reference", "loop", "sound", "detail"];
pub const MIX_OPTIONS: &[&str] = &["out", "stems", "detail"];
pub const JUDGE_OPTIONS: &[&str] = &["intent", "model"];
pub const CHECK_OPTIONS: &[&str] = &["detail"];
pub use process::{LOOP_OPTIONS, OPTIONS as PROCESS_OPTIONS};

pub(super) fn read(path: &Path) -> Result<Pcm, Failure> {
    let pcm =
        Pcm::read(path).map_err(|e| Failure::bad_input(format!("can't decode audio: {e}")))?;
    if !(8_000..=192_000).contains(&pcm.rate) {
        return Err(Failure::bad_input(
            "audio analysis supports sample rates 8000..192000 Hz",
        ));
    }
    Ok(pcm)
}

/// Options may precede the clip (judge --intent text clip) or follow it. Boolean
/// switches never swallow a positional path; duplicate and unknown options fail.
fn parse(args: &[String], known: &[&str]) -> Result<(String, Opts), Failure> {
    let mut path = None;
    let mut opts = Opts::new();
    let mut i = 0;
    while i < args.len() {
        if let Some(raw) = args[i].strip_prefix("--") {
            let (name, inline) = raw
                .split_once('=')
                .map_or((raw, None), |(n, v)| (n, Some(v)));
            if !known.contains(&name) || opts.text(name).is_some() {
                return Err(Failure::usage(format!(
                    "unknown or repeated option --{name}"
                )));
            }
            let value = if let Some(value) = inline {
                value.to_owned()
            } else if ["remove-dc", "declick", "trim-silence", "loop", "detail"].contains(&name) {
                "true".into()
            } else {
                i += 1;
                args.get(i)
                    .filter(|v| !v.starts_with("--"))
                    .ok_or_else(|| Failure::usage(format!("--{name} needs a value")))?
                    .clone()
            };
            opts.set(name, value);
        } else if path.replace(args[i].clone()).is_some() {
            return Err(Failure::usage("takes exactly one clip, scenario or spec"));
        }
        i += 1;
    }
    Ok((
        path.ok_or_else(|| Failure::usage("needs a clip, scenario or spec"))?,
        opts,
    ))
}

pub fn run(command: &str, args: &[String]) -> Result<Output, Failure> {
    if command == "audio" {
        return Err(Failure::usage(
            "choose audio inspect, process, loop, mix, judge or check; galaxy-gm guide audio",
        ));
    }
    let known = match command {
        "audio inspect" => INSPECT_OPTIONS,
        "audio process" => PROCESS_OPTIONS,
        "audio loop" => LOOP_OPTIONS,
        "audio mix" => MIX_OPTIONS,
        "audio judge" => JUDGE_OPTIONS,
        "audio check" => CHECK_OPTIONS,
        _ => return Err(Failure::usage("unknown audio command")),
    };
    // Redact even parse errors and paths for judge, just like the music command.
    if command == "audio judge" {
        return judge::run(
            args,
            known,
            crate::openrouter::ApiKey::from_env(),
            &mut crate::openrouter::Https::new(std::time::Duration::from_secs(120)),
        );
    }
    let (path, opts) = parse(args, known)?;
    let path = Path::new(&path);
    let detailed = process::flag(&opts, "detail")?;
    let mut report = match command {
        "audio inspect" => {
            let target = process::number(&opts, "target-lufs", -70.0, 0.0)?;
            let mut out = if let Some(sound) = opts.text("sound") {
                if opts.text("loop").is_some() {
                    return Err(Failure::usage(
                        "--sound uses module loop settings; omit --loop",
                    ));
                }
                playback::inspect_sound(path, sound, target)?
            } else {
                inspect::inspect_with_loop(&read(path)?, target, process::flag(&opts, "loop")?)
            };
            if let Some(reference) = opts.text("reference") {
                let reference = inspect::inspect(&read(Path::new(reference))?, None);
                let delta = out["loudness"]["integrated_lufs"]
                    .as_f64()
                    .zip(reference["loudness"]["integrated_lufs"].as_f64())
                    .map(|(a, b)| a - b);
                out["reference"] = json!({"loudness_difference_lu":delta,"spectrum":reference["spectrum"],
                    "detail":"Band fractions and centroid are gain-invariant; audition with the reported loudness difference compensated."});
            }
            out
        }
        "audio process" => process::run(path, &opts, false)?,
        "audio loop" => process::run(path, &opts, true)?,
        "audio mix" => mix::run(path, &opts)?,
        "audio check" => return check::run_with_detail(path, detailed),
        _ => unreachable!(),
    };
    if !detailed {
        summary::compact(&mut report);
    }
    Ok(Output::Json(report))
}
