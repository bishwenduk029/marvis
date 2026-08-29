//! Mic capture (with Silero VAD) and speaker playback. All audio is mono f32
//! internally at 16 kHz for capture; playback uses the TTS native rate.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use crate::paths;

pub type Emit = Arc<dyn Fn(f32) + Send + Sync>;

const TARGET_RATE: u32 = 16000;
const WINDOW: usize = 512;

pub struct Speech {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

fn energy(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f32 = samples.iter().map(|s| s * s).sum();
    (sum / samples.len() as f32).sqrt() * 5.0
}

/// Naive linear resampler. Good enough for 48k -> 16k voice capture.
fn resample(input: &[f32], in_rate: u32, out_rate: u32) -> Vec<f32> {
    if in_rate == out_rate || input.len() < 2 {
        return input.to_vec();
    }
    let ratio = in_rate as f64 / out_rate as f64;
    let mut out = Vec::with_capacity((input.len() as f64 / ratio) as usize + 1);
    let mut i = 0.0f64;
    while i < input.len() as f64 {
        let idx = i as usize;
        let next = (idx + 1).min(input.len() - 1);
        let frac = (i - idx as f64) as f32;
        out.push(input[idx] * (1.0 - frac) + input[next] * frac);
        i += ratio;
    }
    out
}

/// Record until the user stops talking (VAD + trailing silence), or `stop`
/// flips, or the hard time cap hits. Returns the speech-only samples.
pub fn record_speech(emit: Emit, stop: Arc<AtomicBool>) -> Option<Speech> {
    let host = cpal::default_host();
    let device = host.default_input_device()?;
    let supported = device.default_input_config().ok()?;
    let config = supported.config();
    let in_rate = config.sample_rate.0;
    let channels = config.channels as usize;
    let sample_format = supported.sample_format();

    // Build VAD at the target rate (16k).
    let mut silero = SileroVadModelConfig::default();
    silero.model = Some(paths::model("silero_vad.onnx").to_string_lossy().into_owned());
    silero.threshold = 0.5;
    silero.min_silence_duration = 0.6;
    silero.min_speech_duration = 0.3;
    silero.max_speech_duration = 20.0;
    let vad_config = VadModelConfig {
        silero_vad: silero,
        ten_vad: Default::default(),
        sample_rate: TARGET_RATE as i32,
        num_threads: 1,
        provider: Some("cpu".into()),
        debug: false,
    };
    let vad = match VoiceActivityDetector::create(&vad_config, 30.0) {
        Some(v) => v,
        None => {
            eprintln!("marvis: silero_vad.onnx missing at {}", vad_config.silero_vad.model.as_deref().unwrap_or("?"));
            return None;
        }
    };

    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let err_fn = |e| eprintln!("mic error: {e}");

    let stream = match sample_format {
        SampleFormat::F32 => {
            let tx = tx.clone();
            device.build_input_stream(
                &config,
                move |data: &[f32], _| {
                    let mono = mono_f32(data, channels);
                    let _ = tx.send(mono);
                },
                err_fn,
                None,
            )
        }
        SampleFormat::I16 => {
            let tx = tx.clone();
            device.build_input_stream(
                &config,
                move |data: &[i16], _| {
                    let mono: Vec<f32> =
                        data.chunks(channels).map(|f| f[0] as f32 / 32768.0).collect();
                    let _ = tx.send(mono);
                },
                err_fn,
                None,
            )
        }
        SampleFormat::U16 => {
            let tx = tx.clone();
            device.build_input_stream(
                &config,
                move |data: &[u16], _| {
                    let mono: Vec<f32> = data
                        .chunks(channels)
                        .map(|f| (f[0] as f32 - 32768.0) / 32768.0)
                        .collect();
                    let _ = tx.send(mono);
                },
                err_fn,
                None,
            )
        }
        SampleFormat::I32 => {
            let tx = tx.clone();
            device.build_input_stream(
                &config,
                move |data: &[i32], _| {
                    let mono: Vec<f32> = data
                        .chunks(channels)
                        .map(|f| f[0] as f32 / 2147483648.0)
                        .collect();
                    let _ = tx.send(mono);
                },
                err_fn,
                None,
            )
        }
        _ => {
            eprintln!("marvis: unsupported mic sample format: {sample_format:?}");
            return None;
        }
    }
    .ok()?;
    stream.play().ok()?;

    let mut speech = Vec::new();
    let mut got_speech = false;
    let mut last_speech = Instant::now();
    let start = Instant::now();

    while !stop.load(Ordering::Relaxed) {
        let chunk = match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(c) => c,
            Err(_) => {
                if got_speech && last_speech.elapsed() > Duration::from_millis(1200) {
                    break;
                }
                continue;
            }
        };
        emit(energy(&chunk));
        let r = resample(&chunk, in_rate, TARGET_RATE);
        for w in r.chunks(WINDOW) {
            vad.accept_waveform(w);
            while let Some(seg) = vad.front() {
                speech.extend_from_slice(seg.samples());
                last_speech = Instant::now();
                got_speech = true;
                vad.pop();
            }
        }
        if got_speech && last_speech.elapsed() > Duration::from_millis(1200) {
            break;
        }
        if start.elapsed() > Duration::from_secs(30) {
            break;
        }
    }
    vad.flush();
    while let Some(seg) = vad.front() {
        speech.extend_from_slice(seg.samples());
        vad.pop();
    }

