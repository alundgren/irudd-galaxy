use super::{
    inspect,
    meter::{Meter, db},
};
use galaxy_audio::{Bank, Mixer, Scene};
use galaxy_module::{Failure, discover, gate, module::Module};
use galaxy_motion::Opts;
use galaxy_sim::{Session, scenario::InputsFile};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scenario {
    modules: PathBuf,
    ticks: u64,
    #[serde(default = "seed")]
    seed: u64,
    inputs: Option<PathBuf>,
}
fn seed() -> u64 {
    1
}

pub(super) fn run(path: &Path, opts: &Opts) -> Result<Value, Failure> {
    let text = std::fs::read_to_string(path).map_err(|e| Failure::bad_input(e.to_string()))?;
    let scenario: Scenario =
        toml::from_str(&text).map_err(|e| Failure::bad_input(e.to_string()))?;
    if scenario.ticks == 0 || scenario.ticks > 108_000 {
        return Err(Failure::bad_input(
            "mix ticks must be 1..108000 (up to one hour)",
        ));
    }
    let base = path.parent().unwrap_or(Path::new("."));
    let root = base.join(&scenario.modules);
    let dirs = if root.join("manifest.toml").is_file() {
        vec![root]
    } else {
        discover::modules_in(&root)
    };
    if dirs.is_empty() {
        return Err(Failure::bad_input("scenario modules contains no modules"));
    }
    gate::open(&dirs)?;
    let modules: Vec<_> = dirs
        .iter()
        .map(|p| Module::read(p))
        .collect::<Result<_, _>>()?;
    let bank = Bank::load(&modules);
    if !bank.problems.is_empty() {
        return Err(Failure::bad_input(bank.problems.join("\n")));
    }
    let input = scenario
        .inputs
        .as_ref()
        .map(|p| InputsFile::read(&base.join(p)))
        .transpose()
        .map_err(Failure::bad_input)?
        .unwrap_or_default();
    if !input.clicks.is_empty() || !input.pointers.is_empty() || !input.hot_loads.is_empty() {
        return Err(Failure::bad_input(
            "offline audio accepts event inputs only; clicks, pointers and hot loads need a played session",
        ));
    }
    if input.inputs.iter().any(|i| i.tick > scenario.ticks) {
        return Err(Failure::bad_input(
            "scenario contains inputs after its last tick",
        ));
    }
    let mut session = Session::new(modules.clone(), scenario.seed).map_err(Failure::bad_input)?;
    for i in &input.inputs {
        session.check_input(i).map_err(Failure::bad_input)?;
    }
    let mut inputs = input.by_tick();
    let mut sources = vec![];
    for module in &modules {
        if let Some(text) = module.files.get("audio.toml") {
            let table: toml::Value =
                toml::from_str(text).map_err(|e| Failure::bad_input(e.to_string()))?;
            if let Some(sounds) = table.get("sounds").and_then(toml::Value::as_table) {
                for (name, sound) in sounds {
                    let file = sound
                        .get("file")
                        .and_then(toml::Value::as_str)
                        .unwrap_or("");
                    let pcm = super::read(&module.manifest.dir.join(file))?;
                    let volume = sound
                        .get("volume")
                        .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
                        .unwrap_or(1.0);
                    let levels = super::meter::levels(&pcm);
                    sources.push(json!({"sound":module.full(name),"file":file,"source_loudness":levels,
                        "declared_volume":volume,"nominal_gain_db":db((volume*0.7).powi(2)),
                        "detail":"Source measured before clamp, without playback gains. Nominal gain assumes default master and settled voice; the bus report measures actual playback, including overlap and fades."}));
                }
            }
        }
    }
    let rate = 48_000;
    let mut mixer = Mixer::new(rate);
    let mut scene = Scene::new(bank);
    let mut next_fact = 0;
    let update = |scene: &mut Scene, mixer: &mut Mixer, session: &Session, next: &mut usize| {
        for action in scene.update(
            &session.world,
            session.world.facts[*next..].iter().map(|f| f.kind.as_str()),
        ) {
            mixer.apply(action);
        }
        *next = session.world.facts.len();
    };
    update(&mut scene, &mut mixer, &session, &mut next_fact);
    let mut output = Meter::new(rate, 2);
    let mut pre = Meter::new(rate, 2);
    let mut buses: [Meter; 3] = std::array::from_fn(|_| Meter::new(rate, 2));
    let names = ["sound", "music", "ambience"];
    let mut writers = vec![];
    if let Some(out) = opts.text("out") {
        writers.push((None, wav(Path::new(out))?));
    }
    // Each new stem path is explicit in the result. Refuse existing directory so a
    // failed write cannot replace earlier work.
    let stems = opts.text("stems").map(PathBuf::from);
    if let Some(dir) = &stems {
        std::fs::create_dir(dir)
            .map_err(|e| Failure::bad_input(format!("--stems needs a new directory: {e}")))?;
        for (i, name) in names.iter().enumerate() {
            writers.push((Some(i), wav(&dir.join(format!("{name}.wav")))?));
        }
    }
    let mut windows = vec![];
    let mut window_buses: [Vec<[f32; 2]>; 3] =
        std::array::from_fn(|_| Vec::with_capacity(rate as usize));
    let mut max_voices = 0;
    for tick in 1..=scenario.ticks {
        session
            .step(&inputs.remove(&tick).unwrap_or_default())
            .map_err(Failure::runtime)?;
        update(&mut scene, &mut mixer, &session, &mut next_fact);
        max_voices = max_voices.max(mixer.voice_count());
        for _ in 0..rate / galaxy_api::engine::TICKS_PER_SECOND {
            let frame = mixer.frame_with_buses();
            output.push(frame.output);
            pre.push(frame.pre_clamp);
            for (i, bus) in buses.iter_mut().enumerate() {
                bus.push(frame.buses[i]);
                window_buses[i].push(frame.buses[i]);
            }
            for (index, writer) in &mut writers {
                for sample in index.map_or(frame.output, |i| frame.buses[i]) {
                    writer
                        .write_sample(sample)
                        .map_err(|e| Failure::runtime(e.to_string()))?;
                }
            }
        }
        if tick % 30 == 0 || tick == scenario.ticks {
            windows.push(overlap_window(&window_buses, rate, (tick - 1) / 30));
            for b in &mut window_buses {
                b.clear();
            }
        }
    }
    for (_, writer) in writers {
        writer
            .finalize()
            .map_err(|e| Failure::runtime(e.to_string()))?;
    }
    let bus_report: serde_json::Map<String, Value> = names
        .into_iter()
        .zip(buses.iter().map(Meter::report))
        .map(|(n, v)| (n.into(), v))
        .collect();
    let mut findings = vec![];
    if mixer.limited_samples > 0 {
        findings.push(inspect::finding("mix_clipping",0.0,scenario.ticks as f64/30.0,
        json!({"limited_samples":mixer.limited_samples,"pre_clamp_peak_dbfs":pre.report()["sample_peak_dbfs"]}),
        "Lower sound volumes in audio.toml or process sources to lower targets; final clamp cannot restore clipped detail."));
    }
    for w in &windows {
        for pair in w["pairs"].as_array().into_iter().flatten() {
            if pair["spectral_overlap"].as_f64().is_some_and(|v| v > 0.7)
                && pair["level_difference_db"]
                    .as_f64()
                    .is_some_and(|v| v.abs() < 6.0)
            {
                findings.push(inspect::finding("possible_masking",w["start_s"].as_f64().unwrap(),w["end_s"].as_f64().unwrap(),pair.clone(),
                "Audition simultaneous buses; lower the background volume, use gentle EQ, or prompt 'sparse arrangement, space for foreground sounds'. Overlap is an indicator, not proof of masking."));
            }
        }
    }
    Ok(
        json!({"measurement_version":1,"reading":"Source loudness and actual simultaneous bus contributions are separate. Bus levels include all playback gains and fades, before the shared clamp.",
        "sources":sources,"playback":output.report(),"pre_clamp":pre.report(),"buses":bus_report,
        "windows":windows,"findings":findings,"limited_samples":mixer.limited_samples,
        "stolen_voices":mixer.stolen_voices,"max_voices":max_voices,"ticks":scenario.ticks,
        "world_hash":session.world.hash(),"wav":opts.text("out"),"stems":stems,
        "detail":"48 kHz, same Scene and Mixer as play and galaxy audio. Every 1 s window compares simultaneous RMS levels and normalized spectral band overlap. Bus stems are float WAV before final clamp; their sum may exceed full scale. Short effects need peaks and momentary measurements."}),
    )
}

