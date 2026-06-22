# Architecture

`orbital-face-host` is a face runtime, not a companion backend.

## Runtime flow

1. `main.rs` parses `--face` and the optional local `--bridge` URL.
2. `face_pack.rs` loads and validates `manifest.json` and the entry script.
3. `runtime.rs` owns the current state, Lua host, debug overlay, FPS, and safe
   callback handling.
4. `events.rs` parses shared bridge messages and backward-compatible stdin
   JSON lines.
5. `lua_host.rs` exposes lifecycle callbacks and the constrained drawing API.
6. `renderer.rs` executes the shared draw-command list.
7. `wayland_app.rs` hosts the Linux layer-shell surface; `app.rs` and
   `window.rs` contain the SDL3 platform path.
8. `bridge.rs` runs the reconnecting WebSocket worker used by the face host;
   `orbital-runtime-mock` provides the local test server.
9. `runtime_v0.rs` contains the backend state machine, protocol parser, command
   parser, and model-provider boundary used by the interactive
   `orbital-runtime` binary.
10. `model_provider.rs` isolates deterministic mock generation and optional
    Ollama `/api/chat` and `/api/tags` HTTP calls.
11. `context/` owns explicit clipboard snapshots, bounded text/file
    attachments, Windows active-window metadata, watch state, and prompt
    context formatting.
12. `quick_capture.rs` isolates selected-text capture and clipboard
    preservation; `hotkeys.rs` isolates optional Windows global shortcuts.
13. `speech/` isolates bounded CPAL microphone capture, mock/whisper.cpp STT,
    and none/Piper/Windows-SAPI TTS providers.

## Boundaries

Face scripts receive state and draw. They do not own IO, networking, tools,
voice, agents, or application lifecycle. The bridge remains a host-level
transport.

Windowing is platform-specific because transparent click-through, focus,
positioning, and stacking are not portable desktop concepts. Drawing, face
packs, Lua, and events remain shared.
