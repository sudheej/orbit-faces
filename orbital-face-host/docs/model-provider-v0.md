# Local Model Provider v0

Orbital Runtime supports a deterministic mock provider and an optional local
Ollama provider. Mock remains the default and requires no network service.

## Mock provider

```sh
cargo run --bin orbital-runtime -- --model mock
```

The mock provider returns short deterministic responses and emits simulated
text chunks through the same runtime speaking path used by Ollama. It is used
by automated tests.

## Ollama provider

The recommended lightweight model is Qwen 2.5 1.5B:

```sh
ollama pull qwen2.5:1.5b
cargo run --bin orbital-runtime -- --model ollama
```

The default Ollama model is `qwen2.5:1.5b`. An optional coding-focused model is:

```sh
ollama pull qwen2.5-coder:1.5b
cargo run --bin orbital-runtime -- \
  --model ollama \
  --ollama-model qwen2.5-coder:1.5b
```

Full configuration:

```sh
cargo run --bin orbital-runtime -- \
  --model ollama \
  --ollama-base-url http://localhost:11434 \
  --ollama-model qwen2.5:1.5b
```

The provider uses Ollama's local `POST /api/chat` endpoint and streams its
newline-delimited JSON responses. `GET /api/tags` supplies the `/model` health
and installed-model check.

## System prompt

The default prompt asks Orbital for concise answers suitable for a small face:

> You are Orbital, a concise local desktop companion. Reply in short, useful
> answers. Prefer 1-3 sentences unless the user asks for detail. You are
> running through a small desktop face, so keep captions compact.

Override it for one runtime session:

```sh
cargo run --bin orbital-runtime -- \
  --model ollama \
  --system-prompt "Reply in one concise sentence."
```

## Streaming captions

As model chunks arrive, the runtime accumulates them and sends compact
`speaking` captions to the face. Captions retain the most recent 72 characters,
and the runtime cycles a synthetic audio level between `0.2` and `0.9`.

The runtime stores the last six user/assistant exchanges in memory and includes
them in later `/api/chat` requests. `/clear` removes this history. Nothing is
persisted to disk.

## Diagnostics and fallback

Use `/model` to print:

- provider and selected model;
- Ollama base URL;
- server reachability;
- whether the selected model appears in `/api/tags`;
- the last model error;
- the appropriate `ollama pull` command.

If Ollama is unavailable, the runtime remains open, sends a brief error state
to the face, returns it to idle, and prints recovery guidance. Fall back with:

```sh
cargo run --bin orbital-runtime -- --model mock
```

## Limitations

- Ollama is optional and never contacted in mock mode or tests.
- Generation is synchronous in the terminal runtime.
- Only text chat is supported.
- History is session-local and capped at six exchanges.
- There is no tool calling, persistent memory, voice, screen context, MCP, or
  remote model service integration.
