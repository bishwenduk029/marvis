//! Shared bits: model paths, an energy callback type, and small DSP helpers.

use std::path::PathBuf;
use std::sync::Arc;

/// A cheap callback for streaming a 0..1 audio level. `Arc` so it can be
/// shared into cpal audio-thread closures.
pub type Emit = Arc<dyn Fn(f32) + Send + Sync>;

/// Root directory for downloaded models (`~/.local/share/marvis/models` by
/// default, overridable with `MARVIS_MODELS`).
pub fn model_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("MARVIS_MODELS") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".local/share")
        });
    base.join("marvis/models")
}

/// Path to a model file relative to [`model_dir`].
pub fn model(name: &str) -> PathBuf {
    model_dir().join(name)
}

/// Root-mean-square of a sample buffer, scaled so typical speech sits in 0..1.
pub fn energy(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f32 = samples.iter().map(|s| s * s).sum();
    (sum / samples.len() as f32).sqrt() * 5.0
}

/// Naive linear resampler. Fine for 48k → 16k voice capture.
pub fn resample(input: &[f32], in_rate: u32, out_rate: u32) -> Vec<f32> {
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
