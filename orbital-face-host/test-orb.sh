#!/usr/bin/env bash

set -u

cd -- "$(dirname -- "${BASH_SOURCE[0]}")"

if [[ -n "${WAYLAND_DISPLAY:-}" ]]; then
  export SDL_VIDEODRIVER="${SDL_VIDEODRIVER:-wayland}"
fi

echo "Building orbital-face-host..."
cargo build --quiet

coproc ORB { exec ./target/debug/orbital-face-host; }
orb_pid=$ORB_PID
orb_input=${ORB[1]}

cleanup() {
  exec {orb_input}>&- 2>/dev/null || true
  if kill -0 "$orb_pid" 2>/dev/null; then
    kill "$orb_pid" 2>/dev/null || true
    wait "$orb_pid" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

send_event() {
  if ! kill -0 "$orb_pid" 2>/dev/null; then
    echo "The orb process is no longer running." >&2
    exit 1
  fi

  printf '%s\n' "$1" >&"$orb_input"
}

while true; do
  cat <<'MENU'

Orb test menu
  1) Idle
  2) Listening
  3) Thinking
  4) Speaking
  5) Always on top: on
  6) Always on top: off
  7) Close face
  q) Quit
MENU

  read -r -p "Select: " choice
  case "$choice" in
    1) send_event '{"state":"idle"}' ;;
    2) send_event '{"state":"listening"}' ;;
    3) send_event '{"state":"thinking"}' ;;
    4) send_event '{"state":"speaking"}' ;;
    5) send_event '{"always_on_top":true}' ;;
    6) send_event '{"always_on_top":false}' ;;
    7)
      echo "Closing face..."
      exit 0
      ;;
    q | Q) exit 0 ;;
    *) echo "Unknown selection: $choice" ;;
  esac
done
