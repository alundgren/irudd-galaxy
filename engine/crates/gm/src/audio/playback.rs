use super::{inspect, process};
use galaxy_audio::{Action, Bank, Bus, Clip, Mixer, Pcm};
use galaxy_module::{Failure, module::Module};
use serde_json::{Value, json};
use std::path::Path;

/// Inspect a named module sound, distinguishing one-shot use from music/ambience.
pub(super) fn inspect_sound(
    path: &Path,
    name: &str,
    target: Option<f64>,
) -> Result<Value, Failure> {
    let module = Module::read(path)?;
    let bank = Bank::load([&module]);
    if !bank.problems.is_empty() {
        return Err(Failure::bad_input(bank.problems.join("\n")));
    }
    let text = module
        .files
        .get("audio.toml")
        .ok_or_else(|| Failure::bad_input("module has no audio.toml"))?;
    let table: toml::Value = toml::from_str(text).map_err(|e| Failure::bad_input(e.to_string()))?;
    let sound = table
        .get("sounds")
        .and_then(|s| s.get(name))
        .ok_or_else(|| Failure::bad_input("unknown local sound name in audio.toml"))?;
    let file = sound
        .get("file")
        .and_then(toml::Value::as_str)
        .expect("Bank validated sound file");
    let number = |key, default| {
        sound
            .get(key)
            .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
            .unwrap_or(default)
    };
    let crossfade = number("loop_crossfade", 2.0) as f32;
    let volume = number("volume", 1.0) as f32;
    let music = table
        .get("events")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .any(|e| {
            e.get("sound").and_then(toml::Value::as_str) == Some(name)
                && e.get("bus").and_then(toml::Value::as_str) == Some("music")
        });
    let ambience = table
        .get("places")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flat_map(|t| t.values())
        .any(|v| v.as_str() == Some(name));
    let pcm = super::read(&module.manifest.dir.join(file))?;
    let mut report = inspect::inspect(&pcm, target);
    report["sound"] = json!(module.full(name));
    report["looping"] = json!(music || ambience);
    report["reading"] = json!(
        "Source measurements; loop findings use playback_seam rendered with the module sound settings. A one-shot has no loop seam."
    );
    if music || ambience {
        let clip = Clip::read(&module.manifest.dir.join(file))
            .map_err(Failure::bad_input)?
            .with_loop_crossfade(crossfade);
        let playback = seam(&clip, volume);
        for boundary in playback["boundaries"].as_array().unwrap() {
            if boundary["level_jump_db"].as_f64().is_some_and(|v| v > 3.0)
                || boundary["sample_jump"].as_f64().is_some_and(|v| v > 0.05)
                || boundary["slope_jump"].as_f64().is_some_and(|v| v > 0.05)
            {
                let at = boundary["at_s"].as_f64().unwrap();
                report["findings"].as_array_mut().unwrap().push(inspect::finding("loop_seam", at, at, boundary.clone(),
                    "Audition the rendered transition; adjust loop_crossfade or prepare a loop with audio loop, then set loop_crossfade=0."));
            }
        }
        report["source_seam"] = inspect::seam(&pcm);
        report["playback_seam"] = playback;
        report["seam_undefined_reason"] = json!(
            "Use playback_seam for this looping module sound; source_seam is only the raw endpoints."
        );
    }
    Ok(report)
}

/// Render through the actual player mixer: source clamp, resampling, crossfade,
/// wrap position and settled volume/master gain all use the playback implementation.
pub(super) fn seam(clip: &Clip, volume: f32) -> Value {
    let rate = 48_000;
    let source_rate = f64::from(clip.sample_rate());
    let count = clip.frames();
    let overlap = clip.loop_crossfade_frames();
    let period = count - overlap;
    let mut points = vec![];
    for repetition in 0..2 {
        if overlap > 0 {
            points.push((count - overlap + repetition * period, "blend_start"));
        }
        points.push((count + repetition * period, "wrap"));
    }
    let to_output = |frame: usize| (frame as f64 * f64::from(rate) / source_rate).ceil() as usize;
    let last = to_output(points.last().unwrap().0);
    let mut mixer = Mixer::new(rate);
    mixer.apply(Action::Play {
        clip: clip.clone(),
        bus: Bus::Music,
        volume,
        fade: 0.0,
    });
    // Retain only boundary neighborhoods, so even long recordings need bounded memory.
    let radius = rate as usize / 10;
    let mut neighborhoods: Vec<_> = points.iter().map(|_| Vec::new()).collect();
    for i in 0..=last + radius {
        let frame = mixer.frame();
        for ((at, _), neighborhood) in points.iter().zip(&mut neighborhoods) {
            let at = to_output(*at);
            if i >= at.saturating_sub(radius) && i <= at + radius {
                neighborhood.push(frame);
            }
        }
    }
    let boundaries: Vec<_> = points
        .iter()
        .zip(neighborhoods)
        .map(|((at, kind), frames)| {
            let at = to_output(*at);
            let local_at = radius.min(at);
            let mut boundary = if local_at >= 2 {
                process::boundary(
                    &Pcm {
                        frames,
                        rate,
                        channels: 2,
                    },
                    local_at,
                )
            } else {
                json!({"undefined_reason":"loop is too short for a boundary measurement"})
            };
            boundary["at_s"] = json!(at as f64 / f64::from(rate));
            boundary["kind"] = json!(kind);
            boundary
        })
        .collect();
    json!({"sample_rate":rate,"effective_crossfade_s":overlap as f64/source_rate,
        "period_s":period as f64/source_rate,"boundaries":boundaries,
        "detail":"Isolated looping module sound through the real 48 kHz Mixer, declared volume and default master 0.7. Two wraps and blend starts, chronological. Includes playback crossfade and short-clip disabling. Use audio mix for simultaneous buses and event transitions; listen through the whole blend."})
}
