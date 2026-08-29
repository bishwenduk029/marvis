# Third-party notices

## omarchy-omapilot (view layer)

This plugin's visual layer is adapted from
[omarchy-omapilot](https://github.com/spencerbull/omarchy-omapilot)
by Spencer Bull, licensed under the MIT License.

The following files are copied from omarchy-omapilot, with only the changes
described below:

- `components/VoiceNode.qml`, `components/VoiceWave.qml`,
  `components/ThinkingScanner.qml`, `components/StateLightBar.qml`,
  `components/ResponseActivityBorder.qml`, `components/ActivityFilament.qml`
  — copied without functional changes.
- `components/StateColor.js`, `components/StatePhrases.js`
  — copied without changes.
- `components/AnswerCurtain.qml`
  — copied; the `MarkdownView` child (which depends on omapilot's
  `Protocol.js` runtime) was replaced with a plain wrapped `Text`, because the
  Marvis daemon delivers replies as plain strings.
- `BarWidget.qml`
  — adapted from omapilot's bar widget; all panel/composer wiring was removed,
  leaving a toggle button plus the state light.

omarchy-omapilot's MIT License text:

```
MIT License

Copyright (c) 2025 Spencer Bull

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
