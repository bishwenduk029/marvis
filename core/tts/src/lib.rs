//! Text-to-speech: Kokoro synthesis + speaker playback with energy metering.
//! The engine loads on first use and is cached for the process.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use marvis_common::{model, Emit, energy};
use sherpa_onnx::{GenerationConfig, OfflineTts, OfflineTtsConfig, OfflineTtsKokoroModelConfig};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// Synthesized speech: mono f32 samples at `sample_rate`.
pub struct Utterance {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

/// Synthesize text with Kokoro.
pub fn synthesize(text: &str) -> Result<Utterance, String> {
    let engine = engine()?;
    let guard = engine.lock().map_err(|_| "tts lock poisoned".to_string())?;
    let gen = GenerationConfig { sid: 0, speed: 1.0, ..Default::default() };
    let audio = guard
        .generate_with_config(text, &gen, None::<fn(&[f32], f32) -> bool>)
        .ok_or_else(|| "tts failed".to_string())?;
    Ok(Utterance { samples: audio.samples().to_vec(), sample_rate: audio.sample_rate() as u32 })
}

/// Play samples on the default output device, emitting RMS energy. Returns
/// `false` if playback was interrupted or failed.
pub fn play(u: &Utterance, emit: Emit, stop: Arc<AtomicBool>) -> bool {
    if u.samples.is_empty() {
        return true;
    }
    let host = cpal::default_host();
    let device = match host.default_output_device() {
        Some(d) => d,
        None => return false,
    };
    let supported = match device.default_output_config() {
        Ok(s) => s,
        Err(_) => return false,
    };
    let config = supported.config();
    let channels = config.channels as usize;
    let sample_format = supported.sample_format();

    let samples = Arc::new(u.samples.clone());
    let cursor = Arc::new(Mutex::new(0usize));
    let err_fn = |e| eprintln!("speaker error: {e}");

    let stream = match sample_format {
        SampleFormat::F32 => device.build_output_stream(
            &config,
            {
                let (s, c, e) = (samples.clone(), cursor.clone(), emit.clone());
                move |data: &mut [f32], _| write_f32(data, &s, &c, &e, channels)
            },
            err_fn,
            None,
        ),
        SampleFormat::I16 => device.build_output_stream(
            &config,
            {
                let (s, c, e) = (samples.clone(), cursor.clone(), emit.clone());
                move |data: &mut [i16], _| write_i16(data, &s, &c, &e, channels)
            },
            err_fn,
            None,
        ),
        SampleFormat::U16 => device.build_output_stream(
            &config,
            {
                let (s, c, e) = (samples.clone(), cursor.clone(), emit.clone());
                move |data: &mut [u16], _| write_u16(data, &s, &c, &e, channels)
            },
            err_fn,
            None,
        ),
        _ => return false,
    };
    let stream = match stream {
        Ok(s) => s,
        Err(_) => return false,
    };
    if stream.play().is_err() {
        return false;
    }

    let total_frames = samples.len();
    let frame = Duration::from_secs_f32(1.0 / u.sample_rate as f32);
    loop {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        if *cursor.lock().unwrap() >= total_frames {
            std::thread::sleep(frame * 64); // let the tail play out
            return true;
        }
        std::thread::sleep(frame * 32);
    }
}

// --- internals -------------------------------------------------------------

fn engine() -> Result<&'static Mutex<OfflineTts>, String> {
    static TTS: OnceLock<Mutex<OfflineTts>> = OnceLock::new();
    if let Some(t) = TTS.get() {
        return Ok(t);
    }
    let config = OfflineTtsConfig {
        model: sherpa_onnx::OfflineTtsModelConfig {
            kokoro: OfflineTtsKokoroModelConfig {
                model: Some(model("kokoro/model.onnx").to_string_lossy().into_owned()),
                voices: Some(model("kokoro/voices.bin").to_string_lossy().into_owned()),
                tokens: Some(model("kokoro/tokens.txt").to_string_lossy().into_owned()),
                data_dir: Some(
                    model("kokoro/espeak-ng-data").to_string_lossy().into_owned(),
                ),
                length_scale: 1.0,
                ..Default::default()
            },
            num_threads: 2,
            debug: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let tts = OfflineTts::create(&config)
        .ok_or_else(|| "failed to load Kokoro (run scripts/fetch-models.sh)".to_string())?;
    let _ = TTS.set(Mutex::new(tts));
    Ok(TTS.get().unwrap())
}

fn write_f32(data: &mut [f32], samples: &[f32], cursor: &Mutex<usize>, emit: &Emit, channels: usize) {
    let mut c = cursor.lock().unwrap();
    for frame in data.chunks_mut(channels) {
        let s = if *c < samples.len() { samples[*c] } else { 0.0 };
        for ch in frame.iter_mut() {
            *ch = s;
        }
        *c += 1;
    }
    let start = c.saturating_sub(data.len() / channels);
    if start < samples.len() {
        emit(energy(&samples[start..c.min(samples.len())]));
    }
}

fn write_i16(data: &mut [i16], samples: &[f32], cursor: &Mutex<usize>, emit: &Emit, channels: usize) {
    let mut c = cursor.lock().unwrap();
    for frame in data.chunks_mut(channels) {
        let s = if *c < samples.len() { samples[*c] } else { 0.0 };
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        for ch in frame.iter_mut() {
            *ch = v;
        }
        *c += 1;
    }
    let start = c.saturating_sub(data.len() / channels);
    if start < samples.len() {
        emit(energy(&samples[start..c.min(samples.len())]));
    }
}

fn write_u16(data: &mut [u16], samples: &[f32], cursor: &Mutex<usize>, emit: &Emit, channels: usize) {
    let mut c = cursor.lock().unwrap();
    for frame in data.chunks_mut(channels) {
        let s = if *c < samples.len() { samples[*c] } else { 0.0 };
        let v = ((s.clamp(-1.0, 1.0) * 32767.0) + 32768.0) as u16;
        for ch in frame.iter_mut() {
            *ch = v;
        }
        *c += 1;
    }
    let start = c.saturating_sub(data.len() / channels);
    if start < samples.len() {
        emit(energy(&samples[start..c.min(samples.len())]));
    }
}
