//! Channel loops, loops that follow a world value, cue variation and the contact cue, on
//! the generic `hum` module (`engine/tests/play/loops/hum`), headless.
use galaxy_audio::{Action, Bank, Clip, Heard, Mixer, Scene};
use galaxy_module::module::Module;
use galaxy_sim::{Session, scenario::InputsFile};
use std::path::{Path, PathBuf};

const RATE: f64 = 48000.0;

fn engine() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn module() -> Module {
    Module::read(&engine().join("tests/play/loops/hum")).unwrap()
}

/// The scripted run's mix, with the world hash after each tick.
fn render(module: Module) -> (Vec<[f32; 2]>, Vec<String>, Session) {
    let bank = Bank::load([&module]);
    assert!(bank.problems.is_empty(), "{:?}", bank.problems);
    let mut session = Session::new(vec![module], 7).unwrap();
    let mut scene = Scene::new(bank);
    let mut mixer = Mixer::new(48000);
    let mut inputs = InputsFile::read(&engine().join("tests/play/loops-inputs.toml"))
        .unwrap()
        .by_tick();
    let mut next = 0;
    let mut pcm = vec![];
    let mut hashes = vec![];
    for tick in 0..=300 {
        if tick > 0 {
            session
                .step(&inputs.remove(&tick).unwrap_or_default())
                .unwrap();
        }
        let facts = &session.world.facts[next..];
        for action in scene.update(
            &session.world,
            facts
                .iter()
                .map(|f| Heard::fact(&f.kind, f.tick, &f.payload)),
        ) {
            mixer.apply(action);
        }
        next = session.world.facts.len();
        if tick > 0 {
            let mut block = [[0.0; 2]; 1600];
            mixer.render(&mut block);
            pcm.extend(block);
            hashes.push(session.world.hash());
        }
    }
    assert_eq!(mixer.limited_samples, 0);
    (pcm, hashes, session)
}

/// The amplitude of `hz` in the left channel over `duration` seconds from `start`.
fn amplitude(pcm: &[[f32; 2]], start: f64, duration: f64, hz: f64) -> f64 {
    let first = (start * RATE) as usize;
    let n = (duration * RATE) as usize;
    let (mut re, mut im) = (0.0, 0.0);
    for (i, frame) in pcm[first..first + n].iter().enumerate() {
        let phase = 2.0 * std::f64::consts::PI * hz * i as f64 / RATE;
        re += f64::from(frame[0]) * phase.cos();
        im += f64::from(frame[0]) * phase.sin();
    }
    2.0 * re.hypot(im) / n as f64
}

/// The strongest frequency between `lo` and `hi` Hz, in 1 Hz steps.
fn strongest(pcm: &[[f32; 2]], start: f64, duration: f64, lo: u32, hi: u32) -> f64 {
    (lo..=hi)
        .map(f64::from)
        .max_by(|a, b| {
            amplitude(pcm, start, duration, *a).total_cmp(&amplitude(pcm, start, duration, *b))
        })
        .unwrap()
}

