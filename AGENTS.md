# Marvis — agent guide

Local AI voice agent (JARVIS for Linux): sherpa-onnx STT/TTS + a
`pi_agent_rust`-style harness (zerostack) for thinking, surfaced as an
Omarchy/Quickshell plugin and (later) a Tauri desktop app. All on-device
except the LLM (remote by default).

## Layout

- `core/common` — shared types (`Event` etc.)
- `core/stt` — sherpa-onnx SenseVoice transcription
- `core/tts` — sherpa-onnx Kokoro synthesis
- `core/harness` — LLM provider glue (openrouter default)
- `core/engine` — the voice pipeline: listen → transcribe → think → speak
- `core/daemon` — `marvis-daemon`: Unix-socket server wrapping the engine
- `apps/omarchy-plugin` — QML UI (ambient overlay + bar widget); protocol
  reference: `apps/omarchy-plugin/README.md`
- `apps/desktop` — Tauri app (src-tauri + static ui/)
- `apps/web` — placeholder, empty
- `scripts/fetch-models.sh` — downloads the ONNX models

## Architecture: the daemon is the product

`core/daemon/src/main.rs` is the authority. `marvis-daemon` listens on
`$XDG_RUNTIME_DIR/marvis.sock` (fallback `/tmp/marvis.sock`) and wraps
`core/engine`. **All UIs are thin clients over this socket — the events are
the only UI contract.** Never add a second channel between UI and engine.

Protocol: JSON lines, one object per line.

- client → daemon: `{"cmd":"start"|"interrupt"|"ping"|"quit"}`
- daemon → client: `{"event":"state"|"energy"|"transcript"|"reply"|"activity","value":...}`

`energy` is a number (0.0–1.0, drives the voice wave); everything else is a
string. `state` cycles idle|listening|thinking|speaking.

## Build / run

```sh
cargo check --workspace          # also what CI runs (.github/workflows/ci.yml)
cargo test --workspace
cargo build -p marvis-daemon
scripts/fetch-models.sh          # before first run
```

## Env vars

`MARVIS_MODELS` (model dir), `MARVIS_LLM_PROVIDER` (default openrouter),
`MARVIS_LLM_MODEL`, `OPENROUTER_API_KEY`. Defaults live in
`core/daemon/src/main.rs` — don't cache them here.

## Conventions

- Workspace crates are named `marvis-*`; edition 2021; single `lib.rs` per
  core crate.
- UIs render only from daemon events; state colour/mapping belongs in QML
  (`StateColor.js`, `MarvisStore.qml`), never in Rust.
