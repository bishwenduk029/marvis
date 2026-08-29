//! Speech-to-text: PipeWire capture (`parec`) + Silero VAD + Moonshine
//! transcription. Models load on first use and are cached for the process.

use marvis_common::{model, Emit, energy};
use sherpa_onnx::{
    OfflineMoonshineModelConfig, OfflineRecognizer, OfflineRecognizerConfig, SileroVadModelConfig,
    VadModelConfig, VoiceActivityDetector,
};
use std::io::Read;
use std::sync::Arc;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const TARGET_RATE: u32 = 16000;
const WINDOW: usize = 512;

/// A recorded utterance: mono f32 samples at `sample_rate`.
pub struct Speech {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

/// Record until the user stops talking (VAD + trailing silence), `stop`
/// flips, or the hard time cap hits. Returns the speech-only samples.
///
/// Capture is a `parec` subprocess (s16le mono 16 kHz on stdout): PipeWire's
/// own path, which measures clean on this machine, unlike cpal's ALSA route.
/// Killing the child gives instant interrupt latency.
pub fn record_speech(emit: Emit, stop: Arc<AtomicBool>) -> Option<Speech> {
    let mut child = Command::new("parec")
        .args(["--format=s16le", "--channels=1", "--rate=16000"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .or_else(|_| {
            eprintln!("marvis-stt: parec not found, trying pw-record");
            Command::new("pw-record")
                .args(["--format", "s16", "--channels", "1", "--rate", "16000", "-"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
        })
        .ok()?;
    let mut out = child.stdout.take()?;

    let vad = make_vad()?;
    let mut speech = Vec::new();
    let mut got_speech = false;
    let mut last_speech = Instant::now();
    let start = Instant::now();
    let mut buf = vec![0u8; 1024]; // 512 s16 samples ≈ 32 ms

    'outer: while !stop.load(Ordering::Relaxed) {
        if out.read_exact(&mut buf).is_err() {
            break;
        }
        let chunk: Vec<f32> = buf
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
            .collect();
        emit(energy(&chunk));
        for w in chunk.chunks(WINDOW) {
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
    let _ = child.kill();
    let _ = child.wait();
    vad.flush();
    while let Some(seg) = vad.front() {
        speech.extend_from_slice(seg.samples());
        vad.pop();
    }

    if got_speech && speech.len() as f32 / TARGET_RATE as f32 > 0.2 {
        Some(Speech { samples: speech, sample_rate: TARGET_RATE })
    } else {
        None
    }
}

/// Transcribe speech samples with Moonshine (cached recognizer).
pub fn transcribe(samples: &[f32], sample_rate: u32) -> Result<String, String> {
    let rec = recognizer()?;
    let guard = rec.lock().map_err(|_| "recognizer lock poisoned".to_string())?;
    let stream = guard.create_stream();
    stream.accept_waveform(sample_rate as i32, samples);
    guard.decode(&stream);
    match stream.get_result() {
        Some(r) if !r.text.trim().is_empty() => Ok(r.text.trim().to_string()),
        _ => Err("no speech recognized".into()),
    }
}

/// Load the VAD + recognizer so the first turn is already warm.
pub fn warmup() {
    let _ = recognizer();
}

// --- internals -------------------------------------------------------------

fn make_vad() -> Option<VoiceActivityDetector> {
    let mut silero = SileroVadModelConfig::default();
    silero.model = Some(model("silero_vad.onnx").to_string_lossy().into_owned());
    silero.threshold = 0.5;
    silero.min_silence_duration = 0.6;
    silero.min_speech_duration = 0.3;
    silero.max_speech_duration = 20.0;
    let config = VadModelConfig {
        silero_vad: silero,
        ten_vad: Default::default(),
        sample_rate: TARGET_RATE as i32,
        num_threads: 1,
        provider: Some("cpu".into()),
        debug: false,
    };
    VoiceActivityDetector::create(&config, 30.0)
}

fn recognizer() -> Result<&'static std::sync::Mutex<OfflineRecognizer>, String> {
    use std::sync::OnceLock;
    static RECOGNIZER: OnceLock<std::sync::Mutex<OfflineRecognizer>> = OnceLock::new();
    if let Some(r) = RECOGNIZER.get() {
        return Ok(r);
    }
    // Moonshine tiny int8: ~120 MB total, built for fast CPU transcription.
    let mut config = OfflineRecognizerConfig::default();
    config.model_config.moonshine = OfflineMoonshineModelConfig {
        preprocessor: Some(model("moonshine/preprocess.onnx").to_string_lossy().into_owned()),
        encoder: Some(model("moonshine/encode.int8.onnx").to_string_lossy().into_owned()),
        cached_decoder: Some(
            model("moonshine/cached_decode.int8.onnx").to_string_lossy().into_owned(),
        ),
        uncached_decoder: Some(
            model("moonshine/uncached_decode.int8.onnx").to_string_lossy().into_owned(),
        ),
        merged_decoder: None,
    };
    config.model_config.tokens = Some(model("moonshine/tokens.txt").to_string_lossy().into_owned());
    config.model_config.provider = Some("cpu".into());
    config.model_config.num_threads = 2;
    config.model_config.debug = false;
    let rec = OfflineRecognizer::create(&config)
        .ok_or_else(|| "failed to load Moonshine (run scripts/fetch-models.sh)".to_string())?;
    let _ = RECOGNIZER.set(std::sync::Mutex::new(rec));
    Ok(RECOGNIZER.get().unwrap())
}