#[test]
fn a_channel_loop_starts_and_stops_on_its_events_and_audio_changes_no_world() {
    let (pcm, hashes, session) = render(module());
    // The beam (330 Hz) is held from tick 31 to tick 181: on with the press, steady,
    // then gone after its 0.3 s fade.
    // The hum's own partials leak a little into any short window, so silence is
    // measured against the beam's level.
    let before = amplitude(&pcm, 0.5, 0.4, 330.0);
    let held: Vec<f64> = [1.6, 2.8, 4.0, 5.2, 5.8]
        .iter()
        .map(|&t| amplitude(&pcm, t, 0.1, 330.0))
        .collect();
    for level in &held {
        assert!((level - held[0]).abs() < 0.002, "steady: {held:?}");
    }
    assert!(held[0] > 0.05, "{held:?}");
    assert!(before < held[0] * 0.02, "silent before: {before} {held:?}");
    let after = amplitude(&pcm, 6.5, 0.4, 330.0);
    assert!(after < held[0] * 0.02, "faded out after the stop: {after}");
    // Sound is presentation only: the world is the same without the audio table.
    let mut plain = module();
    plain.files.0.remove("audio.toml");
    let (_, plain_hashes, _) = render(plain);
    assert_eq!(hashes, plain_hashes);
    let (replay, _) = galaxy_sim::recording::replay(&{
        let mut s = Session::new(vec![module()], 7).unwrap();
        s.start_recording(galaxy_sim::BuildStamp::current());
        let mut inputs = InputsFile::read(&engine().join("tests/play/loops-inputs.toml"))
            .unwrap()
            .by_tick();
        for tick in 1..=300 {
            s.step(&inputs.remove(&tick).unwrap_or_default()).unwrap();
        }
        s.recording().unwrap()
    })
    .unwrap();
    assert_eq!(replay.world.hash(), session.world.hash());
}

#[test]
fn the_engine_hum_follows_the_ship_s_speed_and_the_ambience_its_distance() {
    let (pcm, _, _) = render(module());
    // At rest the 110 Hz hum plays at pitch 0.8; at top speed (14) at 1.6.
    let rest = strongest(&pcm, 6.5, 0.25, 70, 120);
    assert!((rest - 88.0).abs() <= 1.0, "{rest}");
    let fast = strongest(&pcm, 8.0, 0.25, 140, 200);
    assert!((fast - 176.0).abs() <= 1.5, "{fast}");
    assert!(amplitude(&pcm, 8.0, 0.25, 176.0) > amplitude(&pcm, 6.5, 0.25, 88.0));
    // Pinned against the rock, it settles back near rest.
    let pinned = strongest(&pcm, 9.5, 0.25, 70, 120);
    assert!((pinned - 88.0).abs() <= 2.0, "{pinned}");
    // The bay's 165 Hz ambience is at full gain by the beacon and quieter 17 units off.
    let near = amplitude(&pcm, 5.0, 0.5, 165.0);
    let far = amplitude(&pcm, 9.4, 0.5, 165.0);
    assert!(far < near * 0.4 && far > near * 0.2, "{near} {far}");
}

#[test]
fn each_ping_varies_within_its_range_and_the_same_run_gives_the_same_draws() {
    let (pcm, _, session) = render(module());
    let pings: Vec<u64> = session
        .world
        .facts
        .iter()
        .filter(|f| f.kind == "hum.ping")
        .map(|f| f.tick)
        .collect();
    assert_eq!(pings.len(), 3);
    // A fact cues on the tick after it was recorded; measure inside each ping.
    let pitches: Vec<f64> = pings
        .iter()
        .map(|&t| strongest(&pcm, (t as f64) / 30.0 + 0.03, 0.1, 780, 980))
        .collect();
    for p in &pitches {
        assert!(
            (792.0..=968.0).contains(p),
            "within 880 +/- 10%: {pitches:?}"
        );
    }
    assert!(
        pitches[0] != pitches[1] || pitches[1] != pitches[2],
        "the pings differ: {pitches:?}"
    );
    let (again, _, _) = render(module());
    assert_eq!(pcm, again, "seeded by tick and entity, not by chance");
}

#[test]
fn the_contact_fact_cues_the_bump_once() {
    let (pcm, _, session) = render(module());
    let contacts: Vec<_> = session
        .world
        .facts
        .iter()
        .filter(|f| f.kind == "engine.contact")
        .collect();
    assert_eq!(contacts.len(), 1, "{contacts:?}");
    let c = contacts[0];
    assert_eq!(c.payload["entity"], "hum.ship");
    assert_eq!(c.payload["other"], "hum.rock");
    assert!(c.payload["speed"].as_f64().unwrap() > 10.0);
    let at = c.tick as f64 / 30.0;
    let during = amplitude(&pcm, at + 0.05, 0.1, 140.0);
    let before = amplitude(&pcm, at - 0.4, 0.1, 140.0);
    assert!(
        during > 0.02 && before < during * 0.25,
        "{before} then {during}"
    );
}

