#!/usr/bin/env bash

set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")"

WHISPER_REPO="https://github.com/ggml-org/whisper.cpp.git"
WHISPER_MODEL_URL="https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin"
PIPER_URL="https://github.com/rhasspy/piper/releases/download/2023.11.14-2/piper_linux_x86_64.tar.gz"
PIPER_VOICE_URL="https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/lessac/medium/en_US-lessac-medium.onnx"
PIPER_CONFIG_URL="https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/lessac/medium/en_US-lessac-medium.onnx.json"

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Required command not found: $1" >&2
    exit 1
  fi
}

download_if_missing() {
  local url=$1
  local destination=$2

  if [[ -f "$destination" ]]; then
    echo "Already installed: $destination"
    return
  fi

  echo "Downloading: $destination"
  curl --fail --location --progress-bar \
    --output "${destination}.part" \
    "$url"
  mv -- "${destination}.part" "$destination"
}

require_command git
require_command cmake
require_command curl
require_command tar

mkdir -p models voices

download_if_missing "$WHISPER_MODEL_URL" "models/ggml-tiny.en.bin"

if [[ ! -d whisper.cpp/.git ]]; then
  echo "Cloning whisper.cpp..."
  git clone --depth 1 "$WHISPER_REPO" whisper.cpp
else
  echo "Already cloned: whisper.cpp"
fi

echo "Building whisper-cli..."
cmake -S whisper.cpp -B whisper.cpp/build -DCMAKE_BUILD_TYPE=Release
cmake --build whisper.cpp/build --parallel "$(nproc)" --config Release

if [[ ! -x piper/piper ]]; then
  echo "Downloading Piper..."
  piper_archive=$(mktemp --suffix=.tar.gz)
  trap 'rm -f -- "${piper_archive:-}"' EXIT
  curl --fail --location --progress-bar \
    --output "$piper_archive" \
    "$PIPER_URL"
  tar -xzf "$piper_archive"
  rm -f -- "$piper_archive"
  trap - EXIT
else
  echo "Already installed: piper/piper"
fi

download_if_missing "$PIPER_VOICE_URL" "voices/en_US-lessac-medium.onnx"
download_if_missing "$PIPER_CONFIG_URL" "voices/en_US-lessac-medium.onnx.json"

echo
echo "Verifying speech dependencies..."

required_files=(
  "models/ggml-tiny.en.bin"
  "whisper.cpp/build/bin/whisper-cli"
  "piper/piper"
  "voices/en_US-lessac-medium.onnx"
  "voices/en_US-lessac-medium.onnx.json"
)

failed=0
for path in "${required_files[@]}"; do
  if [[ -e "$path" ]]; then
    printf 'OK  %s\n' "$path"
  else
    printf 'MISSING  %s\n' "$path" >&2
    failed=1
  fi
done

if ((failed != 0)); then
  echo "Speech dependency verification failed." >&2
  exit 1
fi

echo
echo "Speech dependencies installed and verified."
