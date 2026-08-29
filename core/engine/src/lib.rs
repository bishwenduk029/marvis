//! The voice pipeline: listen → transcribe → think → speak.
//!
//! UI-agnostic — it emits [`Event`]s through a callback. The Tauri app maps
//! them to window events; the daemon maps them to JSON socket messages.

use marvis_common::Emit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The four states a UI renders.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Idle,
    Listening,
    Thinking,
    Speaking,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Listening => "listening",
            Phase::Thinking => "thinking",
            Phase::Speaking => "speaking",
        }
    }
}

/// Everything the engine reports. Serialize with [`Event::name`] +
/// [`Event::value`]; `Energy` is a plain number string.
#[derive(Clone, Debug)]
pub enum Event {
    State(Phase),
    Energy(f32),
    Transcript(String),
    Reply(String),
    Activity(String),
}

impl Event {
    pub fn name(&self) -> &'static str {
        match self {
            Event::State(_) => "state",
            Event::Energy(_) => "energy",
            Event::Transcript(_) => "transcript",
            Event::Reply(_) => "reply",
            Event::Activity(_) => "activity",
        }
    }

    pub fn value(&self) -> String {
        match self {
            Event::State(p) => p.as_str().to_string(),
            Event::Energy(v) => format!("{v}"),
            Event::Transcript(t) | Event::Reply(t) | Event::Activity(t) => t.clone(),
        }
    }
}

/// Run one full turn. `stop` lets a caller interrupt mid-listening or
/// mid-speaking.
pub fn run_turn(on_event: impl Fn(&Event) + Send + Sync + 'static, stop: Arc<AtomicBool>) {
    let on_event = Arc::new(on_event);

    let emit_energy: Emit = {
        let on_event = on_event.clone();
        Arc::new(move |level| on_event(&Event::Energy(level.clamp(0.0, 1.0))))
    };
    let emit = |e: Event| on_event(&e);

    // 1. Listen.
    emit(Event::State(Phase::Listening));
    let speech = match marvis_stt::record_speech(emit_energy.clone(), stop.clone()) {
        Some(s) => s,
        None => {
            if !stop.load(Ordering::Relaxed) {
                emit(Event::Reply("I didn't catch that.".into()));
            }
            emit(Event::State(Phase::Idle));
            return;
        }
    };
    if stop.load(Ordering::Relaxed) {
        emit(Event::State(Phase::Idle));
        return;
    }

    // 2. Understand.
    emit(Event::State(Phase::Thinking));
    let text = match marvis_stt::transcribe(&speech.samples, speech.sample_rate) {
        Ok(t) => t,
        Err(e) => {
            emit(Event::Reply(format!("(stt) {e}")));
            emit(Event::State(Phase::Idle));
            return;
        }
    };
    emit(Event::Transcript(text.clone()));

    let reply = {
        let on_event = on_event.clone();
        match marvis_harness::run(&text, move |line| {
            if !line.is_empty() {
                on_event(&Event::Activity(line.to_string()));
            }
        }) {
            Ok(r) => r,
            Err(e) => {
                emit(Event::Reply(format!("(brain) {e}")));
                emit(Event::State(Phase::Idle));
                return;
            }
        }
    };
    emit(Event::Reply(reply.clone()));
    if stop.load(Ordering::Relaxed) {
        emit(Event::State(Phase::Idle));
        return;
    }

    // 3. Speak.
    emit(Event::State(Phase::Speaking));
    let utterance = match marvis_tts::synthesize(&reply) {
        Ok(u) => u,
        Err(e) => {
            emit(Event::Reply(format!("(tts) {e}")));
            emit(Event::State(Phase::Idle));
            return;
        }
    };
    marvis_tts::play(&utterance, emit_energy, stop);
    emit(Event::State(Phase::Idle));
}
