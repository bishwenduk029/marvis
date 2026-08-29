//! Round-trip benchmark on this machine: Kokoro synthesizes a sentence, then
//! SenseVoice transcribes that same audio. Reports cold vs warm real-time
//! factors and peak RSS, so model choices are data, not hope.
//!
//! Run: cargo run -p marvis-engine --example voicebench

use std::time::Instant;

fn peak_rss_mb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM"))
                .and_then(|l| l.split_whitespace().nth(1).and_then(|v| v.parse::<u64>().ok()))
        })
        .map(|kb| kb / 1024)
        .unwrap_or(0)
}

fn main() {
    let text = "Hello, I am Marvis, your local voice assistant, running fully on device.";
    let text2 = "This second sentence is timed with a warm model.";

    println!("== Kokoro TTS ==");
    let t = Instant::now();
    let u1 = match marvis_tts::synthesize(text) {
        Ok(u) => u,
        Err(e) => {
            eprintln!("tts failed: {e}");
            return;
        }
    };
    let cold = t.elapsed().as_secs_f32();
    let dur1 = u1.samples.len() as f32 / u1.sample_rate as f32;
    println!("  cold (load+synth): {cold:.2}s  audio {dur1:.2}s  rtf {:.2}", cold / dur1);

    let t = Instant::now();
    let u2 = marvis_tts::synthesize(text2).expect("warm tts failed");
    let warm = t.elapsed().as_secs_f32();
    let dur2 = u2.samples.len() as f32 / u2.sample_rate as f32;
    println!("  warm synth:        {warm:.2}s  audio {dur2:.2}s  rtf {:.2}", warm / dur2);

    println!("== SenseVoice ASR ==");
    let t = Instant::now();
    match marvis_stt::transcribe(&u1.samples, u1.sample_rate) {
        Ok(heard) => {
            let e = t.elapsed().as_secs_f32();
            println!("  cold (load+asr):   {e:.2}s  for {dur1:.2}s audio  rtf {:.2}", e / dur1);
            println!("  heard: \"{heard}\"");
        }
        Err(e) => eprintln!("  stt failed: {e}"),
    }
    let t = Instant::now();
    match marvis_stt::transcribe(&u2.samples, u2.sample_rate) {
        Ok(heard) => {
            let e = t.elapsed().as_secs_f32();
            println!("  warm asr:          {e:.2}s  for {dur2:.2}s audio  rtf {:.2}", e / dur2);
            println!("  heard: \"{heard}\"");
        }
        Err(e) => eprintln!("  stt failed: {e}"),
    }

    println!("== peak RSS: {} MB ==", peak_rss_mb());
}
