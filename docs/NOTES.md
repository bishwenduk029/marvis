# Marvis — note to self: what happened, what it cost, what to remember

The honest build log. Written after the first working end-to-end voice loop
(click → moonshine → Liquid LFM → vits → speaker) on a 2017 laptop with 3.3 GB
RAM. Every entry here was paid for in real debugging time.

## The journey of the "brain" (the biggest lesson)

1. **zerostack** (Rust coding agent, 16 MB RAM, excellent) — dropped: no SDK,
   subprocess-spawn per query felt heavy, and the user wanted an SDK.
2. **maki** (Rust coding agent, Lua plugins, headless stream-json) — dropped:
   same subprocess model; binary is 101 MB.
3. **pi_agent_rust SDK** (in-process agent, real `pi::sdk`) — fought it for
   5 CI cycles, then dropped. See "why it failed" below.
4. **`genai` non-streaming chat** — landed. ~80 lines, stable Rust, compiles in
   seconds, model swap via env var. CI went 8 min → 1m11s.

> Lesson: **an "agent harness" is a cost you pay every build.** For a voice
> shell, a plain OpenAI-compatible chat call is the product; the agent layer
> is an opt-in later. Don't pick the heavy SDK because it's impressive.

### Why pi_agent_rust failed (so I don't try again blindly)

- Its `fsqlite` dep uses `#![feature(core_intrinsics)]` → **nightly-only at
  every published version** (verified 0.3.5 AND 0.3.12). The README's
  `rust-version = 1.95` is aspirational; their own CI pins
  `nightly-2026-07-05`, and users normally get prebuilt binaries.
- `default-features = false` is **broken in the published crate**:
  `memory.rs`/`session_index.rs` import `session_sqlite` (gated behind
  `sqlite-sessions`) unconditionally → E0432.
- Dep tree: swc + gix + rquickjs + fsqlite ≈ 2 GB of artifacts, 8 min CI.
  Filled this machine's 1.7 GB tmpfs and hung the box.
- genai quirk worth remembering: its OpenRouter adapter wants
  **`OPEN_ROUTER_API_KEY`** (underscored), not `OPENROUTER_API_KEY`; the
  daemon mirrors one to the other at startup.

## Model choices — measured, not hoped (this CPU: AMD E2-7110, 4 slow cores)

| Task | First pick | Reality | Final pick | Measured |
|---|---|---|---|---|
| TTS | Kokoro en v0.19 (345 MB) | warm RTF **10.1** (28.8 s for 3 s audio) | **vits-piper amy-medium** (60 MB) | warm RTF **0.80** |
| STT | SenseVoice fp32 (234 MB) | never finished 5 s of audio (>10 min) | **moonshine tiny int8** (~120 MB, 4 files) | warm RTF **0.33** |
| LLM | local 7B | impossible in 3.3 GB | remote via OpenRouter; `liquid/lfm-2.5-2.6b:free` | ~1 s first token, free |
| VAD | silero (628 KB) | fine | kept | threshold 0.5, 1.2 s trailing silence |

- Z.ai GLM 5.3-flash is **forced-reasoning** on OpenRouter (cannot disable) —
  returns reasoning with `content: null`. Wrong shape for voice. Any model
  that "thinks" before speaking adds dead air.
- Peak RSS with STT+TTS models warm: **~360 MB**. Fits. LLM stays remote for
  this reason.

## Capture: the cpal war story

- cpal (ALSA host) opens ALSA `default` → on this machine that route delivers
  **non-normalized garbage** (samples ±2.9! energy 14.5 when 1.0 is max).
  Same garbage via the `pipewire` ALSA device. Both routes broken, both
  silent — no error, just wrong samples → VAD never fires.
- `parec`/`paplay` (PipeWire native) measure **clean** (−34 dBFS floor).
  Final architecture: spawn `parec` (s16le mono 16k) per turn for capture,
  `paplay --raw` for playback, kill the child for instant interrupt.
  **cpal is gone from the project entirely.**
