#!/usr/bin/env bash
# Fetch the three ONNX models Marvis needs. Small (VAD) + SenseVoice (STT) +
# Kokoro (TTS). Re-run any time to update.
set -euo pipefail

DIR="${MARVIS_MODELS:-${XDG_DATA_HOME:-$HOME/.local/share}/marvis/models}"
mkdir -p "$DIR"
cd "$DIR"

BASE=https://github.com/k2-fsa/sherpa-onnx/releases/download

fetch() { # url out
  if [ -e "$2" ]; then echo "have $2"; else
    echo "fetching $2 ..."; curl -fL --retry 3 -o "$2.tmp" "$1" && mv "$2.tmp" "$2"
  fi
}

fetch "$BASE/asr-models/silero_vad.onnx" silero_vad.onnx

if [ ! -d sense-voice ]; then
  echo "fetching SenseVoice ..."
  curl -fL --retry 3 -o sv.tar.bz2 "$BASE/asr-models/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17.tar.bz2"
  mkdir -p sense-voice && tar -xjf sv.tar.bz2 -C sense-voice --strip-components=1
  rm sv.tar.bz2
fi

if [ ! -d kokoro ]; then
  echo "fetching Kokoro ..."
  curl -fL --retry 3 -o kokoro.tar.bz2 "$BASE/tts-models/kokoro-en-v0_19.tar.bz2"
  mkdir -p kokoro && tar -xjf kokoro.tar.bz2 -C kokoro --strip-components=1
  rm kokoro.tar.bz2
fi

echo "models ready in $DIR"
