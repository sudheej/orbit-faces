# Architecture

`orbital-face-host` is intentionally small. It is a face/window runtime, not the future companion backend.

## Runtime Flow

1. `main.rs` chooses a face package directory.
2. `lua_host.rs` reads `manifest.json` and loads `main.lua`.
3. `window.rs` creates a small SDL3 borderless window.
4. `events.rs` reads JSON lines from stdin on a background thread.
5. `app.rs` runs the SDL event loop, forwards state changes to Lua, and renders Lua draw commands.
6. `renderer.rs` draws a tiny immediate-mode command list to SDL.

## Lua Boundary

Lua scripts own visual behavior only. They receive state, time, and a constrained drawing context. They do not own the app backend, IO, network, tools, voice, or agents.

Expected callbacks:

```lua
function companion.load()
end

function companion.state_changed(state)
end

function companion.update(dt)
end

function companion.draw(ctx)
end
```

## Platform Boundary

`src/platform` is reserved for platform-specific experiments. Unsafe code and native APIs should stay there instead of leaking into the app loop or renderer.