- If audio is "pegged at max" again: check `Internal Mic Boost` first.
  PipeWire's session manager **restored +24 dB boost** after I set 0 dB —
  mic clipped at full scale, VAD saw a wall of fake speech.
  `amixer -c 1 sset 'Internal Mic Boost' 0dB` (needs re-checking after
  PipeWire restores state; `alsactl store` needs sudo).

## Daemon design lessons

- **Broadcast must never block.** v1 wrote to every client under a mutex; a
  dead client's socket buffer filled and froze the entire pipeline (say
  worked, start hung forever). Fix: per-client writer threads + bounded
  queues + `try_send`; slow/dead clients are dropped, events are lossy.
- **One turn at a time** via an `AtomicBool running`; a `start` while running
  is *barge-in*: set stop, wait ≤800 ms for the turn to wind down, start
  listening. Clicking while she speaks cuts her off and takes the floor —
  that's the Siri gesture.
- Lazy-load models once (`OnceLock` + get/set; **`get_or_try_init` is still
  unstable** — E0658 on stable), then `warmup()` at daemon boot so the first
  turn isn't 25 s of model loading.
- eprintln telemetry in the capture path saved the day; add it before you
  need it. (But: `pkill -f marvis-daemon` from a bash tool **matches the
  command line of the shell issuing it** and self-kills mid-script — use
  `pkill -x`.)

## UI (Omarchy/Quickshell plugin) lessons

- omapilot (MIT) donated the view layer: VoiceWave/VoiceNode/StateColor —
  theme inheritance (`Color.accent`) comes free. Keep the attribution.
- A QML **singleton with a duplicate method name** fails to compile → every
  component depending on it becomes "unavailable" → the bar widget silently
  doesn't load → and combined with hot-reload churn, quickshell can crash
  (took the user's workspace dark twice). Symptom trace ends in
  `Plugin widget ... failed: Type X unavailable`.
- The bar icon is the only clickable surface; the ambient wave is
  deliberately click-through.
- Quickshell's Socket gets a stale `connected=true` after a refused connect —
  always force `connected=false` then reconnect via a timer that never stops.
- Deploy by **atomic copy** (cp to temp, mv into place), never cp -r directly
  into the watched plugin dir (reload races), and symlinks are rejected by
  `omarchy plugin validate`.
- Console.log from plugins doesn't reach the journal reliably — the qslog
  under `/run/user/<uid>/quickshell/by-id/<id>/log.qslog` and stderr of a
  manually launched `quickshell -n -p ...` are the real sources.

## Workflow lessons

- **This box can't build heavy trees.** 3.3 GB RAM + tmpfs /tmp (1.7 GB) +
  flaky network. GH Actions (public repo = free) is the build farm: CI went
  from impossible → 1m11s after the harness swap. Local builds: only after
  deps are light, and never two cargo runs at once.
- /tmp is tmpfs — a "disk full" there is a RAM leak. Cleaned 1.2–1.5 GB of
  cargo temp dirs twice.
- crates.io API needs a User-Agent header. GitHub job logs API needs admin
  (use `gh auth login` + `gh run view --log-failed`). SSH key: `ssh-keygen
  -t ed25519`, add at github.com/settings/keys.
- `git add -A` after moving directories = disaster if .gitignore has
  root-anchored paths; 1.5 GB of target/ got committed once. Match
  `target/` and `gen/` anywhere. Fix: history rewrite (orphan branch) —
  .git went 1.5 GB → 240 KB.
- CI (ubuntu-latest) needs `libasound2-dev` for anything touching ALSA.
- QML `.js` files start with `.pragma library` — strip before `node --check`,
  and give mktemp `--suffix=.js`.

## Where it stands (2026-08-30)

- Working: click → talk → transcript → Liquid reply → spoken answer; barge-in;
  plugin connected; CI green; artifacts published.
- Known rough edges: ~6 s think-latency (sentence-streamed TTS will halve the
  perceived wait); mic boost may revert on PipeWire state restore; desktop
  (Tauri) app is deferred, not wired to the daemon yet.
- Next, in value order: sentence-chunked TTS → local LLM via llama.cpp
  (LFM 2.5 2.6B GGUF would make it fully offline) → JARVIS tools (hypr/agent/
  open) as an opt-in harness → packaging (systemd user service + installer).
