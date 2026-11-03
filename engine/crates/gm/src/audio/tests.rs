use super::*;
use crate::openrouter::{ApiKey, HttpReply, Transport};
use serde_json::{Value, json};
use std::{collections::VecDeque, fs};

fn tone(seconds: f64, hz: f64, amplitude: f64) -> Pcm {
    let rate = 48_000;
    Pcm {
        rate,
        channels: 2,
        frames: (0..(seconds * f64::from(rate)) as usize)
            .map(|i| {
                let v = (amplitude
                    * (std::f64::consts::TAU * hz * i as f64 / f64::from(rate)).sin())
                    as f32;
                [v, v]
            })
            .collect(),
    }
}
fn options(pairs: &[(&str, &str)]) -> Opts {
    let mut o = Opts::new();
    for (k, v) in pairs {
        o.set(k, *v);
    }
    o
}
fn num(v: &Value, path: &[&str]) -> f64 {
    path.iter().fold(v, |v, k| &v[*k]).as_f64().unwrap()
}
fn output(v: Output) -> Value {
    match v {
        Output::Json(v) => v,
        _ => panic!("expected JSON"),
    }
}

#[test]
fn planted_defects_have_timestamps_and_actions() {
    let mut pcm = tone(4.0, 3000.0, 0.6);
    for f in &mut pcm.frames {
        f[0] += 0.04;
        f[1] = -f[0];
    }
    pcm.frames[48_000] = [1.3, -1.3];
    pcm.frames[..4800].fill([0.0; 2]);
    pcm.frames[3 * 48_000..].fill([0.0; 2]);
    let r = inspect::inspect(&pcm, Some(-26.0));
    let findings = r["findings"].as_array().unwrap();
    for kind in [
        "above_target",
        "clipping",
        "dc_offset",
        "head_silence",
        "tail_silence",
        "stereo",
        "bright_band",
    ] {
        assert!(findings.iter().any(|f| f["kind"] == kind), "missing {kind}");
    }
    assert!(findings.iter().all(|f| f["start_s"].is_number()
        && f["end_s"].is_number()
        && f["suggested_action"].is_string()));
    assert!((num(&r, &["spectrum", "centroid_hz"]) - 3000.0).abs() < 150.0);
    assert!(
        !r["spectrum"]["narrow_resonances"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn repairs_preserve_source_reduce_defects_and_refuse_overwrite() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("source.wav");
    let out = temp.path().join("new.wav");
    let mut pcm = tone(4.0, 1000.0, 0.2);
    for f in &mut pcm.frames {
        f[0] += 0.04;
        f[1] += 0.04;
    }
    pcm.frames[48_000] = [0.9; 2];
    process::write(&src, &pcm).unwrap();
    let original = fs::read(&src).unwrap();
    let opts = options(&[
        ("out", out.to_str().unwrap()),
        ("remove-dc", "true"),
        ("declick", "true"),
        ("target-lufs", "-26"),
    ]);
    let r = process::run(&src, &opts, false).unwrap();
    assert!(num(&r, &["before", "defects", "clicks"]) >= 1.0);
    assert_eq!(num(&r, &["after", "defects", "clicks"]), 0.0);
    assert!(num(&r, &["after", "defects", "dc_max"]) < 0.001);
    assert!((num(&r, &["after", "loudness", "integrated_lufs"]) + 26.0).abs() < 0.1);
    assert_eq!(fs::read(&src).unwrap(), original);
    assert!(process::run(&src, &opts, false).is_err());
    assert!(process::write(&src, &pcm).is_err());
}

#[test]
fn eq_reduces_highs_trim_and_fades_have_measurable_effects() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("source.wav");
    let out = temp.path().join("eq.wav");
    let mut pcm = tone(2.0, 1000.0, 0.1);
    let high = tone(2.0, 10000.0, 0.1);
    for (f, h) in pcm.frames.iter_mut().zip(high.frames) {
        for c in 0..2 {
            f[c] += h[c];
        }
    }
    process::write(&src, &pcm).unwrap();
    let r = process::run(
        &src,
        &options(&[
            ("out", out.to_str().unwrap()),
            ("high-shelf", "4000,-6"),
            ("trim", "0.2,1.8"),
            ("fade-in", "0.02"),
            ("fade-out", "0.02"),
        ]),
        false,
    )
    .unwrap();
    assert!(
        num(&r, &["after", "spectrum", "air_fraction"])
            < num(&r, &["before", "spectrum", "air_fraction"]) * 0.6
    );
    let after = read(&out).unwrap();
    assert_eq!(after.frames[0], [0.0; 2]);
    assert_eq!(*after.frames.last().unwrap(), [0.0; 2]);
    assert!((inspect::duration(&after) - 1.6).abs() < 0.001);
    let notch = temp.path().join("notch.wav");
    let n = process::run(
        &src,
        &options(&[("out", notch.to_str().unwrap()), ("notch", "1000,-6,4")]),
        false,
    )
    .unwrap();
    assert!(
        num(&n, &["after", "spectrum", "air_fraction"])
            > num(&n, &["before", "spectrum", "air_fraction"])
    );
}

#[test]
fn loop_fixes_a_level_seam_and_rechecks_exported_repetitions() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("source.wav");
    let out = temp.path().join("loop.wav");
    let mut pcm = tone(4.3, 333.0, 0.3);
    let len = pcm.frames.len() as f32;
    for (i, f) in pcm.frames.iter_mut().enumerate() {
        for s in f {
            *s *= 0.1 + 0.9 * i as f32 / len;
        }
    }
    process::write(&src, &pcm).unwrap();
    let r = process::run(
        &src,
        &options(&[
            ("out", out.to_str().unwrap()),
            ("blend", "0.5"),
            ("search", "0.1"),
        ]),
        true,
    )
    .unwrap();
    assert!(num(&r, &["before", "seam", "level_jump_db"]) > 10.0);
    assert!(
        num(&r, &["after", "seam", "level_jump_db"]) < 3.0,
        "{}",
        r["after"]["seam"]
    );
    assert!(num(&r, &["after", "seam", "sample_jump"]) < 0.02);
    assert_eq!(r["loop_validation"]["repetitions"], 3);
    assert!(
        r["loop_validation"]["clicks"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(num(&r, &["after", "duration_s"]) < 3.9);
}

#[test]
fn undefined_and_out_of_bounds_values_fail_numeric_specs() {
    let temp = tempfile::tempdir().unwrap();
    let wav = temp.path().join("tone.wav");
    let spec = temp.path().join("spec.toml");
    process::write(&wav, &tone(1.0, 1000.0, 0.1)).unwrap();
    fs::write(&spec,"clip='tone.wav'\r\n[[expect]]\r\nvalue='loudness.integrated_lufs'\r\nat_least=-20.1\r\nat_most=-19.9\r\n").unwrap();
    assert_eq!(output(check::run(&spec).unwrap())["ok"], true);
    for (field, bound) in [
        ("loudness.integrated_lufs", "-30"),
        ("loudness.short_term_max_lufs", "0"),
    ] {
        fs::write(
            &spec,
            format!("clip='tone.wav'\n[[expect]]\nvalue='{field}'\nat_most={bound}\n"),
        )
        .unwrap();
        let f = check::run(&spec).err().unwrap();
        assert_eq!(f.code, 5);
        assert_eq!(f.kind, "audio_expectations");
        assert!(f.fields["findings"][0]["suggested_action"].is_string());
    }
    fs::write(
        &spec,
        "clip='tone.wav'\n[[expect]]\nvalue='typo'\nequals=0\n",
    )
    .unwrap();
    assert_eq!(check::run(&spec).err().unwrap().code, 4);
}

#[test]
fn real_mix_has_separate_source_and_playback_levels_and_same_hash() {
    let temp = tempfile::tempdir().unwrap();
    let module = temp.path().join("demo");
    fs::create_dir(&module).unwrap();
    fs::write(
        module.join("manifest.toml"),
        "id='demo'\nversion='0.1.0'\nmin_engine_version='0.5.0'\n",
    )
    .unwrap();
    fs::write(module.join("audio.toml"),"[sounds.tone]\nfile='tone.wav'\nvolume=0.5\nloop_crossfade=0\n[[events]]\nevent='engine.start'\nsound='tone'\nbus='music'\n").unwrap();
    process::write(&module.join("tone.wav"), &tone(1.0, 1000.0, 0.1)).unwrap();
    let scenario = temp.path().join("mix.toml");
    fs::write(&scenario, "modules='demo'\nticks=60\n").unwrap();
    let r = mix::run(&scenario, &Opts::new()).unwrap();
    let src = r["sources"][0]["source_loudness"]["integrated_lufs"]
        .as_f64()
        .unwrap();
    let playback = num(&r, &["buses", "music", "integrated_lufs"]);
    assert!((playback - src - 20.0 * 0.35_f64.log10()).abs() < 0.1);
    assert_eq!(r["limited_samples"], 0);
    let m = galaxy_module::module::Module::read(&module).unwrap();
    let mut session = galaxy_sim::Session::new(vec![m], 1).unwrap();
    for _ in 0..60 {
        session.step(&[]).unwrap();
    }
    assert_eq!(r["world_hash"], session.world.hash());
}

struct Canned {
    replies: VecDeque<Result<HttpReply, String>>,
    bodies: Vec<Value>,
}
impl Transport for Canned {
    fn post(&mut self, _url: &str, _auth: &str, body: &str) -> Result<HttpReply, String> {
        self.bodies.push(serde_json::from_str(body).unwrap());
        self.replies.pop_front().expect("unexpected paid call")
    }
}
fn reply(content: Value, cost: Option<f64>) -> Result<HttpReply, String> {
    Ok(HttpReply {status:200,body:json!({"id":"gen-test","model":"google/test-flash",
    "usage":{"prompt_tokens":100,"completion_tokens":100,"cost":cost},"choices":[{"message":{"content":content.to_string()}}]}).to_string()})
}
fn description() -> Value {
    json!({"description":"steady synthetic tone","segments":[{"start_s":0,"end_s":1,"mood":"neutral","instruments":"uncertain synthesizer","energy":"steady","vocals":"none","certainty":"uncertain"}]})
}
fn assessment() -> Value {
    json!({"verdict":"mismatch","summary":"a steady tone, not the intended arrangement","findings":(["mood","instruments","energy","fit"].map(|a|json!({"start_s":0,"end_s":1,"aspect":a,"assessment":"single tone","certainty":"uncertain","suggested_action":"prompt for layered warm pads"})))})
}

#[test]
fn judge_is_blind_first_reports_actual_cost_and_redacts_all_paths() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("secret-intent.wav");
    process::write(&src, &tone(1.0, 1000.0, 0.1)).unwrap();
    let key = "test-private-key-12345678901234567890";
    let mut blind = description();
    blind["description"] = json!(format!("tone {key}"));
    let mut http = Canned {
        replies: VecDeque::from([reply(blind, Some(0.001)), reply(assessment(), Some(0.002))]),
        bodies: vec![],
    };
    let args = vec![
        "--intent".into(),
        format!("warm orchestral intent {key}"),
        src.to_string_lossy().into_owned(),
    ];
    let r = output(judge::run(&args, JUDGE_OPTIONS, Some(ApiKey::new(key)), &mut http).unwrap());
    assert_eq!(http.bodies.len(), 2);
    assert!(!http.bodies[0].to_string().contains("orchestral"));
    assert!(!http.bodies[0].to_string().contains("secret-intent"));
    assert!(http.bodies[1].to_string().contains("orchestral"));
    assert!(!http.bodies.iter().any(|b| b.to_string().contains(key)));
    assert!(!r.to_string().contains(key));
    assert!((r["cost_usd"].as_f64().unwrap() - 0.003).abs() < 1e-9);
    assert_eq!(r["assessment"]["verdict"], "mismatch");
    let mut http = Canned {
        replies: VecDeque::from([reply(description(), None), Err(format!("failure {key}"))]),
        bodies: vec![],
    };
    let f = judge::run(&args, JUDGE_OPTIONS, Some(ApiKey::new(key)), &mut http)
        .err()
        .unwrap();
    assert_eq!(f.kind, "openrouter");
    assert_eq!(http.bodies.len(), 2);
    assert!(!f.to_json().to_string().contains(key));
    assert!(f.fields["calls"][0]["cost_usd"].is_null());
    let mut http = Canned {
        replies: VecDeque::new(),
        bodies: vec![],
    };
    assert_eq!(
        judge::run(&args, JUDGE_OPTIONS, None, &mut http)
            .err()
            .unwrap()
            .kind,
        "no_key"
    );
    assert!(http.bodies.is_empty());
}

#[test]
fn option_positions_and_invalid_inputs_are_explicit() {
    let args = ["--intent", "warm pads", "clip.wav"].map(str::to_owned);
    let (path, opts) = parse(&args, JUDGE_OPTIONS).unwrap();
    assert_eq!(path, "clip.wav");
    assert_eq!(opts.text("intent"), Some("warm pads"));
    for args in [
        vec!["a.wav", "--image", "foo"],
        vec!["a.wav", "--target-lufs", "NaN"],
        vec!["--intent"],
    ] {
        assert!(
            run(
                "audio inspect",
                &args.into_iter().map(str::to_owned).collect::<Vec<_>>()
            )
            .is_err()
        );
    }
}

#[test]
fn peak_ceiling_wins_over_an_unreachable_loudness_target() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("tone.wav");
    let out = temp.path().join("limited.wav");
    process::write(&src, &tone(1.0, 1000.0, 0.1)).unwrap();
    let r = process::run(
        &src,
        &options(&[
            ("out", out.to_str().unwrap()),
            ("target-lufs", "-5"),
            ("peak", "-12"),
        ]),
        false,
    )
    .unwrap();
    assert_eq!(r["gain"]["peak_limited"], true);
    assert!(num(&r, &["after", "loudness", "true_peak_dbtp"]) <= -11.999);
    assert!(num(&r, &["after", "loudness", "integrated_lufs"]) < -10.0);
}

