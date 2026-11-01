//! Render the exact presentation mixer offline, driven by an ordinary scripted session.
use crate::sim::{bad_modules, load_modules, parse, usage};
use galaxy_audio::{Action, Bank, Mixer, Scene};
use galaxy_module::{Failure, cli::Output};
use galaxy_sim::{Session, scenario::InputsFile};
use serde_json::json;

/// The same fact cursor drives live and offline presentation. Start at zero for
/// a fresh world, at the saved fact count on resume, or just before a hot-load.
pub(crate) struct Cues {
    scene: Scene,
    next_fact: usize,
}

impl Cues {
    pub(crate) fn new(bank: Bank, first_fact: usize) -> Self {
        Self {
            scene: Scene::new(bank),
            next_fact: first_fact,
        }
    }

    pub(crate) fn update(&mut self, world: &galaxy_sim::World) -> Vec<Action> {
        let actions = self.scene.update(
            world,
            world.facts[self.next_fact..]
                .iter()
                .map(|f| f.kind.as_str()),
        );
        self.next_fact = world.facts.len();
        actions
    }
}

pub(crate) const OPTIONS: &[&str] = &["--modules", "--seed", "--ticks", "--inputs", "--out"];
const RATE: u32 = 48_000;

pub fn render(args: &[String]) -> Result<Output, Failure> {
    let (positional, options) = parse(args, OPTIONS)?;
    if !positional.is_empty() {
        return Err(usage("audio takes options only"));
    }
    let path = options
        .out
        .as_ref()
        .ok_or_else(|| usage("audio needs --out <file.wav>"))?;
    let modules = load_modules(&options)?;
    let bank = Bank::load(&modules);
    if !bank.problems.is_empty() {
        return Err(Failure::bad_input(bank.problems.join("\n")));
    }
    let mut scene = Cues::new(bank, 0);
    let mut session = Session::new(modules, options.seed.unwrap_or(1)).map_err(bad_modules)?;
    let script = options
        .inputs
        .as_ref()
        .map(|p| InputsFile::read(p))
        .transpose()
        .map_err(Failure::bad_input)?
        .unwrap_or_default();
    if !script.clicks.is_empty() || !script.hot_loads.is_empty() {
        return Err(usage(
            "audio accepts event inputs; clicks and hot_load entries are not supported",
        ));
    }
    for input in &script.inputs {
        session.check_input(input).map_err(Failure::bad_input)?;
    }
    let mut inputs = script.by_tick();
    let ticks = options.ticks.unwrap_or(300);
    // PCM RIFF has a 32-bit byte length; reject an unwritable run before creating a file.
    if ticks > (u64::from(u32::MAX) - 44) / (u64::from(RATE / 30) * 4) {
        return Err(usage("audio duration exceeds the WAV size limit"));
    }
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| Failure::bad_input(format!("can't create WAV: {e}")))?;
    let mut writer = hound::WavWriter::new(
        std::io::BufWriter::new(file),
        hound::WavSpec {
            channels: 2,
            sample_rate: RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .map_err(|e| Failure::runtime(e.to_string()))?;
    let mut mixer = Mixer::new(RATE);
    for action in scene.update(&session.world) {
        mixer.apply(action);
    }
    let mut peak = 0.0_f32;
    let mut squares = 0.0_f64;
    let mut samples = 0_u64;
    let mut max_voices = 0;
    for tick in 1..=ticks {
        session
            .step(&inputs.remove(&tick).unwrap_or_default())
            .map_err(Failure::runtime)?;
        for action in scene.update(&session.world) {
            mixer.apply(action);
        }
        max_voices = max_voices.max(mixer.voice_count());
        for _ in 0..RATE / galaxy_api::engine::TICKS_PER_SECOND {
            for sample in mixer.frame() {
                peak = peak.max(sample.abs());
                squares += f64::from(sample).powi(2);
                samples += 1;
                writer
                    .write_sample((sample * 32767.0).round() as i16)
                    .map_err(|e| Failure::runtime(e.to_string()))?;
            }
        }
    }
    writer
        .finalize()
        .map_err(|e| Failure::runtime(e.to_string()))?;
    Ok(Output::Json(
        json!({ "wav": path, "sample_rate": RATE, "frames": samples / 2, "peak": peak,
        "rms": (squares / samples.max(1) as f64).sqrt(), "limited_samples": mixer.limited_samples,
        "stolen_voices": mixer.stolen_voices, "max_voices": max_voices, "ticks": ticks,
        "world_hash": session.world.hash() }),
    ))
}
