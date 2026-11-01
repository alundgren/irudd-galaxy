use galaxy_audio::{Action, Bus, Clip, MAX_VOICES, Mixer};

fn play(clip: Clip, bus: Bus, fade: f32) -> Action {
    Action::Play {
        clip,
        bus,
        volume: 1.0,
        fade,
    }
}
fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 0.00001, "{a} != {b}");
}

#[test]
fn crossfade_is_sample_accurate_and_volume_ramps_reach_the_target() {
    let mut mixer = Mixer::new(1000);
    mixer.apply(play(
        Clip::new(vec![[0.5, 0.0]], 1000).unwrap(),
        Bus::Ambience,
        0.1,
    ));
    let mut initial = [[0.0; 2]; 100];
    mixer.render(&mut initial);
    near(initial[99][0], 0.35);
    mixer.apply(play(
        Clip::new(vec![[0.0, 0.5]], 1000).unwrap(),
        Bus::Ambience,
        0.1,
    ));
    let mut transition = [[0.0; 2]; 100];
    mixer.render(&mut transition);
    near(transition[49][0], 0.175);
    near(transition[49][1], 0.175);
    for frame in transition {
        near(frame[0] + frame[1], 0.35);
    }
    assert_eq!(mixer.voice_count(), 1);
    mixer.apply(Action::Volume {
        bus: Some(Bus::Ambience),
        volume: 0.5,
        fade: 0.1,
    });
    mixer.render(&mut initial);
    near(initial[99][1], 0.175);
    mixer.apply(Action::Stop {
        bus: Bus::Ambience,
        fade: 0.1,
    });
    mixer.render(&mut initial);
    near(initial[99][1], 0.0);
    assert_eq!(mixer.voice_count(), 0);
}

#[test]
fn rate_conversion_interpolates_and_wraps_stereo_without_channel_swapping() {
    let mut mixer = Mixer::new(16000);
    mixer.apply(Action::Volume {
        bus: None,
        volume: 1.0,
        fade: 0.0,
    });
    mixer.apply(play(
        Clip::new(vec![[0.0, 0.0], [0.5, -0.5], [0.0, 0.0], [-0.5, 0.5]], 8000).unwrap(),
        Bus::Music,
        0.005,
    ));
    mixer.render(&mut [[0.0; 2]; 80]);
    for expected in [0.0, 0.25, 0.5, 0.25, 0.0, -0.25, -0.5, -0.25] {
        let frame = mixer.frame();
        near(frame[0], expected);
        near(frame[1], -expected);
    }
}

#[test]
fn effects_end_and_overload_is_bounded_without_stealing_music() {
    let clip = Clip::new(vec![[0.1; 2]; 100], 1000).unwrap();
    let mut mixer = Mixer::new(1000);
    mixer.apply(play(clip.clone(), Bus::Music, 0.01));
    for _ in 0..MAX_VOICES + 5 {
        mixer.apply(play(clip.clone(), Bus::Sound, 0.01));
    }
    assert_eq!(mixer.voice_count(), MAX_VOICES);
    assert_eq!(mixer.stolen_voices, 6);
    mixer.render(&mut [[0.0; 2]; 120]);
    assert_eq!(mixer.voice_count(), 1);
    near(mixer.frame()[0], 0.07);
}

#[test]
fn safety_clamp_and_nonfinite_inputs_never_reach_the_output() {
    let clip = Clip::new(vec![[1.0; 2]; 100], 1000).unwrap();
    let mut mixer = Mixer::new(1000);
    for _ in 0..4 {
        mixer.apply(play(clip.clone(), Bus::Sound, 0.005));
    }
    let mut frames = [[0.0; 2]; 50];
    mixer.render(&mut frames);
    assert!(
        frames
            .iter()
            .flatten()
            .all(|s| s.is_finite() && s.abs() <= 1.0)
    );
    assert!(mixer.limited_samples > 0);
    mixer.apply(Action::Volume {
        bus: None,
        volume: f32::NAN,
        fade: 0.0,
    });
    assert_eq!(mixer.frame(), [0.0; 2]);
    assert!(Clip::new(vec![[f32::NAN; 2]], 1000).is_err());
    assert!(Clip::new(vec![], 1000).is_err());
}
