//! Marvis — a local AI voice agent. One small transparent orb window.
//! Idle state touches no mic, no models, no CPU loop.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod asr;
mod paths;
mod tts;
mod zerostack;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Idle,
    Listening,
    Thinking,
    Speaking,
}

impl Phase {
    fn as_str(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Listening => "listening",
            Phase::Thinking => "thinking",
            Phase::Speaking => "speaking",
        }
    }
}

struct App {
    phase: Mutex<Phase>,
    stop: Arc<AtomicBool>,
}

fn set_phase(app: &AppHandle, phase: Phase) {
    let app_state = app.state::<App>();
    {
        let mut current = app_state.phase.lock().unwrap();
        if *current == phase {
            return;
        }
        *current = phase;
    }
    let _ = app.emit("state", phase.as_str());
}

fn emitter(app: &AppHandle) -> audio::Emit {
    let app = app.clone();
    Arc::new(move |level: f32| {
        let _ = app.emit("energy", level.clamp(0.0, 1.0));
    })
}

#[tauri::command]
fn start(app: AppHandle) {
    let app_state = app.state::<App>();
    let stop = app_state.stop.clone();
    stop.store(false, Ordering::SeqCst);
    std::thread::spawn(move || run(app, stop));
}

#[tauri::command]
fn interrupt(app: AppHandle) {
    app.state::<App>().stop.store(true, Ordering::SeqCst);
}

fn run(app: AppHandle, stop: Arc<AtomicBool>) {
    let emit = emitter(&app);

    // 1. Listen.
    set_phase(&app, Phase::Listening);
    let speech = match audio::record_speech(emit.clone(), stop.clone()) {
        Some(s) => s,
        None => {
            if !stop.load(Ordering::Relaxed) {
                let _ = app.emit("reply", "I didn't catch that.");
            }
            set_phase(&app, Phase::Idle);
            return;
        }
    };
    if stop.load(Ordering::Relaxed) {
        set_phase(&app, Phase::Idle);
        return;
    }

    // 2. Understand.
    set_phase(&app, Phase::Thinking);
    let text = match asr::transcribe(&speech.samples, speech.sample_rate) {
        Ok(t) => t,
        Err(e) => {
            let _ = app.emit("reply", format!("(stt) {e}"));
            set_phase(&app, Phase::Idle);
            return;
        }
    };
    let _ = app.emit("transcript", &text);

    let reply = {
        let activity_app = app.clone();
        match zerostack::run(&text, move |line| {
            if !line.is_empty() {
                let _ = activity_app.emit("activity", line);
            }
        }) {
            Ok(r) => r,
            Err(e) => {
                let _ = app.emit("reply", format!("(brain) {e}"));
                set_phase(&app, Phase::Idle);
                return;
            }
        }
    };
    let _ = app.emit("reply", &reply);
    if stop.load(Ordering::Relaxed) {
        set_phase(&app, Phase::Idle);
        return;
    }

    // 3. Speak.
    set_phase(&app, Phase::Speaking);
    let utterance = match tts::synthesize(&reply) {
        Ok(u) => u,
        Err(e) => {
            let _ = app.emit("reply", format!("(tts) {e}"));
            set_phase(&app, Phase::Idle);
            return;
        }
    };
    audio::play(&utterance.samples, utterance.sample_rate, emit, stop);

    set_phase(&app, Phase::Idle);
}

fn main() {
    tauri::Builder::default()
        .manage(App { phase: Mutex::new(Phase::Idle), stop: Arc::new(AtomicBool::new(false)) })
        .invoke_handler(tauri::generate_handler![start, interrupt])
        .run(tauri::generate_context!())
        .expect("error while running marvis");
}
