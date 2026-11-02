use crate::{Action, Clip, Mixer};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

/// Optional live output. Missing or unusable devices are a note, never a game failure.
/// Commands are bounded; neither the simulation nor the audio callback waits on a lock.
pub struct Output {
    // Drop the stream and queued actions before releasing our PCM ownership.
    _stream: Option<cpal::Stream>,
    tx: Option<SyncSender<Vec<Action>>>,
    retained: Vec<Clip>,
    warned: bool,
}

impl Output {
    pub fn open() -> Self {
        Self::open_with(open)
    }

    fn open_with(open: impl FnOnce() -> Result<Opened, String>) -> Self {
        match open() {
            Ok((stream, tx)) => {
                eprintln!("galaxy: audio output ready");
                Self {
                    _stream: Some(stream),
                    tx: Some(tx),
                    retained: Vec::new(),
                    warned: false,
                }
            }
            Err(e) => {
                eprintln!("galaxy: audio unavailable; continuing silently ({e})");
                Self {
                    _stream: None,
                    tx: None,
                    retained: Vec::new(),
                    warned: false,
                }
            }
        }
    }

    pub fn send(&mut self, actions: Vec<Action>) {
        // Only this producer releases the last PCM reference. Count one means
        // neither the queue, a voice nor a bank can still clone this buffer.
        // Run even on empty ticks so finished hot-load buffers are reclaimed.
        self.retained
            .retain(|clip| Arc::strong_count(&clip.frames) > 1);
        if actions.is_empty() {
            return;
        }
        if let Some(tx) = &self.tx {
            for action in &actions {
                if let Action::Play { clip, .. } = action
                    && !self
                        .retained
                        .iter()
                        .any(|held| Arc::ptr_eq(&held.frames, &clip.frames))
                {
                    self.retained.push(clip.clone());
                }
            }
            match tx.try_send(actions) {
                Ok(()) => (),
                Err(std::sync::mpsc::TrySendError::Full(_)) => {
                    if !self.warned {
                        eprintln!("galaxy: audio command queue full; skipping cues");
                        self.warned = true;
                    }
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    eprintln!("galaxy: audio output stopped; continuing silently");
                    self.tx = None;
                }
            }
        }
    }
}

