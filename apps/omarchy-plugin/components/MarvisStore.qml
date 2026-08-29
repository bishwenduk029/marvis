pragma Singleton
import QtQuick
import Quickshell
import Quickshell.Io

// Marvis store: the socket-driven replacement for omapilot's TypeScript
// runtime. Owns the one connection to the Marvis daemon (JSON lines over a
// Unix socket) and mirrors its events as plain properties.
//
//   client -> daemon: {"cmd":"start"} | {"cmd":"interrupt"} | {"cmd":"ping"}
//   daemon -> client: {"event":"state"|"energy"|"transcript"|"reply"|"activity","value":...}
Scope {
  id: root

  // Mirrored daemon state.
  property string state: "idle"          // idle | listening | thinking | speaking
  property real energy: 0                // 0.0 - 1.0
  property string transcript: ""
  property string reply: ""
  property string activity: ""
  readonly property bool connected: socket.connected

  // The daemon listens on $XDG_RUNTIME_DIR/marvis.sock (i.e. /run/user/<uid>)
  // with a /tmp fallback for odd setups.
  readonly property string primaryPath:
    (Quickshell.env("XDG_RUNTIME_DIR") || "/run/user/1000") + "/marvis.sock"
  readonly property string fallbackPath: "/tmp/marvis.sock"
  property string socketPath: primaryPath

  function send(json) {
    if (socket.connected) socket.write(json + "\n")
  }
  function start() { send('{"cmd":"start"}') }
  function interrupt() { send('{"cmd":"interrupt"}') }
  function ping() { send('{"cmd":"ping"}') }
  // The one gesture: idle starts a session, anything active interrupts it.
  function toggle() {
    if (state === "idle") start()
    else interrupt()
  }

  // After an error or disconnect, quickshell's Socket ignores a plain
  // connected = true (its requested state never changed), so always drop the
  // link first, then reconnect on the next event-loop pass.
  function reconnect() {
    socket.connected = false
    Qt.callLater(function() { socket.connected = true })
  }

  function handleLine(line) {
    var message
    try { message = JSON.parse(String(line)) } catch (e) { return }
    if (!message || typeof message !== "object") return
    switch (String(message.event || "")) {
      case "state":
        root.state = String(message.value || "idle")
        if (root.state === "idle") root.transcript = ""
        break
      case "energy":
        var level = Number(message.value)
        root.energy = isFinite(level) ? Math.max(0, Math.min(1, level)) : 0
        break
      case "transcript":
        root.transcript = String(message.value || "")
        break
      case "reply":
        root.reply = String(message.value || "")
        break
      case "activity":
        root.activity = String(message.value || "")
        break
    }
  }

  Socket {
    id: socket
    path: root.socketPath
    parser: SplitParser {
      onRead: function(data) { root.handleLine(data) }
    }
    onConnectedChanged: {
      if (connected) {
        root.ping()
        retryTimer.stop()
      } else {
        retryTimer.restart()
      }
    }
    onError: {
      // Primary socket missing -> try /tmp once, then keep retrying either way.
      if (root.socketPath !== root.fallbackPath)
        root.socketPath = root.fallbackPath
      retryTimer.restart()
    }
  }

  // The daemon may start long after the shell does. connected flips false on
  // any disconnect/error, so this is the single reconnect path.
  Timer {
    id: retryTimer
    interval: 3000
    repeat: true
    running: !socket.connected
    onTriggered: root.reconnect()
  }

  Component.onCompleted: socket.connected = true
}
