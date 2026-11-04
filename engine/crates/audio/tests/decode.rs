use galaxy_audio::{Action, Bus, Clip, Mixer};
use std::path::Path;

#[test]
fn authoring_pcm_retains_overshoots_and_source_channel_count() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("over.wav");
    let mut writer = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 1,
            sample_rate: 48000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    writer.write_sample(1.2_f32).unwrap();
    writer.write_sample(-1.1_f32).unwrap();
    writer.finalize().unwrap();
    let pcm = galaxy_audio::Pcm::read(&path).unwrap();
    assert_eq!(pcm.channels, 1);
    assert_eq!(pcm.frames[0], [1.2; 2]);
    assert!(
        Clip::read(&path).is_err(),
        "playback still refuses over-range float WAV"
    );
}

#[test]
fn ogg_vorbis_decodes_and_plays_finite_nonzero_samples() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/audio/demo/sounds/high.ogg");
    let clip = Clip::read(&path).unwrap();
    assert_eq!(clip.sample_rate(), 16000);
    assert_eq!(
        clip.frames(),
        1600,
        "do not keep codec padding at a loop seam"
    );
    let mut mixer = Mixer::new(48000);
    mixer.apply(Action::Play {
        clip,
        bus: Bus::Sound,
        volume: 1.0,
        fade: 0.005,
        pitch: 1.0,
    });
    let mut frames = [[0.0; 2]; 4800];
    mixer.render(&mut frames);
    assert!(frames.iter().flatten().all(|s| s.is_finite()));
    let peak = frames.iter().flatten().fold(0.0_f32, |a, b| a.max(b.abs()));
    assert!(peak > 0.1 && peak < 0.25, "{peak}");
}

#[test]
fn wav_float_stereo_and_integer_depths_decode() {
    let tmp = tempfile::tempdir().unwrap();
    for bits in [8, 16, 24, 32] {
        let path = tmp.path().join(format!("pcm{bits}.wav"));
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 22050,
            bits_per_sample: bits,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        writer.write_sample(0_i32).unwrap();
        writer.write_sample(1_i32 << (bits - 2)).unwrap();
        writer.finalize().unwrap();
        let clip = Clip::read(&path).unwrap();
        assert_eq!(clip.frames(), 2);
        assert_eq!(clip.sample_rate(), 22050);
    }
    let path = tmp.path().join("float.wav");
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 48000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(&path, spec).unwrap();
    writer.write_sample(0.25_f32).unwrap();
    writer.write_sample(-0.25_f32).unwrap();
    writer.finalize().unwrap();
    assert_eq!(Clip::read(&path).unwrap().frames(), 1);
}

#[test]
fn mp3_decodes_to_the_tone_it_was_made_from() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/audio/demo/sounds");
    let clip = Clip::read(&dir.join("high.mp3")).unwrap();
    assert_eq!(clip.sample_rate(), 16000);
    // Untagged MP3 keeps the encoder's delay and final-frame padding: at most a few
    // frames of 576 samples beyond the 1600 authored.
    assert!(
        (1600..=1600 + 4 * 576).contains(&clip.frames()),
        "{}",
        clip.frames()
    );
    let mut mixer = Mixer::new(16000);
    mixer.apply(Action::Play {
        clip,
        bus: Bus::Sound,
        volume: 1.0,
        fade: 0.005,
        pitch: 1.0,
    });
    let mut frames = [[0.0; 2]; 1600];
    mixer.render(&mut frames);
    assert!(frames.iter().flatten().all(|s| s.is_finite()));
    let peak = frames.iter().flatten().fold(0.0_f32, |a, b| a.max(b.abs()));
    assert!(peak > 0.1 && peak < 0.3, "{peak}");
}

#[test]
fn a_broken_mp3_is_an_error_not_silence() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("broken.mp3");
    std::fs::write(&path, b"ID3 this is not audio").unwrap();
    assert!(Clip::read(&path).is_err());
}
