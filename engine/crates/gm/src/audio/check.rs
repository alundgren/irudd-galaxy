use galaxy_module::{Failure, cli::Output};
use galaxy_motion::Opts;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    clip: Option<PathBuf>,
    scenario: Option<PathBuf>,
    expect: Vec<Expect>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Expect {
    value: String,
    at_least: Option<f64>,
    at_most: Option<f64>,
    equals: Option<f64>,
}

pub(super) fn run(path: &Path) -> Result<Output, Failure> {
    let text = std::fs::read_to_string(path).map_err(|e| Failure::bad_input(e.to_string()))?;
    let spec: Spec = toml::from_str(&text).map_err(|e| Failure::bad_input(e.to_string()))?;
    if spec.clip.is_some() == spec.scenario.is_some() || spec.expect.is_empty() {
        return Err(Failure::bad_input(
            "audio spec needs exactly one clip or scenario and at least one [[expect]]",
        ));
    }
    for e in &spec.expect {
        let bounds = [e.at_least, e.at_most, e.equals];
        if bounds.iter().all(Option::is_none)
            || bounds.into_iter().flatten().any(|v| !v.is_finite())
            || e.at_least.zip(e.at_most).is_some_and(|(a, b)| a > b)
            || e.equals.is_some() && (e.at_least.is_some() || e.at_most.is_some())
        {
            return Err(Failure::bad_input(
                "expect needs finite ordered at_least/at_most bounds, or equals alone",
            ));
        }
    }
    let base = path.parent().unwrap_or(Path::new("."));
    let report = match spec.clip {
        Some(clip) => super::inspect::inspect(&super::read(&base.join(clip))?, None),
        None => super::mix::run(&base.join(spec.scenario.unwrap()), &Opts::new())?,
    };
    let mut results = vec![];
    let mut findings = vec![];
    for e in spec.expect {
        let got = lookup(&report, &e.value).ok_or_else(|| {
            Failure::bad_input(format!(
                "unknown measurement {}; inspect the report's field names",
                e.value
            ))
        })?;
        if !got.is_null() && !got.is_number() {
            return Err(Failure::bad_input(format!(
                "{} is not a numeric measurement",
                e.value
            )));
        }
        let pass = got.as_f64().is_some_and(|v| {
            e.at_least.is_none_or(|b| v >= b)
                && e.at_most.is_none_or(|b| v <= b)
                && e.equals.is_none_or(|b| v == b)
        });
        let action = action(&e.value);
        results.push(json!({"value":e.value,"got":got,"at_least":e.at_least,"at_most":e.at_most,"equals":e.equals,"pass":pass,
            "reason":got.is_null().then_some("measurement is undefined; see the run's undefined_reason"),"suggested_action":action}));
        if !pass {
            findings.push(super::inspect::finding(
                "expectation",
                0.0,
                report["duration_s"]
                    .as_f64()
                    .unwrap_or_else(|| report["ticks"].as_f64().unwrap_or(0.0) / 30.0),
                results.last().unwrap().clone(),
                action,
            ));
        }
    }
    let ok = findings.is_empty();
    let out = json!({"measurement_version":1,"ok":ok,"reading":if ok {"All numeric expectations passed"} else {"Audio expectations failed; act on each suggested_action"},
        "expectations":results,"findings":findings,"run":report,"detail":"Undefined values fail, never become zero. One local analysis per spec; no paid model calls."});
    if ok {
        Ok(Output::Json(out))
    } else {
        let mut failure = Failure::audio_expectations("audio numeric expectations failed");
        failure.fields = out.as_object().unwrap().clone();
        Err(failure)
    }
}

fn lookup<'a>(report: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(report, |v, key| {
        if let Some(a) = v.as_array() {
            key.parse::<usize>().ok().and_then(|i| a.get(i))
        } else {
            v.get(key)
        }
    })
}
fn action(field: &str) -> &'static str {
    if field.contains("seam") {
        "audio loop --blend 1 --search 1; then inspect the exported loop and audition repeats"
    } else if field.contains("click") {
        "audio process --declick; inspect again"
    } else if field.contains("dc_") {
        "audio process --remove-dc"
    } else if field.contains("silence") {
        "audio process --trim-silence or --trim start,end"
    } else if field.contains("spectrum") {
        "Compare a loudness-matched reference; use gentle shelf/notch EQ or prompt 'rounded attacks, muted timbres, sparse voicing'"
    } else if field.contains("stereo") {
        "Regenerate with 'stable centred bass, mono-compatible stereo'"
    } else {
        "Inspect source and playback levels; process --target-lufs or --gain-db, or adjust audio.toml volume; regenerate damaged clipping with 'clean, undistorted, restrained dynamics'"
    }
}