type Opened = (cpal::Stream, SyncSender<Vec<Action>>);
fn open() -> Result<Opened, String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no audio output device")?;
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    let format = config.sample_format();
    let config: cpal::StreamConfig = config.into();
    if config.channels == 0 || config.sample_rate.0 == 0 {
        return Err("invalid output configuration".into());
    }
    let (tx, rx) = sync_channel(64);
    let stream = match format {
        cpal::SampleFormat::F32 => stream::<f32>(&device, &config, rx),
        cpal::SampleFormat::F64 => stream::<f64>(&device, &config, rx),
        cpal::SampleFormat::I8 => stream::<i8>(&device, &config, rx),
        cpal::SampleFormat::I16 => stream::<i16>(&device, &config, rx),
        cpal::SampleFormat::I32 => stream::<i32>(&device, &config, rx),
        cpal::SampleFormat::I64 => stream::<i64>(&device, &config, rx),
        cpal::SampleFormat::U8 => stream::<u8>(&device, &config, rx),
        cpal::SampleFormat::U16 => stream::<u16>(&device, &config, rx),
        cpal::SampleFormat::U32 => stream::<u32>(&device, &config, rx),
        cpal::SampleFormat::U64 => stream::<u64>(&device, &config, rx),
        _ => return Err(format!("unsupported output sample format {format}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, tx))
}

fn stream<T: cpal::SizedSample + cpal::FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    rx: Receiver<Vec<Action>>,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    let mut mixer = Mixer::new(config.sample_rate.0);
    let channels = usize::from(config.channels);
    device.build_output_stream(
        config,
        move |data: &mut [T], _| {
            // A bounded pass even if the producer is unusually busy.
            for _ in 0..64 {
                let Ok(actions) = rx.try_recv() else { break };
                for action in actions {
                    mixer.apply(action);
                }
            }
            for frame in data.chunks_mut(channels) {
                let stereo = mixer.frame();
                for (c, sample) in frame.iter_mut().enumerate() {
                    let value = if channels == 1 {
                        (stereo[0] + stereo[1]) * 0.5
                    } else {
                        stereo.get(c).copied().unwrap_or(0.0)
                    };
                    *sample = T::from_sample(value);
                }
            }
        },
        |e| eprintln!("galaxy: audio output error; continuing silently ({e})"),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finished_pcm_is_reclaimed_by_the_producer_after_hot_load() {
        // A channel stands in for CPAL; no device is opened. Dropping the source
        // clip models replacing the scene/bank during a hot-load.
        let (tx, rx) = sync_channel(64);
        let mut output = Output {
            _stream: None,
            tx: Some(tx),
            retained: Vec::new(),
            warned: false,
        };
        let mut mixer = Mixer::new(48_000);
        for bus in [crate::Bus::Music, crate::Bus::Sound] {
            let clip = crate::Clip::new(vec![[0.25; 2]; 480_000], 48_000).unwrap();
            let pcm = std::sync::Arc::downgrade(&clip.frames);
            output.send(vec![Action::Play {
                clip,
                bus,
                volume: 1.0,
                fade: 0.0,
            }]);
            for action in rx.try_recv().unwrap() {
                mixer.apply(action);
            }
            // Collect while the outgoing voice still owns the old bank's PCM.
            output.send(vec![]);
            if bus == crate::Bus::Music {
                mixer.apply(Action::Stop { bus, fade: 0.01 });
            }
            for _ in 0..480_000 {
                mixer.frame();
            }
            assert_eq!(mixer.voice_count(), 0);
            assert!(
                pcm.strong_count() > 0,
                "the callback freed the sample buffer when its voice ended"
            );
            output.send(vec![]);
            assert_eq!(
                pcm.strong_count(),
                0,
                "the producer must reclaim unused PCM"
            );
        }
    }

    #[test]
    fn queued_and_stolen_voices_keep_pcm_owned_by_the_producer() {
        let (tx, rx) = sync_channel(1);
        let mut output = Output {
            _stream: None,
            tx: Some(tx),
            retained: Vec::new(),
            warned: false,
        };
        let clip = Clip::new(vec![[0.25; 2]; 480_000], 48_000).unwrap();
        let pcm = Arc::downgrade(&clip.frames);
        let play = Action::Play {
            clip,
            bus: crate::Bus::Sound,
            volume: 1.0,
            fade: 0.0,
        };
        output.send(vec![play.clone(); crate::MAX_VOICES + 1]);
        // Saturating the queue and reusing the same clip must not add owners
        // indefinitely or lose the pin protecting queued commands.
        output.send(vec![play]);
        assert!(output.warned);
        assert_eq!(output.retained.len(), 1);
        output.send(vec![]);
        let mut mixer = Mixer::new(48_000);
        for action in rx.try_recv().unwrap() {
            mixer.apply(action);
        }
        assert_eq!(mixer.stolen_voices, 1);
        let replacement = Action::Play {
            clip: Clip::new(vec![[0.0; 2]; 1], 48_000).unwrap(),
            bus: crate::Bus::Music,
            volume: 1.0,
            fade: 0.0,
        };
        for _ in 0..crate::MAX_VOICES {
            mixer.apply(replacement.clone());
        }
        assert_eq!(pcm.strong_count(), 1, "only the producer owns retired PCM");
        output.send(vec![]);
        assert_eq!(pcm.strong_count(), 0);
    }
    #[test]
    #[ignore = "opens a real output device when present; run explicitly to probe silent fallback"]
    fn output_device_smoke() {
        let mut output = Output::open();
        output.send(vec![Action::Stop {
            bus: crate::Bus::Music,
            fade: 0.01,
        }]);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    #[test]
    fn no_device_is_silent_and_commands_are_harmless() {
        let mut output = Output::open_with(|| Err("no audio output device".into()));
        assert!(output._stream.is_none());
        output.send(vec![Action::Stop {
            bus: crate::Bus::Music,
            fade: 1.0,
        }]);
        assert!(output.tx.is_none());
    }
}