    drop(stream);
    if got_speech && speech.len() as f32 / TARGET_RATE as f32 > 0.2 {
        Some(Speech { samples: speech, sample_rate: TARGET_RATE })
    } else {
        None
    }
}

fn mono_f32(data: &[f32], channels: usize) -> Vec<f32> {
    data.chunks(channels)
        .map(|f| f.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// Play samples through the default output device, emitting RMS energy.
/// Returns false if playback was interrupted or failed.
pub fn play(samples: &[f32], rate: u32, emit: Emit, stop: Arc<AtomicBool>) -> bool {
    if samples.is_empty() {
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

    let samples = Arc::new(samples.to_vec());
    let cursor = Arc::new(Mutex::new(0usize));
    let err_fn = |e| eprintln!("speaker error: {e}");

    let stream = match sample_format {
        SampleFormat::F32 => {
            let (s, c, e) = (samples.clone(), cursor.clone(), emit.clone());
            device.build_output_stream(
                &config,
                move |data: &mut [f32], _| write_f32(data, &s, &c, &e, channels),
                err_fn,
                None,
            )
        }
        SampleFormat::I16 => {
            let (s, c, e) = (samples.clone(), cursor.clone(), emit.clone());
            device.build_output_stream(
                &config,
                move |data: &mut [i16], _| write_i16(data, &s, &c, &e, channels),
                err_fn,
                None,
            )
        }
        SampleFormat::U16 => {
            let (s, c, e) = (samples.clone(), cursor.clone(), emit.clone());
            device.build_output_stream(
                &config,
                move |data: &mut [u16], _| write_u16(data, &s, &c, &e, channels),
                err_fn,
                None,
            )
        }
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
    let frame_len = Duration::from_secs_f32(1.0 / rate as f32);
    // Sleep while playing, but wake often enough to honor interrupt.
    loop {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        let done = *cursor.lock().unwrap() >= total_frames;
        if done {
            std::thread::sleep(frame_len * 64); // let the tail play out
            return true;
        }
        std::thread::sleep(frame_len * 32);
    }
}

fn write_f32(
    data: &mut [f32],
    samples: &[f32],
    cursor: &Mutex<usize>,
    emit: &Emit,
    channels: usize,
) {
    let mut c = cursor.lock().unwrap();
    let frames = data.len() / channels;
    let mut sum = 0.0;
    let mut n = 0;
    for frame in data.chunks_mut(channels) {
        let s = if *c < samples.len() { samples[*c] } else { 0.0 };
        for ch in frame.iter_mut() {
            *ch = s;
        }
        sum += s * s;
        n += 1;
        *c += 1;
    }
    if n > 0 && *c <= samples.len() {
        emit((sum / n as f32).sqrt() * 5.0);
    }
    drop(c);
    let _ = frames;
}

fn write_i16(
    data: &mut [i16],
    samples: &[f32],
    cursor: &Mutex<usize>,
    emit: &Emit,
    channels: usize,
) {
    let mut c = cursor.lock().unwrap();
    let mut sum = 0.0;
    let mut n = 0;
    for frame in data.chunks_mut(channels) {
        let s = if *c < samples.len() { samples[*c] } else { 0.0 };
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        for ch in frame.iter_mut() {
            *ch = v;
        }
        sum += s * s;
        n += 1;
        *c += 1;
    }
    if n > 0 && *c <= samples.len() {
        emit((sum / n as f32).sqrt() * 5.0);
    }
}

fn write_u16(
    data: &mut [u16],
    samples: &[f32],
    cursor: &Mutex<usize>,
    emit: &Emit,
    channels: usize,
) {
    let mut c = cursor.lock().unwrap();
    let mut sum = 0.0;
    let mut n = 0;
    for frame in data.chunks_mut(channels) {
        let s = if *c < samples.len() { samples[*c] } else { 0.0 };
        let v = ((s.clamp(-1.0, 1.0) * 32767.0) + 32768.0) as u16;
        for ch in frame.iter_mut() {
            *ch = v;
        }
        sum += s * s;
        n += 1;
        *c += 1;
    }
    if n > 0 && *c <= samples.len() {
        emit((sum / n as f32).sqrt() * 5.0);
    }
}
