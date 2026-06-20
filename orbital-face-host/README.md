# orbital-face-host

Minimal Windows-first desktop companion face host prototype using Rust, SDL3, and Lua.

This project proves the face/window layer only. It does not include LLMs, voice, marketplace, agents, memory, cloud sync, or plugin security.

## Run

Requirements:
- Rust stable
- Windows is the primary target
- SDL3 is built from source through the `sdl3` crate feature in this prototype

```sh
cargo run
```

For an interactive test menu:

```sh
./test-orb.sh
```

The default face package is `examples/basic_orb`. You can also pass a face directory:

```sh
cargo run -- examples/basic_orb
```

Send state changes by typing JSON lines into stdin:

```json
{"state":"listening"}
{"state":"thinking"}
{"state":"speaking"}
{"state":"idle"}
```

Always-on-top can be controlled without focusing the face window:

```json
{"always_on_top":true}
{"always_on_top":false}
```

Keyboard:
- `T`: toggle always-on-top if the platform delivers keys to the non-focusable window
- `Esc`: quit

Mouse:
- Drag the visible orb area to move the window.

## Face Package

The example package lives in `examples/basic_orb`:

- `manifest.json` declares the script path and window size.
- `main.lua` defines `companion.load`, `companion.state_changed`, `companion.update`, and `companion.draw`.

The drawing API is intentionally tiny:

```lua
ctx.clear(r, g, b, a)
ctx.circle(x, y, radius, r, g, b, a)
ctx.rect(x, y, width, height, r, g, b, a)
```

## Current Status

Implemented:
- Borderless SDL3 window
- Lua-driven orb animation
- `idle`, `listening`, `thinking`, and `speaking` states
- Stdin JSON state events
- Stdin always-on-top control
- Manual dragging
- SDL hit-test setup for draggable visible region
- SDL3 circular window shape/alpha mask attempt
- Windows-only always-on-top toggle attempt
- Transparent-window flag attempt

Known limitations:
- Per-pixel transparency depends on SDL3 renderer/backend and Windows compositor behavior.
- The circular shape uses `SDL_SetWindowShape`; Windows behavior must be verified on a real desktop.
- SDL documents fully transparent shaped pixels as click-through, but this still requires Windows verification with the selected renderer/backend.
- Avoiding keyboard focus is attempted with SDL's `NOT_FOCUSABLE` flag, but behavior must be verified on real Windows builds.

See `docs/windowing-notes.md` for platform details.
