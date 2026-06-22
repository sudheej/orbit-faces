# Face Pack v0

Face Pack v0 is a directory containing a manifest, a Lua entry script, and
optional assets:

```text
face-name/
├── manifest.json
├── main.lua
└── assets/
```

## Manifest

```json
{
  "kind": "orbital.face",
  "version": "0.1",
  "name": "Basic Orb",
  "entry": "main.lua",
  "window": {
    "width": 260,
    "height": 260,
    "transparent": true,
    "borderless": true,
    "always_on_top": false
  },
  "states": [
    "idle",
    "listening",
    "thinking",
    "speaking",
    "happy",
    "error",
    "sleeping"
  ]
}
```

Fields:

- `kind`: must be `orbital.face`.
- `version`: must be `0.1`.
- `name`: non-empty display name.
- `entry`: relative path to an existing Lua script.
- `window.width`, `window.height`: positive logical dimensions.
- `window.transparent`: request transparent window content.
- `window.borderless`: request no native frame.
- `window.always_on_top`: initial stacking preference. The Hyprland backend
  starts on Overlay so the companion is visible; it can be moved to Top at
  runtime.
- `states`: unique supported state names and must include `idle`.

Invalid JSON, unsupported kind/version, invalid dimensions, missing `idle`, or
a missing entry script produces a clear startup error.

## Events

Preferred event:

```json
{
  "type": "state",
  "state": "speaking",
  "emotion": "focused",
  "caption": "Looking at your terminal...",
  "audio_level": 0.72
}
```

`emotion`, `caption`, and `audio_level` are optional. Unknown fields are
ignored.

Backward-compatible input is accepted:

```json
{"state":"thinking"}
```

Unknown states log a warning and are delivered to Lua as `idle`.

## Lua lifecycle

All functions are optional:

```lua
function companion.load(ctx)
end

function companion.state_changed(event)
end

function companion.update(dt)
end

function companion.draw(ctx)
end

function companion.hit_test(x, y)
  return true
end
```

`load(ctx)` receives `name`, `width`, and `height`.

`state_changed(event)` receives `type`, `state`, `emotion`, `caption`, and
`audio_level`.

Missing callbacks are ignored. Callback failures are logged by Rust; update
continues where practical, and the most recent successful draw command list is
kept after a draw error. A syntax/load error prevents startup because no valid
face script exists.

## Drawing context

Stateful helpers:

```lua
ctx.set_color(255, 255, 255, 255)
ctx.set_alpha(0.8)
ctx.draw_circle(130, 130, 80)
ctx.draw_text("hello", 10, 10)
local time = ctx.get_time()
local state = ctx.get_state()
```

Compatibility helpers remain available:

```lua
ctx.clear(r, g, b, a)
ctx.circle(x, y, radius, r, g, b, a)
ctx.rect(x, y, width, height, r, g, b, a)
```

Text uses a deliberately small built-in bitmap font. This is not a general UI
or game-engine API.

## Limitations

- Asset loading is reserved for a later Face Pack revision.
- The current hitmask is circular unless Lua supplies `hit_test`.
- Lua is trusted local code; there is no sandbox or permission model.
- There is no hot reload or package marketplace.
