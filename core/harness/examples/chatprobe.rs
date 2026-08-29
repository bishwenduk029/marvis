//! Chat probe: one harness round-trip against the configured LLM.
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn main() {
    let stop = Arc::new(AtomicBool::new(false));
    let _ = stop;
    eprintln!("probe: calling harness (model from MARVIS_LLM_MODEL or default)...");
    let reply = marvis_harness::run("Reply with exactly: voice path ok", |_| {});
    match reply {
        Ok(t) => println!("REPLY: {t}"),
        Err(e) => {
            eprintln!("ERROR: {e}");
            std::process::exit(1);
        }
    }
}
