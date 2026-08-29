//! Marvis harness — the brain, via the `genai` multi-provider client.
//!
//! Non-streaming on purpose: the engine speaks the full reply through Kokoro,
//! so deltas buy nothing yet. When sentence-chunked TTS lands, this switches
//! to `exec_chat_stream` and `on_activity` starts carrying text deltas.
//!
//! Contract: `run(user_text, on_activity) -> Result<String, String>`.
//! Default model is a fast non-reasoning chat model via OpenRouter; override
//! with `MARVIS_LLM_MODEL` (genai syntax: `open_router::<provider/model>`).

use genai::chat::{ChatMessage, ChatRequest};

const SYSTEM: &str = "You are Marvis, a concise voice assistant. Answer in plain spoken \
                      English, one or two sentences unless asked for more.";

pub fn run(user_text: &str, _on_activity: impl FnMut(&str)) -> Result<String, String> {
    let model = std::env::var("MARVIS_LLM_MODEL")
        .unwrap_or_else(|_| "open_router::google/gemini-2.0-flash-001".into());

    let client = genai::Client::builder().build();
    let chat_req = ChatRequest::new(vec![
        ChatMessage::system(SYSTEM),
        ChatMessage::user(user_text),
    ]);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("async runtime: {e}"))?;

    let res = rt.block_on(client.exec_chat(&model, chat_req, None))
        .map_err(|e| e.to_string())?;
    res.first_text()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "empty reply".to_string())
}
