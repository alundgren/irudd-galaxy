use super::{
    inspect,
    meter::{Meter, db},
};
use galaxy_api::ui::UiCue;
use galaxy_audio::{Bank, Heard, Mixer, Scene};
use galaxy_module::{Failure, discover, gate, module::Module};
use galaxy_motion::Opts;
use galaxy_sim::recording::RecEvent;
use galaxy_sim::{Input, Recording, Replay, Session, World, scenario::InputsFile};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scenario {
    modules: PathBuf,
    /// Required with inputs; with a recording, defaults to the ticks it played.
    ticks: Option<u64>,
    #[serde(default = "seed")]
    seed: u64,
    inputs: Option<PathBuf>,
    /// A played session's recording.json: its world and its UI sounds, mixed with the
    /// sounds in `modules`.
    recording: Option<PathBuf>,
}
fn seed() -> u64 {
    1
}

/// What drives the mix: a scripted session, or a recording run again.
enum Run<'a> {
    Scripted {
        session: Session,
        inputs: BTreeMap<u64, Vec<Input>>,
    },
    Recorded(Replay<'a>),
}

impl Run<'_> {
    fn world(&self) -> &World {
        match self {
            Run::Scripted { session, .. } => &session.world,
            Run::Recorded(replay) => &replay.session.world,
        }
    }

    fn step(&mut self) -> Result<(), Failure> {
        match self {
            Run::Scripted { session, inputs } => {
                let tick = session.world.tick + 1;
                session
                    .step(&inputs.remove(&tick).unwrap_or_default())
                    .map_err(Failure::runtime)
            }
            Run::Recorded(replay) => replay.step().map(|_| ()).map_err(Failure::runtime),
        }
    }
}

pub(super) fn run(path: &Path, opts: &Opts) -> Result<Value, Failure> {
    let text = std::fs::read_to_string(path).map_err(|e| Failure::bad_input(e.to_string()))?;
    let scenario: Scenario =
        toml::from_str(&text).map_err(|e| Failure::bad_input(e.to_string()))?;
    let base = path.parent().unwrap_or(Path::new("."));
    if scenario.recording.is_some() && scenario.inputs.is_some() {
        return Err(Failure::bad_input(
            "a mix scenario takes inputs or a recording, not both",
        ));
    }
    let recording = scenario
        .recording
        .as_ref()
        .map(|p| Recording::read(&base.join(p)))
        .transpose()
        .map_err(Failure::bad_input)?;
    let played = recording
        .as_ref()
        .map(|r| r.end.tick.saturating_sub(r.start.tick));
    let ticks = scenario
        .ticks
        .or(played)
        .ok_or_else(|| Failure::bad_input("a mix scenario with inputs needs ticks"))?;
    if ticks == 0 || ticks > 108_000 || played.is_some_and(|p| ticks > p) {
        return Err(Failure::bad_input(
            "mix ticks must be 1..108000 (up to one hour), and no more than a recording played",
        ));
    }
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
    if input.needs_window() || !input.hot_loads.is_empty() {
        return Err(Failure::bad_input(
            "offline audio accepts event inputs only; clicks, pointers and hot loads need a played session",
        ));
    }
    if input.inputs.iter().any(|i| i.tick > ticks) {
        return Err(Failure::bad_input(
            "scenario contains inputs after its last tick",
        ));
    }
    let mut run = match &recording {
        Some(rec) => Run::Recorded(Replay::new(rec).map_err(Failure::bad_input)?),
        None => {
            let session =
                Session::new(modules.clone(), scenario.seed).map_err(Failure::bad_input)?;
            for i in &input.inputs {
                session.check_input(i).map_err(Failure::bad_input)?;
            }
            Run::Scripted {
                session,
                inputs: input.by_tick(),
            }
        }
    };
    // A recording's UI cues, by the frame of the mix they start on: the mix's first frame
    // is the screen clock at the recording's first tick.
    let start = run.world().tick;
    let mut cues: Vec<(u64, &UiCue)> = recording
        .iter()
        .flat_map(|r| &r.events)
        .filter_map(|e| match e {
            RecEvent::UiSound(cue) => Some(cue),
            _ => None,
        })
        .map(|cue| {
            let at = (f64::from(cue.time) - start as f64 / 30.0) * 48_000.0;
            (at.max(0.0).round() as u64, cue)
        })
        .collect();
    cues.sort_by_key(|(at, _)| *at);
    let mut cues = cues.into_iter().peekable();
    let mut heard_ui = Vec::new();
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
    // A recording's world already holds its saved facts; only new ones sound.
    let mut next_fact = if recording.is_some() {
        run.world().facts.len()
    } else {
        0
    };
    let update = |scene: &mut Scene, mixer: &mut Mixer, world: &World, next: &mut usize| {
        for action in scene.update(
            world,
            world.facts[*next..]
                .iter()
                .map(|f| Heard::fact(&f.kind, f.tick, &f.payload)),
        ) {
            mixer.apply(action);
        }
        *next = world.facts.len();
    };
    update(&mut scene, &mut mixer, run.world(), &mut next_fact);
    let mut frame_at = 0_u64;
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
    for tick in 1..=ticks {
        run.step()?;
        update(&mut scene, &mut mixer, run.world(), &mut next_fact);
        max_voices = max_voices.max(mixer.voice_count());
        for _ in 0..rate / galaxy_api::engine::TICKS_PER_SECOND {
            while let Some((_, cue)) = cues.next_if(|(at, _)| *at <= frame_at) {
                let played = scene.ui(cue);
                heard_ui.push(json!({"t": frame_at as f64 / f64::from(rate),
                    "action": cue.action.name(), "kind": cue.kind, "look": cue.look,
                    "widget": cue.widget, "sound": played.as_ref().map(|(_, name)| *name)}));
                if let Some((action, _)) = played {
                    mixer.apply(action);
                }
            }
            frame_at += 1;
            max_voices = max_voices.max(mixer.voice_count());
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
        if tick % 30 == 0 || tick == ticks {
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
        findings.push(inspect::finding("mix_clipping",0.0,ticks as f64/30.0,
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
        "stolen_voices":mixer.stolen_voices,"max_voices":max_voices,"ticks":ticks,
        "world_hash":run.world().hash(),"wav":opts.text("out"),"stems":stems,
        "ui_sounds":heard_ui,
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
    json!({"start_s":second,"end_s":second as f64+frames[0].len() as f64/f64::from(rate),"rms_dbfs":names.into_iter().zip(power.map(db)).map(|(name, value)| (name.to_owned(), json!(value))).collect::<serde_json::Map<String, Value>>(),"pairs":pairs})
}
