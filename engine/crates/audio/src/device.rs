use crate::{Action, Mixer};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

/// Optional live output. Missing or unusable devices are a note, never a game failure.
/// Commands are bounded; neither the simulation nor the audio callback waits on a lock.
pub struct Output {
    _stream: Option<cpal::Stream>,
    tx: Option<SyncSender<Vec<Action>>>,
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
                    warned: false,
                }
            }
            Err(e) => {
                eprintln!("galaxy: audio unavailable; continuing silently ({e})");
                Self {
                    _stream: None,
                    tx: None,
                    warned: false,
                }
            }
        }
    }

    pub fn send(&mut self, actions: Vec<Action>) {
        if actions.is_empty() {
            return;
        }
        if let Some(tx) = &self.tx {
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
