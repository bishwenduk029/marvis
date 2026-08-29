//! Offline speech-to-text via sherpa-onnx SenseVoice. The recognizer is
//! created once and cached; it is the most expensive object we hold.

use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineSenseVoiceModelConfig};
use std::sync::{Mutex, OnceLock};

use crate::paths;

static RECOGNIZER: OnceLock<Mutex<OfflineRecognizer>> = OnceLock::new();

fn recognizer() -> Result<&'static Mutex<OfflineRecognizer>, String> {
    RECOGNIZER.get_or_init(|| {
        let mut config = OfflineRecognizerConfig::default();
        config.model_config.sense_voice = OfflineSenseVoiceModelConfig {
            model: Some(
                paths::model("sense-voice/model.onnx").to_string_lossy().into_owned(),
            ),
            language: Some("auto".into()),
            use_itn: true,
        };
        config.model_config.tokens =
            Some(paths::model("sense-voice/tokens.txt").to_string_lossy().into_owned());
        config.model_config.provider = Some("cpu".into());
        config.model_config.num_threads = 2;
        config.model_config.debug = false;

        Mutex::new(
            OfflineRecognizer::create(&config)
                .expect("failed to load SenseVoice model (run scripts/fetch-models.sh)"),
        )
    });
    Ok(RECOGNIZER.get().unwrap())
}

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
