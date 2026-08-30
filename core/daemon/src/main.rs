//! Marvis daemon — a Unix-socket server wrapping the voice engine.
//!
//! Protocol (JSON lines):
//!   client -> daemon: {"cmd":"start"} | {"cmd":"interrupt"} | {"cmd":"say"} | {"cmd":"ping"} | {"cmd":"quit"}
//!   daemon -> client: {"event":"<name>","value":<...>}
//!
//! Events mirror `marvis_engine::Event` (state/energy/transcript/reply/
//! activity); `energy` is a JSON number, everything else a JSON string.
//!
//! Each client gets its own writer thread with a bounded queue: broadcast is
//! non-blocking (try_send), so a dead or slow client can never stall the
//! voice pipeline — its events are simply dropped until it reconnects.

use marvis_engine::{run_turn, Event, Phase};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Clients = Arc<Mutex<Vec<SyncSender<String>>>>;

fn socket_path() -> PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(dir).join("marvis.sock")
}

fn broadcast_raw(clients: &Clients, line: &str) {
    let mut list = clients.lock().unwrap();
    list.retain(|tx| match tx.try_send(format!("{line}\n")) {
        Ok(()) => true,
        Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => false,
    });
}

fn broadcast(clients: &Clients, event: &Event) {
    // Errors surface as replies; log them so failures aren't silent.
    if let Event::Reply(text) = event {
        if text.starts_with('(') {
            eprintln!("marvis-daemon: turn error: {text}");
        }
    }
    let value = match event {
        Event::Energy(v) => serde_json::json!(v),
        _ => serde_json::json!(event.value()),
    };
    let line = format!("{}\n", serde_json::json!({ "event": event.name(), "value": value }));
    let mut list = clients.lock().unwrap();
    list.retain(|tx| match tx.try_send(line.clone()) {
        Ok(()) => true,
        Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => false,
    });
}

fn handle_client(
    stream: UnixStream,
    clients: Clients,
    stop: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
) {
    for line in BufReader::new(stream).lines().map_while(Result::ok) {
        let cmd: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        match cmd["cmd"].as_str() {
            Some("start") => {
                // One turn at a time. A start while running means barge-in:
                // stop the current turn, wait for it to wind down, then go.
                if running.swap(true, Ordering::SeqCst) {
                    stop.store(true, Ordering::SeqCst);
                    let deadline = Instant::now() + Duration::from_millis(800);
                    while running.load(Ordering::SeqCst) && Instant::now() < deadline {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    if running.swap(true, Ordering::SeqCst) {
                        continue;
                    }
                }
                stop.store(false, Ordering::SeqCst);
                let (clients, stop, running) = (clients.clone(), stop.clone(), running.clone());
                std::thread::spawn(move || {
                    run_turn(move |e| broadcast(&clients, e), stop);
                    running.store(false, Ordering::SeqCst);
                });
            }
            Some("say") => {
                let text = cmd["value"].as_str().unwrap_or_default().to_string();
                if text.is_empty() || running.swap(true, Ordering::SeqCst) {
                    continue;
                }
                let (clients, running) = (clients.clone(), running.clone());
                std::thread::spawn(move || {
                    let emit: marvis_common::Emit = {
                        let clients = clients.clone();
                        Arc::new(move |v| broadcast(&clients, &Event::Energy(v)))
                    };
                    broadcast(&clients, &Event::State(Phase::Speaking));
                    match marvis_tts::synthesize(&text) {
                        Ok(u) => {
                            marvis_tts::play(&u, emit, Arc::new(AtomicBool::new(false)));
                        }
                        Err(e) => broadcast(&clients, &Event::Reply(format!("(tts) {e}"))),
                    }
                    broadcast(&clients, &Event::State(Phase::Idle));
                    running.store(false, Ordering::SeqCst);
                });
            }
            Some("interrupt") => stop.store(true, Ordering::SeqCst),
            Some("ping") => broadcast_raw(&clients, "{\"event\":\"pong\",\"value\":\"\"}"),
            Some("quit") => std::process::exit(0),
            _ => {}
        }
    }
}

fn main() {
    let path = socket_path();
    if path.exists() {
        let _ = std::fs::remove_file(&path);
    }
    let listener = UnixListener::bind(&path).expect("failed to bind marvis socket");
    println!("marvis-daemon listening on {path:?}");

    let clients: Clients = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let running = Arc::new(AtomicBool::new(false));

    // Pre-load both models in the background so the first turn is warm.
    std::thread::spawn(|| {
        marvis_stt::warmup();
        marvis_tts::warmup();
        println!("marvis-daemon: models warm");
    });

    for stream in listener.incoming().map_while(Result::ok) {
        let writer = match stream.try_clone() {
            Ok(w) => w,
            Err(_) => continue,
        };
        // Bounded queue: a full queue means a dead/slow client; try_send in
        // broadcast drops it from the list rather than ever blocking.
        let (tx, rx) = sync_channel::<String>(64);
        clients.lock().unwrap().push(tx);
        std::thread::spawn(move || {
            let mut writer = writer;
            for line in rx {
                if writer.write_all(line.as_bytes()).is_err() {
                    break;
                }
            }
        });
        let (clients, stop, running) = (clients.clone(), stop.clone(), running.clone());
        std::thread::spawn(move || handle_client(stream, clients, stop, running));
    }
}
