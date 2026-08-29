//! Marvis daemon — a Unix-socket server wrapping the voice engine.
//!
//! Protocol (JSON lines):
//!   client -> daemon: {"cmd":"start"} | {"cmd":"interrupt"} | {"cmd":"ping"} | {"cmd":"quit"}
//!   daemon -> client: {"event":"<name>","value":<...>}
//!
//! Events mirror `marvis_engine::Event` (state/energy/transcript/reply/
//! activity); `energy` is a JSON number, everything else a JSON string.

use marvis_engine::{run_turn, Event};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

type Clients = Arc<Mutex<Vec<UnixStream>>>;

fn socket_path() -> PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(dir).join("marvis.sock")
}

fn broadcast(clients: &Clients, event: &Event) {
    let value = match event {
        Event::Energy(v) => serde_json::json!(v),
        _ => serde_json::json!(event.value()),
    };
    let line = format!("{}\n", serde_json::json!({ "event": event.name(), "value": value }));
    let mut list = clients.lock().unwrap();
    list.retain_mut(|s| s.write_all(line.as_bytes()).is_ok());
}

fn handle_client(stream: UnixStream, clients: Clients, stop: Arc<AtomicBool>, running: Arc<AtomicBool>) {
    for line in BufReader::new(stream).lines().map_while(Result::ok) {
        let cmd: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        match cmd["cmd"].as_str() {
            Some("start") => {
                // One turn at a time; ignore start while already running.
                if running.swap(true, Ordering::SeqCst) {
                    continue;
                }
                stop.store(false, Ordering::SeqCst);
                let (clients, stop, running) = (clients.clone(), stop.clone(), running.clone());
                std::thread::spawn(move || {
                    run_turn(move |e| broadcast(&clients, e), stop);
                    running.store(false, Ordering::SeqCst);
                });
            }
            Some("interrupt") => stop.store(true, Ordering::SeqCst),
            Some("ping") => {
                let mut list = clients.lock().unwrap();
                for s in list.iter_mut() {
                    let _ = s.write_all(b"{\"event\":\"pong\",\"value\":\"\"}\n");
                }
            }
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

    for stream in listener.incoming().map_while(Result::ok) {
        let write = match stream.try_clone() {
            Ok(w) => w,
            Err(_) => continue,
        };
        clients.lock().unwrap().push(write);
        let (clients, stop, running) = (clients.clone(), stop.clone(), running.clone());
        std::thread::spawn(move || handle_client(stream, clients, stop, running));
    }
}
