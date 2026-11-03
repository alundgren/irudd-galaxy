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
    assert!(r["windows"][0]["rms_dbfs"]["music"].is_number());
    assert!(r["windows"][0]["rms_dbfs"]["sound"].is_null());
    let summary = output(run("audio mix", &[scenario.to_str().unwrap().into()]).unwrap());
    assert!(
        summary.to_string().len() < 18000,
        "{}",
        summary.to_string().len()
    );
    assert_eq!(summary["windows"]["window_count"], 2);
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

#[test]
fn bursts_between_old_fft_windows_and_short_edges_are_measured() {
    let mut original: Option<f64> = None;
    for offset in [0, 2400] {
        let mut pcm = tone(2.0, 10000.0, 0.5);
        for (i, f) in pcm.frames.iter_mut().enumerate() {
            let j = (i % 4800) as isize - offset;
            let envelope = if (0..1440).contains(&j) {
                (std::f64::consts::PI * j as f64 / 1440.0).sin().powi(2) as f32
            } else {
                0.0
            };
            for x in f {
                *x *= envelope;
            }
        }
        let report = inspect::inspect(&pcm, None);
        let centroid = num(&report, &["spectrum", "centroid_hz"]);
        assert!((centroid - 10000.0).abs() < 30.0);
        assert!(num(&report, &["spectrum", "air_fraction"]) > 0.99);
        assert!(
            report["findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["kind"] == "bright_band")
        );
        if let Some(previous) = original {
            assert!((centroid - previous).abs() < 1.0);
        }
        original = Some(centroid);
    }
    let short = inspect::spectrum(&tone(0.01, 10000.0, 0.2));
    assert!(short["air_fraction"].as_f64().unwrap() > 0.99);
}

#[test]
fn decimal_edit_endpoints_accept_the_exact_clip_end() {
    let temp = tempfile::tempdir().unwrap();
    for (seconds, option, value) in [(0.1, "fade-out", "0.1"), (4.3, "trim", "0,4.3")] {
        let src = temp.path().join(format!("{option}.wav"));
        let out = temp.path().join(format!("{option}-edited.wav"));
        let pcm = tone(seconds, 1000.0, 0.1);
        process::write(&src, &pcm).unwrap();
        process::run(
            &src,
            &options(&[("out", out.to_str().unwrap()), (option, value)]),
            false,
        )
        .unwrap();
        assert_eq!(read(&out).unwrap().frames.len(), pcm.frames.len());
    }
}

fn resonance(spec: &Value, hz: f64) -> &Value {
    spec["narrow_resonances"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| (r["hz"].as_f64().unwrap() - hz).abs() < 30.0)
        .unwrap_or_else(|| panic!("missing peak {hz}: {}", spec["narrow_resonances"]))
}

#[test]
fn quiet_resonances_survive_notches_and_loop_edits_at_consistent_levels() {
    let temp = tempfile::tempdir().unwrap();
    // Dominant warm bed plus a small partial below the old 1% cutoff and noise floor.
    let mut pcm = tone(4.0, 300.0, 0.2);
    let mut rng = 17u32;
    for (i, f) in pcm.frames.iter_mut().enumerate() {
        rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
        let noise = (f64::from(rng) / f64::from(u32::MAX) - 0.5) * 0.004;
        let partial = 0.008 * (std::f64::consts::TAU * 1184.0 * i as f64 / 48000.0).sin();
        for x in f {
            *x += (partial + noise) as f32;
        }
    }
    let src = temp.path().join("source.wav");
    let cut = temp.path().join("cut.wav");
    let looped = temp.path().join("looped.wav");
    process::write(&src, &pcm).unwrap();
    let report = process::run(
        &src,
        &options(&[("out", cut.to_str().unwrap()), ("notch", "1184,-3,4")]),
        false,
    )
    .unwrap();
    let before = resonance(&report["before"]["spectrum"], 1184.0);
    let after = resonance(&report["after"]["spectrum"], 1184.0);
    let reduction = num(before, &["level_dbfs"]) - num(after, &["level_dbfs"]);
    assert!((reduction - 3.0).abs() < 0.2, "level reduction {reduction}");
    let prominence = num(before, &["prominence_db"]) - num(after, &["prominence_db"]);
    assert!(
        (prominence - 3.0).abs() < 0.7,
        "prominence reduction {prominence}"
    );
    let loop_report = process::run(
        &cut,
        &options(&[
            ("out", looped.to_str().unwrap()),
            ("blend", "0.25"),
            ("search", "0"),
        ]),
        true,
    )
    .unwrap();
    let measured = inspect::inspect(&read(&looped).unwrap(), None);
    assert_eq!(loop_report["after"]["spectrum"], measured["spectrum"]);
    let loop_peak = resonance(&measured["spectrum"], 1184.0);
    assert!((num(loop_peak, &["level_dbfs"]) - num(after, &["level_dbfs"])).abs() < 0.5);
    let boundaries = loop_report["loop_validation"]["boundaries"]
        .as_array()
        .unwrap();
    assert!(
        boundaries
            .windows(2)
            .all(|w| w[0]["at_s"].as_f64() <= w[1]["at_s"].as_f64())
    );
}

#[test]
fn seams_require_loop_intent_and_module_seams_use_the_actual_crossfade() {
    let temp = tempfile::tempdir().unwrap();
    let module = temp.path().join("demo");
    fs::create_dir(&module).unwrap();
    fs::write(
        module.join("manifest.toml"),
        "id='demo'\nversion='0.1.0'\nmin_engine_version='0.5.0'\n",
    )
    .unwrap();
    let mut pcm = tone(8.0, 1000.0, 0.3);
    for (i, f) in pcm.frames.iter_mut().enumerate() {
        let gain = 0.1 + 0.9 * i as f32 / 384000.0;
        for x in f {
            *x *= gain;
        }
    }
    process::write(&module.join("bed.wav"), &pcm).unwrap();
    let ordinary = inspect::inspect(&pcm, None);
    assert!(ordinary["seam"].is_null());
    assert!(
        !ordinary["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["kind"] == "loop_seam")
    );
    let declared = inspect::inspect_with_loop(&pcm, None, true);
    assert!(
        declared["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["kind"] == "loop_seam")
    );
    let table = "[sounds.bed]\nfile='bed.wav'\n[[events]]\nevent='engine.start'\nsound='bed'\nbus='music'\n";
    fs::write(module.join("audio.toml"), table).unwrap();
    let report = playback::inspect_sound(&module, "bed", None).unwrap();
    assert_eq!(report["playback_seam"]["effective_crossfade_s"], 2.0);
    assert!(num(&report, &["source_seam", "level_jump_db"]) > 15.0);
    for b in report["playback_seam"]["boundaries"].as_array().unwrap() {
        assert!(num(b, &["level_jump_db"]) < 3.0, "{b}");
    }
    fs::write(
        module.join("audio.toml"),
        table.replace("file='bed.wav'", "file='bed.wav'\nloop_crossfade=0"),
    )
    .unwrap();
    let hard = playback::inspect_sound(&module, "bed", None).unwrap();
    assert!(num(&hard["playback_seam"]["boundaries"][0], &["level_jump_db"]) > 15.0);
    fs::write(
        module.join("audio.toml"),
        table.replace("bus='music'", "bus='sound'"),
    )
    .unwrap();
    let oneshot = playback::inspect_sound(&module, "bed", None).unwrap();
    assert_eq!(oneshot["looping"], false);
    assert!(oneshot.get("playback_seam").is_none());
    let short = galaxy_audio::Clip::new(tone(1.0, 1000.0, 0.1).frames, 48000)
        .unwrap()
        .with_loop_crossfade(2.0);
    assert_eq!(playback::seam(&short, 1.0)["effective_crossfade_s"], 0.0);
}

#[test]
fn summary_is_bounded_warmth_is_descriptive_and_dc_scales_are_explained() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("warm.wav");
    let out = temp.path().join("dc.wav");
    let mut pcm = tone(30.0, 300.0, 0.1);
    for (i, f) in pcm.frames.iter_mut().enumerate() {
        for x in f {
            *x += if i < 48000 { 0.008 } else { 0.002 };
        }
    }
    process::write(&src, &pcm).unwrap();
    let report = process::run(
        &src,
        &options(&[("out", out.to_str().unwrap()), ("remove-dc", "true")]),
        false,
    )
    .unwrap();
    assert_eq!(
        report["before"]["dc_mean"],
        report["edits"][0]["removed_dc"]
    );
    assert!((num(&report, &["before", "defects", "dc_max"]) - 0.008).abs() < 0.00001);
    let args = vec![src.to_str().unwrap().into()];
    let summary = output(run("audio inspect", &args).unwrap());
    let detailed = output(run("audio inspect", &[args[0].clone(), "--detail".into()]).unwrap());
    assert!(
        summary.to_string().len() < 14000,
        "{}",
        summary.to_string().len()
    );
    assert!(detailed["spectrum"]["timeline"].is_array());
    assert!(
        summary["spectrum"]["timeline"]["window_count"]
            .as_u64()
            .unwrap()
            > 100
    );
    let warm: Vec<_> = summary["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["kind"] == "low_mid_dominance")
        .collect();
    assert_eq!(warm.len(), 1);
    assert!(warm[0]["occurrences"].as_u64().unwrap() > 100);
    assert!(
        !warm[0]["suggested_action"]
            .as_str()
            .unwrap()
            .contains("low-shelf")
    );
    assert!(
        warm[0]["suggested_action"]
            .as_str()
            .unwrap()
            .contains("Keep intended warmth")
    );
}

#[test]
fn successful_expectations_have_no_repair_advice() {
    let temp = tempfile::tempdir().unwrap();
    process::write(&temp.path().join("clean.wav"), &tone(0.2, 1000.0, 0.1)).unwrap();
    let spec = temp.path().join("check.toml");
    fs::write(
        &spec,
        "clip='clean.wav'\n[[expect]]\nvalue='defects.clipped_frames'\nequals=0\n",
    )
    .unwrap();
    let report = output(check::run(&spec).unwrap());
    assert_eq!(report["expectations"][0]["pass"], true);
    assert!(report["expectations"][0].get("suggested_action").is_none());
}

#[test]
fn exact_judge_request_audio_preserves_mp3_and_wav_rate_duration_and_level() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let temp = tempfile::tempdir().unwrap();
    let wav = temp.path().join("processed.wav");
    let src = temp.path().join("source.wav");
    process::write(&src, &tone(1.2, 1184.0, 0.2)).unwrap();
    process::run(
        &src,
        &options(&[
            ("out", wav.to_str().unwrap()),
            ("notch", "1184,-3,4"),
            ("trim", "0,1"),
        ]),
        false,
    )
    .unwrap();
    let mp3 =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/audio/demo/sounds/high.mp3");
    let mp3_edit = temp.path().join("mp3-edited.wav");
    let mp3_loop = temp.path().join("mp3-loop.wav");
    process::run(
        &mp3,
        &options(&[
            ("out", mp3_edit.to_str().unwrap()),
            ("notch", "1000,-3,4"),
            ("trim", "0,0.16"),
        ]),
        false,
    )
    .unwrap();
    process::run(
        &mp3_edit,
        &options(&[
            ("out", mp3_loop.to_str().unwrap()),
            ("blend", "0.02"),
            ("search", "0"),
        ]),
        true,
    )
    .unwrap();
    for path in [mp3, mp3_loop, wav] {
        let source = read(&path).unwrap();
        let mut blind = description();
        blind["segments"][0]["end_s"] = json!(inspect::duration(&source));
        blind["segments"][0]["certainty"] = json!("certain");
        let mut assess = assessment();
        for f in assess["findings"].as_array_mut().unwrap() {
            f["end_s"] = json!(inspect::duration(&source));
        }
        let mut http = Canned {
            replies: VecDeque::from([reply(blind, None), reply(assess, None)]),
            bodies: vec![],
        };
        let report = output(
            judge::run(
                &[
                    path.to_str().unwrap().into(),
                    "--intent".into(),
                    "warm pad".into(),
                ],
                JUDGE_OPTIONS,
                Some(ApiKey::new("test-key")),
                &mut http,
            )
            .unwrap(),
        );
        assert!(
            report["reliability"]
                .as_str()
                .unwrap()
                .contains("Two runs on the same audio can disagree")
        );
        for body in &http.bodies {
            let encoded = body["messages"][0]["content"][1]["input_audio"]["data"]
                .as_str()
                .unwrap();
            let bytes = STANDARD.decode(encoded).unwrap();
            let mut reader = hound::WavReader::new(std::io::Cursor::new(bytes)).unwrap();
            assert_eq!(reader.spec().sample_rate, source.rate);
            assert_eq!(reader.spec().channels, source.channels);
            let decoded: Vec<_> = reader
                .samples::<i16>()
                .map(|s| f64::from(s.unwrap()) / 32768.0)
                .collect();
            assert_eq!(
                decoded.len(),
                source.frames.len() * source.channels as usize
            );
            let mut squares = 0.0;
            for (s, expected) in decoded.iter().zip(
                source
                    .frames
                    .iter()
                    .flat_map(|f| f.iter().take(source.channels as usize)),
            ) {
                assert!((s - f64::from(*expected)).abs() < 0.00005);
                squares += s * s;
            }
            let delta = meter::db(squares / decoded.len() as f64).unwrap()
                - meter::db(inspect::energy(&source.frames)).unwrap();
            assert!(delta.abs() < 0.01, "level delta {delta}");
            eprintln!(
                "payload {}: {} Hz, {} channels, {} frames, {:.6} s, RMS delta {:.6} dB",
                path.extension().unwrap().to_str().unwrap(),
                source.rate,
                source.channels,
                source.frames.len(),
                inspect::duration(&source),
                delta
            );
        }
    }
}

#[test]
fn audio_help_locates_harshness_and_explains_scores_and_judge_uses() {
    let guide = galaxy_module::help::guide_topic("audio").unwrap();
    assert!(guide.contains("narrow_resonances") && guide.contains("attacks/crest"));
    assert!(!guide.contains("high-shelf 4000"));
    assert!(guide.contains("measurements outrank"));
    let loop_help = galaxy_module::help::for_args(
        galaxy_module::discover::Tool::Gm,
        &["audio".into(), "loop".into()],
    );
    assert!(
        loop_help
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .contains("0 best, lower better, unbounded above")
    );
    let judge_help = galaxy_module::help::for_args(
        galaxy_module::discover::Tool::Gm,
        &["audio".into(), "judge".into()],
    );
    let judge_help = judge_help.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(judge_help.contains("One use means one run") && judge_help.contains("two paid calls"));
}
