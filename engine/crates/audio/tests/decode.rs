use galaxy_audio::{Action, Bus, Clip, Mixer};
use std::path::Path;

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
