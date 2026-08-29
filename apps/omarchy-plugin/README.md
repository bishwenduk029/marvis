# Marvis — Omarchy plugin

Marvis voice assistant as a native Omarchy/Quickshell surface. The visual layer
(the ambient light node, voice wave, scanner, and answer curtain) is adapted
from [omarchy-omapilot](https://github.com/spencerbull/omarchy-omapilot) (MIT);
state comes from the Marvis daemon over a Unix socket instead of omapilot's
TypeScript runtime. See `THIRD_PARTY_NOTICES.md`.

## What you get

- **Ambient overlay** (`Ambient.qml`): a light node bleeding up from the bottom
  edge of the focused output. Colour and motion follow the daemon state —
  listening (theme accent), thinking (rotated hue + scanner), speaking (wave
  follows the daemon's energy level). The live transcript shows in the caption;
  the reply slides down as a curtain until the session returns to idle.
- **Bar widget** (`BarWidget.qml`): one icon in the right section. Click to
  start a session when idle, click again to interrupt. A small state light
  mirrors the ambient node.

The voice node is deliberately click-through and never takes keyboard focus, so
talking to Marvis never steals your caret.

## Socket protocol

JSON lines over a Unix socket. The plugin connects to
`$XDG_RUNTIME_DIR/marvis.sock` (i.e. `/run/user/<uid>/marvis.sock`), falling
back to `/tmp/marvis.sock`, and reconnects every 3 s while the daemon is down.

Client → daemon:

| Command | Meaning |
|---|---|
| `{"cmd":"start"}` | begin a voice session |
| `{"cmd":"interrupt"}` | interrupt the active session |
| `{"cmd":"ping"}` | liveness check (sent on connect) |

Daemon → client (one JSON object per line):

| Event | Value |
|---|---|
| `{"event":"state","value":"idle\|listening\|thinking\|speaking"}` | session state |
| `{"event":"energy","value":0.0}` | output level, 0.0–1.0 |
| `{"event":"transcript","value":"..."}` | live transcript |
| `{"event":"reply","value":"..."}` | final answer text |
| `{"event":"activity","value":"..."}` | status line shown under the caption |

State mapping: `idle` → node dormant, `listening` → listening, `thinking` →
thinking, `speaking` → speaking (energy drives the wave). Colours derive from
the Omarchy theme accent, so the plugin inherits your theme.

## Install

```sh
# from this directory
ln -s "$PWD" ~/.config/omarchy/plugins/dev.marvis.app
# or: scripts/install.sh

omarchy plugin validate dev.marvis.app
omarchy shell restart   # or log out/in
```

Then enable the **Marvis** bar widget in the bar settings (right section) and
start the Marvis daemon. The overlay loads automatically with the shell.
