use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub trait AudioCaptureProvider {
    fn capture_wav(&self, seconds: u64, output_path: &Path) -> anyhow::Result<()>;
    fn capture_until_pause(&self, max_seconds: u64, output_path: &Path) -> anyhow::Result<()>;
    fn supported(&self) -> bool;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemAudioCapture;

impl AudioCaptureProvider for SystemAudioCapture {
    fn capture_wav(&self, seconds: u64, output_path: &Path) -> anyhow::Result<()> {
        capture_wav(seconds, output_path)
    }

    fn supported(&self) -> bool {
        cpal::default_host().default_input_device().is_some()
    }

    fn capture_until_pause(&self, max_seconds: u64, output_path: &Path) -> anyhow::Result<()> {
        capture_wav_until_pause(max_seconds, output_path)
    }
}

fn capture_wav(seconds: u64, output_path: &Path) -> anyhow::Result<()> {
    capture_wav_inner(seconds, output_path, false)
}

fn capture_wav_until_pause(max_seconds: u64, output_path: &Path) -> anyhow::Result<()> {
    capture_wav_inner(max_seconds, output_path, true)
}

fn capture_wav_inner(seconds: u64, output_path: &Path, stop_on_pause: bool) -> anyhow::Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow::anyhow!("no default microphone input device is available"))?;
    let supported = device
        .default_input_config()
        .map_err(|error| anyhow::anyhow!("failed to read microphone configuration: {error}"))?;
    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let samples = Arc::new(Mutex::new(Vec::<i16>::new()));
    let error_slot = Arc::new(Mutex::new(None::<String>));

    let stream = match sample_format {
        SampleFormat::F32 => {
            let stream_samples = Arc::clone(&samples);
            let stream_error = Arc::clone(&error_slot);
            device.build_input_stream(
                config,
                move |data: &[f32], _| append_samples(&stream_samples, data.iter().map(f32_to_i16)),
                move |error: cpal::Error| record_stream_error(&stream_error, error),
                None,
            )
        }
        SampleFormat::I16 => {
            let stream_samples = Arc::clone(&samples);
            let stream_error = Arc::clone(&error_slot);
            device.build_input_stream(
                config,
                move |data: &[i16], _| append_samples(&stream_samples, data.iter().copied()),
                move |error: cpal::Error| record_stream_error(&stream_error, error),
                None,
            )
        }
        SampleFormat::U16 => {
            let stream_samples = Arc::clone(&samples);
            let stream_error = Arc::clone(&error_slot);
            device.build_input_stream(
                config,
                move |data: &[u16], _| append_samples(&stream_samples, data.iter().map(u16_to_i16)),
                move |error: cpal::Error| record_stream_error(&stream_error, error),
                None,
            )
        }
        format => anyhow::bail!("unsupported microphone sample format {format}"),
    }
    .map_err(|error| anyhow::anyhow!("failed to open microphone stream: {error}"))?;

    stream
        .play()
        .map_err(|error| anyhow::anyhow!("failed to start microphone stream: {error}"))?;
    if stop_on_pause {
        wait_for_pause(&samples, Duration::from_secs(seconds))?;
    } else {
        std::thread::sleep(Duration::from_secs(seconds));
    }
    drop(stream);

    if let Some(error) = error_slot.lock().ok().and_then(|mut slot| slot.take()) {
        anyhow::bail!("microphone stream failed: {error}");
    }
    let samples = samples
        .lock()
        .map_err(|_| anyhow::anyhow!("microphone sample buffer was poisoned"))?;
    anyhow::ensure!(!samples.is_empty(), "microphone captured no audio samples");
    let spec = hound::WavSpec {
        channels: config.channels,
        sample_rate: config.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(output_path, spec)?;
    for sample in samples.iter().copied() {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    Ok(())
}

fn wait_for_pause(samples: &Arc<Mutex<Vec<i16>>>, max_duration: Duration) -> anyhow::Result<()> {
    const SPEECH_RMS: f32 = 0.015;
    const PAUSE: Duration = Duration::from_millis(900);
    let started_at = Instant::now();
    let mut inspected = 0;
    let mut speech_started = false;
    let mut last_voice = started_at;

    while started_at.elapsed() < max_duration {
        std::thread::sleep(Duration::from_millis(50));
        let (rms, new_len) = {
            let data = samples
                .lock()
                .map_err(|_| anyhow::anyhow!("microphone sample buffer was poisoned"))?;
            let chunk = &data[inspected.min(data.len())..];
            let rms = if chunk.is_empty() {
                0.0
            } else {
                let energy = chunk
                    .iter()
                    .map(|sample| {
                        let normalized = *sample as f32 / i16::MAX as f32;
                        normalized * normalized
                    })
                    .sum::<f32>();
                (energy / chunk.len() as f32).sqrt()
            };
            (rms, data.len())
        };
        inspected = new_len;
        if rms >= SPEECH_RMS {
            speech_started = true;
            last_voice = Instant::now();
        } else if speech_started && last_voice.elapsed() >= PAUSE {
            return Ok(());
        }
    }
    anyhow::ensure!(
        speech_started,
        "no speech detected before auto-listen timed out"
    );
    Ok(())
}

fn record_stream_error(slot: &Arc<Mutex<Option<String>>>, error: cpal::Error) {
    if let Ok(mut slot) = slot.lock() {
        *slot = Some(error.to_string());
    }
}

fn append_samples<I>(target: &Arc<Mutex<Vec<i16>>>, samples: I)
where
    I: IntoIterator<Item = i16>,
{
    if let Ok(mut target) = target.lock() {
        target.extend(samples);
    }
}

fn f32_to_i16(sample: &f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

fn u16_to_i16(sample: &u16) -> i16 {
    (*sample as i32 - 32_768) as i16
}
