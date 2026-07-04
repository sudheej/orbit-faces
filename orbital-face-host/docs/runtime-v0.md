# Orbital Runtime v0

Orbital Runtime v0 is the minimal local companion backend for the face host. It
owns companion state, accepts terminal text, calls a model provider, and sends
visual state events over Orbital Local Bridge v0.

It intentionally does not provide voice, screen capture, tools, memory, MCP,
marketplace support, cloud services, or agent orchestration.

## Run

Terminal 1:

```sh
cargo run --bin orbital-runtime
```

Terminal 2:

```sh
cargo run -- --face examples/basic_orb --bridge ws://127.0.0.1:7373
```

Type a message in the runtime terminal. The runtime drives the face through
`listening`, `thinking`, `speaking`, and back to `idle`.

The runtime remains open if the face host disconnects. A restarted face host
can connect to the same server and sends a new `face.ready` event.

## Commands

- `/status`: print current runtime state, provider, connected face, last input,
  last response, conversation size, model error, and recent bridge events.
- `/model`: check provider reachability and selected-model availability.
- `/clear`: clear the in-memory conversation history.
- `/context`: list explicitly attached context and watch status.
- `/clear-context`: clear context and disable watch mode.
- `/clipboard`: attach one text clipboard snapshot.
- `/active-window`: attach Windows foreground title/process metadata.
- `/watch` and `/unwatch`: enable or disable prompt-time window metadata
  refresh.
- `/attach-text <text>` and `/attach-file <path>`: attach manual context.
- `/selection`: attach the current Windows text selection.
- `/ask <question>`: explicit form of a normal context-aware prompt.
- `/ask-selection <question>`: capture a selection and ask immediately.
- `/ask-selection-once <question>`: use the selection for one request only.
- `/listen [seconds]`: explicitly record and transcribe one bounded utterance.
- `/auto-listen [on|off]`: keep taking conversational turns, ending each one
  after a pause and resuming after the response.
- `/face <name-or-directory>`: hot-reload a face pack in the connected host.
- `/remember`, `/memories`, `/memory-search`, `/forget-memory`: explicitly
  manage local long-term memories.
- `/memory-status`, `/memory-clear-session`, `/memory-off`, `/memory-on`:
  inspect and control local persistence.
- `/tools`, `/tool-info`, `/tool`, `/tool-history`, `/tool-clear-session`:
  inspect, execute, and audit local tools.
- `/transcribe-file <path>`: transcribe a WAV without asking the model.
- `/say <text>`: invoke the selected TTS provider.
- `/speech-status`: print speech provider configuration and recent results.
- `/demo`: run `idle -> listening -> thinking -> speaking -> happy -> idle`.
- `/ping`: send a JSON protocol ping and log the matching pong.
- `/quit`: stop the runtime.

Any non-command line is treated as a user prompt.

## Model providers

Runtime v0 includes `MockModelProvider`, selected by default:

```sh
cargo run --bin orbital-runtime -- --model mock
```

The provider is deterministic, local, and requires no network service.

The optional Ollama provider uses `qwen2.5:1.5b` by default:

```sh
ollama pull qwen2.5:1.5b
cargo run --bin orbital-runtime -- --model ollama
```

See [model-provider-v0.md](model-provider-v0.md) for provider configuration,
streaming captions, history, diagnostics, and limitations.

See [context-v0.md](context-v0.md) for explicit clipboard, attachment, and
Windows active-window context behavior.

See [quick-capture-v0.md](quick-capture-v0.md) for Windows selection capture,
clipboard preservation, and optional global hotkeys.

See [speech-io-v0.md](speech-io-v0.md) for explicit microphone capture,
whisper.cpp, Piper, Windows SAPI, and privacy limits.

## Interaction behavior

- `face.ready` stores the face name/version and sends `idle` with `Ready`.
- Click and drag events are logged.
- Double-click and `toggle_listening` actions toggle the visual listening
  state. Voice input is not implemented.
- Runtime ping messages are answered by the face host with pong.
- Unknown messages produce a warning and do not terminate the runtime.

## Current limitations

- One face host connection is handled at a time.
- The bridge binds only to `127.0.0.1:7373`.
- Mock speaking is simulated; Ollama captions use streamed response chunks.
- The runtime uses blocking worker threads and synchronous model generation.
- There is no message persistence or delivery guarantee while disconnected.
