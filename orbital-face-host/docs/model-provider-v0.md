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

Ollama model thinking is disabled by default to keep voice turns responsive,
including for reasoning-capable models such as Gemma 4. Enable it explicitly
for tasks where additional latency is acceptable:

```sh
cargo run --bin orbital-runtime -- \
  --model ollama \
  --ollama-model gemma4:e2b \
  --enable-model-thinking
```

The provider uses Ollama's local `POST /api/chat` endpoint and streams its
newline-delimited JSON responses. `GET /api/tags` supplies the `/model` health
and installed-model check.

## System prompt

The default prompt defines a neutral, ongoing companion rather than a fixed
desktop-assistant personality:

> You are an ongoing personal companion. No fixed personality, species, or
> role is imposed; develop a consistent, natural relationship with the user
> from the configured identity and your shared history.

Override it for one runtime session:

```sh
cargo run --bin orbital-runtime -- \
  --model ollama \
  --system-prompt "You are Pip, a curious fox-like companion who is playful but calm."
```

Learned continuity is supplied separately from personality. The model is told
to use it as ordinary shared history and to say "you told me before" rather
than exposing database records, tool calls, or internal memory labels.

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
- Model-suggested local tools are optional, use Ollama's separate tool-call
  response channel, are audited, and are limited to one call. Future high-risk
  tools are confirmation-gated. There is no autonomous tool chain, MCP, or
  remote model service integration.