#[test]
fn intersample_overshoot_is_a_finding_even_without_full_scale_samples() {
    let mut pcm = tone(0.2, 12000.0, 0.1);
    for (i, f) in pcm.frames.iter_mut().enumerate() {
        *f = [(1.2 * (std::f64::consts::TAU * i as f64 / 4.0 + std::f64::consts::FRAC_PI_4).sin())
            as f32; 2];
    }
    let r = inspect::inspect(&pcm, None);
    assert_eq!(r["defects"]["clipped_frames"], 0);
    assert!(
        r["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["kind"] == "true_peak_overshoot")
    );
}

#[test]
fn invalid_judge_timestamps_keep_answer_and_cost_and_do_not_call_assessor() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("tone.wav");
    process::write(&src, &tone(1.0, 1000.0, 0.1)).unwrap();
    let mut blind = description();
    blind["segments"][0]["end_s"] = json!(30);
    let mut http = Canned {
        replies: VecDeque::from([reply(blind, Some(0.001))]),
        bodies: vec![],
    };
    let args = vec![
        src.to_string_lossy().into_owned(),
        "--intent".into(),
        "warm pads".into(),
    ];
    let f = judge::run(
        &args,
        JUDGE_OPTIONS,
        Some(ApiKey::new("test-private-key")),
        &mut http,
    )
    .err()
    .unwrap();
    assert_eq!(http.bodies.len(), 1);
    assert_eq!(f.fields["answer"]["segments"][0]["end_s"], 30);
    assert_eq!(f.fields["calls"][0]["cost_usd"], 0.001);
    assert!(
        http.bodies[0]["messages"][0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("1.000000 seconds")
    );
}
