//! Marvis harness — the brain, driven by the jcode agent harness.
//!
//! Instead of talking to an LLM directly, Marvis rides jcode's agent runtime
//! through [`jcode_sdk`]: the user's configured providers, skills and memory
//! all apply, and each daemon run gets a persistent jcode session (so the
//! conversation has history). The jcode runtime is found at its normal
//! harness-API socket and auto-started when missing.
//!
//! Contract: `run(user_text, on_activity) -> Result<String, String>`, plus
//! [`interrupt`] so barge-in can cancel an in-flight turn mid-thinking.
//!
//! Env:
//! - `MARVIS_LLM_MODEL`     jcode model id to switch the session to (ids as
//!                          reported by jcode's model list, e.g. `glm-5.3`).
//!                          Unset = whatever the user's jcode defaults to.
//! - `MARVIS_JCODE_APPROVE` `0`/`false`/`off` to deny tool permission prompts
//!                          instead of auto-allowing them (default: allow).
//! - `MARVIS_JCODE_WORKDIR` working directory for Marvis's jcode session
//!                          (default `~/.local/share/marvis/agent`), holding
//!                          her persona `AGENTS.md`.
//! - `JCODE_API_SOCKET`     jcode's own override for the API socket path.

use jcode_sdk::{
    ApiEvent, ConnectOptions, JcodeClient, PermissionDecision, SessionInfo,
};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// One live connection to the user's jcode runtime plus Marvis's session.
struct Brain {
    client: JcodeClient,
    session: SessionInfo,
}

static BRAIN: Mutex<Option<Arc<Brain>>> = Mutex::new(None);

/// Marvis's persona, written into the session working directory once. jcode
/// reads `AGENTS.md` from the session's cwd, so the user can edit it to
/// reshape her.
const PERSONA: &str = r#"# Marvis

You are Marvis, the user's local voice assistant on their Linux laptop.
Your replies are spoken aloud with a TTS voice and questions arrive from
speech-to-text, so:

- Answer in plain spoken English, one or two sentences unless asked for more.
- No markdown, no lists, no code blocks, no emoji. Spell out symbols.
- If you use a tool or a skill, still end with a short spoken answer.
- If asked who you are: you are Marvis.
"#;

/// The client + session, connecting lazily and reconnecting if the runtime
/// went away. Connection failures are never cached: the next turn retries.
fn brain() -> Result<Arc<Brain>, String> {
    let mut guard = BRAIN.lock().unwrap();
    if let Some(b) = guard.as_ref() {
        if !b.client.is_closed() {
            return Ok(Arc::clone(b));
        }
    }
    *guard = None;

    let mut opts = ConnectOptions::default();
    opts.client_name = "marvis/0.1.0".into();
    opts.request_timeout = Some(Duration::from_secs(120));
    let client = JcodeClient::connect(opts).map_err(|e| format!("jcode runtime: {e}"))?;

    let dir = agent_dir()?;
    let session = client
        .create_session(Some(dir.display().to_string()))
        .map_err(|e| format!("jcode session: {e}"))?;

    if let Ok(model) = std::env::var("MARVIS_LLM_MODEL") {
        let model = model.trim();
        if !model.is_empty() {
            client
                .set_model(&session.session_id, model)
                .map_err(|e| format!("jcode set_model {model:?}: {e}"))?;
        }
    }

    let brain = Arc::new(Brain { client, session });
    *guard = Some(Arc::clone(&brain));
    Ok(brain)
}

/// Working directory for Marvis's jcode sessions; creates it and seeds the
/// persona `AGENTS.md` if absent (never clobbers user edits).
fn agent_dir() -> Result<PathBuf, String> {
    let dir = match std::env::var("MARVIS_JCODE_WORKDIR") {
        Ok(d) if !d.trim().is_empty() => PathBuf::from(d),
        _ => {
            let data = std::env::var("XDG_DATA_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share")
                });
            data.join("marvis/agent")
        }
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("agent dir {}: {e}", dir.display()))?;
    write_persona(&dir);
    Ok(dir)
}

fn write_persona(dir: &Path) {
    let path = dir.join("AGENTS.md");
    if !path.exists() {
        let _ = std::fs::write(path, PERSONA);
    }
}

fn auto_approve() -> bool {
    !matches!(
        std::env::var("MARVIS_JCODE_APPROVE").as_deref(),
        Ok("0") | Ok("false") | Ok("off")
    )
}

