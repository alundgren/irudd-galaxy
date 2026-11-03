use std::path::Path;
use std::sync::Arc;

/// Decoded PCM, stereo frames in [-1, 1]. Mono is copied to both channels.
#[derive(Clone, Debug)]
pub struct Clip {
    pub(crate) frames: Arc<[[f32; 2]]>,
    pub(crate) rate: u32,
    /// Source frames a loop crossfades from its end into its start; 0 is a hard wrap.
    pub(crate) seam: usize,
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
            seam: 0,
        })
    }

    /// When looped, crossfade the last `seconds` into the start instead of jumping back,
    /// which makes the loop that much shorter. A clip shorter than four fades wraps as
    /// authored: short loops are cut to whole cycles, and an early restart would put
    /// them out of phase.
    pub fn with_loop_crossfade(mut self, seconds: f32) -> Self {
        let seconds = if seconds.is_finite() {
            seconds.max(0.0)
        } else {
            0.0
        };
        let frames = (f64::from(seconds) * f64::from(self.rate)).round() as usize;
        self.seam = if frames * 4 <= self.frames.len() {
            frames
        } else {
            0
        };
        self
    }
    pub fn loop_crossfade_frames(&self) -> usize {
        self.seam
    }
    /// Root mean square of every sample, both channels: 1 is a full-scale square wave.
    pub fn rms(&self) -> f32 {
        let sum: f64 = self
            .frames
            .iter()
            .flatten()
            .map(|&s| f64::from(s) * f64::from(s))
            .sum();
        (sum / (self.frames.len() * 2) as f64).sqrt() as f32
    }

    pub fn frames(&self) -> usize {
        self.frames.len()
    }
    pub fn sample_rate(&self) -> u32 {
        self.rate
    }

    /// Decode WAV PCM/float, Ogg Vorbis or MP3 before playback, never in the callback.
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
            "mp3" => mp3(path)?,
            _ => return Err("sound files must be .wav, .ogg (Vorbis) or .mp3".into()),
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

/// MP3 through symphonia's decoder. A LAME tag's encoder delay and padding are trimmed
/// (gapless), so a tagged loop keeps its authored length. Lossy decoding can overshoot
/// full scale by a little, so samples are clamped to [-1, 1] rather than rejected. A
/// frame that fails to decode is skipped, as players do; a file with none is an error.
fn mp3(path: &Path) -> Result<(Vec<f32>, u16, u32), String> {
    use symphonia::core::codecs::CodecParameters;
    use symphonia::core::errors::Error;
    use symphonia::core::formats::TrackType;
    use symphonia::core::io::MediaSourceStream;

    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = symphonia::core::formats::probe::Hint::new();
    hint.with_extension("mp3");
    let mut reader = symphonia::default::get_probe()
        .probe(&hint, stream, Default::default(), Default::default())
        .map_err(|e| format!("not an MP3 file: {e}"))?;
    let track = reader
        .default_track(TrackType::Audio)
        .ok_or("the MP3 file has no audio track")?;
    let id = track.id;
    let Some(CodecParameters::Audio(params)) = track.codec_params.clone() else {
        return Err("the MP3 file has no audio track".into());
    };
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &Default::default())
        .map_err(|e| e.to_string())?;
    let (mut samples, mut packet_samples) = (Vec::new(), Vec::<f32>::new());
    let mut format: Option<(u16, u32)> = None;
    loop {
        let packet = match reader.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.to_string()),
        };
        if packet.track_id != id {
            continue;
        }
        let buffer = match decoder.decode(&packet) {
            Ok(buffer) => buffer,
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(e.to_string()),
        };
        let spec = buffer.spec();
        let this = (
            u16::try_from(spec.channels().count()).unwrap_or(u16::MAX),
            spec.rate(),
        );
        check_format(this.0, this.1)?;
        if format.is_some_and(|f| f != this) {
            return Err("MP3 frames change channels or sample rate; use one format".into());
        }
        format = Some(this);
        buffer.copy_to_vec_interleaved(&mut packet_samples);
        samples.extend(packet_samples.iter().map(|s| s.clamp(-1.0, 1.0)));
    }
    let (channels, rate) = format.ok_or("no MP3 frame decoded")?;
    Ok((samples, channels, rate))
}

fn check_format(channels: u16, rate: u32) -> Result<(), String> {
    if !(1..=2).contains(&channels) || rate == 0 {
        Err("sound files must be mono or stereo with a nonzero sample rate".into())
    } else {
        Ok(())
    }
}
