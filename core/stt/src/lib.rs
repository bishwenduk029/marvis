//! Speech-to-text: microphone capture with Silero VAD, then SenseVoice
//! transcription. Models load on first use and are cached for the process.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use marvis_common::{model, Emit, energy, resample};
use sherpa_onnx::{
    OfflineRecognizer, OfflineRecognizerConfig, OfflineSenseVoiceModelConfig, SileroVadModelConfig,
    VadModelConfig, VoiceActivityDetector,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

const TARGET_RATE: u32 = 16000;
const WINDOW: usize = 512;

/// A recorded utterance: mono f32 samples at `sample_rate`.
pub struct Speech {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

/// Record until the user stops talking (VAD + trailing silence), `stop` flips,
/// or the hard time cap hits. Returns the speech-only samples.
pub fn record_speech(emit: Emit, stop: Arc<AtomicBool>) -> Option<Speech> {
    let host = cpal::default_host();
    let device = host.default_input_device()?;
    let supported = device.default_input_config().ok()?;
    let config = supported.config();
    let in_rate = config.sample_rate.0;
    let channels = config.channels as usize;
    let sample_format = supported.sample_format();

    let vad = make_vad()?;

    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let err_fn = |e| eprintln!("mic error: {e}");

    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(
            &config,
            {
                let tx = tx.clone();
                move |data: &[f32], _| {
                    let _ = tx.send(mono_f32(data, channels));
                }
            },
            err_fn,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            &config,
            {
                let tx = tx.clone();
                move |data: &[i16], _| {
                    let _ = tx.send(data.chunks(channels).map(|f| f[0] as f32 / 32768.0).collect());
                }
            },
            err_fn,
            None,
        ),
        SampleFormat::I32 => device.build_input_stream(
            &config,
            {
                let tx = tx.clone();
                move |data: &[i32], _| {
                    let _ = tx
                        .send(data.chunks(channels).map(|f| f[0] as f32 / 2147483648.0).collect());
                }
            },
            err_fn,
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
            &config,
            {
                let tx = tx.clone();
                move |data: &[u16], _| {
                    let _ = tx
                        .send(data.chunks(channels).map(|f| (f[0] as f32 - 32768.0) / 32768.0).collect());
                }
            },
            err_fn,
            None,
        ),
        _ => {
            eprintln!("marvis-stt: unsupported mic sample format: {sample_format:?}");
            return None;
        }
    }
    .ok()?;
    if stream.play().is_err() {
        return None;
    }

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
        for w in resample(&chunk, in_rate, TARGET_RATE).chunks(WINDOW) {
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

/// Transcribe speech samples with SenseVoice (cached recognizer).
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
    RECOGNIZER.get_or_try_init(|| {
        let mut config = OfflineRecognizerConfig::default();
        config.model_config.sense_voice = OfflineSenseVoiceModelConfig {
            model: Some(model("sense-voice/model.onnx").to_string_lossy().into_owned()),
            language: Some("auto".into()),
            use_itn: true,
        };
        config.model_config.tokens =
            Some(model("sense-voice/tokens.txt").to_string_lossy().into_owned());
        config.model_config.provider = Some("cpu".into());
        config.model_config.num_threads = 2;
        config.model_config.debug = false;
        OfflineRecognizer::create(&config)
            .ok_or_else(|| "failed to load SenseVoice (run scripts/fetch-models.sh)".to_string())
            .map(std::sync::Mutex::new)
    })
}

fn mono_f32(data: &[f32], channels: usize) -> Vec<f32> {
    data.chunks(channels).map(|f| f.iter().sum::<f32>() / channels as f32).collect()
}