#[test]
fn a_loop_on_the_same_channel_carries_on_and_another_crossfades() {
    let a = Clip::new(vec![[0.5, 0.5]; 1000], 1000).unwrap();
    let b = Clip::new(vec![[0.25, 0.25]; 1000], 1000).unwrap();
    let start = |clip: &Clip| Action::Loop {
        clip: clip.clone(),
        channel: 0,
        volume: 1.0,
        fade: 0.01,
        pitch: 1.0,
    };
    let mut mixer = Mixer::new(1000);
    mixer.apply(start(&a));
    let mut block = [[0.0; 2]; 50];
    mixer.render(&mut block);
    mixer.apply(start(&a));
    assert_eq!(mixer.voice_count(), 1, "the same sound keeps playing");
    mixer.apply(start(&b));
    mixer.render(&mut block);
    assert_eq!(mixer.voice_count(), 1, "the old loop faded out");
    assert!((block[49][0] - 0.25 * 0.7).abs() < 1e-5, "{}", block[49][0]);
    // A loop never ends by itself; a stop on its channel ends it.
    let mut long = [[0.0; 2]; 3000];
    mixer.render(&mut long);
    assert_eq!(mixer.voice_count(), 1);
    mixer.apply(Action::StopChannel {
        channel: 0,
        fade: 0.01,
    });
    mixer.render(&mut block);
    assert_eq!(mixer.voice_count(), 0);
}

#[test]
fn tunes_ramp_gain_and_rate_and_reset_puts_them_back() {
    // A one-second ramp from 0 to 1: its value is how far the loop has played.
    let ramp: Vec<[f32; 2]> = (0..1000).map(|i| [i as f32 / 1000.0; 2]).collect();
    let mut mixer = Mixer::new(1000);
    mixer.apply(Action::Tune {
        channel: Some(3),
        gain: 0.5,
        pitch: 2.0,
        fade: 0.0,
    });
    mixer.apply(Action::Loop {
        clip: Clip::new(ramp, 1000).unwrap(),
        channel: 3,
        volume: 1.0,
        fade: 0.0,
        pitch: 1.0,
    });
    let mut block = [[0.0; 2]; 200];
    mixer.render(&mut block);
    // Twice the rate: 200 frames in, it is 0.4 s into the clip, at half gain.
    let expected = 0.398 * 0.5 * 0.7;
    assert!(
        (block[199][0] - expected).abs() < 0.002,
        "{}",
        block[199][0]
    );
    mixer.apply(Action::ResetTunes);
    let before = mixer.frame()[0];
    let after = mixer.frame()[0];
    assert!(
        ((after - before) - 0.001 * 0.7).abs() < 1e-4,
        "rate 1 again"
    );
}

