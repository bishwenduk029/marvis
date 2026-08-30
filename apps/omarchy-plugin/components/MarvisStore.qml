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

  // The daemon listens on $XDG_RUNTIME_DIR/marvis.sock (i.e. /run/user/<uid>).
  // No /tmp fallback: a wrong path would stick forever, while retrying the
  // right one recovers as soon as the daemon is up.
  readonly property string socketPath:
    (Quickshell.env("XDG_RUNTIME_DIR") || "/run/user/1000") + "/marvis.sock"

  // Liveness: a daemon restart can leave this Socket holding a zombie
  // connection it still believes is up (writes vanish, no error fires), and
  // then the retry timer never runs because `connected` reads true. So ping
  // every tick and treat a stale pong as a dead link worth reconnecting.
  property real lastPong: 0

  function send(json) {
    console.log("[marvis] send:", json, "connected:", socket.connected)
    if (socket.connected) socket.write(json + "\n")
  }
  function start() { send('{"cmd":"start"}') }
  function interrupt() { send('{"cmd":"interrupt"}') }
  function ping() { send('{"cmd":"ping"}') }
  function say(text) { send(JSON.stringify({ cmd: "say", value: text })) }
  // Spoken once, on the first successful link to the daemon.
  property bool greeted: false
  function greet() {
    if (root.greeted) return
    root.greeted = true
    say("Marvis online.")
  }
  // The one gesture: idle starts; while she is talking, cut her off and take
  // the floor (barge-in); mid-thought/listening, interrupt. A click while the
  // link is down first recovers the connection, then runs the gesture as soon
  // as the socket is back — a click must never be a silent no-op.
  property bool pendingGesture: false
  function toggle() {
    if (!connected) {
      pendingGesture = true
      reconnect()
      return
    }
    gesture()
  }
  function gesture() {
    if (state === "idle") start()
    else if (state === "speaking") {
      interrupt()
      start()
    } else interrupt()
  }

  // After an error or disconnect, quickshell's Socket ignores a plain
  // connected = true (its requested state never changed), so always drop the
  // link first, then reconnect on the next event-loop pass.
  function reconnect() {
    socket.connected = false
    Qt.callLater(function() { socket.connected = true })
  }

  function handleLine(line) {
    console.log("[marvis] recv:", String(line).substring(0, 80))
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
      case "pong":
        root.lastPong = Date.now()
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
        root.lastPong = Date.now()
        root.ping()
        root.greet()
        retryTimer.stop()
        if (root.pendingGesture) {
          root.pendingGesture = false
          root.gesture()
        }
      } else {
        retryTimer.restart()
      }
    }
    onError: {
      // Daemon not up yet (or busy warming models) — keep retrying.
      retryTimer.restart()
    }
  }

  // The reconnect function (defined above) forces the link down and retries;
  // this timer always ticks so any failure mode still recovers. While the
  // link looks up it doubles as a liveness check: ping, and if no pong came
  // back within ~9s the link is a zombie — force a reconnect.
  Timer {
    id: retryTimer
    interval: 3000
    repeat: true
    running: true
    onTriggered: {
      if (!socket.connected) {
        root.reconnect()
        return
      }
      root.ping()
      if (root.lastPong > 0 && Date.now() - root.lastPong > 9000) {
        console.log("[marvis] link is stale, reconnecting")
        root.reconnect()
      }
    }
  }

  Component.onCompleted: socket.connected = true
}
