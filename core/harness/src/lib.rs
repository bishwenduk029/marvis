//! Marvis harness — the agent brain.
//!
//! Contract (stable, used by `marvis-engine`):
//!   run(user_text, on_activity) -> Result<String, String>
//!
//! Backed by the `pi_agent_rust` SDK in-process (no subprocess), via its
//! stable `pi::sdk` surface: `create_agent_session` + `AgentSessionHandle`,
//! mirroring the SDK's official headless one-shot pattern. Auth flows through
//! the SDK's own provider resolution (provider "openrouter" reads
//! OPENROUTER_API_KEY from the environment).

use std::sync::{Arc, Mutex};

use pi::model::AssistantMessageEvent;
use pi::sdk::{AgentEvent, AgentSessionHandle, ContentBlock, SessionOptions, create_agent_session};

/// Public contract — `on_activity` must be `Send` so it can be parked behind a
/// mutex and shared into the SDK's async callback.
pub fn run(user_text: &str, on_activity: impl FnMut(&str) + Send) -> Result<String, String> {
    let provider = std::env::var("MARVIS_LLM_PROVIDER").unwrap_or_else(|_| "openrouter".into());
    let model = std::env::var("MARVIS_LLM_MODEL").unwrap_or_else(|_| "z-ai/glm-5.3-flash".into());

    // `on_activity` is FnMut owned here; the SDK wants an Fn + Send + Sync +
    // 'static callback, so park it behind Arc<Mutex<_>>.
    let cb: Arc<Mutex<dyn FnMut(&str) + Send>> = Arc::new(Mutex::new(on_activity));

    let reactor = asupersync::runtime::reactor::create_reactor().map_err(|e| e.to_string())?;
    let runtime = asupersync::runtime::RuntimeBuilder::current_thread()
        .with_reactor(reactor)
        .build()
        .map_err(|e| e.to_string())?;

    runtime.block_on(async {
        let mut handle: AgentSessionHandle = create_agent_session(SessionOptions {
            provider: Some(provider),
            model: Some(model),
            no_session: true, // one-shot: no session file persisted
            ..SessionOptions::default()
        })
        .await
        .map_err(|e| e.to_string())?;

        let cb2 = Arc::clone(&cb);
        let assistant = handle
            .prompt(user_text, move |event| match &event {
                AgentEvent::MessageUpdate {
                    assistant_message_event: AssistantMessageEvent::TextDelta { delta, .. },
                    ..
                } => (cb2.lock().unwrap())(delta),
                AgentEvent::ToolExecutionStart { tool_name, .. } => {
                    (cb2.lock().unwrap())(&format!("[tool] {tool_name}"));
                }
                AgentEvent::AgentEnd { error: Some(err), .. } => {
                    (cb2.lock().unwrap())(&format!("[error] {err}"));
                }
                _ => {}
            })
            .await
            .map_err(|e| e.to_string())?;

        Ok(assistant
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text(t) => Some(t.text.clone()),
                _ => None,
            })
            .collect::<String>())
    })
}
