//! Marvis harness — the brain, kept deliberately lean: a streaming
//! OpenAI-compatible chat call (OpenRouter by default, any local server via
//! env). No agent loop, no tools — those come later if/when needed.
//!
//! Contract: `run(user_text, on_activity) -> Result<String, String>` where
//! `on_activity` receives text deltas as they stream in.

use std::io::{BufRead, BufReader};

pub fn run(user_text: &str, mut on_activity: impl FnMut(&str)) -> Result<String, String> {
    let base = std::env::var("MARVIS_LLM_BASE")
        .unwrap_or_else(|_| "https://openrouter.ai/api/v1".into());
    let key = std::env::var("MARVIS_LLM_KEY")
        .or_else(|_| std::env::var("OPENROUTER_API_KEY"))
        .map_err(|_| "no API key (set MARVIS_LLM_KEY or OPENROUTER_API_KEY)".to_string())?;
    let model = std::env::var("MARVIS_LLM_MODEL")
        .unwrap_or_else(|_| "z-ai/glm-5.3-flash".into());

    let body = serde_json::json!({
        "model": model,
        "messages": [
            {
                "role": "system",
                "content": "You are Marvis, a concise voice assistant. Answer in plain spoken \
                            English, one or two sentences unless asked for more."
            },
            { "role": "user", "content": user_text }
        ],
        "stream": true
    });

    let url = format!("{}/chat/completions", base.trim_end_matches('/'));
    let response = ureq::post(&url)
        .set("Authorization", &format!("Bearer {key}"))
        .set("Content-Type", "application/json")
        .send_json(body)
        .map_err(|e| format!("request failed: {e}"))?;

    if response.status() != 200 {
        return Err(format!("llm returned HTTP {}", response.status()));
    }

    let mut full = String::new();
    let mut line = String::new();
    let mut reader = BufReader::new(response.into_reader());
    loop {
        line.clear();
        if reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            break;
        }
        let data = match line.trim().strip_prefix("data:") {
            Some(d) => d.trim(),
            None => continue,
        };
        if data == "[DONE]" {
            break;
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(data) {
            if let Some(delta) = value["choices"][0]["delta"]["content"].as_str() {
                full.push_str(delta);
                on_activity(delta);
            }
        }
    }

    if full.trim().is_empty() {
        return Err("empty reply".into());
    }
    Ok(full.trim().to_string())
}
