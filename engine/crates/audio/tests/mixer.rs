use galaxy_audio::{Action, Bus, Clip, MAX_VOICES, Mixer};

fn play(clip: Clip, bus: Bus, fade: f32) -> Action {
    Action::Play {
        clip,
        bus,
        volume: 1.0,
        fade,
        pitch: 1.0,
    }
}
fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 0.00001, "{a} != {b}");
}

#[test]
fn stems_and_pre_clamp_peaks_are_from_the_same_playback_frame() {
    let mut mixer = Mixer::new(1000);
    for bus in [Bus::Sound, Bus::Music, Bus::Ambience] {
        mixer.apply(play(
            Clip::new(vec![[0.9; 2]; 1000], 1000).unwrap(),
            bus,
            0.01,
        ));
    }
    for _ in 0..20 {
        mixer.frame();
    }
    let frame = mixer.frame_with_buses();
    for c in 0..2 {
        near(frame.pre_clamp[c], 1.89);
        near(frame.output[c], 1.0);
        near(frame.buses.iter().map(|b| b[c]).sum(), frame.pre_clamp[c]);
    }
    assert!(mixer.limited_samples > 0);
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

#[test]
fn a_loop_crossfades_its_end_into_its_start_without_a_jump() {
    // A ramp from -0.5 to 0.5: a hard wrap jumps by a whole 1.0 at the seam.
    let count = 1000;
    let frames: Vec<[f32; 2]> = (0..count)
        .map(|i| [i as f32 / count as f32 - 0.5; 2])
        .collect();
    let step = 1.0 / count as f32;
    let largest_step = |clip: Clip| {
        let mut mixer = Mixer::new(1000);
        mixer.apply(Action::Volume {
            bus: None,
            volume: 1.0,
            fade: 0.0,
        });
        mixer.apply(play(clip, Bus::Music, 0.0));
        let mut out = vec![[0.0; 2]; 3500];
        mixer.render(&mut out);
        // Skip the 5 ms fade-in; three loops follow.
        out[10..]
            .windows(2)
            .map(|w| (w[1][0] - w[0][0]).abs())
            .fold(0.0, f32::max)
    };
    let clip = Clip::new(frames, 1000).unwrap();
    assert!(largest_step(clip.clone()) > 0.9, "a hard wrap jumps");
    let smooth = clip.with_loop_crossfade(0.1);
    assert_eq!(smooth.loop_crossfade_frames(), 100);
    // Over the 100-frame fade the ramp travels 0.9 down plus its own rise: about 0.01
    // a frame, never a jump.
    let worst = largest_step(smooth);
    assert!(worst < 10.0 * step + 0.0001, "seam step {worst}");
}

#[test]
fn a_clip_shorter_than_four_fades_loops_as_authored() {
    // 25 whole cycles: a clean loop. Restarting it early would put it out of phase.
    let frames: Vec<[f32; 2]> = (0..1000)
        .map(|i| {
            let v = (f64::from(i) * std::f64::consts::TAU / 40.0).sin() as f32 * 0.5;
            [v, -v]
        })
        .collect();
    let render = |clip: Clip| {
        let mut mixer = Mixer::new(1000);
        mixer.apply(play(clip, Bus::Ambience, 0.0));
        let mut out = vec![[0.0; 2]; 3000];
        mixer.render(&mut out);
        out
    };
    let clip = Clip::new(frames, 1000).unwrap();
    assert_eq!(
        clip.clone()
            .with_loop_crossfade(0.25)
            .loop_crossfade_frames(),
        250
    );
    let short = clip.clone().with_loop_crossfade(0.3);
    assert_eq!(short.loop_crossfade_frames(), 0);
    assert_eq!(render(clip), render(short));
}
