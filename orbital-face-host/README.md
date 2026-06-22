# orbital-face-host

Minimal Face Pack Runtime v0 for a transparent desktop companion face.

The shared runtime uses Rust, Lua through `mlua`, JSON stdin events, and a tiny
immediate-mode drawing API. Hyprland uses a native `wlr-layer-shell` surface;
Windows and macOS currently use the SDL3 path and still require platform
validation.

The runtime includes a deterministic mock provider and optional local Ollama
text generation. This repository still excludes voice, tools, agents,
persistent memory, marketplace, cloud services, and plugin permissions.

## Run

Requirements:

- Rust stable
- Hyprland or another `wlr-layer-shell` compositor on Linux
- CMake and native build tools for SDL3 builds

Run the default face:

```sh
cargo run
```

Select a face pack:

```sh
cargo run -- --face examples/basic_orb
```

Interactive test menu:

```sh
./test-orb.sh
./test-orb.sh examples/pixel_pet
```

Without `--bridge`, stdin remains available as the legacy/simple test mode.

## Running with Orbital Runtime

Terminal 1:

```sh
cargo run --bin orbital-runtime
```

Terminal 2:

```sh
cargo run -- --face examples/basic_orb --bridge ws://127.0.0.1:7373
```

Type messages into the runtime terminal. The runtime uses its local mock model
provider and drives the face through thinking, speaking, and idle. Runtime
commands include `/status`, `/model`, `/context`, `/clipboard`,
`/attach-text`, `/attach-file`, `/watch`, `/demo`, `/ping`, and `/quit`.

See [docs/runtime-v0.md](docs/runtime-v0.md) for behavior, model-provider
details, and limitations.

### Running with Ollama

Pull the recommended lightweight model:

```sh
ollama pull qwen2.5:1.5b
```

Terminal 1:

```sh
cargo run --bin orbital-runtime -- --model ollama --ollama-model qwen2.5:1.5b
```

Terminal 2:

```sh
cargo run -- --face examples/basic_orb --bridge ws://127.0.0.1:7373
```

Then type a prompt into the runtime terminal. Ollama response chunks drive
compact speaking captions on the face. Mock remains the default and fallback:

```sh
cargo run --bin orbital-runtime -- --model mock
```

See [docs/model-provider-v0.md](docs/model-provider-v0.md) for the optional
coding model, base URL and system-prompt options, `/model`, `/clear`, and
failure behavior.

### Adding explicit context

Clipboard example:

```text
/clipboard
summarize this
```

File example:

```text
/attach-file ./README.md
what does this project do?
```

Windows active-window metadata example:

```text
/watch
what window am I working in?
/unwatch
```

Use `/context` to inspect attached items and `/clear-context` to remove them.
Clipboard is read only on `/clipboard`; watch mode tracks title/process
metadata only and captures no screenshots. See
[docs/context-v0.md](docs/context-v0.md) for privacy behavior and limits.

### Windows quick capture

Start the runtime with optional global hotkeys:

```sh
cargo run --bin orbital-runtime -- \
  --model ollama \
  --ollama-model qwen2.5:1.5b \
  --enable-hotkeys
```

Then select an error in VS Code, a terminal, or another Windows application and
press `Ctrl+Alt+A`. The equivalent runtime command is:

```text
/ask-selection explain this error
```

Orbital captures the selection explicitly, preserves existing text clipboard
content when possible, adds foreground title/process/PID metadata, and sends
the question to the local model. Mock mode supports the same flow:

```sh
cargo run --bin orbital-runtime -- --model mock --enable-hotkeys
```

Use `/ask-selection-once` for one-request-only selection context. See
[docs/quick-capture-v0.md](docs/quick-capture-v0.md) for hotkeys, clipboard
preservation, privacy behavior, and Windows limitations.

### Explicit speech I/O

Mock speech flow, with no microphone or speakers:

```sh
cargo run --bin orbital-runtime -- --model mock --stt mock --tts none
```

```text
/listen 5
```

Whisper.cpp flow:

```sh
cargo run --bin orbital-runtime -- \
  --model ollama \
  --ollama-model qwen2.5:1.5b \
  --stt whisper \
  --whisper-bin ./whisper.cpp/build/bin/whisper-cli \
  --whisper-model-path ./models/ggml-tiny.en.bin \
  --tts none
```

TTS examples:

```sh
cargo run --bin orbital-runtime -- --tts windows-sapi
```

```text
/say Hello, I am Orbital.
```

Or:

```sh
cargo run --bin orbital-runtime -- \
  --tts piper \
  --piper-bin ./piper \
  --piper-model ./voices/en_US-lessac-medium.onnx
```

Speech is push-to-talk only. There is no wake word or background microphone
monitoring. See [docs/speech-io-v0.md](docs/speech-io-v0.md).

## Running with mock runtime

Terminal 1:

```sh
cargo run --bin orbital-runtime-mock
```

Terminal 2:

```sh
cargo run -- --face examples/basic_orb --bridge ws://127.0.0.1:7373
```

The mock runtime listens on localhost, sends the sequence `idle -> listening ->
thinking -> speaking -> happy -> idle`, and logs events received from the face
host. Clicking, double-clicking, or completing a drag sends an event back to
the mock runtime.

The face host remains open if the runtime is unavailable and retries the
connection every two seconds. Restarting the mock runtime causes the face host
to reconnect and send `face.ready` again.

See [docs/bridge-protocol-v0.md](docs/bridge-protocol-v0.md) for the JSON
protocol and current limitations.

## Events

Typed state event:

```json
{"type":"state","state":"listening","caption":"Listening..."}
```

Optional state metadata:

```json
{"type":"state","state":"speaking","emotion":"focused","caption":"Working...","audio_level":0.8}
```

The original short form remains supported:

```json
{"state":"thinking"}
```

Unknown JSON fields are ignored. A state not listed in the face manifest logs a
warning and falls back to `idle`.

Runtime test controls are also accepted:

```json
{"always_on_top":true}
{"debug":true}
```

## Local controls

- `A`: toggle always-on-top
- `D`: toggle debug overlay
- `Esc`: quit

On Hyprland, click the orb first to grant on-demand keyboard focus. The stdin
and interactive-script controls do not require focus.

Mouse dragging starts only inside the face hit region.

## Face packs

The default pack is under `examples/basic_orb/`:

```text
examples/basic_orb/
├── manifest.json
├── main.lua
└── assets/
```

See [docs/face-pack-v0.md](docs/face-pack-v0.md) for the manifest, event
contract, Lua lifecycle, and drawing API.

See [docs/windowing-notes.md](docs/windowing-notes.md) for platform behavior and
limitations.

## Example Face Packs

Each pack uses the same runtime, manifest contract, events, and Lua API:

```sh
cargo run -- --face examples/basic_orb
cargo run -- --face examples/pixel_pet
cargo run -- --face examples/terminal_cube
cargo run -- --face examples/minimal_dot
```

- `basic_orb`: smooth glowing reference orb.
- `pixel_pet`: blocky retro desktop mascot with expressions and sleep `Z`s.
- `terminal_cube`: developer-oriented terminal monitor with scanlines, loading
  indicators, waveform output, and error diagnostics.
- `minimal_dot`: low-distraction dot, rings, orbiting indicators, and
  audio-reactive pulse.

All four packs declare `idle`, `listening`, `thinking`, `speaking`, `happy`,
`error`, and `sleeping`.

See [docs/example-face-packs.md](docs/example-face-packs.md) for their visual
intent and testing guidance. A future irregular
[Winamp-style panel](docs/winamp-panel-future.md) is documented but deliberately
not implemented yet.