#[test]
fn bad_loops_follows_and_variation_are_findings() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("manifest.toml"),
        "id='bad'\nversion='0.1.0'\nmin_engine_version='0.4.0'\nevents=['on']\n",
    )
    .unwrap();
    std::fs::copy(
        engine().join("tests/play/loops/hum/sounds/hum.wav"),
        dir.path().join("hum.wav"),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("audio.toml"),
        "[sounds.hum]\nfile='hum.wav'\n\
         [[events]]\nevent='on'\nsound='hum'\nchannel='a'\n\
         [[events]]\nevent='on'\nsound='hum'\nbus='music'\nchannel='b'\n\
         [[events]]\nevent='on'\nsound='hum'\nvary_pitch=0.9\n\
         [[follow]]\nchannel='nope'\nentity='x'\ncomponent='engine.motion'\nfield='vx'\nfrom=0\nto=1\ngain=[0,1]\n\
         [[follow]]\nchannel='a'\nentity='x'\ncomponent='engine.motion'\nfield='vx'\ndistance_to='y'\nfrom=0\nto=1\ngain=[0,1]\n\
         [[follow]]\nambience=true\nentity='x'\ndistance_to='y'\nfrom=1\nto=1\npitch=[1,1]\n\
         [[follow]]\nchannel='a'\nentity='x'\ndistance_to='y'\nfrom=0\nto=1\n\
         [[follow]]\nchannel='a'\nentity='x'\ndistance_to='y'\nfrom=0\nto=1\npitch=[0.1,1]\n\
         [[follow]]\nchannel='a'\nentity='x'\ncomponent='engine.motion'\nfield=['vx_typo','vy']\nfrom=0\nto=1\ngain=[0,1]\n\
         [[follow]]\nchannel='a'\nentity='x'\ncomponent='engine.motoin'\nfield='vx'\nfrom=0\nto=1\ngain=[0,1]\n\
         [[follow]]\nchannel='a'\nentity='x'\ncomponent='engine.being'\nfield='player'\nfrom=0\nto=1\ngain=[0,1]\n",
    )
    .unwrap();
    let bank = Bank::load([&Module::read(dir.path()).unwrap()]);
    let p = &bank.problems;
    assert_eq!(p.len(), 10, "{p:#?}");
    for needle in [
        "loops on the sound bus",
        "vary_pitch",
        "channel `nope` is started by no",
        "component and field, or distance_to",
        "from and to must be finite and differ",
        "give gain, pitch or both",
        "pitch values",
        "`engine.motion` declares no field `vx_typo`",
        "component `engine.motoin` is not declared",
        "`engine.being.player` is not a number",
    ] {
        assert!(p.iter().any(|m| m.contains(needle)), "{needle}: {p:#?}");
    }
}

#[test]
fn a_one_field_list_follows_its_length_and_a_plain_field_its_signed_value() {
    // Moving at -14 along x: a list reads the vector's length (14, top pitch), a plain
    // field the signed value (-14, below `from`, so the start pitch).
    for (field, want) in [("['vx']", 1.6), ("['vx', 'vy']", 1.6), ("'vx'", 0.8)] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("manifest.toml"),
            "id='one'\nversion='0.1.0'\nmin_engine_version='0.4.0'\nwrites=['engine.position','engine.motion']\n",
        )
        .unwrap();
        std::fs::copy(
            engine().join("tests/play/loops/hum/sounds/hum.wav"),
            dir.path().join("hum.wav"),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("audio.toml"),
            format!(
                "[sounds.hum]\nfile='hum.wav'\n\
                 [[events]]\nevent='engine.start'\nsound='hum'\nchannel='engine'\n\
                 [[follow]]\nchannel='engine'\nentity='ship'\ncomponent='engine.motion'\n\
                 field={field}\nfrom=0\nto=14\npitch=[0.8,1.6]\n"
            ),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("start.toml"),
            "[entities.ship]\n\"engine.position\" = { x = 0.0, y = 0.0 }\n\
             \"engine.motion\" = { vx = -14.0, vy = 0.0 }\n",
        )
        .unwrap();
        let module = Module::read(dir.path()).unwrap();
        let bank = Bank::load([&module]);
        assert!(bank.problems.is_empty(), "{:?}", bank.problems);
        let session = Session::new(vec![module], 7).unwrap();
        let mut scene = Scene::new(bank);
        let pitch = scene
            .update(&session.world, Vec::<&str>::new())
            .into_iter()
            .find_map(|a| match a {
                Action::Tune {
                    channel: Some(_),
                    pitch,
                    ..
                } => Some(pitch),
                _ => None,
            })
            .unwrap();
        assert!(
            (pitch - want).abs() < 1e-4,
            "field = {field}: pitch {pitch}"
        );
    }
}
