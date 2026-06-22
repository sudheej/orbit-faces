#!/usr/bin/env bash

set -u

cd -- "$(dirname -- "${BASH_SOURCE[0]}")"

face_dir=${1:-examples/basic_orb}
face_dirs=()
for manifest in examples/*/manifest.json; do
  face_dirs+=("${manifest%/manifest.json}")
done

if [[ ! -f "$face_dir/manifest.json" ]]; then
  echo "Face pack not found: $face_dir" >&2
  exit 1
fi

if [[ -n "${WAYLAND_DISPLAY:-}" ]]; then
  export SDL_VIDEODRIVER="${SDL_VIDEODRIVER:-wayland}"
fi

echo "Building orbital-face-host..."
cargo build --quiet

orb_pid=
orb_input=

stop_face() {
  if [[ -n "$orb_input" ]]; then
    exec {orb_input}>&- 2>/dev/null || true
    orb_input=
  fi

  if [[ -n "$orb_pid" ]] && kill -0 "$orb_pid" 2>/dev/null; then
    kill "$orb_pid" 2>/dev/null || true
    wait "$orb_pid" 2>/dev/null || true
  fi
  orb_pid=
}

start_face() {
  coproc ORB { exec ./target/debug/orbital-face-host --face "$face_dir"; }
  orb_pid=$ORB_PID
  orb_input=${ORB[1]}
}

switch_face() {
  stop_face
  face_dir=$1
  echo "Opening face: $face_dir"
  start_face
}

cycle_face() {
  local index

  for index in "${!face_dirs[@]}"; do
    if [[ "${face_dirs[$index]}" == "$face_dir" ]]; then
      switch_face "${face_dirs[$(((index + 1) % ${#face_dirs[@]}))]}"
      return
    fi
  done

  switch_face "${face_dirs[0]}"
}

choose_face() {
  local index selection

  printf '\nAvailable faces:\n'
  for index in "${!face_dirs[@]}"; do
    printf '  %d) %s\n' "$((index + 1))" "${face_dirs[$index]}"
  done

  read -r -p "Select face: " selection
  if [[ "$selection" =~ ^[0-9]+$ ]] &&
    ((selection >= 1 && selection <= ${#face_dirs[@]})); then
    switch_face "${face_dirs[$((selection - 1))]}"
  else
    echo "Unknown face selection: $selection"
  fi
}

cleanup() {
  stop_face
}
trap cleanup EXIT INT TERM

start_face

send_event() {
  if [[ -z "$orb_pid" ]] || ! kill -0 "$orb_pid" 2>/dev/null; then
    echo "The orb process is no longer running." >&2
    exit 1
  fi

  printf '%s\n' "$1" >&"$orb_input"
}

while true; do
  printf '\nOrb test menu: %s\n' "$face_dir"
  cat <<'MENU'
  1) Idle
  2) Listening
  3) Thinking
  4) Speaking
  5) Always on top: on
  6) Always on top: off
  7) Close face
  8) Debug overlay: on
  9) Debug overlay: off
  10) Happy
  11) Error
  12) Sleeping
  n) Next face
  f) Choose face
  q) Quit
MENU

  read -r -p "Select: " choice
  case "$choice" in
    1) send_event '{"type":"state","state":"idle"}' ;;
    2) send_event '{"type":"state","state":"listening","caption":"Listening..."}' ;;
    3) send_event '{"type":"state","state":"thinking","caption":"Thinking..."}' ;;
    4) send_event '{"type":"state","state":"speaking","audio_level":0.8}' ;;
    5) send_event '{"always_on_top":true}' ;;
    6) send_event '{"always_on_top":false}' ;;
    7)
      echo "Closing face..."
      exit 0
      ;;
    8) send_event '{"debug":true}' ;;
    9) send_event '{"debug":false}' ;;
    10) send_event '{"type":"state","state":"happy"}' ;;
    11) send_event '{"type":"state","state":"error"}' ;;
    12) send_event '{"type":"state","state":"sleeping"}' ;;
    n | N) cycle_face ;;
    f | F) choose_face ;;
    q | Q) exit 0 ;;
    *) echo "Unknown selection: $choice" ;;
  esac
done
