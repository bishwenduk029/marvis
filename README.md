# marvis

A Siri-like local AI voice agent for Linux. You speak, it listens, thinks, and talks back — all on-device except the LLM, which runs remotely by default.

## Architecture

```
 mic ──> sherpa-onnx STT ──> zerostack (coding agent)
                               │  remote LLM provider (default: openrouter)
                               │  local llama.cpp — planned
                               v
 speaker <── sherpa-onnx Kokoro TTS

 Tauri canvas orb UI wraps the whole loop
```

## Current state

Works now:
- Voice pipeline skeleton (mic capture -> STT -> response -> TTS -> speaker)
- Tauri canvas orb UI
- STT + Kokoro TTS models (sherpa-onnx)
- zerostack driver (remote LLM provider, default openrouter)

Deferred:
- Local llama.cpp model serving
- Hyprland / herdr / browser tools

## Run

```sh
./scripts/fetch-models.sh   # models land in ~/.local/share/marvis/models
cargo build && cargo run    # in src-tauri/
```

Env vars:
- `MARVIS_LLM_PROVIDER` — LLM provider (default: openrouter)
- `MARVIS_LLM_MODEL` — model id
- `OPENROUTER_API_KEY` — API key for openrouter

## Memory discipline

Target machine has 3.3GB RAM. Keep idle CPU at ~0 and load STT/TTS models on demand — never hold them resident when the pipeline is idle.
