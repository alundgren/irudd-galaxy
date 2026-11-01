use std::path::Path;
use std::sync::Arc;

/// Decoded PCM, stereo frames in [-1, 1]. Mono is copied to both channels.
#[derive(Clone, Debug)]
pub struct Clip {
    pub(crate) frames: Arc<[[f32; 2]]>,
    pub(crate) rate: u32,
}

impl Clip {
    pub fn new(frames: Vec<[f32; 2]>, rate: u32) -> Result<Self, String> {
        if rate == 0 || frames.is_empty() {
            return Err("audio must contain frames at a nonzero sample rate".into());
        }
        if frames
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1.0)
        {
            return Err("audio samples must be finite and in [-1, 1]".into());
        }
        Ok(Self {
            frames: frames.into(),
            rate,
        })
    }

    pub fn frames(&self) -> usize {
        self.frames.len()
    }
    pub fn sample_rate(&self) -> u32 {
        self.rate
    }

    /// Decode WAV PCM/float or Ogg Vorbis before playback, never in the callback.
    pub fn read(path: &Path) -> Result<Self, String> {
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        let (samples, channels, rate) = match ext {
            "wav" => {
                let mut reader = hound::WavReader::open(path).map_err(|e| e.to_string())?;
                let spec = reader.spec();
                check_format(spec.channels, spec.sample_rate)?;
                let samples = match spec.sample_format {
                    hound::SampleFormat::Float => reader
                        .samples::<f32>()
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|e| e.to_string())?,
                    hound::SampleFormat::Int => {
                        if !(1..=32).contains(&spec.bits_per_sample) {
                            return Err("unsupported WAV bit depth".into());
                        }
                        let scale = 2_f32.powi(i32::from(spec.bits_per_sample) - 1);
                        reader
                            .samples::<i32>()
                            .map(|s| s.map(|s| s as f32 / scale))
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(|e| e.to_string())?
                    }
                };
                (samples, spec.channels, spec.sample_rate)
            }
            "ogg" => {
                let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
                let mut reader =
                    lewton::inside_ogg::OggStreamReader::new(file).map_err(|e| e.to_string())?;
                let channels = u16::from(reader.ident_hdr.audio_channels);
                let rate = reader.ident_hdr.audio_sample_rate;
                let serial = reader.stream_serial();
                check_format(channels, rate)?;
                let mut samples = Vec::new();
                while let Some(packet) = reader.read_dec_packet_itl().map_err(|e| e.to_string())? {
                    if reader.stream_serial() != serial
                        || u16::from(reader.ident_hdr.audio_channels) != channels
                        || reader.ident_hdr.audio_sample_rate != rate
                    {
                        return Err(
                            "chained Ogg streams are not supported; use one stream per sound"
                                .into(),
                        );
                    }
                    samples.extend(packet.into_iter().map(|s| f32::from(s) / 32768.0));
                }
                // Lewton cannot trim its final packet when every audio packet fits on
                // one page (no earlier granule position). Honor the final PCM length
                // here too, otherwise tiny authored loops acquire a padded tail.
                if let Some(frames) = reader.get_last_absgp() {
                    let length = frames.saturating_mul(u64::from(channels));
                    if length < samples.len() as u64 {
                        samples.truncate(length as usize);
                    }
                }
                (samples, channels, rate)
            }
            _ => return Err("sound files must be .wav or .ogg (Vorbis)".into()),
        };
        if samples.len() % usize::from(channels) != 0 {
            return Err("incomplete audio frame".into());
        }
        let frames = samples
            .chunks_exact(usize::from(channels))
            .map(|s| [s[0], *s.get(1).unwrap_or(&s[0])])
            .collect();
        Self::new(frames, rate)
    }
}

fn check_format(channels: u16, rate: u32) -> Result<(), String> {
    if !(1..=2).contains(&channels) || rate == 0 {
        Err("sound files must be mono or stereo with a nonzero sample rate".into())
    } else {
        Ok(())
    }
}
