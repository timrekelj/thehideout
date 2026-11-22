//! A microphone level meter that needs no room, for the settings "test your mic" bar.
//!
//! The platform audio device module hands its capture straight to WebRTC and offers no tap
//! (a sink on the local track gets no frames), so outside a room the level comes from a
//! plain `cpal` input stream instead. It only measures: nothing is sent anywhere.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::audio::{level_of, LevelCell};

/// Measures the input device until dropped. The `cpal` stream is not `Send`, so a thread
/// owns it.
pub(crate) struct MicMeter {
    stop: Arc<AtomicBool>,
}

impl Drop for MicMeter {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl MicMeter {
    /// `device_name` is the device's name as the platform lists it; `None` is the default
    /// input. Fails (with a message fit for the UI) when the device cannot be opened, which
    /// on macOS is usually missing microphone permission.
    pub(crate) fn start(
        device_name: Option<String>,
        cell: Arc<LevelCell>,
    ) -> Result<MicMeter, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("voice-mic-meter".into())
            .spawn(move || {
                let stream = match open(device_name, cell) {
                    Ok(s) => {
                        let _ = ready_tx.send(Ok(()));
                        s
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                while !flag.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(50));
                }
                drop(stream);
            })
            .map_err(|e| e.to_string())?;
        match ready_rx.recv_timeout(Duration::from_secs(15)) {
            Ok(Ok(())) => Ok(MicMeter { stop }),
            Ok(Err(e)) => Err(e),
            Err(_) => {
                stop.store(true, Ordering::Relaxed);
                Err("timed out opening the microphone (waiting for permission?)".into())
            }
        }
    }
}

fn open(device_name: Option<String>, cell: Arc<LevelCell>) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = match &device_name {
        None => host.default_input_device().ok_or("no input device")?,
        Some(name) => host
            .input_devices()
            .map_err(|e| e.to_string())?
            .find(|d| d.description().is_ok_and(|desc| desc.name() == name))
            .ok_or_else(|| format!("input device {name:?} not found"))?,
    };
    let supported = device.default_input_config().map_err(|e| e.to_string())?;
    let config = supported.config();
    let on_error = |e| eprintln!("thehideout-voice: mic meter: {e}");
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            config,
            move |data: &[f32], _| {
                let pcm: Vec<i16> = data
                    .iter()
                    .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                    .collect();
                cell.record(level_of(&pcm));
            },
            on_error,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            config,
            move |data: &[i16], _| cell.record(level_of(data)),
            on_error,
            None,
        ),
        other => return Err(format!("unsupported microphone sample format {other:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}
