# Marvis — agent guide

Local AI voice agent (JARVIS for Linux): sherpa-onnx STT/TTS + the jcode
agent harness (via the `jcode-sdk` Rust crate) for thinking, surfaced as an
Omarchy/Quickshell plugin and (later) a Tauri desktop app. STT/TTS and the
voice pipeline are on-device; thinking rides the user's jcode runtime, so
their configured providers, skills and memory all apply.

## Layout

- `core/common` — shared types (`Event` etc.)
- `core/stt` — sherpa-onnx SenseVoice transcription
- `core/tts` — sherpa-onnx Kokoro synthesis
- `core/harness` — the brain: connects to the jcode agent runtime and drives
  one jcode session (providers, skills, memory all come from jcode)
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

- client → daemon: `{"cmd":"start"|"interrupt"|"say"|"ping"|"quit"}`
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

## Env vars

- `MARVIS_MODELS` — model dir for sherpa-onnx
- `MARVIS_LLM_MODEL` — jcode model id for the session (unset = jcode default),
  e.g. `liquid/lfm-2.5-2.6b:free` (genai's `open_router::` prefix is gone;
  run `jcode model list` for valid ids)
- `MARVIS_JCODE_APPROVE` — `0`/`false`/`off` to deny jcode tool-permission
  prompts instead of auto-allowing (default: allow)
- `MARVIS_JCODE_WORKDIR` — working dir for Marvis's jcode sessions (default
  `~/.local/share/marvis/agent`, holds her persona `AGENTS.md`)
- `JCODE_API_SOCKET` — jcode's own socket override (passthrough)

Defaults live in `core/harness/src/lib.rs` — don't cache them here.

## Conventions

- Workspace crates are named `marvis-*`; edition 2021; single `lib.rs` per
  core crate.
- UIs render only from daemon events; state colour/mapping belongs in QML
  (`StateColor.js`, `MarvisStore.qml`), never in Rust.
