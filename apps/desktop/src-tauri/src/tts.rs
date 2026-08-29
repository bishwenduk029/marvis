//! Offline text-to-speech via sherpa-onnx Kokoro. Generates full audio;
//! playback streams it out in `audio::play`.

use sherpa_onnx::{GenerationConfig, OfflineTts, OfflineTtsConfig, OfflineTtsKokoroModelConfig};
use std::sync::{Mutex, OnceLock};

use crate::paths;

static TTS: OnceLock<Mutex<OfflineTts>> = OnceLock::new();

fn engine() -> Result<&'static Mutex<OfflineTts>, String> {
    TTS.get_or_init(|| {
        let config = OfflineTtsConfig {
            model: sherpa_onnx::OfflineTtsModelConfig {
                kokoro: OfflineTtsKokoroModelConfig {
                    model: Some(
                        paths::model("kokoro/model.onnx").to_string_lossy().into_owned(),
                    ),
                    voices: Some(
                        paths::model("kokoro/voices.bin").to_string_lossy().into_owned(),
                    ),
                    tokens: Some(
                        paths::model("kokoro/tokens.txt").to_string_lossy().into_owned(),
                    ),
                    data_dir: Some(
                        paths::model("kokoro/espeak-ng-data")
                            .to_string_lossy()
                            .into_owned(),
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
        Mutex::new(
            OfflineTts::create(&config).expect("failed to load Kokoro model (run scripts/fetch-models.sh)"),
        )
    });
    Ok(TTS.get().unwrap())
}

pub struct Utterance {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

pub fn synthesize(text: &str) -> Result<Utterance, String> {
    let engine = engine()?;
    let guard = engine.lock().map_err(|_| "tts lock poisoned".to_string())?;
    let gen = GenerationConfig { sid: 0, speed: 1.0, ..Default::default() };
    let audio = guard
        .generate_with_config(text, &gen, None::<fn(&[f32], f32) -> bool>)
        .ok_or_else(|| "tts failed".to_string())?;
    Ok(Utterance { samples: audio.samples().to_vec(), sample_rate: audio.sample_rate() as u32 })
}