fn wav(path: &Path) -> Result<hound::WavWriter<std::io::BufWriter<std::fs::File>>, Failure> {
    if path.extension().and_then(|e| e.to_str()) != Some("wav") {
        return Err(Failure::usage("output must end in .wav"));
    }
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| Failure::bad_input(format!("can't create new WAV: {e}")))?;
    hound::WavWriter::new(
        std::io::BufWriter::new(file),
        hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .map_err(|e| Failure::runtime(e.to_string()))
}

fn overlap_window(frames: &[Vec<[f32; 2]>; 3], rate: u32, second: u64) -> Value {
    let names = ["sound", "music", "ambience"];
    let specs: Vec<_> = frames
        .iter()
        .map(|f| {
            inspect::spectrum(&galaxy_audio::Pcm {
                frames: f.clone(),
                rate,
                channels: 2,
            })
        })
        .collect();
    let power = frames.each_ref().map(|f| inspect::energy(f));
    let bands = |s: &Value| {
        let low = s["low_mid_fraction"].as_f64().unwrap_or(0.0);
        let mid = s["presence_fraction"].as_f64().unwrap_or(0.0);
        let high = s["air_fraction"].as_f64().unwrap_or(0.0);
        [low, mid, high, (1.0 - low - mid - high).max(0.0)]
    };
    let mut pairs = vec![];
    for (a, b) in [(0, 1), (0, 2), (1, 2)] {
        let overlap = (power[a] > 1e-10 && power[b] > 1e-10).then(|| {
            bands(&specs[a])
                .into_iter()
                .zip(bands(&specs[b]))
                .map(|(a, b)| a.min(b))
                .sum::<f64>()
        });
        pairs.push(json!({"a":names[a],"b":names[b],"level_difference_db":db(power[a]).zip(db(power[b])).map(|(a,b)|a-b),
            "spectral_overlap":overlap,"undefined_reason":overlap.is_none().then_some("one bus is silent or below -100 dBFS")}));
    }
    json!({"start_s":second,"end_s":second as f64+frames[0].len() as f64/f64::from(rate),"rms_dbfs":power.map(db),"pairs":pairs})
}
