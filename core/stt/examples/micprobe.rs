//! Mic probe: calls record_speech and prints every step, to find hangs.
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn main() {
    eprintln!("probe: calling record_speech (12s window)...");
    let stop = Arc::new(AtomicBool::new(false));
    let emit: marvis_common::Emit = Arc::new(|v| eprintln!("probe: energy {v:.3}"));
    let speech = marvis_stt::record_speech(emit, stop);
    eprintln!(
        "probe: returned {} samples @ {}",
        speech.as_ref().map(|s| s.samples.len()).unwrap_or(0),
        speech.as_ref().map(|s| s.sample_rate).unwrap_or(0)
    );
}
