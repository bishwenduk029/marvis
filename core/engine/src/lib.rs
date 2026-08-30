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
/// mid-speaking. In conversation mode (default) she keeps listening after
/// each reply: half-duplex, so the mic opens only after her voice finished
/// and she can never hear herself. A silent follow-up window ends the
/// conversation on its own; clicking while listening ends it immediately.
/// `MARVIS_CONVERSATION=0` gives one turn per click.
pub fn run_turn(on_event: impl Fn(&Event) + Send + Sync + 'static, stop: Arc<AtomicBool>) {
    let on_event = Arc::new(on_event);

    let emit_energy: Emit = {
        let on_event = on_event.clone();
        Arc::new(move |level| on_event(&Event::Energy(level.clamp(0.0, 1.0))))
    };
    let emit = |e: Event| on_event(&e);

    // Errors are spoken, not just shown: a voice assistant that fails
    // silently reads as "dead". `shown` goes to the UI verbatim (with the
    // (stt)/(brain) tag the daemon logs); `spoken` is the short phrase
    // passed to TTS.
    let speak_error = {
        let on_event = on_event.clone();
        let emit_energy = emit_energy.clone();
        let stop = stop.clone();
        move |shown: String, spoken: &str| {
            on_event(&Event::Reply(shown));
            if let Ok(u) = marvis_tts::synthesize(spoken) {
                on_event(&Event::State(Phase::Speaking));
                marvis_tts::play(&u, emit_energy, stop);
            }
            on_event(&Event::State(Phase::Idle));
        }
    };

    let converse = std::env::var("MARVIS_CONVERSATION")
        .map(|v| !matches!(v.as_str(), "0" | "false" | "off"))
        .unwrap_or(true);
    let mut relisten = false;

    loop {
        // 1. Listen. On follow-up passes, a 30s silent window ends the
        // conversation: a watchdog flips stop, which record_speech honours.
        emit(Event::State(Phase::Listening));
        let watchdog = if relisten {
            let stop = stop.clone();
            let done = Arc::new(AtomicBool::new(false));
            let done2 = done.clone();
            Some((
                std::thread::spawn(move || {
                    for _ in 0..300 {
                        if done2.load(Ordering::Relaxed) {
                            return;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    stop.store(true, Ordering::Relaxed);
                }),
                done,
            ))
        } else {
            None
        };
        let speech = match marvis_stt::record_speech(emit_energy.clone(), stop.clone()) {
            Some(s) => s,
            None => {
                if let Some((w, done)) = watchdog {
                    done.store(true, Ordering::Relaxed);
                    let _ = w.join();
                }
                if relisten {
                    // Conversation over: silence or a click. Exit quietly.
                    emit(Event::State(Phase::Idle));
                    return;
                }
                if !stop.load(Ordering::Relaxed) {
                    speak_error("I didn't catch that.".into(), "I didn't catch that.");
                } else {
                    emit(Event::State(Phase::Idle));
                }
                return;
            }
        };
        if let Some((w, done)) = watchdog {
            done.store(true, Ordering::Relaxed);
            let _ = w.join();
        }
        if stop.load(Ordering::Relaxed) {
            emit(Event::State(Phase::Idle));
            return;
        }

        // 2. Understand.
        emit(Event::State(Phase::Thinking));
        let text = match marvis_stt::transcribe(&speech.samples, speech.sample_rate) {
            Ok(t) => t,
            Err(e) => {
                speak_error(format!("(stt) {e}"), "I couldn't make out what you said.");
                return;
            }
        };
        emit(Event::Transcript(text.clone()));

        let reply = {
            let on_event = on_event.clone();
            // Barge-in while thinking: stop can fire mid-turn (a new `start` or
            // `interrupt` command), so a watcher cancels the in-flight jcode turn
            // instead of waiting for a reply nobody will hear.
            let done = Arc::new(AtomicBool::new(false));
            let watcher = {
                let done = done.clone();
                let stop = stop.clone();
                std::thread::spawn(move || {
                    while !stop.load(Ordering::Relaxed) && !done.load(Ordering::Relaxed) {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    if !done.load(Ordering::Relaxed) {
                        marvis_harness::interrupt();
                    }
                })
            };
            let result = marvis_harness::run(&text, move |line| {
                if !line.is_empty() {
                    on_event(&Event::Activity(line.to_string()));
                }
            });
            done.store(true, Ordering::Relaxed);
            let _ = watcher.join();
            // A barge-in during thinking means the user already moved on; don't
            // speak the stale reply.
            if stop.load(Ordering::Relaxed) {
                emit(Event::State(Phase::Idle));
                return;
            }
            match result {
                Ok(r) => r,
                Err(e) => {
                    speak_error(
                        format!("(brain) {e}"),
                        "Sorry, something went wrong while thinking.",
                    );
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
        marvis_tts::play(&utterance, emit_energy.clone(), stop.clone());

        // 4. Conversation: relisten after a short settle so her last words
        // leave the room before the mic opens (half-duplex anti-echo).
        if !converse || stop.load(Ordering::Relaxed) {
            break;
        }
        relisten = true;
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    emit(Event::State(Phase::Idle));
}
