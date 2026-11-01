use galaxy_audio::{Bank, Mixer, Scene};
use galaxy_module::module::Module;
use galaxy_sim::{Session, recording, scenario::InputsFile};
use std::path::{Path, PathBuf};

fn engine() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn module() -> Module {
    Module::read(&engine().join("tests/play/audio/demo")).unwrap()
}

#[test]
fn scripted_mix_has_audible_events_and_crossfade_without_changing_world_or_replay() {
    let module = module();
    let bank = Bank::load([&module]);
    assert!(bank.problems.is_empty(), "{:?}", bank.problems);
    let mut plain = module.clone();
    plain.files.0.remove("audio.toml");
    let mut silent = Session::new(vec![plain], 7).unwrap();
    let mut session = Session::new(vec![module], 7).unwrap();
    session.start_recording(galaxy_sim::BuildStamp::current());
    let mut scene = Scene::new(bank);
    let mut mixer = Mixer::new(48000);
    let mut inputs = InputsFile::read(&engine().join("tests/play/audio-inputs.toml"))
        .unwrap()
        .by_tick();
    let mut pcm = vec![];
    for a in scene.update(&session.world, []) {
        mixer.apply(a);
    }
    for tick in 1..=300 {
        let input = inputs.remove(&tick).unwrap_or_default();
        let first = session.world.facts.len();
        session.step(&input).unwrap();
        silent.step(&input).unwrap();
        for action in scene.update(
            &session.world,
            session.world.facts[first..].iter().map(|f| f.kind.as_str()),
        ) {
            mixer.apply(action);
        }
        let mut block = [[0.0; 2]; 1600];
        mixer.render(&mut block);
        pcm.extend(block);
        assert_eq!(session.world.hash(), silent.world.hash(), "tick {tick}");
    }
    assert_eq!(mixer.limited_samples, 0);
    let amplitude = |start: f64, duration: f64, hz: f64| {
        let start = (start * 48000.0) as usize;
        let n = (duration * 48000.0) as usize;
        let mut re = 0.0;
        let mut im = 0.0;
        for (i, frame) in pcm[start..start + n].iter().enumerate() {
            let phase = 2.0 * std::f64::consts::PI * hz * i as f64 / 48000.0;
            re += f64::from(frame[0]) * phase.cos();
            im += f64::from(frame[0]) * phase.sin();
        }
        2.0 * re.hypot(im) / n as f64
    };
    assert!(amplitude(1.5, 0.1, 220.0) > 0.08);
    assert!(amplitude(1.02, 0.1, 880.0) > 0.1, "event ping");
    assert!(amplitude(2.05, 0.1, 220.0) > amplitude(2.85, 0.1, 220.0));
    assert!(amplitude(2.05, 0.1, 440.0) < amplitude(2.85, 0.1, 440.0));
    assert!(amplitude(3.5, 0.1, 220.0) < 0.0001);
    assert!(amplitude(3.5, 0.1, 440.0) > 0.08);
    assert!(amplitude(6.0, 0.1, 330.0) > 0.01, "music loop");
    assert!(amplitude(7.7, 0.1, 330.0) < 0.0001, "music stopped");
    let record = session.recording().unwrap();
    let (replay, _) = recording::replay(&record).unwrap();
    assert_eq!(replay.world.hash(), session.world.hash());
}

#[test]
fn missing_corrupt_and_misdeclared_assets_are_findings() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("manifest.toml"),
        "id='demo'\nversion='0.1.0'\nmin_engine_version='0.3.0'\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("broken.wav"), b"not audio").unwrap();
    std::fs::write(dir.path().join("audio.toml"), "[sounds.missing]\nfile='missing.wav'\n[sounds.broken]\nfile='broken.wav'\n[sounds.escape]\nfile='../other.wav'\n").unwrap();
    let module = Module::read(dir.path()).unwrap();
    let bank = Bank::load([&module]);
    assert_eq!(bank.problems.len(), 3, "{:?}", bank.problems);
    for name in ["missing", "broken", "escape"] {
        assert!(bank.problems.iter().any(|p| p.contains(name)));
    }
}
