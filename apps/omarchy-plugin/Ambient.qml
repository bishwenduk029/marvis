import QtQuick
import Quickshell
import Quickshell.Hyprland
import "components" as Marvis

// Marvis ambient overlay root.
//
// This is the plugin's `overlay` entry point; the manifest sets `keepLoaded`,
// so Omarchy keeps it live from shell start. It owns the voice node and the
// answer curtain, and derives their presentation state from MarvisStore, which
// mirrors the Marvis daemon over its Unix socket.
//
// The node itself is deliberately inert (click-through, no keyboard focus —
// voice must never steal the caret), so toggling lives on the bar widget's
// button and on this root's toggle()/open()/close() host contract.
Item {
  id: root

  // Injected by the host's overlay loader.
  property var shell: null
  property var manifest: null
  property string omarchyPath: ""
  property var pluginRegistry: null
  property var barWidgetRegistry: null

  property bool motionEnabled: true

  // ------------------------------------------------------------- placement
  // The output Hyprland has focused — where the user actually is.
  readonly property string focusedScreenName:
    Hyprland.focusedMonitor ? String(Hyprland.focusedMonitor.name || "") : ""
  readonly property var activeScreen: {
    var screens = Quickshell.screens || []
    for (var i = 0; i < screens.length; i++)
      if (String(screens[i].name || "") === root.focusedScreenName) return screens[i]
    return screens.length > 0 ? screens[0] : null
  }

  // ---------------------------------------------------------------- phase
  // idle -> dormant, listening -> listening, thinking -> thinking,
  // speaking -> speaking.
  readonly property string phase: {
    switch (Marvis.MarvisStore.state) {
      case "listening": return "listening"
      case "thinking": return "thinking"
      case "speaking": return "speaking"
      default: return "dormant"
    }
  }

  readonly property bool curtainShown:
    Marvis.MarvisStore.reply !== "" && Marvis.MarvisStore.state !== "idle"

  // --------------------------------------------------------------- control
  function toggle() { Marvis.MarvisStore.toggle() }

  // Host contract: shell.summon(id, payload) -> open(), shell.hide(id) -> close().
  function open() { toggle() }
  function close() {
    Marvis.MarvisStore.interrupt()
    Marvis.MarvisStore.reply = ""
  }

  // --------------------------------------------------------------- surfaces
  Marvis.VoiceNode {
    phase: root.phase
    transcript: Marvis.MarvisStore.transcript
    status: Marvis.MarvisStore.activity
    speaking: root.phase === "speaking"
    // The daemon measures its own output envelope; once it says "speaking",
    // the node follows the reported energy instead of the decorative fallback.
    playbackMetered: Marvis.MarvisStore.state === "speaking"
    playbackLevel: Marvis.MarvisStore.energy
    level: Marvis.MarvisStore.energy
    targetScreen: root.activeScreen
    motionEnabled: root.motionEnabled
  }

  Marvis.AnswerCurtain {
    shown: root.curtainShown
    question: Marvis.MarvisStore.transcript
    markdown: Marvis.MarvisStore.reply
    provenance: Marvis.MarvisStore.activity !== "" ? Marvis.MarvisStore.activity : "Marvis"
    targetScreen: root.activeScreen
    motionEnabled: root.motionEnabled
    onLinkActivated: function(url) {
      if (root.shell && typeof root.shell.run === "function")
        root.shell.run(["xdg-open", url])
    }
  }

  // A new question invalidates the previous reply.
  onPhaseChanged: {
    if (phase === "listening" || phase === "thinking") Marvis.MarvisStore.reply = ""
  }
}
