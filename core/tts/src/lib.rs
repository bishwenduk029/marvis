//! Text-to-speech: vits/piper synthesis + `paplay` playback with energy
//! metering. The engine loads on first use and is cached for the process.

use marvis_common::{model, Emit, energy};
use sherpa_onnx::{GenerationConfig, OfflineTts, OfflineTtsConfig, OfflineTtsVitsModelConfig};
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// Synthesized speech: mono f32 samples at `sample_rate`.
pub struct Utterance {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

/// Synthesize text with vits/piper.
pub fn synthesize(text: &str) -> Result<Utterance, String> {
    let engine = engine()?;
    let guard = engine.lock().map_err(|_| "tts lock poisoned".to_string())?;
    let gen = GenerationConfig { sid: 0, speed: 1.0, ..Default::default() };
    let audio = guard
        .generate_with_config(text, &gen, None::<fn(&[f32], f32) -> bool>)
        .ok_or_else(|| "tts failed".to_string())?;
    Ok(Utterance { samples: audio.samples().to_vec(), sample_rate: audio.sample_rate() as u32 })
}

/// Play samples through `paplay --raw`, emitting RMS energy paced with real
/// playback. Returns `false` if playback was interrupted or failed.
pub fn play(u: &Utterance, emit: Emit, stop: Arc<AtomicBool>) -> bool {
    if u.samples.is_empty() {
        return true;
    }
    let mut child = match Command::new("paplay")
        .args([
            "--raw",
            "--format=s16le",
            "--channels=1",
            &format!("--rate={}", u.sample_rate),
        ])
        .stdin(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .or_else(|_| {
            Command::new("pw-play")
                .args([
                    "--format",
                    "s16",
                    "--channels",
                    "1",
                    "--rate",
                    &u.sample_rate.to_string(),
                    "-",
                ])
                .stdin(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
        }) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let mut stdin = match child.stdin.take() {
        Some(s) => s,
        None => return false,
    };

    // Pace writes at real-time so energy events track what is heard.
    let chunk_dur = Duration::from_secs_f32(1600.0 / u.sample_rate as f32); // 100 ms
    for chunk in u.samples.chunks(1600) {
        if stop.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return false;
        }
        let mut bytes = Vec::with_capacity(chunk.len() * 2);
        let mut sum = 0.0;
        for &s in chunk {
            let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
            bytes.extend_from_slice(&v.to_le_bytes());
            sum += s * s;
        }
        if stdin.write_all(&bytes).is_err() {
            return false;
        }
        emit(energy(chunk).min(1.0));
        std::thread::sleep(chunk_dur);
    }
    drop(stdin); // EOF: paplay drains and exits
    let _ = child.wait();
    true
}

/// Load the TTS engine so the first turn is already warm.
pub fn warmup() {
    let _ = engine();
}

// --- internals -------------------------------------------------------------

fn engine() -> Result<&'static Mutex<OfflineTts>, String> {
    static TTS: OnceLock<Mutex<OfflineTts>> = OnceLock::new();
    if let Some(t) = TTS.get() {
        return Ok(t);
    }
    // Vits/Piper: ~60 MB and ~12x faster than Kokoro on a weak CPU.
    let config = OfflineTtsConfig {
        model: sherpa_onnx::OfflineTtsModelConfig {
            vits: OfflineTtsVitsModelConfig {
                model: Some(model("vits-amy/en_US-amy-medium.onnx").to_string_lossy().into_owned()),
                tokens: Some(model("vits-amy/tokens.txt").to_string_lossy().into_owned()),
                data_dir: Some(model("vits-amy/espeak-ng-data").to_string_lossy().into_owned()),
                noise_scale: 0.667,
                noise_scale_w: 0.8,
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
        .ok_or_else(|| "failed to load vits model (run scripts/fetch-models.sh)".to_string())?;
    let _ = TTS.set(Mutex::new(tts));
    Ok(TTS.get().unwrap())
}
