//! Microphone capture.
//!
//! §4 -- the hotkey starts recording immediately and the panel appears second,
//! so this must be startable without anything on screen having happened yet.
//!
//! The cpal stream runs on its own thread and hands samples to a callback, so
//! the buffer is shared and the lock is held for as short a time as possible:
//! blocking in an audio callback drops samples, and dropped samples are words
//! the transcript will not contain.

use crate::error::{Error, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

pub const SAMPLE_RATE: u32 = 16_000;

/// A recording in progress. Dropping it stops the stream.
pub struct Recording {
    stream: cpal::Stream,
    buffer: Arc<Mutex<Vec<f32>>>,
    started: std::time::Instant,
}

/// cpal's `Stream` is not `Send` on every host, but the recorder only ever
/// touches it from the command thread that made it.
unsafe impl Send for Recording {}

impl Recording {
    /// Everything captured so far. Cheap enough to poll for a level meter.
    pub fn samples(&self) -> Vec<f32> {
        self.buffer
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn elapsed_ms(&self) -> i64 {
        self.started.elapsed().as_millis() as i64
    }

    /// Peak of the most recent window, for the equalizer bars. Peak rather
    /// than mean for the same reason the fingerprint uses it: speech is gaps.
    pub fn level(&self) -> f32 {
        let buf = self.buffer.lock().unwrap_or_else(|p| p.into_inner());
        let window = buf.len().saturating_sub(1600);
        buf[window..].iter().fold(0.0_f32, |m, s| m.max(s.abs()))
    }

    pub fn stop(self) -> Vec<f32> {
        drop(self.stream);
        Arc::try_unwrap(self.buffer)
            .map(|m| m.into_inner().unwrap_or_else(|p| p.into_inner()))
            .unwrap_or_else(|arc| arc.lock().unwrap_or_else(|p| p.into_inner()).clone())
    }
}

/// Downmix to mono and decimate to 16 kHz, emitting one sample every `ratio`
/// input frames. `carry` persists across callbacks so the phase does not reset
/// on every buffer, which would drift the length of a long recording.
///
/// Nearest-neighbour, which is crude and correct enough at speech bandwidth.
/// A device already at 16 kHz has ratio 1 and keeps every frame.
fn decimate(data: &[f32], channels: usize, ratio: f64, carry: &mut f64) -> Vec<f32> {
    let channels = channels.max(1);
    let frames = data.len() / channels;
    let mut out = Vec::with_capacity((frames as f64 / ratio).ceil() as usize + 1);

    for frame in 0..frames {
        *carry += 1.0;
        if *carry < ratio {
            continue;
        }
        *carry -= ratio;
        let start = frame * channels;
        let mixed: f32 = data[start..start + channels].iter().sum::<f32>() / channels as f32;
        out.push(mixed);
    }
    out
}

/// Opens the default input at 16 kHz mono where the device allows it.
///
/// Downmixing is done here rather than later because transcribe.cpp wants mono
/// and a stereo buffer would otherwise be transcribed at double speed.
pub fn start() -> Result<Recording> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| Error::Other("no microphone available".into()))?;

    let default = device
        .default_input_config()
        .map_err(|e| Error::Other(format!("no input config: {e}")))?;
    let channels = default.channels() as usize;
    let device_rate = default.sample_rate();

    let buffer = Arc::new(Mutex::new(Vec::<f32>::new()));
    let sink = Arc::clone(&buffer);

    // Below the rate we need there is nothing to decimate, and clamping the
    // ratio would keep every frame and transcribe the result as though it were
    // 16kHz -- the wrong tempo, and confidently wrong words rather than an
    // obvious failure.
    if device_rate < SAMPLE_RATE {
        return Err(Error::Other(format!(
            "the microphone runs at {device_rate}Hz; transcription needs at least {SAMPLE_RATE}Hz"
        )));
    }
    let ratio = device_rate as f64 / SAMPLE_RATE as f64;
    let mut carry = 0.0_f64;

    let stream = device
        .build_input_stream(
            &default.config(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let out = decimate(data, channels, ratio, &mut carry);
                if let Ok(mut sink) = sink.lock() {
                    sink.extend_from_slice(&out);
                }
            },
            move |err| eprintln!("audio input error: {err}"),
            None,
        )
        .map_err(|e| Error::Other(format!("could not open the microphone: {e}")))?;

    stream
        .play()
        .map_err(|e| Error::Other(format!("could not start recording: {e}")))?;

    Ok(Recording {
        stream,
        buffer,
        started: std::time::Instant::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 48 kHz stereo is the common default. Getting this wrong makes every
    /// recording three times too long and transcribes to nonsense.
    #[test]
    fn forty_eight_kilohertz_stereo_becomes_one_third_as_many_mono_samples() {
        let frames = 4800;
        let data: Vec<f32> = (0..frames * 2).map(|i| i as f32).collect();
        let mut carry = 0.0;

        let out = decimate(&data, 2, 3.0, &mut carry);

        assert!(
            (out.len() as i64 - 1600).abs() <= 1,
            "expected ~1600 samples, got {}",
            out.len()
        );
    }

    /// A device already at the rate we want must lose nothing.
    #[test]
    fn a_device_already_at_sixteen_kilohertz_keeps_every_frame() {
        let data = vec![0.5_f32; 1000];
        let mut carry = 0.0;
        assert_eq!(decimate(&data, 1, 1.0, &mut carry).len(), 1000);
    }

    /// Phase carries across callbacks; resetting it each buffer would drift
    /// the length of anything longer than one callback.
    #[test]
    fn phase_carries_between_callbacks() {
        let data: Vec<f32> = vec![1.0; 100];
        let mut carried = 0.0;
        let across: usize = (0..10)
            .map(|_| decimate(&data, 1, 3.0, &mut carried).len())
            .sum();

        let mut fresh_total = 0;
        for _ in 0..10 {
            let mut reset = 0.0;
            fresh_total += decimate(&data, 1, 3.0, &mut reset).len();
        }

        assert_eq!(across, 333, "1000 frames at 3:1 is 333 samples");
        assert_ne!(across, fresh_total, "resetting per callback drifts");
    }

    #[test]
    fn stereo_is_averaged_not_interleaved() {
        let mut carry = 0.0;
        let out = decimate(&[1.0, 0.0, 1.0, 0.0], 2, 1.0, &mut carry);
        assert_eq!(out, vec![0.5, 0.5]);
    }
}
