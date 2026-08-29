import QtQuick
import qs.Commons
import qs.Ui
import "components" as Marvis

// Marvis bar widget: one icon plus the shared state light. Clicking toggles a
// session — start when idle, interrupt when the daemon is active — mirroring
// the gesture contract of the ambient overlay.
//
// Adapted from omarchy-omapilot's BarWidget (MIT): everything panel- and
// composer-related was removed; this widget is only the toggle and the state
// readout, because all conversation surfaces live in Ambient.qml.
BarWidget {
  id: root
  moduleName: "dev.marvis.app"

  readonly property string phase: {
    switch (Marvis.MarvisStore.state) {
      case "listening": return "listening"
      case "thinking": return "thinking"
      case "speaking": return "answering"
      default: return "dormant"
    }
  }
  readonly property bool busy: Marvis.MarvisStore.state !== "idle"

  function open() { Marvis.MarvisStore.start() }
  function close() { Marvis.MarvisStore.interrupt() }
  function togglePanel() { Marvis.MarvisStore.toggle() }
  function toggle() { Marvis.MarvisStore.toggle() }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: root.busy ? "󰓛" : "󰓭"
    active: root.busy
    tooltipText: root.busy ? "Marvis (" + Marvis.MarvisStore.state + ") — click to interrupt"
                           : "Marvis — click to talk"
    Accessible.name: tooltipText
    onPressed: function(b) {
      if (b === Qt.RightButton) Marvis.MarvisStore.interrupt()
      else Marvis.MarvisStore.toggle()
    }
  }

  // The same state vocabulary as the ambient node, condensed into the bar slot.
  Marvis.StateLightBar {
    anchors {
      bottom: parent.bottom
      left: parent.left
      right: parent.right
      leftMargin: Style.space(4)
      rightMargin: Style.space(4)
    }
    phase: root.phase
    visible: root.phase !== "dormant"
  }
}