/// One activity line for the UI, or None if the event isn't user-visible.
fn activity_line(ev: &ApiEvent) -> Option<String> {
    match ev {
        // jcode's stable display vocabulary: "connecting", "waiting for
        // response", "streaming", "retrying (2/4)" ...
        ApiEvent::ConnectionPhase { phase, .. } => Some(phase.clone()),
        ApiEvent::SessionStatus { status, .. } if status != "idle" => Some(status.clone()),
        ApiEvent::ToolStart { name, .. } => Some(format!("using {name}")),
        ApiEvent::ToolDone {
            name,
            error: Some(_),
            ..
        } => Some(format!("{name} failed")),
        ApiEvent::PermissionRequest { tool_name, .. } => {
            Some(format!("asking to use {tool_name}"))
        }
        _ => None,
    }
}

/// Run one conversational turn through jcode. Blocks until the turn ends.
pub fn run(user_text: &str, mut on_activity: impl FnMut(&str)) -> Result<String, String> {
    let brain = brain()?;
    let auto = auto_approve();
    let session_id = brain.session.session_id.clone();

    let stream = brain.client.events(Some(&session_id));
    brain
        .client
        .send_message(&session_id, user_text, Vec::new(), None)
        .map_err(|e| format!("jcode send: {e}"))?;

    let mut text = String::new();
    let last_activity = RefCell::new(String::new());
    loop {
        let Some(ev) = stream.next() else {
            return Err("jcode connection closed mid-turn".into());
        };
        if let Some(line) = activity_line(&ev) {
            if *last_activity.borrow() != line {
                last_activity.borrow_mut().clone_from(&line);
                on_activity(&line);
            }
        }
        match ev {
            ApiEvent::TextDelta { text: delta, .. } => text.push_str(&delta),
            ApiEvent::PermissionRequest {
                request_id, tool_name, ..
            } => {
                let decision = if auto {
                    PermissionDecision::Allow
                } else {
                    on_activity(&format!("denied {tool_name}"));
                    PermissionDecision::Deny
                };
                brain
                    .client
                    .respond_to_permission(&session_id, &request_id, decision)
                    .map_err(|e| format!("jcode permission: {e}"))?;
            }
            ApiEvent::TurnDone { .. } => {
                let reply = text.trim().to_string();
                return if reply.is_empty() {
                    Err("empty reply".into())
                } else {
                    Ok(reply)
                };
            }
            ApiEvent::Error { message, .. } => return Err(format!("jcode: {message}")),
            _ => {}
        }
    }
}

/// Cancel an in-flight turn (barge-in while thinking). Cheap no-op if idle.
pub fn interrupt() {
    if let Some(brain) = BRAIN.lock().unwrap().as_ref() {
        let _ = brain.client.cancel(&brain.session.session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persona_is_seeded_without_clobbering() {
        let dir = std::env::temp_dir().join(format!("marvis-agent-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write_persona(&dir);
        let first = std::fs::read_to_string(dir.join("AGENTS.md")).unwrap();
        std::fs::write(dir.join("AGENTS.md"), "user edited").unwrap();
        write_persona(&dir);
        let second = std::fs::read_to_string(dir.join("AGENTS.md")).unwrap();
        assert_eq!(first, PERSONA);
        assert_eq!(second, "user edited");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn approval_env_is_opt_out() {
        std::env::set_var("MARVIS_JCODE_APPROVE", "0");
        assert!(!auto_approve());
        std::env::set_var("MARVIS_JCODE_APPROVE", "1");
        assert!(auto_approve());
        std::env::remove_var("MARVIS_JCODE_APPROVE");
    }

    #[test]
    fn activities_map_to_short_lines() {
        let ev = ApiEvent::ToolStart {
            session_id: "s".into(),
            call_id: "c".into(),
            name: "bash".into(),
        };
        assert_eq!(activity_line(&ev).as_deref(), Some("using bash"));
        let ev = ApiEvent::ConnectionPhase {
            session_id: "s".into(),
            phase: "waiting for response".into(),
        };
        assert_eq!(activity_line(&ev).as_deref(), Some("waiting for response"));
    }

    /// Real turn against the user's jcode runtime: `cargo test -p
    /// marvis-harness -- --ignored`.
    #[test]
    #[ignore = "needs a live jcode runtime"]
    fn live_turn() {
        let reply = run("Reply with exactly: hello", |_| {}).expect("live turn");
        assert!(reply.to_lowercase().contains("hello"), "got: {reply}");
    }
}
